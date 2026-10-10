#!/usr/bin/env bash
# Install Katachi for the current user on Linux: binary, icon, and app-menu entry.
# Run from the release tarball (or a checkout after `cargo build --release`).
set -euo pipefail
cd "$(dirname "$0")"
BIN=katachi
[ -f "$BIN" ] || BIN=../target/release/katachi
ICON=icon.png
[ -f "$ICON" ] || ICON=../assets/icon/icon.png

install -Dm755 "$BIN" "$HOME/.local/bin/katachi"
install -Dm644 "$ICON" "$HOME/.local/share/icons/hicolor/512x512/apps/katachi.png"
install -Dm644 /dev/stdin "$HOME/.local/share/applications/katachi.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Katachi
Comment=Design tool for interfaces and graphics
Exec=$HOME/.local/bin/katachi %f
Icon=katachi
Terminal=false
Categories=Graphics;Development;
MimeType=application/x-katachi;
DESKTOP
command -v update-desktop-database >/dev/null && update-desktop-database "$HOME/.local/share/applications" || true
echo "Installed Katachi to ~/.local/bin/katachi (find it in your app menu)."
