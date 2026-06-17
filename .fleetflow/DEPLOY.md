# Chronista Hub — Deploy Runbook

FleetStage stage ladder: **demo(local) → scratch → rehearsal → live(=prod)**。
deploy SSOT は Podman **Quadlet `.container`**（rootless、`systemctl --user`、creo-memories と同 idiom）。
host 命名 `{service}.{stage}.chronista.club`（stage は live で省略、ADR-019）。

---

## scratch（`hub.scratch.chronista.club`）

deploy 先: **fleet-worker-01 相乗り**（anycreative tenant、rootless = linuxbrew user）。

構成（このリポの `.fleetflow/`）:
- image: `ghcr.io/chronista-club/chronista-hub-server:latest`（stage 非依存。`build-images.yml` が main push で push）
- Quadlet: `chronista-hub-scratch-server.container`
- env: `.env.scratch.server.template`
- Caddy: `Caddyfile.scratch`
- ports: REST `12780:3000/tcp`（Caddy 経由）/ Unison `12779:7879/udp`（直 expose）
- federation addr: **`hub.scratch.chronista.club:12779`**（UDP）

### 前提
- fleet-worker-01 への SSH（linuxbrew rootless user）
- `op`（1Password CLI、FleetFlowVault）— admin key を使う場合のみ
- chronista.club の DNS 操作権限
- `#16` merge 済 → GHCR に image が push されていること（`build-images.yml` 完了を確認）

### 手順

**0. DNS（手元）**
```
A  hub.scratch.chronista.club  → <fleet-worker-01 public IP>
```

**1. port 衝突確認（fleet-worker-01）** — 相乗りなので先に確認
```bash
ss -tlpn | grep -E '12780' || echo "tcp 12780 free"
ss -ulpn | grep -E '12779' || echo "udp 12779 free"
```
衝突したら `.fleetflow/*.container` と `Caddyfile.scratch` の port を空き番号へ。

**2. image pull**
```bash
podman login ghcr.io   # private package の場合のみ (GH PAT: read:packages)
podman pull ghcr.io/chronista-club/chronista-hub-server:latest
```

**3. ディレクトリ + env（fleet-worker-01、linuxbrew user）**
```bash
mkdir -p ~/chronista-hub/data/scratch
# scratch は no-auth sandbox なので secret 無しなら template を素コピーで可:
cp <repo>/.fleetflow/.env.scratch.server.template ~/chronista-hub/.env.scratch.server
# admin key 等の op:// 参照を有効化した場合のみ:
# op inject -i <repo>/.fleetflow/.env.scratch.server.template -o ~/chronista-hub/.env.scratch.server
```

**4. Quadlet 配置 + 起動**
```bash
cp <repo>/.fleetflow/chronista-hub-scratch-server.container \
   ~/.config/containers/systemd/chronista-hub-scratch-server.container
loginctl enable-linger "$USER"        # logout 後も user service を生かす（初回のみ）
systemctl --user daemon-reload
systemctl --user start chronista-hub-scratch-server.service
systemctl --user status chronista-hub-scratch-server.service --no-pager
```

**5. firewall**
```bash
# inbound: TCP 80/443 (Caddy)、UDP 12779 (Unison)。REST 12780 は localhost のみ（公開しない）
# 例 (ufw): sudo ufw allow 80,443/tcp ; sudo ufw allow 12779/udp
```

**6. Caddy 取り込み**
`Caddyfile.scratch` の site block を fleet-worker-01 の Caddy へ（共有 Caddy なら `import`、専用なら mount）→ Caddy reload。Caddy が Let's Encrypt で `hub.scratch.chronista.club` の TLS を自動取得。

### 検証
```bash
# REST (Caddy 経由 HTTPS)
curl -fsS https://hub.scratch.chronista.club/health        # → {"status":"ok",...}
# Unison federation (VP 側): daemon に env set + 再起動
#   CHRONISTA_HUB_ADDR=hub.scratch.chronista.club:12779
# → 実 world が register/discover、REST tree にも出る:
curl -fsS https://hub.scratch.chronista.club/v1/tree/<world-handle>
```

### ログ / 運用
```bash
journalctl --user -u chronista-hub-scratch-server.service -f
systemctl --user restart chronista-hub-scratch-server.service
```

### ロールバック
```bash
systemctl --user stop chronista-hub-scratch-server.service
rm ~/.config/containers/systemd/chronista-hub-scratch-server.container
systemctl --user daemon-reload
```

### 注意
- scratch = **no-auth sandbox**（`STUB_AUTH_ALLOWED=true`）。機密データを置かない。
- データは `~/chronista-hub/data/scratch`（embedded RocksDB）。捨てて良い前提。
- 実 auth に上げる場合は `.env` の `STUB_*` を外し `CREO_ID_*` + JWKS を設定。

---

## live（`hub.chronista.club`）

scratch と同じ deploy 形態（fleet-worker-01 相乗り、rootless Quadlet）。命名 ADR-019 で
live は subdomain prefix 省略。config skeleton は揃っているが、**deploy 前に code 対応が 3 件**ある。

構成（`.fleetflow/`）:
- Quadlet: `chronista-hub-live-server.container`
- env: `.env.live.server.template`（**実 Creo ID JWKS auth**、`op inject` 必須）
- Caddy: `Caddyfile.live`（`hub.chronista.club` + apex `chronista.club` portal / `/@handle` alias proxy）
- ports（scratch と非衝突）: REST `12880:3000/tcp` / Unison `12879:7879/udp`
- federation addr: **`hub.chronista.club:12879`**

### live の残課題（federation と public-web で切り分け）

> **portal = pure Caddy（app 実装を入れない方針）**: apex `chronista.club` は Caddy の
> `reverse_proxy`（`/@handle` → hub）+ 静的 landing のみ。app 側ロジックは一切持たず、
> すべて hub service 側に置く（`/@handle` の中身も hub が出す）。→ 「portal service」は不要。

| 用途 | 残 code gap |
|---|---|
| **live federation**（VP ↔ hub）| **#1 Unison cert のみ** — dev self-signed（SkipVerification）→ proper cert / TrustAnchors。hub に `CertSource` 未実装（ADR-018 の負債）。auth/port/deploy は config 済 |
| **live public-web**（`chronista.club/@handle` brand URL）| **#2 hub の `/@handle` public route** — 現状 `/v1/tree/{handle}` のみ。canonical = `hub.chronista.club/@handle`、apex は Caddy alias proxy（config 済）、landing も Caddy（`file_server`）|

→ live **federation** を回すだけなら gap は **#1（cert）のみ**。public-web（handle ページ）は
**#2（hub app 側）**が要るが、federation とは独立した別トラック。

### 手順（上記 3 件が解決した後）
scratch の手順と同型。差分のみ:
- `.env.live.server` は **`op inject` 必須**（`HUB_ADMIN_KEY` + 実 auth）。stub にしない。
- DNS: `hub.chronista.club` + `chronista.club`（apex portal）の A record → fleet-worker-01。
- firewall: TCP 80/443 + **UDP 12879**。
- Caddy: `Caddyfile.live`（2 ホスト分）を取り込み。
- 検証: `curl https://hub.chronista.club/health` / VP daemon に `CHRONISTA_HUB_ADDR=hub.chronista.club:12879`。

### 注意
- live = 実データ・実 auth。scratch のように捨てられない。
- cert が proper になるまで federation を公開しない（INSECURE 回避）。
