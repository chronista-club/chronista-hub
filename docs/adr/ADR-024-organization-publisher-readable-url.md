# ADR-024: 名義（人と団体）と読める公開 URL — 会社の handle で公開物を出す

- **Status**: Accepted（2026-10-14 mako 裁定。段 1 を実装）
- **Date**: 2026-10-14
- **Related**: ADR-002（rename の取り置き）/ 008（usr_id が stable key）/ 012・019（`hub.chronista.club/@{handle}` が正式な住所）/ 013（団体は同じ handle 空間）/ 023（誰か・つながり・中身の 3 層、課金はアプリ）
- **裁定の記録**: mem_1Cg1sxM3TXiHxAh5irCxPu（名義）、mem_1Cg1sk8vs728Hz4jTsLCis（有料の線）、mem_1Cg1tCJFvTXQeezxFDdbhv（lab 以上）

## Context

mako（2026-10-14、原文）:

> 私が会社用の（ANYCRETIVE用のslug（handle？）を設定して、ANYCRETIVE publisher handleとして使うとしたら、どこにどうモジュールがあるといいかConceptionしたい
> mitoのaltasでやってるweekly ship logがpublicなんだけど、slugを使いたいけど、handleが個人よりは会社のハンドルにぶら下げたいなと。

この時点の状態:

- 名義になれるのは人だけ。団体（ADR-013 の organization / org-membership）は spec に予定として書かれているだけで、実装は無い
- Creo Memories の公開ページの URL は `app.creo-memories.in/r/atl_…`。名義も読める名前も出ていない（2026-10-03「まだrでいいや。運用して考えよう。」）
- handle の正式な住所は `hub.chronista.club/@{handle}`（ADR-019）だが、HTML のページはまだ無く API だけ

## Decision

### D1. 会社は Hub の団体。ログインは持たない

- `@anycreative` を人と同じ handle 空間に作る。`user` table の行で、`account_type = organization`（ADR-013 の shared namespace）
- 団体自身は Creo ID のアカウントを持たない。人が membership（`org_membership`、役割 owner / admin / member / guest）で団体として振る舞う。共有パスワードを生まない
- 団体の持ち主は Hub。spec の organization / org-membership の `ownership "creo-id"` は ADR-023 D3（handle は Hub）より前の記述なので `hub` に直す
- 段 1 は持ち主 mako 1 人（migration 013 で結ぶ）。招待と役割の変更は後の段

### D2. 読める公開 URL は Hub が索引を持ち、中身のアプリへ案内する

- 正式な住所は `hub.chronista.club/@{handle}/{slug}`。Hub の `publication` table が（名義の usr_id、slug）→ アプリの公開ページの URL を持ち、302 で案内する。中身は Hub に置かない
- `hub.chronista.club/@{handle}` は名義のページ。その名義の公開物を並べる
- 索引は usr_id に付く。handle を変えても公開物は付いていき、取り置き中の旧 handle（ADR-002 追記）は新しい住所へ 301

### D3. 読める公開 URL は有料の特典。判定はアプリ

mako「読める公開 URL　<-ここが綺麗かな。有料化するとしたら。」「labからにしようかな。」「deskはいらんと思うんだよね。」

| | 無料のまま | 有料（Creo Memories の lab 以上） |
|---|---|---|
| Hub の handle（Creo Apps 共通の住所） | ✓ | ✓ |
| atlas / memory の slug（中身の呼び名） | ✓ | ✓ |
| ID の公開 URL（`/r/atl_…`） | ✓ | ✓ |
| 読める公開 URL（`/@{handle}/{slug}`） | | ✓ |

- プランは Creo Memories の持ち物（ADR-023: 課金は各アプリ）。Hub はプランを知らない
- だから索引への登録は **アプリが product token で行う**（scope `register_publication`）。アプリは本人とプランを確かめたうえで、操作した人の Creo ID `sub`（`actorSub`）を添える
- Hub が確かめるのは 3 つ: 名義があること、actor がその名義を使えること（本人、または団体の guest 以外）、案内先がそのアプリの host（home_url / login_url の host とその 1 つ上）であること
- 利用者が Hub に直接登録する道は作らない（プランの判定を素通りするため）
- 解約したときに読める URL をどうするかは、Creo Memories の段で決める（提案: 404 にせず ID の公開ページへ案内し続け、新規だけ止める）

### D4. API

| | 誰が | 何をする |
|---|---|---|
| `GET /v1/me/publishers` | 人（user-jwt） | 自分が使える名義。自分（handle があれば）と所属する団体（guest 以外）。アプリが「公開名義」の選択肢に使う |
| `PUT /v1/publications/@{handle}/{slug}` | アプリ（product token） | 登録 / 置き換え。body `{ actorSub, url, title? }`。新規 201、更新 200、別のアプリの slug は 409 |
| `DELETE /v1/publications/@{handle}/{slug}?actorSub=` | アプリ | 登録したアプリだけが消せる |
| `GET /v1/users/@{handle}/publications` | 誰でも | 名義の公開物の一覧 |
| `GET /v1/users/@{handle}` | 誰でも | 団体も返す。`accountType` を足した |
| `GET /@{handle}` / `GET /@{handle}/{slug}` | 誰でも | 名義のページ / 302 で案内（no-store） |

slug は小文字英数とハイフン、63 文字まで（Creo Memories の atlas slug と同じ幅）。`identity` などの予約語と、名簿にある app_id（`/@{handle}/{product-slug}/…` の木と混ざらないように）は使えない。

### D5. 最初の 1 件は bootstrap

Creo Memories が登録できるようになるまでのつなぎとして、weekly ship log（atlas `ship-log`、`atl_1CfdiCfAX5XzXdYtVwsWUf`）を migration 013 で `@anycreative/ship-log` に入れる。

## 段の分け方

- **段 1（Hub v0.11.0、本 ADR）**: 団体と membership、名義の一覧、索引と登録 API、名義のページと案内、`/settings` に名義と公開物の一覧、`@anycreative` と ship log の bootstrap
- **段 2（Creo Memories）**: atlas の公開名義の選択（`/v1/me/publishers`）、lab 以上の判定、公開時に Hub へ登録・非公開や削除で取り消し、Hub の product token（scope `register_publication`）の発行と配備
- **後の段**: 団体への招待と役割の変更、団体の作成（今は migration だけ）、解約時の扱い

## Consequences

### 正

- 会社の名義が Creo Apps 共通になる。VP や GFP の公開物も同じ `@anycreative` の下に並べられる
- 中身・課金・名義の持ち主が分かれたまま（ADR-023 の 3 層どおり）。Hub はプランを知らずに済む
- 名義のページが、全アプリの公開物の入口になる

### 負

- アプリは Hub の product token を持ち、公開の変化を Hub に伝える責任を負う（伝え漏れると索引が古いまま残る。案内先が 404 になるだけで、非公開の中身は漏れない。公開の判定はアプリの公開ページがする）
- 段 1 の団体は migration でしか作れない

## 却下案

- **会社用の Creo ID アカウントを作る**: 共有パスワードになり、誰が操作したかが消える
- **Creo Memories の中だけで名義を持つ**: 他のアプリから同じ名義を使えない
- **Hub がプランを見て判定する**: 課金はアプリの持ち物（ADR-023）。Hub に各アプリのプランを写すと持ち主が 2 つになる
- **利用者が Hub に直接登録する**: プランの判定を素通りする
