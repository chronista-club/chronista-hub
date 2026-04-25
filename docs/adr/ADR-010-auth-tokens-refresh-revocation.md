# ADR-010: Auth — token refresh, revocation, audience extensibility

- **Status**: Accepted
- **Date**: 2026-04-25
- **Gap**: G9

## Context

v0.1 spec は OIDC issuer + JWKS URI + 単一 audience を declare するのみ。 refresh / revocation / multi-service audience / product-token rotation policy 未定義。

## Decision

### Token type 整理 (5 種)

| Token | 発行元 | TTL | 用途 |
|---|---|---|---|
| user-jwt | Creo ID (OIDC) | 1h | user → Hub egress |
| user-refresh-token | Creo ID | 30 日 | user-jwt 再発行 |
| product-token | Hub | 1 年 (manual rotation) | product → Hub ingress |
| app-user-token (Phase 2+) | Hub via OAuth | 1h | 3rd party app on behalf of user |
| trusted-service-token | Hub internal | 24h | introspection 等 |

### Refresh / revocation

- **Refresh**: RFC 6749 §6 (Creo ID で実行、 Hub は verify のみ)
- **Revocation**: RFC 7009、 user-token は Creo ID で revoke、 Hub は **denylist (1 分 cache TTL) + webhook push** で反映。 product-token は Hub 自身で即時 invalidate。
- **Introspection**: RFC 7662、 `POST /v1/oauth/introspect` (trusted-service-token)

### Audience as list

```kdl
audiences ["chronista-hub" "chronista-hub-admin" "chronista-hub-public" "chronista-hub-webhook"]
```

将来 sub-service が増えても breaking change にならない。 client は contains() で一致判定。

### Product-token rotation

- TTL 1 年
- 期限 30 日前 reminder
- 30 日 overlap (新旧並走可)
- `POST /v1/apps/{app_id}/tokens/rotate`
- audit log

### JWKS rotation

- 通常 2 key 並走 (current + previous)
- rotation event を Hub に webhook 通知
- jwks cache TTL max 5 分 + push 即時 invalidate

## Consequences

### 正

- JWT 即時 revocation 不可問題に対する **denylist + cache** の妥協は industry standard
- audience list 化で sub-service 追加が non-breaking
- rotation overlap 30 日が dev experience を守る (期限切れで突然停止しない)
- introspection 専用の trusted-service-token で internal traffic を区別

### 負

- denylist fetch (1 分 cache) で Hub → Creo ID 依存が増える (Creo ID down 時の degraded path 設計が要)
- product-token 1 年 TTL は長め、 rotation 忘却リスク (reminder 自動化で軽減)

## 却下案

- **完全 stateless JWT (revocation 不可)** — 実用上 lost device / 漏洩対応できない
- **完全 stateful session (Hub 全 token DB lookup)** — scale しない
- **単一 audience 固定** — sub-service 追加が breaking
- **Ed25519 signature の egress webhook** — symmetric HMAC で十分、 key distribution の複雑化を避ける

## References

- Memory: `mem_1CaQRpmTu5VQMRaFfPAkie`
- Gap list: `mem_1CaP3fUFz5DKW3DzJP1jGf`
- spec: `world-tree.kdl` `auth` block
- 関連 ADR: ADR-002 (Creo ID OIDC delegation の前提)、 ADR-004 (HMAC signing 整合)、 ADR-006 (scope と audience の 2 軸 grant)
- 外部規格: RFC 6749 §6, RFC 7009, RFC 7662
