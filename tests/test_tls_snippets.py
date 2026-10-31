import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from http.server import BaseHTTPRequestHandler,HTTPServer
from unittest.mock import patch
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import grpc
from PySide6.QtCore import Qt
from PySide6.QtTest import QTest
from PySide6.QtWidgets import QApplication,QFileDialog
from code_snippets import generate_snippet,command_groups
from grpc_support import channel_for,discover_contract,describe_method,example_message,invoke
from main import MainWindow,Store,Worker
from request_transport import TLSOptions,expand,parse_message
from ui_pages import GRPC_DEFAULT,HTTP_DEFAULT,Job
from ui_theme import STYLE
from reflection_fixture import ReflectionServer

FIXTURES=Path(__file__).parent/'fixtures'/'tls'

def credentials(mutual=False):
    return grpc.ssl_server_credentials([((FIXTURES/'server.key').read_bytes(),(FIXTURES/'server.pem').read_bytes())],
        root_certificates=(FIXTURES/'ca.pem').read_bytes() if mutual else None,require_client_auth=mutual)

def secure_data(server,mutual=False):
    return dict(GRPC_DEFAULT,server='https://'+server.address,tls=True,tls_verify=True,tls_ca_file=str(FIXTURES/'ca.pem'),
        tls_cert_file=str(FIXTURES/'client.pem') if mutual else '',tls_key_file=str(FIXTURES/'client.key') if mutual else '',
        service='secure.Customers',rpc_method='GetCustomer',message='{"name":"TLS request"}')


