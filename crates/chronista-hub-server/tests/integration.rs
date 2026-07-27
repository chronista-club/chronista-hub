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
        owner: None,
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
        7,
        "expected 7 migrations applied, got {applied:?}"
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
async fn node_register_discover_unregister() {
    let (storage, _log, _pt) = setup_mem().await;

    // register: node_id keyed (ADR-020 §S2)。 owner 無し (未認証 permissive) = public。
    let reg_at = storage
        .register_node(
            Some("nd_test1"),
            "test-node",
            "Test Node",
            &["[2400:4150::1]:32000".to_string()],
            None,
            Visibility::Public,
        )
        .await
        .unwrap();
    assert!(!reg_at.is_empty(), "registered_at should be set");

    // discover: vp-node list に node_id + endpoints 付きで現れる
    let nodes = storage.list_resources_by_type("vp-node").await.unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].handle, "test-node");
    assert_eq!(nodes[0].payload["node_id"], "nd_test1");
    assert_eq!(nodes[0].payload["endpoints"][0], "[2400:4150::1]:32000");

    // unregister by node_id → entry が消え、削除数 1 を返す
    let removed = storage
        .unregister_node(Some("nd_test1"), None, None)
        .await
        .unwrap();
    assert_eq!(removed, 1, "one entry removed");
    assert!(
        storage
            .list_resources_by_type("vp-node")
            .await
            .unwrap()
            .is_empty(),
        "registry empty after unregister"
    );

    // 冪等: 2 回目は削除対象なし = 0 (no-op、エラーにならない)
    let removed2 = storage
        .unregister_node(Some("nd_test1"), None, None)
        .await
        .unwrap();
    assert_eq!(removed2, 0, "idempotent: nothing to remove");
}

/// owner/visibility 分離 (ADR-020 §S5): Discover は「自分の node + public」だけを
/// 返し、 他人の private node は存在ごと見えない。 Unregister は owner guard。
#[tokio::test]
async fn node_owner_visibility_isolation() {
    let (storage, _log, _pt) = setup_mem().await;

    // user A: private node (認証済み登録の default)
    storage
        .register_node(
            Some("nd_a_priv"),
            "a-private",
            "A Private",
            &[],
            Some("usr_a"),
            Visibility::Private,
        )
        .await
        .unwrap();
    // user A: public node (明示 opt-in)
    storage
        .register_node(
            Some("nd_a_pub"),
            "a-public",
            "A Public",
            &[],
            Some("usr_a"),
            Visibility::Public,
        )
        .await
        .unwrap();
    // user B: private node
    storage
        .register_node(
            Some("nd_b_priv"),
            "b-private",
            "B Private",
            &[],
            Some("usr_b"),
            Visibility::Private,
        )
        .await
        .unwrap();
    // legacy 行相当: owner 無し + public (旧 client の permissive 登録)
    storage
        .register_node(
            Some("nd_legacy"),
            "legacy",
            "Legacy",
            &[],
            None,
            Visibility::Public,
        )
        .await
        .unwrap();

    // A の視界 = 自分の private + 自分の public + legacy public (B の private は見えない)
    let seen_by_a = storage.list_nodes_visible_to(Some("usr_a")).await.unwrap();
    let ids_a: Vec<&str> = seen_by_a.iter().map(|w| w.id.as_str()).collect();
    assert_eq!(
        ids_a,
        vec!["vp-node:nd_a_priv", "vp-node:nd_a_pub", "vp-node:nd_legacy"],
        "A sees own nodes + public only"
    );

    // B の視界 = 自分の private + A の public + legacy (A の private は見えない)
    let seen_by_b = storage.list_nodes_visible_to(Some("usr_b")).await.unwrap();
    let ids_b: Vec<&str> = seen_by_b.iter().map(|w| w.id.as_str()).collect();
    assert_eq!(
        ids_b,
        vec!["vp-node:nd_a_pub", "vp-node:nd_b_priv", "vp-node:nd_legacy"],
        "B sees own nodes + public only"
    );

    // 未認証 (viewer None) = public のみ
    let seen_anon = storage.list_nodes_visible_to(None).await.unwrap();
    let ids_anon: Vec<&str> = seen_anon.iter().map(|w| w.id.as_str()).collect();
    assert_eq!(
        ids_anon,
        vec!["vp-node:nd_a_pub", "vp-node:nd_legacy"],
        "anonymous sees public only"
    );

    // Unregister owner guard: B は A の node を消せない (存在も漏らさず removed=0)
    let removed = storage
        .unregister_node(Some("nd_a_priv"), None, Some("usr_b"))
        .await
        .unwrap();
    assert_eq!(removed, 0, "B cannot remove A's node");

    // 本人は消せる
    let removed = storage
        .unregister_node(Some("nd_a_priv"), None, Some("usr_a"))
        .await
        .unwrap();
    assert_eq!(removed, 1, "owner can remove own node");

    // owner 無し legacy entry は認証済み user からも掃除できる (stale entry 掃除の経路)
    let removed = storage
        .unregister_node(Some("nd_legacy"), None, Some("usr_b"))
        .await
        .unwrap();
    assert_eq!(
        removed, 1,
        "ownerless legacy entry is removable by any user"
    );
}

