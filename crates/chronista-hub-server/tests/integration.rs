//! 統合テスト — embedded SurrealDB (Mem / RocksDB) に対して migration + storage +
//! event_log + consumer + HTTP API を検証。 別プロセス不要 (in-process embedded)。

use std::path::Path;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chronista_hub_server::app::{AppState, build_router};
use chronista_hub_server::auth::StubVerifier;
use chronista_hub_server::consumer::{spawn_consumer, tick};
use chronista_hub_server::db::{connect_mem, run_pending_migrations};
use chronista_hub_server::event_log::EventLog;
use chronista_hub_server::model::{EventEnvelope, EventKind, Resource, Visibility};
use chronista_hub_server::product_token::ProductTokenStore;
use chronista_hub_server::storage::{Storage, TreeReadOptions};
use http_body_util::BodyExt;
use tower::ServiceExt;

fn migrations_dir() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../migrations"))
}

fn sample_resource(id: &str, handle: &str) -> Resource {
    Resource {
        id: id.into(),
        r#type: "memories-atlas".into(),
        path: format!("/creo-memories/{id}"),
        handle: handle.into(),
        visibility: Visibility::Public,
        payload: serde_json::json!({ "title": "Atlas" }),
        created_at: "2026-06-11T00:00:00Z".into(),
        updated_at: "2026-06-11T00:00:00Z".into(),
    }
}

fn sample_event(event_id: &str, idem: &str, res: Resource) -> EventEnvelope {
    EventEnvelope {
        event_id: event_id.into(),
        app_id: "creo-memories".into(),
        kind: EventKind::ResourceCreated,
        resource: res,
        idempotency: idem.into(),
        emitted_at: "2026-06-11T00:00:00Z".into(),
    }
}

async fn setup_mem() -> (Storage, EventLog, ProductTokenStore) {
    let db = connect_mem("chronista", "hub").await.unwrap();
    let applied = run_pending_migrations(&db, migrations_dir()).await.unwrap();
    assert_eq!(
        applied.len(),
        6,
        "expected 6 migrations applied, got {applied:?}"
    );
    (
        Storage::new(db.clone()),
        EventLog::new(db.clone()),
        ProductTokenStore::new(db),
    )
}

#[tokio::test]
async fn migrations_seed_reserved_handles() {
    let db = connect_mem("chronista", "hub").await.unwrap();
    run_pending_migrations(&db, migrations_dir()).await.unwrap();
    // 2 回目は冪等 (pending 0)
    let again = run_pending_migrations(&db, migrations_dir()).await.unwrap();
    assert!(
        again.is_empty(),
        "second run should be no-op, got {again:?}"
    );

    let count: Vec<i64> = db
        .query("SELECT VALUE count() FROM user WHERE account_type = 'reserved' GROUP ALL")
        .await
        .unwrap()
        .take(0)
        .unwrap();
    assert!(
        count.first().copied().unwrap_or(0) >= 12,
        "reserved handles seeded"
    );
}

