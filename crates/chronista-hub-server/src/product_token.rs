//! ProductTokenStore — Hub 発行 product-token (ADR-010 Phase 2)。
//!
//! opaque token 方式: `cht_<random>` を発行し、 DB (`hub_product_token`) には SHA-256 hash
//! のみ保存。 検証は hash lookup (embedded SurrealDB なので µs 級)。 即時 revoke = 行更新。
//! 署名鍵を持たないので鍵管理・rotation が不要、 ADR-010 の「Hub 自身で即時 invalidate」に直結。
//!
//! rotation: 新 token を発行し、 旧 active token の expires_at を 30 日後へ短縮
//! (新旧 30 日 overlap、 ADR-010)。

use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::auth::Principal;
use crate::db::Db;

/// token prefix (secret scanning / 目視識別用)。
const TOKEN_PREFIX: &str = "cht_";
/// default TTL (ADR-010: 1 年)。
const DEFAULT_TTL_DAYS: u32 = 365;
/// rotation 時の旧 token overlap (ADR-010: 30 日)。
const ROTATE_OVERLAP_DAYS: u32 = 30;

/// 平文 token を生成し (plaintext, sha256-hex) を返す。 平文は呼び出し側が一度だけ返す。
fn generate_token() -> (String, String) {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    let plaintext = format!("{TOKEN_PREFIX}{}", hex::encode(bytes));
    (plaintext.clone(), hash_token(&plaintext))
}

fn hash_token(plaintext: &str) -> String {
    hex::encode(Sha256::digest(plaintext.as_bytes()))
}

mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        bytes.as_ref().iter().map(|b| format!("{b:02x}")).collect()
    }
}

/// 発行結果。 `token` (平文) はこのレスポンス以外で取得不可。
#[derive(Serialize)]
pub struct IssuedToken {
    /// 平文 token (一度きり)。
    pub token: String,
    pub token_id: String,
    pub app_id: String,
    pub scopes: Vec<String>,
    pub expires_at: String,
}

// Debug は手動実装で平文 token を REDACTED に (将来の `{:?}` log 追加による漏洩を構造的に防ぐ)。
impl std::fmt::Debug for IssuedToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IssuedToken")
            .field("token", &"[REDACTED]")
            .field("token_id", &self.token_id)
            .field("app_id", &self.app_id)
            .field("scopes", &self.scopes)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// token のメタ情報 (一覧用、 平文/hash は含まない)。
#[derive(Debug, Serialize, Deserialize)]
pub struct TokenMeta {
    pub token_id: String,
    pub app_id: String,
    pub scopes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub created_at: String,
    pub expires_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<String>,
}

#[derive(Clone)]
pub struct ProductTokenStore {
    db: Db,
}

