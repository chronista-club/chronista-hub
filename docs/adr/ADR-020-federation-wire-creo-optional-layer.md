# ADR-020: 連邦 wire — Creo discovery/relay の optional 層（federation transport doctrine）

- **Status**: Proposed（VP federation は park 中、 着手は VP 再開と同期。 hub 側は土台を先行で育てられる）
- **Date**: 2026-06-27
- **Doctrine SSOT**: VP Transport 哲学（`mem_1CcRw6kSu9Jr3ejhZ4ALUJ`、 doc 27 §3.4）/ VP spine §8 federation（`mem_1CcBVgNRhWLy9vZdTmAAt6`）/ doc 27 §62「全通信 unison channel」/ doc 28 §5.3「federation = 番地解決だけが変わる」
- **Related**: ADR-018（world↔hub discovery = Unison）/ ADR-006（scope）/ ADR-002（Creo ID identity）/ ADR-019（host 命名）/ #12（multi-hub federation, deferred）
- **Driver**: VP→hub handoff `mem_1CcTJTbyhuTry5ACNvyJhG`

## Context

VP の agent 委譲（delegate/respond/complete + nostos 三相 Outcome + World 中央 durable store + Push/Pull self-heal）は **v1 ローカルで出荷済・実機自走済**。federation は doc 28 §5.3「**動詞・state machine・Outcome は不変、 変わるのは番地解決だけ**」に立つ。

本 ADR は federation を「hub に機能を足す」ではなく、**VP doctrine を hub 跨ぎに延長する**視点で据える。鍵は VP spine §8 の定義:

> federation = **home-World single authority + projection** + **連邦 wire 一本（address-routed・store-and-forward）** + remote = 一 backing-kind。 presence は home World にだけ繋ぐ。 topology = trusted peer mesh（手動 peering）+ **Creo を discovery/relay の optional 層**。

つまり **chronista-hub = この「Creo の discovery/relay の optional 層」**。authority は持たない。

### hub 現状棚卸し（v0.1.0、 ADR-018）

- ✅ **registry**: `worlds.Register`/`Discover`（`unison_server.rs` / `storage::register_world`）。cross-transport（REST `/v1/tree/{handle}` にも出る）。ただし Discover は `{handle, name, registered_at}` のみで **到達可能 addr を持たない**（`storage.rs:204`）。
- ❌ **rendezvous / relay / store-and-forward**: 未実装（grep 一致なし）。
- 🟡 **identity 境界**: REST data plane は完備（`auth.rs`）。**Unison discovery surface は無認証**（`unison_server.rs:37` の handler が ctx を捨てる）。
- ✅ **cert blocker は上流解決済**: club-unison **1.2.0** が hub federation のために `spawn_listen_with_cert`（`CertSource`）を追加。hub は本 ADR で **1.1.0 → 1.3.0 へ bump 済**（1.3.0 は raw QUIC ALPN `"unison"` も追加、 Apple `NWProtocolQUIC` interop）。

## Decision

連邦 wire を **home-World 群の間に張る一本の wire** とし、 hub をその **discovery/relay を担う optional 層** とする。hub は authority も canonical store も持たない。

### D1. hub = optional discovery/relay 層（**純 stateless**、 authority 不持）

- spine §8「Creo を discovery/relay の optional 層」を hub の正準定義とする。**remote world は projection、 authority は home World**（「1場1authority」の World スケール相似）。
- **hub は durable も buffer も持たない（純 stateless）**。hub の state は **ユーザ/agent 数に関係なく O(1)**（100k でも 1M でも hub は何も積まない）。これが「破綻しないスケール」の構造的保証。
- store-and-forward は hub でなく **home-World daemon（常時 substrate / TheWorld 不死）の reconcile** が担う（既存 #596 が現にやっている）。agent session が cold でも World daemon は生きているので、 target offline 時は **送り手 World daemon が delivered=true まで再送**する。hub が預かる必要がない。
- **opt-in + degrade**: VP は `CHRONISTA_HUB_ADDR` 未設定で全 skip（machine-local）、 hub down でも machine-local 継続（VP hub_client と一致）。hub は「居れば速い・居なくても困らない」層。
- topology = trusted peer mesh（手動 peering）+ hub = optional。
- 〔緊張点1 解決 2026-06-27〕 hub stateless / store-and-forward = home-World daemon で確定（user: 「hub が持つのは避けたい・100k 来ても破綻しない」）。

