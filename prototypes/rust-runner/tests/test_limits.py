import json
import subprocess
import unittest
from package_fixture import write_json
from test_e2e import BINARY
from test_transports import TransportTests,http_step


class LimitTests(unittest.TestCase):
    setUp=TransportTests.setUp
    tearDown=TransportTests.tearDown
    service=TransportTests.service
    run_case=TransportTests.run_case

    def test_template_expansion_is_bounded_before_network(self):
        report=self.run_case([http_step(request={'method':'POST','body':{'type':'json','value':['${large}']*100}})],variables={'large':'x'*100000},code=2)
        self.assertIn('resolved JSON exceeds',report['steps'][0]['error'])
        self.assertEqual(self.http.calls,[])
        report=self.run_case([http_step(request={'method':'POST','body':{'type':'text','value':'${large}'*100}})],variables={'large':'x'*100000},code=2)
        self.assertIn('resolved text exceeds',report['steps'][0]['error'])
        self.assertEqual(self.http.calls,[])

    def test_form_request_limit_stops_large_expansion(self):
        report=self.run_case([http_step(request={'method':'POST','max_request_bytes':4096,'body':{'type':'form','fields':[{'name':'a','value':'${large}'}]*20}})],variables={'large':'a'*1000},code=2)
        self.assertIn('request body exceeds',report['steps'][0]['error'])
        self.assertEqual(self.http.calls,[])

    def test_proto_import_size_limit_is_checked_offline(self):
        self.run_case([http_step()])
        (self.root/'main.proto').write_text('syntax="proto3"; import "large.proto";')
        (self.root/'large.proto').write_text('//'+'a'*1100000)
        write_json(self.root/'env.json',{'schema_version':'reqon-environment/v1','id':'local','transport':{'grpc_proto_files':['main.proto']}})
        result=subprocess.run([str(BINARY),'validate',str(self.root),'--env',str(self.root/'env.json'),'--json'],capture_output=True)
        self.assertEqual(result.returncode,2)

    def test_malformed_headers_fail_offline(self):
        self.run_case([http_step()])
        before=len(self.http.calls)
        case=json.loads((self.root/'case.json').read_text())
        case['steps'].append(http_step(id='bad',request={'headers':{'X-Custom':'line\r\ninjection'}}))
        write_json(self.root/'case.json',case)
        result=subprocess.run([str(BINARY),'validate',str(self.root),'--json'],capture_output=True)
        self.assertEqual(result.returncode,2)
        self.assertEqual(len(self.http.calls),before)


del TransportTests
