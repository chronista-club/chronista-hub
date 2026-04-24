# ADR-002: Identity delegation to Creo ID (G1)

- **Status**: Accepted
- **Date**: 2026-04-25
- **Gap**: G1 Handle ライフサイクル未定義

## Context

spec v0.1 は user resource を定義するが、 **handle のライフサイクル** (claim / rename / reclaim / canonicalization) が未記述。 各 product (creo-memories / vp / cplp / fleetstage / gfp) が user を参照する際の "どの identifier が stable key か" も曖昧なまま。

README には **Creo ID (`id.creo-memories.in`) が identity SSOT** と宣言済だが、 Hub 側で何をどこまで index し、 Creo ID の変更をどう反映するかの具体契約が無い。

## Decision

**Creo ID 完全委譲**:

| Sub-decision | 決定 |
|---|---|
| Claim mechanism | **Creo ID delegate** — Hub は `user.created` event を mirror するのみ |
| Rename propagation | **Soft redirect (301、 90 日保持)** — `old_handle.redirect_until` tombstone |
| Reclaim policy | **Cooldown 180 日** — Creo ID 側 `handle_reclaim_eligible_at` を反映 |
| Canonicalization | **Lowercase canonical + 301 redirect** — pattern 現状維持 `^[a-z0-9][a-z0-9-]{0,30}$` |

**stable key**: `resource-base.owner` は `usr_id` (EntId、 `usr_Fj7cx53h` 形式) で参照する。 `handle` は display / URL canonical のみに使う。

spec 追加 block (v0.2 draft):

```kdl
identity-delegation {
    primary-owner "creo-id"
    hub-role "mirror"

    rename-policy {
        mode "soft-redirect"
        redirect-retention-days 90
        path-identifier "usr_id"
    }

    reclaim-policy {
        cooldown-days 180
    }

    canonicalization {
        lowercase #true
        redirect-on-mismatch "301"
    }
}
```

## Consequences

### 正

- Hub の責務境界が明確 ("connector + index"、 authoritative identity store ではない)
- handle rename に耐える resource model (`usr_id` 参照) を自動導出 → gap G8 (owner primary key 曖昧) の部分解決
- Creo ID 側の policy 変更に対して Hub は reactive、 中央集権を回避
- Twitter (`display_name` vs `@handle`)、 GitHub (`login` vs `id`) と同 idiom、 OSS 移植性高

### 負

- Hub 側で `usr_id ↔ handle` の 2 軸 lookup index が必要 (lookup table 追加、 cache 考慮)
- rename event の transaction 境界設計が必須 (Hub index 更新と path redirect table の原子性)
- 180 日 cooldown 期間の state 管理コスト (削除済 handle の預かり)

## 却下案

### G1.1 claim mechanism

- **FCFS (GitHub 方式)**: 却下、 squatting risk
- **email-verified claim**: △、 Creo ID 側で吸収できるので Hub に置く必要なし
- **運営審査**: 却下、 scale しない

### G1.2 rename propagation

- **Hard break (410 Gone)**: 却下、 link rot、 cross-product 参照壊れる
- **Permanent alias**: 却下、 impersonation risk (reclaim 時)
- **変更不可**: 却下、 UX、 Creo ID 側の自由度を奪う

### G1.3 reclaim policy

- **即時 reclaim**: 却下、 impersonation risk ↑
- **Never reclaim**: 却下、 handle 枯渇、 削除後の救済パス無し

### G1.4 canonicalization

- **Case-sensitive**: 却下、 impersonation risk 大
- **完全拒否 (404)**: 却下、 UX 悪い

## References

- Memory: `mem_1CaP9DxkU2ZgHpXRQRwcMS` (G1 決定 drill-down)
- Memory: `mem_1CYmEaviFM46m16MdAkyqY` (EntId 設計、 `usr_` prefix 前例)
- Gap list: `mem_1CaP3fUFz5DKW3DzJP1jGf` (G1, G8 の原典)
- spec: `docs/spec/world-tree.kdl` path-schema, resource-type "user"
- 関連 ADR: ADR-003 (API surface は `usr_id` を stable key として埋め込む)