/// write-side owner guard (ADR-020 §S5): 他人の node_id を Register で乗っ取れない
/// (owner/endpoints/visibility の上書き防止)。 未認証・App も owned node を消せない。
#[tokio::test]
async fn node_register_hijack_and_delete_guards() {
    let (storage, _log, _pt) = setup_mem().await;

    // A が private node を owner 登録
    storage
        .register_node(
            Some("nd_x"),
            "x",
            "X",
            &["[2400:4150::1]:32000".to_string()],
            Some("usr_a"),
            Visibility::Private,
        )
        .await
        .unwrap();

    // B が同じ node_id で乗っ取ろうとする → Err (書き込まれない)
    let hijack = storage
        .register_node(
            Some("nd_x"),
            "x-evil",
            "X Evil",
            &["[dead:beef::1]:1".to_string()],
            Some("usr_b"),
            Visibility::Public,
        )
        .await;
    assert!(hijack.is_err(), "B cannot hijack A's node_id via Register");

    // 未認証 (owner None) も乗っ取れない
    let hijack_anon = storage
        .register_node(
            Some("nd_x"),
            "x-anon",
            "X Anon",
            &["[dead:beef::2]:2".to_string()],
            None,
            Visibility::Public,
        )
        .await;
    assert!(
        hijack_anon.is_err(),
        "anonymous cannot hijack A's owned node_id"
    );

    // A の entry は無傷 (owner/endpoints/visibility そのまま)
    let nodes = storage.list_resources_by_type("vp-node").await.unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].owner.as_deref(), Some("usr_a"));
    assert_eq!(nodes[0].visibility, Visibility::Private);
    assert_eq!(nodes[0].payload["endpoints"][0], "[2400:4150::1]:32000");

    // 本人は自分の node を再 Register で更新できる (owner 一致)
    storage
        .register_node(
            Some("nd_x"),
            "x",
            "X",
            &["[2400:4150::1]:33000".to_string()],
            Some("usr_a"),
            Visibility::Public,
        )
        .await
        .unwrap();
    let nodes = storage.list_resources_by_type("vp-node").await.unwrap();
    assert_eq!(
        nodes[0].visibility,
        Visibility::Public,
        "owner self-update ok"
    );
    assert_eq!(nodes[0].payload["endpoints"][0], "[2400:4150::1]:33000");

    // 未認証 (requester None) は A の owned node を消せない (§S5、 permissive path guard)
    let removed = storage
        .unregister_node(Some("nd_x"), None, None)
        .await
        .unwrap();
    assert_eq!(removed, 0, "anonymous cannot delete an owned node");

    // owner 無し entry を追加 → 未認証でも掃除できる (stale 掃除経路は維持)
    storage
        .register_node(
            Some("nd_free"),
            "free",
            "Free",
            &[],
            None,
            Visibility::Public,
        )
        .await
        .unwrap();
    let removed = storage
        .unregister_node(Some("nd_free"), None, None)
        .await
        .unwrap();
    assert_eq!(removed, 1, "anonymous can still clean ownerless entries");
}

/// REST 迂回防止 (ADR-020 §S5、 VP_NODE_REST_GUARD): 未認証 REST read
/// (`/v1/tree/@handle`・`/v1/resources/{id}` の backing) から vp-node の非 public を
/// 隠す。 product resource (type != vp-node) は private でも従来通り読める (guard 非対象)。
#[tokio::test]
async fn rest_read_hides_nonpublic_vp_nodes() {
    let (storage, _log, _pt) = setup_mem().await;
    let opts = TreeReadOptions::default();

    // 同じ handle の下に public / private の vp-node + private の product resource
    storage
        .register_node(
            Some("nd_pub"),
            "alice",
            "Alice Public",
            &["[2400:4150::1]:32000".to_string()],
            Some("usr_a"),
            Visibility::Public,
        )
        .await
        .unwrap();
    storage
        .register_node(
            Some("nd_priv"),
            "alice",
            "Alice Private",
            &["[2400:4150::9]:32000".to_string()],
            Some("usr_a"),
            Visibility::Private,
        )
        .await
        .unwrap();
    storage
        .upsert_resource(&Resource {
            visibility: Visibility::Private,
            ..sample_resource("atlas_p", "alice")
        })
        .await
        .unwrap();

    // tree read (handle): private vp-node だけ落ち、 product は private でも残る
    let tree = storage
        .get_resources_by_handle("alice", &opts)
        .await
        .unwrap();
    let ids: Vec<&str> = tree.iter().map(|r| r.id.as_str()).collect();
    assert!(ids.contains(&"vp-node:nd_pub"), "public node visible");
    assert!(
        !ids.contains(&"vp-node:nd_priv"),
        "private node hidden from REST tree"
    );
    assert!(ids.contains(&"atlas_p"), "private product unaffected");

    // path read も同じ guard
    let by_path = storage
        .get_resources_by_path("alice", "/", &opts)
        .await
        .unwrap();
    assert!(
        !by_path.iter().any(|r| r.id == "vp-node:nd_priv"),
        "private node hidden from path read"
    );

    // 直接 id read: private vp-node は存在ごと見えない、 public / product は読める
    assert!(
        storage
            .get_resource_by_id("vp-node:nd_priv")
            .await
            .unwrap()
            .is_none(),
        "private node hidden by id"
    );
    assert!(
        storage
            .get_resource_by_id("vp-node:nd_pub")
            .await
            .unwrap()
            .is_some(),
        "public node readable by id"
    );
    assert!(
        storage
            .get_resource_by_id("atlas_p")
            .await
            .unwrap()
            .is_some(),
        "private product readable by id"
    );
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
