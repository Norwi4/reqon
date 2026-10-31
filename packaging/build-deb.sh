#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
root="$(pwd -P)"
version="${1:-0.11.0}"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo 'Expected a numeric x.y.z version' >&2; exit 1; }
[[ "$(uname -s)" == Linux && "$(dpkg --print-architecture)" == amd64 ]] || { echo 'Build on Linux amd64' >&2; exit 1; }
python_bin="${PYTHON:-python3}"
if [[ ! -x build/linux/venv/bin/python ]]; then "$python_bin" -m venv build/linux/venv; fi
python_bin="$root/build/linux/venv/bin/python"
"$python_bin" -m pip install -r requirements.txt
"$python_bin" -m PyInstaller --noconfirm --clean --windowed --collect-data grpc_tools \
    --name Getman --distpath dist/linux --workpath build/linux/pyinstaller --specpath build/linux main.py
stage="$root/build/linux/deb-stage"
[[ "$stage" == "$root/build/linux/deb-stage" && "$root" != / ]] || exit 1
rm -rf -- "$stage"
mkdir -p "$stage/opt/getman" "$stage/usr/bin" "$stage/usr/share/applications" \
    "$stage/usr/share/icons/hicolor/scalable/apps" "$stage/usr/share/doc/getman" "$stage/DEBIAN"
cp -a dist/linux/Getman/. "$stage/opt/getman/"
install -m 644 packaging/getman.svg "$stage/usr/share/icons/hicolor/scalable/apps/getman.svg"
install -m 644 README.md "$stage/usr/share/doc/getman/README.md"
printf '#!/bin/sh\nexec /opt/getman/Getman "$@"\n' > "$stage/usr/bin/getman"
chmod 755 "$stage/usr/bin/getman"
cat > "$stage/usr/share/applications/getman.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=Getman
GenericName=Local API Client
Comment=Local HTTP and gRPC collections
Comment[ru]=Локальный клиент HTTP и gRPC
Exec=getman
TryExec=getman
Icon=getman
Categories=Development;Network;
Terminal=false
StartupWMClass=Getman
EOF
size="$(du -sk "$stage/opt" | cut -f1)"
cat > "$stage/DEBIAN/control" <<EOF
Package: getman
Version: $version
Section: devel
Priority: optional
Architecture: amd64
Maintainer: Getman <local@localhost>
Installed-Size: $size
Depends: ca-certificates, libc6 (>= 2.36), libstdc++6 (>= 12), libgcc-s1, libgl1, libegl1, libopengl0, libglib2.0-0, libdbus-1-3, libfontconfig1, libfreetype6, libx11-6, libx11-xcb1, libxext6, libxrender1, libsm6, libice6, libxkbcommon0, libxkbcommon-x11-0, libxcb1, libxcb-shape0, libxcb-cursor0, libxcb-icccm4, libxcb-image0, libxcb-keysyms1, libxcb-render-util0, libxcb-xinerama0, libxcb-xkb1
Description: Local native HTTP and gRPC API client
 Qt Widgets desktop application with local collections, environments,
 autosave, protobuf reflection and portable collection import/export.
 Built for Astra Linux 1.8 amd64. No cloud account or browser engine.
EOF
find "$stage" -type d -exec chmod 755 {} +
chmod 644 "$stage/DEBIAN/control"
target="$root/dist/getman_${version}_astra1.8_amd64.deb"
dpkg-deb --root-owner-group -Zxz -z1 --build "$stage" "$target"
dpkg-deb --info "$target"
"$python_bin" -m pip freeze > "dist/getman_${version}_linux-dependencies.txt"
cp /etc/os-release "dist/getman_${version}_linux-build-os.txt"
sha256sum "$target" > "$target.sha256"
printf '\nCreated %s\n' "$target"
