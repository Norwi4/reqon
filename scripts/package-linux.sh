#!/usr/bin/env bash
set -euo pipefail

source_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$source_dir/Cargo.toml" | head -n 1 | tr -d '\r')
binary=${1:-${CARGO_TARGET_DIR:-$source_dir/target}/release/reqon}
output=${2:-$source_dir/dist}
cli_binary=${3:-$(dirname "$binary")/reqon-cli}
[[ $(uname -m) == x86_64 ]] || { echo 'This package targets Linux x86_64.' >&2; exit 1; }
test -x "$binary"
test -x "$cli_binary"
test "$("$cli_binary" --version)" = "reqon-cli $version"
mkdir -p "$output"
output=$(cd "$output" && pwd)
stage=$(mktemp -d "${TMPDIR:-/tmp}/reqon-linux-package.XXXXXX")
trap 'rm -rf -- "$stage"' EXIT
portable="$stage/Reqon"
mkdir -p "$portable/licenses"
install -m 755 "$binary" "$portable/reqon"
install -m 755 "$cli_binary" "$portable/reqon-cli"
install -m 644 "$source_dir/docs/cli-runner.md" "$portable/"
install -m 644 "$source_dir/THIRD_PARTY_NOTICES.md" "$portable/"
install -m 644 "$source_dir/docs/release-$version.md" "$portable/Release-notes.md"
install -m 644 "$source_dir/docs/linux.md" "$portable/README.md"
install -m 644 "$source_dir/docs/upgrade-0.20.0.md" "$portable/Upgrade-0.20.0.md"
install -m 644 "$source_dir/docs/dependencies.json" "$portable/licenses/"
install -m 644 "$source_dir/assets/BRUNO-LICENSE.txt" "$portable/licenses/interface-MIT.txt"
cp -R "$source_dir/assets/licenses" "$portable/licenses/libraries"
find "$source_dir/assets/fonts" -maxdepth 1 -type f -name '*.txt' -exec cp {} "$portable/licenses/" \;
install -m 644 "$source_dir/assets/brand/circular-v1/png/reqon-256.png" "$portable/reqon.png"
install -m 644 "$source_dir/packaging/reqon.desktop" "$portable/reqon.desktop"
find "$portable" -type d -exec chmod 755 {} +
find "$portable" -type f ! -name reqon ! -name reqon-cli -exec chmod 644 {} +
epoch=${SOURCE_DATE_EPOCH:-$(date +%s)}
tar --sort=name --mtime="@$epoch" --owner=0 --group=0 --numeric-owner -C "$stage" -czf "$output/Reqon-$version-linux-x86_64.tar.gz" Reqon

deb="$stage/debian-package"
mkdir -p "$deb/DEBIAN" "$deb/usr/bin" "$deb/usr/share/doc/reqon" "$deb/usr/share/applications"
install -m 755 "$binary" "$deb/usr/bin/reqon"
install -m 755 "$cli_binary" "$deb/usr/bin/reqon-cli"
cp -R "$portable/licenses" "$deb/usr/share/doc/reqon/"
install -m 644 "$portable/THIRD_PARTY_NOTICES.md" "$deb/usr/share/doc/reqon/copyright"
install -m 644 "$portable/README.md" "$portable/Release-notes.md" "$portable/Upgrade-0.20.0.md" "$portable/cli-runner.md" "$deb/usr/share/doc/reqon/"
install -m 644 "$source_dir/packaging/reqon.desktop" "$deb/usr/share/applications/"
for size in 16 24 32 48 64 128 256 512; do
  icon_dir="$deb/usr/share/icons/hicolor/${size}x${size}/apps"
  mkdir -p "$icon_dir"
  install -m 644 "$source_dir/assets/brand/circular-v1/png/reqon-$size.png" "$icon_dir/reqon.png"
done
mkdir -p "$stage/debian"
cat > "$stage/debian/control" <<EOF
Source: reqon
Section: devel
Priority: optional
Maintainer: Reqon <local@localhost>

Package: reqon
Architecture: amd64
Description: Reqon API client
EOF
shlibs=$(cd "$stage" && dpkg-shlibdeps -O -e"$deb/usr/bin/reqon" -e"$deb/usr/bin/reqon-cli" | sed -n 's/^shlibs:Depends=//p')
test -n "$shlibs"
cat > "$deb/DEBIAN/control" <<EOF
Package: reqon
Version: $version
Architecture: amd64
Maintainer: Reqon <local@localhost>
Section: devel
Priority: optional
Homepage: https://reqon.ru/
Installed-Size: $(du -sk "$deb/usr" | cut -f1)
Depends: $shlibs, libsecret-tools, ca-certificates, libxkbcommon0, libxkbcommon-x11-0, libx11-6, libx11-xcb1, libxcursor1, libxi6, libxrandr2, libxinerama1, libxcb1, libxcb-render0, libxcb-shape0, libxcb-xfixes0, libwayland-client0, libwayland-cursor0, libwayland-egl1, libegl1, libgl1, libvulkan1
Recommends: gnome-keyring, git, xdg-desktop-portal, xdg-desktop-portal-gtk, mesa-vulkan-drivers
Description: Native API client for HTTP, GraphQL, gRPC and WebSocket
 Reqon provides local collections, environments, JavaScript tests,
 Git integration and an integrated terminal.
EOF
(cd "$deb" && find usr -type f -print0 | sort -z | xargs -0 md5sum > DEBIAN/md5sums)
desktop-file-validate "$deb/usr/share/applications/reqon.desktop"
dpkg-deb --root-owner-group -Zxz --build "$deb" "$output/reqon_${version}_amd64.deb"
(cd "$output" && sha256sum "Reqon-$version-linux-x86_64.tar.gz" "reqon_${version}_amd64.deb" > "Reqon-$version-linux-SHA256SUMS.txt")
echo "Packaged Reqon $version for Linux x86_64."
