"""HTTP editor workflows and real local wire behavior in an isolated workspace."""
import base64,json,os,sys,tempfile,threading,unittest
from pathlib import Path
from unittest.mock import patch
from dataclasses import replace
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
from urllib.parse import urlsplit,parse_qs
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
os.environ.setdefault('QT_QPA_PLATFORM','offscreen')
import jwt
from PySide6.QtCore import Qt
from PySide6.QtTest import QTest
from PySide6.QtWidgets import QApplication
from main import MainWindow,Store
from general_settings import GeneralConfig,effective_request
from http_runtime import send_http,CookieStore
from grpc_support import Cancellation
from proxy_support import ProxyConfig
from request_transport import prepare_http
from http_features import preview_headers,configure_tls
from command_import import parse_command
from code_snippets import generate_snippet
from ui_theme import STYLE
from ui_brand import VERSION


class EditorNetwork(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.seen=[];owner=cls
        class Handler(BaseHTTPRequestHandler):
            def log_message(self,*_):pass
            def do_POST(self):self.do_GET()
            def do_GET(self):
                body=self.rfile.read(int(self.headers.get('Content-Length',0)));owner.seen.append((self.command,self.path,dict(self.headers),body))
                if self.path=='/folded':self.connection.sendall(b'HTTP/1.1 200 OK\r\nX-Folded: one\r\n two\r\nContent-Length: 2\r\n\r\nok');return
                self.send_response(302 if self.path in ('/redirect','/loop','/other') else 200)
                if self.path=='/redirect':self.send_header('Location','/echo')
                if self.path=='/loop':self.send_header('Location','/loop')
                if self.path=='/other':self.send_header('Location',f'http://127.0.0.1:{owner.other.server_port}/echo')
                if self.path=='/cookie':self.send_header('Set-Cookie','editor=yes; Path=/')
                self.send_header('Content-Length','2');self.end_headers();self.wfile.write(b'ok')
        cls.server=ThreadingHTTPServer(('127.0.0.1',0),Handler);cls.other=ThreadingHTTPServer(('127.0.0.1',0),Handler)
        for server in (cls.server,cls.other):threading.Thread(target=server.serve_forever,kwargs={'poll_interval':.02},daemon=True).start()
    @classmethod
    def tearDownClass(cls):
        for server in (cls.server,cls.other):server.shutdown();server.server_close()
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.root=Path(self.temp.name);self.config=GeneralConfig(working_directory=str(self.root),postman_token=False)
        self.cookies=CookieStore(self.root);self.seen.clear()
    def tearDown(self):self.temp.cleanup()
    def send(self,path='/echo',**fields):
        data=dict(url=f'http://127.0.0.1:{self.server.server_port}'+path,http_timeout=2,general_overrides=True,**fields)
        return send_http(effective_request(data,self.config,self.root),{},ProxyConfig(system=False,environment=False),self.cookies,Cancellation())
    def test_generated_headers_match_wire_and_respect_override_disabled(self):
        self.send();headers={k.lower():v for k,v in self.seen[-1][2].items()}
        self.assertEqual(headers['user-agent'],'Reqon/'+VERSION);self.assertEqual(headers['accept-encoding'],'gzip, deflate');self.assertEqual(headers['cache-control'],'no-cache')
        self.send(headers=[['User-Agent','custom']],disabled_headers=['cache-control','accept'])
        headers={k.lower():v for k,v in self.seen[-1][2].items()};self.assertEqual(headers['user-agent'],'custom');self.assertNotIn('cache-control',headers);self.assertNotIn('accept',headers)
        self.assertEqual(len(preview_headers({'url':'https://example.test'},{})),7)
    def test_jwt_header_query_base64_and_asymmetric_signatures(self):
        secret='a sufficiently long shared secret for tests';data=dict(url='https://example.test',auth_type='JWT Bearer',jwt_secret=base64.b64encode(secret.encode()).decode(),jwt_secret_base64=True,jwt_payload='{"sub":"42"}',jwt_headers='{"kid":"local"}')
        headers=prepare_http(data,{})[2];token=headers['Authorization'].split(' ',1)[1]
        self.assertEqual(jwt.decode(token,secret,algorithms=['HS256']),{'sub':'42'});self.assertEqual(jwt.get_unverified_header(token)['kid'],'local')
        data.update(jwt_location='Query Param',jwt_query_name='access_token');url,_,headers,_=prepare_http(data,{})
        self.assertNotIn('Authorization',headers);self.assertEqual(jwt.decode(parse_qs(urlsplit(url).query)['access_token'][0],secret,algorithms=['HS256'])['sub'],'42')
        from cryptography.hazmat.primitives.asymmetric import rsa
        from cryptography.hazmat.primitives import serialization
        key=rsa.generate_private_key(65537,2048);data.update(jwt_location='Request Header',jwt_algorithm='RS256',jwt_secret_base64=False,jwt_secret=key.private_bytes(serialization.Encoding.PEM,serialization.PrivateFormat.PKCS8,serialization.NoEncryption()).decode())
        token=prepare_http(data,{})[2]['Authorization'].split(' ',1)[1];self.assertEqual(jwt.decode(token,key.public_key(),algorithms=['RS256'])['sub'],'42')
        with self.assertRaisesRegex(ValueError,'must match'):prepare_http(dict(data,jwt_headers='{"alg":"HS256"}'),{})
    def test_multipart_binary_and_graphql_actual_bytes(self):
        (self.root/'upload.bin').write_bytes(b'\x00\xfffile')
        rows=[['note','hello','','', 'Text'],['document','upload.bin','',True,'File'],['disabled','omit','',False,'Text']];rows[0][3]=True
        self.send(method='POST',body_type='form-data',multipart_rows=rows)
        _,_,headers,body=self.seen[-1];boundary=next(v for k,v in headers.items() if k.lower()=='content-type').split('boundary=')[1]
        self.assertIn(boundary.encode(),body);self.assertIn(b'\x00\xfffile',body);self.assertIn(b'hello',body);self.assertNotIn(b'omit',body)
        self.send(method='POST',body_type='binary',body_file='upload.bin');self.assertEqual(self.seen[-1][3],b'\x00\xfffile')
        self.send(method='POST',body_type='GraphQL',graphql_query='query Get { viewer { id } }',graphql_variables='{"id":1}')
        self.assertEqual(json.loads(self.seen[-1][3])['variables'],{'id':1})
        with self.assertRaises(ValueError):self.send(method='POST',body_type='GraphQL',graphql_variables='[]')
        with self.assertRaises(ValueError):self.send(method='POST',body_type='binary',body_file='../outside.bin')
    def test_redirect_method_authorization_referer_and_limits(self):
        fields=dict(method='POST',body_type='raw',body='hello',headers=[['Authorization','Bearer local'],['Referer','https://example.test']])
        self.send('/redirect',**fields);self.assertEqual(self.seen[-1][0],'GET');self.assertEqual(self.seen[-1][3],b'')
        self.send('/other',http_follow_original_method=True,http_remove_referer=True,**fields)
        method,_,headers,body=self.seen[-1];headers={k.lower():v for k,v in headers.items()}
        self.assertEqual(method,'POST');self.assertEqual(body,b'hello');self.assertNotIn('authorization',headers);self.assertNotIn('referer',headers)
        self.send('/other',http_follow_auth=True,**fields);self.assertIn('Bearer local',self.seen[-1][2].values())
        with self.assertRaisesRegex(ValueError,'limit: 2'):self.send('/loop',http_max_redirects=2)
    def test_cookie_disable_url_encoding_and_strict_parser(self):
        self.send('/cookie');self.send();self.assertIn('editor=yes',self.seen[-1][2].values())
        self.send(http_disable_cookies=True);self.assertFalse(any(k.lower()=='cookie' for k in self.seen[-1][2]))
        self.send('/space here/тест');self.assertEqual(self.seen[-1][1],'/space%20here/%D1%82%D0%B5%D1%81%D1%82')
        self.assertEqual(self.send('/folded')['body'],'ok')
        with self.assertRaisesRegex(ValueError,'Strict HTTP parser'):self.send('/folded',http_strict_parser=True)
    def test_command_file_references_do_not_read_or_execute_and_roundtrip(self):
        with patch('builtins.open',side_effect=AssertionError('read')),patch('subprocess.run',side_effect=AssertionError('execute')):
            data=parse_command("curl https://example.test -F 'file=@private.bin' --form-string 'value=@literal'").data
            self.assertEqual(data['multipart_rows'][0][4],'File');self.assertEqual(data['multipart_rows'][1][4],'Text')
            self.assertEqual(parse_command('curl https://example.test --data-binary @private.bin').data['body_file'],'private.bin')
        (self.root/'file with space.bin').write_bytes(b'local')
        data.update(_general=self.config,multipart_rows=[['file','file with space.bin','',True,'File'],['text','@literal','',True,'Text']])
        imported=parse_command(generate_snippet(data)[1]).data;self.assertEqual(imported['multipart_rows'][1][1],'@literal');self.assertEqual(Path(imported['multipart_rows'][0][1]).name,'file with space.bin')
    def test_tls_choices_apply_and_reject_invalid_input(self):
        import ssl,warnings
        context=ssl.create_default_context()
        with warnings.catch_warnings():
            warnings.simplefilter('ignore',DeprecationWarning);configure_tls(context,{'http_disabled_tls':'TLSv1.2','http_ciphers':'ECDHE+AESGCM','http_server_cipher_order':True},{})
        self.assertTrue(context.options&ssl.OP_NO_TLSv1_2);self.assertTrue(context.options&ssl.OP_CIPHER_SERVER_PREFERENCE)
        with self.assertRaisesRegex(ValueError,'Unknown TLS protocol'):configure_tls(context,{'http_disabled_tls':'invalid'},{})
        with self.assertRaisesRegex(ValueError,'Invalid TLS cipher'):configure_tls(context,{'http_ciphers':'not-a-cipher'},{})


class EditorUI(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app=QApplication.instance() or QApplication([]);cls.app.setStyle('Fusion');cls.app.setStyleSheet(STYLE)
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.store=Store(self.temp.name);self.window=MainWindow(self.store);self.window.resize(1213,872);self.window.show()
        self.page=self.window.new_request({'name':'Keep name','url':'https://example.test','method':'GET'});QTest.qWait(30)
    def tearDown(self):
        self.window.close();self.window.deleteLater();self.app.processEvents();self.store.db.close();self.temp.cleanup()
    def test_graphql_only_changes_non_post_method_and_headers_reveal(self):
        with patch('ui_workspace_panels.show_toast') as toast:
            self.page.body_type.setCurrentText('GraphQL');self.assertEqual(self.page.method.currentText(),'POST');toast.assert_called_once()
            self.page.body_type.setCurrentText('none');self.page.body_type.setCurrentText('GraphQL');toast.assert_called_once()
        self.page.tabs.setCurrentIndex(2);panel=self.page.headers_panel;panel.refresh();self.assertFalse(panel.generated.isVisible());panel.toggle.click();self.assertTrue(panel.generated.isVisible())
        self.assertEqual(self.page.tabs.tabText(1),'Auth')
    def test_sidebar_console_and_info(self):
        self.window.rail_buttons['Collections'].click();self.assertTrue(self.window._sidebar_collapsed);self.assertTrue(self.window.rail_buttons['Collections'].isVisible());self.assertEqual(self.window.sidebar_container.width(),76)
        self.window.rail_buttons['Collections'].click();self.assertFalse(self.window._sidebar_collapsed)
        self.window.console_button.click();self.assertTrue(self.window.console_panel.isVisible());self.assertEqual(self.window.console_panel.stack.currentIndex(),0)
        self.window.console_entries.extend(['GET /one 200','POST /two ERROR']);panel=self.window.console_panel;panel.refresh();panel.search.setText('ERROR');self.assertNotIn('/one',panel.output.toPlainText());self.assertIn('/two',panel.output.toPlainText());panel.clear();self.assertEqual(panel.stack.currentIndex(),0)
        self.window.info_button.click();self.assertEqual(self.window.info_button.text(),'Info');self.assertIn(VERSION,self.window.info_dialog.version_label.text());self.assertIn('https://github.com/Norwi4/reqon/issues',self.window.info_dialog.issues_link.text());self.window.info_dialog.accept()
    def test_command_edit_updates_same_draft_and_invalid_input_preserves_request(self):
        self.window.show_code_snippet();panel=self.window.snippet_panel;uid=self.page._draft_id;count=self.window.workbench.count()
        panel.editor.setPlainText("curl -X PATCH https://example.test/edited --json '{\"value\":42}'");QTest.qWait(800)
        page=self.window.current_page();self.assertEqual(page.method.currentText(),'PATCH');self.assertEqual(page._draft_id,uid);self.assertEqual(page.name.text(),'Keep name');self.assertEqual(self.window.workbench.count(),count)
        saved=page.collect();panel.editor.setPlainText("curl 'unfinished");QTest.qWait(750);self.assertEqual(self.window.current_page().collect(),saved);self.assertEqual(panel.editor.toPlainText(),"curl 'unfinished")
        other=self.window.new_request({'url':'https://other.test'});QTest.qWait(250);self.assertIn('other.test',panel.editor.toPlainText());self.assertEqual(other.method.currentText(),'GET')
        self.window.workbench.setCurrentWidget(page);QTest.qWait(250);self.assertEqual(panel.editor.toPlainText(),"curl 'unfinished")
    def test_manual_save_command_edit_and_new_fields_persist(self):
        self.window.general_preferences.config=replace(self.window.general_preferences.config,autosave=False,ask_unsaved=False)
        page=self.page;uid=page._draft_id;before=self.store.all('SELECT data FROM local_drafts WHERE uid=?',(uid,))[0][0]
        self.window.show_code_snippet();self.window.snippet_panel.editor.setPlainText('curl -X DELETE https://example.test/edited');QTest.qWait(800)
        page=self.window.current_page();self.assertNotEqual(page.collect(),page._saved);self.assertEqual(page.save_state.text(),'Unsaved changes')
        self.assertEqual(self.store.all('SELECT data FROM local_drafts WHERE uid=?',(uid,))[0][0],before)
        page.auth_type.setCurrentText('JWT Bearer');page.auth_panel.jwt_secret.setPlainText('local');page.http_max_redirects.setValue(3);page.body_type.setCurrentText('GraphQL');page.body_modes.graphql_query.setPlainText('{ viewer { id } }')
        data=page.collect();self.window.flush_page(page,force=True)
        from ui_pages import RequestPage
        reopened=RequestPage(data);self.assertEqual(reopened.collect(),data);reopened.deleteLater()
    def test_collection_roundtrip_and_standard_postman_body_fields(self):
        cid=self.store.insert('collections',{'name':'HTTP editor','kind':'http'})
        data=dict(self.page.collect(),body_type='form-data',multipart_rows=[['text','value','note',True,'Text'],['file','local.bin','',False,'File']],auth_type='API Key',auth_value='key:secret',api_key_location='Query Params',http_max_redirects=3)
        self.store.save_request(data,collection_id=cid);document=self.store.export_document(cid);request=document['item'][0]['request']
        self.assertEqual(request['body']['formdata'][1]['src'],'local.bin');self.assertTrue(request['body']['formdata'][1]['disabled']);self.assertEqual(request['auth']['type'],'apikey')
        copied=self.store.import_document(document);rid=self.store.children(copied)[0][1];self.assertEqual(json.loads(self.store.entity('request',rid)['data']),data)
        document['item'][0].pop('x-getman');copied=self.store.import_document(document);rid=self.store.children(copied)[0][1];standard=json.loads(self.store.entity('request',rid)['data'])
        self.assertEqual(standard['multipart_rows'],data['multipart_rows']);self.assertEqual(standard['api_key_location'],'Query Params')
    def test_disabling_auth_header_remains_available_and_settings_are_live(self):
        page=self.page;page.auth_type.setCurrentText('Basic Auth');page.auth_panel.username.setText('user');page.auth_panel.password.setText('password')
        page.tabs.setCurrentIndex(2);panel=page.headers_panel;panel.expanded=True;panel.refresh()
        row=next(r for r in range(panel.generated.rowCount()) if panel.generated.item(r,1).text()=='Authorization')
        panel.generated.item(row,0).setCheckState(Qt.CheckState.Unchecked);panel.refresh();self.assertIn('authorization',page.collect()['disabled_headers']);self.assertNotIn('Authorization',prepare_http(page.collect(),{})[2])
        self.assertTrue(any(panel.generated.item(r,1).text()=='Authorization' for r in range(panel.generated.rowCount())))
        self.assertFalse(page.general_overrides.isChecked());self.assertTrue(page.http_verify.isEnabled());page.http_verify.setChecked(True);self.assertTrue(page.general_overrides.isChecked());self.assertTrue(page.runtime_data_provider()['http_verify'])


if __name__=='__main__':unittest.main(verbosity=2)
