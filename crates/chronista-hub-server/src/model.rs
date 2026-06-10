//! World Tree の中核型。 TS 参照実装 (`storage.ts` / `event-log.ts`) と JSON 互換。
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

/// World Tree 上の 1 resource。 hub_resource table と 1:1 (record id とは別に `rid` を持つ)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    pub id: String,
    #[serde(rename = "type")]
    pub r#type: String,
    pub path: String,
    pub handle: String,
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
