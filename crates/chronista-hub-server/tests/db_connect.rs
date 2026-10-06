//! DB 接続の URL 切替 (ADR-022) — `rocksdb://` (embedded 永続) と `ws://` (remote)。
//!
//! remote は実 SurrealDB が要るので `CHRONISTA_HUB_SURREAL_TEST_URL` で gate する
//! (未設定なら skip)。 手元では:
//!
//! ```sh
//! surreal start --user root --pass root --bind 127.0.0.1:18000 memory &
//! CHRONISTA_HUB_SURREAL_TEST_URL=ws://127.0.0.1:18000 \
//!   cargo test -p chronista-hub-server --test db_connect
//! ```
//!
//! 資格情報は `CHRONISTA_HUB_SURREAL_TEST_USERNAME` / `_PASSWORD` (default root/root、 root level)。
//!
//! ⚠️ 使い捨てのローカル server 専用。 `001_bootstrap` が `DEFINE NAMESPACE chronista` を
//! 発行するので、 共有 instance (Haven 等) の root で流さないこと。

use std::path::{Path, PathBuf};

use chronista_hub_server::config::{DbAuth, DbAuthLevel};
use chronista_hub_server::db::{connect, run_pending_migrations};

fn migrations_dir() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../migrations"))
}

/// ws テストは同じ server に 001 の DDL (DEFINE NAMESPACE 等) を投げるので、 並列だと
/// write conflict になる。 本番は Hub 1 台が起動時に 1 回流すだけなので、 テストだけ直列化する。
static WS_SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn unique_tmp_dir(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("hub-{label}-{}-{nanos}", std::process::id()))
}

#[tokio::test]
async fn rocksdb_url_writes_to_disk() {
    // 再起動をまたぐ永続化は scripts/e2e.sh が実プロセスの再起動で検証する。 embedded engine は
    // drop 後も同一プロセス内では RocksDB の LOCK をすぐ手放さないので、 ここでは開き直さない。
    let dir = unique_tmp_dir("rocksdb");
    let url = format!("rocksdb://{}", dir.display());

    let db = connect(&url, None, "chronista", "hub").await.unwrap();
    let applied = run_pending_migrations(&db, migrations_dir()).await.unwrap();
    assert_eq!(
        applied.len(),
        7,
        "fresh DB applies all migrations: {applied:?}"
    );
    let again = run_pending_migrations(&db, migrations_dir()).await.unwrap();
    assert!(again.is_empty(), "migrations re-applied: {again:?}");

    // mem ではなく rocksdb engine で開かれた証拠 = 指定 dir に RocksDB の実体がある。
    assert!(
        dir.join("CURRENT").exists(),
        "no RocksDB files under {dir:?}"
    );

    drop(db);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn ws_url_connects_signs_in_and_migrates() {
    let Ok(url) = std::env::var("CHRONISTA_HUB_SURREAL_TEST_URL") else {
        eprintln!(
            "[skip] ws_url_connects_signs_in_and_migrates: CHRONISTA_HUB_SURREAL_TEST_URL not set"
        );
        return;
    };
    let _serial = WS_SERIAL.lock().await;
    let auth = DbAuth {
        level: DbAuthLevel::Root,
        username: std::env::var("CHRONISTA_HUB_SURREAL_TEST_USERNAME").unwrap_or("root".into()),
        password: std::env::var("CHRONISTA_HUB_SURREAL_TEST_PASSWORD").unwrap_or("root".into()),
    };
    // 実行ごとに別 database にして、 共有 server 上でも前回の状態を引きずらない。
    let dbname = format!("hub_test_{}", std::process::id());

    let db = connect(&url, Some(&auth), "chronista_test", &dbname)
        .await
        .unwrap();
    let applied = run_pending_migrations(&db, migrations_dir()).await.unwrap();
    assert_eq!(
        applied.len(),
        7,
        "remote DB applies all migrations: {applied:?}"
    );

    db.query(format!("REMOVE DATABASE {dbname}"))
        .await
        .unwrap()
        .check()
        .unwrap();
}

/// 本番の運用形 (ADR-022 D2): bootstrap 済みの DB に database user で繋ぎ、 以後の
/// migration (DEFINE TABLE 等) を適用できること。 001 の DEFINE NAMESPACE は root 専用なので
/// bootstrap は root で済ませておく。
#[tokio::test]
async fn ws_database_user_applies_later_migrations() {
    let Ok(url) = std::env::var("CHRONISTA_HUB_SURREAL_TEST_URL") else {
        eprintln!(
            "[skip] ws_database_user_applies_later_migrations: CHRONISTA_HUB_SURREAL_TEST_URL not set"
        );
        return;
    };
    let _serial = WS_SERIAL.lock().await;
    let root = DbAuth {
        level: DbAuthLevel::Root,
        username: std::env::var("CHRONISTA_HUB_SURREAL_TEST_USERNAME").unwrap_or("root".into()),
        password: std::env::var("CHRONISTA_HUB_SURREAL_TEST_PASSWORD").unwrap_or("root".into()),
    };
    let dbname = format!("hub_dbuser_{}", std::process::id());

    // bootstrap (root): 既存 migration を全部適用し、 database user を作る。
    let admin = connect(&url, Some(&root), "chronista_test", &dbname)
        .await
        .unwrap();
    run_pending_migrations(&admin, migrations_dir())
        .await
        .unwrap();
    admin
        .query("DEFINE USER OVERWRITE hubsvc ON DATABASE PASSWORD 'pw-test' ROLES OWNER")
        .await
        .unwrap()
        .check()
        .unwrap();

    // 既存 7 本 + 新しい 008 を置いた dir を database user で流す → 008 だけ適用される。
    let dir = unique_tmp_dir("migrations");
    std::fs::create_dir_all(&dir).unwrap();
    for entry in std::fs::read_dir(migrations_dir()).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), dir.join(entry.file_name())).unwrap();
    }
    std::fs::write(
        dir.join("008_test_probe.surql"),
        "DEFINE TABLE OVERWRITE probe SCHEMALESS;",
    )
    .unwrap();

    let svc = DbAuth {
        level: DbAuthLevel::Database,
        username: "hubsvc".into(),
        password: "pw-test".into(),
    };
    let db = connect(&url, Some(&svc), "chronista_test", &dbname)
        .await
        .unwrap();
    let applied = run_pending_migrations(&db, &dir).await.unwrap();
    assert_eq!(applied, vec!["008_test_probe".to_string()]);

    let _ = std::fs::remove_dir_all(&dir);
    admin
        .query(format!("REMOVE DATABASE {dbname}"))
        .await
        .unwrap()
        .check()
        .unwrap();
}
