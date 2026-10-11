-- 010: app.icon_url — /start に並べる各アプリの公式アイコン (裁定 2026-10-11)
--
-- Hub が勝手に印を作らず、 各アプリが自分で出している公式のアイコンを名簿に持つ。
-- live は HUB_ADMIN_KEY 未設定で管理 API が使えないので migration で入れる。
-- 既に icon_url がある行は触らない (管理 API で後から変えた値を上書きしない)。
-- GFP は公開アイコンが無い (2026-10-11 実測) ので空のまま → /start は頭文字の placeholder。
--
-- startup-migrator が `USE NS chronista DB hub;` で wrap するため USE 文は書かない。

FOR $a IN [
    { app_id: "creo-memories", icon_url: "https://creo-memories.in/creo-logo-v2.svg" },
    { app_id: "vantage-point", icon_url: "https://vantage-point.app/favicon.svg" }
] {
    UPDATE type::record('app', $a.app_id) SET icon_url = $a.icon_url WHERE icon_url = NONE;
};
