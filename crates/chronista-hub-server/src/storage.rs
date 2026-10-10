//! Storage — `hub_resource` table への read/write。 TS `SurrealStorage` と振る舞い互換。
//!
//! record id = type::record('hub_resource', resource.id)。 SurrealDB の `id` は予約名なので
//! product 側 resource id は `rid` field に複製して保持する。

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::db::Db;
use crate::model::{AppEntry, AppManifest, AppStatus, Resource, UserEntry, Visibility};

/// REST の未認証 tree/resource read で **vp-node (federation registry) の非 public を
/// 隠す** guard (ADR-020 §S5)。 owner/visibility 分離は Unison `nodes.Discover` が
/// owner-aware に担うが、 同じ `vp-node` 行 (endpoints 入り) は auth 無しの REST
/// `/v1/tree/@handle` / `/v1/resources/{id}` からも読めてしまう。 REST は principal を
/// 持たない (全 caller 未認証扱い) ため owner 判定はできず、 **public のみ露出**して
/// private/shared node の endpoints 漏洩を塞ぐ。 product resource (type != vp-node) は
/// 一切影響を受けない。
///
/// 旧名 `vp-world` (ADR-021 の語彙切替前に登録された行) も同じ node registry なので含める。
/// 含めないと非 public の旧行の endpoints が漏れる (2026-10-08 live で発覚)。
const VP_NODE_REST_GUARD: &str =
    " AND NOT (type IN ['vp-node', 'vp-world'] AND visibility != 'public')";

#[derive(Debug, Clone, Default)]
pub struct TreeReadOptions {
    pub visibility: Option<Visibility>,
    pub r#type: Option<String>,
    pub limit: Option<usize>,
}

/// hub_resource の行 (DB column 名に合わせる)。
#[derive(Debug, Serialize, Deserialize)]
struct ResourceRow {
    rid: String,
    #[serde(rename = "type")]
    r#type: String,
    path: String,
    handle: String,
    /// 所有者 usr_id。 旧行には column 自体が無い → serde default で None (migration 不要)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    owner: Option<String>,
    visibility: Visibility,
    payload: Value,
    #[serde(rename = "createdAt")]
    created_at: String,
    #[serde(rename = "updatedAt")]
    updated_at: String,
}

