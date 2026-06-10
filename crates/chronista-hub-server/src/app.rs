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
use crate::storage::{Storage, TreeReadOptions};

#[derive(Clone)]
pub struct AppState {
    pub storage: Storage,
    pub event_log: EventLog,
    pub verifier: Arc<dyn Verifier>,
    pub service: String,
    pub version: String,
}

/// 500 に落とすための anyhow ラッパ。
pub struct AppError(anyhow::Error);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": self.0.to_string() })),
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
        bearer,
        app_token,
        &[PrincipalKind::User, PrincipalKind::App],
        &required,
    ) {
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
