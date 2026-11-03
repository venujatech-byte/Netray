#!/usr/bin/env bash
set -euo pipefail

version=${1:?Usage: package-arch.sh VERSION PAYLOAD_ROOT OUTPUT_DIR}
payload_root=${2:?Usage: package-arch.sh VERSION PAYLOAD_ROOT OUTPUT_DIR}
output_dir=${3:?Usage: package-arch.sh VERSION PAYLOAD_ROOT OUTPUT_DIR}
command -v zstd >/dev/null || { echo "zstd is required." >&2; exit 1; }

case "$(uname -m)" in
    x86_64) arch=x86_64 ;;
    aarch64|arm64) arch=aarch64 ;;
    *) echo "Unsupported Arch package architecture: $(uname -m)" >&2; exit 1 ;;
esac

root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT
cp -a "$payload_root/." "$root/"
cat > "$root/.PKGINFO" <<EOF
pkgname = netray
pkgbase = netray
pkgver = ${version}-1
pkgdesc = Desktop client for V2Ray and SSH tunnels
url = https://github.com/venujatech-byte/Netray
builddate = $(date +%s)
packager = Netray Contributors
size = $(du -sb "$root" | cut -f1)
arch = $arch
license = custom
depend = alsa-lib
depend = systemd-libs
depend = libxkbcommon
depend = libx11
depend = wayland
depend = vulkan-icd-loader
EOF
mkdir -p "$output_dir"
tar --zstd -cf "$output_dir/netray-${version}-1-${arch}.pkg.tar.zst" -C "$root" .PKGINFO opt usr
