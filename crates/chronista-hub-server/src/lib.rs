//! Chronista Hub server — Node Tree meta-registry (axum + embedded SurrealDB)。
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
pub mod unison_server;

pub const SERVICE_NAME: &str = "chronista-hub";
/// 版数は Cargo.toml (workspace.package.version) を単一の真実源とする。
/// ハードコードすると /health と federation identity が drift するため env! で追従。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
