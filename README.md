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

## Development

```bash
bun install
bun run typecheck
bun run check
bun test
```

## Workspace layout (monorepo)

2026-04-25 監修で **monorepo 一本化** 戦略に統合。 server / spec / KDL codegen tool を 1 repo に同居させて maintenance burden を最小化。

```
chronista-hub/
├── apps/
│   └── chronista-hub-server/  (Hono + Bun backend)
├── packages/                   (KDL spec → multi-target codegen pipeline)
│   ├── kdl-parser/             (kdljs wrapper + typed AST)
│   ├── codegen-ts/             (AST → TypeScript interface)
│   ├── codegen-zod/            (AST → Zod schema + inferred type)
│   ├── codegen-surql/          (AST → SurrealQL DEFINE 文)
│   ├── codegen-rust/           (AST → Rust struct + serde)
│   └── cli/                    (unified `kdl-schema gen` command)
├── migrations/                 (SurrealQL schema migration、 codegen-surql 自動生成可能)
└── docs/
    └── spec/                   (KDL spec v0.1 + README)
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
