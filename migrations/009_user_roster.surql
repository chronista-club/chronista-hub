-- 009: user — 利用者の名簿と handle (ADR-023 D3、 ADR-002 / 008 / 012 の規則)
--
-- `user` table は 002 (spec の codegen) で定義済み。 これまで 004 の予約名 (account_type =
-- reserved) しか入っていなかった。 ここから本物の利用者の行が入る。
--
-- 裁定 2026-10-09: handle は「必要になったときに claim する」。
-- - 行は初回接触 (`GET /v1/me`) で作る。 鍵は Hub が振る `usr_` EntId (ADR-008)。
--   record id = type::record('user', usr_id)、 同じ値を `usr_id` field にも持つ
-- - handle は claim (`PUT /v1/me/handle`) まで空。 だから必須を外す。 unique index は
--   空の行を数えないので、 handle 無しの利用者は何人いてもよい
-- - display_name は呼び名。 自由文字列で一意ではない (handle とは別持ち)
-- - `creo_sub` = Creo ID の `sub`。 本人を特定する唯一の手がかり。 1 人 1 行なので一意。
--   予約名の行には無い
-- - `email` は Creo ID が持つ (ADR-023 D1)。 Hub は知らないので必須を外す。 予約名の行の
--   `system@chronista.club` はそのまま
-- - handle は一意。 予約名も同じ table にあるので、 index が claim の衝突も防ぐ
--
-- startup-migrator が `USE NS chronista DB hub;` で wrap するため USE 文は書かない。

DEFINE FIELD OVERWRITE handle ON user TYPE option<string>
  COMMENT '住所。 小文字英数 ^[a-z0-9][a-z0-9-]{0,30}$、 claim するまで空'
  PERMISSIONS FULL;

DEFINE FIELD OVERWRITE email ON user TYPE option<string>
  COMMENT '個人なら user email、 org なら管理 email。 本物の利用者は持たない (Creo ID が持つ)'
  PERMISSIONS FULL;

DEFINE FIELD OVERWRITE usr_id ON user TYPE option<string>
  COMMENT 'Hub が振る EntId (usr_…)。 record id と同じ値。 予約名の行には無い'
  PERMISSIONS FULL;

DEFINE FIELD OVERWRITE creo_sub ON user TYPE option<string>
  COMMENT 'Creo ID の sub。 本人だけに見せる。 予約名の行には無い'
  PERMISSIONS FULL;

DEFINE FIELD OVERWRITE created_at ON user TYPE option<datetime>
  DEFAULT time::now()
  PERMISSIONS FULL;

DEFINE INDEX OVERWRITE user_handle_idx ON user FIELDS handle UNIQUE;
DEFINE INDEX OVERWRITE user_creo_sub_idx ON user FIELDS creo_sub UNIQUE;
