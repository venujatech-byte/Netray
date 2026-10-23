#!/usr/bin/env bash
set -euo pipefail

version=v26.10.21
case "$(uname -m)" in
    x86_64) asset_arch=linux-64 ;;
    aarch64|arm64) asset_arch=linux-arm64-v8a ;;
    *) echo "Unsupported Linux architecture: $(uname -m)" >&2; exit 1 ;;
esac

asset="Xray-$asset_arch.zip"
base_url="https://github.com/XTLS/Xray-core/releases/download/$version"
work=$(mktemp -d)
package="dist/netray-linux-$asset_arch"
trap 'rm -rf "$work"' EXIT

cargo build --release
mkdir -p "$package" dist
curl --fail --location --silent --show-error "$base_url/$asset" -o "$work/$asset"
curl --fail --location --silent --show-error "$base_url/$asset.dgst" -o "$work/$asset.dgst"

expected=$(awk '$1 == "SHA256" || $1 == "SHA2-256" { print $NF; exit }' "$work/$asset.dgst")
actual=$(sha256sum "$work/$asset" | awk '{ print $1 }')
if [[ -z "$expected" || "${actual,,}" != "${expected,,}" ]]; then
    echo "Xray release SHA-256 checksum is missing or does not match." >&2
    exit 1
fi

mkdir "$work/extract"
unzip -oq "$work/$asset" -d "$work/extract"
cp -a "$work/extract/." "$package/"
cp target/release/netray "$package/netray"
chmod +x "$package/netray" "$package/xray"
cat > "$package/THIRD-PARTY-NOTICES.txt" <<EOF
Netray bundles the unmodified Xray-core $version executable from XTLS/Xray-core.
License: Mozilla Public License 2.0; see the accompanying LICENSE file.
Corresponding source: https://github.com/XTLS/Xray-core/tree/$version
Dependency list: https://github.com/XTLS/Xray-core/blob/$version/go.mod
EOF

archive="dist/netray-linux-$asset_arch.tar.gz"
tar -czf "$archive" -C "$package" .
echo "Portable release package: $archive"
