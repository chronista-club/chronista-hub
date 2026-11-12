#!/usr/bin/env bash
# 配布用 release binary を新規 embedded DB で起動し、再起動後の永続化まで確認する。
set -euo pipefail
ENGINE="${CONTAINER_ENGINE:-docker}"
IMAGE="${1:?usage: smoke-image.sh IMAGE}"
CONTAINER=""
cleanup() {
  if [[ -n "$CONTAINER" ]]; then
    "$ENGINE" logs "$CONTAINER" || true
    "$ENGINE" rm -f "$CONTAINER" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT
CONTAINER=$("$ENGINE" run -d --name "hub-image-smoke-$$" -p 127.0.0.1::3000 \
  -e STUB_AUTH_ALLOWED=true -e AUTO_MIGRATE_ENABLED=true \
  -e CHRONISTA_HUB_DB_URL=surrealkv:///tmp/hub-smoke "$IMAGE")
base_url() {
  local port
  port=$("$ENGINE" port "$CONTAINER" 3000/tcp | sed -n 's/^127\.0\.0\.1://p')
  [[ "$port" =~ ^[0-9]+$ ]]
  printf 'http://127.0.0.1:%s' "$port"
}
wait_for_health() {
  for _ in $(seq 1 30); do
    curl --silent --fail --max-time 2 "$BASE/health/deep" && return 0
    sleep 1
  done
  return 1
}
BASE=$(base_url)
wait_for_health
curl --silent --show-error --fail --max-time 5 -X POST "$BASE/v1/events" \
  -H 'Content-Type: application/json' -H 'X-App-Token: app:creo-memories:register_resource' \
  --data '{"event_id":"ev_smoke_1","app_id":"creo-memories","kind":"resource.created","idempotency":"idem_smoke_1","emitted_at":"2026-11-12T00:00:00Z","resource":{"id":"atlas_smoke","type":"memories-atlas","path":"/creo-memories/atlas_smoke","handle":"mito","visibility":"public","payload":{"title":"Image smoke"},"createdAt":"2026-11-12T00:00:00Z","updatedAt":"2026-11-12T00:00:00Z"}}'
found=false
for _ in $(seq 1 30); do
  if curl --silent --fail --max-time 2 "$BASE/v1/tree/@mito" | grep -q 'atlas_smoke'; then
    found=true
    break
  fi
  sleep 1
done
[[ "$found" == true ]]
"$ENGINE" restart "$CONTAINER" >/dev/null
BASE=$(base_url)
wait_for_health
curl --silent --fail --max-time 2 "$BASE/v1/tree/@mito" | grep -q 'atlas_smoke'
echo 'Image smoke passed: health, event ingestion, tree read, restart persistence'
