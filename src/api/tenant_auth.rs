//! AI Markets tenant auth for the SpaceBot HTTP API.
//!
//! One dashboard token per buyer:
//! `HMAC-SHA256(secret, "spacebot-dashboard:v1:" + userId)` as hex, first 48 chars.
//! The secret is `SPACEBOT_TENANT_SECRET` or `AIMARKETS_SERVICE_SECRET` — the same
//! seed the marketplace API uses (`SPACEBOT_DASHBOARD_TOKEN_SEED` / `AIMARKETS_SERVICE_SECRET`).
//!
//! `SPACEBOT_TENANT_ID` pins this process to one buyer (one container, one data
//! volume). Without it, the token is still checked against the request Host, so
//! buyer A's token is rejected on buyer B's hostname. The sqlite/redb data
//! directory stays process-wide: production runs one container per buyer.

use sha2::{Digest, Sha256};

const USER_ID_LEN: usize = 24;

pub fn tenant_secret() -> Option<String> {
    let raw = std::env::var("SPACEBOT_TENANT_SECRET")
        .or_else(|_| std::env::var("AIMARKETS_SERVICE_SECRET"))
        .unwrap_or_default();
    let secret = raw.trim().to_string();
    if secret.is_empty() {
        None
    } else {
        Some(secret)
    }
}

pub fn tenant_id() -> Option<String> {
    let raw = std::env::var("SPACEBOT_TENANT_ID").unwrap_or_default();
    let id = raw.trim().to_ascii_lowercase();
    if is_user_id(&id) { Some(id) } else { None }
}

pub fn is_user_id(value: &str) -> bool {
    value.len() == USER_ID_LEN && value.bytes().all(|b| b.is_ascii_hexdigit())
}

/// First DNS label when it is a buyer id (`{userId}.spacebot.aimarkets.vn`).
pub fn user_id_from_host(host: &str) -> Option<String> {
    let host = host.split(',').next()?.trim().to_ascii_lowercase();
    let host = host.split(':').next()?.trim();
    let label = host.split('.').next()?.trim();
    if is_user_id(label) {
        Some(label.to_string())
    } else {
        None
    }
}

pub fn dashboard_token(secret: &str, user_id: &str) -> String {
    let mut mac = hmac_sha256(
        secret.as_bytes(),
        format!("spacebot-dashboard:v1:{user_id}").as_bytes(),
    );
    let hex = hex::encode(mac);
    mac.fill(0);
    hex.chars().take(48).collect()
}

pub fn tokens_equal(presented: &str, expected: &str) -> bool {
    if presented.is_empty() || presented.len() != expected.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in presented.bytes().zip(expected.bytes()) {
        diff |= a ^ b;
    }
    diff == 0
}

fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut key_block = [0u8; BLOCK];
    if key.len() > BLOCK {
        let digest = Sha256::digest(key);
        key_block[..32].copy_from_slice(&digest);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0x36u8; BLOCK];
    let mut opad = [0x5cu8; BLOCK];
    for i in 0..BLOCK {
        ipad[i] ^= key_block[i];
        opad[i] ^= key_block[i];
    }
    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(message);
    let inner_hash = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner_hash);
    let out: [u8; 32] = outer.finalize().into();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_matches_the_marketplace_api_vector() {
        // node: crypto.createHmac('sha256', 'seed').update('spacebot-dashboard:v1:6a69f0cdd7faa829b87a852e').digest('hex').slice(0, 48)
        let token = dashboard_token("seed", "6a69f0cdd7faa829b87a852e");
        assert_eq!(token, "c055dc1db2aff38bf2f89f5213586697b16f1e4daed8b02f");
        assert_ne!(token, dashboard_token("seed", "6a69f0cdd7faa829b87a852f"));
        assert!(tokens_equal(&token, &token));
        assert!(!tokens_equal(&token, "0".repeat(48).as_str()));
    }

    #[test]
    fn host_label_is_the_buyer_id() {
        assert_eq!(
            user_id_from_host("6a69f0cdd7faa829b87a852e.spacebot.aimarkets.vn"),
            Some("6a69f0cdd7faa829b87a852e".into())
        );
        assert_eq!(user_id_from_host("SpaceBot.aimarkets.vn"), None);
        assert_eq!(
            user_id_from_host("6a69f0cdd7faa829b87a852e.spacebot.aimarkets.vn:443"),
            Some("6a69f0cdd7faa829b87a852e".into())
        );
    }
}