### D2. 番地 = 場 namespace（**location 独立** home-World id、 machine は hub が解決）

- 場 = `agent@lane@namespace`（VP の場 namespace、 topic と**同一** namespace）。hub は独自 handle 体系を**発明しない**。
- **home-World identity = location 独立の stable-id `wld_xxx`**（EntId 風、 不変）。VP の I1「id ⟂ location」の literal 延長（namespace を git/path から、 LaneId を path/location から切ったのと**同じ手を World に一段上げる**）。canonical 番地 = `agent@lane@namespace@wld_xxx`、 **machine を含まない**。
- **handle = display（人間可読）/ wld_id = routing key**（ADR-002/008 idiom を World に踏襲）。handle↔wld_id は ADR-002 の rename/reclaim policy を再利用。
- **machine は identity でなく「今の居場所」**: hub registry = `wld_xxx → reachable endpoint(s)`（可変、 庭師 reconcile で heal）。`studio-pc:` は「wld_xxx が今 studio-pc に居る」の*解決済み view*であって canonical でない。→ World 移動・hostname 改名・衝突に番地が壊れない。
- **physical 意味（physical control fleet「lane を楽器に」）は World/lane の attribute** として表す（「この World は MIDI 機材付き機械に bind」）。identity でなく property なので移動に強く、 意味も保つ。
- hub が解決するのは **wld_id → endpoint** だけ。それ以下（`agent@lane`）は target home-World が local 解決（VP の「World→SP reverse-wake 不要」発見と一致）。
- 現状の flat handle（`vp-world:{handle}`、 `storage.rs:192`、 handle=OS hostname）を **wld_id=routing key / handle=display** の形へ寄せ直す。
- 〔緊張点 解決 2026-06-27〕 location 独立（option A）で確定（user: 「A で」、 VP I1 史と一直線）。

### D3. 連邦 wire 一本 = 到達性の degrade ladder（direct → relay → store-and-forward）

rendezvous/relay/buffer は**別 feature でなく、同じ一本の wire の到達手段が degrade するだけ**。しかも degrade ladder は **全段 hub stateless**（D1）:

| 到達性 | hub の役割 | hub state |
|---|---|---|
| (a) **direct** — reachable | endpoint を渡すだけ（rendezvous）。data は world↔world direct QUIC | stateless |
| (b) **relay** — NAT / direct 不可、 target は online | in-flight live relay（packet 転送、 貯めない） | stateless |
| (c) **target offline** | **hub は何もしない** → 送り手 home-World daemon が reconcile で再送（D1） | stateless / 貯蔵は home-World |

同じ「場への tell/ask」が、 到達できなければ degrade するだけ。canonical durable store は常に各 home-World の SurrealDB（delegation #595「World 中央 store へ canonical 直行」と同型）。hub は (a) endpoint 解決 + (b) live relay の二役だけで、 どちらも state を積まない。

### D4. transport = unison channel（REST は過渡期）

- doctrine 終着 = doc 27 §62「全通信 unison channel」。federation surface は **unison channel**（場 subscribe + tell/observe/ask）で作る。cross-world read も REST `/v1/tree` でなく **場を ask/observe する unison channel**。
- 動詞 tell/observe/ask を 1 stream で native に（Unison channel = Event + Request/Response）。規律 = QoS で stream 分離（discovery=control と wire data は別 stream、 「1 channel に全部寄せない」）。
- 規律 §3.4.4「1 connection / N streams-by-QoS / 1 protocol」を hub↔world にも適用。
- hub の REST 2系統（ADR-018）は過渡期。federation は全 unison で作り、 既存 REST ingestion/read の移行は別 decision に切る。

### D5. wire payload = nostos 三相 Outcome（**hub には opaque**）/ authority = 各 home-World が自分の側

