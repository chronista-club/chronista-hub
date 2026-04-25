# Architecture Decision Records

> Chronista Hub の設計判断を immutable な artifact として残すログ。

## Why ADR

spec 原典 (`docs/spec/world-tree.kdl`) は「何を作るか (what)」を宣言するが、ADR は「なぜその形にしたか (why)」を残す。 decision 当時の trade-off、 却下案、 前提条件を時系列で辿れるようにして、 将来の reviewer / contributor が context なしに spec の意図を再構築できるようにする。

## Format

Michael Nygard 流の簡略版を採用:

```markdown
# ADR-NNN: <Short Title>

- **Status**: Proposed | Accepted | Superseded by ADR-XXX | Deprecated
- **Date**: YYYY-MM-DD
- **Context**: 何が問題だったか (< 10 行)
- **Decision**: 何を決めたか (結論)
- **Consequences**: 採用による正負の影響
- **References**: 関連 ADR / spec 箇所 / memory ID
```

## Rules

- **Immutable**: 承認された ADR は編集しない。変更したい場合は新 ADR で `Supersedes` する
- **Short**: 1 ADR = 1 decision、 200-500 行以内が目安
- **Linked**: spec 上の該当箇所 + creo-memories の詳細 memory ID を `References` に書く
- **Numbered**: `ADR-NNN-kebab-case-title.md`、 3 桁連番

## Relationship to memories

| レイヤー | 役割 | 場所 |
|---|---|---|
| spec (`world-tree.kdl`) | **what** — 宣言的な型 / API | repo committed |
| ADR (`docs/adr/*`) | **why** — 決定の根拠と却下案 | repo committed |
| memory (creo-memories) | **how / detail** — 議論過程 / drill-down / cross-ref | creo-memories atlas |

ADR は spec ↔ memory の橋。 spec を読んで "なぜ?" と思ったら ADR で short answer を、 さらに深く知りたければ memory で full context を追う。

## Index

| # | Title | Status | Date | Related |
|---|---|---|---|---|
| [ADR-001](./ADR-001-monorepo-strategy.md) | Monorepo strategy | Accepted | 2026-04-25 | — |
| [ADR-002](./ADR-002-creo-id-identity-delegation.md) | Identity delegation to Creo ID (G1) | Accepted | 2026-04-25 | gap G1, G8 |
| [ADR-003](./ADR-003-api-surface-rest-cursor.md) | REST API surface with cursor pagination (G2) | Accepted (target, MVP subset landed in AC-15) | 2026-04-25 | gap G2 |
| [ADR-004](./ADR-004-event-sync-reliability.md) | Event sync reliability (G3) | Accepted | 2026-04-25 | gap G3 |
| [ADR-005](./ADR-005-tombstone-and-gdpr.md) | Soft delete tombstone + GDPR purge (G4) | Accepted | 2026-04-25 | gap G4 |
| [ADR-006](./ADR-006-scope-granularity.md) | Scope granularity — dotted OAuth notation (G5) | Accepted | 2026-04-25 | gap G5 |
| [ADR-007](./ADR-007-dangling-refs-and-vp-actor.md) | Dangling refs resolution + vp-actor (G7) | Accepted | 2026-04-25 | gap G7 |
| [ADR-008](./ADR-008-owner-primary-key-usrid.md) | Owner primary key — `usr_id` (EntId) + handle display (G8) | Accepted | 2026-04-25 | gap G1, G8 |
| [ADR-009](./ADR-009-product-manifest-schema.md) | Product manifest schema (well-known JSON) (G6) | Accepted | 2026-04-25 | gap G6 |
| [ADR-010](./ADR-010-auth-tokens-refresh-revocation.md) | Auth — token refresh, revocation, audience (G9) | Accepted | 2026-04-25 | gap G9 |
| [ADR-011](./ADR-011-spec-versioning-semver.md) | Spec versioning — SemVer with pre-1.0 minor-breaking (G10) | Accepted | 2026-04-25 | gap G10 |
| [ADR-012](./ADR-012-path-canonical-and-subdomain-reserve.md) | Path-based canonical URLs + subdomain reservation (G11) | Accepted | 2026-04-25 | gap G11 |
| [ADR-013](./ADR-013-organization-shared-namespace.md) | Organization / team — shared `@handle` namespace (G12) | Accepted (Phase 1 reserve, Phase 2 impl) | 2026-04-25 | gap G12 |
| [ADR-014](./ADR-014-observability-and-discovery.md) | Observability + Discovery (Phase 1-3 graded) (G13, G14) | Accepted | 2026-04-25 | gap G13, G14 |
| [ADR-015](./ADR-015-ac-15-mvp-api-shape.md) | AC-15 MVP API shape — current state codification | Accepted (Phase 1 current state) | 2026-04-26 | ADR-003 相補 |
