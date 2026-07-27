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
# → 実 node が register/discover、REST tree にも出る:
curl -fsS https://hub.scratch.chronista.club/v1/tree/<node-handle>
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
| **live federation**（VP ↔ hub）| ✅ **cert gap 解消**（ADR-020 §S1 で CertSource 配線、`.env.live` の `CHRONISTA_HUB_CERT_MODE`）。register/discover/auth/node_id/endpoints（S2/S3）も code 済。✅ **2026-06-28 公開越し疎通実証済**（self-signed + pin）。残るは **cert 永続化 = 実 CA cert 発行（DNS-01、Step 1）→ file mode 切替**（VP は System trust = pin 不要に） |
| **live public-web**（`chronista.club/@handle`）| #2 hub の `/@handle` public route が未（federation とは独立した別トラック）|

→ live **federation** の安定運用に残るのは **op 設定（実 CA cert 発行 + file mode 切替、Step 1）** のみ。

### 接続の principal = IPv6 GUA（ADR-020 D3 / council 2026-06-28）
tailnet 依存は外した。federation の direct data-path は **IPv6 GUA**（overlay 不要、cross-internet 直結）。**ただし IPv6 GUA principal は node↔node direct の話** — nodes が自身の GUA を advertise（S2 で hub が交換）し peer 同士が直結する。**hub の住所ではない**: hub は discovery/relay の合流点で、nodes が到達できれば IPv4 で足りる。direct 全滅時は hub relay（S4、未実装）。

> **🟢 デプロイ決定（2026-06-28, mito）= hub は IPv4 で立てる**。fleet-worker-01 は Sakura 共有セグメント＝**IPv4-only**（`163.43.117.17`、IPv6 GUA 無し。usacloud で確認済）。node 間は IPv6 GUA direct で動くので **hub は IPv4 で OK**（node 的には IPv6 で federation する）。現行 DNS は **`A hub.chronista.club → 163.43.117.17`** のみ（AAAA 不要）。hub 自身の IPv6 化（Sakura router+switch ~¥2-3k/月 / Fly.io）は IPv6-only peer 完全対応が要る時の **将来 option**（nodes 無改修で差し替え可）。

### Step 0（現行 IPv4）. DNS + 到達性
```bash
# DNS (CF token で設定): hub は IPv4 のみ
#   A  hub.chronista.club → 163.43.117.17    (REST/Caddy + Unison QUIC 両方)
# firewall (fleet-worker-01): UDP 12879 (Unison) + TCP 80/443 (Caddy)
#   例(ufw): sudo ufw allow 12879/udp ; sudo ufw allow 80,443/tcp
# 到達 smoke:
nc -uvz hub.chronista.club 12879 ; curl -fsS https://hub.chronista.club/health
```

### （将来 option）hub 自身を IPv6 GUA 化する場合（Sakura router+switch / Fly.io）
> 下記は hub を IPv6 到達可能にしたくなった時用（IPv6-only peer 対応）。現行 IPv4 deploy には不要。
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

### Step 1. cert（実 CA = Let's Encrypt、file mode）★決定 2026-06-28 mito

**狙い**: hub は LE の実 CA cert を `CertSource::FromFile` で出し、client（VP / live_probe）は
`TrustAnchors::System`（webpki-roots = Mozilla bundle、ISRG Root 含む）で検証する。
→ **cert 配布も pin も不要。cert が回っても client 無変更**（self-signed の「restart で cert
変化 → pin 済 client 弾かれる」ephemeral 脆さを構造的に根絶）。

cert 取得は **DNS-01（Cloudflare）**。12879/udp の QUIC listener は Caddy の後ろではない（生 QUIC）
ので HTTP-01 ではなく DNS-01 で取り、PEM を `CertSource::FromFile` に直接食わせる。DNS-01 は
TXT レコードだけで完結し inbound port を一切使わない（QUIC listener を乱さない）。

