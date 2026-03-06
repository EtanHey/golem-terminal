#!/usr/bin/env bash
# Launch Golem Terminal GUI.
# Usage:
#   ./launch.sh              # Build release + launch
#   ./launch.sh --fast       # Skip build, launch existing binary
#   ./launch.sh --debug      # Build debug + launch with UDS socket
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CARGO="${CARGO:-$HOME/.cargo/bin/cargo}"

MODE="release"
SKIP_BUILD=false
ENABLE_SOCKET=false

for arg in "$@"; do
    case "$arg" in
        --fast) SKIP_BUILD=true ;;
        --debug) MODE="debug"; ENABLE_SOCKET=true ;;
    esac
done

# Build
if [[ "$SKIP_BUILD" == "false" ]]; then
    if [[ "$MODE" == "release" ]]; then
        "$CARGO" build --features gui --release --manifest-path "$SCRIPT_DIR/Cargo.toml"
    else
        "$CARGO" build --features gui --manifest-path "$SCRIPT_DIR/Cargo.toml"
    fi
fi

BINARY="${CARGO_TARGET_DIR:-$SCRIPT_DIR/target}/$MODE/golem-terminal"

if [[ ! -f "$BINARY" ]]; then
    echo "Binary not found at $BINARY — run without --fast first"
    exit 1
fi

# UDS socket for debug/orchestration
if [[ "$ENABLE_SOCKET" == "true" ]]; then
    SOCKET="/tmp/golem-terminal-debug-$$.sock"
    export SESHAT_TEST_SOCKET="$SOCKET"
    cleanup() { rm -f "$SOCKET"; }
    trap cleanup EXIT
    echo "Debug socket: $SOCKET"
fi

# macOS .app bundle
if [[ "$(uname)" == "Darwin" ]]; then
    APP_DIR="${CARGO_TARGET_DIR:-$SCRIPT_DIR/target}/$MODE/Golem Terminal.app"
    MACOS_DIR="$APP_DIR/Contents/MacOS"
    RES_DIR="$APP_DIR/Contents/Resources"
    mkdir -p "$MACOS_DIR" "$RES_DIR"

    ln -sf "$BINARY" "$MACOS_DIR/golem-terminal"

    ICNS="$SCRIPT_DIR/assets/icon.icns"
    [[ -f "$ICNS" ]] && cp -f "$ICNS" "$RES_DIR/icon.icns"

    cat > "$APP_DIR/Contents/Info.plist" << 'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Golem Terminal</string>
  <key>CFBundleExecutable</key><string>golem-terminal</string>
  <key>CFBundleIconFile</key><string>icon</string>
  <key>CFBundleIdentifier</key><string>com.golem.terminal</string>
  <key>CFBundleVersion</key><string>0.2.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST

    echo "Launching Golem Terminal ($MODE)..."
    exec "$MACOS_DIR/golem-terminal" ui
else
    echo "Launching Golem Terminal ($MODE)..."
    exec "$BINARY" ui
fi
