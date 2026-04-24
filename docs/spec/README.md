# Chronista Hub — Spec

> [Linear Epic AC-10](https://linear.app/chronista/issue/AC-10) / Phase 0 は [AC-11](https://linear.app/chronista/issue/AC-11)

## 何

Chronista Hub (`chronista.club` 上の新 product) の **World Tree** — identity + stateful resource meta-registry — を KDL で spec 化したもの。 Creo ID / 各 Chronista product (Memories / VP / CPLP / FleetStage / GFP) が共通で参照する single source of truth。

## なぜ KDL

- **host-language DSL 勝利** (memory: `config-dsl-pattern-learnings`)
- **data-code 分離** (spec は data、 実装は別 language)
- 既存 Chronista 使用例: `fleet.kdl`, `worker-files.kdl`
- JSON 依存なし (nested block で payload shape 表現)
- codegen ready (TS / Rust / Ruby / SurrealQL へ emit)

## ファイル

| file | 内容 |
|---|---|
| `world-tree.kdl` | meta / path schema / resource shape / sync / auth / products / resource-types / codegen hints |
| `resources/` (将来) | 各 resource-type が成長したら分割先 |

## 主要決定 (memory 原典: `chronista-hub-ownership.md`)

- **brand**: Chronista Hub
- **domain**: `chronista.club` (Creo ID は `id.creo-memories.in` で auth 専任)
- **URL path**: `/@{handle}/{product-slug}/{resource-type}/{id}`
- **reserved slugs**: `identity` / `apps` / `world` / `.well-known`
- **ownership model**: index-only (products DB が primary、 Hub は navigation cache)
- **sync**: event-sourced (products publish events)
- **spec format**: KDL (schema) + Ruby DSL (Ops、 後 phase)

## Architecture (要点)

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

## Phase roadmap (AC-10)

| Phase | 内容 | Linear |
|---|---|---|
| 0 | KDL spec (本 artifact) | [AC-11](https://linear.app/chronista/issue/AC-11) |
| 1 | Core registry MVP + Memories pilot | [AC-12](https://linear.app/chronista/issue/AC-12) |
| 2 | Pilot pair (Memories + VP dual) | 起票予定 |
| 3 | End user dashboard (`@handle/`) | 起票予定 |
| 4 | Universal public URL | 起票予定 |
| 5 | 3rd party SDK | 起票予定 |

## Spec 原則

1. **KDL native** — JSON Schema 参照無し、 payload は nested `field` block
2. **Versioned** — `meta { version "..." }` + API path `/v1/`
3. **Codegen ready** — 各 target へ emit hint を spec に含める
4. **Evolvable** — field 追加は backwards compat、 breaking 変更は version bump

## 関連 memory (creo-memories 側)

- `chronista-hub-ownership.md` — decision 原典
- `worker-lane-architecture-decision.md` — parallel axis の architecture
- `config-dsl-pattern-learnings.md` — DSL 選定理論
- `creo-ops-language-ruby.md` — Ruby DSL の住処 (Ops layer)
- `creo-id-identity-principle.md` — Creo ID 側 foundation (Email = Identity SSOT)

## 関連 Linear

- Parent Epic: [AC-10](https://linear.app/chronista/issue/AC-10) Epic: Chronista Hub — World Tree MVP service
- 依存: [CREO-93](https://linear.app/chronista/issue/CREO-93) Creo ID 独立認証サーバ化 (auth 専任)
- 関連 axis: [VP-85](https://linear.app/chronista/issue/VP-85) Epic: VP Lanes (並行 worker orchestration)
- 関連 axis: [CREO-117](https://linear.app/chronista/issue/CREO-117) ccws port allocator
