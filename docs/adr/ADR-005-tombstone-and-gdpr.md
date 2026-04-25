# ADR-005: Soft delete tombstone + GDPR purge

- **Status**: Accepted
- **Date**: 2026-04-25
- **Gap**: G4

## Context

v0.1 spec は `kind "resource.deleted"` event は持つが、 `resource-base` に `deleted_at` 等の tombstone field 無し。 削除後の挙動 (cascade / retain / purge) 未定義、 GDPR right-to-be-forgotten 未対応。

## Decision

**Soft delete (tombstone) + indefinite retention + explicit purge endpoint**:

- `resource-base.deleted_at` field 追加 (timestamp、 alive 時 NULL)
- default query で deleted を除外 (`?include_deleted=true` で復元可)
- purge は明示 endpoint `POST /v1/resources/{id}/purge` で 2-step confirm
- cascade は **retain-orphan** (Hub は cross-product cascade を強制しない、 product 自身が子 resource の delete event を個別 emit)
- stale ref 解決は **`410 Gone` + RFC 7807 body**
- GDPR purge: `POST /v1/users/{handle}/gdpr-purge` で 30 日 SLA broadcast、 user record は 180 日 tombstone 後 actual purge

event-schema 拡張: `deleted_at` / `deleted_reason` / `final_payload`、 `kind` enum に `resource.purged` / `user.gdpr_purge_requested` 追加。

## Consequences

### 正

- replay / audit / time-travel debugging 可能
- GDPR 法準拠 (art.17 + 30 日 SLA)
- product autonomy 保護 (Hub が cascade を独断しない)
- `410 Gone` で consumer が "存在しない" vs "削除された" を区別可、 error recovery が正しく書ける

### 負

- DB が tombstone を抱え続ける (purge endpoint で operator 制御)
- consumer は `?include_deleted=true` を意識する必要

## 却下案

- **Hard delete** — replay / audit 不能、 GDPR 困難
- **Auto-expire TTL** — predictable でない、 operator 視認性低
- **Cascade default** — cross-product boundary を Hub が決めるのは越権

## References

- Memory: `mem_1CaPCrdu2YNwbeAHSrg5sY`
- Gap list: `mem_1CaP3fUFz5DKW3DzJP1jGf`
- spec: `world-tree.kdl` `tombstone-policy`, `gdpr`, `resource-base.deleted_at`
- 関連 ADR: ADR-003 (RFC 7807 採用)、 ADR-004 (event sync replay 整合)
