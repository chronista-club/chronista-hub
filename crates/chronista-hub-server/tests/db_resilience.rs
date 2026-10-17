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

use chronista_hub_server::config::{DbAuth, DbAuthLevel};
use chronista_hub_server::db::{Db, connect};

async fn remote() -> Option<Db> {
    let url = std::env::var("CHRONISTA_HUB_SURREAL_TEST_URL").ok()?;
    let auth = DbAuth {
        level: DbAuthLevel::Root,
        username: std::env::var("CHRONISTA_HUB_SURREAL_TEST_USERNAME").unwrap_or("root".into()),
        password: std::env::var("CHRONISTA_HUB_SURREAL_TEST_PASSWORD").unwrap_or("root".into()),
    };
    let dbname = format!("resilience_{}", std::process::id());
    Some(
        connect(&url, Some(&auth), "chronista_test", &dbname)
            .await
            .unwrap(),
    )
}

/// セッションが使えなくなった (認証が外れた) 接続を、 点検が作り直して戻す。
/// 2026-10-15 は「セッションが無い」で、 ここでは「セッションの認証が無い」で同じ壊れ方を作る
#[tokio::test]
async fn a_broken_session_is_replaced_by_a_fresh_connection() {
    let Some(db) = remote().await else {
        eprintln!("skip: CHRONISTA_HUB_SURREAL_TEST_URL unset");
        return;
    };
    assert!(db.is_remote());
    db.ping().await.expect("a fresh connection answers");
    let before = db.conn();

    // 共有の接続を壊す
    db.conn()
        .query("INVALIDATE")
        .await
        .unwrap()
        .check()
        .unwrap();
    assert!(
        db.ping().await.is_err(),
        "the broken session must fail the ping"
    );

    let h = db.check().await;
    assert!(h.ok, "{h:?}");
    assert_eq!(h.reconnects, 1);
    assert_eq!(db.generation(), 1);
    assert!(
        !std::sync::Arc::ptr_eq(&before, &db.conn()),
        "a new connection replaced the old one"
    );
    db.ping().await.expect("the new connection answers");

    // 健全なら作り直さない
    let h = db.check().await;
    assert!(h.ok);
    assert_eq!(h.reconnects, 1);
}

/// 同時に点検しても、 作り直しは 1 回にまとまる。
#[tokio::test]
async fn concurrent_checks_reconnect_once() {
    let Some(db) = remote().await else {
        eprintln!("skip: CHRONISTA_HUB_SURREAL_TEST_URL unset");
        return;
    };
    db.conn()
        .query("INVALIDATE")
        .await
        .unwrap()
        .check()
        .unwrap();
    let (a, b, c) = tokio::join!(db.check(), db.check(), db.check());
    assert!(a.ok && b.ok && c.ok, "{a:?} {b:?} {c:?}");
    assert_eq!(db.health().reconnects, 1, "one reconnect for one breakage");
}

/// kick で見張りがすぐ点検する (定期の間隔を待たない)。
#[tokio::test]
async fn a_kick_wakes_the_watchdog() {
    let Some(db) = remote().await else {
        eprintln!("skip: CHRONISTA_HUB_SURREAL_TEST_URL unset");
        return;
    };
    let _w = db.spawn_watchdog(std::time::Duration::from_secs(3600));
    db.conn()
        .query("INVALIDATE")
        .await
        .unwrap()
        .check()
        .unwrap();
    db.kick();
    for _ in 0..50 {
        if db.health().reconnects == 1 && db.health().ok {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(db.health().reconnects, 1, "{:?}", db.health());
    db.ping().await.unwrap();
}
