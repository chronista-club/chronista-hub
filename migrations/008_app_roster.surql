-- 008: app — 家族のアプリの名簿 (ADR-009 の段階 1、 ADR-023 追記)
--
-- `app` table は 002 (spec `resource-type "app"` の codegen) で定義済みで、 これまで空だった。
-- 段階 1 では管理 API (`PUT /v1/apps/{app_id}`) で登録し、 `/start` と `GET /v1/apps` が
-- ここを読む。 manifest の取得 (`.well-known/chronista-hub-manifest`)、 scope の承認、
-- 24h refresh は段階 2。
--
-- record id = type::record('app', app_id)。 一意性は record id が持つので index は張らない
-- (live の `app` に既存行があっても migration が失敗しないように)。
--
-- startup-migrator が `USE NS chronista DB hub;` で wrap するため USE 文は書かない。

-- ログインを始める URL。 `/start` が link にする。 無いアプリ (VP など CLI でログインする
-- もの) は `/start` の一覧に出ない。
DEFINE FIELD OVERWRITE login_url ON app TYPE option<string>
  COMMENT 'ログインを始める URL (https)。 /start の link 先 — ADR-023'
  PERMISSIONS FULL;

-- 002 (spec) では必須だが、 段階 1 は管理 API で登録するので manifest を持たないアプリがある
-- (各アプリの well-known はまだ配信されていない)。 段階 2 で取得を始めたら必須に戻す。
DEFINE FIELD OVERWRITE manifest_url ON app TYPE option<string>
  COMMENT 'ADR-009 の .well-known/chronista-hub-manifest。 段階 1 では任意'
  PERMISSIONS FULL;

-- ============================================================
-- 家族のアプリを seed (既にある行は触らない)
-- ============================================================
FOR $a IN [
    {
        app_id: "creo-memories",
        name: "Creo Memories",
        description: "記憶と Atlas",
        home_url: "https://app.creo-memories.in",
        login_url: "https://app.creo-memories.in/auth/login",
        status: "active"
    },
    {
        app_id: "gfp",
        name: "GFP",
        description: "Go Fast Packing",
        home_url: "https://app.gfp.works",
        login_url: "https://app.gfp.works/auth/login",
        status: "active"
    },
    {
        app_id: "vantage-point",
        name: "Vantage Point",
        description: "端末で `vp auth login` を実行してログイン",
        status: "active"
    }
] {
    LET $existing = (SELECT id FROM type::record('app', $a.app_id));
    IF array::len($existing) = 0 {
        CREATE type::record('app', $a.app_id) CONTENT $a;
    };
};