impl ProductTokenStore {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// 新規 token を発行。 平文はこの戻り値のみ。
    pub async fn issue(
        &self,
        app_id: &str,
        scopes: &[String],
        ttl_days: Option<u32>,
        note: Option<String>,
    ) -> anyhow::Result<IssuedToken> {
        let (plaintext, hash) = generate_token();
        let ttl = ttl_days.unwrap_or(DEFAULT_TTL_DAYS);

        let rows: Vec<serde_json::Value> = self
            .db
            .query(format!(
                "CREATE type::record('hub_product_token', $hash) CONTENT {{
                    token_hash: $hash,
                    app_id: $app_id,
                    scopes: $scopes,
                    note: $note,
                    created_at: time::now(),
                    expires_at: time::now() + {ttl}d,
                    revoked_at: NONE
                }} RETURN token_hash, app_id, scopes, <string> expires_at AS expires_at"
            ))
            .bind(("hash", hash.clone()))
            .bind(("app_id", app_id.to_string()))
            .bind(("scopes", scopes.to_vec()))
            .bind(("note", note))
            .await?
            .take(0)?;

        let row = rows
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("token create returned no row"))?;
        Ok(IssuedToken {
            token: plaintext,
            token_id: hash,
            app_id: app_id.to_string(),
            scopes: scopes.to_vec(),
            expires_at: row
                .get("expires_at")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
        })
    }

    /// 平文 token を検証。 active (未 revoke + 未失効) なら App principal を返す。
    pub async fn verify(&self, plaintext: &str) -> anyhow::Result<Option<Principal>> {
        if !plaintext.starts_with(TOKEN_PREFIX) {
            return Ok(None);
        }
        let hash = hash_token(plaintext);
        #[derive(Deserialize)]
        struct Row {
            app_id: String,
            scopes: Vec<String>,
        }
        let rows: Vec<serde_json::Value> = self
            .db
            .query(
                "SELECT app_id, scopes FROM hub_product_token
                 WHERE token_hash = $hash AND revoked_at IS NONE AND expires_at > time::now()
                 LIMIT 1",
            )
            .bind(("hash", hash))
            .await?
            .take(0)?;
        let Some(value) = rows.into_iter().next() else {
            return Ok(None);
        };
        let row: Row = serde_json::from_value(value)?;
        Ok(Some(Principal::App {
            app_id: row.app_id,
            scopes: row.scopes,
        }))
    }

    /// rotation (ADR-010): 新 token を発行し、 旧 active token の expires_at を
    /// 30 日後へ短縮 (既に 30 日未満ならそのまま)。
    ///
    /// - **atomic**: 短縮 + 発行を 1 transaction で実行 (途中 crash で「旧だけ短縮・新なし」を防ぐ)
    /// - **scopes 継承**: `scopes` が空なら直近 active token の scopes を引き継ぐ
    ///   (`POST /rotate {}` で権限なし token が生まれる事故の防止)
    pub async fn rotate(
        &self,
        app_id: &str,
        scopes: &[String],
        note: Option<String>,
    ) -> anyhow::Result<IssuedToken> {
        let (plaintext, hash) = generate_token();
        let ttl = DEFAULT_TTL_DAYS;
        // scopes 未指定なら直近 active token から継承 (SQL 側の分岐に頼らず Rust で SQL を出し分け)
        let scopes_expr = if scopes.is_empty() {
            "$inherited"
        } else {
            "$scopes"
        };
        let sql = format!(
            "BEGIN TRANSACTION;
             LET $inherited = (SELECT VALUE scopes FROM hub_product_token
                 WHERE app_id = $app_id AND revoked_at IS NONE AND expires_at > time::now()
                 ORDER BY created_at DESC LIMIT 1)[0] ?? [];
             UPDATE hub_product_token
                 SET expires_at = time::now() + {ROTATE_OVERLAP_DAYS}d
                 WHERE app_id = $app_id AND revoked_at IS NONE
                   AND expires_at > time::now() + {ROTATE_OVERLAP_DAYS}d;
             CREATE type::record('hub_product_token', $hash) CONTENT {{
                 token_hash: $hash,
                 app_id: $app_id,
                 scopes: {scopes_expr},
                 note: $note,
                 created_at: time::now(),
                 expires_at: time::now() + {ttl}d,
                 revoked_at: NONE
             }} RETURN token_hash, app_id, scopes, <string> expires_at AS expires_at;
             COMMIT TRANSACTION;"
        );
        let mut response = self
            .db
            .query(sql)
            .bind(("app_id", app_id.to_string()))
            .bind(("scopes", scopes.to_vec()))
            .bind(("hash", hash.clone()))
            .bind(("note", note))
            .await?
            .check()?;
        // statement index (実測: BEGIN/COMMIT も index を占有する):
        // 0=BEGIN 1=LET 2=UPDATE 3=CREATE 4=COMMIT
        let rows: Vec<serde_json::Value> = response.take(3)?;
        let row = rows
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("rotate create returned no row"))?;
        Ok(IssuedToken {
            token: plaintext,
            token_id: hash,
            app_id: app_id.to_string(),
            scopes: row
                .get("scopes")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            expires_at: row
                .get("expires_at")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
        })
    }

    /// 即時 revoke。 対象が app_id に属し active だったら true
    /// (app_id を照合し、 別 app の token を hash 指定で revoke できないようにする)。
    pub async fn revoke(&self, app_id: &str, token_id: &str) -> anyhow::Result<bool> {
        let rows: Vec<serde_json::Value> = self
            .db
            .query(
                "UPDATE hub_product_token SET revoked_at = time::now()
                 WHERE token_hash = $id AND app_id = $app_id AND revoked_at IS NONE
                 RETURN token_hash",
            )
            .bind(("id", token_id.to_string()))
            .bind(("app_id", app_id.to_string()))
            .await?
            .take(0)?;
        Ok(!rows.is_empty())
    }

    /// app の token 一覧 (メタのみ)。
    pub async fn list(&self, app_id: &str) -> anyhow::Result<Vec<TokenMeta>> {
        let rows: Vec<serde_json::Value> = self
            .db
            .query(
                "SELECT token_hash AS token_id, app_id, scopes, note,
                        <string> created_at AS created_at,
                        <string> expires_at AS expires_at,
                        IF revoked_at IS NONE THEN NONE ELSE <string> revoked_at END AS revoked_at
                 FROM hub_product_token WHERE app_id = $app_id ORDER BY created_at",
            )
            .bind(("app_id", app_id.to_string()))
            .await?
            .take(0)?;
        rows.into_iter()
            .map(|v| serde_json::from_value::<TokenMeta>(v).map_err(anyhow::Error::from))
            .collect()
    }
}
