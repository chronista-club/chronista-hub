# ADR-022: DB 接続を URL 化し、本番は remote SurrealDB へ寄せる（Studio 直結のため）

- **Status**: **Accepted**（2026-10-06 mako 裁定 — 選択肢「2. SurrealDB を別プロセスにし、Hub はそこへ接続」を選択）
- **Date**: 2026-10-06
- **Supersedes**: ADR-016 の **DB 節のみ**（「embedded (in-process) + kv-rocksdb」）。言語・HTTP・layout の決定は ADR-016 のまま有効
- **Related**: ADR-016（Rust + embedded SurrealDB）/ ADR-014（observability）

## Context

Hub の永続データは ADR-016 以来、Hub プロセス内の embedded SurrealDB（kv-rocksdb）に置いている。
この構成では、手元の **SurrealDB Studio から本番データを直接見られない**:

- embedded には Studio が話す RPC（WebSocket / HTTP）の受け口が無い。SDK は embedded DB をサーバとして公開する API を持たない（RPC サーバは `surreal` バイナリ側の実装）
- 同じ RocksDB ディレクトリは LOCK を取られるので、Hub 稼働中に `surreal start rocksdb://...` で横から開けない。稼働中のディレクトリコピーは整合性が保証されない

検討した選択肢（2026-10-06）:

1. snapshot を export して手元で見る — 構成は変わらないが、ライブではない
2. **SurrealDB を別プロセスにして Hub はそこへ接続、Studio も Tailscale 越しに同じ DB へ繋ぐ** ← 採用
3. SurrealDB の RPC を Hub に自前実装 — protocol 追従コストが割に合わない

mako は「中身をライブで見たい」として 2 を選んだ。

## Decision

### D1. 接続先は URL 一本で決める

`surrealdb::engine::any`（`Surreal<Any>`）に切り替え、接続先を `CHRONISTA_HUB_DB_URL` で渡す。ADR-016 が
「将来 remote へ swap 可能（`Surreal<Any>` 抽象）」として確保していた経路をそのまま使う。

| URL | 意味 | 用途 |
|---|---|---|
| `rocksdb://<dir>` | embedded 永続（従来どおり） | 手元開発 / e2e |
| `ws://host:port` / `wss://...` | remote | 本番 |
| `mem://` | embedded 揮発 | test |

- `CHRONISTA_HUB_DB_URL` 未設定時は、旧 env `CHRONISTA_HUB_DB_PATH` を `rocksdb://<path>` として扱う（default `./data/hub.rocksdb`）。既存の deploy / `scripts/e2e.sh` は無変更で動く
- embedded のコードパスは消さない。remote が落ちたときの退避先、オフライン開発の場として残す

### D2. remote は最小権限の database user で signin する

| env | 意味 |
|---|---|
| `SURREALDB_USERNAME` / `SURREALDB_PASSWORD` | 両方揃って有効。片方だけは起動エラー |
| `SURREALDB_AUTH_LEVEL` | `database`（default）/ `namespace` / `root`。未知値は起動エラー |

共有 instance では `DEFINE USER ... ON DATABASE ... ROLES OWNER` の database user を使う（migration が
`DEFINE TABLE` 等を発行するので OWNER が要る）。password はログ・Debug 出力に出さない。

**bootstrap だけは root が要る**（2026-10-06 実測、SurrealDB 3.2.3）。`001_bootstrap` の
`DEFINE NAMESPACE IF NOT EXISTS` は namespace が既にあっても root 以外を拒否する
（`DEFINE DATABASE` は namespace user 以上、`DEFINE TABLE` は database user で可）。共有 instance の root を
Hub に渡すと他 tenant のデータに触れられるので、Hub には渡さない。代わりに:

- 既存データの移行: export に `_migrations`（001〜007 適用済み）が含まれるので、import 後の Hub は 001 を流さない
- 新規の remote DB: 運用者が root で一度だけ bootstrap（`run_pending_migrations` 相当、または `surreal import`）し、
  database user を作ってから Hub を database user で起動する

008 以降の migration は database user で流せる（`tests/db_connect.rs` の `ws_database_user_applies_later_migrations`）。
適用済みの 001 は書き換えない（forward-only）。

### D3. migration は従来どおり起動時に SDK 経由で適用

`run_pending_migrations` は `Surreal<Any>` 上でそのまま動く。`_migrations` table も remote 側に作られる。

### D4. 本番の配置先: live storage host の SurrealDB

