import gzip,json,os,socketserver,ssl,sys,tempfile,threading,time,unittest
from pathlib import Path
from dataclasses import replace
from unittest.mock import patch
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
os.environ.setdefault('QT_QPA_PLATFORM','offscreen')
from general_settings import GeneralConfig,GeneralPreferences,effective_request,resolve_file
from http_runtime import send_http,CookieStore,read_limited
from grpc_support import Cancellation,invoke
from proxy_support import ProxyConfig
from request_transport import expand
from generate_tls_fixtures import generate
from proxy_fixture import Proxy
from reflection_fixture import ReflectionServer

DIRECT=ProxyConfig(system=False,environment=False)


class GeneralNetworkTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp=tempfile.TemporaryDirectory();cls.root=Path(cls.temp.name);generate(cls.root)
        cls.seen=[];cls.slow_started=threading.Event();owner=cls
        class Handler(BaseHTTPRequestHandler):
            def log_message(self,*_):pass
            def do_POST(self):self.do_GET()
            def do_GET(self):
                raw=self.rfile.read(int(self.headers.get('Content-Length',0)));owner.seen.append((self.path,{key.lower():value for key,value in self.headers.items()},raw))
                if self.path=='/slow':owner.slow_started.set();time.sleep(.25)
                self.send_response(302 if self.path=='/redirect' else 200)
                if self.path=='/set':self.send_header('Set-Cookie','local_cookie=yes; Path=/; HttpOnly')
                if self.path=='/redirect':self.send_header('Location','/echo')
                if self.path=='/large':body=gzip.compress(b'x'*(2*1024*1024));self.send_header('Content-Encoding','gzip')
                else:body=json.dumps({'cookie':self.headers.get('Cookie'),'path':self.path,'body':raw.decode(errors='replace')}).encode()
                self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(body)));self.end_headers()
                try:self.wfile.write(body)
                except (OSError,ssl.SSLError):pass
        cls.http=ThreadingHTTPServer(('127.0.0.1',0),Handler);cls.https=ThreadingHTTPServer(('127.0.0.1',0),Handler)
        context=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER);context.load_cert_chain(str(cls.root/'server.pem'),str(cls.root/'server.key'));cls.https.socket=context.wrap_socket(cls.https.socket,server_side=True)
        for server in (cls.http,cls.https):threading.Thread(target=server.serve_forever,kwargs={'poll_interval':.02},daemon=True).start()
    @classmethod
    def tearDownClass(cls):
        for server in (cls.http,cls.https):server.shutdown();server.server_close()
        cls.temp.cleanup()
    def config(self,**kwargs):return replace(GeneralConfig(working_directory=str(self.root)),**kwargs)
    def send(self,path='/echo',config=None,proxy=DIRECT,cookies=None,**data):
        data=dict(url=f'http://127.0.0.1:{self.http.server_port}'+path,**data)
        return send_http(effective_request(data,config or self.config(),self.root),{},proxy,cookies,Cancellation())

    def test_headers_cookies_persist_disable_redirect_and_http_version(self):
        with tempfile.TemporaryDirectory() as folder:
            cookies=CookieStore(folder);first=self.send('/set',cookies=cookies)
            self.assertEqual(first['http_version'],'HTTP/1.0')
            second=self.send(cookies=CookieStore(folder));self.assertEqual(json.loads(second['body'])['cookie'],'local_cookie=yes')
            self.assertEqual(self.seen[-1][1]['cache-control'],'no-cache');token=self.seen[-1][1]['postman-token']
            self.send(config=self.config(disable_cookies=True,no_cache=False,postman_token=False),cookies=cookies)
            self.assertNotIn('cookie',self.seen[-1][1]);self.assertNotIn('cache-control',self.seen[-1][1]);self.assertNotIn('postman-token',self.seen[-1][1])
            self.send();self.assertNotEqual(token,self.seen[-1][1]['postman-token'])
            self.assertEqual(self.send('/redirect',config=self.config(follow_redirects=False))['status'],302)
            self.assertEqual(json.loads(self.send('/redirect')['body'])['path'],'/echo')

    def test_deadline_and_response_limit_cover_decoded_bytes(self):
        with self.assertRaises((TimeoutError,OSError)):self.send('/slow',config=self.config(timeout_ms=40))
        with self.assertRaisesRegex(ValueError,'Max response size'):self.send('/large',config=self.config(max_response_mb=1))
        self.assertEqual(len(self.send('/large',config=self.config(max_response_mb=0))['body']),2*1024*1024)
        self.assertEqual(self.send('/slow',config=self.config(timeout_ms=0))['status'],200)

    def test_files_validation_and_explicit_per_request_override(self):
        body=self.root/'body.json';body.write_text('{"value":42}')
        schema=self.root/'schema.json';schema.write_text('{"type":"object","properties":{"value":{"type":"string"}}}')
        with self.assertRaisesRegex(ValueError,'Request validation'):
            self.send(method='POST',body_type='file',body_file='body.json',http_schema='schema.json')
        result=self.send(config=self.config(request_validation=False),method='POST',body_type='file',body_file='body.json',http_schema='schema.json')
        self.assertEqual(json.loads(result['body'])['body'],'{"value":42}')
        with tempfile.NamedTemporaryFile() as external:
            with self.assertRaisesRegex(ValueError,'outside'):resolve_file(external.name,self.config())
            self.assertEqual(resolve_file(external.name,self.config(read_outside=True)),Path(external.name).resolve())
        self.assertEqual(self.send('/redirect',general_overrides=True,http_follow_redirects=False)['status'],302)
        self.send(config=self.config(request_validation=False),method='POST',body_type='raw',body='{broken')
        self.assertEqual(self.seen[-1][2],b'{broken')

    def test_tls_verification_keylog_and_proxy_survive_new_transport(self):
        for protocol in ('http','socks5h'):
            with Proxy(protocol,auth=True) as proxy:
                config=DIRECT.__class__(system=False,environment=False,custom=True,protocol=protocol,host='127.0.0.1',port=str(proxy.port),auth=True,username='test',password='secret')
                self.assertEqual(self.send(proxy=config)['status'],200)
        data=dict(url=f'https://localhost:{self.https.server_port}/echo',http_ca_file=str(self.root/'ca.pem'))
        result=send_http(effective_request(data,self.config(verify_ssl=True,tls_keylog=True),self.root),{},DIRECT,None,Cancellation())
        self.assertEqual(result['status'],200);self.assertIn('TRAFFIC_SECRET',(self.root/'logs/tls-keys.log').read_text())
        data.pop('http_ca_file')
        with self.assertRaises(ssl.SSLError):send_http(effective_request(data,self.config(verify_ssl=True),self.root),{},DIRECT,None,Cancellation())
        self.assertEqual(send_http(effective_request(data,self.config(verify_ssl=False),self.root),{},DIRECT,None,Cancellation())['status'],200)

    def test_http2_and_auto_negotiate_real_h2_tls_server(self):
        import h2.connection,h2.config,h2.events
        calls=[]
        class Handler(socketserver.BaseRequestHandler):
            def handle(self):
                connection=h2.connection.H2Connection(config=h2.config.H2Configuration(client_side=False));connection.initiate_connection();self.request.sendall(connection.data_to_send())
                while True:
                    raw=self.request.recv(65535)
                    if not raw:return
                    for event in connection.receive_data(raw):
                        if isinstance(event,h2.events.RequestReceived):calls.append(event.headers)
                        if isinstance(event,h2.events.DataReceived):connection.acknowledge_received_data(event.flow_controlled_length,event.stream_id)
                        if isinstance(event,h2.events.StreamEnded):
                            connection.send_headers(event.stream_id,[(b':status',b'200'),(b'content-type',b'application/json')]);connection.send_data(event.stream_id,b'{"h2":true}',end_stream=True)
                    self.request.sendall(connection.data_to_send())
        class Server(socketserver.ThreadingTCPServer):daemon_threads=True
        server=Server(('127.0.0.1',0),Handler)
        context=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER);context.load_cert_chain(str(self.root/'server.pem'),str(self.root/'server.key'));context.set_alpn_protocols(['h2']);server.socket=context.wrap_socket(server.socket,server_side=True)
        threading.Thread(target=server.serve_forever,kwargs={'poll_interval':.02},daemon=True).start()
        try:
            for version in ('Auto','HTTP/2'):
                data=effective_request(dict(url=f'https://localhost:{server.server_address[1]}/h2'),self.config(http_version=version),self.root)
                result=send_http(data,{},DIRECT,None,Cancellation());self.assertEqual(result['http_version'],'HTTP/2');self.assertEqual(json.loads(result['body']),{'h2':True})
            self.assertEqual(len(calls),2)
        finally:server.shutdown();server.server_close()

    def test_grpc_general_defaults_and_tls_key_log(self):
        import grpc
        credentials=grpc.ssl_server_credentials([((self.root/'server.key').read_bytes(),(self.root/'server.pem').read_bytes())]);server=ReflectionServer('generaltest',credentials)
        try:
            data=effective_request(dict(kind='grpc',server=server.address,tls=True,service='generaltest.Customers',rpc_method='GetCustomer',message='{"name":"general"}'),self.config(tls_keylog=True),self.root)
            data['_proxy_config']=DIRECT;self.assertEqual(json.loads(invoke(data,{},expand)['body']),{'name':'general'})
        finally:server.close()

    def test_forced_http2_rejects_http1_and_cancel_interrupts_waiting_response(self):
        data=effective_request(dict(url=f'https://localhost:{self.https.server_port}/echo'),self.config(http_version='HTTP/2'),self.root)
        with self.assertRaisesRegex(ValueError,'did not negotiate HTTP/2'):send_http(data,{},DIRECT,None,Cancellation())
        cancellation=Cancellation();errors=[]
        data=effective_request(dict(url=f'http://127.0.0.1:{self.http.server_port}/slow'),self.config(),self.root)
        def send():
            try:send_http(data,{},DIRECT,None,cancellation)
            except Exception as error:errors.append(error)
        self.slow_started.clear();thread=threading.Thread(target=send);thread.start();self.assertTrue(self.slow_started.wait(3));cancellation.cancel();thread.join(1)
        self.assertFalse(thread.is_alive());self.assertTrue(errors)


