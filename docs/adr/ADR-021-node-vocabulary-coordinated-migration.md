# ADR-021: hub 語彙の node 移行 — VP 命名エピックへの協調追随（順序と影響範囲）

- **Status**: **Accepted**（2026-07-27 mako 裁定 — 「語彙・命名は新しい方へきっちり合わせる。一時的に動作が止まるのは一切問題ない」。詳細は §P1 裁定）
- **Date**: 2026-07-27
- **Related**: ADR-020（federation wire / registry の owner・visibility・relay）/ ADR-018（world↔hub discovery = Unison）/ ADR-011（spec SemVer）/ ADR-019（host 命名）
- **Driver**: VP v0.56.0 PR #939「境界（据え置き・意図的）」節が hub にボールを預けた
- **Memories**: `mem_1CdQxvayZBB3E768g1mDbQ`（x→y 台帳・命名の SSOT）/ `mem_1CdRjeJcbwiVm4EFDa5Q4d`（TODO ③ 協調 PR）

## Context

VP が 2026-07-27 に **v0.56.0（命名エピック、PR #936〜#946 の 10 本）** を出荷し、JoJo 由来命名を repo の生きた面から全撤去した。hub に効くのは **PR #939**（149 files / ±2.5k）で、`World` が 1 語 3 義だったものを 3 語へ分解している:

| x（旧） | y（新） | 義 |
|---|---|---|
| TheWorld / World（プロセス） | **daemon** | 常駐プロセス |
| `@world`（address 階層 87 箇所） | **@machine** | scope（今の居場所） |
| WorldId / hub_worlds / WorldEntry | **node** | federation identity（不変の番地） |

hub に関係するのは 3 番目の **node** 義だけである。ADR-020 D2 の「machine = 今の居場所 / `wld_id` = 不変の番地」という区別は、この分解でむしろ語彙として明確になった。

VP は **wire を意図的に据え置いた**。PR #939 のコミットメッセージより:

> **境界（据え置き・意図的）** — hub 実 wire: `"worlds"` channel / `"wld_id"` field / `wld_` prefix は hub.chronista.club（別デプロイ・別 repo）との共有 protocol — **協調変更として別途**。local CLI↔daemon の `"world"` key は同 binary なので `"node"` へ

結果、VP 側は型名だけ `NodeEntry` / `HubNodesCache` / `connect_and_open_nodes` に移り、ワイヤーは旧名のまま（`hub_client.rs:543` が今も `resp.get("worlds")` を読む）という **意図的な非対称** で止まっている。

### 非対称性 — VP と hub でリネームの重さが違う

VP は「local DB を初期化して終わり」で通せた（doc 54 §8.1「legacy データは初期化、migration コードを書かない」）。hub は違う:

1. **稼働中の共有サービス**である。channel 名を変えた瞬間、v0.56.0 未満の VP は `open_channel("worlds")` に失敗して federation から落ちる。
2. **spec を公開している**（`docs/spec/world-tree.kdl`、ADR-011 で SemVer 管理）。file 名・resource-type 名・scope 名は紙面上の公開契約。

したがって hub 側は「一気に置換」ではなく **順序と互換窓を設計してから**着手する。本 ADR はその順序と影響範囲を確定させ、裁定が要る点を明示するための紙である。

---

## §0 前提条件 — deploy 経路の健全性（**2026-07-27 復旧済み — 経緯を保存**）

