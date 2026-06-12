//! JwksVerifier の hermetic テスト。
//!
//! 固定 RSA テスト鍵で JWT を署名 → JWKS で検証する round-trip。 network 非依存
//! (`fetch_jwks` のみローカル HTTP server を立てて検証)。 鍵は test 専用の捨て鍵。

use chronista_hub_server::auth::{JwksVerifier, Principal, Verifier, fetch_jwks};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde::Serialize;

const KID: &str = "test-key-1";
const ISSUER: &str = "https://id.creo-memories.in/";
const FUTURE: i64 = 4_102_444_800; // 2100-01-01
const PAST: i64 = 1_000_000_000; // 2001-09-09

// test 専用 捨て RSA 鍵 (PKCS#1)。
const PRIVATE_PEM: &str = "-----BEGIN RSA PRIVATE KEY-----
MIIEogIBAAKCAQEAuvVcwOJyT1qoTOY3EkvutMwr1HDNOlWd9LlRb4XPxaJIpNJf
DLWQVCdKXp9P2MGARjQCxrDU7B6eJ9IDqOvwcmFMUEalyf3eQfq/A8Nnv5bevpyo
WDmFkQWro3XmfBiLJQ+9UiPhOdqHYeWVUI1rOx4UKDaZ73V5q/xDqZ8OpQVpQ0fg
Ls9Y4A7vj93LBkVI3cV7uc+h7zqTAX71J901eWTmJKcNB74qloYTgNVjNEOCOIK7
13nX8C84SvgcZLX0NthK+buIQK1kQsA7jkTeiKhshwntDKCfAiBTOy0oRTBP/5KN
Ggv8bcFzr2isRePA89TZN/1zx+8+p5/yAgsKHwIDAQABAoIBACONThaV2TPy4udA
rf9KNjnmS31p9/TcXB2x8eT5trF380V4hb+mdSwzjoZg2C+5WDnBYTfEX7EI/31G
CBEi0MYHb5oiXRuErbOxSYqfKYb62x+3LaIdSiNyRxnd7Tby+d7R1+gbT5SPVEhO
/q5jPh1vUlj1TR+GoN0OKxXB2+iQQ00yGwmHImJYJ43BLkkn3YfXJqK5/dMxLbCE
t2cuhMtO57EVVlhfENa9uYo6n8x9x788bp2hImIaG5ntOSq4rctGYd0FJfExnCoa
mI5TwuU7aOp4R12g/gbAG3dUY8CiRzShqQRIZEDF60agMVtEKTwd+8OFqQ4oBhgj
Ifb8klECgYEA/Hl7s2LBUZvOQ42loawdrRVwPk81MbOTn2kLVdy9FG9zJf8yqPXw
pdlcEgBkoQ27NtgiMFkkMtRli57hpQyZlNfZKTPx1CXUwD6XxdSkaMArnvkSZJSk
x7C0zFybm0204xnTiLKQyWBI+KW1bnG/QwAy/b6jUkylVlzEDzQJAPECgYEAvZGu
A917GJAnM8+sAVSF8S4c7O/VaZBAtUIGUmXh8JWvVulr0PTXSB/ixaJUrWvw4JHo
2hWg9ed37Mgr+KpFLZwlOTkOlzS/iA4AnbiiYEv32sFJQuVm0ZT6ix9xJQFM5uK/
9YXifgQRvZzEwO0ZqqJgGINrRWxFafVHIbYDvA8CgYB1R01B7+7TJOf0k1jMN/J1
E09Xcl3IX52EYDxGv0oJsxevH9N9jvkhYU2Wgx47ffBoMo/3G4FoJyegasZwb+Dr
tjSHIj0EiipAvxKrb/KLQjFBIHv9wtqkdB4YDDCwPLF5COctSZ1eHd7nuboEusvY
qMAHBMZDFZ17942PbmF8UQKBgATIxnGGh3LJQJQIK7kk3vSFS2mXa/VsFJX+gpZV
x+wAexpgbb4qT7ycQWbnf+eYj827IPtQDG3oV5h8PM/bzD8ob7AQBpQ+Wo8ee1l/
rWlswWad9jFgBMZJUkFsm7hpXf19v4Z8yIiRpbj5WeXclgc+bdpwhqaL4vyXmiH5
rAJ1AoGAAbJqKN/5c5A9BMpj8WzvOPcEnIC6XJQoyQdfjM6IsxorwPbZfnnRZ/DC
OUEP5MSBzliDkQr+a4Set+7rUmE5MJWaqv0AcC0WnH3JlTgPYBOrp76sESvJMCZH
ZiiSEbKWItWMMxIG1NEQKClhMa8c5JvWVPstcm8H0LgTcWkkL60=
-----END RSA PRIVATE KEY-----
";

