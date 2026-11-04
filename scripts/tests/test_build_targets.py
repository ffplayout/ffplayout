"""Check build target routing and packaging without compiling dependencies."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
MOCK = r'''#!/usr/bin/env python3
import json, os, pathlib, sys
args = sys.argv[1:]
with open('calls.jsonl', 'a') as log:
    log.write(json.dumps(args) + '\n')
if args[:2] == ['image', 'inspect']:
    print('test-image|echo build')
elif args[:2] == ['container', 'inspect']:
    sys.exit(1)
elif args[0] == 'create':
    mounts = [args[i+1] for i, arg in enumerate(args) if arg == '-v']
    output = next(m.split(':/artifacts:')[0] for m in mounts if ':/artifacts:' in m)
    pathlib.Path('artifact-path').write_text(output)
elif args[0] == 'exec':
    output = pathlib.Path(pathlib.Path('artifact-path').read_text())
    (output / 'ffplayout').write_text('static test binary')
    if output.name == 'debian-static':
        (output / 'ffplayout_1.2.3-1_amd64.deb').write_text('test package')
'''


class BuildTargetTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / 'scripts/lib').mkdir(parents=True)
        for name in ('build.sh', 'lib/build-container.sh'):
            shutil.copyfile(ROOT / 'scripts' / name, self.root / 'scripts' / name)
        manual = self.root / 'scripts/man_create.sh'
        manual.write_text('#!/bin/sh\nexit 0\n')
        manual.chmod(0o755)
        self.cli = self.root / 'container-cli'
        self.cli.write_text(MOCK)
        self.cli.chmod(0o755)
        (self.root / 'Cargo.toml').write_text('[workspace.package]\nversion = "1.2.3"\n')
        for name in ('assets', 'docker', 'docs', 'migrations'):
            (self.root / name).mkdir()
        for name in ('LICENSE', 'README.md', 'assets/config.db', 'assets/config.db-wal'):
            (self.root / name).write_text('test')

    def run_build(self, target, features='embed_frontend', *options, **settings):
        env = {key: value for key, value in os.environ.items()
               if not key.startswith(('CARGO_', 'FFMPEG_', 'CONTAINER_'))}
        env.update(CONTAINER_CLI=str(self.cli), CARGO_FEATURES=features, **settings)
        return subprocess.run(['/bin/bash', 'scripts/build.sh', target, *options],
                              cwd=self.root, env=env, capture_output=True, text=True)

    def calls(self):
        path = self.root / 'calls.jsonl'
        return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []

    def test_musl_routes_and_packages_separately(self):
        result = self.run_build('alpine-static', 'embed_frontend,ffmpeg-filter,ffmpeg-device')
        self.assertEqual(result.returncode, 0, result.stderr)
        build = next(args for args in self.calls() if args[0] == 'build')
        self.assertIn('./docker/musl.Dockerfile', build)
        self.assertIn('FFMPEG_AVFILTER=1', build)
        self.assertIn('FFMPEG_AVDEVICE=1', build)
        self.assertIn('localhost/ffplayout-musl-builder:latest', build)
        archive = self.root / 'ffplayout-v1.2.3_linux-musl-x86_64.tar.gz'
        with tarfile.open(archive) as package:
            self.assertEqual(package.extractfile('ffplayout').read(), b'static test binary')
            self.assertNotIn('assets/config.db', package.getnames())
            self.assertNotIn('assets/config.db-wal', package.getnames())
        self.assertFalse(list((self.root / 'target').glob('.*.lock')))

    def test_musl_defaults_to_server_features(self):
        result = self.run_build('alpine-static', '')
        self.assertEqual(result.returncode, 0, result.stderr)
        execute = next(args for args in self.calls() if args[0] == 'exec')
        self.assertIn('CARGO_FEATURES=embed_frontend', execute)

    def test_debian_static_keeps_its_builder_and_package(self):
        result = self.run_build('debian-static')
        self.assertEqual(result.returncode, 0, result.stderr)
        build = next(args for args in self.calls() if args[0] == 'build')
        self.assertIn('./docker/static.Dockerfile', build)
        self.assertTrue((self.root / 'ffplayout_1.2.3-1_amd64.deb').is_file())

    def test_unsupported_musl_features_fail_before_building(self):
        for feature in ('desktop', 'desktop-cpu', 'desktop-base', 'ff-engine/desktop', 'ff-engine/desktop-gpu'):
            with self.subTest(feature=feature):
                result = self.run_build('alpine-static', f'embed_frontend,{feature}')
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('server outputs only', result.stderr)
                self.assertEqual(self.calls(), [])

    def test_musl_rejects_vaapi(self):
        result = self.run_build('alpine-static', FFMPEG_VAAPI='1')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('does not support VAAPI', result.stderr)
        self.assertEqual(self.calls(), [])

    def test_profiling_settings_reach_musl_container(self):
        result = self.run_build('alpine-static', 'embed_frontend', '--profiling')
        self.assertEqual(result.returncode, 0, result.stderr)
        execute = next(args for args in self.calls() if args[0] == 'exec')
        self.assertIn('CARGO_PROFILE_RELEASE_DEBUG=1', execute)
        self.assertIn('CARGO_PROFILE_RELEASE_STRIP=false', execute)


if __name__ == '__main__':
    unittest.main()
