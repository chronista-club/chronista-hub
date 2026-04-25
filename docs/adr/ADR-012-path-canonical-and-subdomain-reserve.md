# ADR-012: Path-based canonical URLs + subdomain reservation

- **Status**: Accepted
- **Date**: 2026-04-25
- **Gap**: G11

## Context

v0.1 spec は `/@{handle}/...` path scheme を持つが、 `{handle}.chronista.club` subdomain との関係が未決。 trailing slash / case sensitivity / path traversal / IDN policy も未定義。

## Decision

### Canonical form: **path-based `/@{handle}/...`**

- `chronista.club/@{handle}/...` を canonical
- `{handle}.chronista.club` は **永久 reserve** (squatting 防止、 wildcard `*.chronista.club` は 404 / root redirect)
- 単一 TLS cert で運用可、 GitHub `@user` style と一貫、 AC-15 impl `/tree/@:handle` と整合

### System subdomains 予約

`id`, `api`, `www`, `admin`, `docs`, `status`, `assets`, `webhooks` を reserve。

### Path slug 拡張 (reserved)

v0.1: `identity`, `apps`, `world`, `.well-known`
v0.2 追加: `api`, `oauth`, `errors`, `static`, `health`, `_`

`@` prefix は handle 専用、 それ以外は system 領域。

### Trailing slash: **301 to no-trailing-slash**

```
/@mito/ → 301 → /@mito
```

### Case sensitivity: **lowercase canonical** (G1.4 整合)

```
/@Mito → 301 → /@mito
```

例外: EntId (`usr_Fj7cx53h`) は Base58 case-mixed のまま。

### IDN: 現バージョン非対応

handle が ASCII-only (`^[a-z0-9][a-z0-9-]{0,30}$`) なので Cyrillic homograph 問題なし。 IDN 対応は v1.x 以降。

### Path traversal: 400 reject

`..` / `~` 含む path は 400 Bad Request、 normalize before route。

### Dual path scheme

```
/@{handle}/...           # user-facing canonical
/v1/users/{usr_id}/...   # stable / admin (rename 影響なし)
```

ADR-008 の owner key 設計と整合。

## Consequences

### 正

- 単一 TLS cert (LetsEncrypt HTTP-01 で済む)
- `{handle}.chronista.club` の squatting 防止 (forward-compat investment)
- 301 canonicalize で SEO duplicate content 回避、 cache 効率
- `@` prefix の slug 二分で system path との衝突を構造的に防止

### 負

- `{handle}.chronista.club` 利用希望が出ても永久 reserve なので switch コスト
- IDN 非対応で global 用途には拡張版が必要 (v1.x 以降の課題)

## 却下案

- **subdomain canonical** — wildcard cert 必要、 DNS 複雑、 GitHub 非互換
- **dual-canonical (両併存)** — SEO / cache / link 混乱、 canonical が無いと unstable
- **trailing-slash any (preserve as-is)** — duplicate URL cache、 GitHub と Apache 系の慣習混在で consumer 困惑

## References

- Memory: `mem_1CaQUm6HyBqsjQqApXtWwM`
- Gap list: `mem_1CaP3fUFz5DKW3DzJP1jGf`
- spec: `world-tree.kdl` `path-schema`
- 関連 ADR: ADR-002 (G1 lowercase canonical 同 policy)、 ADR-008 (dual path scheme)、 AC-15 commit `15653cc` (`/tree/@:handle` 実装)
