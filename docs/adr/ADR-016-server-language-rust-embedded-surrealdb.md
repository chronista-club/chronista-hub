# ADR-016: Hub server を Rust + embedded SurrealDB で実装する

- **Status**: Accepted
- **Date**: 2026-06-11
- **Supersedes**: scaffold 時 (AC-14) に暗黙採用された TypeScript/Bun 実装

## Context

Hub は World Tree の read API と event ingestion を提供する meta-registry。 全 Chronista
product と Creo ID が参照する hot path であり、 **レスポンスの低レイテンシ** が品質要件として重要。

設計の出発点 (ADR-001 / repo 戦略 memory `mem_1CaP98FgH6GeM1Y8UQK3SE`、 ともに 2026-04-24/25) は
`apps/chronista-hub-server/ # Hub 本体 (Rust + SurrealDB 想定)` と Rust を想定していた。
しかし Phase 1 scaffold (AC-14) では Hono + Bun (TypeScript) で実装され、 **その転換理由は
ADR にも memory にも記録されていない** (= ドキュメントと実装の drift)。

Phase 2-0 の永続化 (In-memory → SurrealDB) を TS で実装した時点 (`feat/persistence-surrealdb`、
commit `9351830`) で、 user が改めて **「低レイテンシを優先して Rust にしたい」** と判断 (2026-06-11)。
API surface が history / tree read / events / consumer / auth stub の最小構成である **今が移植コスト
最小** であることも後押しとなった。

## Decision

**Hub server を Rust で書き直す。**

- **HTTP**: `axum` 0.8 (+ `tower-http`)、 runtime は `tokio`
- **DB**: `surrealdb` crate 3.0 を **embedded (in-process) + `kv-rocksdb`** で使用
  - ネットワークホップ無し・別プロセス不要で最小レイテンシ (= 本 ADR の主目的)
  - migration は起動時に **in-process** で適用 (HTTP `/sql` 経由ではない)
  - 将来 remote / TiKV backend へ swap 可能 (`Surreal<Any>` 抽象)。 水平分散が要るまでは単一インスタンス前提
- **layout**: monorepo を polyglot 化
  - cargo workspace = `crates/*` (server、 将来の Rust client SDK)
  - bun workspace = `packages/*` (KDL codegen pipeline) のみに縮小
  - TS server (`apps/chronista-hub-server`) は撤去 (参照実装は `feat/persistence-surrealdb` branch に保存)

### 再利用する資産 (言語非依存)

- `docs/spec/world-tree.kdl` (KDL spec)
- `migrations/*.surql` (001-004。 SurrealQL は言語非依存)
- `packages/codegen-surql` / `codegen-rust` (spec → 生成)
- `docs/adr/*`

## Consequences

### 正
- read API / ingestion の低レイテンシ (embedded + Rust)
- ecosystem の Rust 資産 (club-unison QUIC、 fleetstage、 codegen-rust) と整合
- spec → `codegen-rust` で resource-type の Rust 型を生成でき、 dogfood ループが閉じる

### 負
- TS で書いた server 実装 (storage / event-log / pool / migrator / routes) は破棄 (参照実装として branch 保存)
- monorepo に cargo + bun の 2 build system が同居 (ADR-001 で既に想定済)
- embedded は単一インスタンス前提 — 水平分散が必要になったら remote 化が必要 (swap path は確保)

## 却下案

- **TS/Bun 継続** — 却下: 低レイテンシ目標に対し Rust が優位。 surface 最小の今が移植コスト最小で、
  TS のまま機能追加すると移植コストが増える。
- **Rust + remote ws SurrealDB** — 却下: 各 query にネットワーク往復が乗りレイテンシ目標に劣る。
  水平分散需要が出た時点で再検討 (embedded → remote は swap 可能)。
- **embedded + kv-surrealkv** — 保留: pure Rust で魅力的だが、 RocksDB の方が枯れている。 v0.x では rocksdb。

## References

- ADR-001 (monorepo strategy、 当初から Rust 想定だった)
- repo 戦略 memory `mem_1CaP98FgH6GeM1Y8UQK3SE`
- TS 参照実装: branch `feat/persistence-surrealdb` (commit `9351830`)
- 兄弟前例: `fleetstage` (axum 0.8 + surrealdb 3.0)、 `creo-elb`
