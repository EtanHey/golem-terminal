#!/usr/bin/env bash
# Install Golem Terminal as a dockable .app in ~/Applications.
# Run once after building; then drag "Golem Terminal" from Applications to your dock.
#
# Usage:
#   ./install.sh              # Build release + install to ~/Applications
#   ./install.sh --no-build   # Skip build, install existing binary
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CARGO="${CARGO:-$HOME/.cargo/bin/cargo}"
INSTALL_DIR="${INSTALL_DIR:-$HOME/Applications}"
APP_NAME="Golem Terminal.app"

SKIP_BUILD=false
for arg in "$@"; do
    case "$arg" in
        --no-build) SKIP_BUILD=true ;;
    esac
done

# Build
if [[ "$SKIP_BUILD" == "false" ]]; then
    echo "Building Golem Terminal (release)..."
    "$CARGO" build --features gui --release --manifest-path "$SCRIPT_DIR/Cargo.toml"
fi

BINARY="${CARGO_TARGET_DIR:-$SCRIPT_DIR/target}/release/golem-terminal"
if [[ ! -f "$BINARY" ]]; then
    echo "Binary not found at $BINARY — run without --no-build first"
    exit 1
fi

# Create .app bundle in install dir
APP_DIR="$INSTALL_DIR/$APP_NAME"
MACOS_DIR="$APP_DIR/Contents/MacOS"
RES_DIR="$APP_DIR/Contents/Resources"

echo "Installing to $APP_DIR"
mkdir -p "$MACOS_DIR" "$RES_DIR"

# Copy binary (not symlink — app must be self-contained)
cp -f "$BINARY" "$MACOS_DIR/golem-terminal-bin"

# Launcher script — macOS runs CFBundleExecutable with no args; binary needs "ui" subcommand
cat > "$MACOS_DIR/golem-terminal" << 'LAUNCHER'
#!/bin/bash
exec "$(dirname "$0")/golem-terminal-bin" ui
LAUNCHER
chmod +x "$MACOS_DIR/golem-terminal"

# Icon
ICNS="$SCRIPT_DIR/assets/icon.icns"
[[ -f "$ICNS" ]] && cp -f "$ICNS" "$RES_DIR/icon.icns"

# Info.plist
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

echo ""
echo "Done. Golem Terminal is installed at:"
echo "  $APP_DIR"
echo ""
echo "To add to dock: open Finder → Applications → drag 'Golem Terminal' to the dock."
echo "Or run: open '$INSTALL_DIR'"
echo ""