impl From<ResourceRow> for Resource {
    fn from(r: ResourceRow) -> Self {
        Resource {
            id: r.rid,
            r#type: r.r#type,
            path: r.path,
            handle: r.handle,
            owner: r.owner,
            visibility: r.visibility,
            payload: r.payload,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

impl ResourceRow {
    fn from_resource(r: &Resource) -> Self {
        ResourceRow {
            rid: r.id.clone(),
            r#type: r.r#type.clone(),
            path: r.path.clone(),
            handle: r.handle.clone(),
            owner: r.owner.clone(),
            visibility: r.visibility,
            payload: r.payload.clone(),
            created_at: r.created_at.clone(),
            updated_at: r.updated_at.clone(),
        }
    }
}

/// `PUT /v1/apps/{app_id}` で置き換えずに残す `app` の field (名簿の管理 API の外で決まるもの)。
const APP_FIELDS_KEPT_ON_PUT: &[&str] = &[
    "scopes",
    "verified",
    "manifest_version",
    "public_key_jwks_uri",
];

/// `app` table の行 (DB column 名 = spec の snake_case)。
#[derive(Debug, Serialize, Deserialize)]
struct AppRow {
    app_id: String,
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    icon_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    home_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    login_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    manifest_url: Option<String>,
    status: AppStatus,
}

impl From<AppRow> for AppEntry {
    fn from(r: AppRow) -> Self {
        AppEntry {
            app_id: r.app_id,
            name: r.name,
            description: r.description,
            icon_url: r.icon_url,
            home_url: r.home_url,
            login_url: r.login_url,
            manifest_url: r.manifest_url,
            status: r.status,
        }
    }
}

impl From<AppEntry> for AppRow {
    fn from(e: AppEntry) -> Self {
        AppRow {
            app_id: e.app_id,
            name: e.name,
            description: e.description,
            icon_url: e.icon_url,
            home_url: e.home_url,
            login_url: e.login_url,
            manifest_url: e.manifest_url,
            status: e.status,
        }
    }
}

#[derive(Clone)]
pub struct Storage {
    db: Db,
}

impl Storage {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    pub async fn get_resources_by_handle(
        &self,
        handle: &str,
        options: &TreeReadOptions,
    ) -> anyhow::Result<Vec<Resource>> {
        let (clause, limit) = filter_clause(options);
        let sql = format!(
            "SELECT * FROM hub_resource WHERE handle = $handle{VP_NODE_REST_GUARD}{clause} ORDER BY createdAt{limit}"
        );
        let mut binds = Map::new();
        binds.insert("handle".into(), json!(handle));
        add_filter_binds(&mut binds, options);
        let rows: Vec<Value> = self
            .db
            .query(sql)
            .bind(Value::Object(binds))
            .await?
            .take(0)?;
        rows_to_resources(rows)
    }

    pub async fn get_resources_by_path(
        &self,
        handle: &str,
        path: &str,
        options: &TreeReadOptions,
    ) -> anyhow::Result<Vec<Resource>> {
        let normalized = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };
        let (clause, limit) = filter_clause(options);
        let sql = format!(
            "SELECT * FROM hub_resource WHERE handle = $handle AND string::starts_with(path, $path){VP_NODE_REST_GUARD}{clause} ORDER BY createdAt{limit}"
        );
        let mut binds = Map::new();
        binds.insert("handle".into(), json!(handle));
        binds.insert("path".into(), json!(normalized));
        add_filter_binds(&mut binds, options);
        let rows: Vec<Value> = self
            .db
            .query(sql)
            .bind(Value::Object(binds))
            .await?
            .take(0)?;
        rows_to_resources(rows)
    }

    pub async fn get_resource_by_id(&self, id: &str) -> anyhow::Result<Option<Resource>> {
        // vp-node 非 public を REST の直接 id read から隠す (§S5、 VP_NODE_REST_GUARD)。
        let sql = format!("SELECT * FROM hub_resource WHERE rid = $id{VP_NODE_REST_GUARD} LIMIT 1");
        let rows: Vec<Value> = self
            .db
            .query(sql)
            .bind(("id", id.to_string()))
            .await?
            .take(0)?;
        Ok(rows_to_resources(rows)?.into_iter().next())
    }

    pub async fn get_app_manifest(&self, app_id: &str) -> anyhow::Result<Option<AppManifest>> {
        // manifest の取得 (well-known) は ADR-009 の段階 2。 今は名簿 (`app` table) から組む。
        let row: Option<Value> = self
            .db
            .query("SELECT * OMIT id FROM ONLY type::record('app', $id)")
            .bind(("id", app_id.to_string()))
            .await?
            .take(0)?;
        let Some(p) = row else {
            return Ok(None);
        };
        Ok(Some(AppManifest {
            app_id: app_id.to_string(),
            name: p
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or(app_id)
                .to_string(),
            version: p
                .get("manifest_version")
                .and_then(|v| v.as_str())
                .unwrap_or("0.0.0")
                .to_string(),
            permissions: p.get("scopes").and_then(|v| v.as_array()).map(|arr| {
                arr.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            }),
        }))
    }

    /// 名簿で active なアプリを app_id 順に返す (`GET /v1/apps`、 `/start`)。
    pub async fn list_active_apps(&self) -> anyhow::Result<Vec<AppEntry>> {
        let rows: Vec<Value> = self
            .db
            .query("SELECT * OMIT id FROM app WHERE status = 'active' ORDER BY app_id")
            .await?
            .take(0)?;
        rows.into_iter()
            .map(|v| {
                serde_json::from_value::<AppRow>(v)
                    .map(AppEntry::from)
                    .map_err(anyhow::Error::from)
            })
            .collect()
    }

    /// 名簿の 1 行を登録 / 置き換える。 `entry` に無い表示系の field は消す (PUT)。
    /// scope や verified など名簿の管理 API が扱わない field は残す (ADR-009 の段階 2 が持つ)。
    pub async fn put_app(&self, entry: &AppEntry) -> anyhow::Result<()> {
        let existing: Option<Value> = self
            .db
            .query("SELECT * OMIT id FROM ONLY type::record('app', $id)")
            .bind(("id", entry.app_id.clone()))
            .await?
            .take(0)?;
        let mut content = Map::new();
        if let Some(Value::Object(old)) = existing {
            for key in APP_FIELDS_KEPT_ON_PUT {
                if let Some(v) = old.get(*key) {
                    content.insert((*key).to_string(), v.clone());
                }
            }
        }
        let Value::Object(new) = serde_json::to_value(AppRow::from(entry.clone()))? else {
            anyhow::bail!("AppRow must serialize to an object");
        };
        content.extend(new);
        self.db
            .query("UPSERT type::record('app', $id) CONTENT $content")
            .bind(("id", entry.app_id.clone()))
            .bind(("content", Value::Object(content)))
            .await?
            .check()?;
        Ok(())
    }

    /// Creo ID の `sub` で利用者の行を引く。 まだ無ければ None。
    pub async fn get_user_by_sub(&self, sub: &str) -> anyhow::Result<Option<UserEntry>> {
        let rows: Vec<Value> = self
            .db
            .query("SELECT * OMIT id FROM user WHERE creo_sub = $sub LIMIT 1")
            .bind(("sub", sub.to_string()))
            .await?
            .take(0)?;
        rows_to_users(rows).map(|v| v.into_iter().next())
    }

    /// 初回接触 (`/v1/me`): `sub` の行が無ければ `usr_` EntId を振って作る (裁定 2026-10-09)。
    /// handle は空のまま。 同時に 2 回来ても `creo_sub` の unique index で 1 行に収束する。
    pub async fn ensure_user(&self, sub: &str) -> anyhow::Result<UserEntry> {
        if let Some(u) = self.get_user_by_sub(sub).await? {
            return Ok(u);
        }
        let usr_id = new_usr_id();
        let created = self
            .db
            .query(
                "CREATE type::record('user', $usr_id) CONTENT {
                    usr_id: $usr_id, creo_sub: $sub, account_type: 'user', locale: 'ja'
                }",
            )
            .bind(("usr_id", usr_id.clone()))
            .bind(("sub", sub.to_string()))
            .await?
            .check();
        match created {
            Ok(_) => tracing::info!(usr_id, "user joined the roster"),
            Err(e) if is_unique_violation(&e) => {
                tracing::info!("ensure_user: lost the race on creo_sub, reusing the row");
            }
            Err(e) => return Err(e.into()),
        }
        self.get_user_by_sub(sub)
            .await?
            .ok_or_else(|| anyhow::anyhow!("user row vanished right after ensure_user"))
    }

    /// handle で利用者の行を引く。 予約名 (account_type = reserved) の行も返す —
    /// 公開するかは呼び出し側が `account_type` で決める。
    pub async fn get_user_by_handle(&self, handle: &str) -> anyhow::Result<Option<UserEntry>> {
        let rows: Vec<Value> = self
            .db
            .query("SELECT * OMIT id FROM user WHERE handle = $handle LIMIT 1")
            .bind(("handle", handle.to_string()))
            .await?
            .take(0)?;
        rows_to_users(rows).map(|v| v.into_iter().next())
    }

    /// handle を claim する (ADR-023 D3、 早い者勝ち)。 handle は正規化済みの前提。
    /// 取られていれば `Ok(None)`。 同時に来た 2 人は handle の unique index が片方を弾く。
    /// 既に handle を持つ行には触らない (rename は次の段)。
    pub async fn claim_handle(
        &self,
        usr_id: &str,
        handle: &str,
    ) -> anyhow::Result<Option<UserEntry>> {
        if self.get_user_by_handle(handle).await?.is_some() {
            return Ok(None);
        }
        let updated = self
            .db
            .query(
                "UPDATE type::record('user', $usr_id) SET handle = $handle
                 WHERE handle = NONE AND account_type = 'user'",
            )
            .bind(("usr_id", usr_id.to_string()))
            .bind(("handle", handle.to_string()))
            .await?
            .check();
        match updated {
            // WHERE に合わなければ UPDATE は何もせず成功する。 自分の行を引き直して、
            // 要求した handle が本当に付いたときだけ返す (他人の行は決して返さない)
            Ok(_) => Ok(self
                .get_user_by_id(usr_id)
                .await?
                .filter(|u| u.handle.as_deref() == Some(handle))),
            Err(e) if is_unique_violation(&e) => {
                tracing::info!(handle, "claim_handle: lost the race on the unique index");
                Ok(None)
            }
            Err(e) => Err(e.into()),
        }
    }

    /// `usr_id` で利用者の行を引く。
    pub async fn get_user_by_id(&self, usr_id: &str) -> anyhow::Result<Option<UserEntry>> {
        let rows: Vec<Value> = self
            .db
            .query("SELECT * OMIT id FROM user WHERE usr_id = $usr_id LIMIT 1")
            .bind(("usr_id", usr_id.to_string()))
            .await?
            .take(0)?;
        rows_to_users(rows).map(|v| v.into_iter().next())
    }

    /// 呼び名を変える。 `None` で消す。
    pub async fn set_display_name(
        &self,
        usr_id: &str,
        display_name: Option<&str>,
    ) -> anyhow::Result<Option<UserEntry>> {
        self.db
            .query(
                "UPDATE type::record('user', $usr_id) SET display_name = $display_name
                 WHERE account_type = 'user'",
            )
            .bind(("usr_id", usr_id.to_string()))
            .bind(("display_name", display_name.map(str::to_string)))
            .await?
            .check()?;
        self.get_user_by_id(usr_id).await
    }

    pub async fn upsert_resource(&self, resource: &Resource) -> anyhow::Result<()> {
        let row = ResourceRow::from_resource(resource);
        self.db
            .query("UPSERT type::record('hub_resource', $rid) CONTENT $content")
            .bind(("rid", resource.id.clone()))
            .bind(("content", serde_json::to_value(&row)?))
            .await?
            .check()?;
        Ok(())
    }

    pub async fn delete_resource(&self, id: &str) -> anyhow::Result<()> {
        self.db
            .query("DELETE type::record('hub_resource', $rid)")
            .bind(("rid", id.to_string()))
            .await?
            .check()?;
        Ok(())
    }

    /// node registry: `vp-node` resource を upsert。 record id は **node_id keyed**
    /// (location 独立 routing key、 ADR-020 §S2/D2 — handle 改名・衝突で番地が壊れない)。
    /// node_id 無し (旧 client) は handle に fallback。 handle は display 属性、 endpoints は
    /// direct 到達候補 (`["[GUA]:port"]`) として payload 保持 (hub は opaque に扱う)。
    /// owner = 登録 principal の usr_id (ADR-020 §S5 — None は未認証 permissive 登録)、
    /// visibility は Discover の見せ方を決める (`list_nodes_visible_to` が対)。
    /// createdAt/updatedAt は DB 側 `time::now()` を string cast。 registered_at を返す。
    /// Unison `nodes.Register` の backing。 tree read (`/v1/tree/@handle`) にも即現れる。
    ///
    /// **owner guard (write-side、 ADR-020 §S5)**: UPSERT に `WHERE owner = NONE OR
    /// owner = NULL OR owner = $owner` を付け、 **他人が既に持つ node_id は上書きさせない**
    /// (owner/endpoints/visibility の乗っ取り防止 — 別 owner の rid は no-op = 空返り →
    /// Err)。 owner 無し (legacy/permissive) entry と自分の entry のみ書ける。 存在は漏らさない
    /// (mismatch も generic error)。
    pub async fn register_node(
        &self,
        node_id: Option<&str>,
        handle: &str,
        name: &str,
        endpoints: &[String],
        owner: Option<&str>,
        visibility: Visibility,
    ) -> anyhow::Result<String> {
        let rid = match node_id {
            Some(w) => format!("vp-node:{w}"),
            None => format!("vp-node:{handle}"),
        };
        let rows: Vec<Value> = self
            .db
            .query(
                "UPSERT type::record('hub_resource', $rid) CONTENT {
                    rid: $rid, type: 'vp-node', path: '/', handle: $handle,
                    owner: $owner,
                    visibility: $visibility,
                    payload: { name: $name, node_id: $node_id, endpoints: $endpoints },
                    createdAt: <string> time::now(), updatedAt: <string> time::now()
                }
                WHERE owner = NONE OR owner = NULL OR owner = $owner;",
            )
            .bind(("rid", rid))
            .bind(("handle", handle.to_string()))
            .bind(("name", name.to_string()))
            .bind(("node_id", node_id.map(str::to_string)))
            .bind(("endpoints", endpoints.to_vec()))
            .bind(("owner", owner.map(str::to_string)))
            .bind(("visibility", serde_json::to_value(visibility)?))
            .await?
            .take(0)?;
        // owner mismatch → UPSERT が既存 record を触らず空返り。 別 owner に取られている。
        match rows
            .first()
            .and_then(|r| r.get("createdAt"))
            .and_then(|v| v.as_str())
        {
            Some(at) => Ok(at.to_string()),
            None => anyhow::bail!("node_id is already registered by another owner"),
        }
    }

