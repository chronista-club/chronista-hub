# ADR-019: Host 命名スキーム + apex portal + canonical = hub.chronista.club

- **Status**: Accepted (scratch で実装着手、 live canonical 切替は live 構築時)
- **Date**: 2026-06-17
- **Supersedes**: ADR-012 の canonical URL 部分（`chronista.club/@{handle}` → `hub.chronista.club/@{handle}`）。 ADR-012 の subdomain 永久 reserve 方針は維持。
- **Related**: ADR-011 (@handle namespace) / ADR-018 (Unison federation transport) / FleetStage stage ladder

## Context

chronista-hub を FleetStage で **demo → scratch → rehearsal → live(=prod)** と段階 deploy する。 host 命名・ apex `chronista.club` の役割・ canonical handle URL を確定する必要がある。

- ADR-012 は `chronista.club/@{handle}` を canonical、 `{handle}.chronista.club` を永久 reserve とした。
- user 意向: **apex は portal** にしたい。 creo の `{service}.{domain}` 慣習 (`app.` / `mcp.`)。

## Decision

### D1. host 命名スキーム
**`{service}.{stage}.chronista.club`、 stage は live で省略**。
- portal: apex `chronista.club`（特別扱い）
- hub @ live: `hub.chronista.club`
- hub @ scratch: `hub.scratch.chronista.club` / rehearsal: `hub.rehearsal.chronista.club`
- demo は local（loopback/LAN、 FleetStage 管理外）

OR の `api.dev.objectrecords.io`（`{service}.{stage}.{domain}`）先例と同型。

### D2. apex = portal、 canonical = hub.
- apex `chronista.club` = **portal**（landing）。 `/@{handle}` を hub へ **alias proxy**（Caddy reverse_proxy）。
- **canonical = `hub.chronista.club/@{handle}`（正式）**。 `chronista.club/@{handle}` は便宜 alias。
- `rel=canonical` / RFC 7807 error URL / sitemap は hub. を指す。
- → ADR-012 の canonical を本決定で supersede。

### D3. service 面に自己完結
- hub は `hub.chronista.club` に自己完結: REST `/v1` + handle pages `/@handle` + Unison federation + admin。
- federation addr（VP `CHRONISTA_HUB_ADDR`）: `hub.{stage}.chronista.club:<udp-port>`。

### D4. 予約 subdomain
`hub` / `scratch` / `rehearsal` / `portal` 等の stage・service slug は **handle ではない予約 subdomain**（ADR-012 の `status` と同列）。 wildcard `*.chronista.club` → 404 は維持。

### D5. transport 露出（ADR-018 と整合）
- REST(TCP): Caddy が HTTPS 終端 → reverse_proxy。
- Unison(QUIC/UDP): Caddy を通さず host UDP port を直 expose。

## Consequences

### 正
- hub が単一 host に自己完結（API + handle + federation）、 apex は純 portal。 命名が DNS / Caddy / federation addr で一貫。
- live=apex 省略 / 下位 stage=prefix の非対称で stage が一目で分かる。
- brand URL `chronista.club/@handle` は alias で温存（portal proxy）。

### 負
- canonical 変更（apex → hub.）で SEO / 被リンクの正準が動く（`rel=canonical` で吸収）。
- apex portal が `/@handle` を proxy する Caddy 設定が要る。
- per-stage に subdomain + DNS + cert が要る。

## 却下案
- **apex canonical 据え置き（ADR-012 のまま）**: apex が `/@handle` を canonical 提供。 だが apex を portal にしたい意向と two-role になり複雑。
- **全部 hub. に集約（portal なし）**: apex 遊休、 brand landing を持てない。
- **非ブランドドメインに stage 隔離**: 別ドメイン管理が増える。

## References
- 実装: `.fleetflow/`（fleet.kdl / chronista-hub-scratch-server.container / .env.scratch.server.template / Caddyfile.scratch）、 `Dockerfile`、 `.github/workflows/build-images.yml`
- 関連 ADR: ADR-011（@handle）/ ADR-012（subdomain reserve — canonical 部分を本 ADR が supersede）/ ADR-018（Unison federation transport）
- memory: `mem_1Cc1dA79VZu586fjqafiBS`
- FleetFlow 前例: creo-memories `.fleetflow/`（Unison server quadlet）
