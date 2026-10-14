#!/usr/bin/env bash
# Install Katachi for the current user on Linux: the binary, icons, app-menu entry, and the
# .katachi file type (so double-clicking a file opens it). `--uninstall` removes all of it.
# Run it from the release tarball, or from a checkout after `cargo build --release`.
set -euo pipefail
cd "$(dirname "$0")"
ID=io.github.dyascj.Katachi
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
BIN_DIR="$HOME/.local/bin"

# The tarball keeps everything next to this script; a checkout keeps it in packaging/linux.
if [ -d share ]; then RES=share; else RES=../packaging/linux; fi
BIN=katachi
[ -f "$BIN" ] || BIN=../target/release/katachi

refresh() {
  command -v update-desktop-database >/dev/null && update-desktop-database -q "$DATA/applications" || true
  command -v update-mime-database >/dev/null && update-mime-database "$DATA/mime" >/dev/null 2>&1 || true
  command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -qtf "$DATA/icons/hicolor" 2>/dev/null || true
}

# Files from installs before 0.4, which used plain "katachi" names.
rm -f "$DATA/applications/katachi.desktop" "$DATA/icons/hicolor/512x512/apps/katachi.png"

if [ "${1:-}" = "--uninstall" ]; then
  rm -f "$BIN_DIR/katachi" "$DATA/applications/$ID.desktop" "$DATA/mime/packages/$ID.xml" "$DATA/metainfo/$ID.metainfo.xml"
  rm -f "$DATA"/icons/hicolor/*/apps/$ID.png "$DATA/icons/hicolor/scalable/apps/$ID.svg"
  refresh
  echo "Removed Katachi. Your files and settings in $DATA/katachi were left alone."
  exit 0
fi

[ -f "$BIN" ] || { echo "No katachi binary here. Run cargo build --release first." >&2; exit 1; }
install -Dm755 "$BIN" "$BIN_DIR/katachi"
for png in "$RES"/icons/*.png; do
  s=$(basename "$png" .png)
  install -Dm644 "$png" "$DATA/icons/hicolor/${s}x${s}/apps/$ID.png"
done
install -Dm644 "$RES/icons/scalable.svg" "$DATA/icons/hicolor/scalable/apps/$ID.svg"
# The menu entry runs the installed binary by its full path, in case ~/.local/bin isn't on PATH.
tmp=$(mktemp)
sed "s|^Exec=katachi|Exec=\"$BIN_DIR/katachi\"|" "$RES/$ID.desktop" > "$tmp"
install -Dm644 "$tmp" "$DATA/applications/$ID.desktop"
rm -f "$tmp"
install -Dm644 "$RES/$ID.xml" "$DATA/mime/packages/$ID.xml"
install -Dm644 "$RES/$ID.metainfo.xml" "$DATA/metainfo/$ID.metainfo.xml"
refresh
command -v xdg-mime >/dev/null && xdg-mime default "$ID.desktop" application/x-katachi || true

echo "Installed Katachi to $BIN_DIR/katachi. Find it in your app menu."
case ":$PATH:" in *":$BIN_DIR:"*) ;; *) echo "Add $BIN_DIR to your PATH to run katachi from a terminal." ;; esac
