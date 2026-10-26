import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class SupportTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.work = Path(self.temp.name)
        self.bin = self.work / 'bin'
        self.bin.mkdir()
        self.log = self.work / 'calls.jsonl'
        fake = self.bin / 'kubectl'
        fake.write_text('''#!/usr/bin/env python3
import json, os, sys
args = sys.argv[1:]
with open(os.environ['CALL_LOG'], 'a') as f:
    f.write(json.dumps({'args': args, 'stdin': sys.stdin.read()}) + '\\n')
print(json.dumps({'collected': args[-1]}))
''')
        fake.chmod(0o755)
        self.env = dict(os.environ, PATH=str(self.bin) + os.pathsep + os.environ['PATH'], CALL_LOG=str(self.log))
        self.env.pop('INFLUXDB3_TOKEN', None)
        self.env.pop('INFLUXDB3_POD', None)
        self.out = self.work / 'bundle'

    def run_plugin(self, *extra):
        return subprocess.run([os.environ.get('DB3_BINARY', str(ROOT / 'kubectl-db3')), 'support', '--pod', 'db3-0', '--output', str(self.out), *extra], env=self.env, capture_output=True, text=True)

    def calls(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()]

    def test_collects_three_json_results_in_selected_pod(self):
        result = self.run_plugin('--namespace', 'database', '--context', 'test', '--container', 'influxdb3')
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = self.calls()
        self.assertEqual(len(calls), 3)
        expected = [('nodes.json', ['show', 'nodes', '--format', 'json']), ('license-info.json', ['show', 'license', 'info', '--format', 'json']), ('databases.json', ['show', 'databases', '--format', 'json'])]
        for call, (filename, command) in zip(calls, expected):
            self.assertEqual(call['args'][:10], ['--context', 'test', '--namespace', 'database', 'exec', '-i', 'db3-0', '-c', 'influxdb3', '--'])
            self.assertEqual(call['args'][-len(command):], command)
            self.assertEqual(json.loads((self.out / filename).read_text()), {'collected': 'json'})
        manifest = json.loads((self.out / 'manifest.json').read_text())
        self.assertTrue(manifest['complete'])
        self.assertEqual(len(manifest['results']), 3)
        self.assertEqual(self.out.stat().st_mode & 0o777, 0o700)
        for file in self.out.iterdir():
            self.assertEqual(file.stat().st_mode & 0o777, 0o600)

    def test_token_passed_only_on_stdin_and_redacted_from_bundle(self):
        token = 'apiv3_test-secret-$;value'
        self.env['INFLUXDB3_TOKEN'] = token
        # Simulate a server/CLI echoing a credential in a diagnostic.
        fake = self.bin / 'kubectl'
        fake.write_text(fake.read_text().replace("print(json.dumps({'collected': args[-1]}))", "print(json.dumps({'echo': os.environ['INFLUXDB3_TOKEN']})); print(os.environ['INFLUXDB3_TOKEN'], file=sys.stderr)"))
        result = self.run_plugin()
        self.assertEqual(result.returncode, 0, result.stderr)
        for call in self.calls():
            self.assertEqual(call['stdin'], token + '\n')
            self.assertNotIn(token, json.dumps(call['args']))
        self.assertNotIn(token, result.stdout + result.stderr)
        for file in self.out.iterdir():
            self.assertNotIn(token, file.read_text())

    def test_failures_preserve_partial_results_and_continue(self):
        for failure in ('exit', 'json', 'timeout', 'missing'):
            with self.subTest(failure=failure):
                self.out = self.work / failure
                self.env['FAILURE'] = failure
                fake = self.bin / 'kubectl'
                fake.write_text('''#!/usr/bin/env python3
import json, os, sys, time
if 'license' in sys.argv:
    failure = os.environ['FAILURE']
    if failure == 'exit':
        print('permission denied', file=sys.stderr)
        sys.exit(7)
    if failure == 'json':
        print('not JSON')
        sys.exit(0)
    if failure == 'timeout':
        time.sleep(2)
    if failure == 'missing':
        print('executable not found', file=sys.stderr)
        sys.exit(127)
print(json.dumps({'ok': True}))
''')
                result = self.run_plugin('--timeout', '0.1')
                self.assertEqual(result.returncode, 1, result.stderr)
                manifest = json.loads((self.out / 'manifest.json').read_text())
                self.assertFalse(manifest['complete'])
                self.assertEqual([r['ok'] for r in manifest['results']], [True, False, True])
                self.assertEqual(json.loads((self.out / 'databases.json').read_text()), {'ok': True})
                self.assertIn('license-info', result.stderr)
                self.assertTrue(manifest['results'][1]['error'])

    def test_rejects_multiline_token_before_creating_bundle(self):
        self.env['INFLUXDB3_TOKEN'] = 'apiv3_bad\nsecond-line'
        result = self.run_plugin()
        self.assertEqual(result.returncode, 2)
        self.assertIn('single line', result.stderr)
        self.assertFalse(self.out.exists())
        self.assertFalse(self.log.exists())

    def test_remote_shell_executes_exact_commands_and_token_safely(self):
        fake = self.bin / 'kubectl'
        fake.write_text('''#!/usr/bin/env python3
import os, sys
args = sys.argv[1:]
remote = args[args.index('--') + 1:]
os.execvp(remote[0], remote)
''')
        cli = self.bin / 'influxdb3'
        cli.write_text('''#!/usr/bin/env python3
import json, sys
print(json.dumps(sys.argv[1:]))
''')
        cli.chmod(0o755)
        token = 'apiv3_literal-$(touch should-not-exist);\\value'
        self.env['INFLUXDB3_TOKEN'] = token
        result = self.run_plugin()
        self.assertEqual(result.returncode, 0, result.stderr)
        # JSON escaping may change a token's representation; verify the parsed value.
        for filename, command in [('nodes.json', ['show', 'nodes']), ('license-info.json', ['show', 'license', 'info']), ('databases.json', ['show', 'databases'])]:
            parsed = json.loads((self.out / filename).read_text())
            self.assertEqual(parsed, command + ['--format', 'json', '--token', '[REDACTED]'])

    def test_existing_output_is_not_overwritten(self):
        self.out.mkdir()
        marker = self.out / 'keep'
        marker.write_text('original')
        result = self.run_plugin()
        self.assertEqual(result.returncode, 1)
        self.assertEqual(marker.read_text(), 'original')
        self.assertFalse(self.log.exists())

    def test_pod_environment_variable_allows_bare_support(self):
        self.env['INFLUXDB3_POD'] = 'db3-from-env'
        result = subprocess.run([os.environ.get('DB3_BINARY', str(ROOT / 'kubectl-db3')), 'support', '--output', str(self.out)], env=self.env, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(all('db3-from-env' in call['args'] for call in self.calls()))

    def test_missing_pod_is_actionable_and_does_not_contact_cluster(self):
        result = subprocess.run([os.environ.get('DB3_BINARY', str(ROOT / 'kubectl-db3')), 'support'], env=self.env, capture_output=True, text=True)
        self.assertEqual(result.returncode, 2)
        self.assertIn('--pod', result.stderr)
        self.assertFalse(self.log.exists())

    def test_invalid_timeout_rejected(self):
        for value in ('0', '-1', 'nan', 'inf'):
            with self.subTest(value=value):
                result = self.run_plugin('--timeout', value)
                self.assertEqual(result.returncode, 2)
                self.assertFalse(self.out.exists())
                self.assertFalse(self.log.exists())

    def test_real_kubectl_dispatches_plugin_without_cluster_access(self):
        import shutil
        kubectl = shutil.which('kubectl')
        if not kubectl:
            self.skipTest('kubectl is not installed')
        plugin_dir = self.work / 'plugin-path'
        plugin_dir.mkdir()
        (plugin_dir / 'kubectl-db3').symlink_to(os.environ.get('DB3_BINARY', str(ROOT / 'kubectl-db3')))
        self.env['PATH'] = str(plugin_dir) + os.pathsep + self.env['PATH']
        result = subprocess.run([kubectl, 'db3', 'support', '--pod', 'db3-0', '--output', str(self.out)], env=self.env, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(json.loads((self.out / 'manifest.json').read_text())['complete'])
        self.assertEqual(len(self.calls()), 3)


if __name__ == '__main__':
    unittest.main()
