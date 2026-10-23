#!/usr/bin/env bash
set -euo pipefail

app_version=${1:-0.1.0}
if [[ ! "$app_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "Version must use MAJOR.MINOR.PATCH format." >&2
    exit 1
fi

version=v26.10.21
case "$(uname -m)" in
    x86_64) asset_arch=linux-64 ;;
    aarch64|arm64) asset_arch=linux-arm64-v8a ;;
    *) echo "Unsupported Linux architecture: $(uname -m)" >&2; exit 1 ;;
esac

asset="Xray-$asset_arch.zip"
base_url="https://github.com/XTLS/Xray-core/releases/download/$version"
work=$(mktemp -d)
package="$work/netray-linux-$asset_arch"
trap 'rm -rf "$work"' EXIT

cargo build --release
mkdir -p "$package" dist
curl --fail --location --silent --show-error "$base_url/$asset" -o "$work/$asset"
curl --fail --location --silent --show-error "$base_url/$asset.dgst" -o "$work/$asset.dgst"

expected=$(awk 'tolower($1) ~ /^sha(2-)?256=/ { sub(/^[^=]*=/, "", $1); if ($1 == "") $1 = $2; print $1; exit } ($1 == "SHA256" || $1 == "SHA2-256") && $3 == "=" { print $4; exit }' "$work/$asset.dgst")
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

archive="dist/netray-linux-$asset_arch-$app_version.tar.gz"
tar -czf "$archive" -C "$package" .

if ! command -v rpmbuild >/dev/null; then
    echo "rpmbuild is required to create the RPM." >&2
    exit 1
fi
topdir="$work/rpmbuild"
mkdir -p "$topdir/SOURCES" "$work/rpmroot/opt/netray" "$work/rpmroot/usr/bin" \
    "$work/rpmroot/usr/share/applications" "$work/rpmroot/usr/share/icons/hicolor/scalable/apps"
cp -a "$package/." "$work/rpmroot/opt/netray/"
mkdir -p "$work/rpmroot/usr/share/licenses/netray"
cp packaging/LICENSE-Netray.txt "$work/rpmroot/usr/share/licenses/netray/"
cp packaging/netray.desktop "$work/rpmroot/usr/share/applications/"
cp packaging/netray.svg "$work/rpmroot/usr/share/icons/hicolor/scalable/apps/"
ln -s /opt/netray/netray "$work/rpmroot/usr/bin/netray"
tar -czf "$topdir/SOURCES/netray-payload.tar.gz" -C "$work/rpmroot" .
rpmbuild -bb packaging/netray.spec \
    --define "_topdir $topdir" \
    --define "_sourcedir $topdir/SOURCES" \
    --define "netray_version $app_version" \
    --define "netray_release 1"
find "$topdir/RPMS" -name '*.rpm' -exec cp {} dist/ \; -printf 'RPM package: dist/%f\n'

echo "Portable package: $archive"
