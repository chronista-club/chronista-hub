# ADR-001: Monorepo strategy

- **Status**: Accepted
- **Date**: 2026-04-25

## Context

Chronista Hub の実装は spec (KDL) 単体で終わらず、 server 実装 + 多言語 client SDK (TypeScript / Rust / Ruby / 将来 Go / Python) + migration artifact を含む複合構造になる。 `world-tree.kdl` の `codegen { target ... }` では 4 つの emit-path が宣言されており、 この artifact 群を **1 repo に同居させる (monorepo)** か **複数 repo に分離する (poly-repo)** かを選ぶ必要がある。

Chronista の既存 project (`creo-memories`, `vantage-point`, `chronista-fleet`) はいずれも monorepo を採用しており、 エコシステム文化の一貫性も判断材料となる。

## Decision

**Monorepo** を採用。 `chronista-hub` 1 repo で spec + server + client SDK + codegen ツールを管理する。

構造:

```
chronista-hub/
  docs/spec/world-tree.kdl     # spec 原典
  docs/adr/                    # ADR (この directory)
  packages/chronista-hub-client/   # TS client SDK
  crates/chronista-hub-client/     # Rust client SDK
  lib/chronista_hub/               # Ruby client SDK
  apps/chronista-hub-server/       # Hub 本体 (Rust + SurrealDB)
  tools/codegen/                   # KDL → multi-target emitter
```

## Consequences

### 正

- spec の breaking change を同 PR で全 artifact に反映できる (version skew なし)
- Chronista エコシステムの既存 monorepo 文化と一貫、 認知負荷低
- cross-artifact refactoring が容易 (spec change → client types change → server impl change を 1 commit で)
- CI / codegen flow を 1 repo に閉じ込められる

### 負

- repo root が散らかりやすい (対策: `tools/`, `packages/`, `crates/`, `lib/`, `apps/` で階層区分)
- 言語固有の build system を複数抱える (cargo / bun / bundler) — 対策: root に薄い Makefile or justfile で橋渡し
- 外部 contributor にとっての onboarding cost が poly-repo より高い

## 却下案

### Poly-repo

- 却下理由: Phase 1 段階では breaking change を spec + impl で同 PR 反映できる便益が上回る。 外部 contributor はまだ想定ゼロ、 version skew 管理コストも低い。

### Hybrid (server 同居、 client 別 repo)

- 却下理由: 対 3rd-party 公開時には候補になるが、 現時点で client consumer は 1st party のみ想定。 premature separation。

## Future re-evaluation triggers

以下のいずれか満たしたら poly 化 (hybrid) を再検討:

1. client SDK を 3rd party 向けに OSS 独立配布する需要が出た
2. server 実装が内部機密化 (enterprise tier 等) で repo visibility 分離が必要になった
3. client SDK 言語が 6+ 言語に増えて monorepo root が散らかる

## References

- Memory: `mem_1CaP98FgH6GeM1Y8UQK3SE` (repo 戦略決定の議論と根拠)
- spec: `docs/spec/world-tree.kdl` の `codegen` block
- 前例: `creo-memories`, `vantage-point`, `chronista-fleet` (既存 monorepo)
