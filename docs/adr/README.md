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