    /// node registry: `vp-node` resource を削除 (deregister)。 record id は
    /// [`Self::register_node`] と同じく **node_id keyed** (無ければ handle fallback) で
    /// 構成する (鏡像)。 存在しない rid の DELETE は no-op = idempotent。 `RETURN BEFORE`
    /// で削除前の行を受け、 **削除した entry 数** (0 or 1) を返す。 Unison `nodes.Unregister`
    /// の backing (test entry 掃除 + registry lifecycle、 ADR-020 §S2)。
    ///
    /// requester = 削除を求める principal の usr_id (ADR-020 §S5):
    /// - `Some(u)` → owner が u と一致するか、 owner 無し (legacy/未認証登録) の entry のみ
    ///   削除できる。 他人の node は消せない (owner mismatch は削除 0 = 存在も漏らさない)。
    /// - `None` (未認証 permissive、 または owner を持たない App principal) → **owner 無し
    ///   entry のみ** 削除できる。 認証済み user が owner を付けた node は、 未認証・App
    ///   どちらの経路からも消せない (無条件 DELETE は §S5 の isolation を破るため廃止)。
    ///
    /// どちらの分岐も owner を持つ他人の node には触れない。 owner 無し entry を誰でも
    /// 消せる点は permissive window の受容リスク (stale 掃除経路、 required 反転で縮小)。
    pub async fn unregister_node(
        &self,
        node_id: Option<&str>,
        handle: Option<&str>,
        requester: Option<&str>,
    ) -> anyhow::Result<usize> {
        let rid = match (node_id, handle) {
            (Some(w), _) => format!("vp-node:{w}"),
            (None, Some(h)) => format!("vp-node:{h}"),
            (None, None) => anyhow::bail!("node_id or handle required for unregister"),
        };
        // owner 無し entry は NONE (column 不在) と NULL (owner: None bind) の両形があり得る。
        let sql = match requester {
            Some(_) => {
                "DELETE type::record('hub_resource', $rid)
                 WHERE owner = NONE OR owner = NULL OR owner = $requester RETURN BEFORE"
            }
            // 未認証 / App principal は owner 無し entry のみ (他人の owned node は不可)。
            None => {
                "DELETE type::record('hub_resource', $rid)
                 WHERE owner = NONE OR owner = NULL RETURN BEFORE"
            }
        };
        let removed: Vec<Value> = self
            .db
            .query(sql)
            .bind(("rid", rid))
            .bind(("requester", requester.map(str::to_string)))
            .await?
            .take(0)?;
        Ok(removed.len())
    }

