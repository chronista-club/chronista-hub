//! mem_1CgvWFshJUiNpcgfBmNgMT — 同じ振る舞いを embedded / remote で検証する。
//! CHRONISTA_HUB_SURREAL_TEST_URL は使い捨ての loopback server 専用。
//! remote 実行は --test-threads=1（bootstrap の共有 DDL を直列化）。

use std::sync::atomic::{AtomicU64, Ordering};

use chronista_hub_server::config::{DbAuth, DbAuthLevel};
use chronista_hub_server::db::{Db, connect, connect_mem};

static NEXT_DB: AtomicU64 = AtomicU64::new(0);

pub async fn connect_test(ns: &str, dbname: &str) -> anyhow::Result<Db> {
    let Ok(url) = std::env::var("CHRONISTA_HUB_SURREAL_TEST_URL") else {
        return connect_mem(ns, dbname).await;
    };
    let parsed = reqwest::Url::parse(&url)?;
    anyhow::ensure!(
        parsed.scheme() == "ws" && parsed.host_str() == Some("127.0.0.1"),
        "remote tests require a disposable ws://127.0.0.1 server"
    );
    let auth = DbAuth {
        level: DbAuthLevel::Root,
        username: std::env::var("CHRONISTA_HUB_SURREAL_TEST_USERNAME").unwrap_or("root".into()),
        password: std::env::var("CHRONISTA_HUB_SURREAL_TEST_PASSWORD").unwrap_or("root".into()),
    };
    let unique_db = format!(
        "{dbname}_compat_{}_{}",
        std::process::id(),
        NEXT_DB.fetch_add(1, Ordering::Relaxed)
    );
    connect(&url, Some(&auth), ns, &unique_db).await
}
