-- 004: Reserved handle sentinel seed (G12.8 / ADR-013)
--
-- Phase 1 release 前に、 squatting 防止のため重要 handle を sentinel record として
-- 事前投入する。 これらの handle は `account_type=reserved` で claim 不可。
--
-- 由来: ADR-013 G12.8、 spec `world-tree.kdl` の `handle-namespace.reserved-handles`
-- 関連 memory: mem_1CaQaWhLANCtbAdGMfi6un (G12 organization)
-- 関連 ADR: docs/adr/ADR-013-organization-shared-namespace.md
--
-- NS / DB: chronista / hub
-- 適用前提: 002_resource_types_from_spec.surql 適用済 (user table SCHEMAFULL + account_type)
-- startup-migrator が `USE NS chronista DB hub;` で wrap するため USE 文は書かない。
--
-- 注意: user table は codegen 由来の SCHEMAFULL で payload field のみ
-- (handle / email / display_name / avatar_url / bio / locale / account_type)。
-- resource-base 共通 field (created_at / updated_at 等) は現状 table に無いため
-- CONTENT に含めない (含めると SCHEMAFULL で reject される)。

-- ============================================================
-- 前提検証: user table に account_type field が存在しなければ停止
-- ============================================================
LET $account_type_def = (INFO FOR TABLE user).fields.account_type;
IF $account_type_def IS NONE {
    THROW "user.account_type field not yet defined; apply 002_resource_types_from_spec first";
};

-- ============================================================
-- Reserved handle sentinel records (idempotent: 既存 handle は skip)
-- ============================================================
-- 注意: 空の SELECT 結果は [] (≠ NONE)。 existence は array::len で判定する。
FOR $handle IN [
    "chronista-club", "chronista", "creo-memories", "creo",
    "vantage-point", "vp", "cplp", "fleetstage", "fleetflow",
    "gfp", "go-fast-packing", "chronista-hub"
] {
    LET $existing = (SELECT id FROM user WHERE handle = $handle LIMIT 1);
    IF array::len($existing) = 0 {
        CREATE user CONTENT {
            handle: $handle,
            email: "system@chronista.club",
            display_name: $handle,
            account_type: "reserved",
            locale: "ja"
        };
    };
};

-- ============================================================
-- Verification (post-migration)
-- ============================================================
LET $reserved_count = (SELECT VALUE count() FROM user WHERE account_type = "reserved" GROUP ALL)[0] ?? 0;
IF $reserved_count < 12 {
    THROW "Expected at least 12 reserved handles after seed, got " + <string> $reserved_count;
};
