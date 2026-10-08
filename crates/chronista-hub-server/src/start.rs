//! `/start` — Auth0 のやり直し先 (Default Login Route) の受け口 (ADR-023 Q1 の A)。
//!
//! Auth0 は cookie 無しでログイン画面を開いた人を、ここへ `?iss=<issuer>` 付きで送る
//! (OIDC の third-party initiated login と同じ形)。Hub は家族のアプリの一覧を見せ、
//! 押したアプリが自分のログインを始める。Hub は Auth0 の client を持たず、ログインの
//! 途中にも入らない (ADR-023 D2: ログインを Hub に依存させない)。
//!
//! 一覧は当面ここに持つ。アプリの名簿 (ADR-009) ができたら名簿から引く。

use axum::extract::{Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use crate::app::AppState;

/// 家族のアプリ 1 つ。`login_url` はそのアプリがログインを始める URL。
struct FamilyApp {
    name: &'static str,
    description: &'static str,
    login_url: &'static str,
}

const WEB_APPS: &[FamilyApp] = &[
    FamilyApp {
        name: "Creo Memories",
        description: "記憶と Atlas",
        login_url: "https://app.creo-memories.in/auth/login",
    },
    FamilyApp {
        name: "GFP",
        description: "Go Fast Packing",
        login_url: "https://app.gfp.works/auth/login",
    },
];

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
    html(StatusCode::OK, &page("Creo ID でログイン", &app_list()))
}

fn app_list() -> String {
    let mut items = String::new();
    for app in WEB_APPS {
        items.push_str(&format!(
            "<li><a href=\"{url}\"><strong>{name}</strong><span>{desc}</span></a></li>",
            url = escape(app.login_url),
            name = escape(app.name),
            desc = escape(app.description),
        ));
    }
    format!(
        "<p>使うアプリを選んでください。どのアプリも同じ Creo ID でログインできます。</p>\
         <ul class=\"apps\">{items}</ul>\
         <p class=\"note\">Vantage Point は端末で <code>vp auth login</code> を実行してください。</p>"
    )
}

fn page(title: &str, body: &str) -> String {
    format!(
        "<!doctype html><html lang=\"ja\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>{title} — Chronista</title><style>{CSS}</style></head>\
         <body><main><h1>{title}</h1>{body}</main></body></html>",
        title = escape(title),
    )
}

const CSS: &str = ":root{color-scheme:light dark;--bg:#f7f7f5;--fg:#1d1d1f;--muted:#6b6b70;--card:#fff;--line:#e3e3e0;--accent:#2f5bd3}\
@media (prefers-color-scheme:dark){:root{--bg:#141416;--fg:#ececee;--muted:#9a9aa2;--card:#1d1d21;--line:#2c2c31;--accent:#8aa8ff}}\
body{margin:0;background:var(--bg);color:var(--fg);font:16px/1.6 system-ui,-apple-system,\"Hiragino Sans\",sans-serif}\
main{max-width:28rem;margin:0 auto;padding:3rem 1rem}h1{font-size:1.4rem;margin:0 0 1rem}\
.apps{list-style:none;padding:0;margin:1.5rem 0;display:grid;gap:.75rem}\
.apps a{display:flex;flex-direction:column;padding:1rem;border:1px solid var(--line);border-radius:12px;background:var(--card);color:inherit;text-decoration:none}\
.apps a:hover,.apps a:focus-visible{border-color:var(--accent)}.apps span,.note{color:var(--muted);font-size:.9rem}";

fn html(status: StatusCode, body: &str) -> Response {
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

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
