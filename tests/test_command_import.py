"""Command import, real local transport and persistence; never use a user workspace."""
import gzip
import json
import os
from pathlib import Path
import shlex
import shutil
import ssl
import subprocess
import sys
import tempfile
import threading
import unittest
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
from unittest.mock import patch

sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
os.environ.setdefault('QT_QPA_PLATFORM','offscreen')
from command_import import parse_command,split_command
from code_snippets import generate_snippet
from request_transport import prepare_http
from PySide6.QtWidgets import QApplication
from main import MainWindow,Store,Worker
from ui_command_import import CommandImportDialog
from ui_theme import STYLE
from ui_pages import Job


class ParserTests(unittest.TestCase):
    def test_browser_json_headers_auth_and_duplicate_query_values(self):
        result=parse_command('''curl 'https://api.example.test/items?a=x%20y&a=2&empty=&flag' \\
          -H 'Authorization: Bearer secret' -H 'Content-Type: application/json' \\
          --data-raw '{"name":"Русский текст", "url":"https://host/a?x=1&y=2"}' ''')
        data=result.data
        self.assertEqual(data['method'],'POST');self.assertEqual(data['auth_type'],'Bearer Token')
        self.assertEqual(data['auth_value'],'secret');self.assertEqual(len(data['params']),4)
        url,method,headers,body=prepare_http(data,{})
        self.assertEqual(url,'https://api.example.test/items?a=x%20y&a=2&empty=&flag')
        self.assertEqual(json.loads(body)['name'],'Русский текст');self.assertEqual(headers['Authorization'],'Bearer secret')
        data['params'][0][1]='edited value'
        self.assertIn('a=edited+value',prepare_http(data,{})[0])

    def test_auth_cookies_form_get_head_and_custom_methods(self):
        data=parse_command("curl -u user:pass -b 'sid=one; other=two' -d a=1 -d b=two https://example.test").data
        self.assertEqual(data['body_type'],'x-www-form-urlencoded');self.assertEqual(data['form'],[['a','1',''],['b','two','']])
        self.assertEqual(prepare_http(data,{})[2]['Authorization'],'Basic dXNlcjpwYXNz')
        self.assertEqual(prepare_http(data,{})[2]['Cookie'],'sid=one; other=two')
        data=parse_command("curl -G --data-urlencode 'q=a & b' https://example.test/search").data
        self.assertEqual(data['method'],'GET');self.assertEqual(data['body_type'],'none')
        self.assertEqual(prepare_http(data,{})[0],'https://example.test/search?q=a%20%26%20b')
        self.assertEqual(parse_command('curl -sSI https://example.test').data['method'],'HEAD')
        self.assertEqual(parse_command('curl -XPROPFIND https://example.test').data['method'],'PROPFIND')
        self.assertEqual(parse_command('curl --json=\'{"a":1}\' https://example.test').data['method'],'POST')

    def test_bash_ansi_cmd_powershell_and_generated_quoting(self):
        text="curl https://example.test --data-raw $'Русский\\nline\\t\\u263a'"
        self.assertEqual(parse_command(text).data['body'],'Русский\nline\t☺')
        cmd='curl.exe "https://example.test" ^\n -H "Content-Type: application/json" ^\n --data-raw "{\\"name\\":\\"Windows\\"}"'
        self.assertEqual(json.loads(parse_command(cmd).data['body']),{'name':'Windows'})
        chrome='curl "https://example.test" ^\n --data-raw "^{^\\^"name^\\^":^\\^"Chrome^\\^"^}"'
        self.assertEqual(json.loads(parse_command(chrome).data['body']),{'name':'Chrome'})
        ps="curl.exe 'https://example.test' `\n --data-raw 'a=''quoted'''"
        self.assertEqual(prepare_http(parse_command(ps).data,{})[3],b"a='quoted'")
        data={'url':'https://example.test','method':'POST','body_type':'raw','body':'{"value":"it\'s quoted"}', 'headers':[['Content-Type','application/json']]}
        command=generate_snippet(data)[1]
        self.assertEqual(prepare_http(parse_command(command).data,{}),prepare_http(data,{}))

    def test_rejects_ambiguous_requests_and_external_input_without_side_effects(self):
        cases=["curl https://example.test -d @private.json", 'curl https://a.test https://b.test',
               "curl https://example.test --proxy http://proxy.test", "curl https://example.test; echo x",
               "curl https://example.test -d $(cat secret)", "curl https://example.test -d 'unfinished",
               'curl https://example.test --config private.conf', 'curl https://example.test --request',
               "curl https://example.test -F 'file=@private'", "curl https://example.test -H 'x: 1' -H 'x: 2'"]
        with patch('subprocess.run',side_effect=AssertionError('Command execution')),patch('builtins.open',side_effect=AssertionError('File access')):
            for command in cases:
                with self.subTest(command=command),self.assertRaises(ValueError):parse_command(command)

    def test_grpc_fields_tls_metadata_streaming_and_roundtrip(self):
        command='grpcurl -insecure -H "authorization: Bearer token" -H "x-id: 42" -max-time 2.5 -max-msg-sz 10485760 -emit-defaults -d \'{"name":"a"} {"name":"b"}\' localhost:443 example.API/Send'
        data=parse_command(command).data
        self.assertEqual((data['server'],data['service'],data['rpc_method']),('localhost:443','example.API','Send'))
        self.assertTrue(data['tls']);self.assertFalse(data['tls_verify']);self.assertEqual(data['grpc_timeout_ms'],2500)
        self.assertEqual(data['auth_type'],'Bearer Token');self.assertEqual(data['metadata'],[['x-id','42','']])
        self.assertEqual(json.loads(data['message']),[{'name':'a'},{'name':'b'}])
        second=parse_command(generate_snippet(data)[1]).data
        for key in ('server','service','rpc_method','message','metadata','tls','tls_verify','grpc_timeout_ms','grpc_max_response_mb'):
            self.assertEqual(second[key],data[key],key)
        self.assertFalse(parse_command('grpcurl -plaintext localhost:9000 example.API.Send').data['tls'])


