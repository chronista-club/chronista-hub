-- 001: bootstrap — Namespace / Database / migration tracker
--
-- startup-migrator が各 migration を `USE NS chronista DB hub;` で wrap するため、
-- 本ファイルに USE 文は書かない。NS / DB は IF NOT EXISTS で冪等に確保し、
-- 既存データを誤って drop しないようにする (OVERWRITE は使わない)。
--
-- 旧 001_hub_schema.surql の SCHEMALESS skeleton (hub_resource / hub_event) は
-- 003_hub_internal_tables.surql で SCHEMAFULL 定義に置換した。
--
-- NS / DB: chronista / hub (creo-memories の creo/memories とは分離)

DEFINE NAMESPACE IF NOT EXISTS chronista;
DEFINE DATABASE IF NOT EXISTS hub;

-- ============================================================
-- Migration tracker (creo-memories と同 pattern)
-- ============================================================

DEFINE TABLE OVERWRITE _migrations TYPE NORMAL SCHEMAFULL
  COMMENT 'Applied migrations log';

DEFINE FIELD OVERWRITE name ON _migrations TYPE string;
DEFINE FIELD OVERWRITE applied_at ON _migrations TYPE datetime DEFAULT time::now();

DEFINE INDEX OVERWRITE name_idx ON _migrations FIELDS name UNIQUE;
