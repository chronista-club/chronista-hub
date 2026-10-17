//! DB の接続が壊れたときに Hub が自分で戻るか (incident 2026-10-15、 ADR-022 追記)。
//!
//! 実 SurrealDB が要るので `CHRONISTA_HUB_SURREAL_TEST_URL` で gate する (db_connect と同じ、
//! 未設定なら skip)。 手元では:
//!
//! ```sh
//! surreal start --user root --pass root --bind 127.0.0.1:18000 memory &
//! CHRONISTA_HUB_SURREAL_TEST_URL=ws://127.0.0.1:18000 \
//!   cargo test -p chronista-hub-server --test db_resilience
//! ```
//!
//! ⚠️ 使い捨てのローカル server 専用 (共有 instance の root で流さない)。
//!
//! 壊し方: SDK の `invalidate()` でセッションの認証を外す。 2026-10-15 は「セッションが無い」、
//! ここは「セッションの認証が無い」で、 どちらも「今の接続では何も通らない」同じ壊れ方。
//! SurrealQL の `INVALIDATE` 文では外れない (2026-10-18 に確認)。

use std::sync::Arc;
use std::time::Duration;

use chronista_hub_server::config::{DbAuth, DbAuthLevel};
use chronista_hub_server::db::{Db, connect, is_unavailable};

/// 同じ server に並列で ns / db を作ると write conflict になるので直列にする
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn remote(label: &str) -> Option<Db> {
    let url = std::env::var("CHRONISTA_HUB_SURREAL_TEST_URL").ok()?;
    let auth = DbAuth {
        level: DbAuthLevel::Root,
        username: std::env::var("CHRONISTA_HUB_SURREAL_TEST_USERNAME").unwrap_or("root".into()),
        password: std::env::var("CHRONISTA_HUB_SURREAL_TEST_PASSWORD").unwrap_or("root".into()),
    };
    let dbname = format!("resilience_{label}_{}", std::process::id());
    Some(
        connect(&url, Some(&auth), "chronista_test", &dbname)
            .await
            .unwrap(),
    )
}

async fn break_the_session(db: &Db) {
    db.conn().invalidate().await.unwrap();
}

#[tokio::test]
async fn a_broken_session_is_replaced_by_a_fresh_connection() {
    let _serial = SERIAL.lock().await;
    let Some(db) = remote("replace").await else {
        eprintln!("skip: CHRONISTA_HUB_SURREAL_TEST_URL unset");
        return;
    };
    assert!(db.is_remote());
    db.ping().await.expect("a fresh connection answers");
    let before = db.conn();

    break_the_session(&db).await;
    let err = db
        .ping()
        .await
        .expect_err("the broken session must fail the ping");
    assert!(
        is_unavailable(&err),
        "a session without auth is a broken connection for the Hub: {err}"
    );

    let h = db.check().await;
    assert!(h.ok, "{h:?}");
    assert_eq!(h.reconnects, 1);
    assert_eq!(db.generation(), 1);
    assert!(
        !Arc::ptr_eq(&before, &db.conn()),
        "a new connection replaced the old one"
    );
    db.ping().await.expect("the new connection answers");

    // 健全なら作り直さない
    let h = db.check().await;
    assert!(h.ok);
    assert_eq!(h.reconnects, 1);
}

#[tokio::test]
async fn concurrent_checks_reconnect_once() {
    let _serial = SERIAL.lock().await;
    let Some(db) = remote("concurrent").await else {
        eprintln!("skip: CHRONISTA_HUB_SURREAL_TEST_URL unset");
        return;
    };
    break_the_session(&db).await;
    let (a, b, c) = tokio::join!(db.check(), db.check(), db.check());
    assert!(a.ok && b.ok && c.ok, "{a:?} {b:?} {c:?}");
    assert_eq!(db.health().reconnects, 1, "one reconnect for one breakage");
}

#[tokio::test]
async fn a_kick_wakes_the_watchdog() {
    let _serial = SERIAL.lock().await;
    let Some(db) = remote("kick").await else {
        eprintln!("skip: CHRONISTA_HUB_SURREAL_TEST_URL unset");
        return;
    };
    // 定期の点検は 1 時間後。 kick だけが点検を起こす
    let watchdog = db.spawn_watchdog(Duration::from_secs(3600));
    break_the_session(&db).await;
    db.kick();
    for _ in 0..100 {
        if db.health().reconnects == 1 && db.health().ok {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(db.health().reconnects, 1, "{:?}", db.health());
    db.ping().await.unwrap();
    watchdog.abort();
}
