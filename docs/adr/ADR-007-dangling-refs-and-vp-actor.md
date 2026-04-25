# ADR-007: Dangling refs resolution + vp-actor

- **Status**: Accepted
- **Date**: 2026-04-25
- **Gap**: G7

## Context

v0.1 spec に 2 つの dangling reference:

1. `subtree { child "identity" }` を `user` resource-type が宣言、 しかし `resource-type "identity"` 未定義
2. `vp-world.payload.actors` の `element ref target="actor"`、 しかし `resource-type "actor"` 未定義

加えて構文 ad-hoc:
3. `enum-ref="resource-base.visibility"` が formal spec construct として未定義

## Decision

### `identity` → virtual subtree (Hub 外部)

```kdl
subtree {
    child "identity" {
        ownership "external"
        external-source "creo-id"
        external-url "https://id.creo-memories.in/users/{handle}/identity"
    }
}
```

`GET /v1/users/{handle}/identity` は **307 Temporary Redirect** で Creo ID へ。 ADR-002 (Creo ID 委譲) と整合。

### `actor` → `vp-actor` 正式 resource-type

Stand Ensemble (TheWorld / Star Platinum / Paisley Park / Heaven's Door / Gold Experience / Hermit Purple) を VP-owned resource-type として正式宣言。 `vp-world.actors` の target を `vp-actor` に修正。

### `enum-ref` formalization

```kdl
spec-syntax {
    construct "enum-ref" {
        format "<scope>.<enum-name>"
        target-resolution "lookup"
        fallback "error"
    }
}
```

加えて `spec-lint` block で `no-dangling-refs` / `no-undefined-resource-types` / `cross-product-ref-via-user-or-app` rules を error severity で enforce。

### Cross-product ref 制約

product A の resource-type が product B の resource-type を直接 ref することを **禁止**。 Cross-product link は **user or app 経由** に限定。

## Consequences

### 正

- spec 整合性が机上で完結 (build-time validation 可能)
- VP の Stand Ensemble が Hub level で観測可能、 cross-CC 通信が trace-able に
- virtual subtree 概念で identity 以外 (例: 外部 carrier service の shipment 状態) も同 pattern で再利用可
- coupling 防止が spec lint で構造的に enforce、 product 境界が経年劣化しにくい

### 負

- `enum-ref` 構文の正式化により codegen tool は parser に追加対応が必要
- virtual subtree の `external-url` は client 側で redirect-follow が要請される (HTTP redirect 標準なので ergonomic 問題なし)

## 却下案

- `resource-type "identity"` を Hub 内に定義 — ADR-002 の Creo ID 委譲と矛盾、 Hub が auth state を mirror することになる
- `target="actor"` を VP 内部実装の implicit reference として残す — spec の self-completeness を犠牲、 lint 困難
- `enum-ref` を spec 内 ad-hoc のまま — codegen / validator が場当たり parsing

## References

- Memory: `mem_1CaQQF846JWBWoFghCLM9S`
- Stand Ensemble: `mem_1CYspkN7wrpMUA4PTu2RsB`
- Gap list: `mem_1CaP3fUFz5DKW3DzJP1jGf`
- spec: `world-tree.kdl` `spec-syntax`, `spec-lint`, `resource-type "vp-actor"`, `user.subtree`
- 関連 ADR: ADR-002 (identity 委譲)
