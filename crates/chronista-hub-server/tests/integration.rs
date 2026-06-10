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

async fn setup_mem() -> (Storage, EventLog) {
    let db = connect_mem("chronista", "hub").await.unwrap();
    let applied = run_pending_migrations(&db, migrations_dir()).await.unwrap();
    assert_eq!(
        applied.len(),
        4,
        "expected 4 migrations applied, got {applied:?}"
    );
    (Storage::new(db.clone()), EventLog::new(db))
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
    let (storage, _log) = setup_mem().await;
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
async fn event_log_append_and_dedup() {
    let (_storage, log) = setup_mem().await;
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
async fn consumer_applies_event_to_storage() {
    let (storage, log) = setup_mem().await;
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
    let (storage, log) = setup_mem().await;
    let consumer = spawn_consumer(log.clone(), storage.clone(), 50);
    let state = AppState {
        storage,
        event_log: log,
        verifier: Arc::new(StubVerifier),
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
