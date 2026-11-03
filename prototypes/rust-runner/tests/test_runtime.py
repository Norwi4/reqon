"""Real CLI execution: typed scopes, environment references and safe artifacts."""
import json
import os
import subprocess
import tempfile
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path

import jsonschema
from fixture import WorkflowServer
from package_fixture import build_package, write_json
from test_e2e import BINARY, ROOT


class RuntimeTests(unittest.TestCase):
    TOKEN = 'SyntheticReqonSecret_A93<&"'
    PROCESS_NAME = 'REQON_TEST_RUN03_TOKEN'

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='reqon-context-')
        self.root = Path(self.temp.name)
        self.package = self.root / 'package'
        self.files = build_package(self.package)
        self.files['catalog.json']['requires']['capabilities'].append('variables')
        write_json(self.package / 'catalog.json', self.files['catalog.json'])
        self.envfile = self.root / 'environment.json'
        self.server = WorkflowServer()
        self.environment = {'schema_version': 'reqon-environment/v1', 'id': 'local', 'endpoint': self.server.endpoint}
        self.save_env()

    def tearDown(self):
        self.server.close()
        self.temp.cleanup()

    def save_env(self):
        write_json(self.envfile, self.environment)

    def update(self, path, **values):
        self.files[path].update(values)
        write_json(self.package / path, self.files[path])

    def script(self, code, case='cases/chain.json', **fields):
        self.update(case, steps=[{'type': 'javascript', 'id': 'check', 'script': code}], **fields)

    def invoke(self, *args, code=0, token=TOKEN):
        env = os.environ.copy()
        env.pop(self.PROCESS_NAME, None)
        if token is not None:
            env[self.PROCESS_NAME] = token
        result = subprocess.run([str(BINARY), *map(str, args)], capture_output=True, text=True, encoding='utf-8', env=env, timeout=15)
        self.assertEqual(result.returncode, code, result.stdout + result.stderr)
        return result

    def command(self, *args):
        return ['run', self.package, '--env', self.envfile, '--json', self.root / 'result.json', '--junit', self.root / 'junit.xml', *args]

    def run_context(self, *args, code=0, token=TOKEN):
        result = self.invoke(*self.command(*args), code=code, token=token)
        report = json.loads((self.root / 'result.json').read_text(encoding='utf-8'))
        junit = ET.parse(self.root / 'junit.xml').getroot()
        return report, junit, result

    def secret(self):
        self.environment['secrets'] = {'token': {'env': self.PROCESS_NAME}}
        self.save_env()

    def assert_private(self, result, *secrets):
        text = result.stdout + result.stderr
        for name in ('result.json', 'junit.xml'):
            path = self.root / name
            if path.exists():
                raw = path.read_text(encoding='utf-8')
                text += raw
                if name.endswith('.json'):
                    text += json.dumps(json.loads(raw), ensure_ascii=False)
                else:
                    text += ''.join(ET.fromstring(raw).itertext())
                    text += str([dict(e.attrib) for e in ET.fromstring(raw).iter()])
        for secret in (self.TOKEN, *secrets):
            self.assertNotIn(secret, text)
            self.assertNotIn(json.dumps(secret)[1:-1], text)

    def test_scope_precedence_types_and_cli_overrides(self):
        self.update('catalog.json', variables={'a': 'package', 'b': 'package', 'c': 'package', 'd': 'package'})
        self.script("assert(vars.a === 'package' && vars.b === 'case' && vars.c === 'environment' && vars.d === 'cli'); assert(vars.n === 7 && vars.flag === true && vars.nil === null); assert(vars.obj.x[1] === false); return {verified:true};", variables={'b': 'case', 'c': 'case', 'd': 'case'})
        self.environment['variables'] = {'c': 'environment', 'd': 'environment', 'n': 7, 'flag': True, 'nil': None, 'obj': {'x': [1, False]}}
        self.save_env()
        report, _, _ = self.run_context('--case', 'CASE-001', '--var', 'd="cli"')
        values = report['cases'][0]['report']['variables']
        self.assertTrue(values['verified'])
        self.assertIs(values['flag'], True)
        self.assertIsNone(values['nil'])
        self.assertEqual(report['environment_id'], 'local')

    def test_environment_and_runtime_mutations_are_isolated_per_case(self):
        self.environment['variables'] = {'counter': 1}
        self.save_env()
        code = 'assert(vars.counter === 1); assert(typeof vars.created === "undefined"); return {counter:2, created:true};'
        self.script(code)
        self.script(code, 'cases/isolation.json')
        report, _, _ = self.run_context()
        self.assertEqual([r['outcome'] for r in report['cases']], ['passed', 'passed', 'skipped'])
        self.assertEqual(report['cases'][1]['report']['variables']['counter'], 2)

    def test_public_runtime_values_override_initial_values(self):
        self.environment['variables'] = {'value': 1}
        self.save_env()
        self.update('cases/chain.json', steps=[{'type': 'javascript', 'id': 'first', 'script': 'return {value:2};'}, {'type': 'javascript', 'id': 'second', 'script': 'assert(vars.value === 2); return {};'}])
        report, _, _ = self.run_context('--case', 'CASE-001', '--var', 'value=5')
        self.assertEqual(report['cases'][0]['report']['variables']['value'], 2)

    def test_nested_json_keys_are_data_not_javascript_prototype_setters(self):
        obj = {'__proto__': {'value': 'ordinary-data'}, 'text': 'line\n"quote"'}
        self.environment['variables'] = {'obj': obj}
        self.save_env()
        self.script('assert(Object.prototype.hasOwnProperty.call(vars.obj, "__proto__")); return {copied:vars.obj};')
        report, _, _ = self.run_context('--case', 'CASE-001')
        self.assertEqual(report['cases'][0]['report']['variables']['copied'], obj)

    def test_endpoint_override_wins_over_environment(self):
        self.environment['endpoint'] = 'http://127.0.0.1:1'
        self.save_env()
        self.run_context('--endpoint', self.server.endpoint)
        self.assertTrue(self.server.calls)

    def test_offline_environment_validation_never_reads_secrets(self):
        self.secret()
        for path in (self.envfile, self.package):
            flags = [] if path == self.envfile else ['--env', self.envfile]
            result = self.invoke('validate', path, '--json', *flags, token=None)
            self.assertTrue(json.loads(result.stdout)['valid'])
        self.assertEqual(self.server.calls, [])
        self.assertFalse((self.root / 'result.json').exists())

    def test_metadata_uses_real_secret_but_reports_withhold_it(self):
        self.secret()
        self.environment['metadata'] = {'authorization': '${token}', 'x-public': '${label}'}
        self.environment['variables'] = {'label': 'example'}
        self.save_env()
        self.update('cases/chain.json', steps=[{'type': 'grpc', 'id': 'echo', 'method': 'poc.Workflow/Echo', 'request': {'payload': '${token}'}, 'expect': [{'path': '/payload', 'equals': '${token}'}], 'save': {'copied': '/payload'}}])
        report, _, result = self.run_context('--case', 'CASE-001')
        self.assertEqual(self.server.calls[-1][2]['authorization'], self.TOKEN)
        self.assertEqual(self.server.calls[-1][2]['x-public'], 'example')
        run = report['cases'][0]['report']
        self.assertEqual(run['variables'], {})
        self.assertIsNone(run['steps'][0]['response'])
        self.assertEqual(run['steps'][0]['checks'][0]['expected'], '[REDACTED]')
        self.assertTrue(run['steps'][0]['checks'][0]['passed'])
        self.assertEqual(report['redaction']['mode'], 'secret-context/v1')
        self.assert_private(result)

    def test_transformed_js_values_and_failure_messages_are_withheld(self):
        self.secret()
        encoded = ':'.join(self.TOKEN)
        self.script('return {derived: Array.from(vars.token).join(":")};')
        report, _, result = self.run_context('--case', 'CASE-001')
        self.assertEqual(report['cases'][0]['report']['variables'], {})
        self.assert_private(result, encoded)

        self.script('assert(false, Array.from(vars.token).join(":")); return {};')
        report, junit, result = self.run_context('--case', 'CASE-001', code=1)
        self.assertEqual(junit.find('testsuite').attrib['failures'], '1')
        self.assert_private(result, encoded)

    def test_script_test_names_logs_html_and_journal_withhold_transformed_secrets(self):
        self.secret()
        self.script("const derived=Array.from(vars.token).reverse().join('');reqon.log(derived);reqon.test(derived,()=>reqon.assert(true));return {};")
        report,_,result=self.run_context('--case','CASE-001','--html',self.root/'report.html')
        self.assert_private(result,self.TOKEN[::-1])
        step=report['cases'][0]['report']['steps'][0]
        self.assertNotIn('logs',step)
        self.assertEqual(step['script_checks'][0]['name'],'[REDACTED]')
        for path in [self.root/'report.html',*self.root.glob('*.journal.ndjson')]:
            text=path.read_text(encoding='utf8')
            for secret in [self.TOKEN,self.TOKEN[::-1]]:
                self.assertNotIn(secret,text)
                self.assertNotIn(json.dumps(secret)[1:-1],text)
    def test_transformed_grpc_error_is_withheld(self):
        self.secret()
        self.update('cases/chain.json', steps=[{'type': 'grpc', 'id': 'error', 'method': 'poc.Workflow/EchoError', 'request': {'payload': '${token}'}}])
        report, junit, result = self.run_context('--case', 'CASE-001', code=2)
        self.assertEqual(report['cases'][0]['report']['steps'][0]['grpc_code'], 'Unavailable')
        self.assertEqual(junit.find('testsuite').attrib['errors'], '1')
        self.assert_private(result, self.TOKEN[::-1])

    def test_secret_bindings_cannot_be_overwritten_by_js_or_save(self):
        self.secret()
        variants = [
            [{'type': 'javascript', 'id': 'overwrite', 'script': 'return {token:"public", newValue:1};'}],
            [{'type': 'grpc', 'id': 'overwrite', 'method': 'poc.Workflow/Echo', 'request': {'payload': 'public'}, 'save': {'token': '/payload'}}],
        ]
        for steps in variants:
            self.update('cases/chain.json', steps=steps)
            report, _, result = self.run_context('--case', 'CASE-001', code=2)
            self.assertEqual(report['cases'][0]['report']['variables'], {})
            self.assert_private(result)

    def test_secret_names_cannot_collide_with_public_scopes(self):
        self.secret()
        for path in ('catalog.json', 'cases/chain.json'):
            with self.subTest(path=path):
                self.update(path, variables={'token': 'public'})
                result = self.invoke(*self.command(), code=2)
                self.assertEqual(result.stdout, '')
                self.assertEqual(self.server.calls, [])
                self.assert_private(result)
                self.update(path, variables={})
        self.invoke(*self.command('--var', 'token="public"'), code=2)

    def test_missing_empty_and_non_ascii_metadata_secret_fail_before_network(self):
        self.secret()
        for token in (None, ''):
            result = self.invoke(*self.command(), code=2, token=token)
            self.assertEqual(result.stdout, '')
        self.environment['metadata'] = {'authorization': '${token}'}
        self.save_env()
        result = self.invoke(*self.command(), code=2, token='synthetic\nsecret')
        self.assertNotIn('synthetic', result.stderr)
        self.assertEqual(self.server.calls, [])
        self.assertFalse((self.root / 'result.json').exists())

    def test_cli_secret_reference_overrides_environment_reference(self):
        self.secret()
        self.environment['secrets']['token']['env'] = 'REQON_TEST_DOES_NOT_EXIST'
        self.save_env()
        self.script('assert(vars.token.length > 0); return {};')
        _, _, result = self.run_context('--case', 'CASE-001', '--secret', f'token={self.PROCESS_NAME}')
        self.assert_private(result)

    def test_secret_ref_and_public_override_work_without_environment_file(self):
        self.script('assert(vars.count === 3 && vars.token.length > 0); return {};')
        result = self.invoke('run', self.package, '--endpoint', self.server.endpoint, '--case', 'CASE-001', '--var', 'count=3', '--secret', f'token={self.PROCESS_NAME}', '--json', self.root/'result.json', '--junit', self.root/'junit.xml')
        self.assert_private(result)

    def test_authored_names_and_annotations_are_masked_in_secret_mode(self):
        self.secret()
        self.script('return {};', name='Name ' + self.TOKEN, annotations={self.TOKEN: {'value': self.TOKEN}})
        self.files['cases/chain.json']['steps'][0]['id'] = self.TOKEN
        write_json(self.package/'cases/chain.json', self.files['cases/chain.json'])
        _, _, result = self.run_context('--case', 'CASE-001')
        self.assert_private(result)

    def test_input_error_does_not_echo_loaded_secret(self):
        self.secret()
        self.update('cases/chain.json', timeout_ms=self.TOKEN)
        result = self.invoke(*self.command(), code=2)
        self.assertEqual(result.stdout, '')
        self.assert_private(result)
        self.assertFalse((self.root/'result.json').exists())

    def test_environment_cannot_be_overwritten_by_report(self):
        original = self.envfile.read_bytes()
        result = self.invoke('run', self.package, '--env', self.envfile, '--json', self.envfile, '--junit', self.root/'junit.xml', code=2)
        self.assertEqual(self.envfile.read_bytes(), original)
        self.assertEqual(result.stdout, '')
        self.assertEqual(self.server.calls, [])

    def test_invalid_environment_and_overrides_have_value_free_errors(self):
        for mutation in ({'variables': {'bad-name': 1}}, {'schema_version': 'future/v9'}, {'secrets': {'token': {'value': self.TOKEN}}}, {'variables': {str(i): i for i in range(257)}}):
            write_json(self.envfile, {**self.environment, **mutation})
            result = self.invoke('validate', self.envfile, '--json', code=2)
            self.assert_private(result)
        self.save_env()
        for flags in (['--var', 'x=oops'], ['--var','x=1','--var','x=2'], ['--var','__proto__={}'], ['--secret','token=BAD-NAME'], ['--secret','token=X','--secret','token=Y']):
            self.invoke('validate', self.package, '--json', '--env', self.envfile, *flags, code=2)
        self.assertEqual(self.server.calls, [])

    def test_duplicate_environment_keys_and_oversized_context_rejected(self):
        self.envfile.write_text('{"schema_version":"reqon-environment/v1","id":"a","id":"b"}', encoding='utf-8')
        self.invoke('validate', self.envfile, '--json', code=2)
        self.environment['variables'] = {'huge': 'x' * (1024*1024)}
        self.save_env()
        self.invoke('validate', self.envfile, '--json', code=2)

    def test_metadata_reference_and_type_checks_are_offline(self):
        for variables, template in [({}, '${missing}'), ({'value':7}, '${value}')]:
            self.environment.update(variables=variables, metadata={'x-test': template})
            self.save_env()
            self.invoke('validate', self.package, '--json', '--env', self.envfile, code=2)
        self.assertEqual(self.server.calls, [])

    def test_metadata_partial_interpolation_and_bad_keys_rejected(self):
        for metadata in ({'authorization':'Bearer ${token}'}, {'wrong-bin':'value'}, {'x-value':'bad\nvalue'}):
            self.environment['metadata'] = metadata
            self.save_env()
            self.invoke('validate', self.envfile, '--json', code=2)

    def test_unicode_multiline_secret_never_enters_public_artifacts(self):
        secret = 'Synthetic-\u041f\u0430\u0440\u043e\u043b\u044c-\U0001f600\n"<&'
        self.secret()
        self.script('assert(false, vars.token); return {};')
        _, _, result = self.run_context('--case','CASE-001',code=1,token=secret)
        self.assert_private(result, secret)

    def test_defaults_require_capability_and_safe_variable_names(self):
        self.files['catalog.json']['requires']['capabilities'].remove('variables')
        self.update('catalog.json', variables={'foo':1})
        self.invoke('validate', self.package, '--json', code=2)
        self.files['catalog.json']['requires']['capabilities'].append('variables')
        self.update('catalog.json', variables={'__proto__':{}})
        self.invoke('plan', self.package, code=2)

    def test_unselected_case_cannot_hide_invalid_metadata_context(self):
        self.environment['metadata'] = {'x-label':'${label}'}
        self.save_env()
        self.update('cases/chain.json', variables={'label':'present'})
        self.invoke(*self.command('--case','CASE-001'), code=2)
        self.assertEqual(self.server.calls, [])

    def test_secret_timeout_keeps_partial_reports_without_values(self):
        self.secret()
        self.update('catalog.json', timeout_ms=120)
        self.update('cases/chain.json', steps=[{'type':'grpc','id':'slow','method':'poc.Workflow/Slow'}])
        report, _, result = self.run_context(code=2)
        self.assertEqual(report['cases'][1]['outcome'], 'skipped')
        self.assertTrue(self.server.slow_cancelled.wait(2))
        self.assert_private(result)

    def test_secret_cancel_keeps_partial_reports_without_values(self):
        self.secret()
        self.update('cases/chain.json', steps=[{'type':'grpc','id':'slow','method':'poc.Workflow/Slow'}])
        env = os.environ.copy()
        env[self.PROCESS_NAME] = self.TOKEN
        process = subprocess.Popen([str(BINARY), *map(str,self.command('--control-stdin'))], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding='utf-8', env=env)
        try:
            self.assertTrue(self.server.slow_started.wait(5))
            stdout, stderr = process.communicate('{"type":"cancel"}\n',timeout=5)
            self.assertEqual(process.returncode,130,stdout+stderr)
            self.assert_private(subprocess.CompletedProcess([],130,stdout,stderr))
            self.assertEqual(json.loads((self.root/'result.json').read_text())['outcome'],'cancelled')
        finally:
            if process.poll() is None:
                process.kill()
                process.communicate()

    def test_no_secret_state_survives_another_run(self):
        self.secret()
        self.script('assert(vars.token.length > 0); return {};')
        self.run_context('--case','CASE-001')
        self.environment.pop('secrets')
        self.environment['variables'] = {'publicValue':self.TOKEN}
        self.save_env()
        self.script('assert(typeof vars.token === "undefined"); return {};')
        report, _, _ = self.run_context('--case','CASE-001',token=None)
        self.assertNotIn('redaction',report)
        self.assertEqual(report['cases'][0]['report']['variables']['publicValue'],self.TOKEN)

    def test_environment_schema_matches_export_and_example(self):
        result=self.invoke('schema','--kind','environment')
        schema=json.loads(result.stdout)
        jsonschema.Draft202012Validator.check_schema(schema)
        jsonschema.validate(self.environment,schema)
        self.assertEqual(schema,json.loads((ROOT/'schemas/reqon-environment-v1.schema.json').read_text(encoding='utf-8')))


if __name__ == '__main__':
    unittest.main()
