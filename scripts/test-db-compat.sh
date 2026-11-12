#!/usr/bin/env bash
# SDK の接続可否だけでなく、同じ API / recovery テストを両 server に対して走らせる。
# DB はこの script が作る loopback の使い捨て memory instance のみ。
set -euo pipefail
cd "$(dirname "$0")/.."

ENGINE="${CONTAINER_ENGINE:-docker}"
CONTAINER=""
cleanup() {
  if [[ -n "$CONTAINER" ]]; then
    "$ENGINE" rm -f "$CONTAINER" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

for version in 3.3.4 4.0.0; do
  image="docker.io/surrealdb/surrealdb:v$version"
  if [[ "$version" == 4.0.0 ]]; then
    image="docker.io/surrealdb/surrealdb@sha256:c35893410644e65fa90aa1bef446ac9cb0a0c2733b9d405ce3eb56754b85c330"
  fi
  CONTAINER=$("$ENGINE" run -d --name "hub-compat-${version//./-}-$$" -p 127.0.0.1::8000 "$image" \
    start --user root --pass root memory)
  port=$("$ENGINE" port "$CONTAINER" 8000/tcp | sed -n 's/^127\.0\.0\.1://p')
  [[ "$port" =~ ^[0-9]+$ ]]
  actual=""
  for _ in $(seq 1 30); do
    actual=$(curl --silent --fail --max-time 2 "http://127.0.0.1:$port/version" || true)
    [[ "$actual" == "surrealdb-$version" ]] && break
    sleep 1
  done
  if [[ "$actual" != "surrealdb-$version" ]]; then
    "$ENGINE" logs "$CONTAINER"
    echo "expected surrealdb-$version, got: $actual" >&2
    exit 1
  fi
  echo "Testing SDK against $actual"
  CHRONISTA_HUB_SURREAL_TEST_URL="ws://127.0.0.1:$port" \
  CHRONISTA_HUB_SURREAL_TEST_USERNAME=root CHRONISTA_HUB_SURREAL_TEST_PASSWORD=root \
    cargo test --locked -p chronista-hub-server \
    --test db_connect --test db_resilience --test apps --test integration --test orgs \
    --test relay --test settings --test start --test users -- --test-threads=1
  cleanup
  CONTAINER=""
done
