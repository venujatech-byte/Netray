#!/usr/bin/env python3
"""Netray compatibility patch for Xray-core v26.10.21 (MPL-2.0)."""
from pathlib import Path
import sys

MARKER = 'netray-insecure-tls.invalid'


def patch(root):
    root = Path(root)
    conf = root / 'infra/conf/transport_security.go'
    runtime = root / 'transport/internet/tls/config.go'
    text = conf.read_text()
    removed = '''\tif c.AllowInsecure {
\t\treturn nil, errors.PrintRemovedFeatureError(`"allowInsecure"`, `"pinnedPeerCertSha256"(pcs) and "verifyPeerCertByName"(vcn)`)
\t}'''
    assert text.count(removed) == 1, 'Unsupported Xray source: TLS parser changed'
    guard = '\tfor _, name := range strings.Split(c.VerifyPeerCertByName, ",") {\n\t\tif strings.TrimSpace(name) == "netray-insecure-tls.invalid" {\n\t\t\treturn nil, errors.New("reserved Netray certificate name")\n\t\t}\n\t}\n'
    text = text.replace(removed, guard)
    anchor = '\tconfig.UseSystemCa = c.UseSystemCA\n'
    assert text.count(anchor) == 1
    text = text.replace(anchor, anchor + '''
	// Netray: carry explicit certificate bypass through the existing internal config.
	if c.AllowInsecure {
		config.VerifyPeerCertByName = []string{"netray-insecure-tls.invalid"}
		config.PinnedPeerCertSha256 = nil
	}
''')
    conf.write_text(text)
    text = runtime.read_text()
    anchor = '\trandCarrier.Config = config\n'
    assert text.count(anchor) == 1, 'Unsupported Xray source: TLS runtime changed'
    text = text.replace(anchor, anchor + '''
	// Netray: only the explicit allowInsecure parser path installs this marker.
	if len(c.VerifyPeerCertByName) == 1 && c.VerifyPeerCertByName[0] == "netray-insecure-tls.invalid" {
		config.InsecureSkipVerify = true
		config.VerifyPeerCertificate = nil
	}
''')
    runtime.write_text(text)


if __name__ == '__main__':
    patch(sys.argv[1])
