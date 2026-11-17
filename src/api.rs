//! HTTP API server for the Spacebot control interface.
//!
//! Serves the embedded frontend assets and provides a JSON API for
//! managing agents, viewing status, and interacting with the system.
//! Includes an SSE endpoint for realtime event streaming.

pub mod activity;
pub mod agents;
mod attachments;
pub mod autonomy;
pub mod bindings;
pub mod channels;
pub mod chronicle;
pub mod config;
mod cortex;
pub mod cron;
mod factory;
pub mod goals;
pub mod ingest;
mod links;
pub mod mcp;
pub mod memories;
pub mod messaging;
pub mod models;
pub mod notifications;
mod opencode_proxy;
pub mod portal;
pub mod projects;
pub mod prompts;
pub mod providers;
pub mod secrets;
mod server;
pub mod settings;
mod skills;
pub(crate) mod ssh;
mod state;
mod system;
pub mod tasks;
mod tenant_auth;
mod tools;
pub mod usage;
pub mod wakes;
pub mod wiki;
pub mod workers;

pub use server::{api_router, start_http_server};
pub use state::{AgentInfo, ApiEvent, ApiState, ChannelToolCallEntry};
