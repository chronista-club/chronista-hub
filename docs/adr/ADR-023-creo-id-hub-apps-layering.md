# ADR-023: Creo ID / Chronista Hub / 各アプリの分担 — アプリを作るたびにアカウントを増やさない

- **Status**: Accepted（2026-10-08 mako 裁定「OK」で、Q1 / Q2 とも推奨案 A。Q1 は A ができるまで B でつなぐ）
- **Date**: 2026-10-08
- **Supersedes**: ADR-002 の「handle の claim は Creo ID に委譲」の部分（D3）。ADR-002 のそれ以外（rename / reclaim / canonicalization の方針）は有効
- **Related**: ADR-002 / 008 / 009 / 010 / 013 / 019 / 020、creo-memories の canonical rule（`mem_1CbMiGk28cSeQ1BjB6P7hq`）
- **Driver**: creo-memories lead からの handoff（`mem_1CfpH5EKiEaDWFr22cGtF5`、todo `mem_1CfpH5qKPKJM2tnarpgskd`）

## Context

mako（2026-10-08、原文）:

> 私としてはアプリを作成するたびにアカウントを増やしたくなくて、CreoIDを作ったというね。
> 家族とか、そういうrelationshipは、Chronista Hubの方に寄せようかな？

Creo ID（Auth0 tenant `anycreative`、入口 `id.creo-memories.in`）はすでに家族のアプリ（Creo Memories / GFP / VP / fleetstage / CPLP …）の共通ログインになっている。
Hub の README（「Creo ID: auth only」）と ADR-002 / 008 / 009 / 013 も、ログインは Creo ID、つながりは Hub という線で書かれている。
ただしそれは Hub の中の個別の決定で、家族全体の原則としてはどこにも書かれていない。そのため次のものが宙に浮いている。

- tenant 全体に効く設定の持ち主。Auth0 の Default Login Route（cookie 無しでログイン画面を開いた人のやり直し先）は `https://gfp.works/` になっていると強く推定される。Creo の利用者がこの経路に落ちると GFP の紹介ページで止まる
- 新しいアプリを Creo ID につなぐときに、やること・やらないこと
- canonical rule（iss / aud を家族で 1 つに固定し、アプリの区別は scope でする）から外れているアプリの扱い。Hub 自身もその 1 つ

## Decision

### D1. 3 層に分ける

| 層 | 持ち主 | 持つもの | 持たないもの |
|---|---|---|---|
| **誰か** | Creo ID | ログイン、本人確認、ログイン方法の紐づけ（Google / Apple / メールを 1 人にまとめる） | handle、どのアプリを使っているか、アプリのデータ |
| **つながり** | Chronista Hub | 家族のアプリの名簿（ADR-009）、その人がどのアプリを使っているか、organization と所属（ADR-013）、handle（D3）、アプリをまたぐリンク（`/@{handle}/...` の tree） | ログイン、アプリの中身、課金 |
| **中身** | 各アプリ | そのアプリのデータ、権限、課金。利用者の記録は Creo ID の利用者を鍵にして、初めて来たときに作る | 独自のアカウント、独自のログイン画面 |

「家族」はアプリの家族を指す。人と人の関係（Atlas の共有など）は、そのアプリの「中身」に残す。

### D2. ログインを Hub に依存させない

- 各アプリへのログインは **Creo ID だけで完結する**。Hub が止まっても、どのアプリにもログインでき、そのアプリの中身も使える
- Hub は「つながりを見せる場所」で、関所にしない。アプリはログインの途中で Hub を呼ばない
- Hub が止まっている間に欠けてよいのは、アプリをまたぐ表示（`/@{handle}` の tree、他のアプリへのリンク、federation の discovery）だけ
- アプリから Hub への登録（`POST /v1/events`、federation の Register）は、Hub が戻ったときにやり直せば済む形にする（ADR-004 の再送、ADR-020 D1 の「居れば速い・居なくても困らない」と同じ）

### D3. handle の持ち主は Hub

handoff で合意した線引きに合わせ、handle は「つながり」の層に置く。ADR-002 の「claim は Creo ID に委譲し、Hub は `user.created` を mirror する」をこの部分だけ置き換える。

- 理由: Creo ID の実体は Auth0 で、handle を持つ場所も、Hub に `user.created` を送る仕組みも無い。ADR-002 の前提が実在しない
- ADR-002 の rename（90 日の soft redirect）・reclaim（180 日 cooldown）・canonicalization の方針は、持ち主を Hub に読み替えてそのまま使う
- 鍵は Creo ID の `sub`。今の Hub は `sub` をそのまま利用者の鍵にしている（例 `google-oauth2|…`）。ADR-002 / 008 が想定した `usr_` の EntId は発行されていない。Hub 側で EntId を振るかは、名簿と handle を実装するときに決める

