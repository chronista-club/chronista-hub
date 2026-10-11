-- 011: GFP をいったん閉じる (mako 2026-10-11「GFPはいったん閉じておこう」)
--
-- 名簿の行は残し、 status を active → pending に落とす。 /start と GET /v1/apps は active だけを
-- 出すので一覧から消える。 再開は status を active に戻すだけ (管理 API か migration)。
-- 既に active でない行 (管理 API で変えた後など) は触らない。
--
-- startup-migrator が `USE NS chronista DB hub;` で wrap するため USE 文は書かない。

UPDATE type::record('app', 'gfp') SET status = 'pending' WHERE status = 'active';
