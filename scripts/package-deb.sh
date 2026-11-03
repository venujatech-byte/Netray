#!/usr/bin/env bash
set -euo pipefail

version=${1:?Usage: package-deb.sh VERSION PAYLOAD_ROOT OUTPUT_DIR}
payload_root=${2:?Usage: package-deb.sh VERSION PAYLOAD_ROOT OUTPUT_DIR}
output_dir=${3:?Usage: package-deb.sh VERSION PAYLOAD_ROOT OUTPUT_DIR}
command -v dpkg-deb >/dev/null || { echo "dpkg-deb is required." >&2; exit 1; }

case "$(uname -m)" in
    x86_64) arch=amd64 ;;
    aarch64|arm64) arch=arm64 ;;
    *) echo "Unsupported Debian package architecture: $(uname -m)" >&2; exit 1 ;;
esac

root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT
mkdir -p "$root/DEBIAN"
cp -a "$payload_root/." "$root/"
cat > "$root/DEBIAN/control" <<EOF
Package: netray
Version: $version
Section: net
Priority: optional
Architecture: $arch
Depends: libasound2 | libasound2t64, libudev1, libxkbcommon0, libx11-6, libwayland-client0, libvulkan1
Maintainer: Netray Contributors
Description: Desktop client for V2Ray and SSH tunnels
EOF
mkdir -p "$output_dir"
dpkg-deb --build --root-owner-group "$root" "$output_dir/netray-${version}-1.${arch}.deb"
