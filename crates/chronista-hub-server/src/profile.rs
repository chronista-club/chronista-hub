//! `/@{handle}` と `/@{handle}/{slug}` — 名義のページと読める公開 URL (裁定 2026-10-14、 ADR-024)。
//!
//! - `/@{handle}`: 名義 (人 or 団体) のページ。 その名義の公開物を並べる
//! - `/@{handle}/{slug}`: 読める公開 URL。 索引を引いて、 中身のアプリの公開ページへ 302 で案内する。
//!   中身は Hub に置かない (Hub は案内だけ、 ADR-023)
//!
//! 正式な住所は `hub.chronista.club/@{handle}` (ADR-019)。 取り置き中の旧 handle (ADR-002 追記) は
//! 新しい住所へ 301。 期限があるので no-store。

use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::app::{AppError, AppState};
use crate::model::{canonical_handle, canonical_slug};
use crate::start::{escape, html, page};

/// `@` で始まる段だけが名義。 それ以外 (`/favicon.ico` など) は 404
fn handle_of(at: &str) -> Option<String> {
    at.strip_prefix('@').and_then(canonical_handle)
}

fn not_found() -> Response {
    html(
        StatusCode::NOT_FOUND,
        &page("見つかりません", "<p>このページはありません。</p>"),
    )
}

fn moved(location: String) -> Response {
    (
        StatusCode::MOVED_PERMANENTLY,
        [
            (header::LOCATION, location),
            (header::CACHE_CONTROL, "no-store".to_string()),
        ],
    )
        .into_response()
}

/// 名義のページ。
pub async fn profile(
    State(st): State<AppState>,
    Path(at): Path<String>,
) -> Result<Response, AppError> {
    let Some(h) = handle_of(&at) else {
        return Ok(not_found());
    };
    let Some(u) = st.storage.get_user_by_handle(&h).await? else {
        return Ok(
            match st
                .storage
                .resolve_moved_handle(&h)
                .await?
                .and_then(|u| u.handle)
            {
                Some(new) => moved(format!("/@{new}")),
                None => not_found(),
            },
        );
    };
    let Some(p) = u.public() else {
        return Ok(not_found());
    };
    let kind = if u.is_organization() {
        "団体"
    } else {
        "個人"
    };
    let pubs = st.storage.list_publications(&u.usr_id).await?;
    let list = if pubs.is_empty() {
        "<p class=\"note\">公開しているものはまだありません。</p>".to_string()
    } else {
        let items: String = pubs
            .iter()
            .map(|x| {
                let path = format!("/@{}/{}", p.handle, x.slug);
                format!(
                    "<li><a href=\"{href}\"><span class=\"text\"><strong>{title}</strong><span>{path}</span></span></a></li>",
                    href = escape(&path),
                    title = escape(x.title.as_deref().unwrap_or(&x.slug)),
                    path = escape(&format!("@{}/{}", p.handle, x.slug)),
                )
            })
            .collect();
        format!("<ul class=\"apps\">{items}</ul>")
    };
    let title = p
        .display_name
        .clone()
        .unwrap_or_else(|| format!("@{}", p.handle));
    let body = format!(
        "<p class=\"note\">@{handle} · {kind}</p>{list}",
        handle = escape(&p.handle),
    );
    Ok(html(StatusCode::OK, &page(&title, &body)))
}

/// 読める公開 URL。 索引にあれば中身のアプリへ 302 (案内先は登録で変わりうるので覚えさせない)。
pub async fn publication(
    State(st): State<AppState>,
    Path((at, slug)): Path<(String, String)>,
) -> Result<Response, AppError> {
    let (Some(h), Some(slug)) = (handle_of(&at), canonical_slug(&slug)) else {
        return Ok(not_found());
    };
    let Some(u) = st.storage.get_user_by_handle(&h).await? else {
        return Ok(
            match st
                .storage
                .resolve_moved_handle(&h)
                .await?
                .and_then(|u| u.handle)
            {
                Some(new) => moved(format!("/@{new}/{slug}")),
                None => not_found(),
            },
        );
    };
    if u.public().is_none() {
        return Ok(not_found());
    }
    match st.storage.get_publication(&u.usr_id, &slug).await? {
        Some(p) => Ok((
            StatusCode::FOUND,
            [
                (header::LOCATION, p.url),
                (header::CACHE_CONTROL, "no-store".to_string()),
            ],
        )
            .into_response()),
        None => Ok(not_found()),
    }
}