> **RESOLVED 2026-07-27**: 真因 = **2026-07-03 の Caddyfile 全上書き incident の巻き添えで、fleet-worker-01 の `/etc/caddy/Caddyfile` から `hub.chronista.club` site block が消失**していた（edge Caddy は cert を引けず alert 80。QUIC 12879 は Caddy 非経由の直 listener なので federation は無傷 = silent outage）。修理 = backup → 正典 block（`.fleetflow/Caddyfile.live`）を **merge 追記**（全上書き禁止の原則どおり）+ SSOT ヘッダのテナント一覧に hub を追加 → `systemctl reload caddy` → TLS-ALPN-01 で cert 即時再取得（Let's Encrypt YE2、〜2026-10-25）→ `GET /health` 200 確認。apex `chronista.club` block は DNS が Cloudflare 向きのため未取込（取込むと HTTP-01 が失敗し続ける）。残置 follow-up: ① linuxbrew user の野良 `caddy run` ×3（5/25・7/12・7/13 起動、:443 未保持 = 無害だが要掃除）② host 側 Caddyfile の正典 repo（`Caddyfile.host`）にも hub block を反映しないと次の全上書きで再発する。

以下は調査時（復旧前）の観測記録:

| 観測 | 結果 |
|---|---|
| DNS `hub.chronista.club` | ✅ A → `163.43.117.17`（DEPLOY.md の記述と一致） |
| TCP 443 / 80 | ✅ 両方 open（何かが listen している） |
| HTTPS `GET /health` | ❌ **TLS handshake 失敗**。`tlsv1 alert internal error`（alert 80）/ `no peer certificate available` |
| 再現性 | ❌ LibreSSL curl・OpenSSL s_client の 2 クライアントで同一症状 |
| 証明書の有効性（crt.sh） | ✅ Let's Encrypt YE2、2026-06-28 発行 / **2026-09-26 まで有効** = **失効・期限切れではない** |
| サンドボックス網 | ✅ 正常（example.com / github.com とも 200） |
| **QUIC 12879（federation 本体）** | ✅ **生存確認（2026-07-27 追記）** — `unison_ping`（trust=system）で接続成功 = 実 CA cert を正しく提示。`unison_discover` で protocol 0.6.0 / channels `unison.discovery`+`worlds`+`relay` の schema 配信まで完動。**read-only probe のみ、register は書いていない** |

**切り分け結果（2026-07-27）: 障害は Caddy/REST（443）に限局。federation 本体は無傷。** QUIC 12879 は Caddy の後ろではない生 listener（cert を `CertSource::FromFile` で直読み）なので、Caddy が SNI cert を引けない障害の影響を受けていない — DEPLOY.md の構成どおりの独立性が実証された。なお VP 側の `vp auth login`（Creo ID JWT の再取得）とは**無関係** — alert 80 は TLS handshake（server が cert を提示する段階 = クライアント認証より前の層）で起きており、credential の有無は関与しない。7月の token outage（application 層、「Identity 後 60s 沈黙」）とは別の層・別の故障。

`alert 80` + `no peer certificate` は、**Caddy が当該 SNI の証明書を引けていない**ときの典型形である。CT ログに cert がある以上、失効ではなく Caddy 側の storage / site block を疑う。

**federation への波及は未確定**である。DEPLOY.md によれば Unison QUIC 12879 は **Caddy の後ろではない生 listener**（cert を `CertSource::FromFile` で直読み）なので、REST が死んでいても federation は生きている可能性がある。確認には `examples/live_probe.rs` を公開 hub に対して走らせる必要があるが、これは **live registry に実際に register を書く外向き操作**なので未実行。

### G0（gate）

**P0 = この切り分けを終える**こと。deploy 経路が壊れたまま移行を設計しても着地できない。

triage 手順:
1. ~~`live_probe` を実行 → QUIC 面の生死を確定~~ → ✅ **済（2026-07-27）**: register を書かない read-only probe（`unison_ping` + `unison_discover`）で代替し、QUIC 面の生存と schema 配信を確認。「**REST/Caddy のみの障害**」と切り分け完了
2. ~~fleet-worker-01 の Caddy を見る~~ → ✅ **済（2026-07-27）**: `/etc/caddy/Caddyfile` に hub の site block が**存在しない**ことを確認（root caddy の cert storage にも chronista 系 cert 無し = alert 80 と完全整合）。merge 追記 + reload で復旧（冒頭 RESOLVED 参照）
3. ~~過去の類型（`federation-token-outage-2026-07`）と同じパターンかを確認~~ → ✅ **否定（2026-07-27）**: あれは application 層（token 失効、QUIC 接続は確立して沈黙）、今回は transport 層（TLS handshake 不成立）。層も故障箇所も別