    /// 全 handle 横断で type 指定の resource を列挙 (discovery 用)。
    /// `get_resources_by_handle` と違い handle scope を要求しない。
    pub async fn list_resources_by_type(&self, rtype: &str) -> anyhow::Result<Vec<Resource>> {
        let rows: Vec<Value> = self
            .db
            .query("SELECT * FROM hub_resource WHERE type = $rtype ORDER BY createdAt")
            .bind(("rtype", rtype.to_string()))
            .await?
            .take(0)?;
        rows_to_resources(rows)
    }

    /// viewer に見える vp-node を列挙する (Discover の backing、 ADR-020 §S5)。
    ///
    /// - `Some(usr_id)` → 自分が owner の node + `public` な node。
    /// - `None` (未認証 permissive) → `public` のみ。
    ///
    /// legacy 行 (owner 無し) は register 時に `public` 固定だったため public 側に落ちて
    /// 従来通り見える (非破壊)。 `shared` は audience/group モデル導入までどちらにも
    /// 現れない (owner 本人を除く)。
    pub async fn list_nodes_visible_to(
        &self,
        viewer: Option<&str>,
    ) -> anyhow::Result<Vec<Resource>> {
        let sql = match viewer {
            Some(_) => {
                "SELECT * FROM hub_resource
                 WHERE type = 'vp-node' AND (visibility = 'public' OR owner = $viewer)
                 ORDER BY createdAt"
            }
            None => {
                "SELECT * FROM hub_resource
                 WHERE type = 'vp-node' AND visibility = 'public'
                 ORDER BY createdAt"
            }
        };
        let rows: Vec<Value> = self
            .db
            .query(sql)
            .bind(("viewer", viewer.map(str::to_string)))
            .await?
            .take(0)?;
        rows_to_resources(rows)
    }
}

