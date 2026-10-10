//! `/settings` — 自分の handle と呼び名を見て変える 1 枚 (裁定 2026-10-10)。
//!
//! データの持ち主は分けたまま (誰か = Creo ID、 つながり = Hub)、 利用者向けの設定画面は
//! Hub に 1 つ。 handle は「必要になったときに claim する」ので、 アプリがここへ誘導する。
//!
//! ブラウザは Creo ID に PKCE (Authorization Code + S256) でログインして token を取り、
//! Hub の `/v1/me` を叩く。 Hub は Auth0 の SPA client を 1 つ持つ (client_id は公開値、
//! secret は無い)。 token はブラウザの sessionStorage にだけ置き、 Hub は cookie も session も
//! 持たない。 アカウント側 (email、 パスワード、 ログイン方法) は Creo ID の持ち物なので、
//! ここでは見せるだけで、 変更は Creo ID のフローへ委譲する。
//!
//! `HUB_CLIENT_ID` が無ければ 503 (準備中)。

use axum::extract::State;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;
use base64::Engine;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::app::AppState;
use crate::config::SettingsConfig;
use crate::start::{escape, html, page};

pub async fn settings(State(st): State<AppState>) -> Response {
    let Some(cfg) = st.settings.as_ref() else {
        tracing::debug!("settings: HUB_CLIENT_ID unset");
        return html(
            StatusCode::SERVICE_UNAVAILABLE,
            &page(
                "設定は準備中です",
                "<p>この画面はまだ使えません。お使いのアプリから設定してください。</p>",
            ),
        );
    };
    let mut resp = html(StatusCode::OK, &page("設定", &body(cfg, &st.issuer)));
    // token を sessionStorage に置くので、 XSS と clickjacking の二重目の壁として CSP を付ける。
    // inline script は中身が固定なので hash で許可する (nonce 不要)。 /start とは共有しない
    if let Ok(v) = HeaderValue::from_str(&csp(&st.issuer)) {
        resp.headers_mut()
            .insert(header::CONTENT_SECURITY_POLICY, v);
    }
    resp
}

/// `/settings` の Content-Security-Policy。 script は `JS` の sha256 だけ、 通信は自分と Creo ID だけ。
fn csp(issuer: &str) -> String {
    let hash = base64::engine::general_purpose::STANDARD.encode(Sha256::digest(JS.as_bytes()));
    let origin = issuer.trim_end_matches('/');
    format!(
        "default-src 'none'; script-src 'sha256-{hash}'; style-src 'unsafe-inline'; \
         connect-src 'self' {origin}; form-action 'self'; base-uri 'none'; frame-ancestors 'none'"
    )
}

fn body(cfg: &SettingsConfig, issuer: &str) -> String {
    // JS に渡す公開値。 `</script>` で抜けられないように `<` を < に逃がす
    let config = json!({
        "clientId": cfg.client_id,
        "issuer": issuer,
        "audience": cfg.audience,
        "redirectUri": format!("{}/settings", cfg.public_url),
    })
    .to_string()
    .replace('<', "\\u003c");
    format!(
        "<noscript><p>この画面には JavaScript が要ります。</p></noscript>\
         <div id=\"app\" aria-live=\"polite\"><p class=\"note\">読み込み中…</p></div>\
         <p class=\"note\">ログインとアカウント（メール、パスワード、ログイン方法）は \
         <a href=\"{issuer}\">Creo ID</a> が持っています。ここで変えられるのは、家族のアプリで \
         共通の handle と呼び名です。</p>\
         <script id=\"cfg\" type=\"application/json\">{config}</script>\
         <script>{JS}</script>",
        issuer = escape(issuer),
    )
}

