import json
import subprocess
import unittest
from package_fixture import write_json
from test_e2e import BINARY
from test_transports import TransportTests, http_step


class IterationTests(unittest.TestCase):
    setUp=TransportTests.setUp
    tearDown=TransportTests.tearDown
    service=TransportTests.service
    run_case=TransportTests.run_case

    def test_json_rows_repeats_isolation_and_stable_instance_ids(self):
        data=self.root/'data.json'
        write_json(data,[{'n':7,'flag':True},{'n':8,'flag':False}])
        script={'type':'javascript','id':'row','script':"assert(vars.leaked===undefined); assert(typeof vars.data.n==='number'); return {row:vars.data,repeat:vars.iteration,leaked:true};"}
        self.run_case([script],cli=['--data',data,'--iterations','2'])
        cases=self.report['cases']
        self.assertEqual([c['data_row'] for c in cases],[1,2,1,2])
        self.assertEqual([c['iteration'] for c in cases],[1,1,2,2])
        self.assertEqual([c['report']['variables']['row']['n'] for c in cases],[7,8,7,8])
        self.assertEqual(len(set(c['instance_id'] for c in cases)),4)
        command=[str(BINARY),'plan',str(self.root),'--data',str(data),'--iterations','2']
        first=subprocess.check_output(command)
        second=subprocess.check_output(command)
        self.assertEqual(first,second)
        self.assertEqual(json.loads(first)['plan_sha256'],self.report['plan']['plan_sha256'])

    def test_csv_unicode_quoted_fields_remain_strings(self):
        data=self.root/'rows.csv'
        data.write_text('n,text\r\n7,"Привет, мир"\r\n',encoding='utf8')
        report=self.run_case([{'type':'javascript','id':'csv','script':"assert(vars.data.n==='7'); assert(vars.data.text==='Привет, мир'); return {};"}],cli=['--data',data])
        self.assertEqual(report['outcome'],'passed')

    def test_tag_selection_and_empty_selection(self):
        self.run_case([http_step()],tags=['smoke','http'],extra_cases=[[http_step()]],cli=['--tag','smoke','--exclude-tag','slow'])
        self.assertEqual(len(self.report['cases']),1)
        result=subprocess.run([str(BINARY),'plan',str(self.root),'--tag','missing'],capture_output=True)
        self.assertEqual(result.returncode,2)
        self.assertIn(b'no enabled cases',result.stderr)

    def test_bail_and_default_continue_respect_dependencies(self):
        failed={'type':'javascript','id':'fail','script':"assert(false); return {};"}
        self.run_case([failed,http_step()],extra_cases=[[http_step()]],code=1)
        self.assertEqual(len(self.http.calls),1)
        self.assertEqual(self.report['cases'][0]['report']['steps'][1]['outcome'],'skipped')
        self.run_case([failed,http_step()],extra_cases=[[http_step()]],cli=['--bail'],code=1)
        self.assertEqual(len(self.http.calls),1)
        self.assertEqual(self.report['cases'][1]['skip_reason'],'bail after case failure')

    def test_pause_delay_and_dataset_validation(self):
        report=self.run_case([{'type':'pause','id':'pause','duration_ms':20},http_step()],cli=['--iterations','2','--delay-ms','20'])
        self.assertGreaterEqual(report['steps'][0]['duration_ms'],15)
        self.assertGreaterEqual(self.report['duration_ms'],55)
        for name,content in [('empty.json','[]'),('duplicate.json','[{"a":1,"a":2}]'),('bad.csv','a,a\n1,2\n')]:
            path=self.root/name
            path.write_text(content,encoding='utf8')
            result=subprocess.run([str(BINARY),'plan',str(self.root),'--data',str(path)],capture_output=True)
            self.assertEqual(result.returncode,2)


del TransportTests
