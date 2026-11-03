import json
import unittest
from test_transports import TransportTests, http_step


class HookTests(unittest.TestCase):
    setUp=TransportTests.setUp
    tearDown=TransportTests.tearDown
    service=TransportTests.service
    run_case=TransportTests.run_case

    def test_hook_order_reqon_api_and_persistent_worker_with_fresh_vm(self):
        hooks={
            'pre':[{'script':"reqon.vars.set('value','pre'); globalThis.marker=123; reqon.log('pre');"}],
            'post':[{'script':"assert(globalThis.marker===undefined,'VM global leaked'); assert(response.headers['x-value']==='pre'); reqon.vars.set('value','post'); reqon.log('post');"}],
            'tests':[{'script':"reqon.test('post ran',()=>reqon.assert(vars.value==='post')); reqon.test('status',()=>reqon.assert(reqon.response.status===200)); reqon.vars.set('value','tests');"}],
        }
        report=self.run_case([http_step(request={'headers':{'x-value':'${value}'}},hooks=hooks,expect=[{'path':'/headers/x-value','equals':'pre'}]),{'type':'javascript','id':'next','script':"assert(vars.value==='tests'); return {done:true};"}])
        self.assertEqual(report['variables']['value'],'tests')
        self.assertEqual(len(report['steps'][0]['script_checks']),2)
        events=[json.loads(s) for s in self.completed.stdout.splitlines()]
        self.assertEqual(sum(e['type']=='script_worker_started' for e in events),1)

    def test_additional_requests_require_explicit_origin_and_budget(self):
        step={'type':'javascript','id':'call','script':"const r=reqon.send({url:'/echo',method:'POST',body:{type:'json',value:{number:7}}}); reqon.assert(r.body.body.number===7); return {status:r.status};"}
        self.run_case([step],code=2)
        self.assertEqual(len(self.http.calls),0)
        report=self.run_case([step],transport={'script_http_origins':[self.http.endpoint],'script_http_max_calls':1})
        self.assertEqual(report['variables']['status'],200)
        step['script']="reqon.send({url:'/echo'}); reqon.send({url:'/echo'}); return {};"
        self.run_case([step],transport={'script_http_origins':[self.http.endpoint],'script_http_max_calls':1},code=2)
        self.assertEqual(len(self.http.calls),2)

    def test_origin_denied_and_tests_fail_with_names(self):
        step={'type':'javascript','id':'call','script':"reqon.send({url:'http://127.0.0.1:1/'}); return {};"}
        self.run_case([step],transport={'script_http_origins':[self.http.endpoint],'script_http_max_calls':1},code=2)
        self.assertEqual(len(self.http.calls),0)
        report=self.run_case([http_step(hooks={'tests':[{'script':"reqon.test('expected failure',()=>assert(false,'fixture verdict'));"}]})],code=1)
        self.assertEqual(report['steps'][0]['script_checks'][0]['name'],'expected failure')
        self.assertFalse(report['steps'][0]['script_checks'][0]['passed'])

    def test_pre_failure_prevents_network_and_loop_is_bounded(self):
        self.run_case([http_step(hooks={'pre':[{'script':"assert(false,'stop')"}]})],code=1)
        self.assertEqual(len(self.http.calls),0)
        report=self.run_case([http_step(hooks={'pre':[{'script':'for(;;){}','timeout_ms':20}]})],code=2)
        self.assertEqual(len(self.http.calls),0)
        self.assertIn('timeout',report['steps'][0]['error'])

    def test_script_output_and_logs_are_bounded(self):
        self.run_case([{'type':'javascript','id':'large','script':"return {text:'a'.repeat(1100000)};"}],code=2)
        self.run_case([{'type':'javascript','id':'logs','script':"for(let i=0;i<100;i++)reqon.log('log'); return {};"}],code=2)


del TransportTests