- 連邦 wire が運ぶのは単なる discovery でなく、 **delegation の durable cross-agent future**。payload の型 = nostos `Outcome<O, I, E>`（Done / **Reborn**=NeedsInput / Failed）。**この型は home-World ↔ home-World の契約**。
- machine を跨いでも agent の旅（delegate → Reborn → respond → complete）が途切れない。**これが「Agent の可能性を飛躍させる」構造的核**。hub はこれを *可能にする*（relay）が *理解はしない*（enable ≠ understand）。
- **authority モデル = 各 home-World が自分の側を authoritative に持つ（projection モデル、 spine §8 literal）**。A→B の委譲なら:
  - A（World-A）の **outbox**「B に委譲・待ち」 = A の home-World が authority
  - B（World-B）の **inbox**「A からのタスク・自分の進捗」 = B の home-World が authority
  - wire は **片方向 tell の交換**（A→B = タスク、 B→A = nostos Outcome）。**同じ record を co-edit しない**。各々が自分の半分の真実を持ち、 相手へ tell して **ack されるまで home-World reconcile が再送**。
  - 帰結: **どの World も他 World の agent の authority を持たない → 水平スケール**（B の進捗書込が送り手 World の hotspot にならない）。VP 既存の wire（home-World 中央 store + reconcile-until-ack）を World 跨ぎに 2 本並べただけ＝新機構ゼロ。
- **hub は payload-opaque = club-nostos に依存しない**。wire message = `{宛先 address + 不透明 payload}`。hub は address で routing するだけで Outcome の中身を覗かない。club-nostos は **VP 側だけの依存**（型契約は home-World 間）。payload schema が進化しても **hub 無改修**＝ D1 の dumb pipe / 水平スケールを守る。
- **観測は payload でなく envelope の opaque metadata で**: envelope に小さな routing タグ（例 `kind=delegation, phase=reborn`）を載せ、 hub はそのタグだけ見て metrics を取る。Outcome の中身（タスク内容・結果）は blind のまま（郵便局が封筒の種別スタンプは見るが中身は読まない）。
- canonical durable store は各 home-World（delegation #595 = World 中央 SurrealDB）、 hub は純 relay（stateless、 D1）。`lane_query_for`（wire→lane 翻訳 = federation 不変条件の swappable 層）の hub 跨ぎ版は **home-World 側**に居る（hub でなく）。
- 〔緊張点1 解決 2026-06-27〕 authority = option 2（各 home-World が自分の側）で確定（user: 「2 で行こう」）。
- 〔緊張点3 解決 2026-06-27〕 hub payload-opaque（club-nostos 非依存、 観測は envelope metadata）で確定（user: 「opaque で進めて」）。

## 解決済の緊張

- ✅ **緊張点1（連邦 wire の store 配置）— 2026-06-27 確定**: hub = 純 stateless（durable も buffer も持たない、 O(1) state）。store-and-forward = home-World daemon reconcile。delegation authority = 各 home-World が自分の側（option 2、 projection モデル、 片方向 tell 交換）。→ D1 / D3 / D5 に反映済。
- ✅ **緊張点2（addressing — handle = machine か world か）— 2026-06-27 確定**: location 独立（option A）。home-World stable-id `wld_xxx`（id⟂location、 I1 literal 延長）= routing key、 handle = display（ADR-002/008 idiom）、 machine = 今の居場所（hub が `wld_id→endpoint` 解決）、 physical 意味 = World/lane の attribute。→ D2 に反映済。
- ✅ **緊張点3（club-nostos を hub に入れる位置）— 2026-06-27 確定**: hub payload-opaque（club-nostos 非依存）。Outcome 型は home-World 間契約、 hub は address で routing するだけ。観測は envelope の opaque metadata（kind/phase タグ）で payload 非依存。→ D5 に反映済。

## Open points（残り — 設計詳細、 doctrine で方向は決定済）

1. **endpoint liveness = reconciliation**: 庭師モデル（desired=registry × actual=reachability を heal）。presence/liveness は tail-loss 許容 tier（spine durability tier）。← fork でなく実装詳細（方向は doctrine 確定）。

## 実装 ladder（段階的・急がない／「天井は高く、 実装は最小」）