---

## §1 影響範囲 — 4 層の棚卸し

grep 実測（`world|wld_`、node_modules / target 除外）。**依存の下から上**へ並べる。

### 層 A — wire protocol（共有・**唯一の破壊面**）

`crates/chronista-hub-server/src/hub_protocol.kdl`（v0.6.0、27 hits）

| 要素 | 現在 | 破壊性 |
|---|---|---|
| channel 名 | `"worlds"` | **高** — 旧 client の `open_channel` が失敗 |
| `Discover` の returns | `WorldList { worlds: json }` | **中** — key 欠落で client がパース失敗 |
| `Register` / `Unregister` の field | `wld_id` | **中** — 省略時 fallback 有り（handle）だが routing key を失う |
| `relay` channel | `to` / `from`（値は wld_id、**field 名は中立**） | **無** — rename 不要 |
| protocol version | `0.6.0` | bump 要 |

`relay` channel が既に中立名なのは僥倖。破壊面は `worlds` channel 1 本に集中している。

### 層 B — server 実装（内部のみ・破壊性なし）

| file | hits | 主な識別子 |
|---|---|---|
| `tests/integration.rs` | 98 | test 名・assert |
| `src/unison_server.rs` | 80 | `register_channel("worlds")` / `handle_worlds` / `live_wlds` / `WorldList` |
| `src/storage.rs` | 41 | `register_world` / `unregister_world` / `list_worlds_visible_to` / `VP_WORLD_REST_GUARD` |
| `examples/worlds_demo.rs` | 37 | file 名ごと |
| `examples/registry_gc.rs` | 18 | stale entry 掃除 |
| `src/model.rs` / `config.rs` / `main.rs` / `lib.rs` / `live_probe.rs` | 各 1-2 | コメント中心 |

### 層 C — 永続層（**要注意 — ただし今なら安い。§2.1 参照**）

**実体は spec 由来のテーブルではない。** `register_world` は `hub_resource` テーブルへ書く（`storage.rs:235-242`）:

```rust
"UPSERT type::record('hub_resource', $rid) CONTENT {
    rid: $rid, type: 'vp-world', path: '/', handle: $handle, ...
```

つまり永続面の "world" は次の 2 つだけ:

| 実体 | 場所 | 影響 |
|---|---|---|
| **type 判別値 `'vp-world'`** | `storage.rs:19,236,339,344` の WHERE 句 | rename すると既存行が全 query から外れる |
| **record id prefix `vp-world:{wld_id}`** | `storage.rs:229-230,286-287` | rename すると既存行が孤児化 |
| ~~table `vp_world`~~ / `vp_actor.world_id` | `migrations/002`（spec codegen） | **federation 経路では未使用** — 影響は spec 整合のみ |

### 層 D — spec / docs（紙面の公開契約）

| file | hits | 内容 |
|---|---|---|
| `docs/spec/world-tree.kdl` | 11 | **file 名ごと** / `resource-type "vp-world"` / `slug "vp/worlds/{world_id}"` / field `world_id` / `reserved-path-slug "world"` / `child "vp/worlds"` / scope `events.publish.vp-world`・`events.delete.vp-world` |
| `docs/adr/ADR-020` | 55 | 本文（immutable — 編集せず本 ADR で追補） |
| `docs/adr/ADR-018` | 9 | **タイトルに world**（immutable） |
| `.fleetflow/DEPLOY.md` | 8 | 運用手順 |
| `README.md` / `docs/spec/README.md` / CHANGELOG / migrations doc | 各 3-5 | — |

### 爆風が届かない面（確認済み・重要）

grep 実測で、次は **実装に存在しない**（spec 紙面のみ）:

