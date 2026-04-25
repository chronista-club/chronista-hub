# ADR-015: AC-15 MVP API shape — current state codification

- **Status**: Accepted (Phase 1 current state、 ADR-003 target に向けて段階的収束)
- **Date**: 2026-04-26
- **Related Gap**: G2 (ADR-003 と相補)

## Context

ADR-003 (`docs/adr/ADR-003-api-surface-rest-cursor.md`) は v1.0 GA 時点での **target REST API surface** を定義する。 一方、 AC-15 (commit `15653cc`) で landed した Tree read API v1 は **MVP subset** であり、 ADR-003 の full scope の一部のみ実装。

ADR-003 内に divergence note を入れて current state は記録しているが、 **MVP shape 自体を独立 ADR で formalize** することで:

- Phase 1 開発中の **API consumer (内部 / 1st party SDK) が依拠する contract** が明示化される
- 将来 ADR-003 へ段階的収束する際の **gap closure plan** を 1 ADR に束ねられる
- 「過去版 spec / impl の snapshot」として historical 価値も持つ

## Decision

### AC-15 で landed した API shape (Phase 1 MVP contract)

```
GET /tree/@:handle              # handle root subtree
GET /tree/@:handle/*path        # 任意 path の subtree (wildcard)
GET /resources/:id              # 単一 resource (flat)
GET /apps/:appId/manifest       # app manifest

AC-16: POST /events              # event ingress (G3 / ADR-004)
AC-17: pluggable auth middleware (StubVerifier、 G9 / ADR-010)
```

### Storage interface の boundary 設計

`apps/chronista-hub-server/src/storage.ts`:

```typescript
export interface Storage {
    getResourcesByHandle(handle, opts?): Promise<Resource[]>
    getResourcesByPath(handle, path, opts?): Promise<Resource[]>
    getResourceById(id): Promise<Resource | null>
    getAppManifest(appId): Promise<AppManifest | null>
}
```

`InMemoryStorage` で stub、 AC-16 / 後続で SurrealDB-backed に差し替え。 Tree read handler (`tree.ts`) は Storage interface のみに依存する **TDD-friendly hexagonal boundary**。

### MVP Response shape (flat、 not enveloped)

```json
{ "handle": "mito", "path": "/", "resources": [...] }
```

ADR-003 の `{ data, meta }` envelope ではなく flat。 Phase 1 simplicity 優先。

### MVP Error shape

```json
{ "error": "invalid handle" }    // 400
{ "error": "not found" }          // 404
```

ADR-003 の RFC 7807 Problem Details ではなく simple flat。 Phase 1 simplicity 優先。

### MVP Pagination / filtering / search

- Pagination: `TreeReadOptions.cursor` / `limit` 宣言済、 **未実装**
- Filtering: `visibility` / `type` query param のみ
- Search (`GET /search`): **未実装** (ADR-014 の Phase 3 へ)
- `/products` endpoint: **未実装**

### Auth state (AC-17)

`StubVerifier` (`packages/.../middleware`) で auth middleware 自体は wired up、 ただし実体は stub。 AC-18+ で Creo ID JWKS 経由の検証実装予定。

## Convergence plan toward ADR-003 target

| 項目 | 現状 (AC-15) | Target (ADR-003) | 収束 phase |
|---|---|---|---|
| Path style | `/tree/@:handle/*path` (wildcard) | `/v1/users/{handle}/{product}/{type}/{id}` (structured) | TBD: keep wildcard? evaluate at AC-18+ |
| Resource by id | `/resources/:id` (flat) | `/v1/users/.../{type}/{id}` (nested) | AC-16+ |
| Apps | `/apps/:appId/manifest` | `/apps`, `/apps/{app_id}` | AC-18+ |
| `/search` | 未実装 | 必須 | AC-17+ (ADR-014) |
| `/products` | 未実装 | 必須 | AC-17+ |
| Pagination | 宣言のみ | cursor 適用 | AC-16+ |
| Response envelope | flat | `{ data, meta }` | AC-16+ refactor |
| Errors | `{ error }` | RFC 7807 | AC-16+ refactor |
| Auth | StubVerifier | Creo ID JWKS verification | AC-18+ |
| Rate limiting | 未実装 | token-bucket + headers | AC-17+ (ADR-014) |
| ETag / Cache | 未実装 | Phase 1 必須 | AC-17+ |

**注**: `/tree/@:handle/*path` の wildcard 形式と ADR-003 の structured form は **別 trade-off**:
- wildcard: 新 resource-type 追加で route 変更不要、 spec 進化への耐性
- structured: typing 明示的、 client side で type 安全な generate 容易

選択は AC-18+ でのみ決定 (現時点では impl 都合で wildcard を選んだ)。

## Consequences

### 正

- Phase 1 開発の **API contract が docs に固定**、 internal SDK / consumer が依拠先を持てる
- ADR-003 (target) と ADR-015 (current state) の **2 軸 ADR 構造**で「目指す形」と「今ある形」が独立に進化可能
- TDD-friendly hexagonal boundary (Storage interface) は AC-16 で SurrealDB-backed 化しても tree.test.ts 変更不要
- `/tree/...` wildcard は spec evolution に対する hedge、 Phase 1 の rapid iteration を阻害しない

### 負

- 2 つの ADR (003 + 015) を同期 maintain する cost
- 後で ADR-003 の structured form に switch する場合、 client SDK は breaking 化する (mitigate: `/v1/users/...` を v1 で同居、 `/tree/...` を sunset)

## 却下案

### ADR-003 を AC-15 shape に書き換える

ADR-003 を current state に合わせて rewrite する案。 却下理由: ADR-003 は **意図的 target**、 immutable な意思決定 record。 implementation refinement で書き換えると ADR の immutability 原則 (本 README 参照) に反する。 別 ADR (本 ADR-015) で current state を残す方が筋。

### Status を Proposed に格下げ

ADR-003 を Proposed に戻す案。 却下理由: 全 14 gap drill-down で既に Accepted、 ADR-003 自体の根拠は否定されていない。 単に impl の Phase 1 cut が小さいだけ。

### MVP shape を spec ではなく code comment / README にだけ残す

ADR にしない案。 却下理由: AC-15 で確立した contract は **Phase 1 期間の SDK consumer 全員が依拠する** 重大事実、 ADR で固定するのが trace と stability の確保に必要。

## References

- AC-15 commit: `15653cc feat(server): Tree read API v1 — 4 endpoints + Storage interface`
- AC-16 commit: `7dad5d1 feat(server): event-sourced ingestion — POST /events + consumer`
- AC-17 commit: `adc6a73 feat(server): pluggable auth middleware + StubVerifier`
- impl: `apps/chronista-hub-server/src/{tree,storage,health}.ts`
- 関連 ADR: ADR-003 (target API surface)、 ADR-004 (event sync)、 ADR-010 (auth tokens)、 ADR-014 (Phase scoping for /search etc)
- 関連 Memory:
  - `mem_1CaP9W1ZLLtZhuTPyLzSgY` G2 API surface drill-down (target side)
  - `mem_1CaP3fUFz5DKW3DzJP1jGf` gap list 原典
- Phase scope: ADR-014 (G13/G14) と整合