- **土台（済）**: club-unison 1.1.0 → 1.3.0 bump（cert API + ALPN）。build/test 22 green/clippy clean。
- **S1 cert（済 2026-06-27）**: `spawn_unison` を `spawn_listen_with_cert` に切替、 `CHRONISTA_HUB_CERT_MODE`（dev / self-signed / file）で cert source 選択。dev default で loopback 無回帰（worlds_demo 相互 discovery 実機 PASS）。self-signed は cert DER を `CHRONISTA_HUB_CERT_OUT` に export → client は **`TrustAnchors::Custom` に cert DER を pin**（hash でなく cert そのもの — club-unison の trust model）。direct wire（D3-a）+ 非 loopback federation の前提が揃った。
- **S2 rendezvous**: `worlds` channel に `endpoints` 追加（場→host→endpoint）。protocol 0.1.0 → 0.2.0、 両側 codegen 同期。direct QUIC（D3-a）成立。
- **S3 discovery auth**: ADR-006 に `federation.register`/`federation.read` 追加、 worlds channel を Creo ID token で認証（D1 の identity 境界）。
- **S4 relay**: address-routed relay（D3-b）。
- **S5 store-and-forward**: transient buffer（D3-c）。
- **S6 nostos payload**: 三相 Outcome を wire に載せる（D5）。
- **終着**: federation surface 全 unison channel 化（D4、 REST は漸進撤去）。

> ⚠️ cross-repo: 1.3.0 で raw QUIC ALPN `"unison"` が必須化。hub は server 側で ALPN を出すため、 **VP hub_client も 1.3.0 追従が実機 federation の前提**（両端で同 label を negotiate）。

## Consequences

### 正
- hub の正体が doctrine で一意に定まる（optional discovery/relay 層、 authority 不持）。倒れても machine-local が動く degrade が構造から導かれる。
- rendezvous/relay/buffer が「一本の wire の degrade ladder」に畳まれ、 concept の dilemma が消える（doctrine が言う「正しい徴」）。
- 番地が VP の場 namespace と同一 → hub が独自体系を再発明せず、 VP の wire-unison 移行（B-4）とそのまま地続き。
- federation が nostos 三相 Outcome を運ぶ → machine 跨ぎの durable cross-agent future が成立（Agent の飛躍）。

### 負
- protocol version bump で VP との両側同期コスト（codegen + hub_client 改修 + ALPN 追従）。
- cert/TrustAnchors/hash-pinning が hub・client 両側に必要（ADR-018 負債の返済）。
- 全 unison 化は ADR-018 の REST 2系統からの漸進移行コストを将来抱える（別 decision に切る）。

## 却下案
- **hub に機能・durable store を集約**: spine §8「optional 層」「home-World single authority」に正面から反する。hub を SPOF 化する。
- **v2(rendezvous)/v3(relay+buffer) を別 feature に分割**: 連邦 wire 一本の degrade ladder を二分する。doctrine 純正でない（store-and-forward は wire の本質的性質）。
- **REST で federation discovery/read を手書き**: doc 27 §62「全通信 unison channel」に反する。ADR-018 で既に却下した路線の再来。
- **hub 独自 handle namespace**: 場 namespace の再発明、 「1場1authority」を壊す。

## References
- doctrine: Transport 哲学 `mem_1CcRw6kSu9Jr3ejhZ4ALUJ` / spine §8 `mem_1CcBVgNRhWLy9vZdTmAAt6` / wire-unison 移行 `mem_1CcTJPrZRjY4qmPxKAsFpY` / delegation `mem_1CcT6r9YDdC31wsvbjyqNo` / rebuild L0 portless `mem_1CcRkUQDpHa2g1u6dCm3Mf`
- handoff: `mem_1CcTJTbyhuTry5ACNvyJhG`（VP→hub 要求）/ federation 統合 plan: `mem_1Cc1dA79VZu586fjqafiBS`
- 版数（2026-06-27）: club-unison `1.3.0`（1.2.0=cert spawn、 1.3.0=ALPN）/ club-nostos `0.2.0`（三相 Outcome、 hub 未導入）
- 実装: `crates/chronista-hub-server/src/{unison_server.rs, storage.rs, hub_protocol.kdl, auth.rs, config.rs}`
- 関連 ADR: ADR-018 / ADR-006 / ADR-002 / ADR-011（SemVer）/ ADR-019。VP 側: doc 27 §3.4/§62、 doc 28 §5.3/§7
