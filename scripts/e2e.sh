#!/usr/bin/env bash
# Binary e2e — embedded RocksDB に対し publish → consumer → tree read、
# さらに server 再起動後もデータが残存することを検証する (永続化の核心)。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PORT="${PORT:-3999}"
DB_DIR="$(mktemp -d)/hub.rocksdb"
BIN="$ROOT/target/debug/chronista-hub-server"

echo "==> cargo build"
cargo build -q

wait_health() {
  for _ in $(seq 1 50); do
    curl -fs "http://127.0.0.1:$PORT/health" >/dev/null 2>&1 && return 0
    sleep 0.3
  done
  echo "server did not become healthy"; return 1
}

NOW="2026-06-11T00:00:00Z"
EVENT=$(cat <<JSON
{"event_id":"ev_e2e_1","app_id":"creo-memories","kind":"resource.created","idempotency":"idem_e2e_1","emitted_at":"$NOW","resource":{"id":"atlas_e2e","type":"memories-atlas","path":"/creo-memories/atlas_e2e","handle":"mito","visibility":"public","payload":{"title":"E2E Atlas"},"createdAt":"$NOW","updatedAt":"$NOW"}}
JSON
)

echo "==> boot #1 (auto-migrate)"
# STUB_AUTH_ALLOWED=true: e2e を hermetic に保つ (default は起動時に外部 JWKS を fetch する)。
AUTO_MIGRATE_ENABLED=true STUB_AUTH_ALLOWED=true CHRONISTA_HUB_DB_PATH="$DB_DIR" CHRONISTA_HUB_PORT="$PORT" \
  MIGRATIONS_DIR="$ROOT/migrations" "$BIN" >/tmp/hub-rust-1.log 2>&1 &
PID1=$!
wait_health

echo "==> POST /v1/events"
curl -fs -X POST "http://127.0.0.1:$PORT/v1/events" \
  -H "Content-Type: application/json" \
  -H "X-App-Token: app:creo-memories:register_resource" \
  -d "$EVENT"; echo
sleep 2

echo "==> GET /v1/tree/@mito (before restart)"
BEFORE=$(curl -fs "http://127.0.0.1:$PORT/v1/tree/@mito"); echo "$BEFORE"
echo "$BEFORE" | grep -q "atlas_e2e" || { echo "FAIL: resource not found before restart"; kill $PID1; exit 1; }

kill $PID1; wait $PID1 2>/dev/null || true

echo "==> boot #2 (no migrate, same DB dir)"
STUB_AUTH_ALLOWED=true CHRONISTA_HUB_DB_PATH="$DB_DIR" CHRONISTA_HUB_PORT="$PORT" "$BIN" >/tmp/hub-rust-2.log 2>&1 &
PID2=$!
wait_health

echo "==> GET /v1/tree/@mito (after restart — must still contain atlas_e2e)"
AFTER=$(curl -fs "http://127.0.0.1:$PORT/v1/tree/@mito"); echo "$AFTER"
kill $PID2; wait $PID2 2>/dev/null || true

if echo "$AFTER" | grep -q "atlas_e2e"; then
  echo "✅ E2E PASS — data persisted across restart"
else
  echo "❌ E2E FAIL — data lost on restart"; exit 1
fi