class ImportUIAndTransport(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app=QApplication.instance() or QApplication([]);cls.app.setStyle('Fusion');cls.app.setStyleSheet(STYLE)
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.store=Store(self.temp.name);self.window=MainWindow(self.store)
    def tearDown(self):
        self.window.close();self.window.deleteLater();self.app.processEvents();self.store.db.close();self.temp.cleanup()
    def request(self,data):
        result=[];worker=Worker(data,{});worker.done.connect(result.append);worker.run();return result[0]

    def test_dialog_drafts_restore_settings_and_collection_roundtrip(self):
        dialog=CommandImportDialog(self.window);count=self.window.workbench.count()
        dialog.editor.setPlainText("curl -kL -m 2.5 -XPROPFIND 'https://example.test?a=x%20y&flag' --json '{\"value\":1}'")
        dialog.refresh();self.assertTrue(dialog.import_button.isEnabled());self.assertEqual(self.window.workbench.count(),count)
        dialog.apply();page=self.window.import_command_request(dialog.imported);saved=page.collect()
        self.assertEqual(saved['method'],'PROPFIND');self.assertEqual(page.tabs.currentIndex(),3)
        self.assertFalse(saved['http_verify']);self.assertTrue(saved['http_follow_redirects']);self.assertEqual(saved['http_timeout'],2.5)
        self.assertEqual(prepare_http(saved,{})[0],'https://example.test?a=x%20y&flag')
        cid=self.store.insert('collections',{'name':'Imported','kind':'http'});page.collection_id=cid;self.window.flush_page(page)
        record=json.loads(self.store.all('SELECT data FROM requests WHERE id=?',(page.request_id,))[0][0]);self.assertEqual(record,saved)
        page.http_timeout.setValue(6.5);self.window.flush_page(page);self.assertEqual(json.loads(self.store.all('SELECT data FROM requests WHERE id=?',(page.request_id,))[0][0])['http_timeout'],6.5)
        imported_cid=self.store.import_document(self.store.export_document(cid))
        copied=json.loads(self.store.all('SELECT data FROM requests WHERE collection_id=?',(imported_cid,))[0][0])
        self.assertEqual(copied,page.collect())
        self.window.save_workspace();self.window.close();self.window.deleteLater();self.app.processEvents()
        self.window=MainWindow(self.store);restored=self.window.current_page().collect()
        self.assertEqual(restored['method'],'PROPFIND');self.assertEqual(restored['http_timeout'],6.5)
        dialog.deleteLater()

    def test_invalid_and_cancelled_import_leave_workspace_unchanged_and_grpc_does_not_connect(self):
        count=self.window.workbench.count();dialog=CommandImportDialog(self.window)
        dialog.editor.setPlainText('curl --unknown x');dialog.apply();self.assertIsNone(dialog.imported)
        self.assertFalse(dialog.import_button.isEnabled());self.assertEqual(self.window.workbench.count(),count)
        dialog.reject();dialog.deleteLater()
        with patch.object(Job,'launch',side_effect=AssertionError('Unexpected connection')):
            page=self.window.import_command_request(parse_command('grpcurl -plaintext -d \'{"name":"hello"}\' localhost:9000 demo.API/Send'))
            self.assertEqual(page.service.text(),'demo.API');self.assertEqual(page.rpc_method.text(),'Send');self.assertEqual(page.tabs.currentIndex(),1)

    def test_real_http_matches_curl_and_handles_redirects_and_compression(self):
        captured=[]
        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                if self.path=='/redirect':self.send_response(302);self.send_header('Location','/target');self.end_headers();return
                self.do_POST()
            def do_POST(self):
                body=self.rfile.read(int(self.headers.get('Content-Length',0)))
                captured.append((self.command,self.path,self.headers.get('Authorization'),self.headers.get('Content-Type'),body))
                response=gzip.compress(b'{"ok":true}');self.send_response(200);self.send_header('Content-Encoding','gzip');self.end_headers();self.wfile.write(response)
            def log_message(self,*_):pass
        server=ThreadingHTTPServer(('127.0.0.1',0),Handler);thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
        try:
            url=f'http://127.0.0.1:{server.server_port}'
            command=f"curl --compressed -u user:pass '{url}/items?q=x%20y&flag' --json '{{\"value\":42}}'"
            page=self.window.import_command_request(parse_command(command))
            result=self.request(page.collect());self.assertEqual(result['status'],200);self.assertEqual(json.loads(result['body']),{'ok':True})
            curl=shutil.which('curl.exe') or shutil.which('curl')
            if curl:
                subprocess.run([curl,*split_command(command)[1:]],capture_output=True,check=True,timeout=10)
                self.assertEqual(captured[-1],captured[-2])
            result=self.request(parse_command(f'curl {url}/redirect').data);self.assertEqual(result['status'],302)
            result=self.request(parse_command(f'curl -L {url}/redirect').data);self.assertEqual(result['status'],200)
        finally:server.shutdown();server.server_close();thread.join(3)

    def test_imported_tls_settings_control_real_https(self):
        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):self.send_response(200);self.end_headers();self.wfile.write(b'ok')
            def log_message(self,*_):pass
        try:from generate_tls_fixtures import generate
        except ImportError:self.skipTest('cryptography is needed to generate isolated HTTPS test certificates')
        fixtures=Path(self.temp.name)/'tls';generate(fixtures)
        server=ThreadingHTTPServer(('127.0.0.1',0),Handler);context=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        context.load_cert_chain(str(fixtures/'server.pem'),str(fixtures/'server.key'));server.socket=context.wrap_socket(server.socket,server_side=True)
        thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
        try:
            url=f'https://localhost:{server.server_port}/'
            self.assertIn('error',self.request(parse_command('curl '+url).data))
            self.assertEqual(self.request(parse_command('curl -k '+url).data)['status'],200)
            command='curl --cacert '+shlex.quote(str(fixtures/'ca.pem'))+' '+url
            result=self.request(parse_command(command).data);self.assertEqual(result['status'],200,result)
        finally:server.shutdown();server.server_close();thread.join(3)

    def test_imported_grpc_invocation_uses_message_and_reflection(self):
        from error_fixture import ErrorServer
        server=ErrorServer()
        try:
            command=f'grpcurl -plaintext -max-time 3 -H "x-id: 42" -d \'{{"name":"imported"}}\' 127.0.0.1:{server.port} errors.API/Echo'
            page=self.window.import_command_request(parse_command(command));result=self.request(page.collect())
            self.assertNotIn('error',result,result);self.assertEqual(json.loads(result['body']),{'name':'imported'})
        finally:server.close()


if __name__=='__main__':unittest.main(verbosity=2)
