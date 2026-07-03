//! Storage — `hub_resource` table への read/write。 TS `SurrealStorage` と振る舞い互換。
//!
//! record id = type::record('hub_resource', resource.id)。 SurrealDB の `id` は予約名なので
//! product 側 resource id は `rid` field に複製して保持する。

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::db::Db;
use crate::model::{AppManifest, Resource, Visibility};

/// REST の未認証 tree/resource read で **vp-world (federation registry) の非 public を
/// 隠す** guard (ADR-020 §S5)。 owner/visibility 分離は Unison `worlds.Discover` が
/// owner-aware に担うが、 同じ `vp-world` 行 (endpoints 入り) は auth 無しの REST
/// `/v1/tree/@handle` / `/v1/resources/{id}` からも読めてしまう。 REST は principal を
/// 持たない (全 caller 未認証扱い) ため owner 判定はできず、 **public のみ露出**して
/// private/shared world の endpoints 漏洩を塞ぐ。 product resource (type != vp-world) は
/// 一切影響を受けない。
const VP_WORLD_REST_GUARD: &str = " AND NOT (type = 'vp-world' AND visibility != 'public')";

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
            "SELECT * FROM hub_resource WHERE handle = $handle{VP_WORLD_REST_GUARD}{clause} ORDER BY createdAt{limit}"
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
            "SELECT * FROM hub_resource WHERE handle = $handle AND string::starts_with(path, $path){VP_WORLD_REST_GUARD}{clause} ORDER BY createdAt{limit}"
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
        // vp-world 非 public を REST の直接 id read から隠す (§S5、 VP_WORLD_REST_GUARD)。
        let sql =
            format!("SELECT * FROM hub_resource WHERE rid = $id{VP_WORLD_REST_GUARD} LIMIT 1");
        let rows: Vec<Value> = self
            .db
            .query(sql)
            .bind(("id", id.to_string()))
            .await?
            .take(0)?;
        Ok(rows_to_resources(rows)?.into_iter().next())
    }

    pub async fn get_app_manifest(&self, app_id: &str) -> anyhow::Result<Option<AppManifest>> {
        // manifest 専用 ingestion path は未実装 (ADR-009 Phase 2)。
        // 暫定: type='app' の resource を引いて payload を mapping。
        let rows: Vec<Value> = self
            .db
            .query("SELECT * FROM hub_resource WHERE rid = $id AND type = 'app' LIMIT 1")
            .bind(("id", app_id.to_string()))
            .await?
            .take(0)?;
        let resources = rows_to_resources(rows)?;
        let Some(row) = resources.into_iter().next() else {
            return Ok(None);
        };
        let p = &row.payload;
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

    /// world registry: `vp-world` resource を upsert。 record id は **wld_id keyed**
    /// (location 独立 routing key、 ADR-020 §S2/D2 — handle 改名・衝突で番地が壊れない)。
    /// wld_id 無し (旧 client) は handle に fallback。 handle は display 属性、 endpoints は
    /// direct 到達候補 (`["[GUA]:port"]`) として payload 保持 (hub は opaque に扱う)。
    /// owner = 登録 principal の usr_id (ADR-020 §S5 — None は未認証 permissive 登録)、
    /// visibility は Discover の見せ方を決める (`list_worlds_visible_to` が対)。
    /// createdAt/updatedAt は DB 側 `time::now()` を string cast。 registered_at を返す。
    /// Unison `worlds.Register` の backing。 tree read (`/v1/tree/@handle`) にも即現れる。
    ///
    /// **owner guard (write-side、 ADR-020 §S5)**: UPSERT に `WHERE owner = NONE OR
    /// owner = NULL OR owner = $owner` を付け、 **他人が既に持つ wld_id は上書きさせない**
    /// (owner/endpoints/visibility の乗っ取り防止 — 別 owner の rid は no-op = 空返り →
    /// Err)。 owner 無し (legacy/permissive) entry と自分の entry のみ書ける。 存在は漏らさない
    /// (mismatch も generic error)。
    pub async fn register_world(
        &self,
        wld_id: Option<&str>,
        handle: &str,
        name: &str,
        endpoints: &[String],
        owner: Option<&str>,
        visibility: Visibility,
    ) -> anyhow::Result<String> {
        let rid = match wld_id {
            Some(w) => format!("vp-world:{w}"),
            None => format!("vp-world:{handle}"),
        };
        let rows: Vec<Value> = self
            .db
            .query(
                "UPSERT type::record('hub_resource', $rid) CONTENT {
                    rid: $rid, type: 'vp-world', path: '/', handle: $handle,
                    owner: $owner,
                    visibility: $visibility,
                    payload: { name: $name, wld_id: $wld_id, endpoints: $endpoints },
                    createdAt: <string> time::now(), updatedAt: <string> time::now()
                }
                WHERE owner = NONE OR owner = NULL OR owner = $owner;",
            )
            .bind(("rid", rid))
            .bind(("handle", handle.to_string()))
            .bind(("name", name.to_string()))
            .bind(("wld_id", wld_id.map(str::to_string)))
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
            None => anyhow::bail!("wld_id is already registered by another owner"),
        }
    }

    /// world registry: `vp-world` resource を削除 (deregister)。 record id は
    /// [`Self::register_world`] と同じく **wld_id keyed** (無ければ handle fallback) で
    /// 構成する (鏡像)。 存在しない rid の DELETE は no-op = idempotent。 `RETURN BEFORE`
    /// で削除前の行を受け、 **削除した entry 数** (0 or 1) を返す。 Unison `worlds.Unregister`
    /// の backing (test entry 掃除 + registry lifecycle、 ADR-020 §S2)。
    ///
    /// requester = 削除を求める principal の usr_id (ADR-020 §S5):
    /// - `Some(u)` → owner が u と一致するか、 owner 無し (legacy/未認証登録) の entry のみ
    ///   削除できる。 他人の world は消せない (owner mismatch は削除 0 = 存在も漏らさない)。
    /// - `None` (未認証 permissive、 または owner を持たない App principal) → **owner 無し
    ///   entry のみ** 削除できる。 認証済み user が owner を付けた world は、 未認証・App
    ///   どちらの経路からも消せない (無条件 DELETE は §S5 の isolation を破るため廃止)。
    ///
    /// どちらの分岐も owner を持つ他人の world には触れない。 owner 無し entry を誰でも
    /// 消せる点は permissive window の受容リスク (stale 掃除経路、 required 反転で縮小)。
    pub async fn unregister_world(
        &self,
        wld_id: Option<&str>,
        handle: Option<&str>,
        requester: Option<&str>,
    ) -> anyhow::Result<usize> {
        let rid = match (wld_id, handle) {
            (Some(w), _) => format!("vp-world:{w}"),
            (None, Some(h)) => format!("vp-world:{h}"),
            (None, None) => anyhow::bail!("wld_id or handle required for unregister"),
        };
        // owner 無し entry は NONE (column 不在) と NULL (owner: None bind) の両形があり得る。
        let sql = match requester {
            Some(_) => {
                "DELETE type::record('hub_resource', $rid)
                 WHERE owner = NONE OR owner = NULL OR owner = $requester RETURN BEFORE"
            }
            // 未認証 / App principal は owner 無し entry のみ (他人の owned world は不可)。
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

    /// viewer に見える vp-world を列挙する (Discover の backing、 ADR-020 §S5)。
    ///
    /// - `Some(usr_id)` → 自分が owner の world + `public` な world。
    /// - `None` (未認証 permissive) → `public` のみ。
    ///
    /// legacy 行 (owner 無し) は register 時に `public` 固定だったため public 側に落ちて
    /// 従来通り見える (非破壊)。 `shared` は audience/group モデル導入までどちらにも
    /// 現れない (owner 本人を除く)。
    pub async fn list_worlds_visible_to(
        &self,
        viewer: Option<&str>,
    ) -> anyhow::Result<Vec<Resource>> {
        let sql = match viewer {
            Some(_) => {
                "SELECT * FROM hub_resource
                 WHERE type = 'vp-world' AND (visibility = 'public' OR owner = $viewer)
                 ORDER BY createdAt"
            }
            None => {
                "SELECT * FROM hub_resource
                 WHERE type = 'vp-world' AND visibility = 'public'
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
