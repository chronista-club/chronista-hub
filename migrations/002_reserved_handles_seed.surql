-- 002: Reserved handle sentinel seed (G12.8 / ADR-013)
--
-- Phase 1 release 前に、 squatting 防止のため重要 handle を sentinel record として
-- 事前投入する。 これらの handle は `account_type=reserved` で claim 不可。
--
-- 由来: ADR-013 G12.8、 spec `world-tree.kdl` の `handle-namespace.reserved-handles`
-- 関連 memory: mem_1CaQaWhLANCtbAdGMfi6un (G12 organization)
-- 関連 ADR: docs/adr/ADR-013-organization-shared-namespace.md
--
-- NS / DB: chronista / hub
-- 適用前提: 001_hub_schema.surql 適用済 + user table が存在 (AC-15+ で field 定義予定)

USE NS chronista;
USE DB hub;

-- ============================================================
-- Reserved handle sentinel records
-- ============================================================
-- account_type="reserved" → user / organization 双方として claim 不可。
-- 後続の AC-* で user table が SCHEMAFULL + account_type field を持つ前提。
-- account_type field が無い段階では本 migration を skip / hold。

-- 検証: user table に account_type field が存在しなければエラーで停止
LET $has_account_type_field = (
    SELECT VALUE name FROM ONLY (INFO FOR TABLE user).fields
    WHERE name = 'account_type'
);
IF $has_account_type_field IS NONE THEN
    THROW "user.account_type field not yet defined; apply schema migration first"
END;

-- 投入 (idempotent: 既存 handle の record があれば overwrite せず skip)
FOR $handle IN [
    "chronista-club", "chronista", "creo-memories", "creo",
    "vantage-point", "vp", "cplp", "fleetstage", "fleetflow",
    "gfp", "go-fast-packing", "chronista-hub"
] {
    LET $existing = (SELECT VALUE id FROM user WHERE handle = $handle LIMIT 1);
    IF $existing IS NONE THEN
        CREATE user CONTENT {
            handle: $handle,
            email: "system@chronista.club",
            display_name: $handle,
            account_type: "reserved",
            locale: "ja",
            created_at: time::now(),
            updated_at: time::now()
        }
    END
};

-- ============================================================
-- Verification (post-migration)
-- ============================================================

-- 期待値: 12 reserved record (重複なしを前提)
LET $reserved_count = (SELECT count() AS c FROM user WHERE account_type = "reserved" GROUP ALL).c;
IF $reserved_count < 12 THEN
    THROW "Expected at least 12 reserved handles after seed, got " + $reserved_count
END;

-- 表示用 (オプション、 manual run 時の確認)
SELECT handle, account_type, created_at FROM user WHERE account_type = "reserved" ORDER BY handle;