- **公開 URL** `vp/worlds/{world_id}` — Rust 側に一致ゼロ。`register_world` は `path: '/'` を書いており、この slug は未実装
- **OAuth scope** `events.publish.vp-world` / `events.delete.vp-world` — spec のみ。実際に使われている federation scope は `federation.register` 1 本（`unison_server.rs:320,397`）

→ **発行済みトークンにも公開 URL にも "world" は焼かれていない。** 移行は spec 紙面と wire に閉じる。これは当初の想定よりはるかに軽い。

---

## §2 語彙の写像（x→y 候補）

台帳（`mem_1CdQxvayZBB3E768g1mDbQ`）の「federation identity → node」を hub へ延長した案。**未裁定**。

| 層 | x | y（案） |
|---|---|---|
| A | channel `"worlds"` | `"nodes"` |
| A | `WorldList { worlds }` | `NodeList { nodes }` |
| A | field `wld_id` | `node_id`（§2.1 で別途裁定） |
| B | `handle_worlds` / `register_channel("worlds")` | `handle_nodes` / `register_channel("nodes")` |
| B | `Storage::register_world` / `unregister_world` / `list_worlds_visible_to` | `register_node` / `unregister_node` / `list_nodes_visible_to` |
| B | `live_wlds` / `VP_WORLD_REST_GUARD` | `live_nodes` / `VP_NODE_REST_GUARD` |
| B | `examples/worlds_demo.rs` | `examples/nodes_demo.rs` |
| C | type 値 `'vp-world'` / rid `vp-world:{id}` | `'vp-node'` / `vp-node:{id}` |
| D | `docs/spec/world-tree.kdl` | **要裁定** — file 名を変えるか（spec の identity そのもの） |
| D | `resource-type "vp-world"` / field `world_id` | `"vp-node"` / `node_id` |
| D | `reserved-path-slug "world"` | `"node"` を予約（`"world"` の予約解除可否は別途） |

### §2.1 `wld_` prefix — 「名前」ではなく「データ」

`wld_id` の**値**（`wld_a1b2...`）は creo EntId であり、**VP 側が発行**して hub は opaque に保持する。VP は #939 で prefix を据え置いた（113 箇所に `wld_` が残存）。

ここで **今しかない窓**がある。VP v0.56.0 は DB dir を `db/world` → `db/machine` に移し、**local DB を再初期化**した。その代償として `node_identity:self` が**再発行**される — #939 のリリースノート文言:

> node_identity 再発行 = hub 上は**新 node として再登録**（旧 entry は stale aging で消える）

つまり **hub registry の既存行は、各マシンが v0.56.0 で再登録した時点で軒並み stale になる**。層 C の rename コストが実質ゼロになる稀な窓が、今開いている。窓は各マシンが再登録して新しい行を積むほど閉じていく。

裁定が要る 2 択:

- **(a) prefix 据え置き** — 値は `wld_` のまま、field 名だけ `node_id` に。VP 現状と整合、追加コストゼロ。ただし `node_id: "wld_xxx"` という見た目の捻れが残る
- **(b) prefix も `nd_` へ** — 見た目が揃う。VP 側の ID 発行器の変更が要り、**両 repo の同時変更**になる。層 C の窓が開いている今なら、既存データを捨てるコストは払わずに済む

---

## §3 wire 移行戦略の選択肢（**要裁定**）

### W1 — 二枚看板（dual-listen / 両キー同梱）

- hub が `"worlds"` と `"nodes"` の**両 channel** を `register_channel` し、同一 handler へ流す
- `Discover` の reply に **`worlds` と `nodes` の両キー**を載せる（純 additive、旧 client 無改修で動く）
- request 側は `node_id` を読み、無ければ `wld_id` に fallback（両読み）
- 全マシンの VP が新版に揃った後、hub の次版で旧名を撤去

**利点**: 無停止。VP と hub の deploy 順序に依存しない（TODO ③ の「hub deploy → VP 追随の順」という制約自体が消える）。「他マシン VP が揃うまで federation 送信を控える」運用も不要。
**欠点**: 一時的に別名が 2 セット。撤去 PR を 1 本忘れずに打つ必要がある。

