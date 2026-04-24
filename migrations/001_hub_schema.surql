-- 001: chronista/hub DB 初期 schema skeleton (AC-14)
--
-- 本 migration は Hub server の starting point。 AC-15 (Tree read API) /
-- AC-16 (Event ingestion) / AC-17 (Auth middleware) で field を充実させる。
-- 現 Phase 1-1 では table 存在のみ宣言、 field 定義は次 phase。
--
-- NS / DB: chronista / hub (creo-memories の creo/memories とは分離)

-- ============================================================
-- 1. Namespace / Database 準備
-- ============================================================

DEFINE NAMESPACE OVERWRITE chronista;

USE NS chronista;

DEFINE DATABASE OVERWRITE hub;

USE DB hub;

-- ============================================================
-- 2. Table skeleton — hub_resource
-- ============================================================
-- World Tree 上の 1 resource (handle / app / world-node / capability)。
-- AC-15 / AC-16 で fields 追加予定。

DEFINE TABLE OVERWRITE hub_resource TYPE NORMAL SCHEMALESS
  COMMENT 'World Tree resource — 次 phase で SCHEMAFULL 化 + fields 定義';

-- ============================================================
-- 3. Table skeleton — hub_event
-- ============================================================
-- Event-sourced ingestion の receive 先。 AC-16 で consumer + fields。

DEFINE TABLE OVERWRITE hub_event TYPE NORMAL SCHEMALESS
  COMMENT 'Ingested events from products — 次 phase で SCHEMAFULL 化';

-- ============================================================
-- 4. Migration tracker (creo-memories と同 pattern)
-- ============================================================

DEFINE TABLE OVERWRITE _migrations TYPE NORMAL SCHEMALESS
  COMMENT 'Applied migrations log';

DEFINE FIELD OVERWRITE name ON _migrations TYPE string;
DEFINE FIELD OVERWRITE applied_at ON _migrations TYPE datetime DEFAULT time::now();
