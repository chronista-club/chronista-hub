# ADR-011: Spec versioning — SemVer with pre-1.0 minor-breaking allowance

- **Status**: Accepted
- **Date**: 2026-04-25
- **Gap**: G10

## Context

v0.1 spec は `meta { version "0.1" }` のみで forward migration の規則、 breaking change handling、 codegen artifact の version pinning が無い。 spec evolution の changelog / migration guide の場所も未定義。

## Decision

### SemVer 準拠 (pre-1.0 例外)

```
MAJOR.MINOR.PATCH

MAJOR: breaking changes
MINOR: additive
PATCH: 文言 / clarification
```

- pre-1.0 (0.x.0) の MINOR は breaking 許容 (industry convention)
- 1.0 以降は strict SemVer

### Breaking 判定基準 (機械的に classifier)

🔴 MAJOR: field 削除 / rename / required 化 / type 変更、 resource-type 削除 / rename、 scope / endpoint 削除 / rename、 **default 挙動変更** (silent breaking 注意)

🟢 MINOR: optional field 追加、 endpoint / scope / resource-type 追加、 enum value 追加 (open enum 前提)

🟢 PATCH: description / comment / ordering 変更

### Open enum convention

新 enum value 追加は MINOR、 client は graceful unknown variant で対応:
- Rust: `#[serde(other)] Unknown(String)`
- TS: `type X = "a" | "b" | (string & {})`
- Ruby: symbol with default fallback

spec lint で `extensible #true` default。

### Multi-version coexistence

- 3 major version 並走、 sunset 通知 6 ヶ月
- URL prefix 完全分離 (`/v0/`, `/v1/`, `/v2/`)

### Codegen artifact pinning

各 SDK に spec version embedded constant:
- TS: `export const SPEC_VERSION = "0.2.0"`
- Rust: `pub const SPEC_VERSION: &str = "0.2.0"`
- Ruby: `ChronistaHub::SPEC_VERSION = "0.2.0"`
- SurrealQL: `migrations/NNN_spec_0_2_0.surql`

### Runtime version check

```
Header: X-Chronista-Spec-Version: 0.2.0
```

- major mismatch → 400
- client newer → 200 + Warning header

### Spec evolution dashboard

```
docs/spec/
  world-tree.kdl
  CHANGELOG.md            # Keep a Changelog format
  migrations/0.1-to-0.2.md
  archive/world-tree-v0.1.kdl  # immutable snapshot
```

### MAJOR bump → ADR 必須

各 major (1.0, 2.0, ...) は **dedicated ADR** + migration guide doc。

### spec lint

```
version-bump-required-on-breaking-change   error
changelog-entry-required                   error
migration-guide-required-on-major          error
archive-snapshot-required-on-release       error
```

## Consequences

### 正

- "default 挙動変更を MAJOR breaking" の明示は CREO-127 型の silent breaking 予防の構造化
- Open enum convention で polyglot (Rust strict / JS loose) consumer の互換性確保
- 3 major 並走 + 6 ヶ月 sunset で consumer migration cost 抑制
- archive snapshot で過去 client が想定した spec を後から確認可能

### 負

- spec change ごとに lint / changelog / archive を run する CI 仕事が増える (codegen と同居で軽減)
- pre-1.0 の breaking minor は外部 consumer に注意喚起が要 (release notes に明記)

## 却下案

- **strict SemVer from start** — pre-1.0 の rapid evolution が困難、 0.x の業界慣例と乖離
- **Closed enum default** — exhaustive match 文化 (Rust/Kotlin) には合うが、 polyglot で MINOR breaking 多発
- **Single live version (no parallel)** — consumer migration window がない、 migration friction 大

## References

- Memory: `mem_1CaQUXVemqZV8qqmLQ6HGr`
- Gap list: `mem_1CaP3fUFz5DKW3DzJP1jGf`
- spec: `world-tree.kdl` `spec-versioning`, `meta.changelog-url`
- 関連 ADR: ADR-001 (monorepo strategy で artifact 同 repo)、 ADR-009 (manifest version compatibility 同 policy)
- 外部規格: SemVer 2.0, Keep a Changelog