> ⚠️ **順序**: 先に cert を発行して `data/live` に置く → その後で `.env.live.server` を file mode に。
> cert files 不在のまま file mode で起動すると `FromFile` が読めず起動失敗する。未発行の間は
> `.env` の self-signed fallback（template のコメント行）で立てておく。

```bash
# 前提: A hub.chronista.club → 163.43.117.17 は設定済 (Step 0)。CF token = shared-cloudflare
#       (@CreoMemories、Zone:DNS:Edit for chronista.club)。fleet-worker-01 = linuxbrew rootless。

# 1. acme.sh 導入（linuxbrew user、rootless。daily 更新 cron も自動設置）— 初回のみ
curl https://get.acme.sh | sh -s email=mito@chronista.club
export PATH="$HOME/.acme.sh:$PATH"   # or: source ~/.bashrc

# 2. CF token を env に（acme.sh が CF API で _acme-challenge TXT を立てる）
export CF_Token="<shared-cloudflare token>"      # token 単体で zone auto-detect 可

# 3. 発行（DNS-01、LE 本番）。RSA-2048 = 最も枯れた chain（R10/R11 → ISRG Root X1）
acme.sh --issue --dns dns_cf -d hub.chronista.club --server letsencrypt
#   ec-256 にするなら末尾に --keylength ec-256（chain は E1/E2 → ISRG Root X2、両方 bundle 在）

# 4. install-cert: data/live に fullchain+key を deploy し、reloadcmd で hub restart
#    ⚠️ cron は user-session env を持たない → systemctl --user 用に XDG_RUNTIME_DIR を明示注入
acme.sh --install-cert -d hub.chronista.club \
  --fullchain-file ~/chronista-hub/data/live/unison-cert.pem \
  --key-file       ~/chronista-hub/data/live/unison-key.pem \
  --reloadcmd "export XDG_RUNTIME_DIR=/run/user/\$(id -u); systemctl --user restart chronista-hub-live-server.service"
#   --ecc を付ける（ec-256 で発行した場合のみ install 時も --ecc が要る）
```

更新（90日）は acme.sh の daily cron が残量 <30日で自動 renew → reloadcmd で hub restart →
`FromFile` が新 cert を再読込（hub は起動時に cert を resolve するので restart で反映、hot-reload は不要）。
linger=yes 前提（Step 2-5）。cron でなく systemd `--user` timer に寄せたい場合は
`acme.sh --cron` を叩く `.timer`/`.service` を `~/.config/systemd/user/` に置く。

### Step 2-5. deploy（rootless、 tailscale SSH で実施可。 2026-06-28 実績）
- `.env.live.server`: template から生成。`HUB_ADMIN_KEY` は op inject で入れる or **省略可**（未設定 = admin API 404 で無効、 federation には不要）。実 Creo ID JWKS auth（stub にしない）。
- `mkdir -p ~/chronista-hub/data/live` → Quadlet を `~/.config/containers/systemd/` に配置 → `systemctl --user daemon-reload && systemctl --user start chronista-hub-live-server`。`loginctl enable-linger`。
- **⚠️ stale-image 罠**: podman は local に `:latest` cache が在ると**再 pull しない**。quadlet に **`Pull=newer`** を入れてあるので restart で最新を取得。手動なら `podman pull ...:latest` → restart（2026-06-28 に古い `:latest` を掴み federation コード無しで起動 → cert ログ/起動順序の不在で検知）。
- **firewall（ufw、 sudo = root login 要）**: `sudo ufw allow 12879/udp`（Unison federation）。`80,443/tcp` は許可済（Caddy）、 `tailscale0` 全許可。**`12880/tcp` は開けない**（REST は Caddy or host loopback）。default deny(incoming) なので **12879/udp 未許可だと QUIC が沈黙**（TCP timeout / UDP の `nc -uz` は DROP でも誤 success に注意）。
- Caddy（REST HTTPS / handle ページ、 federation には不要）: `Caddyfile.live` 取り込み → reload。

### 検証（federation 疎通）

