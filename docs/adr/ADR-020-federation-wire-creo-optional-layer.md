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
- **hub は per-pair / per-message / per-delegation state を持たない**。hub が積む durable state は **discovery registry の O(worlds)（machine 数で bound、 `wld_id→endpoint`）のみ**で、 ユーザ/agent/メッセージ数に対しては **O(0)**（100k user/agent でも積まない）。これが「破綻しないスケール」の構造的保証。
- **hole-punch（NAT 穴あけ rendezvous）は採らない**〔VP 2026-06-27 refinement〕: hole-punch は hub に **per-pair coordination state**（candidate 交換 + simultaneous-open timing）を持たせる**唯一の機構**＝ D1 を破る。NAT は relay で解く（D3-b、 stateless in-flight）。hole-punch は冗長・可逆（将来 bandwidth 都合で要れば足せる）。
- store-and-forward は hub でなく **home-World daemon（常時 substrate / TheWorld 不死）の reconcile** が担う（既存 #596 が現にやっている）。agent session が cold でも World daemon は生きているので、 target offline 時は **送り手 World daemon が delivered=true まで再送**する。hub が預かる必要がない。
- **opt-in + degrade**: VP は `CHRONISTA_HUB_ADDR` 未設定で全 skip（machine-local）、 hub down でも machine-local 継続（VP hub_client と一致）。hub は「居れば速い・居なくても困らない」層。
- topology = trusted peer mesh（手動 peering）+ hub = optional。
- 〔緊張点1 解決 2026-06-27〕 hub stateless / store-and-forward = home-World daemon で確定（user: 「hub が持つのは避けたい・100k 来ても破綻しない」）。

### D2. 番地 = 場 namespace（**location 独立** home-World id、 machine は hub が解決）

- 場 = `agent@lane@namespace`（VP の場 namespace、 topic と**同一** namespace）。hub は独自 handle 体系を**発明しない**。
- **home-World identity = location 独立の stable-id `wld_xxx`**（EntId 風、 不変）。VP の I1「id ⟂ location」の literal 延長（namespace を git/path から、 LaneId を path/location から切ったのと**同じ手を World に一段上げる**）。canonical 番地 = `agent@lane@namespace@wld_xxx`、 **machine を含まない**。
- **handle = display（人間可読）/ wld_id = routing key**（ADR-002/008 idiom を World に踏襲）。handle↔wld_id は ADR-002 の rename/reclaim policy を再利用。
- **machine は identity でなく「今の居場所」**: hub registry = `wld_xxx → reachable endpoint(s)`（可変、 庭師 reconcile で heal）。`studio-pc:` は「wld_xxx が今 studio-pc に居る」の*解決済み view*であって canonical でない。→ World 移動・hostname 改名・衝突に番地が壊れない。
- **registry endpoint field（実装済 2026-06-28）**: `Register` が `endpoints`（`["[GUA]:port"]` 候補配列）を carry、 `Discover` が `wld_id → endpoints` を返す（protocol 0.2.0、 additive）。rid を **wld_id keyed**（`vp-world:{wld_id}`）に re-key、 handle は display 属性。hub は endpoints を **opaque な順序付き候補**として保持（dialer=VP が解釈、 relay は混ぜない別レイヤー）。registry は O(worlds) の stateless な discovery state（D1）。
- **physical 意味（physical control fleet「lane を楽器に」）は World/lane の attribute** として表す（「この World は MIDI 機材付き機械に bind」）。identity でなく property なので移動に強く、 意味も保つ。
- hub が解決するのは **wld_id → endpoint** だけ。それ以下（`agent@lane`）は target home-World が local 解決（VP の「World→SP reverse-wake 不要」発見と一致）。
- 現状の flat handle（`vp-world:{handle}`、 `storage.rs:192`、 handle=OS hostname）を **wld_id=routing key / handle=display** の形へ寄せ直す。
- 〔緊張点 解決 2026-06-27〕 location 独立（option A）で確定（user: 「A で」、 VP I1 史と一直線）。

### D3. 連邦 wire 一本 = 到達性の degrade ladder（direct → relay → store-and-forward）

