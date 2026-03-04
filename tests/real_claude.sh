#!/usr/bin/env bash
# E2E test: launch real Claude Code via the GUI harness, verify it responds.
#
# Usage:  ./tests/real_claude.sh
#
# Requires: `claude` in PATH (already authed via OAuth).
# Drives the Golem Terminal through its UDS test harness — never spawns Claude
# from within Claude Code itself, avoiding the self-launch restriction.
set -euo pipefail

CARGO="${CARGO:-$HOME/.cargo/bin/cargo}"
SOCKET="/tmp/golem-test-claude-$$.sock"
GUI_PID=""

cleanup() {
    exec 3>&- 2>/dev/null || true
    exec 4<&- 2>/dev/null || true
    [[ -n "${NC_PID:-}" ]] && kill "$NC_PID" 2>/dev/null; wait "$NC_PID" 2>/dev/null || true
    [[ -n "$GUI_PID" ]] && kill "$GUI_PID" 2>/dev/null; wait "$GUI_PID" 2>/dev/null || true
    rm -f "$SOCKET" "${FIFO_IN:-}" "${FIFO_OUT:-}"
}
trap cleanup EXIT

# ── Preflight ────────────────────────────────────────────────────────────────

command -v claude >/dev/null 2>&1 || { echo "SKIP: claude not in PATH"; exit 0; }

# ── Build ────────────────────────────────────────────────────────────────────

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"

echo "--- Building golem-terminal (gui) ---"
"$CARGO" build --features gui --manifest-path "$SCRIPT_DIR/Cargo.toml" 2>&1
BINARY="${CARGO_TARGET_DIR:-$SCRIPT_DIR/target}/debug/golem-terminal"

# ── Launch GUI with claude --print ───────────────────────────────────────────

echo "--- Launching: $BINARY ui -- claude --print 'respond with just the word pong' ---"
SESHAT_TEST_SOCKET="$SOCKET" "$BINARY" ui -- claude --print "respond with just the word pong" &
GUI_PID=$!

# Wait for the UDS socket to appear.
for _ in $(seq 1 100); do
    [[ -S "$SOCKET" ]] && break
    sleep 0.05
done
[[ -S "$SOCKET" ]] || { echo "FAIL: socket never appeared after 5s"; exit 1; }

# ── Connect via nc -U (FIFOs for bidirectional I/O) ──────────────────────────

FIFO_IN=$(mktemp -u /tmp/golem-fifo-in-XXXXXX)
FIFO_OUT=$(mktemp -u /tmp/golem-fifo-out-XXXXXX)
mkfifo "$FIFO_IN" "$FIFO_OUT"
nc -U "$SOCKET" < "$FIFO_IN" > "$FIFO_OUT" &
NC_PID=$!
exec 3>"$FIFO_IN" 4<"$FIFO_OUT"
rm -f "$FIFO_IN" "$FIFO_OUT"  # safe: fds keep the pipes alive

send()  { echo "$1" >&3; }
recv()  { IFS= read -r -t 10 line <&4; echo "$line"; }
query() { send "$1"; recv; }

wait_status() {
    local target="$1" max="${2:-200}"
    for _ in $(seq 1 "$max"); do
        local resp
        resp=$(query '{"cmd":"status"}')
        [[ "$resp" == *"\"$target\""* ]] && return 0
        sleep 0.05
    done
    echo "FAIL: status never reached '$target' (last: $resp)"
    exit 1
}

decode_output() {
    local resp hex
    resp=$(query '{"cmd":"output"}')
    # Extract hex-encoded output from {"output":"<hex>"}.
    hex="${resp#*\"output\":\"}"
    hex="${hex%\"*}"
    printf '%s' "$hex" | xxd -r -p 2>/dev/null || true
}

wait_output_contains() {
    local target="$1" max="${2:-600}" decoded=""
    for _ in $(seq 1 "$max"); do
        decoded=$(decode_output)
        if [[ "$decoded" == *"$target"* ]]; then
            return 0
        fi
        sleep 0.05
    done
    echo "FAIL: output never contained '$target'"
    echo "DEBUG: last decoded output (${#decoded} bytes):"
    echo "$decoded" | head -20
    exit 1
}

# ── Drive the test ───────────────────────────────────────────────────────────

echo "--- Sending launch ---"
send '{"cmd":"launch"}'

echo "--- Waiting for ready ---"
wait_status "ready" 200
echo "OK: status=ready"

echo "--- Waiting for 'pong' in output (up to 30s) ---"
wait_output_contains "pong" 600
echo "OK: output contains 'pong'"

echo "--- Waiting for idle (claude --print should exit on its own) ---"
wait_status "idle" 600
echo "OK: status=idle"

echo ""
echo "PASS: real_claude_print_pong"
