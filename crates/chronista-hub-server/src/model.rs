//! Node Tree の中核型。 TS 参照実装 (`storage.ts` / `event-log.ts`) と JSON 互換。
//!
//! API / event の JSON field 名は camelCase (`createdAt` 等) を維持する。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    Public,
    Shared,
    Private,
}

/// Node Tree 上の 1 resource。 hub_resource table と 1:1 (record id とは別に `rid` を持つ)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    pub id: String,
    #[serde(rename = "type")]
    pub r#type: String,
    pub path: String,
    pub handle: String,
    /// 所有者 usr_id (EntId、 spec `resource-base.owner`)。 None = owner 不明 (legacy /
    /// 未認証登録)。 書き込み元は principal 由来の federation 経路のみ — ingestion
    /// (`/v1/events`) には配線しない (app が任意 usr_id を名乗る spoofing 面を開かない)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    pub visibility: Visibility,
    pub payload: serde_json::Value,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppManifest {
    #[serde(rename = "appId")]
    pub app_id: String,
    pub name: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permissions: Option<Vec<String>>,
}

/// アプリの名簿での状態 (spec `resource-type "app"` の `status`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppStatus {
    /// manifest 取得済、 admin review 待ち
    Pending,
    Active,
    /// deregister 進行中
    Deregistering,
    /// 完全削除済
    Deregistered,
}

/// アプリの名簿の 1 行 (`app` table、 ADR-009 の段階 1)。 API には camelCase で出す。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppEntry {
    pub app_id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home_url: Option<String>,
    /// ログインを始める URL。 `/start` の link 先。 None = CLI でログインするアプリ
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub login_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest_url: Option<String>,
    pub status: AppStatus,
}

/// 利用者の名簿の 1 行 (`user` table、 ADR-023 D3)。 本人向け (`/v1/me`) の形。
///
/// 裁定 2026-10-09: 行は初回接触で作り、 handle は「必要になったときに claim する」。
/// だから `handle` は claim までは `None`。 鍵はいつでも `usr_id`。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserEntry {
    /// Hub が振る EntId (`usr_…`、 ADR-008 の stable key)
    pub usr_id: String,
    /// 住所 (unique)。 claim するまで無い
    pub handle: Option<String>,
    /// 呼び名。 自由文字列で一意ではない
    pub display_name: Option<String>,
    /// Creo ID の `sub`。 公開の形 (`PublicUser`) には出さない
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creo_sub: Option<String>,
    /// 最後に rename した時刻 (ISO 8601)。 30 日の cooldown の起点。 claim では付かない
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle_changed_at: Option<String>,
    pub account_type: String,
}

/// rename で取り置かれた旧 handle (裁定 2026-10-12: 100 日、 本人だけが取り戻せる)。
/// DB の行そのまま (snake_case)。 API に出す形は `me_json` が組む
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandleReserve {
    pub handle: String,
    pub usr_id: String,
    pub new_handle: String,
    /// ISO 8601。 これを過ぎたら誰でも claim できる
    pub expires_at: String,
}

/// rename の結果。
#[derive(Debug)]
pub enum RenameOutcome {
    Renamed(UserEntry),
    /// 使用中、 予約名、 他人の取り置き
    Taken,
    /// 30 日の cooldown の中。 `until` はいつから変えられるか (ISO 8601)
    Cooldown {
        until: String,
    },
}

impl UserEntry {
    /// `/@{handle}` (ADR-012 の canonical path)。 handle が無ければ無い
    pub fn canonical_path(&self) -> Option<String> {
        self.handle.as_ref().map(|h| format!("/@{h}"))
    }

    /// 公開の形。 handle を claim した利用者と団体だけが公開の住所を持つ。
    /// 予約名 (account_type = reserved) は名義ではないので無い
    pub fn public(&self) -> Option<PublicUser> {
        if !self.is_publisher_kind() {
            return None;
        }
        let handle = self.handle.clone()?;
        Some(PublicUser {
            usr_id: self.usr_id.clone(),
            canonical_path: format!("/@{handle}"),
            handle,
            display_name: self.display_name.clone(),
            account_type: self.account_type.clone(),
        })
    }