> **2026-10-07 追記 — live の切替は保留**。mako 裁定「まずは rocksdb の方を選択していこう」により、
> live の Hub は当面 embedded RocksDB（`CHRONISTA_HUB_DB_PATH`）のまま運用する。D1〜D3 のコード
> （URL 切替・signin）は入れ、remote 側の準備（fleetstage #256 の namespace / database / user と
> 管理端末の ingress）も残す。切り替える時は下記「移行手順」の 2〜3 だけでよい。保留中は Studio から
> live を直接は見られない。
>
> **2026-10-08 追記 — 切替を実施へ**。mako 裁定「切り替えていこう」。下記「移行手順」で live を remote へ移す。

本番 remote は **live 世代 `g20260921d1` の storage host（`fleetstage-storage-g20260921d1` / `100.125.152.46`）の
SurrealDB live（port 18001）に `chronista` namespace / `hub` database を切る**。

- 当初は Haven（storage-anycreative-01）を想定したが、Haven と fleet-worker-01 は 2026-09-21 の Debian 世代交換で
  **退役済み**（fleetstage `AGENTS.md`「live の実体」）。chronista-hub の Hub live も現在は worker（`fleetstage-worker-g20260921d1`）で動く
- storage の nftables（`storage-guard.nft`）は 18001 を **worker の Tailscale IP からだけ** 許可している。Hub（worker 上）→ storage は既存の許可経路に乗る
- creo-memories / GFP / objectrecords と同じ「app は worker、data は storage」の型

**Studio からの閲覧**は同じ nftables で塞がれている（管理端末の IP は許可されていない）。
2026-10-06 mako 裁定で、**nftables の許可リストに管理端末（`makomba-1` / `100.71.138.86`）を足し、18001 だけを開ける**
（SSH port forward 案は不採用）。設定は fleetstage 側の Ansible role で行う（fleetstage lane に依頼済み）。

## Consequences

### 正
- 本番 DB を Studio からライブで閲覧・調査できる（目的）
- DB のバックアップ / export を storage host の既存運用（daily-surreal backup）に乗せられる
- Hub の再起動・image 更新と DB のライフサイクルが分離する

### 負
- **各 query にネットワーク往復が乗る**。ADR-016 が embedded を選んだ主目的（最小レイテンシ）を一部手放す。worker → storage の host 間往復
- Hub の稼働が remote DB（storage host）の稼働に依存する。storage 停止 = Hub の read / ingestion 停止
- 資格情報（DB user / password）の管理が増える（`op inject` で `.env.live.server` に入れる）
- 既存 live データの移行が要る（下記）

### 移行手順（実施は fleetstage の worker / storage 作業）
1. ✅ storage host の SurrealDB live に root で `chronista` namespace / `hub` database と、OWNER の database user `hub`・
   VIEWER の namespace user `viewer` を作る（fleetstage role `storage_hub_access`、1Password `FleetFlowVault/fleetstage-chronista-hub-db`）
2. 新しい Hub image（v0.5.0 以降）を worker に読み込む。この時点では env を変えないので RocksDB のまま動く
3. Hub を停止 → live の RocksDB ディレクトリを `surreal start rocksdb://<copy>` で開き、**http** で `surreal export --ns chronista --db hub`
   → storage へ **http** で `surreal import --auth-level database -u hub`（`surreal import` は ws では動かない）。テーブルごとの件数を前後で突き合わせる
4. env を `CHRONISTA_HUB_DB_URL` / `SURREALDB_USERNAME` / `SURREALDB_PASSWORD` / `SURREALDB_AUTH_LEVEL` に切り替え、
   unit の `ExecCondition=test -s …/hub.rocksdb/CURRENT` を外して Hub を起動（migration は `count=0` になるはず）
5. 旧 RocksDB ディレクトリは退避先として一定期間残す。日次 backup に `live/chronista/hub` を足す

予行演習（2026-10-08、手元、SDK 3.1.4 で作った RocksDB / surreal 3.2.3）: export はテーブル定義 16・フィールド定義 104・
インデックス 7・INSERT 4 文で、root 権限の要る文は含まれない。`hub` user での import、新 Hub の起動（migration `count=0`）、
`/v1/tree` の移行前後一致まで確認済み。

## 却下案

- **snapshot export（選択肢 1）** — 却下: 見られるのが export 時点のみで、ライブ調査の要求を満たさない
- **RPC を Hub に自前実装（選択肢 3）** — 却下: SurrealDB の RPC protocol を追従し続けるコストが目的に見合わない
- **embedded の code path を撤去** — 却下: 手元開発・e2e・オフライン時の退避先として価値がある。URL 切替なので維持コストはほぼ無い

## References

- ADR-016（embedded 採用時の判断と、`Surreal<Any>` による swap path の確保）
- fleetstage `AGENTS.md`「live の実体 (2026-09-21 Debian 13 世代交換後)」/ `infra/ansible/roles/storage_surreal/templates/storage-guard.nft.j2`
- 前例: objectrecords `objectrecords-db/src/client.rs`（`any::connect` + signin）
