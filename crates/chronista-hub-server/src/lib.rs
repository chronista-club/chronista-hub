//! Chronista Hub server — World Tree meta-registry (axum + embedded SurrealDB)。
//!
//! ADR-016: 低レイテンシのため Rust + in-process embedded SurrealDB (kv-rocksdb)。

pub mod app;
pub mod auth;
pub mod config;
pub mod consumer;
pub mod db;
pub mod event_log;
pub mod model;
pub mod product_token;
pub mod storage;

pub const SERVICE_NAME: &str = "chronista-hub";
pub const VERSION: &str = "0.0.1";