/// SurrealDB の SELECT 結果 (Vec<Value>) を Resource へ。 record id `id` 等 extra key は無視。
fn rows_to_resources(rows: Vec<Value>) -> anyhow::Result<Vec<Resource>> {
    rows.into_iter()
        .map(|v| {
            serde_json::from_value::<ResourceRow>(v)
                .map(Resource::from)
                .map_err(anyhow::Error::from)
        })
        .collect()
}

/// `user` table の行 (column 名は spec の snake_case)。 予約名の行は usr_id / creo_sub が無い。
#[derive(Debug, Deserialize)]
struct UserRow {
    #[serde(default)]
    usr_id: Option<String>,
    #[serde(default)]
    handle: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    creo_sub: Option<String>,
    account_type: String,
}

fn rows_to_users(rows: Vec<Value>) -> anyhow::Result<Vec<UserEntry>> {
    rows.into_iter()
        .map(|v| {
            let r: UserRow = serde_json::from_value(v)?;
            Ok(UserEntry {
                // 予約名の行には usr_id が無い。 公開もしないので空で持つ
                usr_id: r.usr_id.unwrap_or_default(),
                handle: r.handle,
                display_name: r.display_name,
                creo_sub: r.creo_sub,
                account_type: r.account_type,
            })
        })
        .collect()
}

