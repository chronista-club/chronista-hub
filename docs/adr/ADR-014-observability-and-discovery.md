# ADR-014: Observability + Discovery (Phase 1-3 graded scope)

- **Status**: Accepted (Phase 1 基礎、 Phase 2-3 拡張)
- **Date**: 2026-04-25
- **Gap**: G13, G14

## Context

v0.1 spec は health endpoint / metrics / tracing / logging / rate-limit / status-page の基盤を declare していない。 cross-product discovery / search / notification も同様に未記述。 Hub の operability と consumer-facing discoverability を確保する仕組みが要る。

## Decision

### Observability (G13)

| 機能 | endpoint | format | Phase |
|---|---|---|---|
| Health (shallow) | `GET /health` | application/health+json | 1 |
| Health (deep) | `GET /health/deep` | 同上 + 依存 service 検証 | 1 |
| Metrics | `GET /metrics` | Prometheus | 1 |
| Tracing | W3C Trace Context propagate + OTLP gRPC export | OpenTelemetry | 1 (best effort) |
| Structured logging | JSONL with `ts`/`level`/`service`/`event` + context | — | 1 |
| Rate limiting | token-bucket + `X-RateLimit-*` + 429 / `Retry-After` | — | 1 (basic) |
| Status page | `status.chronista.club` | RSS + dashboard | 2 |

Rate limit defaults:
- ingress per product-token: 1000/min
- egress per user-jwt: 600/min
- egress per IP: 60/min
- webhook events: 100/sec per subscriber

### Discovery / search / notification (G14)

| 機能 | endpoint | Phase |
|---|---|---|
| Public feed (recent / popular) | `GET /v1/feed/recent` / `popular` | 2 |
| Cross-resource search | `GET /v1/search` (SurrealDB FTS + Qdrant semantic) | 3 |
| Notification inbox | `/v1/users/{handle}/inbox` | 3 |
| Follow / timeline | `resource-type "follow"` + `/timeline` | 3 |
| RSS / Atom | `/@{handle}/timeline.rss` etc. | 3 |

search backend は creo-memories の前例 (`mem_1CYpxcy1cdkZ76Ezs5Gmho`) を踏襲、 modes: keyword / semantic / **hybrid (default)**。

## Consequences

### 正

- `application/health+json` 採用で k8s probe / monitoring tool が標準対応のまま使える
- W3C Trace Context で **cross-product (creo-memories ↔ vp ↔ chronista-hub) end-to-end tracing が無料で成立**
- RSS / Atom expose で newsletter / blog reader / IFTTT 等の長尾エコシステム interop
- search backend を creo-memories と同 stack にすることで operational 知見再利用、 migration noise 最小

### 負

- Phase 3 機能 (search / notification / follow) が実装されるまで **discovery は基本的に手動 link 共有依存**
- Prometheus / OpenTelemetry の operational 工数は Phase 1 で必要 (基本だけでも infra cost あり)

## Phase scoping rationale

- **Phase 1 (Hub MVP)**: health / metrics / tracing / logging / rate-limit basic — Hub が prod でまともに動く最低限
- **Phase 2**: status page / public feed — consumer-facing discoverability の第一歩
- **Phase 3**: search / notification / follow / RSS — エコシステム的 social-graph 機能

graded scope により Phase 1 不要 feature を背負わない。

## 却下案

- **Phase 1 で全部入り** — over-engineering、 critical path 拡大、 不要 feature の維持コスト
- **observability を spec 外** — implementation 依存になり、 product / consumer が共通 idiom を持てない
- **search backend を Algolia / Meilisearch 等 SaaS** — Chronista のデータ residency 方針 (self-hosted 主体) と乖離

## References

- Memory: `mem_1CaQaijKJKrdp9Tyyfe8NB`
- Gap list: `mem_1CaP3fUFz5DKW3DzJP1jGf`
- creo-memories search backend 前例: `mem_1CYpxcy1cdkZ76Ezs5Gmho`
- spec: `world-tree.kdl` `observability`, `discovery`, `phase-scoping`
- 関連 ADR: ADR-003 (rate-limit base、 cursor pagination)、 ADR-004 (queue depth metric integration)、 ADR-012 (status.chronista.club reserved subdomain)
- 外部規格: IETF `application/health+json`, OpenTelemetry W3C Trace Context, Prometheus