// 上記 private key に対応する公開鍵 (modulus n、 exponent AQAB)。
const JWK_N: &str = "uvVcwOJyT1qoTOY3EkvutMwr1HDNOlWd9LlRb4XPxaJIpNJfDLWQVCdKXp9P2MGARjQCxrDU7B6eJ9IDqOvwcmFMUEalyf3eQfq_A8Nnv5bevpyoWDmFkQWro3XmfBiLJQ-9UiPhOdqHYeWVUI1rOx4UKDaZ73V5q_xDqZ8OpQVpQ0fgLs9Y4A7vj93LBkVI3cV7uc-h7zqTAX71J901eWTmJKcNB74qloYTgNVjNEOCOIK713nX8C84SvgcZLX0NthK-buIQK1kQsA7jkTeiKhshwntDKCfAiBTOy0oRTBP_5KNGgv8bcFzr2isRePA89TZN_1zx-8-p5_yAgsKHw";

fn test_jwks() -> String {
    format!(
        r#"{{"keys":[{{"kty":"RSA","use":"sig","alg":"RS256","kid":"{KID}","n":"{JWK_N}","e":"AQAB"}}]}}"#
    )
}

#[derive(Serialize)]
struct TestClaims {
    sub: String,
    iss: String,
    aud: serde_json::Value,
    exp: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    handle: Option<String>,
}

fn sign(claims: &TestClaims) -> String {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(KID.into());
    let key = EncodingKey::from_rsa_pem(PRIVATE_PEM.as_bytes()).expect("encoding key");
    encode(&header, claims, &key).expect("sign")
}

fn verifier(allow_app: bool) -> JwksVerifier {
    JwksVerifier::from_jwks_json(
        &test_jwks(),
        ISSUER,
        vec!["chronista-hub".into()],
        allow_app,
    )
    .expect("build verifier")
}

fn claims(aud: serde_json::Value, iss: &str, exp: i64) -> TestClaims {
    TestClaims {
        sub: "usr_Fj7cx53h".into(),
        iss: iss.into(),
        aud,
        exp,
        scope: Some("events.publish.atlas events.read".into()),
        handle: Some("mito".into()),
    }
}

#[test]
fn valid_user_token_verifies() {
    let tok = sign(&claims(serde_json::json!("chronista-hub"), ISSUER, FUTURE));
    match verifier(false).verify_user_token(&tok) {
        Some(Principal::User {
            user_id,
            handle,
            scopes,
        }) => {
            assert_eq!(user_id, "usr_Fj7cx53h");
            assert_eq!(handle.as_deref(), Some("mito"));
            assert!(scopes.contains(&"events.publish.atlas".to_string()));
        }
        other => panic!("expected User principal, got {other:?}"),
    }
}

#[test]
fn wrong_issuer_rejected() {
    let tok = sign(&claims(
        serde_json::json!("chronista-hub"),
        "https://evil.example/",
        FUTURE,
    ));
    assert!(verifier(false).verify_user_token(&tok).is_none());
}

#[test]
fn wrong_audience_rejected() {
    let tok = sign(&claims(
        serde_json::json!("some-other-service"),
        ISSUER,
        FUTURE,
    ));
    assert!(verifier(false).verify_user_token(&tok).is_none());
}

#[test]
fn expired_token_rejected() {
    let tok = sign(&claims(serde_json::json!("chronista-hub"), ISSUER, PAST));
    assert!(verifier(false).verify_user_token(&tok).is_none());
}

#[test]
fn audience_list_intersection_accepted() {
    // token の aud が array で、 我々の audience を 1 つ含む → OK (ADR-010)
    let tok = sign(&claims(
        serde_json::json!(["https://other/api", "chronista-hub"]),
        ISSUER,
        FUTURE,
    ));
    assert!(verifier(false).verify_user_token(&tok).is_some());
}

#[test]
fn garbage_token_rejected() {
    assert!(verifier(false).verify_user_token("not.a.jwt").is_none());
    assert!(verifier(false).verify_user_token("").is_none());
}

#[test]
fn interim_app_token_gated() {
    // allow_stub_app_token=true なら暫定 app-token を受理、 false なら拒否
    let tok = "app:creo-memories:register_resource";
    assert!(matches!(
        verifier(true).verify_app_token(tok),
        Some(Principal::App { .. })
    ));
    assert!(verifier(false).verify_app_token(tok).is_none());
}

#[tokio::test]
async fn fetch_jwks_over_http() {
    // ローカル HTTP server で JWKS を配信 → fetch_jwks で取得 → verifier 構築まで通す
    let app = axum::Router::new().route(
        "/.well-known/jwks.json",
        axum::routing::get(|| async { test_jwks() }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let url = format!("http://{addr}/.well-known/jwks.json");
    let body = fetch_jwks(&url).await.expect("fetch jwks");
    assert!(body.contains(KID));

    let v =
        JwksVerifier::from_jwks_json(&body, ISSUER, vec!["chronista-hub".into()], false).unwrap();
    let tok = sign(&claims(serde_json::json!("chronista-hub"), ISSUER, FUTURE));
    assert!(v.verify_user_token(&tok).is_some());
}
