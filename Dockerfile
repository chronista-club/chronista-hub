# syntax=docker/dockerfile:1
# chronista-hub-server — World Tree meta-registry (REST + Unison/QUIC)。
#
# REST(axum, TCP) と Unison(club-unison, QUIC/UDP) を 1 バイナリで提供する。
# embedded SurrealDB (kv-rocksdb、 ADR-016) なので別 DB プロセスは不要。

# --- builder ---------------------------------------------------------------
# Debian suite を trixie に pin。 浮動 tag (rust:1.95) はベース移動で glibc 版が
# 変わり、 runtime と suite がズレると `GLIBC_x.xx not found` で起動失敗する。
# builder / runtime とも trixie 固定で再現性を担保する (creo-unison-server 前例)。
FROM rust:1.95-trixie AS builder
# protoc: club-unison (buffa / Protocol Buffers) の build script が要求。
# clang: surrealdb kv-rocksdb の librocksdb-sys (bindgen) が libclang を要求。
#        creo-unison-server は remote SurrealDB なので protoc のみだったが、
#        hub は embedded rocksdb ゆえ clang が追加で要る。
RUN apt-get update \
    && apt-get install -y --no-install-recommends protobuf-compiler clang \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY . .
# surrealdb-core (embedded) + rocksdb + quinn の重量 graph。 opt-level 2 で
# compile を軽くする (hub は I/O bound なので runtime 影響は無視できる)。
# ⚠️ 低 RAM 環境 (例 4GB podman VM) では full 並列で OOM する。 その場合は
#    `--build-arg CARGO_BUILD_JOBS=1` を渡す (CI=16GB では既定の full 並列で可)。
ARG CARGO_BUILD_JOBS
# server 本体 + quic_probe example (QUIC liveness probe、 issue #35) を同一 build で。
# probe は host の systemd timer から image を entrypoint override で回すので、
# 別 toolchain を host に置かずに済む。
RUN CARGO_PROFILE_RELEASE_OPT_LEVEL=2 \
    cargo build --release ${CARGO_BUILD_JOBS:+--jobs ${CARGO_BUILD_JOBS}} \
    -p chronista-hub-server --bins --example quic_probe

# --- runtime ---------------------------------------------------------------
# builder と同じ Debian suite (trixie) で glibc 版を揃える。
FROM debian:trixie-slim AS runtime
# ca-certificates: Creo ID JWKS fetch (reqwest TLS)。
# libstdc++6: embedded rocksdb (C++) を動的リンクするため runtime に要る。
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libstdc++6 \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /build/target/release/chronista-hub-server /usr/local/bin/chronista-hub-server
# QUIC liveness probe (issue #35)。 host の probe timer が
# `podman run --entrypoint quic_probe <image>` で経路死を検知する。
COPY --from=builder /build/target/release/examples/quic_probe /usr/local/bin/quic_probe
# AUTO_MIGRATE_ENABLED 時に listen 前へ適用する migration 群。
# MIGRATIONS_DIR default は ./migrations、 WORKDIR /app なので /app/migrations を読む。
COPY migrations /app/migrations

# REST(axum/TCP) と Unison(QUIC/UDP)。 実 bind は env で制御する:
#   CHRONISTA_HUB_PORT        REST    → 0.0.0.0:PORT       (default 3000)
#   CHRONISTA_HUB_UNISON_ADDR Unison  → container 内は [::]:7879 を指定すること
#   CHRONISTA_HUB_DB_PATH     RocksDB → /app/data 配下を volume mount 推奨
EXPOSE 3000/tcp
EXPOSE 7879/udp

ENTRYPOINT ["chronista-hub-server"]
