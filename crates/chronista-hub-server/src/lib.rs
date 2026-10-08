//! Chronista Hub server — Node Tree meta-registry (axum + SurrealDB)。
//!
//! ADR-016: Rust 実装。 DB 接続先は ADR-022 で URL 化 (embedded rocksdb / remote ws)。

pub mod app;
pub mod auth;
pub mod config;
pub mod consumer;
pub mod db;
pub mod event_log;
pub mod model;
pub mod product_token;
pub mod start;
pub mod storage;
pub mod unison_server;

pub const SERVICE_NAME: &str = "chronista-hub";
/// 版数は Cargo.toml (workspace.package.version) を単一の真実源とする。
/// ハードコードすると /health と federation identity が drift するため env! で追従。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