### W2 — 一斉切替（flag day）

- hub v0.4.0 を deploy し、同時に全マシンの VP を更新
- **利点**: コード負債ゼロ。最短
- **欠点**: 更新が遅れたマシンは federation から落ちる。deploy 順序が固定（hub → VP）で、その窓の間 federation が壊れる

### W3 — wire 凍結・内部のみ rename

- channel `"worlds"` / field `"wld_id"` は **protocol の固有名詞として凍結**し、層 B/C/D だけ node 語彙へ
- **利点**: 破壊性ゼロ。最も安い
- **欠点**: 「実装は node、wire は world」の捻れが恒久化する。VP 側の据え置きコメントも永久に残り、TODO ③ が閉じない

### 比較

| | 無停止 | 一貫性 | 工数 | コード負債 |
|---|---|---|---|---|
| W1 二枚看板 | ✅ | ✅（最終的に） | 中（+撤去 PR 1 本） | 一時的 |
| W2 一斉切替 | ❌ | ✅ | 小 | なし |
| W3 wire 凍結 | ✅ | ❌ | 最小 | 恒久 |

起草時の推奨は W1（無停止・deploy 順序フリー）だったが、裁定で前提が変わった — §P1 参照。

---

## §P1 裁定（2026-07-27 mako）

前提の宣言: **「vp で行った決定はこちらへも確実に合わせる。語彙・命名は新しい方へきっちり合わせる。一時的に動作が止まるのは一切問題ない」**。この宣言により W1 の唯一の優位（無停止）が要件から外れ、4 点が確定した:

| # | 論点 | 裁定 | 根拠 |
|---|---|---|---|
| 1 | wire 移行戦略 | **W2 一斉切替** | 無停止要件が明示的に放棄された。コード負債ゼロ・撤去 PR 不要・語彙が即座に一枚岩 |
| 2 | `wld_` prefix | **(b) `nd_` へ** | 「きっちり合わせる」に従う。§2.1 の窓（v0.56.0 の node_identity 再発行で既存行が stale 化）が開いている今なら data 移行コスト不要。hub は値を opaque に扱うため **hub 実装は prefix 非依存** — VP 側 issuer の変更が実体（P5） |
| 3 | spec file 名 | **`node-tree.kdl` へ rename + 旧 path に tombstone** | 語彙は新しい方へ。immutable ADR からの被リンクは旧 path に残す 1 行 tombstone（コメントのみの有効 KDL）が受ける — リンク切れゼロで両立 |
| 4 | reserved-path-slug | **`"node"` を追加予約、`"world"` も予約継続** | `/world` を解放すると handle path に取られ、旧 URL との混同・squatting の芽になる。予約継続は無コスト |

範囲外として明示: spec `vp-actor.stand` の JoJo enum（`the-world` / `star-platinum` …）は VP #945 の Stand 解体（3 義分解・総称なし）への追随が要るが、**後継語彙が role 毎に異なり本 ADR（node 義）の外**。別 migration として台帳に残す。

---

## §4 実行順序

| # | phase | 内容 | 前提 | 状態 |
|---|---|---|---|---|
| **P0** | 復旧 | §0 の TLS 切り分け → deploy 経路の健全化 | — | ✅ 2026-07-27（真因 = 7/3 incident で Caddy site block 消失。merge 追記 + reload で復旧、`/health` 200） |
| **P1** | 裁定 | §3 の W1/W2/W3 + §2.1 の prefix (a)/(b) + spec file 名の可否 | — | ✅ 2026-07-27（§P1） |
| **P2** | 紙 | 本 ADR を Accepted 化 / spec `world-tree.kdl` → `node-tree.kdl` 改訂 + CHANGELOG（ADR-011 の SemVer 判定 → pre-1.0 minor-breaking 枠、 spec 0.3.0） | P1 | ✅ 2026-07-27 |
| **P3** | 実装 | hub PR — 層 A（W2 = 旧名互換なし一斉切替、 protocol 0.7.0）+ B + C。C は §2.1 の窓が開いているうちに | P2 | ✅ 2026-07-27 |
| **P4** | 出荷 | hub version bump（Cargo.toml 一本 = `hub-v0.3.0-release-version-ssot` の SSOT に従う、 0.4.0）→ deploy → `live_probe` で live 検証 | P0, P3 | 未 |
| **P5** | 追随 | VP 側 PR — `hub_client.rs` の据え置きコメント解消・wire key を新名へ（`"nodes"` / `"node_id"`）+ **ID issuer の prefix `nd_` 化（裁定 2-(b)）**。W2 なので hub deploy と同じ窓で全マシン更新 | P4 | 未 |

