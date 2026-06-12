//! axum Router 組み立て + handler。 TS server (health/tree/events) の API surface を移植。

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::auth::{AuthError, PrincipalKind, Verifier, authenticate};
use crate::event_log::EventLog;
use crate::model::{Visibility, validate_envelope};
use crate::product_token::ProductTokenStore;
use crate::storage::{Storage, TreeReadOptions};

#[derive(Clone)]
pub struct AppState {
    pub storage: Storage,
    pub event_log: EventLog,
    pub verifier: Arc<dyn Verifier>,
    pub product_tokens: ProductTokenStore,
    /// 管理 API (token 発行/rotate/revoke) を守る admin key。 None なら管理 API 無効 (fail-closed)。
    pub admin_key: Option<String>,
    pub service: String,
    pub version: String,
}

/// 500 に落とすための anyhow ラッパ。
pub struct AppError(anyhow::Error);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        // 詳細は log のみに出し、 クライアントには固定メッセージ (内部情報の漏洩防止)。
        tracing::error!(error = %self.0, "internal server error");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "internal server error" })),
        )
            .into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for AppError {
    fn from(e: E) -> Self {
        AppError(e.into())
    }
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(root))
        .route("/health", get(health))
        .route("/v1/tree/{handle}", get(tree_by_handle))
        .route("/v1/tree/{handle}/{*path}", get(tree_by_path))
        .route("/v1/resources/{id}", get(resource_by_id))
        .route("/v1/apps/{app_id}/manifest", get(app_manifest))
        .route("/v1/events", post(post_events))
        // --- admin (X-Admin-Key 必須、 HUB_ADMIN_KEY 未設定なら全て 404) ---
        .route(
            "/v1/apps/{app_id}/tokens",
            get(list_tokens).post(issue_token),
        )
        .route("/v1/apps/{app_id}/tokens/rotate", post(rotate_token))
        .route(
            "/v1/apps/{app_id}/tokens/{token_id}",
            axum::routing::delete(revoke_token),
        )
        .with_state(state)
}

async fn root(State(st): State<AppState>) -> Response {
    Json(json!({ "service": st.service, "version": st.version })).into_response()
}

async fn health(State(st): State<AppState>) -> Response {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    Json(json!({
        "status": "ok",
        "service": st.service,
        "version": st.version,
        "timestamp": ts,
    }))
    .into_response()
}

#[derive(Debug, Deserialize)]
struct TreeQuery {
    visibility: Option<String>,
    #[serde(rename = "type")]
    r#type: Option<String>,
    limit: Option<usize>,
}

impl TreeQuery {
    fn into_options(self) -> TreeReadOptions {
        let visibility = match self.visibility.as_deref() {
            Some("public") => Some(Visibility::Public),
            Some("shared") => Some(Visibility::Shared),
            Some("private") => Some(Visibility::Private),
            _ => None,
        };
        TreeReadOptions {
            visibility,
            r#type: self.r#type,
            limit: self.limit,
        }
    }
}

fn strip_handle(handle: &str) -> &str {
    handle.strip_prefix('@').unwrap_or(handle)
}

async fn tree_by_handle(
    State(st): State<AppState>,
    Path(handle): Path<String>,
    Query(q): Query<TreeQuery>,
) -> Result<Response, AppError> {
    let h = strip_handle(&handle);
    let resources = st
        .storage
        .get_resources_by_handle(h, &q.into_options())
        .await?;
    Ok(Json(json!({ "handle": h, "path": "/", "resources": resources })).into_response())
}

async fn tree_by_path(
    State(st): State<AppState>,
    Path((handle, path)): Path<(String, String)>,
    Query(q): Query<TreeQuery>,
) -> Result<Response, AppError> {
    let h = strip_handle(&handle);
    let normalized = format!("/{path}");
    let resources = st
        .storage
        .get_resources_by_path(h, &normalized, &q.into_options())
        .await?;
    Ok(Json(json!({ "handle": h, "path": normalized, "resources": resources })).into_response())
}

async fn resource_by_id(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    match st.storage.get_resource_by_id(&id).await? {
        Some(r) => Ok(Json(r).into_response()),
        None => Ok((StatusCode::NOT_FOUND, Json(json!({ "error": "not found" }))).into_response()),
    }
}

async fn app_manifest(
    State(st): State<AppState>,
    Path(app_id): Path<String>,
) -> Result<Response, AppError> {
    match st.storage.get_app_manifest(&app_id).await? {
        Some(m) => Ok(Json(m).into_response()),
        None => Ok((StatusCode::NOT_FOUND, Json(json!({ "error": "not found" }))).into_response()),
    }
}