class TLSTransportTests(unittest.TestCase):
    def test_generated_grpcurl_executes_with_mtls_auth_and_local_contract(self):
        binary=os.environ.get('GETMAN_GRPCURL') or shutil.which('grpcurl')
        if not binary:
            local=Path(__file__).resolve().parents[1]/'build'/'test-tools'/'grpcurl.exe'
            if local.exists():binary=str(local)
        if not binary:self.skipTest('Set GETMAN_GRPCURL to test commands using the real grpcurl executable')
        server=ReflectionServer('secure',credentials(True));server.required_token='Bearer test-token'
        try:
            data=dict(secure_data(server,True),auth_type='Bearer Token',auth_value='{{token}}')
            for local_contract in (False,True):
                with tempfile.TemporaryDirectory() as directory:
                    if local_contract:
                        _,contract=discover_contract(data['server'],TLSOptions.from_data(data),metadata=[('authorization','Bearer test-token')])
                        path=Path(directory)/'local contract.protoset';path.write_bytes(contract);data['descriptor_file']=str(path);server.disabled=True
                    _,groups=command_groups(data,{'token':'test-token'})
                    args=[value for group in groups for value in group];args[0]=binary
                    result=subprocess.run(args,capture_output=True,text=True,encoding='utf-8',timeout=15)
                    self.assertEqual(result.returncode,0,result.stderr);self.assertEqual(json.loads(result.stdout),{'name':'TLS request'})
        finally:server.close()

    def test_tls_reflection_schema_example_and_invoke_use_the_same_certificates(self):
        server=ReflectionServer('secure',credentials())
        try:
            data=secure_data(server); tls=TLSOptions.from_data(data)
            methods,contract=discover_contract(data['server'],tls)
            self.assertIn(('secure.Customers','GetCustomer'),methods)
            schema=describe_method(data['server'],tls,'','secure.Customers','GetCustomer',schema_bytes=contract)
            self.assertEqual(schema[0],'Request')
            self.assertIn('name',json.loads(example_message(data['server'],tls,'','secure.Customers','GetCustomer',schema_bytes=contract)))
            self.assertEqual(json.loads(invoke(data,{},expand)['body']),{'name':'TLS request'})
        finally:server.close()

    def test_untrusted_ca_and_wrong_hostname_are_rejected(self):
        server=ReflectionServer('secure',credentials())
        try:
            for address,tls in [(server.address,TLSOptions(True,verify_server=True)),(f'127.0.0.1:{server.port}',TLSOptions(True,str(FIXTURES/'ca.pem'),verify_server=True))]:
                with self.subTest(address=address),channel_for(address,tls) as channel:
                    with self.assertRaises(grpc.FutureTimeoutError):grpc.channel_ready_future(channel).result(timeout=1.5)
        finally:server.close()

    def test_mutual_tls_requires_client_identity(self):
        server=ReflectionServer('secure',credentials(True))
        try:
            with channel_for(server.address,TLSOptions(True,str(FIXTURES/'ca.pem'),verify_server=True)) as channel:
                with self.assertRaises(grpc.FutureTimeoutError):grpc.channel_ready_future(channel).result(timeout=1.5)
            result=invoke(secure_data(server,True),{},expand)
            self.assertEqual(result['status'],0)
        finally:server.close()

    def test_plaintext_choice_is_authoritative_and_certificate_errors_are_readable(self):
        server=ReflectionServer('secure')
        try:
            with channel_for('grpcs://'+server.address,False) as channel:grpc.channel_ready_future(channel).result(timeout=5)
            with self.assertRaisesRegex(ValueError,'both a client'):
                channel_for(server.address,TLSOptions(True,cert_file=str(FIXTURES/'client.pem')))
            with self.assertRaisesRegex(ValueError,'matching PEM'):
                channel_for(server.address,TLSOptions(True,cert_file=str(FIXTURES/'client.pem'),key_file=str(FIXTURES/'server.key')))
        finally:server.close()

    def test_grpcurl_carries_tls_metadata_contract_and_streaming_messages(self):
        data=dict(GRPC_DEFAULT,server='grpcs://{{host}}/',tls=True,tls_verify=True,tls_ca_file='CA file.pem',tls_cert_file='client.pem',tls_key_file='client.key',
            service='example.Service',rpc_method='Call',descriptor_file='contracts/api set.protoset',auth_type='Bearer Token',auth_value='{{token}}',
            metadata_rows=[['X-Active','yes','',True],['X-Disabled','no','',False],['authorization','old','',True]],
            message='[ {"name": "O\'Brien $(echo nope)"}, // ignored\n {"name":"https://example.test/a/*b*/",}, ]')
        label,groups=command_groups(data,{'host':'localhost:50051','token':'secret'})
        args=[value for group in groups for value in group]
        self.assertEqual(label,'gRPCurl'); self.assertNotIn('-plaintext',args)
        self.assertIn(['-cacert','CA file.pem'],groups); self.assertIn(['-protoset','contracts/api set.protoset'],groups)
        self.assertIn(['-H','authorization: Bearer secret'],groups)
        self.assertFalse(any('x-disabled' in value or value=='authorization: old' for value in args))
        self.assertEqual(args[-2:],['localhost:50051','example.Service/Call'])
        payload=args[args.index('-d')+1].splitlines(); self.assertEqual(len(payload),2)
        self.assertEqual(json.loads(payload[1]),{'name':'https://example.test/a/*b*/'})
        command=generate_snippet(data,{'host':'localhost:50051','token':'secret'})[1]
        self.assertEqual(shlex.split(command.replace('\\\n','')),args)

    def test_generated_curl_matches_the_request_sent_by_the_app(self):
        curl=shutil.which('curl.exe') or shutil.which('curl')
        if not curl:self.skipTest('curl executable is not available')
        captured=[]
        class Handler(BaseHTTPRequestHandler):
            def do_HEAD(self):
                self.send_response(200);self.send_header('Content-Length','100');self.end_headers()
            def do_POST(self):
                captured.append((self.path,self.headers.get('Authorization'),self.headers.get('Content-Type'),self.rfile.read(int(self.headers.get('Content-Length',0)))))
                self.send_response(200); self.end_headers(); self.wfile.write(b'{}')
            def log_message(self,*args):pass
        server=HTTPServer(('127.0.0.1',0),Handler); thread=threading.Thread(target=server.serve_forever,daemon=True); thread.start()
        try:
            for mode in ('raw','x-www-form-urlencoded'):
                data=dict(HTTP_DEFAULT,url='http://127.0.0.1:'+str(server.server_port)+'/test?old=1#fragment',method='POST',
                    param_rows=[['q','{{query}}','',True],['disabled','value','',False]],auth_type='Basic Auth',auth_value='user:pass',
                    header_rows=[['authorization','stale','',True],['X-Disabled','no','',False]],body_type=mode,
                    body=json.dumps({'text':"O'Brien $(literal)"}),form_rows=[['text','a & b','',True],['disabled','x','',False]])
                worker=Worker(data,{'query':'a & b'}); worker.run()
                _,groups=command_groups(data,{'query':'a & b'}); args=[value for group in groups for value in group]; args[0]=curl
                result=subprocess.run(args,capture_output=True,timeout=10)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertEqual(captured[-1],captured[-2]); self.assertIn('q=a+%26+b',captured[-1][0]); self.assertNotIn('disabled',captured[-1][0])
                self.assertEqual(captured[-1][1],'Basic dXNlcjpwYXNz')
            _,groups=command_groups(dict(HTTP_DEFAULT,url=f'http://127.0.0.1:{server.server_port}/test',method='HEAD'))
            args=[value for group in groups for value in group];args[0]=curl
            self.assertEqual(subprocess.run(args,capture_output=True,timeout=5).returncode,0)
        finally:server.shutdown();server.server_close();thread.join(3)


