import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class LogsTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.work = Path(self.temp.name)
        self.out = self.work / 'bundle'
        self.log = self.work / 'calls.jsonl'
        fake = self.work / 'kubectl'
        fake.write_text('''#!/usr/bin/env python3
import json, os, sys
args = sys.argv[1:]
with open(os.environ['CALL_LOG'], 'a') as f:
    f.write(json.dumps(args) + '\\n')
if 'get' in args:
    print(json.dumps({'items': [{'metadata': {'name': name}} for name in ['processor-0', 'processor-1']]}))
else:
    print('pod log line')
''')
        fake.chmod(0o755)
        self.env = dict(os.environ, PATH=str(self.work) + os.pathsep + os.environ['PATH'], CALL_LOG=str(self.log))

    def run_logs(self, *extra):
        return subprocess.run([os.environ.get('DB3_BINARY', str(ROOT / 'kubectl-db3')), 'support', 'logs', 'processor', '-n', 'influxdb3', '--output', str(self.out), *extra], env=self.env, capture_output=True, text=True)

    def test_discovers_processor_pods_and_collects_all_container_logs(self):
        result = self.run_logs('--context', 'test-cluster')
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = [json.loads(line) for line in self.log.read_text().splitlines()]
        prefix = ['--context', 'test-cluster', '--namespace', 'influxdb3']
        self.assertEqual(calls[0], prefix + ['get', 'pods', '-l', 'app.kubernetes.io/component=processor', '-o', 'json'])
        self.assertEqual(len(calls), 3)
        for call, pod in zip(calls[1:], ['processor-0', 'processor-1']):
            self.assertEqual(call, prefix + ['logs', pod, '--all-containers=true', '--prefix=true', '--timestamps=true', '--tail=10000'])
            self.assertEqual((self.out / f'{pod}.log').read_text(), 'pod log line\n')
        manifest = json.loads((self.out / 'manifest.json').read_text())
        self.assertTrue(manifest['complete'])
        self.assertEqual(manifest['role'], 'processor')
        self.assertEqual(len(manifest['results']), 2)
        self.assertEqual(self.out.stat().st_mode & 0o777, 0o700)
        for file in self.out.iterdir():
            self.assertEqual(file.stat().st_mode & 0o777, 0o600)

    def test_discovery_errors_and_no_matches_are_reported_in_manifest(self):
        for case in ('forbidden', 'empty', 'invalid', 'timeout'):
            with self.subTest(case=case):
                self.out = self.work / case
                self.env['CASE'] = case
                fake = self.work / 'kubectl'
                fake.write_text('''#!/usr/bin/env python3
import os, sys, time
case = os.environ['CASE']
if case == 'forbidden':
    print('Forbidden: cannot list pods', file=sys.stderr)
    sys.exit(1)
if case == 'empty': print('{"items": []}')
if case == 'invalid': print('not json')
if case == 'timeout': time.sleep(2)
''')
                result = self.run_logs('--timeout', '0.1')
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertNotIn('Traceback', result.stderr)
                manifest = json.loads((self.out / 'manifest.json').read_text())
                self.assertFalse(manifest['complete'])
                self.assertTrue(manifest['error'])
                self.assertEqual(manifest['results'], [])
                self.assertIn('db3:', result.stderr)

    def test_log_failure_preserves_partial_output_and_continues(self):
        fake = self.work / 'kubectl'
        fake.write_text(fake.read_text().replace("print('pod log line')", "print('pod log line'); sys.exit(7 if 'processor-0' in args else 0)"))
        result = self.run_logs('--selector', 'role=processor', '--tail', '-1')
        self.assertEqual(result.returncode, 1, result.stderr)
        manifest = json.loads((self.out / 'manifest.json').read_text())
        self.assertFalse(manifest['complete'])
        self.assertEqual([r['ok'] for r in manifest['results']], [False, True])
        self.assertIn('processor-0', result.stderr)
        self.assertTrue(manifest['results'][0]['error'])
        self.assertEqual((self.out / 'processor-0.log').read_text(), 'pod log line\n')
        calls = [json.loads(line) for line in self.log.read_text().splitlines()]
        self.assertIn('role=processor', calls[0])
        self.assertIn('--tail=-1', calls[1])

    def test_logs_redact_exported_token_without_sending_it_to_kubectl(self):
        self.env['INFLUXDB3_TOKEN'] = 'apiv3_log-secret'
        fake = self.work / 'kubectl'
        fake.write_text(fake.read_text().replace("print('pod log line')", "print(os.environ['INFLUXDB3_TOKEN']); print(os.environ['INFLUXDB3_TOKEN'], file=sys.stderr)"))
        result = self.run_logs()
        self.assertEqual(result.returncode, 0, result.stderr)
        for file in self.out.iterdir():
            self.assertNotIn('apiv3_log-secret', file.read_text())
        self.assertNotIn('apiv3_log-secret', self.log.read_text())

    def test_pod_log_timeout_continues_to_next_pod(self):
        fake = self.work / 'kubectl'
        fake.write_text(fake.read_text().replace("print('pod log line')", "import time\n    if 'processor-0' in args: time.sleep(2)\n    print('pod log line')"))
        result = self.run_logs('--timeout', '0.1')
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertNotIn('Traceback', result.stderr)
        manifest = json.loads((self.out / 'manifest.json').read_text())
        self.assertEqual([r['ok'] for r in manifest['results']], [False, True])
        self.assertIn('timed out', manifest['results'][0]['error'])

    def test_existing_output_is_not_overwritten(self):
        self.out.mkdir()
        (self.out / 'keep').write_text('original')
        result = self.run_logs()
        self.assertEqual(result.returncode, 1)
        self.assertEqual((self.out / 'keep').read_text(), 'original')
        self.assertFalse(self.log.exists())

    def test_shared_options_before_logs_are_preserved(self):
        result = subprocess.run([os.environ.get('DB3_BINARY', str(ROOT / 'kubectl-db3')), 'support', '-n', 'chosen', '--context', 'chosen-context', '--output', str(self.out), 'logs', 'processor'], env=self.env, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        manifest = json.loads((self.out / 'manifest.json').read_text())
        self.assertEqual(manifest['namespace'], 'chosen')
        self.assertEqual(manifest['context'], 'chosen-context')

    def test_all_roles_default_and_explicit_all_collect_same_pods(self):
        roles = ['querier', 'processor', 'ingester', 'compactor']
        fake = self.work / 'kubectl'
        fake.write_text(fake.read_text().replace("['processor-0', 'processor-1']", repr([role + '-0' for role in roles])))
        for role_arg in ([], ['all']):
            with self.subTest(role_arg=role_arg):
                self.out = self.work / ('explicit' if role_arg else 'default')
                result = subprocess.run([os.environ.get('DB3_BINARY', str(ROOT / 'kubectl-db3')), 'support', 'logs', *role_arg, '-n', 'influxdb3', '--output', str(self.out)], env=self.env, capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)
                manifest = json.loads((self.out / 'manifest.json').read_text())
                self.assertTrue(manifest['complete'])
                self.assertEqual(manifest['role'], 'all')
                self.assertEqual(manifest['requested_roles'], roles)
                self.assertEqual(manifest['selector'], 'app.kubernetes.io/component in (querier,processor,ingester,compactor)')
                self.assertEqual({r['pod'] for r in manifest['results']}, {role + '-0' for role in roles})
                for role in roles:
                    self.assertEqual((self.out / f'{role}-0.log').read_text(), 'pod log line\n')
        calls = [json.loads(line) for line in self.log.read_text().splitlines()]
        self.assertEqual(len(calls), 10)
        self.assertIn('app.kubernetes.io/component in (querier,processor,ingester,compactor)', calls[0])

    def test_each_role_uses_its_own_selector(self):
        for role in ('querier', 'processor', 'ingester', 'compactor'):
            with self.subTest(role=role):
                self.out = self.work / role
                result = subprocess.run([os.environ.get('DB3_BINARY', str(ROOT / 'kubectl-db3')), 'support', 'logs', role, '-n', 'influxdb3', '--output', str(self.out)], env=self.env, capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)
                manifest = json.loads((self.out / 'manifest.json').read_text())
                self.assertEqual(manifest['requested_roles'], [role])
                self.assertEqual(manifest['selector'], f'app.kubernetes.io/component={role}')
        calls = [json.loads(line) for line in self.log.read_text().splitlines()]
        discovery = [call for call in calls if 'get' in call]
        self.assertEqual(len(discovery), 4)
        for call, role in zip(discovery, ('querier', 'processor', 'ingester', 'compactor')):
            self.assertIn(f'app.kubernetes.io/component={role}', call)

    def test_all_custom_selector_and_partial_failure(self):
        fake = self.work / 'kubectl'
        fake.write_text(fake.read_text().replace("['processor-0', 'processor-1']", "['querier-0', 'processor-0', 'ingester-0', 'compactor-0']").replace("print('pod log line')", "print('pod log line'); sys.exit(7 if 'querier-0' in args else 0)"))
        result = subprocess.run([os.environ.get('DB3_BINARY', str(ROOT / 'kubectl-db3')), 'support', 'logs', 'all', '-n', 'influxdb3', '--selector', 'app=influxdb3,role in (querier,processor,ingester,compactor)', '--output', str(self.out)], env=self.env, capture_output=True, text=True)
        self.assertEqual(result.returncode, 1, result.stderr)
        manifest = json.loads((self.out / 'manifest.json').read_text())
        self.assertFalse(manifest['complete'])
        self.assertEqual(len(manifest['results']), 4)
        self.assertEqual(sum(r['ok'] for r in manifest['results']), 3)
        calls = [json.loads(line) for line in self.log.read_text().splitlines()]
        self.assertIn('app=influxdb3,role in (querier,processor,ingester,compactor)', calls[0])
        self.assertEqual(len(calls), 5)


if __name__ == '__main__':
    unittest.main()
