#!/usr/bin/env bash
# Launch Golem Terminal GUI with the debug control socket enabled.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CARGO="${CARGO:-$HOME/.cargo/bin/cargo}"
SOCKET="/tmp/golem-terminal-debug-$$.sock"

cleanup() {
    rm -f "$SOCKET"
}
trap cleanup EXIT

"$CARGO" build --features gui --manifest-path "$SCRIPT_DIR/Cargo.toml"

BINARY="${CARGO_TARGET_DIR:-$SCRIPT_DIR/target}/debug/golem-terminal"

export SESHAT_TEST_SOCKET="$SOCKET"

if [[ "$(uname)" == "Darwin" ]]; then
    APP_DIR="${CARGO_TARGET_DIR:-$SCRIPT_DIR/target}/debug/Golem Terminal.app"
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
  <key>CFBundleVersion</key><string>0.1</string>
  <key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST

    exec "$MACOS_DIR/golem-terminal" ui -- "$@"
else
    exec "$BINARY" ui -- "$@"
fi
