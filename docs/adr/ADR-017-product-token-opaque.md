# ADR-017: product-token は opaque + DB hash 方式で実装する

- **Status**: Accepted
- **Date**: 2026-06-12
- **Context part of**: ADR-010 (token 種別と rotation policy は決定済、 token の物理形式が未決だった)

## Context

ADR-010 は product-token (product → Hub ingress、 TTL 1 年、 rotate 30 日 overlap、 **Hub 自身で
即時 invalidate**) を定義したが、 token を署名付き JWT にするか opaque にするかは未決だった。
Phase 2 実装 (本 ADR) で形式を確定する。

## Decision

**opaque token + DB hash** を採用。

- 形式: `cht_<32byte random hex>` (prefix は secret scanning / 目視識別用)
- 保存: SHA-256 hash のみ DB (`hub_product_token`、 migration 006)。 平文は発行レスポンス一度きり
- 検証: hash の DB lookup — record id = `type::record('hub_product_token', hash)` で O(1)。
  embedded SurrealDB (ADR-016) なので in-process µs 級、 低レイテンシ要件と整合
- 即時 revoke: `revoked_at` を立てるだけ (ADR-010 の核心要件に直結)
- rotation: 新 token 発行 + 旧 active token の `expires_at` を 30 日後へ短縮 (新旧 30 日 overlap)
- 有効期限/失効判定は SurrealQL 側 (`expires_at > time::now() AND revoked_at IS NONE`) — 時計の真実を DB に一元化

### 管理 API (admin key gate)

発行/rotate/revoke/一覧は `X-Admin-Key` header (`HUB_ADMIN_KEY` env) で保護。
未設定なら管理 API は **404** (機能ごと隠す、 fail-closed)。 比較は SHA-256 hash 同士。

- `POST /v1/apps/{app_id}/tokens` — 発行 (201、 平文は一度きり)
- `POST /v1/apps/{app_id}/tokens/rotate` — rotation (ADR-010)
- `DELETE /v1/apps/{app_id}/tokens/{token_id}` — 即時 revoke
- `GET /v1/apps/{app_id}/tokens` — 一覧 (メタのみ)

### JWKS 定期 refresh (同 Phase)

user-jwt 側の JWKS を 5 分毎 (env `JWKS_REFRESH_SECS`) に background refetch。
fetch/parse 失敗時は旧 keys を維持 — Creo ID の鍵 rotation に再起動なしで追従 (ADR-010 の
5 分 cache 相当。 push invalidate は将来拡張)。

## Consequences

### 正
- 即時 revocation が行更新 1 つ (JWT + denylist の複雑さを回避)
- 署名鍵の管理・rotation が不要
- GitHub PAT / Stripe API key と同 idiom

### 負
- 検証ごとに DB lookup (embedded なので実質コストなし。 remote DB 化したら再評価)
- token は Hub 単体でしか検証できない (他サービスへの federation には不向き — その用途は user-jwt)

## 却下案

- **署名付き JWT (Hub 鍵)** — stateless 検証は魅力だが、 即時失効に denylist が必要になり
  ADR-010 の核心要件と相性が悪い。 embedded DB 構成では lookup コストの利点も薄い。
- **user-jwt + admin scope で管理 API 保護** — user token への scope 評価導入と Creo ID 側の
  scope 発行連携が前提になり重い。 admin key (env) で開始し将来移行可。

## References

- ADR-010 (token 種別 / rotation policy)、 ADR-016 (embedded SurrealDB)
- migration: `migrations/006_hub_product_token.surql`
- 実装: `crates/chronista-hub-server/src/product_token.rs`
