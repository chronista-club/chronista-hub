# ADR-004: Event sync reliability (G3)

- **Status**: Accepted
- **Date**: 2026-04-25
- **Gap**: G3 Event sync の信頼性保証が薄い

## Context

spec v0.1 の `sync-protocol` は event-sourced を宣言するが、 **信頼性の契約が未記述**:

- 順序保証 (sequence number or per-resource monotonic) 無し
- delivery guarantee (at-least-once / at-most-once / exactly-once) の言質無し
- retry / DLQ / replay policy 無し
- webhook signing (HMAC) 無し → spoofing 耐性ゼロ
- idempotency key の scope 曖昧

これは CREO-127 の "migration runner partial-apply silent skip" と同根の教訓 — **挙動を spec に約束しない限り production で信頼できない**。

## Decision

### Delivery model

| 項目 | 決定 |
|---|---|
| **Delivery guarantee** | **at-least-once** (duplicate は idempotency で吸収 → effectively-exactly-once) |
| **Dedup strategy** | idempotency-key |
| **Dedup scope** | `(app_id, idempotency_key)` |
| **Dedup TTL** | 24 hours (Stripe ingress pattern) |

### Ordering

| 項目 | 決定 |
|---|---|
| **Scope** | per-resource monotonic |
| **Mechanism** | `sequence_number` per `(app_id, resource_id)` |
| **Gap handling** | Hub が gap 検出 → replay request |

### Signing (HMAC-SHA256)

| 方向 | 要件 |
|---|---|
| Ingress | product-token (bearer) + HMAC 任意 (Phase 1) → 必須 (Phase 2) |
| Egress push | Hub-signed per-subscription secret で **必須** |

Headers:
```
X-Chronista-Signature: sha256=<hex>
X-Chronista-Timestamp: <unix>
```

timestamp drift 許容: 5 分 (GitHub webhook pattern)。

### Retry / DLQ (egress push)

```
exponential backoff: 1s → 5s → 25s → 2m → 10m → 1h → 6h
max-attempts: 7
→ DLQ + subscriber alert
```

### Replay

| Mode | Endpoint | 用途 |
|---|---|---|
| Bootstrap | `POST /v1/events/replay { app_id, from_timestamp }` | 新 Hub 初期化時に product が全 event を再 publish |
| Per-resource | `GET /v1/users/{handle}/{product}/{type}/{id}/history?limit=N` | consumer による変遷 fetch |
| DLQ recovery | `POST /v1/subscriptions/{id}/replay { from: "<event_id>" }` | dead-letter 落ち event の再送 |

### Backpressure

| 方向 | 挙動 |
|---|---|
| Ingress 過負荷 | `429 + Retry-After` |
| Egress push 過負荷 | subscriber 5xx → backoff scheme 発動 |
| Hub 内部 queue | `queue_depth_total` metric exposed |

### Event schema 拡張 (v0.1 → v0.2)

追加 field:

```kdl
event-schema {
    field "sequence_number" int       required=#true
    field "resource_id"     string    required=#true
    field "prev_event_id"   uuid                          // optional, strict causality
    field "signature"       string                        // HMAC-SHA256 hex
    field "signature_ts"    timestamp                     // replay 防止
    field "replay"          bool      default=#false      // bootstrap 判別
}
```

### Spec 追加 block (v0.2 draft)

```kdl
sync-protocol {
    default      "event-sourced"
    fallback     "webhook"
    consistency  "eventual"

    delivery-guarantee "at-least-once"
    dedup-strategy     "idempotency-key"
    dedup-scope        "(app_id, idempotency_key)"
    dedup-ttl-hours    24

    ordering {
        scope     "per-resource"
        mechanism "sequence-number"
    }

    signing {
        algorithm                  "HMAC-SHA256"
        ingress                    "optional-phase1-required-phase2"
        egress-push                "required"
        timestamp-drift-max-seconds 300
    }

    retry {
        scheme                    "exponential-backoff"
        backoff-schedule-seconds  [1 5 25 120 600 3600 21600]
        max-attempts              7
        dlq-on-exhaustion         #true
    }

    replay {
        bootstrap-endpoint    "POST /v1/events/replay"
        per-resource-endpoint "GET /v1/users/{handle}/{product}/{type}/{id}/history"
    }

    backpressure {
        ingress     "429-with-retry-after"
        egress-push "subscriber-5xx-backoff"
        metric      "queue_depth_total"
    }
}
```

## Consequences

### 正

- **CREO-127 ファミリの "silent skip" 問題** を構造的に予防 — 挙動が spec で明文化されるので implementer が guess する余地を減らす
- Stripe (idempotency) / GitHub (HMAC + timestamp) の業界 idiom に準拠 → cross-stack consumer / 3rd party にも読みやすい
- DLQ + replay endpoint の両輪で operator に escape hatch を提供、 event が "永遠に失われる" death spiral を防ぐ
- `sequence_number` + UUID v7 の time-sortable 性質で、 consumer が resource 状態を rebuild できる ("時点 T 時点の状態" の再構成)

### 負

- dedup store に SurrealDB table が必要 → storage cost (24h TTL で bound)
- HMAC + timestamp 検証を全 ingress / egress で走らせる CPU cost (対策: signature verify のみ critical path、 compute 自体は軽い)
- Phase 2 で ingress HMAC 必須化する際に product 側 migration が必要 (予告期間を明記する運用が要る)

## 却下案

### Delivery guarantee
- **at-most-once**: 却下、 data loss 許容できない
- **exactly-once (distributed consensus)**: 却下、 overkill、 複雑性の増分に見合う利得なし

### Ordering
- **全 event 全順序**: 却下、 bottleneck、 不要な強さ
- **順序無保証**: 却下、 "updated → created" 逆順で consumer state corruption

### Signing
- **HMAC なし (bearer token only)**: 却下、 token leak したら即座に impersonation 可、 防御層が薄すぎる
- **Ed25519 signature**: 却下、 symmetric HMAC で十分、 key distribution 複雑化させない

### Reclaim (削除済 event の扱い)
- **hard delete**: 却下、 replay 不能になる
- **archival-only (read-only)**: 採用 (implicit)、 削除は tombstone で表現 (G4 で decide)

## References

- Memory: `mem_1CaP9eVseB84i9hvTsDavn` (G3 drill-down + 根拠)
- Memory: `mem_1CaNq1WjN7hantdEVfu853` (CREO-127、 同根の学び)
- Gap list: `mem_1CaP3fUFz5DKW3DzJP1jGf`
- spec: `docs/spec/world-tree.kdl` `sync-protocol`, `event-schema`
- 関連 ADR: ADR-003 (ingress endpoint、 replay endpoint は ADR-003 の API surface に同居)
- 外部規格: Stripe Idempotency, GitHub webhook HMAC signature pattern
