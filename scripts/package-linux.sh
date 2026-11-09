#!/usr/bin/env bash
set -euo pipefail

app_version=${1:-1.0.0}
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
mirror_url="https://sourceforge.net/projects/xray-core.mirror/files/$version"
case "$asset_arch" in
    linux-64) expected="0eec3ed4163c273d8c8bf821f7d219e257921cf584aa3fb970715e458e631978" ;;
    linux-arm64-v8a) expected="d1304946c486d98e6b886e9b931eaaaebb23835b30e217638037b7f2721d4e9c" ;;
esac
work=$(mktemp -d)
package="$work/netray-linux-$asset_arch"
trap 'rm -rf "$work"' EXIT

download_release_asset() {
    local output=$1
    shift
    for url in "$@"; do
        if curl --fail --location --silent --show-error --retry 2 --retry-all-errors \
            --retry-delay 2 --connect-timeout 20 --max-time 180 "$url" -o "$output"; then
            return 0
        fi
        rm -f "$output"
        echo "Download failed; trying the next Xray mirror." >&2
    done
    echo "Could not download $asset from GitHub or SourceForge." >&2
    return 1
}

cargo build --release
mkdir -p "$package" dist
download_release_asset "$work/$asset" "$mirror_url/$asset/download" "$base_url/$asset"

actual=$(sha256sum "$work/$asset" | awk '{ print $1 }')
if [[ -z "$expected" || "${actual,,}" != "${expected,,}" ]]; then
    echo "Xray release SHA-256 checksum is missing or does not match." >&2
    exit 1
fi

mkdir "$work/extract"
unzip -oq "$work/$asset" -d "$work/extract"
cp -a "$work/extract/." "$package/"
python3 scripts/build-xray.py "$package/xray"
cp target/release/netray "$package/netray"
chmod +x "$package/netray" "$package/xray"
cat > "$package/THIRD-PARTY-NOTICES.txt" <<EOF
Netray bundles Xray-core a4a3dff5b20c with a Netray TLS compatibility modification.
License: Mozilla Public License 2.0; see the accompanying LICENSE file.
Corresponding modified source: XRAY-SOURCE.tar.gz; patch: patch-xray-tls.py
Upstream source: https://github.com/XTLS/Xray-core/tree/a4a3dff5b20c
Dependency list: https://github.com/XTLS/Xray-core/blob/a4a3dff5b20c/go.mod
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

bash scripts/package-deb.sh "$app_version" "$work/rpmroot" dist
bash scripts/package-arch.sh "$app_version" "$work/rpmroot" dist

echo "Portable package: $archive"