async fn post_events(
    State(st): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AppError> {
    // --- auth (user|app、 app には register_resource scope 要求) ---
    let bearer = headers.get("authorization").and_then(|v| v.to_str().ok());
    let app_token = headers.get("x-app-token").and_then(|v| v.to_str().ok());
    let required = ["register_resource".to_string()];
    match authenticate(
        st.verifier.as_ref(),
        Some(&st.product_tokens),
        bearer,
        app_token,
        &[PrincipalKind::User, PrincipalKind::App],
        &required,
    )
    .await
    {
        Ok(_principal) => {}
        Err(AuthError::Unauthorized) => {
            return Ok((
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "unauthorized" })),
            )
                .into_response());
        }
        Err(AuthError::InsufficientScope { missing }) => {
            return Ok((
                StatusCode::FORBIDDEN,
                Json(json!({ "error": "insufficient scope", "missing_scopes": missing })),
            )
                .into_response());
        }
    }

    // --- parse + validate ---
    let value: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => {
            return Ok((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "invalid JSON" })),
            )
                .into_response());
        }
    };
    let envelope = match validate_envelope(&value) {
        Ok(e) => e,
        Err(errors) => {
            return Ok((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "validation failed", "details": errors })),
            )
                .into_response());
        }
    };

    // --- append (consumer が storage 反映) ---
    let result = st.event_log.append(&envelope).await?;
    if !result.accepted {
        return Ok((
            StatusCode::CONFLICT,
            Json(json!({ "error": "conflict", "reason": result.reason.unwrap_or_else(|| "duplicate".into()) })),
        )
            .into_response());
    }

    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "accepted": true, "event_id": envelope.event_id })),
    )
        .into_response())
}

// ============================================================
// Admin endpoints — product-token 発行 / rotate / revoke / 一覧 (ADR-010)
// ============================================================

/// X-Admin-Key を HUB_ADMIN_KEY と照合。 未設定なら 404 (機能ごと隠す)、 不一致は 403。
/// 比較は SHA-256 hash 同士 (naive な byte 比較の timing 漏れを避ける)。
fn check_admin(st: &AppState, headers: &HeaderMap) -> Result<(), Box<Response>> {
    use sha2::{Digest, Sha256};
    let Some(expected) = st.admin_key.as_deref() else {
        return Err(Box::new(
            (StatusCode::NOT_FOUND, Json(json!({ "error": "not found" }))).into_response(),
        ));
    };
    let provided = headers
        .get("x-admin-key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if Sha256::digest(provided.as_bytes()) != Sha256::digest(expected.as_bytes()) {
        return Err(Box::new(
            (
                StatusCode::FORBIDDEN,
                Json(json!({ "error": "invalid admin key" })),
            )
                .into_response(),
        ));
    }
    Ok(())
}

#[derive(Debug, Deserialize, Default)]
struct TokenRequest {
    #[serde(default)]
    scopes: Vec<String>,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    ttl_days: Option<u32>,
}

async fn issue_token(
    State(st): State<AppState>,
    Path(app_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AppError> {
    if let Err(resp) = check_admin(&st, &headers) {
        return Ok(*resp);
    }
    let req: TokenRequest = if body.is_empty() {
        TokenRequest::default()
    } else {
        match serde_json::from_slice(&body) {
            Ok(r) => r,
            Err(_) => {
                return Ok((
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "invalid JSON" })),
                )
                    .into_response());
            }
        }
    };
    let issued = st
        .product_tokens
        .issue(&app_id, &req.scopes, req.ttl_days, req.note)
        .await?;
    tracing::info!(app_id = %app_id, token_id = %issued.token_id, "admin: product-token issued");
    Ok((StatusCode::CREATED, Json(issued)).into_response())
}

async fn rotate_token(
    State(st): State<AppState>,
    Path(app_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AppError> {
    if let Err(resp) = check_admin(&st, &headers) {
        return Ok(*resp);
    }
    let req: TokenRequest = if body.is_empty() {
        TokenRequest::default()
    } else {
        serde_json::from_slice(&body).unwrap_or_default()
    };
    let issued = st
        .product_tokens
        .rotate(&app_id, &req.scopes, req.note)
        .await?;
    tracing::info!(app_id = %app_id, token_id = %issued.token_id, "admin: product-token rotated (30d overlap)");
    Ok((StatusCode::CREATED, Json(issued)).into_response())
}

async fn revoke_token(
    State(st): State<AppState>,
    Path((app_id, token_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    if let Err(resp) = check_admin(&st, &headers) {
        return Ok(*resp);
    }
    let revoked = st.product_tokens.revoke(&token_id).await?;
    if revoked {
        tracing::info!(app_id = %app_id, token_id = %token_id, "admin: product-token revoked");
        Ok((StatusCode::OK, Json(json!({ "revoked": true }))).into_response())
    } else {
        Ok((StatusCode::NOT_FOUND, Json(json!({ "error": "not found" }))).into_response())
    }
}

async fn list_tokens(
    State(st): State<AppState>,
    Path(app_id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    if let Err(resp) = check_admin(&st, &headers) {
        return Ok(*resp);
    }
    let tokens = st.product_tokens.list(&app_id).await?;
    Ok(Json(json!({ "tokens": tokens })).into_response())
}
