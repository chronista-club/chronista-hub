//! アプリの名簿 (ADR-009 の段階 1) — `app` table を家族のアプリの正本にする。
//!
//! - `GET /v1/apps`: 公開。active なアプリだけを返す
//! - `PUT /v1/apps/{app_id}`: 管理 (X-Admin-Key)。登録と更新
//! - `/start` は名簿から一覧を引く (ADR-023 追記)

use std::path::Path;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chronista_hub_server::app::{AppState, build_router};
use chronista_hub_server::auth::StubVerifier;
use chronista_hub_server::db::{connect_mem, run_pending_migrations};
use chronista_hub_server::event_log::EventLog;
use chronista_hub_server::product_token::ProductTokenStore;
use chronista_hub_server::storage::Storage;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const ADMIN_KEY: &str = "test-admin-key";

async fn router_with(admin_key: Option<&str>) -> axum::Router {
    let db = connect_mem("chronista", "hub").await.unwrap();
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../migrations");
    run_pending_migrations(&db, &dir).await.unwrap();
    build_router(AppState {
        storage: Storage::new(db.clone()),
        event_log: EventLog::new(db.clone()),
        verifier: Arc::new(StubVerifier),
        product_tokens: ProductTokenStore::new(db),
        admin_key: admin_key.map(String::from),
        issuer: "https://id.creo-memories.in/".into(),
        settings: None,
        service: "chronista-hub".into(),
        version: "0.0.1".into(),
    })
}

async fn send(router: &axum::Router, req: Request<Body>) -> (StatusCode, String) {
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(body.to_vec()).unwrap())
}

async fn get(router: &axum::Router, uri: &str) -> (StatusCode, String) {
    send(
        router,
        Request::builder().uri(uri).body(Body::empty()).unwrap(),
    )
    .await
}

async fn put_app(
    router: &axum::Router,
    app_id: &str,
    key: Option<&str>,
    body: Value,
) -> (StatusCode, String) {
    let mut req = Request::builder()
        .method("PUT")
        .uri(format!("/v1/apps/{app_id}"))
        .header("content-type", "application/json");
    if let Some(k) = key {
        req = req.header("x-admin-key", k);
    }
    send(router, req.body(Body::from(body.to_string())).unwrap()).await
}

fn app_ids(list_body: &str) -> Vec<String> {
    let v: Value = serde_json::from_str(list_body).unwrap();
    v["apps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["appId"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn the_family_apps_are_seeded_into_the_roster() {
    let r = router_with(None).await;
    let (status, body) = get(&r, "/v1/apps").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(app_ids(&body), ["creo-memories", "gfp", "vantage-point"]);

    let v: Value = serde_json::from_str(&body).unwrap();
    let creo = &v["apps"][0];
    assert_eq!(creo["name"], "Creo Memories");
    assert_eq!(creo["loginUrl"], "https://app.creo-memories.in/auth/login");
    // VP は CLI でログインするので loginUrl を持たない
    assert!(v["apps"][2].get("loginUrl").is_none());
}

#[tokio::test]
async fn registering_an_app_needs_the_admin_key() {
    let body = json!({ "name": "New App", "login_url": "https://new.example/login" });

    // HUB_ADMIN_KEY 未設定なら管理 API ごと隠す
    let hidden = router_with(None).await;
    let (status, _) = put_app(&hidden, "new-app", Some(ADMIN_KEY), body.clone()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let r = router_with(Some(ADMIN_KEY)).await;
    let (status, _) = put_app(&r, "new-app", None, body.clone()).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = put_app(&r, "new-app", Some("wrong"), body).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (_, list) = get(&r, "/v1/apps").await;
    assert!(!app_ids(&list).contains(&"new-app".to_string()));
}

#[tokio::test]
async fn a_registered_app_appears_in_the_roster_and_on_start() {
    let r = router_with(Some(ADMIN_KEY)).await;
    let (status, body) = put_app(
        &r,
        "new-app",
        Some(ADMIN_KEY),
        json!({
            "name": "New App",
            "description": "新しいアプリ",
            "home_url": "https://new.example",
            "login_url": "https://new.example/auth/login",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["appId"], "new-app");
    assert_eq!(v["status"], "active");

    let (_, list) = get(&r, "/v1/apps").await;
    assert!(app_ids(&list).contains(&"new-app".to_string()));

    let (_, start) = get(&r, "/start").await;
    assert!(start.contains("https://new.example/auth/login"));
    assert!(start.contains("新しいアプリ"));
}

#[tokio::test]
async fn updating_an_app_replaces_its_entry() {
    let r = router_with(Some(ADMIN_KEY)).await;
    let (status, _) = put_app(
        &r,
        "gfp",
        Some(ADMIN_KEY),
        json!({ "name": "Go Fast Packing", "login_url": "https://app.gfp.works/login" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, start) = get(&r, "/start").await;
    assert!(start.contains("https://app.gfp.works/login"));
    assert!(!start.contains("https://app.gfp.works/auth/login"));
}

#[tokio::test]
async fn a_deregistered_app_leaves_the_roster_and_start() {
    let r = router_with(Some(ADMIN_KEY)).await;
    let (status, _) = put_app(
        &r,
        "gfp",
        Some(ADMIN_KEY),
        json!({ "name": "GFP", "login_url": "https://app.gfp.works/auth/login", "status": "deregistered" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, list) = get(&r, "/v1/apps").await;
    assert_eq!(app_ids(&list), ["creo-memories", "vantage-point"]);
    let (_, start) = get(&r, "/start").await;
    assert!(!start.contains("app.gfp.works"));
}

#[tokio::test]
async fn registration_rejects_unsafe_or_malformed_input() {
    let r = router_with(Some(ADMIN_KEY)).await;
    let cases = [
        // /start に link として出るので https 以外は通さない
        (
            "new-app",
            json!({ "name": "X", "login_url": "javascript:alert(1)" }),
        ),
        (
            "new-app",
            json!({ "name": "X", "login_url": "http://new.example/login" }),
        ),
        (
            "new-app",
            json!({ "name": "X", "home_url": "ftp://new.example" }),
        ),
        (
            "new-app",
            json!({ "name": "X", "icon_url": "data:image/png;base64,AA" }),
        ),
        ("new-app", json!({ "name": "" })),
        ("new-app", json!({ "description": "name が無い" })),
        ("new-app", json!({ "name": "X", "status": "unknown" })),
        // GET の camelCase を PUT し返しても、 URL が黙って消えないよう弾く
        (
            "new-app",
            json!({ "name": "X", "loginUrl": "https://new.example/login" }),
        ),
        ("New_App", json!({ "name": "X" })),
        ("a", json!({ "name": "X" })),
    ];
    for (app_id, body) in cases {
        let (status, resp) = put_app(&r, app_id, Some(ADMIN_KEY), body.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{app_id} {body} → {resp}");
    }
    let (_, list) = get(&r, "/v1/apps").await;
    assert_eq!(app_ids(&list), ["creo-memories", "gfp", "vantage-point"]);
}

#[tokio::test]
async fn the_manifest_route_reads_the_roster() {
    let r = router_with(None).await;
    let (status, body) = get(&r, "/v1/apps/creo-memories/manifest").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["appId"], "creo-memories");
    assert_eq!(v["name"], "Creo Memories");

    let (status, _) = get(&r, "/v1/apps/no-such-app/manifest").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
