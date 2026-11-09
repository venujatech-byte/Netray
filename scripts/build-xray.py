#!/usr/bin/env python3
"""Build and package the explicitly patched MPL-2.0 core, including its source."""
from pathlib import Path
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile

# Keep the patched source aligned with the release assets and geo data.
COMMIT = '1b1647cd9ddf0abc34b981d4611fc03bcac1ca74'
output = Path(sys.argv[1]).resolve()
output.parent.mkdir(parents=True, exist_ok=True)
with tempfile.TemporaryDirectory(prefix='netray-xray-') as work:
    source = Path(work) / 'xray-core'
    source.mkdir()
    subprocess.run(['git', 'init', '--quiet', str(source)], check=True)
    subprocess.run(['git', '-C', str(source), 'fetch', '--quiet', '--depth=1', 'https://github.com/XTLS/Xray-core.git', COMMIT], check=True)
    subprocess.run(['git', '-C', str(source), 'checkout', '--quiet', COMMIT], check=True)
    subprocess.run([sys.executable, str(Path(__file__).with_name('patch-xray-tls.py')), str(source)], check=True)
    shutil.copy2(Path(__file__).with_name('test-xray-tls.go'), source / 'transport/internet/tls/netray_insecure_test.go')
    # Build a portable x86-64 core, including for older CPUs than the CI runner.
    go_env = dict(os.environ, GOTOOLCHAIN='go1.27.3', GOAMD64='v1')
    subprocess.run(['go', 'test', './transport/internet/tls', '-run', '^TestNetrayAllowInsecure$', '-count=1'], cwd=source, env=go_env, check=True)
    subprocess.run(['go', 'build', '-buildvcs=false', '-trimpath', '-ldflags=-s -w -buildid=', '-o', str(output), './main'], cwd=source, env=go_env, check=True)
    subprocess.run([str(output), 'version'], env=go_env, stdout=subprocess.DEVNULL, check=True)
    with tarfile.open(output.parent / 'XRAY-SOURCE.tar.gz', 'w:gz') as archive:
        for entry in source.iterdir():
            if entry.name != '.git':
                archive.add(entry, arcname='xray-core/' + entry.name)
    shutil.copy2(source / 'LICENSE', output.parent / 'LICENSE-Xray')
    shutil.copy2(Path(__file__).with_name('patch-xray-tls.py'), output.parent / 'patch-xray-tls.py')