P2 を P3 より前に置くのは ADR README の原則（spec = what が先、実装が追う）に従う。

---

## Consequences

### 正

- VP と hub で federation 語彙が一致し、「実装は node / wire は world」の捻れが解消する（W1 or W2 の場合）
- ADR-020 D2 の「machine = 今の居場所 / node = 不変の番地」が wire 上でも読めるようになる
- 層 C の rename を §2.1 の窓で済ませれば、データ移行コードを一切書かずに済む（VP doc 54 §8.1 と同じ手）
- 副産物として §0 の TLS 障害を発見した。移行を紙にしなければ、federation が静かに壊れたまま気付かない窓が続いた可能性がある

### 負

- **W2 は flag day** — hub v0.4.0 deploy から全マシンの VP 更新（P5）完了までの窓、federation は止まる。裁定の前提（一時停止許容）どおりの受容リスク
- ADR-018 は**タイトルに world を含む**が immutable なので改名しない。過去 ADR は当時の語彙のまま残り、新旧語彙が docs に併存する（ADR の性質上これは正しい）
- spec file 名の rename に伴う ADR からの被リンク切れは、旧 path `world-tree.kdl` の tombstone（新 path への案内コメントのみ）が受ける（裁定 3）

### 裁定済み（§P1、2026-07-27）

1. W1 / W2 / W3 → **W2**
2. `wld_` prefix → **`nd_` へ**（VP 側 issuer 変更 = P5、hub は opaque で非依存）
3. spec file 名 → **`node-tree.kdl` へ rename + tombstone**
4. `reserved-path-slug` → **`"node"` 追加、`"world"` 予約継続**

### 残 follow-up（本 ADR 外）

- spec `vp-actor.stand` の JoJo enum — VP #945 Stand 解体への追随（後継語彙が role 毎に異なる、別 migration）
- `migrations/002_resource_types_from_spec.surql` の `vp_world` table / `vp_actor.world_id` — 適用済み migration は歴史として不変。federation 経路では未使用（層 C 参照）。spec 0.3.0 からの再 codegen は次に spec 由来 migration を焼く時に自然合流

---

## References

- **VP v0.56.0 / PR #939** — `rename(daemon): World → daemon / @world → @machine / federation identity → node`（vantage-point `28983819`）。「境界（据え置き・意図的）」節が本 ADR の driver
- **ADR-020** §S2（`wld_id` + endpoints）/ §S4（relay）/ §S5（owner・visibility）/ D1・D2（hub = stateless discovery 層、location 独立 id）
- **ADR-018** — world↔hub discovery transport = Unison（channel `worlds` の初出）
- **ADR-011** — spec SemVer（pre-1.0 minor-breaking allowance）
- `mem_1CdQxvayZBB3E768g1mDbQ` — 命名 x→y 台帳（承認 SSOT）
- `mem_1CdRjeJcbwiVm4EFDa5Q4d` — TODO ②③④（マシン移行 / 本 ADR / sibling repo 走査）
- `.fleetflow/DEPLOY.md` — live 構成（REST `12880:3000/tcp` via Caddy / Unison `12879:7879/udp` 直 expose / cert = DNS-01 Cloudflare）
