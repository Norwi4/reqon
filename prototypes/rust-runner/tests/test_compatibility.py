"""Java-format contracts over real gRPC sockets, with isolated fixture data."""
import json
import subprocess
import tempfile
import unittest
from pathlib import Path
from fixture import WorkflowServer
from test_e2e import BINARY


class CompatibilityTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='reqon-java-')
        self.root = Path(self.temp.name)
        self.server = WorkflowServer(java_profile=True)
        self.environment = {'schema_version': 'reqon-environment/v1', 'id': 'local',
                            'endpoint': self.server.endpoint,
                            'regression': {'targets': {'local': self.server.endpoint}, 'call_timeout_ms': 500}}

    def tearDown(self):
        self.server.close()
        self.temp.cleanup()

    def run_steps(self, steps, code=0):
        scenario = {'id': 'JAVA-COMPAT', 'name': 'Compatibility', 'domain': 'local',
                    'bf': 'test', 'kind': 'positive', 'zephyr': 'LOCAL-1', 'tags': ['smoke'], 'steps': steps}
        source = self.root / 'case.yaml'
        source.write_text(json.dumps(scenario), encoding='utf-8')
        env = self.root / 'env.json'
        env.write_text(json.dumps(self.environment), encoding='utf-8')
        result = subprocess.run([str(BINARY), 'run', str(source), '--env', str(env),
                                 '--json', str(self.root / 'result.json'), '--junit', str(self.root / 'result.xml')],
                                capture_output=True, text=True, encoding='utf-8', timeout=10)
        self.assertEqual(result.returncode, code, result.stdout + result.stderr)
        report = json.loads((self.root / 'result.json').read_text(encoding='utf-8'))
        manifest = json.loads((self.root / 'result.manifest.json').read_text(encoding='utf-8'))
        self.assertEqual(manifest['state'], 'complete')
        self.assertEqual(manifest['run_id'], report['run_id'])
        self.assertEqual(report['plan']['cases'][0]['annotations']['vziskaniya']['zephyr'], 'LOCAL-1')
        return report['cases'][0]['report']

    def test_chain_simple_service_case_insensitive_method_and_typed_save(self):
        report = self.run_steps([
            {'service': 'Workflow', 'method': 'create', 'request': {'name': 'test'}, 'save': {'id': 'id'}},
            {'pause': '1ms'},
            {'service': 'poc.Workflow', 'method': 'Get', 'request': {'id': '${id}'},
             'poll': {'interval': '5ms', 'timeout': '1s', 'until': {'equalTo': {'status': 'CREATED'}}},
             'expect': {'notEmpty': ['id'], 'equalTo': {'count': '3'}}, 'save': {'count': 'count'}},
            {'service': 'poc.Workflow', 'method': 'Echo', 'request': {'payload': 'count=${count}'},
             'expect': {'equalTo': {'payload': 'count=3'}}},
        ])
        self.assertEqual(report['variables']['count'], 3)
        self.assertEqual(report['steps'][2]['attempts'], 3)

    def test_expected_business_error_and_transport_error(self):
        self.run_steps([
            {'service': 'poc.Workflow', 'method': 'BusinessError', 'expect': {'success': False, 'errorCode': 'INVALID_INPUT'}},
            {'service': 'poc.Workflow', 'method': 'TransportError', 'expect': {'success': False, 'errorCode': 'UNAVAILABLE'}},
        ])

    def test_explicitly_disabled_target_is_expected_configuration_error(self):
        self.environment['regression']['targets']['disabled'] = ''
        self.run_steps([{'service': 'poc.Workflow', 'method': 'Create', 'target': 'DISABLED',
                         'expect': {'success': False, 'errorCode': 'INVOKER_NOT_CONFIGURED'}}])
        self.assertEqual(self.server.calls, [])

    def test_unexpected_business_error_stops_dependent_steps(self):
        report = self.run_steps([{'service': 'poc.Workflow', 'method': 'BusinessError'}, {'pause': '1ms'}], code=1)
        self.assertEqual(report['steps'][1]['outcome'], 'skipped')

    def test_poll_continue_keeps_last_values(self):
        report = self.run_steps([
            {'service': 'poc.Workflow', 'method': 'Echo', 'poll': {'interval': '5ms', 'timeout': '15ms', 'onTimeout': 'continue'},
             'expect': {'equalTo': {'status': 'NEVER'}}, 'save': {'count': 'count'}},
            {'service': 'poc.Workflow', 'method': 'Echo', 'request': {'payload': 'saved=${count}'}, 'expect': {'equalTo': {'payload': 'saved=7'}}},
        ])
        self.assertTrue(report['steps'][0]['logs'])

    def test_poll_fail_and_missing_save(self):
        for step in [
            {'service': 'poc.Workflow', 'method': 'Echo', 'poll': {'interval': '5ms', 'timeout': '15ms'}, 'expect': {'equalTo': {'status': 'NEVER'}}},
            {'service': 'poc.Workflow', 'method': 'Echo', 'save': {'missing': 'missing'}},
        ]:
            self.run_steps([step], code=1)

    def test_kafka_prepare_fails_before_any_write(self):
        self.run_steps([{'service': 'poc.Workflow', 'method': 'Create'}, {'kafka': {'topic': 'local-fixture'}}], code=2)
        self.assertEqual(self.server.calls, [])

    def test_yaml_plan_validate_are_offline_and_reject_unknown_fields(self):
        source = self.root / 'case.yaml'
        source.write_text('id: YAML\nname: YAML\nsteps:\n  - pause: 1ms\n', encoding='utf-8')
        for command in ['validate', 'plan']:
            result = subprocess.run([str(BINARY), command, str(source)], capture_output=True, timeout=5)
            self.assertEqual(result.returncode, 0, result.stderr)
        for content in ['id: YAML\nid: DUPLICATE\nsteps: [{pause: 1ms}]',
                        'id: YAML\nsteps: [{pause: 1ms, unknown: true}]',
                        'id: YAML\nsteps: [{kafka: {topic: test, from: wrong}}]']:
            source.write_text(content, encoding='utf-8')
            result = subprocess.run([str(BINARY), 'validate', str(source), '--json'], capture_output=True, timeout=5)
            self.assertEqual(result.returncode, 2)
        self.assertEqual(self.server.calls, [])

    def test_cancellation_of_compatibility_call(self):
        source = self.root / 'cancel.yaml'
        source.write_text('id: CANCEL\nsteps:\n  - service: poc.Workflow\n    method: Slow\n', encoding='utf-8')
        proc = subprocess.Popen([str(BINARY), 'run', str(source), '--endpoint', self.server.endpoint, '--control-stdin',
                                 '--json', str(self.root / 'result.json'), '--junit', str(self.root / 'result.xml')],
                                stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding='utf-8')
        try:
            self.assertTrue(self.server.slow_started.wait(5))
            stdout, stderr = proc.communicate('{"type":"cancel"}\n', timeout=5)
            self.assertEqual(proc.returncode, 130, stdout + stderr)
        finally:
            if proc.poll() is None:
                proc.kill(); proc.wait()


if __name__ == '__main__':
    unittest.main()
