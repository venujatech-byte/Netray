# Allow insecure (TLS)

Linux, Windows, and Android store this setting locally, default off. Disconnect before changing it. It applies to TLS outbounds (including imported JSON) and profile ping checks, not Reality or Netray's own HTTPS update/download traffic.

When enabled, certificate chain, expiry, hostname and pin checks are skipped. Custom SNI is preserved. This permits interception by an attacker; prefer a correct verification name or certificate pin where possible. Turning it off restores verification on the next connection.

Official Xray-core removed `allowInsecure`. Netray builds commit `a4a3dff5b20c2faae567b742c172c7b95e75c083` with `scripts/patch-xray-tls.py`. The patch accepts the explicit flag, carries a private marker through the internal configuration, and disables the TLS certificate callback only for that marker. The normal verifier is unchanged for other configurations. The handshake test in `scripts/test-xray-tls.go` checks rejection by default and acceptance only after opt-in.

Release packages include the modified MPL-2.0 core source (`XRAY-SOURCE.tar.gz`), patch, license and notices. Android includes modified source in `assets/licenses/xray-source.tar.gz`. The corresponding source remains MPL-2.0. Build scripts fail if upstream source no longer matches the expected patch anchors. A stock/replacement core that removed this feature cannot honor an enabled setting; its configuration validation will reject it.

Build desktop core: `python3 scripts/build-xray.py /path/to/package/xray` (use `xray.exe` on Windows). Requires Python, Git, network access and Go 1.27.3. Packaging scripts run this automatically. Android uses the official libXray local-core build mode.
