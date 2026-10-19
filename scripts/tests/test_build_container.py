import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from concurrent.futures import ThreadPoolExecutor

HELPER = Path(__file__).resolve().parents[1] / 'lib' / 'build-container.sh'


class BuildContainerTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.state = self.root / 'state.json'
        self.events = self.root / 'events.jsonl'
        self.cli = self.root / 'container-cli'
        self.cli.write_text(f'#!{sys.executable}\n' + '''
import json, os, pathlib, sys, time
args = sys.argv[1:]
statefile = pathlib.Path(os.environ['TEST_STATE'])
state = json.loads(statefile.read_text()) if statefile.exists() else None
with open(os.environ['TEST_EVENTS'], 'a') as log:
    log.write(json.dumps(args) + '\\n')
def save():
    statefile.write_text(json.dumps(state))
if args[:2] == ['image', 'inspect']:
    if os.environ.get('TEST_BLOCK') and args[3] == '{{.Id}}':
        marker = pathlib.Path(os.environ['TEST_BLOCK'])
        marker.touch()
        deadline = time.monotonic() + 5
        while not marker.with_suffix('.release').exists():
            if time.monotonic() >= deadline: sys.exit(9)
            time.sleep(0.01)
    print(os.environ.get('TEST_IMAGE', 'image-1') if args[3] == '{{.Id}}' else 'echo build')
elif args[:2] == ['container', 'inspect']:
    if state is None: sys.exit(1)
    if '--format' in args:
        fmt = args[3]
        if 'workspace' in fmt: print(state['workspace'])
        elif 'created' in fmt: print(state['created'])
        elif 'Running' in fmt: print(str(state['running']).lower())
        elif fmt == '{{.Image}}': print(state['image'])
    else: print('{}')
elif args[0] == 'create':
    assert state is None
    labels = [args[i+1] for i, arg in enumerate(args) if arg == '--label']
    labels = dict(label.split('=', 1) for label in labels)
    state = {'workspace': labels['ffplayout.build.workspace'],
             'created': int(labels['ffplayout.build.created']), 'running': False,
             'image': os.environ.get('TEST_IMAGE', 'image-1'), 'builds': 0}
    save()
elif args[0] == 'rm':
    assert not state['running']
    state = None
    save()
elif args[0] == 'start':
    state['running'] = True
    save()
elif args[0] == 'exec':
    assert state['running']
    state['builds'] += 1
    save()
    sys.exit(int(os.environ.get('TEST_FAIL', '0')))
elif args[0] == 'stop':
    state['running'] = False
    save()
else: raise AssertionError(args)
''')
        self.cli.chmod(0o755)
        self.env = {**os.environ, 'TEST_STATE': str(self.state), 'TEST_EVENTS': str(self.events)}

    def run_build(self, *, target='debian-shared', features='embed_frontend', **env):
        # Exercise the same helper/EXIT cleanup and strict Bash mode as build.sh.
        return subprocess.run(['/bin/bash', '-c', '''
set -euo pipefail
source "$1"
container_cli=$2
target=$3
CARGO_FEATURES=$4
env_names=(CARGO_PROFILE_RELEASE_DEBUG CARGO_PROFILE_RELEASE_STRIP)
trap cleanup_build_container EXIT
container_run_env builder-image -v "$PWD:/src:z"
''', 'test-build', str(HELPER), str(self.cli), target, features],
            cwd=self.root, env={**self.env, **env}, text=True, capture_output=True, timeout=10)

    def load(self):
        return json.loads(self.state.read_text())

    def calls(self, command):
        return [args for args in map(json.loads, self.events.read_text().splitlines()) if args[0] == command]

    def change(self, **values):
        state = self.load()
        state.update(values)
        self.state.write_text(json.dumps(state))

    def test_stopped_container_reused_and_cache_preserved(self):
        for _ in range(2):
            result = self.run_build()
            self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(self.calls('create')), 1)
        self.assertEqual(len(self.calls('rm')), 0)
        self.assertEqual(self.load()['builds'], 2)
        self.assertFalse(self.load()['running'])
        self.assertNotIn('--rm', self.calls('create')[0])
        self.assertIn(str(self.root.resolve()) + ':/src:z', self.calls('create')[0])

    def test_older_than_one_week_is_removed_before_recreation(self):
        self.assertEqual(self.run_build().returncode, 0)
        self.change(created=self.load()['created'] - 604801)
        self.assertEqual(self.run_build().returncode, 0)
        self.assertEqual(len(self.calls('rm')), 1)
        self.assertEqual(len(self.calls('create')), 2)
        self.assertEqual(self.load()['builds'], 1)

    def test_recent_container_is_not_recreated(self):
        self.assertEqual(self.run_build().returncode, 0)
        self.change(created=self.load()['created'] - 604700)
        self.assertEqual(self.run_build().returncode, 0)
        self.assertEqual(len(self.calls('rm')), 0)

    def test_changed_builder_image_recreates_container(self):
        self.assertEqual(self.run_build().returncode, 0)
        self.assertEqual(self.run_build(TEST_IMAGE='image-2').returncode, 0)
        self.assertEqual(len(self.calls('rm')), 1)
        self.assertEqual(self.load()['image'], 'image-2')

    def test_fresh_profile_and_feature_values_on_every_exec(self):
        self.assertEqual(self.run_build(CARGO_PROFILE_RELEASE_DEBUG='1', CARGO_PROFILE_RELEASE_STRIP='false').returncode, 0)
        env = self.env.copy()
        env.pop('CARGO_PROFILE_RELEASE_DEBUG', None)
        env.pop('CARGO_PROFILE_RELEASE_STRIP', None)
        self.env = env
        self.assertEqual(self.run_build(features='desktop,embed_frontend').returncode, 0)
        first, second = self.calls('exec')
        self.assertIn('CARGO_PROFILE_RELEASE_STRIP=false', first)
        self.assertNotIn('CARGO_PROFILE_RELEASE_STRIP=false', second)
        self.assertIn('CARGO_FEATURES=desktop,embed_frontend', second)
        self.assertEqual(len(self.calls('create')), 1)

    def test_failure_stops_but_keeps_container_and_exit_status(self):
        result = self.run_build(TEST_FAIL='7')
        self.assertEqual(result.returncode, 7)
        self.assertFalse(self.load()['running'])
        self.assertEqual(len(self.calls('rm')), 0)
        self.assertEqual(self.run_build().returncode, 0)
        self.assertEqual(self.load()['builds'], 2)

    def test_running_build_is_never_stopped_or_deleted(self):
        self.assertEqual(self.run_build().returncode, 0)
        self.change(running=True, created=0)
        stops = len(self.calls('stop'))
        self.assertNotEqual(self.run_build().returncode, 0)
        self.assertEqual(len(self.calls('stop')), stops)
        self.assertEqual(len(self.calls('rm')), 0)

    def test_other_workspace_container_is_untouched(self):
        self.assertEqual(self.run_build().returncode, 0)
        self.change(workspace='/some/other/project')
        self.assertNotEqual(self.run_build().returncode, 0)
        self.assertEqual(len(self.calls('rm')), 0)

    def test_concurrent_build_is_rejected_before_container_inspection(self):
        # Pause after acquiring the lock, before the first running-state check.
        # This also covers the race when the container does not exist yet.
        marker = self.root / 'blocked'
        with ThreadPoolExecutor(max_workers=1) as executor:
            first = executor.submit(self.run_build, TEST_BLOCK=str(marker))
            try:
                deadline = time.monotonic() + 5
                while not marker.exists():
                    self.assertLess(time.monotonic(), deadline)
                    time.sleep(0.01)
                calls_before = self.events.read_text()
                second = self.run_build()
                self.assertNotEqual(second.returncode, 0)
                self.assertIn('Build is locked:', second.stderr)
                self.assertEqual(self.events.read_text(), calls_before)
                self.assertEqual(len(list((self.root / 'target').glob('.*.lock'))), 1)
            finally:
                marker.with_suffix('.release').touch()
            result = first.result(timeout=10)
            self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(list((self.root / 'target').glob('.*.lock')), [])
        self.assertEqual(self.run_build().returncode, 0)


if __name__ == '__main__':
    unittest.main()
