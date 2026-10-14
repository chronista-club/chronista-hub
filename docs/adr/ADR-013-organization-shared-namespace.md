# ADR-013: Organization / team — shared `@handle` namespace

- **Status**: Accepted (Phase 1 reserve、 Phase 2 impl)
- **Date**: 2026-04-25
- **Gap**: G12

## Context

v0.1 spec は user entity のみで、 group / org / team の概念無し。 `@chronista-club` のような **組織 handle** が user として扱われ、 squatting 可能性。

## Decision

### Shared namespace + `account_type` field

GitHub-style: user / organization / reserved を **同じ `@handle` namespace** で扱い、 user resource の `account_type` field で区別。

```kdl
field "account_type" enum required=#true default="user" {
    value "user"          // 個人
    value "organization"  // 団体・チーム・企業
    value "reserved"      // claim 不可 (squatting 防止)
}
```

### Org-specific extension as separate resource-types

- `resource-type "organization"` — billing / verified / logo 等の org-specific (slug `@{handle}/_org-meta`)
- `resource-type "org-membership"` — bidirectional membership (slug `@{org-handle}/_members/{usr_id}`)、 role enum (owner / admin / member / guest)

### URL form (system slug `_*`)

```
/@chronista-club              # org profile
/@chronista-club/vp/worlds    # org subtree
/@chronista-club/_org-meta    # org-specific
/@chronista-club/_members     # member list
/@mito/_orgs                  # 所属 org 一覧
```

`_` prefix は system reserved (ADR-012 / G11.4)。

### Resource ownership semantics

`owner.account_type=organization` → org-owned。 access 判定は org-membership 参照:

```
visibility=public OR
owner == requester OR
(owner.account_type=organization AND requester ∈ org members)
```

### Phase scoping

- Phase 1: `account_type` field のみ予約 (default `"user"`)、 `organization` / `org-membership` は spec 宣言のみ
- Phase 2: 実装、 endpoints、 access control の org-aware 化

### Squatting 防止 — Phase 1 で reserve

```kdl
reserved-handles [
    "chronista-club" "chronista" "creo-memories" "creo"
    "vantage-point" "vp" "cplp" "fleetstage" "fleetflow"
    "gfp" "go-fast-packing" "chronista-hub"
]
```

`account_type=reserved` で claim 不可、 release 前に DB sentinel 投入。

## Consequences

### 正

- GitHub の battle-tested namespace pattern を踏襲
- API consumer は `/users/{handle}` で user / org 混在対応 (差分は access control のみ)
- `_` prefix の system slug 規約と整合
- org-membership を bidirectional resource にすることで graph traversal が単方向 query で済む
- Phase 1 で reserve list 仕込みで forward-compat の investment

### 負

- access control logic に org-aware 分岐が必要 (Phase 2 で集中投資)
- org / user の差別化は API response の `account_type` field で client が分岐 (consumer 実装の cognitive load 微増)

## 却下案

- **Reserved slug 作戦** (orgs は special handle list のみ) — scale しない、 動的追加が手作業
- **完全別 entity (`/orgs/{org_id}`)** — path style 分離は UX 悪化、 `/@user` と `/orgs/foo` の混在
- **Org の reserved を後から追加** — 後出しは squat されたら回収不能、 Phase 1 で確定が経済合理

## 2026-10-14 追記 — 段 1 を実装（ADR-024）

会社の名義 `@anycreative` のために、organization と membership の最初の段を Hub v0.11.0 で入れた。詳細は ADR-024。

- 団体と membership の持ち主は Hub（ADR-023 D3 で handle が Hub に来たため。本 ADR の spec の `ownership "creo-id"` は直した）
- 団体は `user` table の行（`account_type = organization`）、membership は `org_membership` table（migration 013）
- 段 1 は持ち主 1 人。団体の作成と招待は API が無く、migration で入れる
- access control の org-aware 化（visibility と resource の owner）はまだ。今は「名義として公開物を出せるか」だけを membership で見る

## References

- Memory: `mem_1CaQaWhLANCtbAdGMfi6un`
- Gap list: `mem_1CaP3fUFz5DKW3DzJP1jGf`
- spec: `world-tree.kdl` `handle-namespace`, `resource-type "user"` / `"organization"` / `"org-membership"`, `phase-scoping`
- 関連 ADR: ADR-002 (handle delegation)、 ADR-012 (system `_` slug 規約)、 ADR-005 (cascade retain-orphan)
- 外部参考: GitHub user/org namespace