    /// 名義になれる種類か (人 or 団体)。 予約名は違う
    pub fn is_publisher_kind(&self) -> bool {
        self.account_type == "user" || self.account_type == "organization"
    }

    pub fn is_organization(&self) -> bool {
        self.account_type == "organization"
    }
}

/// 誰にでも見せてよい利用者の形 (`GET /v1/users/@{handle}`)。 ADR-008 の 2 軸併載。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicUser {
    pub usr_id: String,
    pub handle: String,
    pub display_name: Option<String>,
    pub canonical_path: String,
    /// `user` (人) か `organization` (団体、 ADR-013 / ADR-024)
    pub account_type: String,
}

/// 団体での役割 (ADR-013)。 段 1 で使うのは owner だけ。
/// 名義として公開物を出せるのは guest 以外
pub fn role_can_publish(role: &str) -> bool {
    matches!(role, "owner" | "admin" | "member")
}

/// 読める公開 URL の索引の 1 行 (`/@{handle}/{slug}` → アプリの公開ページ、 ADR-024)。
/// DB の行そのまま (snake_case)。 API に出す形は handler が組む
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Publication {
    pub publisher_usr_id: String,
    pub slug: String,
    pub url: String,
    #[serde(default)]
    pub title: Option<String>,
    pub app_id: String,
    #[serde(default)]
    pub updated_at: Option<String>,
}

/// `/@{handle}/{slug}` に使えない slug。 spec の reserved-path-slug と、 handle の下の
/// 予約の子 (`identity`、 node-tree.kdl の user subtree) と同じ並び。 アプリの app_id は
/// handler が名簿を見て弾く (`/@{handle}/{product-slug}/…` の木と混ざらないように)
pub const RESERVED_PUBLICATION_SLUGS: &[&str] = &[
    "identity", "apps", "node", "world", "api", "oauth", "errors", "static", "health",
];

/// 公開物の slug を正規化する。 小文字英数とハイフン、 先頭は英数、 63 文字まで
/// (Creo Memories の atlas slug と同じ幅)。 予約語は None
pub fn canonical_slug(input: &str) -> Option<String> {
    let s = input.trim().to_ascii_lowercase();
    let mut bytes = s.bytes();
    let first_ok = bytes
        .next()
        .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    let rest_ok = bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    (first_ok && rest_ok && s.len() <= 63 && !RESERVED_PUBLICATION_SLUGS.contains(&s.as_str()))
        .then_some(s)
}

/// handle の形 (spec `handle-pattern`、 ADR-002 / 012): `^[a-z0-9][a-z0-9-]{0,30}$`。
/// 大文字は小文字に正規化してから見る (lowercase canonical)。
pub fn canonical_handle(input: &str) -> Option<String> {
    let h = input.trim().to_ascii_lowercase();
    let mut chars = h.bytes();
    let first_ok = chars
        .next()
        .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    let rest_ok = chars.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    (first_ok && rest_ok && h.len() <= 31).then_some(h)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventKind {
    #[serde(rename = "resource.created")]
    ResourceCreated,
    #[serde(rename = "resource.updated")]
    ResourceUpdated,
    #[serde(rename = "resource.deleted")]
    ResourceDeleted,
}

/// product → Hub の event envelope。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub event_id: String,
    pub app_id: String,
    pub kind: EventKind,
    pub resource: Resource,
    pub idempotency: String,
    pub emitted_at: String,
}

/// EventLog に格納された envelope (受信時刻 + 処理時刻つき)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredEvent {
    #[serde(flatten)]
    pub envelope: EventEnvelope,
    /// append された時刻 (unix ms、 insert 順の proxy)
    pub received_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub processed_at: Option<i64>,
}

// ---------------------------------------------------------------------------
// Envelope validation — JSON Value から検証して field-level error を返す
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct ValidationError {
    pub field: String,
    pub message: String,
}

