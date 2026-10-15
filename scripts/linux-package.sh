#!/usr/bin/env bash
# Package a Linux build: scripts/linux-package.sh <katachi binary> <x86_64|aarch64> [out dir]
# Writes Katachi-linux-<arch>.tar.gz (what the in-app updater downloads), a .deb, and an .rpm.
# Needs dpkg-deb and rpmbuild (apt install rpm) for the packages; the tarball needs only tar.
set -euo pipefail
BIN=$1 ARCH=$2 OUT=${3:-dist}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)
ID=io.github.dyascj.Katachi
RES="$ROOT/packaging/linux"
mkdir -p "$OUT"
OUT=$(cd "$OUT" && pwd)
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

# Tarball: katachi/katachi plus the user installer and what it installs.
T="$WORK/tar/katachi"
mkdir -p "$T/share"
install -m755 "$BIN" "$T/katachi"
install -m755 "$ROOT/scripts/linux-install.sh" "$T/linux-install.sh"
cp -R "$RES/icons" "$RES/$ID.desktop" "$RES/$ID.xml" "$RES/$ID.metainfo.xml" "$T/share/"
cp "$ROOT/LICENSE" "$T/"
tar -czf "$OUT/Katachi-linux-$ARCH.tar.gz" -C "$WORK/tar" katachi

# The system layout shared by the .deb and the .rpm.
S="$WORK/root"
install -Dm755 "$BIN" "$S/usr/bin/katachi"
install -Dm644 "$RES/$ID.desktop" "$S/usr/share/applications/$ID.desktop"
install -Dm644 "$RES/$ID.xml" "$S/usr/share/mime/packages/$ID.xml"
install -Dm644 "$RES/$ID.metainfo.xml" "$S/usr/share/metainfo/$ID.metainfo.xml"
for png in "$RES"/icons/*.png; do
  s=$(basename "$png" .png)
  install -Dm644 "$png" "$S/usr/share/icons/hicolor/${s}x${s}/apps/$ID.png"
done
install -Dm644 "$RES/icons/scalable.svg" "$S/usr/share/icons/hicolor/scalable/apps/$ID.svg"
install -Dm644 "$ROOT/LICENSE" "$S/usr/share/licenses/katachi/LICENSE"

SUMMARY="Design tool with Figma's workflow"
DESC="Katachi is a native design tool with Figma's canvas, tools, panels, and shortcuts: auto layout, components, variables, prototyping, Dev Mode, multiplayer, plugins, and .fig import. It runs offline and keeps files on your disk."

if command -v dpkg-deb >/dev/null; then
  DEBARCH=$([ "$ARCH" = aarch64 ] && echo arm64 || echo amd64)
  D="$WORK/deb"
  cp -a "$S" "$D"
  mkdir -p "$D/DEBIAN"
  cat > "$D/DEBIAN/control" <<CONTROL
Package: katachi
Version: $VERSION
Architecture: $DEBARCH
Maintainer: Charles Dyas <dyascj@gmail.com>
Section: graphics
Priority: optional
Homepage: https://github.com/dyascj/katachi
Depends: libc6 (>= 2.28), libxkbcommon0, libxkbcommon-x11-0
Recommends: libvulkan1 | libegl1, libwayland-client0, libx11-6, libxcursor1, libxi6, libxrandr2, xdg-desktop-portal | zenity, curl
Installed-Size: $(du -sk "$S" | cut -f1)
Description: $SUMMARY
 $DESC
CONTROL
  dpkg-deb --root-owner-group -Zxz --build "$D" "$OUT/katachi_${VERSION}_${DEBARCH}.deb" >/dev/null
fi

if command -v rpmbuild >/dev/null; then
  R="$WORK/rpm"
  mkdir -p "$R/SPECS"
  cat > "$R/SPECS/katachi.spec" <<SPEC
%global debug_package %{nil}
%global __os_install_post %{nil}
Name: katachi
Version: $VERSION
Release: 1
Summary: $SUMMARY
License: MIT
URL: https://github.com/dyascj/katachi
AutoReqProv: no
Requires: libxkbcommon.so.0()(64bit), libxkbcommon-x11.so.0()(64bit)
Recommends: mesa-vulkan-drivers, xdg-desktop-portal, curl
%description
$DESC
%install
cp -a $S/. %{buildroot}/
%files
/usr/bin/katachi
/usr/share/applications/$ID.desktop
/usr/share/mime/packages/$ID.xml
/usr/share/metainfo/$ID.metainfo.xml
/usr/share/icons/hicolor/*/apps/$ID.*
%license /usr/share/licenses/katachi/LICENSE
SPEC
  rpmbuild -bb --quiet --target "$ARCH" --define "_topdir $R" "$R/SPECS/katachi.spec" >/dev/null
  cp "$R"/RPMS/*/*.rpm "$OUT/"
fi
ls -1 "$OUT"