class TLSAndSnippetUI(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app=QApplication.instance() or QApplication([]); cls.app.setStyle('Fusion');cls.app.setStyleSheet(STYLE)
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory(); self.store=Store(self.temp.name); self.window=MainWindow(self.store)
        self.window.show();self.app.processEvents()
    def wait_for(self,predicate):
        deadline=time.monotonic()+25
        while not predicate() and time.monotonic()<deadline:QTest.qWait(20)
        self.assertTrue(predicate())
    def tearDown(self):
        self.wait_for(lambda:not Job.running and not self.window.workers)
        self.window.close();self.window.deleteLater();self.app.processEvents();self.store.db.close();self.temp.cleanup()

    def test_lock_toggles_tls_and_settings_and_persists_in_collection(self):
        page=self.window.current_page(); page.server.setText('https://localhost:50051')
        self.assertTrue(page.tls_button.isChecked());self.assertEqual(page.tls_button.heading.text(),'Disable TLS')
        page.tls_button.click();self.assertFalse(page.collect()['tls']);self.assertEqual(page.server.text(),'grpc://localhost:50051')
        self.assertEqual(page.tls_button.heading.text(),'Enable TLS')
        page.tls.setCurrentIndex(1);self.assertTrue(page.tls_button.isChecked());self.assertEqual(page.server.text(),'grpcs://localhost:50051')
        page.tls_button.link.linkActivated.emit('certificates');self.assertEqual(page.tabs.currentIndex(),5)
        page.tls_ca_file.setText(str(FIXTURES/'ca.pem')); page.tls_cert_file.setText(str(FIXTURES/'client.pem'));page.tls_key_file.setText(str(FIXTURES/'client.key'))
        cid=self.store.insert('collections',{'name':'Secure','kind':'grpc'});page.collection_id=cid;self.window.flush_page(page)
        target=str(Path(self.temp.name)/'secure.json')
        with patch.object(QFileDialog,'getSaveFileName',return_value=(target,'')):self.window.export_collection(cid)
        exported=Path(target).read_text(encoding='utf-8');self.assertNotIn('BEGIN PRIVATE KEY',exported)
        with patch.object(QFileDialog,'getOpenFileName',return_value=(target,'')):self.window.import_collection()
        records=[json.loads(row[0]) for row in self.store.all('SELECT data FROM requests ORDER BY id')]
        self.assertEqual(records[-1]['tls_ca_file'],str(FIXTURES/'ca.pem'));self.assertTrue(records[-1]['tls'])

    def test_switching_protocol_preserves_the_selected_connection_mode(self):
        page=self.window.current_page();page.server.setText('grpcs://localhost:50051')
        self.window.switch_request_type(page,'http');http=self.window.current_page()
        self.assertEqual(http.url.text(),'https://localhost:50051')
        self.window.switch_request_type(http,'grpc');page=self.window.current_page()
        self.assertTrue(page.collect()['tls']);page.tls_button.click()
        self.window.switch_request_type(page,'http')
        self.assertEqual(self.window.current_page().url.text(),'http://localhost:50051')

    def test_lock_enables_real_tls_reflection_and_invocation(self):
        server=ReflectionServer('secure',credentials(True))
        try:
            page=self.window.current_page();page.server.setText(server.address)
            page.tls_ca_file.setText(str(FIXTURES/'ca.pem'));page.tls_cert_file.setText(str(FIXTURES/'client.pem'));page.tls_key_file.setText(str(FIXTURES/'client.key'))
            page.tls_button.click();page.method_picker.show_popup();self.wait_for(lambda:page.method_picker.state=='ready')
            page.method_picker.dismiss();page.select_method('secure.Customers','GetCustomer');page.message.setPlainText('{"name":"from UI"}')
            page.send_button.click();self.wait_for(lambda:not self.window.workers)
            self.assertEqual(json.loads(page.response.editor.toPlainText()),{'name':'from UI'})
            page.tls_button.click();self.assertIsNone(page.current_contract())
        finally:server.close()

    def test_snippet_panel_updates_copies_switches_protocol_and_fits_compact_window(self):
        page=self.window.current_page();page.server.setText('localhost:50051');page.select_method('example.API','Send')
        self.window.resize(1000,700);self.app.processEvents();self.window.code_button.click();self.app.processEvents()
        panel=self.window.snippet_panel;self.assertTrue(panel.isVisible());self.assertLessEqual(self.window.width(),1000)
        self.assertIn('-plaintext',panel.command)
        page.tls_button.click();page.metadata.set_rows([['x-id','{{id}}','']]);page.message.setPlainText('{"name":"value"}')
        eid=self.store.insert('environments',{'name':'Local','variables':'{"id":"42"}'})
        self.window.refresh_environments();self.window.env_combo.setCurrentIndex(self.window.env_combo.findData(eid))
        self.wait_for(lambda:'x-id: 42' in panel.command and '-plaintext' not in panel.command)
        panel.copy_button.click();self.assertEqual(QApplication.clipboard().text(),panel.command)
        http=self.window.new_request(dict(HTTP_DEFAULT,url='https://example.test'))
        self.wait_for(lambda:panel.language.text()=='cURL');self.assertIn('curl',panel.command)
        http.url.clear();self.wait_for(lambda:not panel.copy_button.isEnabled());self.assertEqual(panel.command,'')
        panel.close_button.click();self.assertFalse(panel.isVisible());self.assertTrue(self.window.workspace_splitter.widget(0).isVisible())

if __name__=='__main__':unittest.main(verbosity=2)
