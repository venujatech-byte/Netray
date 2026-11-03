#!/usr/bin/env bash
set -euo pipefail

shopt -s nullglob
rpms=(dist/*.rpm)
debs=(dist/*.deb)
arch_packages=(dist/*.pkg.tar.zst)
(( ${#rpms[@]} && ${#debs[@]} && ${#arch_packages[@]} )) || {
    echo "Expected RPM, DEB, and Arch packages in dist/." >&2
    exit 1
}

for package in "${rpms[@]}"; do rpm -qpl "$package" >/dev/null; done
for package in "${debs[@]}"; do dpkg-deb --info "$package" >/dev/null; done
for package in "${arch_packages[@]}"; do tar --zstd -tf "$package" >/dev/null; done
