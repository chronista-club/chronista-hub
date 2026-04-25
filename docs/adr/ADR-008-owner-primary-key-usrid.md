# ADR-008: Owner primary key — `usr_id` (EntId) + handle display

- **Status**: Accepted
- **Date**: 2026-04-25
- **Gap**: G8 (G1 連帯解)

## Context

v0.1 spec の `resource-base.field "owner" ref target="user"` で user の primary key (handle? id?) が曖昧。 G1 (handle lifecycle) の rename / reclaim 設計を踏まえると、 handle は不安定で stable key として不適切。

## Decision

**`usr_id` (EntId、 `usr_Fj7cx53h` 形式) を stable primary key として ref に固定**:

```kdl
resource-base {
    field "owner" ref target="user" key-field="usr_id"
}

spec-syntax {
    construct "ref" {
        default-key-field "{target}_id"  // usr_id, vpa_id, ...
        override-attribute "key-field"
    }
}
```

API response では **2 軸併載**:

```json
{
  "owner": {
    "usr_id": "usr_Fj7cx53h",
    "handle": "mito",
    "canonical_path": "/@mito"
  }
}
```

dual path scheme:
- `/@{handle}/...` — user-facing canonical
- `/v1/users/{usr_id}/...` — stable / admin (rename 影響なし)

## Consequences

### 正

- handle rename に耐える (resource の `owner.usr_id` 不変、 `owner.handle` は derived)
- N+1 lookup 不要 (response に handle 同梱)
- GitHub (`@user` profile vs `/users/N/...`) と同 idiom、 industry-familiar

### 負

- store layer は EntId、 wire layer は dual representation で **expansion logic** が要る (実装は単純、 cost は API response builder のみ)

## 却下案

- **handle を primary key に** — rename / reclaim で resource ref が壊れる
- **`usr_id` のみ wire 表示** — display で再 lookup 必要、 UX 悪化、 over-fetch 発生

## References

- Memory: `mem_1CaQRnG1JFSBy5XQaXCsrR`
- EntId 設計: `mem_1CYmEaviFM46m16MdAkyqY` (`usr_` prefix 前例)
- Gap list: `mem_1CaP3fUFz5DKW3DzJP1jGf`
- spec: `world-tree.kdl` `resource-base.owner`, `api-response-conventions.ref-expansion`
- 関連 ADR: ADR-002 (G1 handle delegation の自然な帰結)
