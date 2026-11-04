"""Exercise the signing helper without touching keychains or Apple's services."""

import base64
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "sign-macos.sh"
MOCK = r'''#!/usr/bin/env python3
import json, os, pathlib, sys
name = pathlib.Path(sys.argv[0]).name
args = sys.argv[1:]
with open(os.environ["MOCK_LOG"], "a") as log:
    log.write(json.dumps([name, args]) + "\n")
if name == "uname":
    print("Darwin")
elif name == "security":
    if args[0] == "import" and os.getenv("FAIL_IMPORT"):
        sys.exit(1)
elif name == "openssl":
    if args[0] == "rand":
        print("test-keychain-password")
    else:
        key = pathlib.Path(args[args.index("-in") + 1])
        assert key.read_text().startswith("-----BEGIN PRIVATE KEY-----")
        assert key.stat().st_mode & 0o777 == 0o600
elif name == "codesign":
    if "--display" in args:
        print("Authority=Developer ID Application: Test", file=sys.stderr)
        print("TeamIdentifier=" + os.getenv("MOCK_TEAM", "TESTTEAM"), file=sys.stderr)
    elif "--verify" not in args:
        assert args[args.index("--options") + 1] == "runtime"
        assert "--timestamp" in args
        assert pathlib.Path(args[args.index("--entitlements") + 1]).is_file()
elif name == "xcrun":
    key = pathlib.Path(args[args.index("--key") + 1])
    assert key.is_file()
    if args[1] == "submit":
        assert "--wait" in args
        assert "--timeout" not in args
        print(json.dumps({"id": "test-submission", "status": os.getenv("MOCK_STATUS", "Accepted")}))
        sys.exit(int(os.getenv("MOCK_SUBMIT_EXIT", "0")))
    else:
        print("Mock notarization rejection details")
elif name == "plutil":
    print(json.loads(pathlib.Path(args[-1]).read_text())[args[1]])
'''


class SigningTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        for name in ("uname", "security", "openssl", "codesign", "xcrun", "plutil"):
            executable = self.bin / name
            executable.write_text(MOCK)
            executable.chmod(0o755)
        self.artifact = self.root / "ffplayout.zip"
        self.artifact.write_bytes(b"test artifact")
        self.log = self.root / "calls.jsonl"
        self.env = dict(os.environ)
        self.env.update(
            PATH=str(self.bin) + os.pathsep + os.environ["PATH"],
            RUNNER_TEMP=str(self.root), MOCK_LOG=str(self.log), GITHUB_ACTIONS="false",
            APPLE_CERTIFICATE=base64.b64encode(b"test certificate").decode(),
            APPLE_CERTIFICATE_PASSWORD="test-certificate-password",
            APPLE_SIGNING_IDENTITY="Developer ID Application: Test",
            APPLE_TEAM_ID="TESTTEAM", APPLE_API_KEY_ID="TESTKEY",
            APPLE_API_PRIVATE_KEY="-----BEGIN PRIVATE KEY-----\ntest\n-----END PRIVATE KEY-----",
            APPLE_ISSUER_ID="test-issuer",
        )

    def run_helper(self, operation, **overrides):
        env = self.env | overrides
        arguments = ["bash", "-x", str(SCRIPT), operation]
        if operation != "check":
            arguments.append(str(self.artifact))
        result = subprocess.run(arguments, env=env, text=True, capture_output=True)
        self.assertEqual(list(self.root.glob("ffplayout-signing.*")), [])
        for secret in (env["APPLE_CERTIFICATE_PASSWORD"], env["APPLE_API_PRIVATE_KEY"]):
            self.assertNotIn(secret, result.stdout + result.stderr)
        return result

    def calls(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()]

    def test_missing_setting_fails_before_signing(self):
        result = self.run_helper("check", APPLE_CERTIFICATE="")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Missing macOS signing setting: APPLE_CERTIFICATE", result.stderr)
        self.assertFalse(self.log.exists())

    def test_sign_verifies_and_deletes_keychain(self):
        result = self.run_helper("sign")
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = self.calls()
        self.assertIn(["codesign", ["--verify", "--strict", "--verbose=2", str(self.artifact)]], calls)
        self.assertEqual(calls[-1][1][0], "delete-keychain")
        self.assertFalse(any(call[1][0] in ("list-keychains", "default-keychain") for call in calls))

    def test_wrong_team_fails_and_deletes_keychain(self):
        result = self.run_helper("sign", MOCK_TEAM="WRONGTEAM")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.calls()[-1][1][0], "delete-keychain")

    def test_import_failure_deletes_keychain(self):
        result = self.run_helper("sign", FAIL_IMPORT="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.calls()[-1][1][0], "delete-keychain")

    def test_notarize_accepts_raw_and_base64_keys(self):
        for key in (self.env["APPLE_API_PRIVATE_KEY"], base64.b64encode(self.env["APPLE_API_PRIVATE_KEY"].encode()).decode()):
            with self.subTest(key_format="pem" if key.startswith("-----") else "base64"):
                result = self.run_helper("notarize", APPLE_API_PRIVATE_KEY=key)
                self.assertEqual(result.returncode, 0, result.stderr)

    def test_rejected_submission_fails_even_with_zero_exit(self):
        result = self.run_helper("notarize", MOCK_STATUS="Invalid")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Mock notarization rejection details", result.stdout)
        self.assertEqual(self.calls()[-1][1][:2], ["notarytool", "log"])

    def test_submit_failure_blocks_release_even_if_report_says_accepted(self):
        result = self.run_helper("notarize", MOCK_SUBMIT_EXIT="1")
        self.assertNotEqual(result.returncode, 0)


if __name__ == "__main__":
    unittest.main()
