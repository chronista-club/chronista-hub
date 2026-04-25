# ADR-006: Scope granularity — dotted OAuth notation

- **Status**: Accepted
- **Date**: 2026-04-25
- **Gap**: G5

## Context

v0.1 spec の scope は `register:resource` / `delete:resource` / `read:public` の 3 種のみ、 all-or-nothing で粒度なし。 product 間で scope 付与が偏り (`creo-memories` / `vp` は register+delete、 `cplp` / `fleetstage` / `gfp` は register のみ) その意図が spec に書かれていない。

## Decision

**dotted notation (OAuth 2.0 風) + per-resource-type 粒度**:

```
events.publish.{type}      events.publish.*       events.delete.{type}
resources.read.public      resources.read.own     resources.read.shared      resources.read.{type}
resources.delete.{type}
apps.register              apps.verify
webhooks.subscribe.{type}
```

形式: `<resource-family>.<action>.<modifier-or-type>`

加えて:
- **least-privilege rule**: `declare-only-what-you-emit` を spec rule 化
- **discovery**: `GET /v1/scopes` runtime endpoint + spec embed + manifest URL での宣言
- **manifest-based assignment flow**: product が `scopes_requested` を manifest で宣言、 Hub admin が approve subset を grant

各 product の v0.2 scope:
- `creo-memories`: `events.publish.memories-atlas` + `events.delete.memories-atlas`
- `vp`: `events.publish.vp-world` + `events.delete.vp-world` + `events.publish.vp-actor` + `events.delete.vp-actor`
- `cplp`: `events.publish.cplp-session` (delete なし、 immutable session)
- `fleetstage`: `events.publish.fleetstage-tenant` + **`events.delete.fleetstage-tenant`** (v0.1 漏れ補完)
- `gfp`: `events.publish.gfp-shipment` + **`events.delete.gfp-shipment`** (v0.1 漏れ補完)

## Consequences

### 正

- GitHub / Google OAuth と同 idiom、 consumer が解釈しやすい
- per-type 粒度で least-privilege 達成
- `cplp` の immutable design は明示的に delete scope 不要として表明 (spec 自己記述)
- `fleetstage` / `gfp` の delete scope 漏れを v0.2 で補完
- 90 日未使用 scope を audit log で detect、 revocation candidate 提示

### 負

- token claim の scope list が長くなる (per-type 粒度なので、 wildcard `events.publish.*` で省略可)
- v0.1 scope (`register:resource`) からの migration alias が必要 (互換 1 サイクル)

## 却下案

- **平文 `register:resource`** — 現状、 粒度なし、 不採用
- **複合 `resource:{type}:{action}`** — parsing 複雑、 OAuth 系列と非互換

## References

- Memory: `mem_1CaPD65gvV2viWFrrLVe4z`
- Gap list: `mem_1CaP3fUFz5DKW3DzJP1jGf`
- spec: `world-tree.kdl` `auth.scope-registry`, `auth.least-privilege-rule`, product manifests
- 関連 ADR: ADR-003 (API endpoints の auth 分岐と整合)、 ADR-009 (manifest scopes_requested)
- 外部参考: GitHub OAuth scope notation (`repo:status`, `user:email`)
