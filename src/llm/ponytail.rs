//! Ponytail coding ruleset for AI Markets workers, fetched from the shared
//! Ponytail service (`PONYTAIL_URL`, see ponytail/deploy/DOKPLOY.md) and
//! prepended to worker system prompts by `SpacebotModel`.
//!
//! Never fails a request: an unreachable service serves the last good copy,
//! or nothing.

use std::sync::{LazyLock, Mutex, MutexGuard};
use std::time::{Duration, Instant};

const REFRESH: Duration = Duration::from_secs(10 * 60);
const RETRY: Duration = Duration::from_secs(60);
const TIMEOUT: Duration = Duration::from_secs(3);
const MODES: [&str; 5] = ["compact", "lite", "full", "ultra", "off"];

static RULES: LazyLock<PonytailRules> = LazyLock::new(|| {
    PonytailRules::new(
        &std::env::var("PONYTAIL_URL").unwrap_or_default(),
        &std::env::var("PONYTAIL_MODE").unwrap_or_default(),
    )
});

/// Current rules text; empty when Ponytail is disabled or has never been reached.
pub async fn rules() -> String {
    RULES.get().await
}

#[derive(Default)]
struct State {
    text: String,
    etag: String,
    next_refresh: Option<Instant>,
    refreshing: bool,
}

struct PonytailRules {
    url: Option<String>,
    client: reqwest::Client,
    state: Mutex<State>,
}

impl PonytailRules {
    fn new(url: &str, mode: &str) -> Self {
        let mode = mode.trim().to_lowercase();
        let mode = if MODES.contains(&mode.as_str()) {
            mode
        } else {
            "compact".to_string()
        };
        let base = url.trim().trim_end_matches('/');
        let url = (!base.is_empty() && mode != "off").then(|| format!("{base}/v1/rules?mode={mode}"));
        Self {
            url,
            client: reqwest::Client::builder()
                .timeout(TIMEOUT)
                .build()
                .unwrap_or_default(),
            state: Mutex::new(State::default()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    async fn get(&'static self) -> String {
        let Some(url) = self.url.as_deref() else {
            return String::new();
        };
        let (text, due) = {
            let mut state = self.lock();
            let due = !state.refreshing
                && state.next_refresh.is_none_or(|at| Instant::now() >= at);
            if due {
                state.refreshing = true;
            }
            (state.text.clone(), due)
        };
        if !due {
            return text;
        }
        // Serve the last good copy while revalidating; only an empty cache waits.
        if text.is_empty() {
            self.refresh(url).await;
            return self.lock().text.clone();
        }
        tokio::spawn(self.refresh(url));
        text
    }

    async fn refresh(&self, url: &str) {
        let etag = self.lock().etag.clone();
        let mut request = self.client.get(url);
        if !etag.is_empty() {
            request = request.header(reqwest::header::IF_NONE_MATCH, etag);
        }
        let outcome = async {
            let response = request.send().await?;
            if response.status() == reqwest::StatusCode::NOT_MODIFIED {
                return Ok(None);
            }
            let response = response.error_for_status()?;
            let etag = response
                .headers()
                .get(reqwest::header::ETAG)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string();
            let text = response.text().await?.trim().to_string();
            Ok::<_, reqwest::Error>(Some((text, etag)))
        }
        .await;

        let mut state = self.lock();
        state.refreshing = false;
        match outcome {
            Ok(fresh) => {
                if let Some((text, etag)) = fresh.filter(|(text, _)| !text.is_empty()) {
                    state.text = text;
                    state.etag = etag;
                }
                state.next_refresh = Some(Instant::now() + REFRESH);
            }
            Err(error) => {
                tracing::warn!(%error, url, "ponytail rules unavailable");
                state.next_refresh = Some(Instant::now() + RETRY);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    fn leak(rules: PonytailRules) -> &'static PonytailRules {
        Box::leak(Box::new(rules))
    }

    #[test]
    fn disabled_without_a_url_or_when_off() {
        assert!(PonytailRules::new("", "compact").url.is_none());
        assert!(PonytailRules::new("http://ponytail:8787", "off").url.is_none());
        assert_eq!(
            PonytailRules::new("http://ponytail:8787/", "FULL").url.as_deref(),
            Some("http://ponytail:8787/v1/rules?mode=full")
        );
        assert_eq!(
            PonytailRules::new("http://ponytail:8787", "bogus").url.as_deref(),
            Some("http://ponytail:8787/v1/rules?mode=compact")
        );
    }

    #[tokio::test]
    async fn fetches_once_then_serves_the_cache() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let hits = Arc::new(AtomicUsize::new(0));
        let served = hits.clone();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                served.fetch_add(1, Ordering::SeqCst);
                let mut buf = [0u8; 2048];
                let _ = socket.read(&mut buf).await;
                let body = "PONYTAIL MODE ACTIVE — level: compact\nrules";
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nETag: \"v1\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            }
        });

        let rules = leak(PonytailRules::new(&format!("http://127.0.0.1:{port}"), ""));
        assert!(rules.get().await.starts_with("PONYTAIL MODE ACTIVE"));
        assert!(rules.get().await.starts_with("PONYTAIL MODE ACTIVE"));
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        assert_eq!(rules.lock().etag, "\"v1\"");
    }

    #[tokio::test]
    async fn an_unreachable_service_yields_nothing_and_backs_off() {
        let port = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
            listener.local_addr().expect("addr").port()
        };
        let rules = leak(PonytailRules::new(&format!("http://127.0.0.1:{port}"), "lite"));
        assert_eq!(rules.get().await, "");
        let state = rules.lock();
        assert!(!state.refreshing);
        assert!(state.next_refresh.is_some_and(|at| at > Instant::now() + RETRY / 2));
    }
}
