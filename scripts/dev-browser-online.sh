#!/usr/bin/env bash
# Start a local zone host and the browser client in online mode.
#
# Reuses (or creates) a short-lived ECDSA P-256 development certificate in
# target/dev-tls/, shared with dev-native.sh, starts mmorpg-zone-host with it,
# and serves the page with Vite configured for that host. Open the printed URL
# in two tabs to play together; the native client can join the same zone.
# Ctrl-C stops both processes. --check verifies the same setup and exits.
#
# Ports default to 4433/udp (WebTransport), 8080/tcp (host status) and
# 5173/tcp (page); override them with MMORPG_DEV_TRANSPORT_PORT,
# MMORPG_DEV_STATUS_PORT and MMORPG_DEV_WEB_PORT. A closed tab's character
# stays for MMORPG_RECONNECT_GRACE_TICKS (default 150, five seconds).
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
readonly ROOT_DIR
readonly DEV_TLS_DIR="$ROOT_DIR/target/dev-tls"
readonly CERTIFICATE_PATH="$DEV_TLS_DIR/cert.pem"
readonly PRIVATE_KEY_PATH="$DEV_TLS_DIR/key.pem"
readonly LOG_DIR="$ROOT_DIR/target/dev-browser-online"
readonly HOST_LOG_PATH="$LOG_DIR/zone-host.log"
readonly WEB_LOG_PATH="$LOG_DIR/vite.log"
readonly TRANSPORT_PORT="${MMORPG_DEV_TRANSPORT_PORT:-4433}"
readonly STATUS_PORT="${MMORPG_DEV_STATUS_PORT:-8080}"
readonly WEB_PORT="${MMORPG_DEV_WEB_PORT:-5173}"
readonly GRACE_TICKS="${MMORPG_RECONNECT_GRACE_TICKS:-150}"
readonly ZONE_ID=1
readonly READY_TIMEOUT_SECONDS=60

usage() {
  printf '%s\n' 'Usage: ./scripts/dev-browser-online.sh [--check]'
  printf '%s\n' 'Starts a local zone host and the browser client in online mode.'
}

check_mode=false
case "${1:-}" in
  "") ;;
  --check) check_mode=true ;;
  --help|-h)
    usage
    exit 0
    ;;
  *)
    printf 'Unknown option: %s\n' "$1" >&2
    usage >&2
    exit 2
    ;;
esac

for command in bun cargo curl openssl; do
  if ! command -v "$command" >/dev/null 2>&1; then
    printf 'Required command not found: %s\n' "$command" >&2
    exit 1
  fi
done

# shellcheck source-path=SCRIPTDIR source=lib/dev-tls.sh
source "$ROOT_DIR/scripts/lib/dev-tls.sh"
mkdir -p "$LOG_DIR"
ensure_dev_certificate "$DEV_TLS_DIR"
certificate_hash="$(dev_certificate_hash "$CERTIFICATE_PATH")"
readonly certificate_hash
readonly SERVER_URL="https://127.0.0.1:$TRANSPORT_PORT/game/matches/zone-$ZONE_ID"
# base64url keeps the hash readable in a query string without percent-encoding.
url_hash="$(tr '+/' '-_' <<<"${certificate_hash%%=*}")"
readonly url_hash
readonly PAGE_URL="http://127.0.0.1:$WEB_PORT/mmorpg/?server=$SERVER_URL&certHash=$url_hash"

cargo build --locked -p mmorpg-game-server --bin mmorpg-zone-host
(cd "$ROOT_DIR/web" && bun install --frozen-lockfile && bun run build:wasm)

host_pid=''
web_pid=''
# Stops the process group PID leads, including children it spawned.
stop() {
  local pid=$1
  if [[ -n "$pid" ]] && kill -0 "$pid" >/dev/null 2>&1; then
    kill -TERM -- "-$pid" 2>/dev/null || kill -TERM "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
  fi
}
cleanup() {
  local status=$?
  trap - EXIT INT TERM
  stop "$web_pid"
  stop "$host_pid"
  if (( status != 0 )); then
    for log in "$HOST_LOG_PATH" "$WEB_LOG_PATH"; do
      if [[ -f "$log" ]]; then
        printf '\n%s:\n' "$log" >&2
        tail -n 40 "$log" >&2 || true
      fi
    done
  fi
  exit "$status"
}
trap cleanup EXIT INT TERM

# Job control gives each background process its own process group, so stopping
# it also stops what it spawned (bun runs Vite under Node).
set -m
(
  unset MMORPG_RECOVERY_DIR
  export MMORPG_ZONE_IDS="$ZONE_ID"
  export MMORPG_PORT="$TRANSPORT_PORT"
  export MMORPG_STATUS_PORT="$STATUS_PORT"
  export MMORPG_CERT_PEM="$CERTIFICATE_PATH"
  export MMORPG_KEY_PEM="$PRIVATE_KEY_PATH"
  export MMORPG_ROUTE_PREFIX=/game
  export MMORPG_RECONNECT_GRACE_TICKS="$GRACE_TICKS"
  export MMORPG_DRAIN_GRACE_MS=500
  exec cargo run --locked --quiet -p mmorpg-game-server --bin mmorpg-zone-host
) </dev/null >"$HOST_LOG_PATH" 2>&1 &
host_pid=$!

(
  cd "$ROOT_DIR/web"
  export VITE_ZONE_SERVER="$SERVER_URL"
  export VITE_ZONE_CERT_HASH="$certificate_hash"
  exec bun x vite --host 127.0.0.1 --port "$WEB_PORT" --strictPort
) </dev/null >"$WEB_LOG_PATH" 2>&1 &
web_pid=$!
set +m

# Waits until URL answers, failing early when PID exits.
wait_for() {
  local name=$1 url=$2 pid=$3
  local deadline=$((SECONDS + READY_TIMEOUT_SECONDS))
  until curl --fail --silent --max-time 1 --output /dev/null "$url"; do
    if ! kill -0 "$pid" >/dev/null 2>&1; then
      printf '%s exited before it became ready.\n' "$name" >&2
      exit 1
    fi
    if (( SECONDS >= deadline )); then
      printf '%s did not become ready within %s seconds.\n' "$name" "$READY_TIMEOUT_SECONDS" >&2
      exit 1
    fi
    sleep 0.2
  done
}
wait_for 'The zone host' "http://127.0.0.1:$STATUS_PORT/readyz" "$host_pid"
wait_for 'Vite' "http://127.0.0.1:$WEB_PORT/mmorpg/" "$web_pid"

printf '\nGreyhaven Vale is online. Open this URL in two tabs (Chromium-based browsers):\n\n  %s\n\n' "$PAGE_URL"
printf 'Without the query, "Play online" on the character screen joins the same host.\n'
printf 'Logs: %s\nPress Ctrl-C to stop.\n' "$LOG_DIR"

verify_page() {
  local page
  page="$(curl --fail --silent --max-time 5 "http://127.0.0.1:$WEB_PORT/mmorpg/")"
  if [[ "$page" != *'id="enter-world"'* ]]; then
    printf 'Vite did not serve the character screen.\n' >&2
    return 1
  fi
  printf 'Check passed: the zone host and the page are serving.\n'
}

# Runs until Ctrl-C (the trap stops both processes) or until either exits.
run_until_stopped() {
  while kill -0 "$host_pid" >/dev/null 2>&1 && kill -0 "$web_pid" >/dev/null 2>&1; do
    sleep 1
  done
  printf 'The zone host or Vite stopped unexpectedly.\n' >&2
  return 1
}

if [[ "$check_mode" == true ]]; then
  verify_page
else
  run_until_stopped
fi
