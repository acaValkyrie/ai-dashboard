#!/usr/bin/env bash
set -euo pipefail

# Registers the release build in the desktop application launcher (Show apps).

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/.." >/dev/null 2>&1 && pwd)"

BINARY="$REPO_ROOT/src-tauri/target/release/ai-dashboard"
ICON="$REPO_ROOT/src-tauri/icons/128x128.png"
APPLICATIONS_DIR="$HOME/.local/share/applications"
DESKTOP_FILE="$APPLICATIONS_DIR/ai-dashboard.desktop"

if [[ ! -x "$BINARY" ]]; then
  echo "error: release binary not found at $BINARY" >&2
  echo "hint: run 'cargo tauri build' or 'cargo build --release' first" >&2
  exit 1
fi

mkdir -p "$APPLICATIONS_DIR"

cat > "$DESKTOP_FILE" <<EOF
[Desktop Entry]
Type=Application
Name=AI Usage
Comment=AI Dashboard
Exec=$BINARY
Icon=$ICON
Terminal=false
Categories=Utility;
EOF

chmod +x "$DESKTOP_FILE"

if command -v desktop-file-validate >/dev/null 2>&1; then
  desktop-file-validate "$DESKTOP_FILE"
fi

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$APPLICATIONS_DIR"
fi

echo "installed: $DESKTOP_FILE"