/// SurrealDB の unique index 違反 ("Database index `…` already contains …")。
fn is_unique_violation(e: &surrealdb::Error) -> bool {
    e.to_string().contains("already contains")
}

/// `usr_` EntId を振る (ADR-008)。 base58 の 16 文字 (約 94 bit)。
fn new_usr_id() -> String {
    use rand::RngCore;
    const ALPHABET: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    let body: String = bytes
        .iter()
        .map(|b| ALPHABET[(*b as usize) % ALPHABET.len()] as char)
        .collect();
    format!("usr_{body}")
}

/// options から WHERE 追加句 + LIMIT 文字列を組む。
fn filter_clause(options: &TreeReadOptions) -> (String, String) {
    let mut clause = String::new();
    if options.visibility.is_some() {
        clause.push_str(" AND visibility = $visibility");
    }
    if options.r#type.is_some() {
        clause.push_str(" AND type = $rtype");
    }
    let limit = match options.limit {
        Some(l) => format!(" LIMIT {l}"),
        None => String::new(),
    };
    (clause, limit)
}

fn add_filter_binds(binds: &mut Map<String, Value>, options: &TreeReadOptions) {
    if let Some(v) = options.visibility {
        binds.insert("visibility".into(), serde_json::to_value(v).unwrap());
    }
    if let Some(t) = &options.r#type {
        binds.insert("rtype".into(), json!(t));
    }
}