**実 CA cert（file mode）後 = System trust、pin なし**（本線）:
```bash
# REST health（host loopback。 Caddy 公開後は https://hub.chronista.club/health）
ssh linuxbrew@<host> 'curl -fsS http://localhost:12880/health'
# cert が実 CA か確認（chain が LE / SAN=hub.chronista.club であること）
echo | openssl s_client -connect hub.chronista.club:443 2>/dev/null | openssl x509 -noout -issuer -subject  # ※REST/Caddy 側
# 公開越しに pin なしで register/discover round-trip（HUB_CERT を渡さない = TrustAnchors::System）
HUB_ADDR=hub.chronista.club:12879 \
  cargo run -p chronista-hub-server --example live_probe
#   → ✓ QUIC connect (System trust) / ✓ Register / ✓ Discover
# VP 実 node: daemon に CHRONISTA_HUB_ADDR=hub.chronista.club:12879 のみ（cert 配布も pin も不要）
```

**self-signed fallback 期（cert 未発行）= Custom pin**:
```bash
ssh linuxbrew@<host> 'cat ~/chronista-hub/data/live/unison-cert.der' > hub-cert.der
HUB_CERT=hub-cert.der HUB_ADDR=hub.chronista.club:12879 \
  cargo run -p chronista-hub-server --example live_probe   # → ✓ (cert pin)
```

> 🧹 `live_probe` は `nd_liveprobe` を register する（live registry に残る無害なテスト entry）。
> 疎通確認後に掃除する（registry の delete/expire path 整備時にまとめて）。

### QUIC liveness 自動回復（issue #35、2026-07-07）

rootless podman の UDP port-forward（pasta）が連続稼働で劣化し、**QUIC(12879) の受付だけ
停止**する事象があった（REST は生きたまま = `/health` では検知できない）。2 層で自動回復する:

```bash
# 全 unit を配置（repo .fleetflow が正、drift 防止）
scp .fleetflow/chronista-hub-live-restart.{service,timer} \
    .fleetflow/chronista-hub-live-quic-probe.{service,timer} \
    linuxbrew@fleet-worker-01:~/.config/systemd/user/
ssh linuxbrew@fleet-worker-01 'systemctl --user daemon-reload \
  && systemctl --user enable --now chronista-hub-live-restart.timer \
  && systemctl --user enable --now chronista-hub-live-quic-probe.timer'
```

- **検知 + 自動回復（`quic-probe`）**: 5 分ごとに image 同梱の `quic_probe` を
  `podman run --network=host --entrypoint quic_probe` で回し、`localhost:12879`（pasta 経路）
  へ実 QUIC 接続。retry 3 回とも失敗 = 受付停止 → `OnFailure` で `restart.service` を呼び
  passt を作り直す。**probe は image に載る**ので `Dockerfile` 変更後の image 反映が前提。
- **予防（`restart`）**: 04:00 JST daily で先回り restart（劣化前に passt 再生成）。

手動確認: `podman run --rm --network=host --pull=never --entrypoint quic_probe -e HUB_ADDR=localhost:12879 ghcr.io/chronista-club/chronista-hub-server:latest`（exit 0 = QUIC 生存）。
`systemctl --user list-timers` で両 timer の次回発火を確認。

### 注意
- live = 実データ・実 auth（Creo ID JWKS）。scratch のように捨てられない。
- **federation auth = `required`（2026-07-04 反転済み）**。未認証は Register/Discover/relay すべて拒否。接続には `VP_OIDC_AUDIENCE=https://hub.chronista.club` 付きの `vp auth login` が必須。stale 掃除は `examples/registry_gc.rs`（owner guard 準拠の手動 Unregister）。
- **full 疎通**（node A → node B の direct/relay dial）は **VP dialer + hub S4 relay** 待ち。本 deploy は register/discover/auth/cert を実ネットで先行検証する段（**公開 IPv4 到達 + cert pin path を 2026-06-28 実証済**。node↔node の IPv6 GUA direct は nodes 側）。
