#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
root="$(pwd -P)"
version="${1:-0.15.0}"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo 'Expected a numeric x.y.z version' >&2; exit 1; }
[[ "$(uname -s)" == Linux && "$(dpkg --print-architecture)" == amd64 ]] || { echo 'Build on Linux amd64' >&2; exit 1; }
python_bin="${PYTHON:-python3}"
if [[ ! -x build/linux/venv/bin/python ]]; then "$python_bin" -m venv build/linux/venv; fi
python_bin="$root/build/linux/venv/bin/python"
"$python_bin" -m pip install -r requirements.txt
"$python_bin" -m PyInstaller --noconfirm --clean --windowed --collect-data grpc_tools \
    --add-data "$root/assets/fonts:assets/fonts" \
    --copy-metadata PySocks --copy-metadata httpcore --copy-metadata jsonschema \
    --collect-data jsonschema_specifications \
    --add-data "$root/Reqon_Icon_Pack_Approved/linux/reqon.png:Reqon_Icon_Pack_Approved/linux" \
    --add-data "$root/Reqon_Icon_Pack_Approved/png/transparent:Reqon_Icon_Pack_Approved/png/transparent" \
    --add-data "$root/reqon-ui-states v2/dark:reqon-ui-states v2/dark" \
    --add-data "$root/reqon-ui-states v2/light:reqon-ui-states v2/light" \
    --add-data "$root/reqon-ui-states v2/manifest.json:reqon-ui-states v2" \
    --name Reqon --distpath dist/linux --workpath build/linux/pyinstaller --specpath build/linux main.py
stage="$root/build/linux/deb-stage"
[[ "$stage" == "$root/build/linux/deb-stage" && "$root" != / ]] || exit 1
rm -rf -- "$stage"
mkdir -p "$stage/opt/reqon" "$stage/usr/bin" "$stage/usr/share/applications" \
    "$stage/usr/share/doc/reqon" "$stage/DEBIAN"
cp -a dist/linux/Reqon/. "$stage/opt/reqon/"
for icon_size in 16 24 32 48 64 128 256 512; do
    icon_dir="$stage/usr/share/icons/hicolor/${icon_size}x${icon_size}/apps"
    mkdir -p "$icon_dir"
    install -m 644 "Reqon_Icon_Pack_Approved/png/transparent/reqon-${icon_size}.png" "$icon_dir/reqon.png"
done
install -m 644 README.md "$stage/usr/share/doc/reqon/README.md"
printf '#!/bin/sh\nexec /opt/reqon/Reqon "$@"\n' > "$stage/usr/bin/getman"
chmod 755 "$stage/usr/bin/getman"
cp "$stage/usr/bin/getman" "$stage/usr/bin/reqon"
cat > "$stage/usr/share/applications/reqon.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=Reqon
GenericName=Local API Client
Comment=Local HTTP and gRPC collections
Comment[ru]=Локальный клиент HTTP и gRPC
Exec=reqon
TryExec=reqon
Icon=reqon
Categories=Development;Network;
Terminal=false
StartupWMClass=Reqon
EOF
size="$(du -sk "$stage/opt" | cut -f1)"
cat > "$stage/DEBIAN/control" <<EOF
Package: reqon
Conflicts: getman
Replaces: getman
Provides: getman
Version: $version
Section: devel
Priority: optional
Architecture: amd64
Maintainer: Reqon <local@localhost>
Installed-Size: $size
Depends: ca-certificates, libc6 (>= 2.36), libstdc++6 (>= 12), libgcc-s1, libgl1, libegl1, libopengl0, libglib2.0-0, libdbus-1-3, libfontconfig1, libfreetype6, libx11-6, libx11-xcb1, libxext6, libxrender1, libsm6, libice6, libxkbcommon0, libxkbcommon-x11-0, libxcb1, libxcb-shape0, libxcb-cursor0, libxcb-icccm4, libxcb-image0, libxcb-keysyms1, libxcb-render-util0, libxcb-xinerama0, libxcb-xkb1
Description: Reqon local native HTTP and gRPC API client
 Qt Widgets desktop application with local collections, environments,
 autosave, protobuf reflection and portable collection import/export.
 Built for Astra Linux 1.8 amd64. No cloud account or browser engine.
EOF
find "$stage" -type d -exec chmod 755 {} +
chmod 644 "$stage/DEBIAN/control"
target="$root/dist/reqon_${version}_astra1.8_amd64.deb"
dpkg-deb --root-owner-group -Zxz -z1 --build "$stage" "$target"
dpkg-deb --info "$target"
"$python_bin" -m pip freeze > "dist/reqon_${version}_linux-dependencies.txt"
cp /etc/os-release "dist/reqon_${version}_linux-build-os.txt"
sha256sum "$target" > "$target.sha256"
printf '\nCreated %s\n' "$target"
