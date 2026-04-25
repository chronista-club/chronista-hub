# Spec CHANGELOG

> Format: [Keep a Changelog](https://keepachangelog.com/), versioning per [SemVer](https://semver.org) (with pre-1.0 minor-breaking allowance per ADR / G10).

## [Unreleased]

(none)

## [0.2.0] — 2026-04-25

Phase 1 着手前の self-review (gap G1-G14) を踏まえた enrichment。 spec の約束を明示化、 silent skip 排除、 Phase 1-3 scope 整理。

### Added

- **`spec-syntax`** block — `ref` / `enum-ref` の formal 構文化 (G7)
- **`spec-lint`** block — 8 rule (dangling refs, undefined types, version bump, etc) (G7, G10)
- **`spec-versioning`** block — SemVer policy, breaking-change classifier, enum extensibility, runtime version check (G10)
- **`identity-delegation`** block — Creo ID 委譲、 rename / reclaim / canonicalization policy (G1)
- **`handle-namespace`** block — shared user / org namespace + reserved handles (G12)
- **`api`** block — REST endpoint enumeration、 cursor pagination、 RFC 7807 errors、 rate limit、 cache (G2)
- **`api-response-conventions`** block — envelope shape + ref-expansion (G8)
- **`tombstone-policy`** block — soft delete、 retention、 purge endpoint、 cascade mode (G4)
- **`gdpr`** block — right-to-be-forgotten flow、 SLA 30 日 (G4)
- **`product-manifest`** block — JSON manifest schema、 registration / deregistration / refresh policy (G6)
- **`observability`** block — health / metrics / tracing / logging / rate-limit / SLA (G13)
- **`discovery`** block — feed / search / notification / follow (G14)
- **`phase-scoping`** block — feature を Phase 1/2/3 に明示 mapping (G12, G14)
- **`auth.scope-registry`** — dotted-notation OAuth-like scope (G5)
- **`auth.user-token` / `product-token` / `app-user-token`** — token type 整理 + refresh / revocation / audience list (G9)
- **resource-type `vp-actor`** — Stand Ensemble の正式宣言 (G7)
- **resource-type `organization`** — phase=2 (G12)
- **resource-type `org-membership`** — phase=2 (G12)
- **resource-type `notification`** — phase=3 (G14)
- **resource-type `follow`** — phase=3 (G14)
- **`resource-base.deleted_at`** field — tombstone 時点 (G4)
- **`resource-base.owner.key-field="usr_id"`** — EntId stable key (G8)
- **event-schema 追加 field**: `sequence_number`, `resource_id`, `prev_event_id`, `signature`, `signature_ts`, `replay`, `deleted_at`, `deleted_reason`, `final_payload`, `kind` enum extension (G3, G4)
- **`user.payload.account_type`** enum field (`user` / `organization` / `reserved`) (G12)
- **`user.subtree`** に system slug 追加: `_org-meta`, `_members`, `_orgs`, `_inbox`, `vp/actors` (G7, G12, G14)
- **`app.payload`** 拡張: `manifest_version`, `manifest_url`, `public_key_jwks_uri`, `status` enum (G6)
- **`codegen.spec-version-pin`** + 各 target の `spec-version-constant` (G10)
- **`codegen.target "zod"`** — runtime validation schema (G10)
- **`meta.changelog-url` / `migration-from-prev-url` / `archive-url`** — spec evolution dashboard (G10)
- archive snapshot: `docs/spec/archive/world-tree-v0.1.kdl` (immutable preservation)

### Changed

- **`resource-base.owner`** ref が `key-field="usr_id"` を明示 (G8)
- **`vp-world.payload.actors`** の `target="actor"` → `target="vp-actor"` (G7、 dangling ref 修正)
- **`user.subtree`** の `child "identity"` を **virtual subtree** に変更 (Hub は store せず Creo ID redirect、 G1, G7)
- **`fleetstage` / `gfp`** の product manifest に `events.delete.{type}` scope 追加 (v0.1 漏れ補完、 G5)
- **`creo-memories` / `vp` / `cplp` / `fleetstage` / `gfp`** の scope を **dotted notation** に refactor (`register:resource` → `events.publish.{type}`、 G5)
- **`gfp-shipment.status`** enum に `cancelled` value 追加 + extensible 化 (G5, G10)
- **`cplp-session.participants`** の ref に `key-field="usr_id"` 明示 (G8)
- **`event-schema.kind`** enum を extensible 化、 `resource.purged` / `user.gdpr_purge_requested` 追加 (G4)
- **enum 全体**: extensible default 化 (open enum convention、 G10)

### Removed

(none — v0.2 は additive のみ、 v0.1 → v0.2 は **non-breaking minor bump** 扱い)

### Migration

詳細: [migrations/0.1-to-0.2.md](./migrations/0.1-to-0.2.md)

## [0.1.0] — 2026-04-24

initial draft (AC-11 完了)、 creo-memories から chronista-hub へ migrate。

archive: `docs/spec/archive/world-tree-v0.1.kdl`