#[tokio::test]
async fn storage_crud() {
    let (storage, _log, _pt) = setup_mem().await;
    let opts = TreeReadOptions::default();

    storage
        .upsert_resource(&sample_resource("atlas_1", "mito"))
        .await
        .unwrap();
    let got = storage.get_resource_by_id("atlas_1").await.unwrap();
    assert_eq!(got.as_ref().unwrap().handle, "mito");
    assert_eq!(got.unwrap().payload["title"], "Atlas");

    // by handle + visibility filter
    storage
        .upsert_resource(&Resource {
            visibility: Visibility::Private,
            ..sample_resource("atlas_2", "mito")
        })
        .await
        .unwrap();
    assert_eq!(
        storage
            .get_resources_by_handle("mito", &opts)
            .await
            .unwrap()
            .len(),
        2
    );
    let pub_only = storage
        .get_resources_by_handle(
            "mito",
            &TreeReadOptions {
                visibility: Some(Visibility::Public),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(pub_only.len(), 1);
    assert_eq!(pub_only[0].id, "atlas_1");

    // by path prefix
    let by_path = storage
        .get_resources_by_path("mito", "/creo-memories", &opts)
        .await
        .unwrap();
    assert_eq!(by_path.len(), 2);

    // update (same id) then delete
    storage
        .upsert_resource(&Resource {
            payload: serde_json::json!({"title":"v2"}),
            ..sample_resource("atlas_1", "mito")
        })
        .await
        .unwrap();
    assert_eq!(
        storage
            .get_resource_by_id("atlas_1")
            .await
            .unwrap()
            .unwrap()
            .payload["title"],
        "v2"
    );
    storage.delete_resource("atlas_1").await.unwrap();
    assert!(
        storage
            .get_resource_by_id("atlas_1")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn world_register_discover_unregister() {
    let (storage, _log, _pt) = setup_mem().await;

    // register: wld_id keyed (ADR-020 §S2)
    let reg_at = storage
        .register_world(
            Some("wld_test1"),
            "test-world",
            "Test World",
            &["[2400:4150::1]:32000".to_string()],
        )
        .await
        .unwrap();
    assert!(!reg_at.is_empty(), "registered_at should be set");

    // discover: vp-world list に wld_id + endpoints 付きで現れる
    let worlds = storage.list_resources_by_type("vp-world").await.unwrap();
    assert_eq!(worlds.len(), 1);
    assert_eq!(worlds[0].handle, "test-world");
    assert_eq!(worlds[0].payload["wld_id"], "wld_test1");
    assert_eq!(worlds[0].payload["endpoints"][0], "[2400:4150::1]:32000");

    // unregister by wld_id → entry が消え、削除数 1 を返す
    let removed = storage
        .unregister_world(Some("wld_test1"), None)
        .await
        .unwrap();
    assert_eq!(removed, 1, "one entry removed");
    assert!(
        storage
            .list_resources_by_type("vp-world")
            .await
            .unwrap()
            .is_empty(),
        "registry empty after unregister"
    );

    // 冪等: 2 回目は削除対象なし = 0 (no-op、エラーにならない)
    let removed2 = storage
        .unregister_world(Some("wld_test1"), None)
        .await
        .unwrap();
    assert_eq!(removed2, 0, "idempotent: nothing to remove");
}

#[tokio::test]
async fn event_log_append_and_dedup() {
    let (_storage, log, _pt) = setup_mem().await;
    let res = sample_resource("atlas_1", "mito");

    assert!(
        log.append(&sample_event("ev_1", "i1", res.clone()))
            .await
            .unwrap()
            .accepted
    );
    // idempotency dup
    let dup = log
        .append(&sample_event("ev_2", "i1", res.clone()))
        .await
        .unwrap();
    assert!(!dup.accepted);
    assert!(dup.reason.unwrap().contains("idempotency"));
    // event_id dup
    let dup2 = log
        .append(&sample_event("ev_1", "i2", res.clone()))
        .await
        .unwrap();
    assert!(!dup2.accepted);
    assert!(dup2.reason.unwrap().contains("event_id"));

    let pending = log.unprocessed(None).await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].envelope.event_id, "ev_1");

    log.mark_processed("ev_1").await.unwrap();
    assert_eq!(log.unprocessed(None).await.unwrap().len(), 0);
}

#[tokio::test]
async fn event_log_dlq_after_max_retries() {
    let (_storage, log, _pt) = setup_mem().await;
    log.append(&sample_event("ev_x", "ix", sample_resource("a", "mito")))
        .await
        .unwrap();

    // max 未満の失敗ではまだ pending (retry 対象)
    log.record_failure("ev_x", "boom", 5).await.unwrap();
    assert_eq!(log.unprocessed(None).await.unwrap().len(), 1);

    // 上限到達で dead-letter 化 → unprocessed から除外 (poison pill にならない)
    for _ in 0..4 {
        log.record_failure("ev_x", "boom", 5).await.unwrap();
    }
    assert_eq!(log.unprocessed(None).await.unwrap().len(), 0);
}

#[tokio::test]
async fn consumer_applies_event_to_storage() {
    let (storage, log, _pt) = setup_mem().await;
    log.append(&sample_event(
        "ev_1",
        "i1",
        sample_resource("atlas_1", "mito"),
    ))
    .await
    .unwrap();

    let (processed, errors) = tick(&log, &storage, 100).await;
    assert_eq!((processed, errors), (1, 0));

    let got = storage.get_resource_by_id("atlas_1").await.unwrap();
    assert_eq!(got.unwrap().handle, "mito");
}

#[tokio::test]
async fn http_publish_then_read() {
    let (storage, log, product_tokens) = setup_mem().await;
    let consumer = spawn_consumer(log.clone(), storage.clone(), 50);
    let state = AppState {
        storage,
        event_log: log,
        verifier: Arc::new(StubVerifier),
        product_tokens,
        admin_key: None,
        service: "chronista-hub".into(),
        version: "0.0.1".into(),
    };
    let router = build_router(state);

    // POST /v1/events (app token with register_resource scope)
    let body = serde_json::to_string(&serde_json::json!({
        "event_id": "ev_http_1",
        "app_id": "creo-memories",
        "kind": "resource.created",
        "idempotency": "idem_http_1",
        "emitted_at": "2026-06-11T00:00:00Z",
        "resource": sample_resource("atlas_http", "mito"),
    }))
    .unwrap();
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/events")
                .header("x-app-token", "app:creo-memories:register_resource")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    // consumer (50ms poll) が反映するまで待つ
    let mut found = false;
    for _ in 0..40 {
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        let r = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/tree/@mito")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = r.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        if v["resources"]
            .as_array()
            .map(|a| !a.is_empty())
            .unwrap_or(false)
        {
            assert_eq!(v["resources"][0]["id"], "atlas_http");
            assert_eq!(v["resources"][0]["createdAt"], "2026-06-11T00:00:00Z");
            found = true;
            break;
        }
    }
    assert!(
        found,
        "resource should appear in tree after consumer applies event"
    );

    // auth 無しは 401
    let unauth = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/events")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauth.status(), StatusCode::UNAUTHORIZED);

    consumer.stop().await;
}

// 注: RocksDB 永続化 (プロセス再起動でデータ残存) は in-process 再接続だと
// RocksDB の LOCK が同一プロセス内で解放されないため unit test 化しない。
// 別プロセス起動での検証は scripts/e2e.sh (バイナリ e2e) で担保する。

// ============================================================
// product-token (ADR-010 Phase 2)
// ============================================================

#[tokio::test]
async fn product_token_issue_verify_revoke() {
    let (_storage, _log, pt) = setup_mem().await;

    let issued = pt
        .issue(
            "creo-memories",
            &["register_resource".into()],
            None,
            Some("test".into()),
        )
        .await
        .unwrap();
    assert!(issued.token.starts_with("cht_"));

    // verify → App principal (app_id + scopes)
    let p = pt.verify(&issued.token).await.unwrap();
    match p {
        Some(chronista_hub_server::auth::Principal::App { app_id, scopes }) => {
            assert_eq!(app_id, "creo-memories");
            assert_eq!(scopes, vec!["register_resource".to_string()]);
        }
        other => panic!("expected App principal, got {other:?}"),
    }

    // 不正 token / prefix 違いは None
    assert!(pt.verify("cht_deadbeef").await.unwrap().is_none());
    assert!(pt.verify("not-a-token").await.unwrap().is_none());

    // revoke → 即時無効
    assert!(pt.revoke("creo-memories", &issued.token_id).await.unwrap());
    assert!(pt.verify(&issued.token).await.unwrap().is_none());
    // 二重 revoke は false
    assert!(!pt.revoke("creo-memories", &issued.token_id).await.unwrap());
    // app_id 不一致でも revoke できない (別 app の token を hash 指定で殺せない)
    let issued2 = pt.issue("creo-memories", &[], None, None).await.unwrap();
    assert!(!pt.revoke("other-app", &issued2.token_id).await.unwrap());
    assert!(pt.verify(&issued2.token).await.unwrap().is_some());
}

#[tokio::test]
async fn product_token_rotate_overlap() {
    let (_storage, _log, pt) = setup_mem().await;

    let old = pt
        .issue("vp", &["register_resource".into()], None, None)
        .await
        .unwrap();
    let new = pt
        .rotate("vp", &["register_resource".into()], None)
        .await
        .unwrap();

    // overlap: 新旧両方とも当面 valid (ADR-010: 30 日並走)
    assert!(pt.verify(&old.token).await.unwrap().is_some());
    assert!(pt.verify(&new.token).await.unwrap().is_some());

    // 旧 token の expires_at は短縮されている (meta で確認)
    let metas = pt.list("vp").await.unwrap();
    assert_eq!(metas.len(), 2);
    let old_meta = metas.iter().find(|m| m.token_id == old.token_id).unwrap();
    let new_meta = metas.iter().find(|m| m.token_id == new.token_id).unwrap();
    assert!(
        old_meta.expires_at < new_meta.expires_at,
        "rotated-out token must expire before the new one: {} vs {}",
        old_meta.expires_at,
        new_meta.expires_at
    );
}

#[tokio::test]
async fn admin_endpoints_gated_and_issue_flow() {
    let (storage, log, product_tokens) = setup_mem().await;

    // --- admin_key 未設定 → 管理 API は 404 (存在ごと隠す) ---
    let state_no_admin = AppState {
        storage: storage.clone(),
        event_log: log.clone(),
        verifier: Arc::new(StubVerifier),
        product_tokens: product_tokens.clone(),
        admin_key: None,
        service: "chronista-hub".into(),
        version: "0.0.1".into(),
    };
    let router = build_router(state_no_admin);
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/apps/creo-memories/tokens")
                .header("x-admin-key", "whatever")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // --- admin_key 設定済み ---
    let state = AppState {
        storage,
        event_log: log,
        verifier: Arc::new(StubVerifier),
        product_tokens,
        admin_key: Some("sekrit-admin".into()),
        service: "chronista-hub".into(),
        version: "0.0.1".into(),
    };
    let router = build_router(state);

    // wrong key → 403
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/apps/creo-memories/tokens")
                .header("x-admin-key", "wrong")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // correct key → 201 + 平文 token
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/apps/creo-memories/tokens")
                .header("x-admin-key", "sekrit-admin")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"scopes":["register_resource"]}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let issued: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let token = issued["token"].as_str().unwrap().to_string();
    assert!(token.starts_with("cht_"));

    // 発行された product-token で events ingestion が通る (本物の認証経路)
    let now = "2026-06-12T00:00:00Z";
    let event = serde_json::json!({
        "event_id": "ev_pt_1", "app_id": "creo-memories", "kind": "resource.created",
        "idempotency": "idem_pt_1", "emitted_at": now,
        "resource": sample_resource("atlas_pt", "mito"),
    });
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/events")
                .header("x-app-token", &token)
                .header("content-type", "application/json")
                .body(Body::from(event.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    // 一覧 (メタのみ、 平文/hash 含まず) → revoke → 同 token で 401
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/apps/creo-memories/tokens")
                .header("x-admin-key", "sekrit-admin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let listed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let token_id = listed["tokens"][0]["token_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        listed["tokens"][0].get("token").is_none(),
        "plaintext must not be listed"
    );

    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/v1/apps/creo-memories/tokens/{token_id}"))
                .header("x-admin-key", "sekrit-admin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/events")
                .header("x-app-token", &token)
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "revoked token must be rejected"
    );
}

#[tokio::test]
async fn product_token_rotate_inherits_scopes_when_empty() {
    let (_storage, _log, pt) = setup_mem().await;

    pt.issue(
        "gfp",
        &["register_resource".into(), "events.read".into()],
        None,
        None,
    )
    .await
    .unwrap();
    // scopes 未指定 (空) で rotate → 旧 token の scopes を継承 (権限なし token 事故の防止)
    let rotated = pt.rotate("gfp", &[], None).await.unwrap();
    assert_eq!(
        rotated.scopes,
        vec!["register_resource".to_string(), "events.read".to_string()]
    );

    // 明示指定があればそちらを使う
    let rotated2 = pt.rotate("gfp", &["only.this".into()], None).await.unwrap();
    assert_eq!(rotated2.scopes, vec!["only.this".to_string()]);
}
