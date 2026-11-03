"""RUN-04/05/06 acceptance against independent HTTP/gRPC/PostgreSQL servers."""
import base64
import json
import subprocess
import tempfile
import time
import unittest
from pathlib import Path

import grpc
from google.protobuf import descriptor_pb2
from fixture import WorkflowServer
from package_fixture import write_json
from service_fixture import HttpFixture, ProxyFixture, PostgresStore, certificates
from test_e2e import BINARY


def http_step(url='/echo', **kwargs):
    request = {'url': url}
    request.update(kwargs.pop('request', {}))
    return {'type': 'http', 'id': kwargs.pop('id', 'http'), 'request': request, **kwargs}


def grpc_step(method='Echo', **kwargs):
    return {'type': 'grpc', 'id': kwargs.pop('id', 'grpc'), 'method': 'poc.Workflow/' + method, 'request': kwargs.pop('request', {}), **kwargs}


class TransportTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='reqon-transport-')
        self.root = Path(self.temp.name)
        self.services = []
        self.http = self.service(HttpFixture())

    def service(self, service):
        self.services.append(service)
        return service

    def tearDown(self):
        for service in reversed(self.services):
            service.close()
        self.temp.cleanup()

    def run_case(self, steps, endpoint=None, transport=None, code=0, variables=None, extra_cases=None, cli=(), **case_fields):
        catalog = {'schema_version': 'reqon-package/v1', 'id': 'transport-tests', 'name': 'Transport tests', 'requires': {'engine': 'reqon', 'version': '>=0.14.0, <0.15.0', 'capabilities': ['grpc.unary', 'http', 'pause', 'variables', 'poll', 'javascript', 'hooks']}, 'cases': ['case.json'], 'variables': variables or {}}
        case = {'schema_version': 'reqon-case/v1', 'id': 'CASE-HTTP', 'name': 'Transport case', 'steps': steps, **case_fields}
        for i, extra in enumerate(extra_cases or []):
            path = f'extra{i}.json'
            catalog['cases'].append(path)
            write_json(self.root / path, {'schema_version': 'reqon-case/v1', 'id': f'EXTRA-{i}', 'name': 'Extra', 'steps': extra})
        write_json(self.root / 'catalog.json', catalog)
        write_json(self.root / 'case.json', case)
        write_json(self.root / 'env.json', {'schema_version': 'reqon-environment/v1', 'id': 'fixture', 'endpoint': endpoint or self.http.endpoint, 'transport': transport or {}})
        command = [str(BINARY), 'run', str(self.root), '--env', str(self.root / 'env.json'), '--json', str(self.root / 'result.json'), '--junit', str(self.root / 'junit.xml')]
        result = subprocess.run([*command,*map(str,cli)], capture_output=True, text=True, encoding='utf8', timeout=30)
        self.completed=result
        self.assertEqual(result.returncode, code, result.stdout + result.stderr)
        report = json.loads((self.root / 'result.json').read_text(encoding='utf8'))
        self.report = report
        return report['cases'][0]['report']

    def test_methods_headers_query_json_and_connection_pool(self):
        steps = [http_step(id=method, request={'method': method, 'query': [{'name': 'q', 'value': 'a &б/?'}, {'name': 'q', 'value': 7}], 'headers': {'X-Custom': '${value}'}, 'body': {'type': 'json', 'value': {'n': 7, 'nil': None}}}, expect=[{'path': '/method', 'equals': method}, {'path': '/query/q', 'equals': ['a &б/?', '7']}, {'path': '/headers/x-custom', 'equals': 'unicode-42'}, {'path': '/body/n', 'equals': 7}, {'target': 'headers', 'path': '/x-multi', 'equals': ['one', 'two']}]) for method in ('GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'OPTIONS')]
        report = self.run_case(steps, variables={'value': 'unicode-42'})
        self.assertEqual(len(report['steps']), 6)
        self.assertEqual(len(self.http.ports), 1)

    def test_text_form_binary_multipart_and_assets(self):
        (self.root / 'payload.bin').write_bytes(bytes([0, 255, 128, 10]))
        steps = [
            http_step(id='text', request={'method': 'POST', 'body': {'type': 'text', 'value': 'Привет ${who}'}}, expect=[{'path': '/body', 'equals': 'Привет мир'}]),
            http_step(id='form', request={'method': 'POST', 'body': {'type': 'form', 'fields': [{'name': 'a', 'value': 'b &c'}, {'name': 'a', 'value': 7}]}}, expect=[{'path': '/body', 'equals': 'a=b+%26c&a=7'}]),
            http_step(id='binary', request={'method': 'POST', 'body': {'type': 'binary', 'path': 'payload.bin'}}, expect=[{'path': '/binary', 'equals': 'AP+ACg=='}]),
            http_step(id='multipart', request={'method': 'POST', 'body': {'type': 'multipart', 'parts': [{'type': 'text', 'name': 'hello', 'value': 'world'}, {'type': 'file', 'name': 'file', 'path': 'payload.bin', 'filename': 'data.bin'}]}}, expect=[{'path': '/body', 'contains': 'name="hello"'}, {'path': '/body', 'contains': 'filename="data.bin"'}]),
            http_step('/binary', id='binary-response', expect=[{'path': '/encoding', 'equals': 'base64'}, {'path': '/data', 'equals': 'AP+ACg=='}]),
        ]
        self.run_case(steps, variables={'who': 'мир'})

    def test_auth_cookie_isolation_and_reused_pool_across_cases(self):
        steps = [http_step('/cookies/set', id='cookie'), http_step(id='auth', request={'auth': {'type': 'basic', 'username': 'user', 'password': 'p:ass'}}, expect=[{'path': '/headers/authorization', 'equals': 'Basic ' + base64.b64encode(b'user:p:ass').decode()}, {'path': '/headers/cookie', 'contains': 'session=fixture'}])]
        self.run_case(steps, extra_cases=[[http_step(expect=[{'path': '/headers/cookie', 'exists': False}])]])
        self.assertEqual(len(self.http.ports), 1)

    def test_redirect_semantics_and_credential_boundaries(self):
        other = self.service(HttpFixture())
        steps = [http_step('/redirect?to=/echo&status=303', id='redirect', request={'method': 'POST', 'body': {'type': 'text', 'value': 'payload'}}, expect=[{'path': '/method', 'equals': 'GET'}, {'path': '/body', 'equals': None}]), http_step('/redirect?to=' + other.endpoint + '/echo', id='cross-origin', request={'headers': {'Authorization': 'secret', 'X-Private': 'private'}, 'redirects': {'max': 3, 'allow_cross_origin': True}}, expect=[{'path': '/headers/authorization', 'exists': False}, {'path': '/headers/x-private', 'exists': False}])]
        self.run_case(steps)
        report = self.run_case([http_step('/redirect?to=' + other.endpoint + '/echo')], code=2)
        self.assertIn('cross-origin', report['steps'][0]['error'])

    def test_compression_and_streaming_response_limit(self):
        self.run_case([http_step('/gzip?size=4096', expect=[{'target': 'status', 'equals': 200}])])
        report = self.run_case([http_step('/gzip?size=100000')], max_response_bytes=4096, code=2)
        self.assertIn('limit', report['steps'][0]['error'])

    def test_http_cancellation_reaches_server(self):
        path=self.root/'scenario.json'
        write_json(path,{'schema_version':'reqon-prototype/v1','id':'cancel-http','endpoint':self.http.endpoint,'timeout_ms':10000,'steps':[http_step('/slow?ms=5000',timeout_ms=9000)]})
        process=subprocess.Popen([str(BINARY),'run',str(path),'--json',str(self.root/'result.json'),'--junit',str(self.root/'junit.xml'),'--control-stdin'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        try:
            self.assertTrue(self.http.slow_started.wait(5))
            started=time.perf_counter()
            stdout,stderr=process.communicate(b'{"type":"cancel"}\n',timeout=5)
            self.assertEqual(process.returncode,130,stdout+stderr)
            self.assertTrue(self.http.slow_cancelled.wait(2),'server did not observe connection cancellation')
            self.assertLess(time.perf_counter()-started,2)
        finally:
            if process.poll() is None:process.kill();process.communicate()

    def test_proxy_authentication_failure_does_not_reach_origin(self):
        proxy=self.service(ProxyFixture(authorization='Bearer expected'))
        self.run_case([http_step(expect=[{'target':'status','equals':200}])],transport={'proxy':proxy.endpoint,'proxy_authorization':'Bearer wrong'},code=1)
        self.assertEqual(len(self.http.calls),0)

    def test_status_missing_null_containment_schema_and_timing(self):
        self.run_case([http_step(expect=[{'target': 'status', 'equals': 200}, {'path': '/nullValue', 'equals': None}, {'path': '/nullValue', 'exists': True}, {'path': '/missing', 'exists': False}, {'path': '/query', 'contains': {}}, {'path': '', 'schema': {'type': 'object', 'required': ['method'], 'properties': {'method': {'enum': ['GET']}}}}, {'target': 'elapsed_ms', 'less_than': 1000}])])
        report = self.run_case([http_step(expect=[{'path': '/missing', 'equals': None}])], code=1)
        self.assertIsNone(report['steps'][0]['checks'][0]['actual'])
        self.assertFalse(report['steps'][0]['checks'][0]['passed'])

    def test_https_ca_and_mtls(self):
        certs = certificates(self.root)
        https = self.service(HttpFixture(tls_dir=self.root))
        self.run_case([http_step()], endpoint=https.endpoint, transport={'ca_cert': 'ca.pem'})
        self.run_case([http_step()], endpoint=https.endpoint, code=2)
        mtls = self.service(HttpFixture(tls_dir=self.root, mtls=True))
        self.run_case([http_step()], endpoint=mtls.endpoint, transport={'ca_cert': 'ca.pem'}, code=2)
        result = self.run_case([http_step()], endpoint=mtls.endpoint, transport={'ca_cert': 'ca.pem', 'client_cert': 'client_cert.pem', 'client_key': 'client_key.pem'})
        self.assertEqual(result['outcome'], 'passed')
        self.assertNotIn(certs['client_key'].decode(), (self.root / 'result.json').read_text())

    def test_explicit_http_proxy_and_connect_tls(self):
        proxy = self.service(ProxyFixture(authorization='Basic synthetic'))
        self.run_case([http_step()], transport={'proxy': proxy.endpoint, 'proxy_authorization': 'Basic synthetic'})
        self.assertIsNone(self.http.calls[-1][2].get('proxy-authorization'))
        certificates(self.root)
        https = self.service(HttpFixture(tls_dir=self.root))
        self.run_case([http_step()], endpoint=https.endpoint, transport={'proxy': proxy.endpoint, 'proxy_authorization': 'Basic synthetic', 'ca_cert': 'ca.pem'})
        self.assertTrue(any(c[0] == 'CONNECT' for c in proxy.calls))

    def test_grpc_tls_mtls_metadata_trailers_proxy(self):
        certs = certificates(self.root)
        credentials = grpc.ssl_server_credentials([(certs['server_key'], certs['server_cert'])], root_certificates=certs['ca'], require_client_auth=True)
        server = self.service(WorkflowServer(credentials=credentials))
        proxy = self.service(ProxyFixture(authorization='Bearer fixture'))
        transport = {'ca_cert': 'ca.pem', 'client_cert': 'client_cert.pem', 'client_key': 'client_key.pem', 'proxy': proxy.endpoint, 'proxy_authorization': 'Bearer fixture'}
        report = self.run_case([grpc_step(expect=[{'target': 'headers', 'path': '/x-fixture', 'equals': ['grpc']}, {'target': 'trailers', 'path': '/x-finished', 'equals': ['yes']}])], endpoint=server.endpoint, transport=transport)
        self.assertEqual(report['outcome'], 'passed')
        self.assertEqual(len(proxy.calls), 1)

    def test_grpc_local_descriptors_proto_defaults_and_negative_status(self):
        server = self.service(WorkflowServer(reflection_enabled=False))
        descriptors = descriptor_pb2.FileDescriptorSet()
        for name in ('poc_common.proto', 'poc_workflow.proto'):
            descriptors.file.add().ParseFromString(server.pool.FindFileByName(name).serialized_pb)
        (self.root / 'service.pb').write_bytes(descriptors.SerializeToString())
        report = self.run_case([grpc_step(expect=[{'path': '/payload', 'equals': ''}]), grpc_step('TransportError', id='negative', expect_code='Unavailable', expect=[{'target': 'grpc_code', 'equals': 'Unavailable'}])], endpoint=server.endpoint, transport={'grpc_descriptor_set': 'service.pb', 'grpc_emit_defaults': True})
        self.assertEqual(report['reflection_requests'], 0)
        (self.root / 'service.proto').write_text('syntax="proto3"; package poc; message Request { string name=1; string id=2; string payload=3; } message Response { string id=2; string status=3; string payload=4; int32 count=5; } service Workflow { rpc Echo(Request) returns(Response); }')
        self.run_case([grpc_step(request={'payload': 'local proto'}, expect=[{'path': '/payload', 'equals': 'local proto'}])], endpoint=server.endpoint, transport={'grpc_proto_files': ['service.proto'], 'grpc_include_paths': ['.']})

    def test_actual_postgres_http_grpc_workflow(self):
        store = self.service(PostgresStore())
        http = self.service(HttpFixture(store=store))
        grpc_server = self.service(WorkflowServer(store=store))
        report = self.run_case([
            http_step(http.endpoint + '/objects', id='create', request={'method': 'POST', 'body': {'type': 'json', 'value': {'name': 'integration'}}}, save={'objectId': '/id'}),
            grpc_step('Get', id='wait', request={'id': '${objectId}'}, poll={'read_only': True, 'interval_ms': 10, 'timeout_ms': 2000, 'until': [{'path': '/status', 'equals': 'CREATED'}]}),
            http_step(http.endpoint + '/objects/${objectId}', id='verify', expect=[{'path': '/status', 'equals': 'CREATED'}, {'path': '/count', 'equals': 4}]),
        ], endpoint=grpc_server.endpoint)
        self.assertEqual(report['steps'][1]['attempts'], 3)
        self.assertEqual(store.count(), 1)


if __name__ == '__main__':
    unittest.main()
