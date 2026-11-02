#!/usr/bin/env python3
"""Exercise Linux core ownership without privileges, routes, or a real server."""
import os
from pathlib import Path
import subprocess
import tempfile
import time
import unittest

SCRIPT = (Path(__file__).resolve().parents[1] / "src/tunnel.rs").read_text().split(
    'let supervisor_script = r#"', 1
)[1].split('"#;', 1)[0]


class SupervisorTests(unittest.TestCase):
    def run_supervisor(self, exit_early):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ("ip", "setcap"):
                path = root / name
                path.write_text("#!/bin/sh\nexit 0\n")
                path.chmod(0o700)
            core = root / "xray"
            core.write_text("#!/bin/sh\necho $$ > \"$PID_FILE\"\n" + (
                "exit 7\n" if exit_early else "exec sleep 60\n"
            ))
            core.chmod(0o700)
            config = root / "config.json"
            config.write_text("{}")
            pid_file = root / "core.pid"
            env = dict(os.environ, PATH=f"{root}:{os.environ['PATH']}", PID_FILE=str(pid_file))
            child = subprocess.Popen(
                ["sh", "-c", SCRIPT, "_", str(core), str(config)],
                stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env=env,
            )
            try:
                deadline = time.monotonic() + 5
                while not pid_file.exists() and time.monotonic() < deadline:
                    time.sleep(0.01)
                self.assertTrue(pid_file.exists())
                if exit_early:
                    self.assertEqual(child.wait(timeout=5), 7)
                else:
                    self.assertIsNone(child.poll())
                    child.stdin.close()
                    child.wait(timeout=5)
                self.assertFalse(config.exists())
                with self.assertRaises(ProcessLookupError):
                    os.kill(int(pid_file.read_text()), 0)
            finally:
                child.stdin.close()
                if child.poll() is None:
                    child.terminate()
                    child.wait(timeout=5)

    def test_core_exit_reaches_parent(self):
        self.run_supervisor(True)

    def test_parent_disconnect_reaps_core(self):
        self.run_supervisor(False)


if __name__ == "__main__":
    unittest.main()