class GeneralUITests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        from PySide6.QtWidgets import QApplication
        from ui_theme import STYLE
        cls.app=QApplication.instance() or QApplication([]);cls.app.setStyle('Fusion');cls.app.setStyleSheet(STYLE)
    def setUp(self):
        from main import MainWindow,Store
        self.temp=tempfile.TemporaryDirectory();self.store=Store(self.temp.name);self.window=MainWindow(self.store);self.window.resize(1440,1000);self.window.show();self.app.processEvents()
    def tearDown(self):
        self.window.general_preferences.save(replace(self.window.general_preferences.config,autosave=True,ask_unsaved=False));self.window.close();self.window.deleteLater();self.app.processEvents();self.store.db.close();self.temp.cleanup()
    def apply(self,**values):
        self.window.general_preferences.save(replace(self.window.general_preferences.config,**values));self.window.general_settings_changed();self.app.processEvents()
    def test_blank_message_fonts_inheritance_and_live_editor_settings(self):
        from PySide6.QtGui import QRawFont,QFont
        from PySide6.QtCore import Qt
        from PySide6.QtTest import QTest
        page=self.window.current_page();self.assertEqual(page.message.toPlainText(),'');self.assertEqual(page.message.placeholderText(),'Compose message');self.assertEqual(page.message.blockCount(),1)
        self.assertEqual(QRawFont.fromFont(page.message.document().defaultFont()).familyName(),'JetBrains Mono')
        self.assertEqual(QRawFont.fromFont(self.window.tree.font()).familyName(),'Inter')
        self.apply(font_size=17,indent_count=2,two_pane=True,tab_icons=False,timeout_ms=1300)
        self.assertEqual(page.message.font().pixelSize(),17);self.assertEqual(page.grpc_timeout_ms.value(),1300);self.assertEqual(page.split.orientation(),Qt.Orientation.Horizontal);self.assertTrue(self.window.workbench.tabIcon(0).isNull())
        page.message.setFocus();QTest.keyClicks(page.message,'{');self.assertEqual(page.message.toPlainText(),'{}')
        QTest.keyClick(page.message,Qt.Key.Key_Return);self.assertEqual(page.message.toPlainText(),'{\n  \n}')
        page.message.clear();self.apply(close_brackets=False,close_quotes=False);QTest.keyClicks(page.message,'["');self.assertEqual(page.message.toPlainText(),'["')
        page.message.clear();self.assertEqual(page.message.placeholderText(),'Compose message');self.assertEqual(page.message.blockCount(),1)
        page.general_overrides.setChecked(True);page.grpc_timeout_ms.setValue(777);self.apply(timeout_ms=999);self.assertEqual(page.grpc_timeout_ms.value(),777)
    def test_local_autosave_off_manual_save_and_optional_close_question(self):
        from PySide6.QtTest import QTest
        from PySide6.QtWidgets import QMessageBox
        page=self.window.current_page();self.apply(autosave=False,ask_unsaved=True);before=self.store.all('SELECT data FROM local_drafts WHERE uid=?',(page._draft_id,))[0][0]
        page.message.setPlainText('{"changed":true}');QTest.qWait(350);self.assertEqual(self.store.all('SELECT data FROM local_drafts WHERE uid=?',(page._draft_id,))[0][0],before)
        with patch.object(QMessageBox,'question',return_value=QMessageBox.StandardButton.Cancel):self.assertFalse(self.window.confirm_close(page))
        self.assertTrue(self.window.flush_page(page,force=True));self.assertIn('changed',self.store.all('SELECT data FROM local_drafts WHERE uid=?',(page._draft_id,))[0][0])
        self.apply(autosave=True);page.message.setPlainText('{"saved":true}');QTest.qWait(350);self.assertIn('saved',self.store.all('SELECT data FROM local_drafts WHERE uid=?',(page._draft_id,))[0][0])
    def test_settings_language_persistence_documentation_and_preview_tabs(self):
        from PySide6.QtTest import QTest
        self.window.show_settings();general=self.window.settings_overlay.general
        general.controls['font_size'].setValue(15);QTest.qWait(400);self.assertEqual(GeneralPreferences(self.store).config.font_size,15)
        self.apply(language='Русский');self.app.processEvents();self.assertEqual(self.window.settings_overlay.navigation['General'].text(),'Основные')
        self.window.grab().save('build/general-russian.png');self.window.settings_overlay.dismiss()
        cid=self.store.insert('collections',{'name':'Docs','kind':'http'});cid2=self.store.insert('collections',{'name':'Other','kind':'http'});self.window.refresh_tree()
        page=self.window.open_item(self.window.tree_item('collection',cid));self.apply(documentation_editor='Markdown editor');page.description.source.setPlainText('# Local docs')
        self.apply(documentation_editor='Reqon editor');self.assertIn('Local docs',page.description.rich.toPlainText());self.assertEqual(self.store.entity('collection',cid)['description'],'# Local docs')
        first=self.window.workbench.count();self.window.open_item(self.window.tree_item('collection',cid2));self.assertEqual(self.window.workbench.count(),first+1)
        third=self.store.insert('collections',{'name':'Third','kind':'http'});fourth=self.store.insert('collections',{'name':'Fourth','kind':'http'});self.window.refresh_tree()
        self.window.open_item(self.window.tree_item('collection',third));self.assertEqual(self.window.workbench.count(),first+1)
        self.apply(new_tab=True);self.window.open_item(self.window.tree_item('collection',fourth));self.assertEqual(self.window.workbench.count(),first+2)
    def test_variable_completion_and_response_link_headers_and_format(self):
        page=self.window.new_request({'url':'http://localhost/a','auth_type':'Bearer Token','auth_value':'secret','headers':[['x-test','yes']]})
        page.url.setText('{{bas');page.url.setCursorPosition(5);page.url.completion.insert('base_url');self.assertEqual(page.url.text(),'{{base_url}}')
        self.apply(retain_headers=False);linked=self.window.open_response_link(page,'http://localhost/b');self.assertEqual(linked.auth_type.currentText(),'No Auth')
        self.apply(retain_headers=True);linked=self.window.open_response_link(page,'http://localhost/b');self.assertEqual(linked.auth_value.text(),'secret')
        result=dict(status=200,reason='OK',headers={'Content-Type':'text/plain'},body='{"a":1}',size=7,duration=1)
        page.show_response(result);self.assertEqual(page.response.editor.toPlainText(),'{"a":1}')
        self.apply(response_format='JSON');self.assertIn('\n',page.response.editor.toPlainText())


if __name__=='__main__':unittest.main()