### D4. tenant 共通の設定は「誰か」の層（＝家族）の持ち物

Auth0 tenant `anycreative` 全体に効く設定は、特定のアプリの持ち物にしない。

- Default Login Route（やり直し先）、ログイン画面の見た目と文言、tenant の表示名、ログイン方法の有効・無効、custom domain
- 変更は mako の判断で行い、記録は Hub の docs（本 ADR とその後継）に置く。各アプリの repo には置かない
- やり直し先は **Q1 の A**（chronista.club の受け口）。それができるまでは B（Creo Memories の Web）

### D5. 新しいアプリを Creo ID につなぐ手順

やること:

1. Auth0 tenant `anycreative` に **application（client）を 1 つ足す**。callback / logout / web origin はそのアプリの URL
2. 共通の audience `https://id.anycreative.tech` に、そのアプリの **scope を足す**（`<app>:read` など）。client にその scope を許可する
3. アプリの verifier を canonical rule に合わせる: iss = `https://id.creo-memories.in/`、aud に `https://id.anycreative.tech` を含むこと、必要な scope を endpoint ごとに確かめる
4. アプリは初めて来た利用者の記録を `sub` を鍵にして自分で作る
5. Hub につなぐ場合は、manifest（ADR-009）を出して product-token（ADR-017）を受け取る

やらないこと:

- tenant を増やさない
- audience を増やさない
- アプリ独自のログイン画面やアカウント登録を持たない
- ログインの途中で Hub を呼ばない（D2）

### D6. canonical rule との関係

canonical rule（iss / aud を 1 つに固定し、アプリの区別は scope）は D5 の手順の中身として本 ADR に取り込む。2026-10-08 時点で外れているもの:

| アプリ | 外れ方 | 扱い |
|---|---|---|
| **Hub 自身** | aud `https://hub.chronista.club`（Auth0 に Hub 専用の API がある） | **Q2 の A** で共通の aud へ寄せる |
| VP | aud `https://api.vantage-point.app` | VP の判断。Hub にログインする VP は Hub の aud に合わせて token を取っている（Q2 の影響を受ける） |
| fleetflowd | 入口が素の Auth0 domain、aud `https://api.fleetstage.cloud` | fleetflow 側の判断。M2M なら canonical rule の適用外 |
| Creo demo | 入口が素の Auth0 domain | creo-memories 側で直す |
| Creo MCP | Auth0 でログインした後、自前の token を発行し直す（iss `https://mcp.creo-memories.in`） | MCP の仕様上の事情。Creo ID の利用者を鍵にしていれば D1 と矛盾しない |
| GFP の CI | 入口が素の domain で audience が無い | GFP 側の課題（本番の設定とずれている） |

各 API が scope を実際に確かめているかは未確認。

## 分岐と裁定（2026-10-08、mako「OK」= 推奨案）

### Q1. やり直し先（Default Login Route）をどこにするか

前提: やり直し先はログインを「始め直せる」URL でなければならない。Auth0 はそこへ `iss=` を付けて送る（OIDC の third-party initiated login と同じ形、handoff の調べ）。

| 案 | 中身 | 良い点 | 気になる点 |
|---|---|---|---|
| **A. chronista.club に受け口を作る（推奨）** | Hub に `iss=` 付きで来た人へ、家族のアプリの一覧を見せる受け口を作る。押したアプリのログインへ送る | 中立。「あなたの Creo ID で使えるアプリ」という Hub の役割と合う。Hub は Auth0 の client を持たなくてよい（D2 を守れる） | 実装が要る。apex `chronista.club` は今 Cloudflare の portal で、`/@{handle}` の proxy もまだ無い。受け口を apex と `hub.` のどちらに置くかも決める |
| B. Creo Memories の Web に向ける | 管理画面の設定だけ | 今すぐ変えられる。利用者がいちばん多い | 中立ではない。GFP の利用者が Creo に落ちる、という逆向きの同じ問題 |
| C. 今のまま（gfp.works） | 何もしない | 手間が無い | Creo の利用者が GFP の紹介ページで止まる問題が残る |

**裁定: A**。A ができるまでの間は B にして、利用者の多い側の迷子を減らす。Auth0 の設定変更は管理画面での作業で、mako が行う。

### Q2. Hub 自身の aud をどうするか

Hub は aud を一覧で持てる（`CREO_ID_AUDIENCES`、ADR-010）。