**data-path = discovery → direct（確定）**〔VP 2026-06-27〕。到達手段が degrade するだけで、 別 feature ではない。degrade ladder は **全段 hub stateless**（D1）:

| 到達性 | hub の役割 | hub state |
|---|---|---|
| (a) **direct（IPv6 GUA = first-class）** | `Discover` で `endpoints`（`["[GUA]:port"]` 候補配列）を返すだけ。data は world↔world **direct** QUIC、 **hub は data 不在**。tailnet は両端にあれば使う opportunistic accelerator（**要求しない**） | discovery registry O(worlds) のみ |
| (b) **relay（universal floor）** | direct 全滅時に hub=dumb forward で中継（target World は outbound dial）。in-flight、 貯めない。relay は **advertise されない**（dialer が direct 全滅で hub relay to wld_id） | per-connection 一時のみ（durable 0） |
| (c) **target offline** | **hub は何もしない** → 送り手 home-World daemon が reconcile で再送 | 0 / 貯蔵は home-World |

- 〔council 2026-06-28〕**tailnet 依存を外した**: 「enroll 済みの相手しか繋げない」は 100k 開放と矛盾 → tailnet は opportunistic accelerator に格下げ、 **IPv6 GUA = first-class direct**（overlay 不要、 障壁は firewall のみ）。IPv4 は必要が出てから。
- **relay = universal floor**（旧「off-tailnet NAT 時に着手」を撤回）: tailnet を外すと off-overlay が常態 → relay こそ「誰でも繋がる」の本体。endpoints（direct 候補）と relay（hub mediate）は **混ぜない**別レイヤー。
- **hole-punch 不採用**（per-pair coordination state を避ける = D1）。
- canonical durable store は常に各 home-World の SurrealDB（delegation #595「World 中央 store へ canonical 直行」と同型）。hub は (a) endpoint 解決 + (b) relay の二役だけで、 どちらも durable state を積まない（O(worlds) registry のみ）。

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

