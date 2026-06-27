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

### live federation の状態（cert gap 解消、 IPv6 GUA が principal）

> **portal = pure Caddy**: apex `chronista.club` は `reverse_proxy`（`/@handle` → hub）+ 静的 landing のみ。app ロジックは hub service 側。「portal service」不要。

| 用途 | 状態 |
|---|---|
| **live federation**（VP ↔ hub）| ✅ **cert gap 解消**（ADR-020 §S1 で CertSource 配線、`.env.live` の `CHRONISTA_HUB_CERT_MODE`）。register/discover/auth/wld_id/endpoints（S2/S3）も code 済。残るは **op 設定（IPv6 GUA + cert 配布、Step 0/1）** のみ |
| **live public-web**（`chronista.club/@handle`）| #2 hub の `/@handle` public route が未（federation とは独立した別トラック）|

→ live **federation** を回すのに残るのは **op 設定（IPv6 GUA 到達性 + cert 配布）** のみ。

### 接続の principal = IPv6 GUA（ADR-020 D3 / council 2026-06-28）
tailnet 依存は外した。federation の direct data-path は **IPv6 GUA**（overlay 不要、cross-internet 直結、障壁は firewall のみ）。VP は hub を **hostname `hub.chronista.club`（AAAA → GUA）で dial** → cert SAN は安定 DNS 名で済む（動的 GUA を IP-SAN に焼かない）。direct 全滅時は hub relay（S4、未実装）。

### Step 0. IPv6 GUA 到達性（最優先 — ここが principal）
```bash
# (a) fleet-worker-01 が安定 IPv6 GUA を持つか
ip -6 addr show scope global | grep inet6          # 2000::/3 の GUA があること
# (b) DNS: hub.chronista.club に AAAA(+A) を張る
#     AAAA  hub.chronista.club → <fleet-worker-01 の GUA>   (Unison/federation の principal)
#     A     hub.chronista.club → <fleet-worker-01 の IPv4>  (REST/Caddy 用)
# (c) IPv6 firewall: Unison UDP を v6 で開放
#     例(ufw): sudo ufw allow 12879/udp              # v4/v6 両方
#     v6 確認: sudo ip6tables -L -n | grep 12879
# (d) 到達 smoke (手元 → GUA へ UDP)
nc -6 -uvz hub.chronista.club 12879 || echo "UDP/v6 到達不可 — DNS/firewall 見直し"
```

### Step 1. cert（self-signed quickstart）
hub は起動時に self-signed cert（SAN=`hub.chronista.club`）を生成し DER を
`~/chronista-hub/data/live/unison-cert.der` に export（`CHRONISTA_HUB_CERT_OUT`）。
VP 側はこの DER を取得し `TrustAnchors::Custom` に pin（hostname dial なので SAN 検証も通る）。
→ public scale（「誰でも繋がる」）に上げる時は `.env.live` を `CHRONISTA_HUB_CERT_MODE=file`
+ 実 CA cert にし、VP は System trust（cert 配布不要）。

### Step 2-5. deploy（scratch と同型、 差分のみ）
- `.env.live.server` は **`op inject` 必須**（`HUB_ADMIN_KEY` + 実 Creo ID JWKS auth）。stub にしない。
- port 衝突確認（`12880/tcp`, `12879/udp`）。image pull。`mkdir -p ~/chronista-hub/data/live`。
- Quadlet 配置 → `loginctl enable-linger` → `systemctl --user daemon-reload && start`。
- Caddy: `Caddyfile.live`（hub.chronista.club + apex）取り込み → reload（REST の HTTPS/LE）。

### 検証（federation 疎通）
```bash
curl -fsS https://hub.chronista.club/health                       # REST (Caddy/HTTPS)
# VP daemon: hub を hostname で dial (AAAA → GUA) + cert DER を Custom pin
#   CHRONISTA_HUB_ADDR=hub.chronista.club:12879
# → 実 world が register(wld_id+endpoints)/discover、REST tree にも出る:
curl -fsS https://hub.chronista.club/v1/tree/<world-handle>
```

### 注意
- live = 実データ・実 auth（Creo ID JWKS）。scratch のように捨てられない。
- **federation auth** は当面 `permissive`（VP が `connect_with_credential` で Creo ID JWT を出すまで）。VP 追従後に `CHRONISTA_HUB_FEDERATION_AUTH=required` へ。
- **full 疎通**（world A → world B の direct/relay dial）は **VP dialer + hub S4 relay** 待ち。本 deploy は register/discover/auth/cert を実ネットで先行検証する段（IPv6 GUA 到達 + cert path の実証）。
