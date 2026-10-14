-- 013: 団体 (organization) と読める公開 URL の索引 (裁定 2026-10-14、 ADR-024)
--
-- 裁定 (mako):
-- - 会社の名義 `@anycreative` を Hub に作る。 人と同じ handle 空間 (ADR-013)、 `user` table の
--   account_type = 'organization'。 団体はログインを持たない。 人が membership で団体として振る舞う
-- - 読める公開 URL `hub.chronista.club/@{handle}/{slug}` は Hub が索引を持ち、 中身のアプリの
--   公開ページへ案内する (ADR-019 の canonical)
-- - 持てるのは Creo Memories の lab 以上だけ。 プランはアプリが判定するので、 索引への登録は
--   アプリが product token で行う (Hub はプランを知らない)
-- - 最初の段は持ち主 mako 1 人。 招待と役割の変更は後
--
-- startup-migrator が `USE NS chronista DB hub;` で wrap するため USE 文は書かない。

-- ---- membership: 誰がどの団体の何か ----
-- table と field 名は 002 (spec の codegen、 org-membership) のまま。 中身は usr_id
-- (spec の `ref target="user" key-field="usr_id"`)。 ここで必須にして index を張る
DEFINE TABLE OVERWRITE org_membership TYPE NORMAL SCHEMAFULL
  COMMENT '団体の membership (ADR-013 の org-membership)。 段 1 は owner だけ';
DEFINE FIELD OVERWRITE org_user ON org_membership TYPE string ASSERT $value != NONE
  COMMENT '団体の usr_id'
  PERMISSIONS FULL;
DEFINE FIELD OVERWRITE member_user ON org_membership TYPE string ASSERT $value != NONE
  COMMENT '人の usr_id'
  PERMISSIONS FULL;
DEFINE FIELD OVERWRITE role ON org_membership TYPE string
  ASSERT $value INSIDE ['owner', 'admin', 'member', 'guest']
  PERMISSIONS FULL;
DEFINE FIELD OVERWRITE joined_at ON org_membership TYPE datetime DEFAULT time::now()
  PERMISSIONS FULL;
DEFINE INDEX OVERWRITE org_membership_pair_idx ON org_membership FIELDS org_user, member_user UNIQUE;
DEFINE INDEX OVERWRITE org_membership_member_idx ON org_membership FIELDS member_user;

-- ---- publication: 読める公開 URL の索引 ----
-- record id = type::record('publication', [publisher_usr_id, slug])。 名義ごとに slug は一意
DEFINE TABLE OVERWRITE publication TYPE NORMAL SCHEMAFULL
  COMMENT '読める公開 URL (/@handle/slug) の索引。 中身はアプリが持ち、 Hub は案内だけ';
DEFINE FIELD OVERWRITE publisher_usr_id ON publication TYPE string ASSERT $value != NONE
  COMMENT '名義 (人 or 団体) の usr_id。 handle が変わっても索引は付いていく';
DEFINE FIELD OVERWRITE slug ON publication TYPE string
  ASSERT string::matches($value, '^[a-z0-9][a-z0-9-]{0,62}$');
DEFINE FIELD OVERWRITE url ON publication TYPE string
  ASSERT string::starts_with($value, 'https://')
  COMMENT '案内先。 登録したアプリの host に限る';
DEFINE FIELD OVERWRITE title ON publication TYPE option<string>;
DEFINE FIELD OVERWRITE app_id ON publication TYPE string ASSERT $value != NONE
  COMMENT '登録したアプリ。 同じアプリだけが書き換え・削除できる';
DEFINE FIELD OVERWRITE updated_by ON publication TYPE option<string>
  COMMENT '最後に登録した人の usr_id';
DEFINE FIELD OVERWRITE created_at ON publication TYPE datetime DEFAULT time::now();
DEFINE FIELD OVERWRITE updated_at ON publication TYPE datetime DEFAULT time::now();
DEFINE INDEX OVERWRITE publication_slug_idx ON publication FIELDS publisher_usr_id, slug UNIQUE;

-- ---- @anycreative (団体) ----
-- usr_id は固定 (環境をまたいで同じ値)。 handle が使用中、 または rename の取り置き中なら作らない
-- (そのときは 3 つとも飛ばす。 /@anycreative はその人のまま)
IF array::len((SELECT VALUE usr_id FROM user WHERE handle = 'anycreative')) = 0
   AND array::len((SELECT VALUE handle FROM handle_reserve WHERE handle = 'anycreative' AND expires_at > time::now())) = 0 {
    CREATE type::record('user', 'usr_vspVZdfCBo2YG3DN') CONTENT {
        usr_id: 'usr_vspVZdfCBo2YG3DN',
        handle: 'anycreative',
        display_name: 'ANYCREATIVE',
        account_type: 'organization',
        locale: 'ja'
    };
};

-- 持ち主 = mako (live の @mako、 usr_K8r5vy1usBGWuXd4)。 その行がある環境だけで結ぶ
-- (2026-10-14 に GET /v1/users/@mako で確認)。 途中で止まった再実行でも二重にしない
IF array::len((SELECT VALUE usr_id FROM user WHERE usr_id = 'usr_K8r5vy1usBGWuXd4' AND account_type = 'user')) > 0
   AND array::len((SELECT VALUE usr_id FROM user WHERE usr_id = 'usr_vspVZdfCBo2YG3DN' AND account_type = 'organization')) > 0
   AND array::len((SELECT VALUE role FROM org_membership WHERE org_user = 'usr_vspVZdfCBo2YG3DN' AND member_user = 'usr_K8r5vy1usBGWuXd4')) = 0 {
    CREATE org_membership CONTENT {
        org_user: 'usr_vspVZdfCBo2YG3DN',
        member_user: 'usr_K8r5vy1usBGWuXd4',
        role: 'owner'
    };
};

-- 最初の 1 件: weekly ship log。 Creo Memories から登録できるようになるまでの bootstrap
-- (atlas ship-log、 atl_1CfdiCfAX5XzXdYtVwsWUf、 公開)
IF array::len((SELECT VALUE usr_id FROM user WHERE usr_id = 'usr_vspVZdfCBo2YG3DN' AND account_type = 'organization')) > 0
   AND array::len((SELECT VALUE slug FROM publication WHERE publisher_usr_id = 'usr_vspVZdfCBo2YG3DN' AND slug = 'ship-log')) = 0 {
    CREATE type::record('publication', ['usr_vspVZdfCBo2YG3DN', 'ship-log']) CONTENT {
        publisher_usr_id: 'usr_vspVZdfCBo2YG3DN',
        slug: 'ship-log',
        url: 'https://app.creo-memories.in/r/atl_1CfdiCfAX5XzXdYtVwsWUf',
        title: 'Weekly Ship Log',
        app_id: 'creo-memories',
        updated_by: 'usr_K8r5vy1usBGWuXd4'
    };
};
