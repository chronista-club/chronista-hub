# ADR-009: Product manifest schema (well-known JSON)

- **Status**: Accepted
- **Date**: 2026-04-25
- **Gap**: G6

## Context

v0.1 spec で各 product は `manifest-url "..."` を宣言するが、 URL 先の中身 (schema) が未定義。 product 登録 / 更新 / 廃止 flow も未定義。

## Decision

**JSON manifest at `.well-known/chronista-hub-manifest`** (RFC 8615 準拠、 OIDC discovery 同 idiom)。

required: `manifest_version`, `product_slug`, `name`, `home_url`, `api_endpoint`, `ingress_endpoint`, `webhook_receive_url`, `resource_types_emitted`, `scopes_requested`, `public_key_jwks_uri`

optional: `description`, `icon_url`, `health_endpoint`, `admin_contact`, `privacy_policy_url`, `terms_url`, `sdk_versions`, `capabilities`, `supported_regions`

### Registration flow

```
1. Dev prepares .well-known/chronista-hub-manifest
2. Dev: POST /v1/apps { manifest_url }
3. Hub fetches → JSON Schema validate
4. Admin reviews scopes_requested
5. Admin approves subset → product-token issued
6. Product publishes events with token
```

### Refresh policy (24h cycle)

- 表示系 (name/description/icon) → 即時反映
- `scopes_requested` 追加 → **要 re-approval** (pending state)
- `scopes_requested` 削除 → 即時 revoke (least privilege)
- endpoint 変更 → 即時反映 + 監査 log
- `public_key_jwks_uri` 変更 → key rotation 判定

### Deregistration

`POST /v1/apps/{app_id}/deregister` → 全 resource に `resource.deleted` (reason="product-deregister") → **90 日 tombstone** (ADR-005 と同 policy) → actual purge → token revoke

### Version compatibility

| Hub | Manifest | 挙動 |
|---|---|---|
| v0.2 | v0.2 | ✅ |
| v0.2 | v0.1 | ⚠️ default 補完 + sunset 警告 |
| v0.2 | v0.3 | ❌ required missing reject |

## Consequences

### 正

- OIDC / OAuth と整合する well-known pattern、 dev に親しい
- `scopes_requested` を spec化することで manifest update が scope 自動付与にならない (intent vs grant 分離)
- 24h refresh で staleness 起因の障害予防 (CREO-127 系統の "silent mismatch" 対策)
- deregistration の 90 日 tombstone は ADR-005 と policy 一致、 mental model 単純

### 負

- product 開発者は manifest を一級成果物として管理する責務 (scaffolding tool で軽減可能)
- 24h refresh の network cost (negligible、 ETag/If-Modified-Since で抑制)

## 却下案

- **inline manifest in product block** — product owner が Hub admin に PR 出さないと update 不可、 autonomy 欠落
- **GraphQL introspection 様の dynamic discovery** — 過剰、 well-known JSON で十分
- **automatic scope approval on manifest update** — least privilege 原則違反、 scope creep の温床

## References

- Memory: `mem_1CaQNJFh51ehZg4Jwwsk9C`
- Gap list: `mem_1CaP3fUFz5DKW3DzJP1jGf`
- spec: `world-tree.kdl` `product-manifest`, `app` resource-type
- 関連 ADR: ADR-005 (90 日 tombstone)、 ADR-006 (scope alignment)
- 外部規格: RFC 8615 well-known URI, OIDC discovery
