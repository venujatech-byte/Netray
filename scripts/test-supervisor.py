#!/usr/bin/env python3
"""Exercise Linux core ownership without privileges, routes, or a real server."""
import os
from pathlib import Path
import subprocess
import shutil
import tempfile
import time
import unittest

SCRIPT = (Path(__file__).resolve().parents[1] / "src/tunnel.rs").read_text().split(
    'let supervisor_script = r#"', 1
)[1].split('"#;', 1)[0]


class SupervisorTests(unittest.TestCase):
    def run_supervisor(self, exit_early, shell):
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
            errors = tempfile.TemporaryFile()
            child = subprocess.Popen(
                [shell, "-c", SCRIPT, "_", str(core), str(config)],
                stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=errors, env=env,
            )
            try:
                deadline = time.monotonic() + 5
                while (not pid_file.exists() or not pid_file.stat().st_size) and time.monotonic() < deadline and child.poll() is None:
                    time.sleep(0.01)
                errors.seek(0)
                self.assertTrue(pid_file.exists() and pid_file.stat().st_size, errors.read().decode(errors="replace"))
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
                errors.close()

    def test_core_exit_reaches_parent(self):
        for shell in ("sh", "bash", "dash"):
            if shutil.which(shell):
                with self.subTest(shell=shell):
                    self.run_supervisor(True, shell)

    def test_parent_disconnect_reaps_core(self):
        for shell in ("sh", "bash", "dash"):
            if shutil.which(shell):
                with self.subTest(shell=shell):
                    self.run_supervisor(False, shell)


if __name__ == "__main__":
    unittest.main()
