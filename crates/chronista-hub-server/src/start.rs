//! `/start` — Auth0 のやり直し先 (Default Login Route) の受け口 (ADR-023 Q1 の A)。
//!
//! Auth0 は cookie 無しでログイン画面を開いた人を、ここへ `?iss=<issuer>` 付きで送る
//! (OIDC の third-party initiated login と同じ形)。Hub は家族のアプリの一覧を見せ、
//! 押したアプリが自分のログインを始める。各アプリのログインの途中に Hub は入らない
//! (ADR-023 D2: ログインを Hub に依存させない)。Hub が持つ唯一の Auth0 client は
//! `/settings` 自身のログイン用 (ADR-023 の 2026-10-11 追記)。
//!
//! 一覧はアプリの名簿 (`app` table、 ADR-009 の段階 1) から引く。`login_url` を持つ active な
//! アプリだけが並ぶ。

use axum::extract::{Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use crate::app::AppState;
use crate::model::AppEntry;

#[derive(Debug, Deserialize)]
pub struct StartQuery {
    iss: Option<String>,
}

pub async fn start(State(st): State<AppState>, Query(q): Query<StartQuery>) -> Response {
    if let Some(iss) = q.iss.as_deref()
        && iss != st.issuer
    {
        // 知らない発行元から来た人は、どのアプリのログインにも送らない。
        tracing::debug!(iss, "start: unknown issuer");
        return html(
            StatusCode::BAD_REQUEST,
            &page(
                "ログインの発行元が違います",
                "<p>このページは Creo ID からだけ使えます。お使いのアプリからログインし直してください。</p>",
            ),
        );
    }
    match st.storage.list_active_apps().await {
        Ok(apps) => html(
            StatusCode::OK,
            &page("Creo ID でログイン", &app_list(&apps)),
        ),
        Err(e) => {
            tracing::error!(error = %e, "start: app roster unavailable");
            html(
                StatusCode::SERVICE_UNAVAILABLE,
                &page(
                    "アプリの一覧を読めませんでした",
                    "<p>お使いのアプリからログインし直してください。</p>",
                ),
            )
        }
    }
}

fn app_list(apps: &[AppEntry]) -> String {
    let mut items = String::new();
    for app in apps {
        let Some(login_url) = app.login_url.as_deref() else {
            continue;
        };
        items.push_str(&format!(
            "<li><a href=\"{url}\"><strong>{name}</strong><span>{desc}</span></a></li>",
            url = escape(login_url),
            name = escape(&app.name),
            desc = escape(app.description.as_deref().unwrap_or("")),
        ));
    }
    format!(
        "<p>使うアプリを選んでください。どのアプリも同じ Creo ID でログインできます。</p>\
         <ul class=\"apps\">{items}</ul>\
         <p class=\"note\">Vantage Point は端末で <code>vp auth login</code> を実行してください。</p>"
    )
}

pub(crate) fn page(title: &str, body: &str) -> String {
    format!(
        "<!doctype html><html lang=\"ja\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>{title} — Chronista</title><style>{CSS}</style></head>\
         <body><main><h1>{title}</h1>{body}</main></body></html>",
        title = escape(title),
    )
}

pub(crate) const CSS: &str = ":root{color-scheme:light dark;--bg:#f7f7f5;--fg:#1d1d1f;--muted:#6b6b70;--card:#fff;--line:#e3e3e0;--accent:#2f5bd3}\
@media (prefers-color-scheme:dark){:root{--bg:#141416;--fg:#ececee;--muted:#9a9aa2;--card:#1d1d21;--line:#2c2c31;--accent:#8aa8ff}}\
body{margin:0;background:var(--bg);color:var(--fg);font:16px/1.6 system-ui,-apple-system,\"Hiragino Sans\",sans-serif}\
main{max-width:28rem;margin:0 auto;padding:3rem 1rem}h1{font-size:1.4rem;margin:0 0 1rem}\
.apps{list-style:none;padding:0;margin:1.5rem 0;display:grid;gap:.75rem}\
.apps a{display:flex;flex-direction:column;padding:1rem;border:1px solid var(--line);border-radius:12px;background:var(--card);color:inherit;text-decoration:none}\
.apps a:hover,.apps a:focus-visible{border-color:var(--accent)}.apps span,.note{color:var(--muted);font-size:.9rem}\
section{padding:1rem;margin:1rem 0;border:1px solid var(--line);border-radius:12px;background:var(--card)}h2{font-size:1rem;margin:0 0 .5rem}\
label{display:block;margin:.5rem 0}input{display:block;width:100%;box-sizing:border-box;margin-top:.25rem;padding:.5rem;font:inherit;border:1px solid var(--line);border-radius:8px;background:var(--bg);color:inherit}\
button{font:inherit;padding:.5rem 1rem;border-radius:8px;border:1px solid var(--accent);background:var(--accent);color:#fff;cursor:pointer}button.secondary{background:transparent;color:var(--accent)}\
.msg{padding:.5rem .75rem;border-radius:8px;background:var(--card);border:1px solid var(--accent)}code{font-size:.9em}";

pub(crate) fn html(status: StatusCode, body: &str) -> Response {
    (
        status,
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        body.to_string(),
    )
        .into_response()
}

pub(crate) fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
