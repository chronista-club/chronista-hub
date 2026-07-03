-- 007: hub_resource.owner — federation discovery の owner 分離 (ADR-020 §S5)
--
-- spec `resource-base.owner` (usr_id ref、 world-tree.kdl) を hub_resource に実装。
-- worlds.Register が connection principal の usr_id を記録し、 Discover が
-- 「自分が owner の world + public」に絞る backing。
--
-- option<string>: 既存行 (owner 無し = legacy/未認証 permissive 登録) は NONE のまま
-- 有効 — migration での backfill 不要。 書き込み元は federation 経路のみ
-- (ingestion `/v1/events` には配線しない — app が任意 usr_id を名乗る面を開かない)。
--
-- startup-migrator が `USE NS chronista DB hub;` で wrap するため USE 文は書かない。

DEFINE FIELD OVERWRITE owner ON hub_resource TYPE option<string>
  COMMENT '所有者 usr_id (EntId、 spec resource-base.owner)。 NONE = legacy/未認証登録';
