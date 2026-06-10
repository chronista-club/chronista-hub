-- 003: hub_resource / hub_event — registry 内部 table (手書き)
--
-- これらは spec の resource-type ではなく Hub registry の内部実体なので codegen 対象外。
-- hub_resource = Tree read API が返す materialized resource index (Storage の backing)。
-- hub_event    = products から受けた event envelope の log (EventLog の backing)。
--
-- field は apps/chronista-hub-server/src/storage.ts の Resource 型 /
-- event-log.ts の StoredEvent 型と 1:1 対応。
--
-- startup-migrator が `USE NS chronista DB hub;` で wrap するため USE 文は書かない。

-- ============================================================
-- hub_resource — World Tree resource index
-- ============================================================
-- record id = type::record('hub_resource', <resource.id>) で SurrealStorage が upsert。
-- SurrealDB の `id` は record 識別子の予約名なので、 product 側 resource id は
-- `rid` field に複製して保持する (SELECT で clean な string として取り出すため)。

DEFINE TABLE OVERWRITE hub_resource TYPE NORMAL SCHEMAFULL
  COMMENT 'World Tree resource index — materialized from events';

DEFINE FIELD OVERWRITE rid ON hub_resource TYPE string ASSERT $value != NONE
  COMMENT 'product 側 resource id (= record id の素材)';
DEFINE FIELD OVERWRITE type ON hub_resource TYPE string ASSERT $value != NONE;
DEFINE FIELD OVERWRITE path ON hub_resource TYPE string ASSERT $value != NONE;
DEFINE FIELD OVERWRITE handle ON hub_resource TYPE string ASSERT $value != NONE;
DEFINE FIELD OVERWRITE visibility ON hub_resource TYPE string ASSERT $value != NONE
  COMMENT 'public | shared | private';
DEFINE FIELD OVERWRITE payload ON hub_resource TYPE object FLEXIBLE;
DEFINE FIELD OVERWRITE createdAt ON hub_resource TYPE string;
DEFINE FIELD OVERWRITE updatedAt ON hub_resource TYPE string;

DEFINE INDEX OVERWRITE handle_idx ON hub_resource FIELDS handle;
DEFINE INDEX OVERWRITE rid_idx ON hub_resource FIELDS rid UNIQUE;

-- ============================================================
-- hub_event — ingested event envelope log
-- ============================================================
-- record id = type::record('hub_event', <event_id>) で SurrealEventLog が append。

DEFINE TABLE OVERWRITE hub_event TYPE NORMAL SCHEMAFULL
  COMMENT 'Ingested events from products (event-sourced ingestion)';

DEFINE FIELD OVERWRITE event_id ON hub_event TYPE string ASSERT $value != NONE;
DEFINE FIELD OVERWRITE app_id ON hub_event TYPE string ASSERT $value != NONE;
DEFINE FIELD OVERWRITE kind ON hub_event TYPE string ASSERT $value != NONE
  COMMENT 'resource.created | resource.updated | resource.deleted';
DEFINE FIELD OVERWRITE resource ON hub_event TYPE object FLEXIBLE;
DEFINE FIELD OVERWRITE idempotency ON hub_event TYPE string ASSERT $value != NONE;
DEFINE FIELD OVERWRITE emitted_at ON hub_event TYPE string;
DEFINE FIELD OVERWRITE received_at ON hub_event TYPE int
  COMMENT 'append 時刻 (unix ms、 insert 順の proxy)';
DEFINE FIELD OVERWRITE processed_at ON hub_event TYPE option<int>
  COMMENT 'consumer 適用済時刻 (unix ms)。 未処理は NONE';

DEFINE INDEX OVERWRITE event_id_idx ON hub_event FIELDS event_id UNIQUE;
DEFINE INDEX OVERWRITE idempotency_idx ON hub_event FIELDS idempotency UNIQUE;
