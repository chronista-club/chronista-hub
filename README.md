# Chronista Hub

World Tree meta-registry for the Chronista ecosystem.

## 何

`chronista.club` 上に展開する **identity + stateful resource meta-registry**。 Creo ID / 各 Chronista product (Memories / VP / CPLP / FleetStage / GFP) の state を navigable な tree として統合し、 universal URL namespace (`chronista.club/@{handle}/...`) を提供する。

## Spec

World Tree v0.1 KDL spec — [`docs/spec/world-tree.kdl`](./docs/spec/world-tree.kdl)

詳細は [`docs/spec/README.md`](./docs/spec/README.md) 参照。

## Architecture

```
┌────────────────────────────────────────┐
│  chronista.club (Chronista Hub)        │
│  ├── /@{handle}/                        │
│  ├── /apps/{app_id}/                    │
│  ├── /world/                            │
│  └── /.well-known/                      │
└───────┬────────────────┬────────────────┘
        │ JWT verify     │ SDK (register event)
        ▼                ▼
id.creo-memories.in     各 product
(Creo ID: auth only)    (state publishers)
```

## Status

- **Phase 0** — KDL spec v0.1 drafted ([AC-11](https://linear.app/chronista/issue/AC-11) Done、 PR #1 で spec 移送)
- **Phase 1-0** — Repo baseline scaffold (本 commit)
- **Phase 1-1** — Core registry MVP backend ([AC-14](https://linear.app/chronista/issue/AC-14))
- **Phase 1-2** — Tree read API v1 ([AC-15](https://linear.app/chronista/issue/AC-15))
- **Phase 1-3** — Event-sourced ingestion ([AC-16](https://linear.app/chronista/issue/AC-16))
- **Phase 1-4** — Auth middleware (Creo ID JWKS) ([AC-17](https://linear.app/chronista/issue/AC-17))
- **Phase 1-5** — Memories hub-sync (pilot) ([AC-18](https://linear.app/chronista/issue/AC-18))
- **Phase 2+** — Pilot pair / End user dashboard / Universal public URL / 3rd party SDK

## Server (Rust)

Hub server は **Rust + axum + embedded SurrealDB (kv-rocksdb)** で実装 (ADR-016、 低レイテンシ重視)。
DB は別プロセス不要の in-process embedded。 起動時 `AUTO_MIGRATE_ENABLED=true` で `migrations/*.surql` を
listen 前に適用する。

```bash
# build / lint / test (SurrealDB は in-process なので別プロセス不要)
cargo build
cargo clippy --all-targets
cargo test

# 起動 (本番想定: Creo ID JWKS で user-jwt を実検証)
AUTO_MIGRATE_ENABLED=true CHRONISTA_HUB_DB_PATH=./data/hub.rocksdb \
  cargo run -p chronista-hub-server

# dev 起動 (無署名 StubVerifier。 JWKS fetch を skip)
STUB_AUTH_ALLOWED=true AUTO_MIGRATE_ENABLED=true CHRONISTA_HUB_DB_PATH=./data/hub.rocksdb \
  cargo run -p chronista-hub-server

# 永続化 e2e (publish → consumer → tree read → 再起動後も残存)
bash scripts/e2e.sh
```

env (DB/migration): `CHRONISTA_HUB_PORT` (default 3000) / `SURREALDB_NAMESPACE` (default `chronista`) /
`SURREALDB_DATABASE` (default `hub`) / `CHRONISTA_HUB_DB_PATH` (RocksDB dir) /
`AUTO_MIGRATE_ENABLED` / `MIGRATIONS_DIR` (default `./migrations`)。

## Auth (ADR-002/010)

- **user-jwt**: Creo ID (OIDC) 発行を **JWKS で RS256 + iss + aud(list) + exp** 実検証 (`sub`→`usr_id`)。
- **product-token** (ingestion、 ADR-017): Hub 発行の opaque token (`cht_...`)。 DB には SHA-256 hash
  のみ保存、 平文は発行時一度きり。 検証は in-process DB lookup、 即時 revoke 可。
  `X-App-Token: cht_...` で events を publish する。
- **管理 API** (`HUB_ADMIN_KEY` 設定時のみ有効、 `X-Admin-Key` header):
  `POST /v1/apps/{id}/tokens` (発行) / `POST .../tokens/rotate` (30 日 overlap rotation) /
  `DELETE .../tokens/{token_id}` (即時 revoke) / `GET .../tokens` (一覧)。 未設定なら全て 404。
- **JWKS refresh**: 5 分毎 (`JWKS_REFRESH_SECS`) に background refetch。 失敗時は旧 keys 維持
  — Creo ID の鍵 rotation に再起動なしで追従。
- **dev**: `STUB_AUTH_ALLOWED=true` で無署名 StubVerifier。 `STUB_APP_TOKEN_ALLOWED=true` で
  JWKS mode でも無署名 app-token (`app:<id>:<scopes>`) を受理 (どちらも default false = fail-closed)。

env (auth): `CREO_ID_ISSUER` (default `https://id.creo-memories.in/`) /
`CREO_ID_JWKS_URL` (default `{issuer}.well-known/jwks.json`) /
`CREO_ID_AUDIENCES` (comma 区切り、 default `chronista-hub`) /
`HUB_ADMIN_KEY` (unset = 管理 API 無効) / `JWKS_REFRESH_SECS` (default 300) /
`STUB_AUTH_ALLOWED` (default false) / `STUB_APP_TOKEN_ALLOWED` (default false)。

> TS/Bun 版 (旧実装) は branch `feat/persistence-surrealdb` に参照保存。

## Codegen / spec (Bun)

```bash
bun install
bun test          # codegen pipeline の unit tests
bun run gen:surql # spec → migrations/002_resource_types_from_spec.surql
```

## Workspace layout (monorepo)

**monorepo 一本化** + **polyglot** (cargo + bun)。 server (Rust) / spec / KDL codegen tool (TS) を 1 repo に同居。

```
chronista-hub/
├── Cargo.toml                  (cargo workspace = crates/*)
├── crates/
│   └── chronista-hub-server/   (Rust: axum + embedded SurrealDB — ADR-016)
├── package.json                (bun workspace = packages/*)
├── packages/                   (KDL spec → multi-target codegen pipeline)
│   ├── kdl-parser/             (kdljs wrapper + typed AST)
│   ├── codegen-ts/             (AST → TypeScript interface)
│   ├── codegen-zod/            (AST → Zod schema + inferred type)
│   ├── codegen-surql/          (AST → SurrealQL DEFINE 文)
│   ├── codegen-rust/           (AST → Rust struct + serde)
│   └── cli/                    (unified `kdl-schema gen` command)
├── migrations/                 (SurrealQL schema migration、 codegen-surql 自動生成可能)
├── scripts/e2e.sh              (binary e2e: 永続化検証)
└── docs/
    ├── spec/                   (KDL spec v0.2 + README)
    └── adr/                    (ADR-001..016)
```

## Codegen scripts

`docs/spec/world-tree.kdl` を SSOT として 4 言語/層を自動生成:

```bash
bun run gen        # 全 4 target を generated/ に
bun run gen:ts     # TypeScript interface → apps/chronista-hub-server/src/generated/
bun run gen:zod    # Zod schema (runtime validation) → apps/chronista-hub-server/src/generated/
bun run gen:surql  # SurrealQL DEFINE 文 → migrations/002_*.surql
```

CLI 単独実行も可能: `bun packages/cli/bin/kdl-schema.ts gen <input.kdl> --target <ts|zod|surql|rust|all> ...`

## Related

- Linear Epic: [AC-10](https://linear.app/chronista/issue/AC-10) Chronista Hub — World Tree MVP service
- Dependency: [`chronista-club/creo-id`](https://github.com/chronista-club/creo-id) — auth server (JWKS 提供元、 別 repo 維持)
- Past separate repo: [`chronista-club/kdl-schema`](https://github.com/chronista-club/kdl-schema) — 2026-04-25 に本 repo へ absorb (archive 予定)

## License

TBD
