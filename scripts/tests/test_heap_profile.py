import json
import os
from pathlib import Path
import shlex
import signal
import subprocess
import sys
import tempfile
import time
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "heap-profile.sh"


class HeapProfileTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.report = self.root / "reports"
        self.capture = self.root / "capture.json"
        self.executable("uname", '#!/bin/sh\nprintf "%s\\n" "${TEST_PLATFORM:-Linux}"\n')
        self.executable("sha256sum", '#!/bin/sh\nprintf "test-hash  %s\\n" "$2"\n')
        self.executable("heaptrack", f"#!{sys.executable}\n" + '''
import json, os, pathlib, sys
if sys.argv[1:] == ["--version"]:
    print("heaptrack test fixture")
    sys.exit(0)
pathlib.Path(os.environ["TEST_CAPTURE"]).write_text(json.dumps({
    "argv": sys.argv[1:],
    "diagnostics": os.environ.get("FFPLAYOUT_PROCESS_DIAGNOSTICS"),
    "cwd": os.getcwd(),
}))
assert sys.argv[1] == "--record-only"
assert sys.argv[2] == "--output"
pathlib.Path(sys.argv[3] + ".gz").write_bytes(b"test fixture, not a real profile")
print("recording fixture stdout")
print("recording fixture stderr", file=sys.stderr)
sys.exit(int(os.environ.get("TEST_EXIT_CODE", "0")))
''')
        self.binary = self.executable("ffplayout test", "#!/bin/sh\nexit 0\n")
        self.env = {
            **os.environ,
            "PATH": str(self.bin) + os.pathsep + os.defpath,
            "TEST_CAPTURE": str(self.capture),
            "TEST_PLATFORM": "Linux",
        }

    def executable(self, name, content):
        path = self.bin / name
        path.write_text(content)
        path.chmod(0o755)
        return path

    def run_script(self, *arguments, **environment):
        return subprocess.run(
            ["/bin/bash", str(SCRIPT), *map(str, arguments)],
            env={**self.env, **environment}, cwd=self.root,
            capture_output=True, text=True, timeout=10,
        )

    def record(self, *arguments, **environment):
        return self.run_script(
            "--output-dir", self.report, "--", self.binary, *arguments, **environment
        )

    def test_records_logs_arguments_and_enables_diagnostics(self):
        result = self.record("--db", "a path/test.db", "--foreground")
        self.assertEqual(result.returncode, 0, result.stderr)
        capture = json.loads(self.capture.read_text())
        self.assertEqual(capture["diagnostics"], "true")
        self.assertEqual(capture["cwd"], str(self.root.resolve()))
        self.assertEqual(Path(capture["argv"][3]).resolve(), self.binary.resolve())
        self.assertEqual(capture["argv"][4:], [
            "--db", "a path/test.db", "--foreground",
            "--log-level", "debug", "--log-to-console", "--log-timestamp",
        ])
        report = next(self.report.iterdir())
        self.assertEqual(report.stat().st_mode & 0o777, 0o700)
        self.assertEqual((report / "exit-code.txt").read_text(), "0\n")
        self.assertIn("fixture stdout", (report / "playout.log").read_text())
        self.assertIn("fixture stderr", (report / "playout.log").read_text())
        self.assertTrue((report / "heaptrack.gz").is_file())
        command = shlex.split((report / "command.txt").read_text())
        self.assertEqual(Path(command[0]).resolve(), self.binary.resolve())

    def test_preserves_explicit_logging_options_without_duplicates(self):
        result = self.record("--log-level=trace", "--log-to-console", "--log-timestamp")
        self.assertEqual(result.returncode, 0, result.stderr)
        args = json.loads(self.capture.read_text())["argv"]
        self.assertNotIn("--log-level", args)
        self.assertEqual(args.count("--log-to-console"), 1)
        self.assertEqual(args.count("--log-timestamp"), 1)

    def test_propagates_failure_and_keeps_its_log(self):
        result = self.record(TEST_EXIT_CODE="7")
        self.assertEqual(result.returncode, 7)
        report = next(self.report.iterdir())
        self.assertEqual((report / "exit-code.txt").read_text(), "7\n")
        self.assertIn("fixture stderr", (report / "playout.log").read_text())

    def test_repeated_runs_never_overwrite_previous_profile(self):
        self.assertEqual(self.record().returncode, 0)
        self.assertEqual(self.record().returncode, 0)
        self.assertEqual(len(list(self.report.glob("*/heaptrack.gz"))), 2)

    def test_argumentless_start_preserves_default_listener_and_its_environment(self):
        for listen in [None, "127.0.0.1:9999"]:
            with self.subTest(listen=listen):
                env = self.env.copy()
                env.pop("LISTEN", None)
                if listen is not None:
                    env["LISTEN"] = listen
                result = subprocess.run(
                    ["/bin/bash", str(SCRIPT), "--output-dir", str(self.report), "--", str(self.binary)],
                    env=env, cwd=self.root, capture_output=True, text=True, timeout=10,
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                args = json.loads(self.capture.read_text())["argv"]
                self.assertEqual(args[4:6], ["--listen", listen or "127.0.0.1:8787"])

    @unittest.skipUnless(hasattr(os, "umask"), "requires Unix permissions")
    def test_application_keeps_callers_umask_and_report_stays_private(self):
        self.executable("heaptrack", f"#!{sys.executable}\n" + '''
import pathlib, subprocess, sys
if sys.argv[1:] == ["--version"]:
    print("heaptrack test fixture")
    sys.exit(0)
sys.exit(subprocess.run(sys.argv[4:]).returncode)
''')
        self.executable("ffplayout test", f"#!{sys.executable}\n" + '''
import os, pathlib
pathlib.Path(os.environ["TEST_SEGMENT"]).write_text("HLS segment")
''')
        for mask, expected in [(0o022, 0o644), (0o002, 0o664), (0o077, 0o600)]:
            with self.subTest(mask=mask):
                segment = self.root / f"segment-{mask}.ts"
                result = subprocess.run(
                    ["/bin/bash", str(SCRIPT), "--output-dir", str(self.report), "--", str(self.binary)],
                    env={**self.env, "TEST_SEGMENT": str(segment)}, cwd=self.root,
                    capture_output=True, text=True, timeout=10,
                    preexec_fn=lambda: os.umask(mask),
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(segment.stat().st_mode & 0o777, expected)
        for report in self.report.iterdir():
            self.assertEqual(report.stat().st_mode & 0o777, 0o700)
            for name in ["playout.log", "exit-code.txt", "command.txt"]:
                self.assertEqual((report / name).stat().st_mode & 0o777, 0o600)

    @unittest.skipUnless(hasattr(os, "killpg"), "requires Unix process groups")
    def test_ctrl_c_waits_for_recording_finalization_and_keeps_its_status(self):
        self.executable("heaptrack", f"#!{sys.executable}\n" + '''
import os, pathlib, signal, sys, time
if sys.argv[1:] == ["--version"]:
    print("heaptrack test fixture")
    sys.exit(0)
def finish(signum, frame):
    time.sleep(0.2)
    print("finalizing recording", flush=True)
    pathlib.Path(sys.argv[3] + ".gz").write_text("complete profile")
    print("recording saved", flush=True)
    sys.exit(int(os.environ["TEST_EXIT_CODE"]))
signal.signal(signal.SIGINT, finish)
print("recording started", flush=True)
while True:
    time.sleep(0.05)
''')
        for status in [0, 7]:
            with self.subTest(status=status):
                previous = set(self.report.iterdir()) if self.report.exists() else set()
                process = subprocess.Popen(
                    ["/bin/bash", str(SCRIPT), "--output-dir", str(self.report), "--", str(self.binary)],
                    env={**self.env, "TEST_EXIT_CODE": str(status)}, cwd=self.root,
                    stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
                    start_new_session=True,
                )
                try:
                    deadline = time.monotonic() + 5
                    report = None
                    while time.monotonic() < deadline:
                        reports = set(self.report.iterdir()) if self.report.exists() else set()
                        created = reports - previous
                        if created:
                            report = created.pop()
                            log = report / "playout.log"
                            if log.exists() and "recording started" in log.read_text():
                                break
                        time.sleep(0.02)
                    else:
                        self.fail("recorder did not start")
                    os.killpg(process.pid, signal.SIGINT)
                    output = process.communicate(timeout=5)[0]
                finally:
                    if process.poll() is None:
                        os.killpg(process.pid, signal.SIGKILL)
                        process.communicate(timeout=5)
                self.assertEqual(process.returncode, status, output)
                self.assertEqual((report / "heaptrack.gz").read_text(), "complete profile")
                self.assertEqual((report / "exit-code.txt").read_text(), f"{status}\n")
                self.assertIn("recording saved", (report / "playout.log").read_text())
                self.assertIn("Profile and log:", output)

    def test_rejects_non_linux_without_creating_a_report(self):
        result = self.record(TEST_PLATFORM="Darwin")
        self.assertEqual(result.returncode, 2)
        self.assertIn("macOS use Instruments", result.stderr)
        self.assertFalse(self.report.exists())

    def test_help_and_invalid_invocations_do_not_start_a_process(self):
        self.assertEqual(self.run_script("--help", TEST_PLATFORM="Darwin").returncode, 0)
        for arguments in [[], ["--output-dir"], ["--duration", "60"], ["--", "/missing/ffplayout"]]:
            with self.subTest(arguments=arguments):
                self.assertEqual(self.run_script(*arguments).returncode, 2)
        self.assertFalse(self.capture.exists())


if __name__ == "__main__":
    unittest.main()