/// 画面の JS。 依存なし、 1 ファイル。 流れ:
/// 1. `?code=` で戻ってきたら PKCE の verifier で token に交換して sessionStorage へ
/// 2. token があれば `/v1/me` を読み、 handle と呼び名のフォームを出す
/// 3. 無ければ「Creo ID でログイン」ボタン → authorize へ
const JS: &str = r#"
(() => {
  const cfg = JSON.parse(document.getElementById('cfg').textContent);
  const app = document.getElementById('app');
  const SS = window.sessionStorage;
  const K = { at: 'hub.settings.access_token', idt: 'hub.settings.id_token', v: 'hub.settings.pkce_verifier', s: 'hub.settings.pkce_state' };
  const esc = (s) => String(s ?? '').replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
  const b64url = (buf) => btoa(String.fromCharCode(...new Uint8Array(buf))).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
  const rand = () => { const a = new Uint8Array(32); crypto.getRandomValues(a); return b64url(a.buffer); };
  const sha256 = async (s) => b64url(await crypto.subtle.digest('SHA-256', new TextEncoder().encode(s)));
  const claims = (jwt) => { try { return JSON.parse(atob(jwt.split('.')[1].replace(/-/g, '+').replace(/_/g, '/'))); } catch { return {}; } };

  async function login() {
    const verifier = rand(), state = rand();
    SS.setItem(K.v, verifier); SS.setItem(K.s, state);
    const u = new URL('authorize', cfg.issuer);
    u.search = new URLSearchParams({
      response_type: 'code', client_id: cfg.clientId, redirect_uri: cfg.redirectUri,
      scope: 'openid profile email', audience: cfg.audience, state,
      code_challenge: await sha256(verifier), code_challenge_method: 'S256',
    });
    location.assign(u);
  }

  // Hub からだけ出る。 Creo ID の SSO session (家族のアプリ全部) までは切らない
  function logout() { forget(); render('ログアウトしました'); }
  function forget() { SS.removeItem(K.at); SS.removeItem(K.idt); }
  class Unauthorized extends Error {}

  async function exchange(code, state) {
    const verifier = SS.getItem(K.v), expected = SS.getItem(K.s);
    SS.removeItem(K.v); SS.removeItem(K.s);
    if (!verifier || !expected || state !== expected) throw new Error('ログインの戻りが一致しません。もう一度ログインしてください。');
    const res = await fetch(new URL('oauth/token', cfg.issuer), {
      method: 'POST', headers: { 'content-type': 'application/x-www-form-urlencoded' },
      body: new URLSearchParams({ grant_type: 'authorization_code', client_id: cfg.clientId, code, code_verifier: verifier, redirect_uri: cfg.redirectUri }),
    });
    if (!res.ok) throw new Error('token の交換に失敗しました (' + res.status + ')');
    const t = await res.json();
    SS.setItem(K.at, t.access_token); if (t.id_token) SS.setItem(K.idt, t.id_token);
  }

  async function api(method, path, body) {
    const res = await fetch(path, {
      method, headers: { authorization: 'Bearer ' + SS.getItem(K.at), ...(body ? { 'content-type': 'application/json' } : {}) },
      body: body ? JSON.stringify(body) : undefined,
    });
    if (res.status === 401) { forget(); throw new Unauthorized('ログインの期限が切れました。もう一度ログインしてください。'); }
    const data = await res.json().catch(() => ({}));
    if (!res.ok) throw new Error(data.error || ('エラー (' + res.status + ')'));
    return data;
  }

  function view(me, msg) {
    const id = claims(SS.getItem(K.idt) || '');
    const handle = me.handle
      ? `<p><strong>@${esc(me.handle)}</strong> <span class="note">${esc(location.origin + me.canonicalPath)}</span></p>
         <p class="note">handle は 1 人 1 つで、今は変えられません。</p>`
      : `<form id="claim"><label>handle <input name="handle" required pattern="[A-Za-z0-9][A-Za-z0-9-]{0,30}" autocomplete="off" placeholder="mito"></label>
         <button>この handle にする</button>
         <p class="note">小文字英数とハイフン、31 文字まで。家族のアプリで共通の住所になり、後から変えられません。必要になるまで決めなくても使えます。</p></form>`;
    app.innerHTML = `
      ${msg ? `<p class="msg">${esc(msg)}</p>` : ''}
      <section><h2>handle</h2>${handle}</section>
      <section><h2>呼び名</h2>
        <form id="name"><label>表示される名前 <input name="displayName" maxlength="80" value="${esc(me.displayName || '')}" placeholder="みと"></label>
        <button>保存</button></form>
        <p class="note">自由に付けられて、同じ名前の人がいても構いません。</p></section>
      <section><h2>アカウント</h2>
        <p>${esc(id.email || '')} <span class="note">${id.email_verified === false ? '（未確認）' : ''}</span></p>
        <p class="note">メール、パスワード、ログイン方法は Creo ID で管理します。</p>
        <p class="note">利用者 ID: <code>${esc(me.usrId)}</code></p>
        <button id="logout" class="secondary">ログアウト</button></section>`;
    const claim = document.getElementById('claim');
    const fail = (err) => (err instanceof Unauthorized ? render(err.message) : view(me, err.message));
    if (claim) claim.onsubmit = async (e) => { e.preventDefault(); try { const h = new FormData(claim).get('handle').trim().toLowerCase(); view(await api('PUT', '/v1/me/handle', { handle: h }), '@' + h + ' を claim しました'); } catch (err) { fail(err); } };
    document.getElementById('name').onsubmit = async (e) => { e.preventDefault(); try { const n = new FormData(e.target).get('displayName'); view(await api('PATCH', '/v1/me', { displayName: n.trim() === '' ? null : n }), '呼び名を保存しました'); } catch (err) { fail(err); } };
    document.getElementById('logout').onclick = logout;
  }

  // msg があれば一番上に出す (ログインの失敗、 期限切れ、 ログアウト)
  async function render(msg) {
    if (!SS.getItem(K.at)) {
      app.innerHTML = `${msg ? `<p class="msg">${esc(msg)}</p>` : ''}<p>家族のアプリで共通の handle と呼び名を設定します。</p><button id="login">Creo ID でログイン</button>`;
      document.getElementById('login').onclick = login;
      return;
    }
    try { view(await api('GET', '/v1/me'), msg); } catch (err) { err instanceof Unauthorized ? render(err.message) : (app.innerHTML = `<p class="msg">${esc(err.message)}</p>`); }
  }

  (async () => {
    const q = new URLSearchParams(location.search);
    let msg;
    if (q.has('code')) {
      try { await exchange(q.get('code'), q.get('state')); } catch (err) { msg = err.message; }
      history.replaceState(null, '', location.pathname);
    } else if (q.has('error')) {
      msg = q.get('error_description') || q.get('error');
      history.replaceState(null, '', location.pathname);
    }
    await render(msg);
  })();
})();
"#;