- ✅ **S2 実装 + transport council（2026-06-28）**: VP が wld_id(#606)+endpoints(#607) を produce → hub が **S2 を index/Discover で consume 実装**（wld_id keyed registry、 endpoints round-trip、 e2e PASS）。council で **tailnet 依存を撤廃**（「enroll 済みだけ繋がる」は 100k 開放と矛盾）→ **IPv6 GUA = first-class direct / relay = universal floor**（旧 deferred を撤回）。→ D2/D3/ladder に反映済。次 = VP dialer（consume 側）+ hub S4 relay。
- ✅ **緊張点1（連邦 wire の store 配置）— 2026-06-27 確定**: hub = 純 stateless（durable も buffer も持たない、 O(1) state）。store-and-forward = home-World daemon reconcile。delegation authority = 各 home-World が自分の側（option 2、 projection モデル、 片方向 tell 交換）。→ D1 / D3 / D5 に反映済。
- ✅ **緊張点2（addressing — handle = machine か world か）— 2026-06-27 確定**: location 独立（option A）。home-World stable-id `wld_xxx`（id⟂location、 I1 literal 延長）= routing key、 handle = display（ADR-002/008 idiom）、 machine = 今の居場所（hub が `wld_id→endpoint` 解決）、 physical 意味 = World/lane の attribute。→ D2 に反映済。
- ✅ **緊張点3（club-nostos を hub に入れる位置）— 2026-06-27 確定**: hub payload-opaque（club-nostos 非依存）。Outcome 型は home-World 間契約、 hub は address で routing するだけ。観測は envelope の opaque metadata（kind/phase タグ）で payload 非依存。→ D5 に反映済。
- ✅ **auth-model 確定（2026-06-27）**: federation（worlds channel + 連邦 wire）の auth は **connection-level（club-unison primitive）**。authN を connection 確立時に1回（principal を `ConnectionContext` に）、 per-message は ctx principal の scope check で **per-frame 0 bytes**。mechanism = club-unison（verifier hook）/ policy = app（hub = Creo ID JWKS）。**理由 = live streaming の小フレーム要求**（per-message token は frame を膨らませ dead-end、 connection auth は「最初の数 turn だけ払い以降 0」）+ datagram も connection auth 継承 + 全 場 uniform（場×動詞×規律 の「規律 = 場-attach 時の access 規律」）。→ S3 に反映、 club-unison feature は別 ADR（mechanism は上流）。
- ✅ **transport ladder refinement（VP 2026-06-27）**: ① **hole-punch rendezvous を ladder から削除**（hub に per-pair coordination state を持たせる唯一の機構 = D1 を破る）。② **relay を NAT 正準解に昇格**（既に D3-b、 hole-punch は冗長・pro-100k・可逆）。③ **data-path = discovery→direct 確定**（同 LAN/tailnet は World 直結 = hub data 不在、 relay は off-tailnet NAT 他人のみ）。④ registry に `wld_id→endpoint` field（D2）。⑤ relay は最初の off-tailnet NAT peer 時着手で可。VP は D1-D5 背骨に全面同意、 本件は ladder のみの refinement。→ D1/D2/D3/ladder に反映済。

## Open points（残り — 設計詳細、 doctrine で方向は決定済）

1. **endpoint liveness = reconciliation**: 庭師モデル（desired=registry × actual=reachability を heal）。presence/liveness は tail-loss 許容 tier（spine durability tier）。← fork でなく実装詳細（方向は doctrine 確定）。

## 実装 ladder（段階的・急がない／「天井は高く、 実装は最小」）

- **土台（済）**: club-unison 1.1.0 → 1.3.0 bump（cert API + ALPN）。build/test 22 green/clippy clean。
- **S1 cert（済 2026-06-27）**: `spawn_unison` を `spawn_listen_with_cert` に切替、 `CHRONISTA_HUB_CERT_MODE`（dev / self-signed / file）で cert source 選択。dev default で loopback 無回帰（worlds_demo 相互 discovery 実機 PASS）。self-signed は cert DER を `CHRONISTA_HUB_CERT_OUT` に export → client は **`TrustAnchors::Custom` に cert DER を pin**（hash でなく cert そのもの — club-unison の trust model）。direct wire（D3-a）+ 非 loopback federation の前提が揃った。
- **S2 registry: wld_id + endpoints index（実装済 2026-06-28、protocol 0.2.0）**: `worlds.Register` が `wld_id`（location 独立 routing key）+ `endpoints`（direct 到達候補 `["[GUA]:port"]` 配列）を read → registry を **wld_id keyed** で upsert（rid `vp-world:{wld_id}`、 handle は display 属性）。`Discover` が wld_id + endpoints を返す。cross-transport（REST tree read）も wld_id-keyed。VP は #606(wld_id)/#607(endpoint) で **produce 済**、 hub は **consume index**。e2e PASS（wld_id/endpoints round-trip）。残: VP **dialer**（discover→direct 試行→relay fallback）が consume 側未了。
- **S3 discovery auth = connection-level（実装済 2026-06-27、club-unison 1.4.0）**: `server.enable_auth(verifier)` に hub の Creo ID verifier を policy 注入（`Vec<u8>`=JWT → `verify_user_token` → `Arc::new(Principal)`、 mechanism = club-unison の `unison.auth` channel、 `CertSource` 哲学と同型）。worlds handler が `ctx.principal()` を downcast し **`federation.register` / `federation.read`**（本 ADR が ADR-006 dotted notation で新設）を per-message gate = **per-frame 0 bytes**（datagram も connection auth を継承）。`CHRONISTA_HUB_FEDERATION_AUTH=required|permissive`（default permissive = 現 client 無回帰）で段階移行。e2e PASS（permissive 通過 / required+cred無 拒否 / required+cred 通過）。〔協調〕VP `hub_client` が `connect_with_credential` 追従後に `required` へ倒す。
- **S4 relay = universal floor（hub 側実装済 2026-07-03、protocol 0.4.0、club-unison 1.5.0）**: 〔council 2026-06-28 で旧「deferred」を撤回〕**tailnet 依存を外した**結果 off-overlay peer が常態 → relay が「誰でも繋がる」の本体（NAT の隅のレアケースではない）。hub=dumb forward / target World=outbound dial。endpoints は direct 候補のみで relay は **advertise しない別レイヤー**（dialer が direct 全滅で hub relay）。**hole-punch は採らない**（D1、per-pair state を避ける）。relay registry は `wld_id → connection ctx` の transient のみ（durable 0 = D1）。残: VP **dialer** が consume 側。
- **S5 discovery owner/visibility 分離（実装済 2026-07-04、protocol 0.5.0）**: Discover が全ユーザの world を無差別に返していた multi-user isolation 欠落の解消。**auth-model を「hub = registry の見せ方の authority」に確定**: user-jwt は身元証明のみ（`sub`=usr_id）とし、 **User principal は認証済みであること自体が federation 参加資格**（JWT の scope claim は見ない — Creo ID の scope 発行に依存せず、 `required` 反転が hub 単独で完結）。App principal（product-token = hub 発行）は従来通り scope check。
  - **D1 との整合**: D1 の「authority 不持」は **wire payload / remote-world projection** の話（authority = home-World、 D5 不変）。registry は D1 が hub に明示的に認めた O(worlds) durable state であり、 その**行の owner/visibility 属性と見せ方の policy は hub の authority**。二層 gate = 「参加できるか（authorize）」×「何が見えるか（owner/visibility）」。
  - **owner**: `worlds.Register` が connection principal の usr_id を記録（spec `resource-base.owner` を federation 経路で実装、 migration 007）。ingestion（`/v1/events`）には**配線しない**（app が任意 usr_id を名乗る spoofing 面を開かない）。
  - **visibility**: spec `resource-base.visibility` 準拠。省略時 default = **認証済 `private`（secure by default）/ 未認証 permissive `public`**（private は owner 概念なしに定義できず、 旧 client の相互発見も壊さない）。`shared` は **audience/group モデル導入までエラー（予約席）** — 将来の user group 間 messaging は group を hub 側 resource（shared の audience、 ADR-013 org namespace と接続）として実装し、 **JWT claim には乗せない**（membership 変更の即時反映）。
  - **Discover** = 自分が owner の world + public（未認証 = public のみ）。**Unregister** = owner guard（他人の world は消せない、 owner 無し legacy entry は掃除可 = stale entry 掃除の経路）。
  - **write-side owner guard（review 反映 2026-07-04、Purple Haze/Moody Blues）**: owner を enforce するのは *read*（Discover）だけでは不足。Register の UPSERT に `WHERE owner = NONE OR owner = NULL OR owner = $owner` を付け、**他人が持つ wld_id を上書きさせない**（owner/endpoints/visibility の乗っ取り = redirect/MITM + owner 剥ぎ取り→正規 Unregister、を防ぐ。 mismatch は no-op → Err）。 Unregister の未認証／App principal（usr_id を持たない）path も **owner 無し entry のみ削除可**に絞る（無条件 DELETE は permissive 経由で他人の owned world を消せるため廃止）。→ *write*（Register）と *permissive path* も owner enforce するのが §S5 の要。
  - **REST 迂回防止（review 反映 2026-07-04）**: 同じ `vp-world` 行（endpoints 入り）は auth 無しの REST `/v1/tree/@handle`・`/v1/resources/{id}` からも読める。 REST は principal を持たない（全 caller 未認証扱い）ため owner 判定できず、**public な vp-world のみ露出**して private/shared world の endpoints 漏洩を塞ぐ（`VP_WORLD_REST_GUARD`、 product resource は非対象）。 owner-aware な discovery は Unison `worlds.Discover` が正路。 REST 全体の server-side visibility enforcement は横断テーマとして別途（本 ADR scope 外）。
  - 〔協調〕VP へ wire 通知済（`019f28dc`）: credential contract は raw user-jwt のまま不変、 VP ② `connect_with_credential` 完了後に `required` 反転 → その後 stale entry（mito-mba.local 重複）掃除。
- **wire envelope + opaque payload**: 連邦 wire の envelope（宛先 + opaque payload + 観測 metadata）。payload は home-World 間契約（hub opaque、 D5）。S&F は home-World daemon（hub でなく）。
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
