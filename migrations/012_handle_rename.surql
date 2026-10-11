-- 012: handle の変更 (rename) — 旧 handle の取り置き (裁定 2026-10-12、 ADR-002 追記)
--
-- 裁定: rename あり。 旧 handle は 100 日間、 本人だけが取り戻せる (他人は claim 不可)。
-- その間 `/@old` は新しい住所へ案内する。 rename 後 30 日は再変更できない。
-- 本人の取り置きは同時に 1 つ (次の rename で前のものは捨てる)。
--
-- - record id = type::record('handle_reserve', <旧 handle>)。 handle の一意性は id が担う
-- - usr_id は unique: 1 人 1 つの取り置き
-- - 期限切れの行は消さずに置いておき、 読む側が expires_at で判断する (lazy)
-- - user.handle_changed_at: 最後に rename した時刻 (cooldown の起点)。 claim では付けない
--
-- startup-migrator が `USE NS chronista DB hub;` で wrap するため USE 文は書かない。

DEFINE TABLE OVERWRITE handle_reserve TYPE NORMAL SCHEMAFULL
  COMMENT 'rename した人のために旧 handle を取り置く (100 日)。 id = 旧 handle';
DEFINE FIELD OVERWRITE handle ON handle_reserve TYPE string ASSERT $value != NONE
  COMMENT '旧 handle (id と同じ値)';
DEFINE FIELD OVERWRITE usr_id ON handle_reserve TYPE string ASSERT $value != NONE
  COMMENT '取り置きの持ち主';
DEFINE FIELD OVERWRITE new_handle ON handle_reserve TYPE string ASSERT $value != NONE
  COMMENT 'rename 先。 案内はこれではなく持ち主の今の handle へ';
DEFINE FIELD OVERWRITE expires_at ON handle_reserve TYPE datetime
  COMMENT 'これを過ぎたら誰でも claim できる';
DEFINE FIELD OVERWRITE created_at ON handle_reserve TYPE datetime DEFAULT time::now();
DEFINE INDEX OVERWRITE handle_reserve_usr_idx ON handle_reserve FIELDS usr_id UNIQUE;

DEFINE FIELD OVERWRITE handle_changed_at ON user TYPE option<datetime>
  COMMENT '最後に rename した時刻。 30 日の cooldown の起点。 claim では付かない'
  PERMISSIONS FULL;
