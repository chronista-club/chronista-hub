//! `/start` — Auth0 のやり直し先 (Default Login Route) の受け口 (ADR-023 Q1 の A)。
//!
//! Creo ID の issuer で来た人に家族のアプリの一覧を見せ、押したアプリのログインへ送る。
//! Hub は Auth0 の client を持たず、ログインの途中に入らない (ADR-023 D2)。

use std::path::Path;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use chronista_hub_server::app::{AppState, build_router};
use chronista_hub_server::auth::StubVerifier;
use chronista_hub_server::db::{connect_mem, run_pending_migrations};
use chronista_hub_server::event_log::EventLog;
use chronista_hub_server::product_token::ProductTokenStore;
use chronista_hub_server::storage::Storage;
use http_body_util::BodyExt;
use tower::ServiceExt;

const ISSUER: &str = "https://id.creo-memories.in/";

async fn router() -> axum::Router {
    let db = connect_mem("chronista", "hub").await.unwrap();
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../migrations");
    run_pending_migrations(&db, &dir).await.unwrap();
    build_router(AppState {
        storage: Storage::new(db.clone()),
        event_log: EventLog::new(db.clone()),
        verifier: Arc::new(StubVerifier),
        product_tokens: ProductTokenStore::new(db),
        admin_key: None,
        issuer: ISSUER.into(),
        settings: None,
        service: "chronista-hub".into(),
        version: "0.0.1".into(),
    })
}

async fn get(uri: &str) -> (StatusCode, String, String) {
    let resp = router()
        .await
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let ctype = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    (status, ctype, String::from_utf8(body.to_vec()).unwrap())
}

#[tokio::test]
async fn start_with_creo_issuer_lists_family_app_logins() {
    let (status, ctype, body) = get("/start?iss=https%3A%2F%2Fid.creo-memories.in%2F").await;
    assert_eq!(status, StatusCode::OK);
    assert!(ctype.starts_with("text/html"), "content-type = {ctype}");
    assert!(body.contains("https://app.creo-memories.in/auth/login"));
    assert!(body.contains("https://app.gfp.works/auth/login"));
    // VP は CLI でログインする
    assert!(body.contains("vp auth login"));
}

#[tokio::test]
async fn start_without_issuer_still_shows_the_list() {
    let (status, _, body) = get("/start").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("https://app.creo-memories.in/auth/login"));
}

#[tokio::test]
async fn start_rejects_an_unknown_issuer() {
    let (status, _, body) = get("/start?iss=https%3A%2F%2Fevil.example%2F").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // 一覧は出さない (知らない発行元から来た人をアプリのログインへ送らない)
    assert!(!body.contains("/auth/login"));
}