| 案 | 中身 | 良い点 | 気になる点 |
|---|---|---|---|
| **A. 共通の aud も受け付け、順に寄せる（推奨）** | `CREO_ID_AUDIENCES` に `https://id.anycreative.tech` を足す。VP のログインを共通の aud に寄せ終えたら `https://hub.chronista.club` を外す | env の追加だけで壊れない。最終的に canonical rule に揃う | VP 側の変更が要る。Hub の権限を scope で表す約束（`hub:*` など）を作る必要がある。共通の API でも VP CLI に refresh_token が出ることを先に確かめる（下記の注意） |
| B. Hub 専用の aud を正式な例外にする | 今のまま | 手間が無い。Hub の API を Auth0 上で独立させておける | 家族の中で Hub だけ別の token が要る。新しいアプリが Hub を呼ぶたびに token を取り分ける |
| C. 一度に切り替える | aud を共通だけにし、VP と同時に出す | 早く揃う | VP と Hub の同時リリースが要る。切替の当日に token が取れないと federation が止まる |

**裁定: A**。

注意: 2026-07-07〜11 に federation が 4 日間止まった件の原因は、VP の token の失効と refresh_token が出ていなかったこと。refresh_token が出る条件の 1 つは Auth0 の API（resource server）側の `allow_offline_access` で、今は Hub 専用の API に設定してある。aud を共通に寄せるときは、共通の API にも同じ設定があることと、VP CLI の client に refresh_token の grant があることを確かめてから切り替える。

## 今 live で使えるもの（2026-10-08 実測）

README の Phase 表は Linear 時代（AC-14〜18）のままだったので、ここに現状を書く（README の Status はここを指す）。

| 機能 | live（`hub.chronista.club`、v0.5.0） | 備考 |
|---|---|---|
| node registry と federation（Unison / QUIC） | **使える** | VP の node 21 件。Register / Discover / relay。Creo ID の user-jwt で認証が必須 |
| tree read（`GET /v1/tree/{handle}`、`/v1/resources/{id}`） | **使える** | 中身は VP の node だけ |
| events の取り込み（`POST /v1/events`） | 動くが**使われていない** | 発行済みの product-token は 0 件。取り込まれた event も 0 件 |
| アプリの名簿（ADR-009） | **無い** → v0.7.0 で段階 1 | 管理 API で登録し、`/start` と `GET /v1/apps` が読む。manifest の取得は段階 2（ADR-009 の 2026-10-09 追記） |
| 利用者の名簿・handle の claim | **無い** → v0.8.0 で claim まで | `GET /v1/me` で名簿に載り `usr_` EntId が付く。`PUT /v1/me/handle` で claim、`GET /v1/users/@{handle}` が公開（2026-10-09 追記）。rename はまだ |
| organization（ADR-013） | **無い** | spec 上の予約のみ |
| `/@{handle}` のページ | **無い** | `hub.` も apex も 404 |
| apex `chronista.club` | 静的な portal（Cloudflare） | Hub への proxy は無い |
| Creo Memories との同期（AC-18） | **無い** | live に Creo の resource は無い |

つまり Hub は今、VP の federation の土台としてだけ live で働いている。Q1 の A により、やり直し先の受け口が `/@{handle}` より先に Hub の最初の利用者向けページになる。

## Consequences

### 正

- 「アプリを作るたびにアカウントを増やさない」が家族の原則として 1 か所に書かれる。新しいアプリは D5 をなぞれば済む
- Hub が止まってもログインと各アプリの中身は止まらない（D2）。Hub は失敗しても困らない層のまま育てられる
- tenant 共通の設定に持ち主ができ、GFP の都合で Creo の利用者が迷子になる類の事故を防げる
- handle の持ち主が実在する場所（Hub）に移り、ADR-002 の前提の穴が塞がる

### 負

- Hub に名簿と handle の実装が要る → v0.8.0 で claim まで実装（2026-10-09 追記）。rename / reclaim はまだ書面上
- `sub` を鍵にしているので、Auth0 でログイン方法の紐づけ先（primary）を取り違えると、同じ人が別人として見える
- Q1 の A と Q2 の A は、どちらも Hub と VP に実装の仕事を生む

## 却下案

- **Hub をログインの入口にする**（Hub が OIDC の client になり、各アプリは Hub 経由でログインする） — Hub が止まると全アプリのログインが止まる。D2 と矛盾する
- **自前の認可サーバ（creo-memories design 25、repo `creo-id`）を再開する** — Auth0 の上で「アカウント 1 つ」はすでに達成できている。2026-04-25 から骨組みで止まっており、止めたままにする
- **アプリごとに tenant を分ける** — アカウントが増える。mako の出発点と逆

## 次の作業（本 ADR の結果）

