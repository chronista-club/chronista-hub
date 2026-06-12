-- 006: hub_product_token — Hub 発行 product-token (ADR-010 Phase 2)
--
-- opaque token 方式: 平文 (`cht_<random>`) は発行時のみ返し、 DB には SHA-256 hash を保存。
-- 検証は hash の DB lookup (embedded なので µs 級)。 即時 revoke = revoked_at を立てるだけ。
-- record id = type::record('hub_product_token', <token_hash>) で O(1) lookup。
--
-- rotation (ADR-010): rotate 時に新 token を発行し、 旧 active token の expires_at を
-- 30 日後へ短縮 (新旧 30 日 overlap)。
--
-- startup-migrator が `USE NS chronista DB hub;` で wrap するため USE 文は書かない。

DEFINE TABLE OVERWRITE hub_product_token TYPE NORMAL SCHEMAFULL
  COMMENT 'Hub-issued product tokens (opaque, hashed) — ADR-010';

DEFINE FIELD OVERWRITE token_hash ON hub_product_token TYPE string ASSERT $value != NONE
  COMMENT 'SHA-256 hex of the plaintext token';
DEFINE FIELD OVERWRITE app_id ON hub_product_token TYPE string ASSERT $value != NONE;
DEFINE FIELD OVERWRITE scopes ON hub_product_token TYPE array<string>;
DEFINE FIELD OVERWRITE note ON hub_product_token TYPE option<string>
  COMMENT 'audit 用メモ (発行理由など)';
DEFINE FIELD OVERWRITE created_at ON hub_product_token TYPE datetime DEFAULT time::now();
DEFINE FIELD OVERWRITE expires_at ON hub_product_token TYPE datetime;
DEFINE FIELD OVERWRITE revoked_at ON hub_product_token TYPE option<datetime>
  COMMENT '即時 invalidate。 NONE = active';

DEFINE INDEX OVERWRITE token_hash_idx ON hub_product_token FIELDS token_hash UNIQUE;
DEFINE INDEX OVERWRITE app_id_idx ON hub_product_token FIELDS app_id;