fn require_str(
    obj: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    errors: &mut Vec<ValidationError>,
) -> Option<String> {
    match obj.get(key).and_then(|v| v.as_str()) {
        Some(s) => Some(s.to_string()),
        None => {
            errors.push(ValidationError {
                field: key.into(),
                message: "required string".into(),
            });
            None
        }
    }
}

/// JSON を検証して EventEnvelope を返す。 不正なら field-level error の Vec。
pub fn validate_envelope(input: &serde_json::Value) -> Result<EventEnvelope, Vec<ValidationError>> {
    let mut errors: Vec<ValidationError> = Vec::new();
    let Some(obj) = input.as_object() else {
        return Err(vec![ValidationError {
            field: String::new(),
            message: "must be object".into(),
        }]);
    };

    let event_id = require_str(obj, "event_id", &mut errors);
    let app_id = require_str(obj, "app_id", &mut errors);
    let idempotency = require_str(obj, "idempotency", &mut errors);
    let emitted_at = require_str(obj, "emitted_at", &mut errors);

    let kind = match obj.get("kind").and_then(|v| v.as_str()) {
        Some("resource.created") => Some(EventKind::ResourceCreated),
        Some("resource.updated") => Some(EventKind::ResourceUpdated),
        Some("resource.deleted") => Some(EventKind::ResourceDeleted),
        _ => {
            errors.push(ValidationError {
                field: "kind".into(),
                message: "must be resource.created | resource.updated | resource.deleted".into(),
            });
            None
        }
    };

    let resource = match obj.get("resource") {
        Some(r) => validate_resource(r, &mut errors),
        None => {
            errors.push(ValidationError {
                field: "resource".into(),
                message: "required object".into(),
            });
            None
        }
    };

    if errors.is_empty() {
        Ok(EventEnvelope {
            event_id: event_id.unwrap(),
            app_id: app_id.unwrap(),
            kind: kind.unwrap(),
            resource: resource.unwrap(),
            idempotency: idempotency.unwrap(),
            emitted_at: emitted_at.unwrap(),
        })
    } else {
        Err(errors)
    }
}

fn validate_resource(
    input: &serde_json::Value,
    errors: &mut Vec<ValidationError>,
) -> Option<Resource> {
    let Some(obj) = input.as_object() else {
        errors.push(ValidationError {
            field: "resource".into(),
            message: "must be object".into(),
        });
        return None;
    };
    let mut local: Vec<ValidationError> = Vec::new();
    let id = require_str(obj, "id", &mut local);
    let r#type = require_str(obj, "type", &mut local);
    let path = require_str(obj, "path", &mut local);
    let handle = require_str(obj, "handle", &mut local);

    let visibility = match obj.get("visibility").and_then(|v| v.as_str()) {
        Some("public") => Some(Visibility::Public),
        Some("shared") => Some(Visibility::Shared),
        Some("private") => Some(Visibility::Private),
        _ => {
            local.push(ValidationError {
                field: "visibility".into(),
                message: "must be public | shared | private".into(),
            });
            None
        }
    };

    let payload = match obj.get("payload") {
        Some(v) if v.is_object() => v.clone(),
        _ => {
            local.push(ValidationError {
                field: "payload".into(),
                message: "must be object".into(),
            });
            serde_json::Value::Null
        }
    };
    let created_at = require_str(obj, "createdAt", &mut local);
    let updated_at = require_str(obj, "updatedAt", &mut local);

    if local.is_empty() {
        Some(Resource {
            id: id.unwrap(),
            r#type: r#type.unwrap(),
            path: path.unwrap(),
            handle: handle.unwrap(),
            // ingestion 経路では owner を受け付けない (federation 経路のみが principal 由来で設定)
            owner: None,
            visibility: visibility.unwrap(),
            payload,
            created_at: created_at.unwrap(),
            updated_at: updated_at.unwrap(),
        })
    } else {
        // resource.<field> に prefix して親へ移す
        for e in local {
            errors.push(ValidationError {
                field: format!("resource.{}", e.field),
                message: e.message,
            });
        }
        None
    }
}
