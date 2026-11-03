import json
import os
import signal
import subprocess
import time
import unittest
import xml.etree.ElementTree as ET
import psutil
from package_fixture import write_json
from test_e2e import BINARY
from test_transports import TransportTests,http_step


class ArtifactTests(unittest.TestCase):
    setUp=TransportTests.setUp
    tearDown=TransportTests.tearDown
    service=TransportTests.service
    run_case=TransportTests.run_case

    def test_streamed_package_artifacts_manifest_and_capture(self):
        self.run_case([http_step('/large?size=100000',expect=[{'target':'status','equals':200}])],cli=['--iterations','5','--html',self.root/'report.html'])
        self.assertEqual(len(self.report['cases']),5)
        self.assertEqual(self.report['cases'][0]['report']['steps'][0]['response']['$reqon_capture'],'omitted')
        self.assertEqual(self.report['cases'][0]['report']['steps'][0]['check_summary']['passed'],1)
        manifest=json.loads((self.root/'result.manifest.json').read_text())
        self.assertEqual(manifest['state'],'complete')
        self.assertEqual(manifest['run_id'],self.report['run_id'])
        records=[json.loads(s) for s in Path(manifest['journal']).read_text(encoding='utf8').splitlines()]
        self.assertEqual(sum(r['type']=='case' for r in records),5)
        self.assertEqual(sum(r['type']=='step' for r in records),5)
        self.assertEqual(len(ET.parse(self.root/'junit.xml').findall('.//testcase')),5)

    def test_html_escapes_script_output(self):
        self.run_case([{'type':'javascript','id':'html','script':"return {payload:'<script>alert(1)</script>'};"}],cli=['--html',self.root/'report.html'])
        html=(self.root/'report.html').read_text(encoding='utf8')
        self.assertNotIn('<script>',html)
        self.assertIn('&lt;script&gt;',html)

    def prepare_running(self):
        path=self.root/'scenario.json'
        write_json(path,{'schema_version':'reqon-prototype/v1','id':'crash-test','endpoint':self.http.endpoint,'timeout_ms':60000,'steps':[http_step(),{'type':'pause','id':'waiting','duration_ms':30000}]})
        return [str(BINARY),'run',str(path),'--json',str(self.root/'result.json'),'--junit',str(self.root/'junit.xml'),'--journal',str(self.root/'journal.ndjson'),'--control-stdin']

    def test_killed_run_recovers_partial_records_as_error(self):
        process=subprocess.Popen(self.prepare_running(),stdout=subprocess.PIPE,stderr=subprocess.PIPE,stdin=subprocess.PIPE,text=True,encoding='utf8')
        try:
            while True:
                event=json.loads(process.stdout.readline())
                if event['type']=='step_started' and event['step_id']=='waiting':break
            process.kill();process.communicate(timeout=10)
            manifest=json.loads((self.root/'result.manifest.json').read_text())
            self.assertEqual(manifest['state'],'running')
            with (self.root/'journal.ndjson').open('ab') as file:file.write(b'{"partial":')
            result=subprocess.run([str(BINARY),'recover',str(self.root/'journal.ndjson'),'--json',str(self.root/'recovered.json'),'--junit',str(self.root/'recovered.xml')],capture_output=True)
            self.assertEqual(result.returncode,2,result.stderr)
            report=json.loads((self.root/'recovered.json').read_text())
            self.assertEqual(report['outcome'],'error')
            self.assertEqual(report['completed_steps'],1)
            self.assertIsNotNone(ET.parse(self.root/'recovered.xml').find('.//error'))
        finally:
            if process.poll() is None:process.kill();process.communicate()

    @unittest.skipIf(os.name=='nt','Unix signal acceptance')
    def test_sigterm_writes_cancelled_reports(self):
        process=subprocess.Popen(self.prepare_running(),stdout=subprocess.PIPE,stderr=subprocess.PIPE,stdin=subprocess.PIPE,text=True)
        try:
            while json.loads(process.stdout.readline())['type']!='step_started':pass
            process.send_signal(signal.SIGTERM)
            process.communicate(timeout=10)
            self.assertEqual(process.returncode,130)
            self.assertEqual(json.loads((self.root/'result.json').read_text())['outcome'],'cancelled')
        finally:
            if process.poll() is None:process.kill();process.communicate()

    def test_blocked_stdout_does_not_block_cancellation(self):
        command=self.prepare_running()
        path=self.root/'scenario.json'
        value=json.loads(path.read_text())
        value['steps']=[{'type':'pause','id':f'pause-{i}','duration_ms':1} for i in range(999)]+[{'type':'pause','id':'keep-alive','duration_ms':30000}]
        write_json(path,value)
        process=subprocess.Popen(command,stdout=subprocess.PIPE,stderr=subprocess.PIPE,stdin=subprocess.PIPE)
        try:
            # Observe enough completed steps to fill both the OS pipe and the
            # event queue. A fixed sleep races slower hosts/software emulation.
            deadline=time.monotonic()+15
            journal=self.root/'journal.ndjson'
            while not journal.exists() or journal.read_bytes().count(b'\n')<501:
                self.assertIsNone(process.poll(),'run exited before stdout saturation')
                self.assertLess(time.monotonic(),deadline,'step progress timed out')
                time.sleep(.01)
            process.stdin.write(b'{"type":"cancel"}\n');process.stdin.flush()
            process.wait(timeout=8)
            process.communicate(timeout=5)
            self.assertEqual(process.returncode,130)
            report=json.loads((self.root/'result.json').read_text())
            self.assertEqual(report['outcome'],'cancelled')
            self.assertGreater(report['events_dropped'],0)
        finally:
            if process.poll() is None:process.kill();process.communicate()

    def test_parent_crash_terminates_busy_js_worker(self):
        command=self.prepare_running()
        path=self.root/'scenario.json';value=json.loads(path.read_text())
        value['steps']=[{'type':'javascript','id':'busy','timeout_ms':60000,'script':'for(;;){}'}]
        write_json(path,value)
        process=subprocess.Popen(command,stdout=subprocess.PIPE,stderr=subprocess.PIPE,stdin=subprocess.PIPE,text=True,encoding='utf8')
        try:
            while True:
                event=json.loads(process.stdout.readline())
                if event['type']=='script_worker_started':break
            worker=psutil.Process(event['pid'])
            time.sleep(.05)
            process.kill();process.communicate(timeout=5)
            worker.wait(timeout=3)
        finally:
            if process.poll() is None:process.kill();process.communicate()


from pathlib import Path
del TransportTests
