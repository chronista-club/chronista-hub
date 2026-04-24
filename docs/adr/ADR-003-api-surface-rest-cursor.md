# ADR-003: REST API surface with cursor pagination (G2)

- **Status**: Accepted (target) — **Partially implemented** in AC-15 as MVP subset
- **Date**: 2026-04-25
- **Gap**: G2 CRUD surface 不完全

## Implementation status (2026-04-25)

AC-15 で landed した Tree read API v1 は本 ADR の **MVP subset** に相当。差分は意図的な簡略化 (MVP 優先) で、 full scope は後続 phase で段階的に収束させる:

| 側面 | 本 ADR (target) | AC-15 実装 (landed) | 収束予定 |
|---|---|---|---|
| Path style | `/v1/users/{handle}/...` | `/tree/@:handle/*path` (wildcard) | 未決、 MVP の flexibility を保持する選択肢も |
| Resource by id | `/v1/users/.../{type}/{id}` (nested) | `/resources/:id` (flat) | AC-16+ で検討 |
| Apps | `/apps`, `/apps/{app_id}` | `/apps/:appId/manifest` | path shape 調整予定 |
| `/search` | 必須 | 未実装 | AC-17+ |
| `/products` | 必須 | 未実装 | AC-17+ |
| Pagination | cursor | interface に宣言のみ、 未適用 | AC-16+ で enable |
| Response envelope | `{ data, meta }` | flat `{ handle, path, resources }` | 段階的に refactor |
| Errors | RFC 7807 | simple `{ error: string }` | AC-16+ |
| Auth | user-jwt / public 分岐 | stub (未 wire) | AC-16+ |

この divergence は Phase 1 MVP の速度優先判断であり、 **本 ADR は v1.0 GA 時点での target** として維持。 各差分は **ADR-006** 以降で個別に決着予定。

## Context

spec v0.1 は `sync-protocol` (product → Hub ingress) を event-sourced で定義済だが、 **Hub → consumer の egress (read API)** が皆無。 endpoint 一覧、 pagination、 filter、 error 形式、 versioning、 rate limiting、 caching 全て未定義。

`api-path-prefix "/v1"` は宣言されているが body が無く、 Phase 1 実装のとっかかりが存在しない状態。

## Decision

**REST API** を基本、 Phase 1 で ingress + egress-read を実装、 egress-push (webhook / SSE) は Phase 2+ に先送り。

### API boundary (3 axes)

| Axis | 方向 | 用途 | Auth |
|---|---|---|---|
| Ingress | product → Hub | event publish (resource CUD) | product-token |
| Egress (read) | any client → Hub | resource fetch / list / search | user-jwt or public |
| Egress (push) | Hub → subscriber | webhook / SSE / WS | signed (Phase 2+) |

### Endpoints (Phase 1 minimal)

Ingress:
```
POST /v1/events                      auth=product-token
```

Egress read:
```
GET /v1/users/{handle}
GET /v1/users/{handle}/{product}
GET /v1/users/{handle}/{product}/{type}/{id}
GET /v1/apps
GET /v1/apps/{app_id}
GET /v1/search?q&type&handle&product
GET /v1/products
```

### Conventions

| 項目 | 決定 |
|---|---|
| Pagination | **cursor-based**、 opaque base64 `{last_id, last_created_at}`、 limit max=100 default=50 |
| Filtering | structured query params (`type`, `visibility`, `handle`, `product`, `since`, `until`) |
| Response envelope | `{ data, meta }` flat |
| Errors | **RFC 7807 Problem Details** (`https://chronista.club/errors/*` publisher) |
| Versioning | URL prefix `/v1/` (major only)、 minor は additive、 3 バージョン並走で sunset |
| Rate limit | `X-RateLimit-*` + `Retry-After` headers、 bucket: ingress=per product-token、 egress=per user-jwt or IP |
| Caching | ETag + Last-Modified、 list `Cache-Control: public, max-age=60` |

spec 追加 block (v0.2 draft):

```kdl
api {
    version "v1"
    base-path "/v1"

    ingress {
        endpoint "POST /events" auth="product-token"
    }

    egress-read {
        endpoint "GET /users/{handle}" auth="user-jwt-or-public"
        endpoint "GET /users/{handle}/{product}" auth="user-jwt-or-public"
        endpoint "GET /users/{handle}/{product}/{type}/{id}" auth="user-jwt-or-public"
        endpoint "GET /apps" auth="public"
        endpoint "GET /apps/{app_id}" auth="public"
        endpoint "GET /search" auth="user-jwt-or-public"
    }

    pagination { style "cursor"; limit-default 50; limit-max 100 }
    errors    { format "rfc7807" }
    rate-limit { enabled #true; bucket-ingress "per-product-token"; bucket-egress "per-user-jwt-or-ip" }
    cache     { etag #true; last-modified #true; list-max-age-seconds 60 }
}
```

## Consequences

### 正

- UUID v7 の time-sortable 性質と cursor pagination が相互補完 (duplicate / skip 回避)
- RFC 7807 採用で **Chronista Hub 自体が `https://chronista.club/errors/*` publisher** になる — meta-registry 思想と整合
- `POST /events` の batch 化で ingress throughput + G3 順序制御の布石
- URL path version (`/v1/`) で simplicity、 Accept header negotiation の複雑性回避

### 負

- GraphQL を拒否したことで、 consumer が "必要な field のみ fetch" を選べない (over-fetch 発生可)
- cursor base64 の opaque 性は debug 時に不便 (対策: 開発環境で decode endpoint 提供)
- Phase 1 rate-limit 閾値未定で、 Phase 2 で運用実データ見て tune が必要

## 却下案

- **Offset + limit pagination**: 却下、 insertion race で duplicate / skip 発生
- **GraphQL only / GraphQL first-class**: 却下、 REST を基本に simplicity 優先、 GraphQL は将来検討
- **Accept header version negotiation**: 却下、 URL 明示の方が debug / curl 友好
- **Flat error `{error: string}`**: 却下、 machine-readable / cross-product 共通化のため RFC 7807 採用

## 未決 (future work)

- GraphQL の追加 (Phase 2+ 検討)
- search backend: creo-memories の SurrealDB FTS + Qdrant semantic 組合せを踏襲検討
- bulk read (複数 id 一括 fetch) endpoint 要否

## References

- Memory: `mem_1CaP9W1ZLLtZhuTPyLzSgY` (G2 drill-down + 根拠)
- Gap list: `mem_1CaP3fUFz5DKW3DzJP1jGf`
- spec: `docs/spec/world-tree.kdl` `meta.api-path-prefix`
- 関連 ADR: ADR-002 (API path に `usr_id` を埋め込む前提)、 ADR-004 (ingress event schema)
- 外部規格: RFC 7807 (Problem Details for HTTP APIs)