| 作業 | 担当 | 状態 |
|---|---|---|
| Auth0 の Default Login Route を Creo Memories の Web に向ける（Q1 のつなぎ） | mako（Auth0 管理画面） | 済（2026-10-08 確認: `app.creo-memories.in/auth/login?iss=…` へ 302） |
| `/start` が live に出たら Default Login Route を `https://hub.chronista.club/start` に向ける | mako（Auth0 管理画面） | 済（2026-10-08 確認: `id.creo-memories.in/login` が `/start?iss=…` へ 302） |
| chronista.club に `iss=` 付きで来た人へアプリ一覧を見せる受け口（Q1 の A） | Hub lane | 済。v0.6.0 で live（2026-10-08、`https://hub.chronista.club/start`、下記の追記） |
| `CREO_ID_AUDIENCES` に `https://id.anycreative.tech` を足す（Q2 の A の 1 歩目） | Hub lane + fleetstage（live env） | 済。v0.6.0 と同時に live の env へ反映（2026-10-08）。受け付ける先を足すだけなので refresh_token の確認は要らない |
| VP の Hub 向けログインを共通の aud に寄せる | VP lane | 未。**先に**共通の API の `allow_offline_access` と VP CLI の refresh_token grant を確かめる |
| VP が寄せ終えたら `https://hub.chronista.club` を外す | Hub lane | 未 |
| creo-memories に本 ADR を参照する節を足す | creo-memories lead | 未 |

### 2026-10-08 追記 — 受け口の置き場所

受け口は `https://hub.chronista.club/start` に置く。apex `chronista.club` は Cloudflare 上の静的な portal で、Hub への proxy がまだ無い。ADR-019 D3（Hub は `hub.` に自己完結）に合わせ、apex から proxy するかは portal 側の都合で後から決める。

- `iss` が Creo ID の issuer と一致すれば家族のアプリの一覧を出す。`iss` が無くても一覧は出す。違う `iss` は 400 で、一覧を出さない
- 一覧は当面 Hub のコード（`src/start.rs`）に持つ。アプリの名簿（ADR-009）ができたら名簿から引く → v0.7.0 で名簿から引くようにした（2026-10-09）。新しいアプリは `PUT /v1/apps/{app_id}` で `login_url` を登録すれば並ぶ
- 並べるのはブラウザでログインするアプリ（Creo Memories、GFP）。VP は CLI なので `vp auth login` を案内する。fleetstage の backstage / hq は運用者向けなので載せない

### 2026-10-09 追記 — 利用者の名簿と handle（D3 の実装、v0.8.0）

mako の裁定（原文）: 「Hubで。」「display_nameをuniqueに出来る？」→「これは別にモテるようにしよう。handleはuniqueで、display_nameは別で」「必要になったときに claim する。というUXで設計したい」「OK、その形で直して進めて」

- **鍵は Hub が振る `usr_` EntId**（D3 で保留していた点）。Creo ID の `sub` は `creo_sub` として行に持ち、本人にだけ見せる。既存の `hub_resource.owner` は `sub` のまま（埋め戻しは後）
- **行は初回接触で作る。** `GET /v1/me` を user-jwt で叩いた時点で名簿に載り、`usr_id` が付く。handle は無くてよい
- **handle は「必要になったときに claim する」。** 共有・公開・@ 言及など handle が要る操作に初めて触れたとき、アプリが Hub の claim へ誘導する。初回ログインで強制しない。`PUT /v1/me/handle`、早い者勝ち、予約名（migration 004）は取れない、同じ handle の再 claim は冪等、別の handle へは変えられない（rename は ADR-002 の方針で次の段）
- **handle は住所、display_name は呼び名。** handle は unique、display_name は自由文字列で一意にしない。`PATCH /v1/me` で claim 前でも変えられる。Creo ID の access token に `name` は無いので初期値は空
- **公開は handle を持つ人だけ。** `GET /v1/users/@{handle}` は `usrId` / `handle` / `displayName` / `canonicalPath` だけを返す（ADR-008 の 2 軸併載）。予約名の行は 404
- 設定の置き場: データの持ち主は分けたまま（誰か = Creo ID、つながり = Hub）、利用者向けの設定画面は Hub に 1 つ置き、アカウント側の操作は Creo ID のフローへ委譲する（構想、未実装）

## References

- handoff: `mem_1CfpH5EKiEaDWFr22cGtF5`、todo: `mem_1CfpH5qKPKJM2tnarpgskd`
- canonical rule: `mem_1CbMiGk28cSeQ1BjB6P7hq`（2026-05-24）
- federation の 4 日停止（2026-07、token 失効と refresh_token の条件）: `mem_1CenUc2rpHFP1VdHj8FUEe`
- ADR-002（identity 委譲）/ ADR-008（owner の鍵）/ ADR-009（manifest）/ ADR-010（token と audience）/ ADR-013（organization）/ ADR-017（product-token）/ ADR-019（apex portal）/ ADR-020（optional 層）
