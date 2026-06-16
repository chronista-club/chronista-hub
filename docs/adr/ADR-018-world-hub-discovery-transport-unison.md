# ADR-018: world↔hub discovery transport — Unison (QUIC) over REST

- **Status**: Accepted (v0.1.0 で実装・出荷)
- **Date**: 2026-06-16
- **Related**: #13 (MVP), #12 (federation, deferred), ADR-014 (discovery), ADR-016 (Rust/axum)

## Context

VP `world` が hub に自分を登録し、 互いを発見する (discovery) ための transport をどれにするか。

- 既存 hub は **axum / REST** (ADR-016)。 discovery endpoint を REST で手書きする選択肢があった。
- VP は内部通信に既に **Unison** (club-unison: QUIC + KDL schema + datagram) を採用 (MCP↔Process が "Unison QUIC primary")。
- 当初 federation (multi-hub) を MVP に想定したが、 運用上 hub は当分 1 つ (#12 で deferred)。 near-term の MVP は **単一 hub 内の相互 discovery**。

## Decision

world↔hub の discovery を **Unison (club-unison) channel** で提供する。 REST は ingestion (`/v1/events`) / tree read に据え置き、 **新しい discovery のみ** Unison surface に載せる薄い層とする。

### D1. transport = Unison (QUIC)、 REST 据え置き

- `worlds` channel: `Register` / `Discover` (RPC)。
- `unison.discovery` channel: server 自身の `protocol.kdl` を hash 付きで配信 (club-unison 組込、 `enable_discovery`)。
- axum (HTTP/TCP) と Unison (QUIC/UDP) を **同一 tokio runtime** で同居 (`spawn_listen`)。 addr は `CHRONISTA_HUB_UNISON_ADDR` (default `[::1]:7879`)。

### D2. registry は既存 resource モデルに載せる

- `worlds.Register` = `vp-world` resource を `hub_resource` に upsert (`storage::register_world`)。
- `worlds.Discover` = `type = 'vp-world'` を列挙 (`storage::list_resources_by_type`)。
- 帰結: Unison で登録 → REST `GET /v1/tree/@handle` でも読める (**cross-transport 一貫性**、 storage 層は 1 つ)。

### D3. rustls CryptoProvider を明示 install

- dependency graph に `aws-lc-rs` (surrealdb/reqwest) と `ring` (quinn/club-unison) が同居し、 rustls 0.23 が process-level default を auto-detect できず panic する。 main 冒頭で `rustls::crypto::ring::default_provider().install_default()` を呼ぶ。

### D4. build 要件 (system tools)

- `protoc` (club-unison の protobuf build)、 `clang` (surrealdb kv-rocksdb の `librocksdb-sys` bindgen)。 README + CI (ubuntu apt) に明記。

## Consequences

### 正

- VP native transport で world↔hub が native。 KDL schema 共有で codegen pipeline を再利用可。
- discovery が `unison.discovery` primitive に乗り **手書き不要**。 REST discovery endpoint の throwaway を回避。
- cross-transport 一貫性 (D2)。 datagram subscribe で将来 realtime discovery に拡張可能。
- federation (#12) に進む時、 この surface がそのまま土台 (per-hub local resolver) になる。

### 負

- 新依存 `club-unison` (v1.1.0 Beta) + QUIC/TLS stack (~25-30 transitive deps)。
- build に `protoc` / `clang` の system tool が必要 (環境差で CI が一度 fail、 ADR-018 D4 で固定)。
- `ProtocolClient::new_default()` は INSECURE (server 証明書未検証、 dev/test 専用)。 本番は TrustAnchors 明示が必要 (follow-up)。
- transport が 2 系統に分かれる (discovery = Unison、 ingestion/read = REST)。 全面 Unison 化は別 decision。

## 却下案

- **REST で discovery endpoint を手書き** (`/v1/resources?type=vp-world` 等) — VP native でない、 KDL 共有の利点なし、 `unison.discovery` を再発明、 federation 移行時に throwaway になる。
- **全面 Unison 化 (ingestion/read も移行)** — MVP に過剰。 ADR-016 の REST 群 migration は重く、 別 decision に切る。
- **multi-hub federation を MVP に** — 運用上 hub は当分 1 つ。 #12 に design record として deferred (~6-12mo)。 federation の ADR は後続番号で起こす。

## References

- 実装: `crates/chronista-hub-server/src/{unison_server.rs, hub_protocol.kdl}` / `storage::{register_world, list_resources_by_type}` / `main.rs` (provider install + spawn) / `config.rs` (`CHRONISTA_HUB_UNISON_ADDR`)
- GitHub: #13 (MVP)、 #12 (federation deferred)、 release `v0.1.0`
- 関連 ADR: ADR-014 (intra-hub discovery — 本 ADR が Unison で実装)、 ADR-016 (Rust/axum/embedded SurrealDB)、 ADR-007 (vp-actor / cross-product ref)
- memory: `mem_1Cc1dA79VZu586fjqafiBS` (統合 plan + 実装記録)、 `mem_1CaVeTysipdgVHoxwxUcPj` / `mem_1CaVeQEKJ...` (federation pair design 2026-04-28)
- club-unison: crates.io `club-unison = "1.1.0"` (lib `unison`)、 repo `github.com/chronista-club/club-unison`
