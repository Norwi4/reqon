import json
import os
from pathlib import Path
import ssl
import socket
import sys
import tempfile
import threading
import time
import unittest
from dataclasses import replace
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
from urllib.request import Request
from unittest.mock import patch

sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
os.environ.setdefault('QT_QPA_PLATFORM','offscreen')
import grpc
from proxy_support import ProxyConfig,ProxyRoute,ProxyPreferences,resolve_proxy,bypass_host,protect_secret
from request_transport import open_http,TLSOptions,expand
from grpc_support import discover_contract,invoke,channel_for
from proxy_fixture import Proxy
from reflection_fixture import ReflectionServer
from generate_tls_fixtures import generate

DIRECT=ProxyConfig(system=False,environment=False)


def custom(proxy,protocol='http',auth=False,**kwargs):
    return replace(DIRECT,custom=True,host='127.0.0.1',port=str(proxy.port),protocol=protocol,auth=auth,username='test',password='secret',**kwargs)


class ProxyTransportTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp=tempfile.TemporaryDirectory();cls.certs=Path(cls.temp.name);generate(cls.certs)
        cls.calls=[];owner=cls
        class Origin(BaseHTTPRequestHandler):
            def log_message(self,*args):pass
            def do_GET(self):
                owner.calls.append((self.path,dict(self.headers)))
                if self.path=='/redirect':
                    self.send_response(302);self.send_header('Location',f'http://localhost:{self.server.server_port}/final');self.end_headers()
                else:
                    self.send_response(200);self.send_header('Content-Length','2');self.end_headers();self.wfile.write(b'OK')
        cls.http=ThreadingHTTPServer(('127.0.0.1',0),Origin);cls.https=ThreadingHTTPServer(('127.0.0.1',0),Origin)
        context=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER);context.load_cert_chain(str(cls.certs/'server.pem'),str(cls.certs/'server.key'))
        cls.https.socket=context.wrap_socket(cls.https.socket,server_side=True)
        for server in (cls.http,cls.https):threading.Thread(target=server.serve_forever,kwargs={'poll_interval':.02},daemon=True).start()
    @classmethod
    def tearDownClass(cls):
        for server in (cls.http,cls.https):server.shutdown();server.server_close()
        cls.temp.cleanup()
    def fetch(self,url,config,**data):
        with open_http(Request(url,headers={'Authorization':'Bearer origin'}),dict(http_timeout=3,**data),{},config) as response:return response.read()

    def test_policy_precedence_bypass_and_no_global_mutation(self):
        env={'http_proxy':'http://env.test:3128','HTTPS_PROXY':'socks5h://secure.test:1080','NO_PROXY':'.direct.test'}
        system=ProxyRoute('http','os.test',8080)
        with patch.dict(os.environ,env,clear=True),patch('proxy_support.system_route',return_value=system) as os_proxy:
            original_environment=dict(os.environ)
            self.assertIsNone(resolve_proxy('https://target.test',DIRECT));os_proxy.assert_not_called()
            self.assertEqual(resolve_proxy('http://target.test').host,'env.test')
            self.assertEqual(resolve_proxy('https://target.test').protocol,'socks5h')
            self.assertIsNone(resolve_proxy('https://a.direct.test'))
            config=ProxyConfig(custom=True,host='custom.test',port='8888')
            self.assertEqual(resolve_proxy('https://a.direct.test',config).host,'custom.test')
            self.assertIsNone(resolve_proxy('https://target.test',replace(config,https=False)))
            self.assertEqual(resolve_proxy('https://target.test',ProxyConfig(environment=False)).host,'os.test')
            for host,port,rules in [('sub.example.test',443,'.example.test'),('foo.test',443,'*.test'),('10.1.2.3',443,'10.0.0.0/8'),('::1',80,'[::1]:80'),('intranet',80,'<local>')]:self.assertTrue(bypass_host(host,port,rules))
            self.assertFalse(bypass_host('evil-example.test',443,'example.test'))
            self.assertEqual(dict(os.environ),original_environment)
            with self.assertRaisesRegex(ValueError,'Invalid host'):resolve_proxy('http://target.test',replace(config,host=''))

    def test_http_forward_connect_auth_and_redirect_bypass(self):
        with Proxy(auth=True) as proxy:
            config=custom(proxy,auth=True)
            self.assertEqual(self.fetch(f'http://127.0.0.1:{self.http.server_port}/plain',config),b'OK')
            self.assertEqual(self.fetch(f'https://localhost:{self.https.server_port}/tls',config,http_ca_file=str(self.certs/'ca.pem')),b'OK')
            self.assertEqual([call[0] for call in proxy.calls],['GET','CONNECT'])
            self.assertTrue(all(call[2] for call in proxy.calls))
            self.assertNotIn('Proxy-Authorization',self.calls[-1][1]);self.assertEqual(self.calls[-1][1]['Authorization'],'Bearer origin')
            self.fetch(f'http://127.0.0.1:{self.http.server_port}/redirect',replace(config,bypass='localhost'))
            self.assertEqual(len(proxy.calls),3);self.assertEqual(self.calls[-1][0],'/final')
            self.assertNotIn('Authorization',self.calls[-1][1]);self.assertNotIn('Proxy-Authorization',self.calls[-1][1])
            with self.assertRaisesRegex(Exception,'407'):self.fetch(f'http://localhost:{self.http.server_port}/',replace(config,password='wrong'))

    def test_socks_protocols_and_dns_modes(self):
        for protocol in ('socks5','socks5h','socks4','socks4a'):
            with self.subTest(protocol=protocol),Proxy(protocol,auth=protocol.startswith('socks5')) as proxy:
                config=custom(proxy,protocol,auth=True)
                host='origin.test' if protocol in ('socks5h','socks4a') else '127.0.0.1'
                self.assertEqual(self.fetch(f'http://{host}:{self.http.server_port}/socks',config),b'OK')
                self.assertEqual(proxy.calls[-1][3],3 if protocol in ('socks5h','socks4a') else 1)
                self.assertEqual(proxy.calls[-1][4],'test')
                self.assertEqual(self.fetch(f'https://localhost:{self.https.server_port}/socks',config,http_ca_file=str(self.certs/'ca.pem')),b'OK')

    def test_direct_ignores_external_proxy_and_bad_custom_does_not_fall_back(self):
        url=f'http://127.0.0.1:{self.http.server_port}/direct'
        with patch.dict(os.environ,{'http_proxy':'http://127.0.0.1:1','no_proxy':''},clear=True):self.assertEqual(self.fetch(url,DIRECT),b'OK')
        count=len(self.calls)
        with self.assertRaises(Exception):self.fetch(url,replace(DIRECT,custom=True,host='127.0.0.1',port='1'))
        self.assertEqual(len(self.calls),count)

    def test_grpc_reflection_and_invoke_through_http_and_socks(self):
        credentials=grpc.ssl_server_credentials([((self.certs/'server.key').read_bytes(),(self.certs/'server.pem').read_bytes())])
        for protocol,tls,verify in [('http',False,False),('http',True,True),('http',True,False),('socks5h',True,True)]:
            with self.subTest(protocol=protocol,tls=tls,verify=verify),Proxy(protocol,auth=True) as proxy:
                server=ReflectionServer('proxytest',credentials if tls else None)
                try:
                    config=custom(proxy,protocol,True)
                    data=dict(kind='grpc',server=server.address,tls=tls,tls_verify=verify,tls_ca_file=str(self.certs/'ca.pem') if verify else '',
                        service='proxytest.Customers',rpc_method='GetCustomer',message='{"name":"through proxy"}',grpc_timeout_ms=4000,_proxy_config=config)
                    methods,_=discover_contract(server.address,TLSOptions.from_data(data));self.assertIn(('proxytest.Customers','GetCustomer'),methods)
                    self.assertEqual(json.loads(invoke(data,{},expand)['body']),{'name':'through proxy'})
                    self.assertGreaterEqual(len(proxy.calls),2)
                finally:server.close()

    def test_grpc_proxy_preserves_tls_verification_and_mutual_tls(self):
        credentials=grpc.ssl_server_credentials([((self.certs/'server.key').read_bytes(),(self.certs/'server.pem').read_bytes())],
            root_certificates=(self.certs/'ca.pem').read_bytes(),require_client_auth=True)
        server=ReflectionServer('proxymtls',credentials)
        try:
            with Proxy() as proxy:
                config=custom(proxy)
                options=TLSOptions(True,str(self.certs/'ca.pem'),str(self.certs/'client.pem'),str(self.certs/'client.key'),True,proxy_config=config)
                with channel_for(server.address,options) as channel:grpc.channel_ready_future(channel).result(3)
                for invalid in (replace(options,ca_file=''),replace(options,server_name='wrong.test')):
                    with channel_for(server.address,invalid) as channel:
                        with self.assertRaises(grpc.FutureTimeoutError):grpc.channel_ready_future(channel).result(.7)
                        self.assertIn('CERTIFICATE_VERIFY_FAILED',channel.last_error or '')
        finally:server.close()

    def test_cancel_during_proxy_connect_closes_the_late_socket(self):
        from tls_channel import TransportChannel
        entered=threading.Event();release=threading.Event();finished=threading.Event();sockets=[]
        def delayed_connect(*args):
            entered.set();release.wait(3);pair=socket.socketpair();sockets.extend(pair);finished.set();return pair[0]
        with patch('proxy_support.open_proxy_socket',side_effect=delayed_connect):
            channel=TransportChannel('localhost:12345',TLSOptions(),proxy=ProxyRoute('http','localhost',1234))
            try:
                future=channel.unary_unary('/demo.API/Test').future(b'',timeout=3)
                self.assertTrue(entered.wait(2));start=time.monotonic();channel.close();self.assertLess(time.monotonic()-start,1.5)
                release.set();self.assertTrue(finished.wait(2))
                deadline=time.monotonic()+1
                while sockets[0].fileno()!=-1 and time.monotonic()<deadline:time.sleep(.01)
                self.assertEqual(sockets[0].fileno(),-1)
            finally:
                release.set();channel.close()
                for connection in sockets:connection.close()


class ProxySettingsUI(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        from PySide6.QtWidgets import QApplication
        from ui_theme import STYLE
        cls.app=QApplication.instance() or QApplication([]);cls.app.setStyle('Fusion');cls.app.setStyleSheet(STYLE)
    def setUp(self):
        from main import MainWindow,Store
        self.temp=tempfile.TemporaryDirectory();self.store=Store(self.temp.name);self.window=MainWindow(self.store)
        self.window.resize(1440,1000);self.window.show();self.app.processEvents()
    def tearDown(self):
        self.window.close();self.window.deleteLater();self.app.processEvents();self.store.db.close();self.temp.cleanup()
    def test_overlay_autosaves_without_another_window_and_bundles_fonts(self):
        from PySide6.QtGui import QFontInfo,QRawFont
        from PySide6.QtWidgets import QApplication
        from PySide6.QtCore import Qt
        from PySide6.QtTest import QTest
        windows={id(w) for w in QApplication.topLevelWidgets() if w.isVisible()}
        self.window.settings_button.click();self.app.processEvents();overlay=self.window.settings_overlay
        self.assertFalse(overlay.isWindow());self.assertEqual({id(w) for w in QApplication.topLevelWidgets() if w.isVisible()},windows)
        self.assertEqual([key for key,button in overlay.navigation.items() if button.isEnabled()],['General','Proxy'])
        overlay.navigation['Proxy'].click()
        self.assertFalse(self.window.workbench.isEnabled())
        overlay.custom.setChecked(True);overlay.host.setText('127.0.0.1');overlay.port.setText('3128');overlay.auth.setChecked(True)
        overlay.username.setText('test');overlay.password.setText('local-secret');QTest.qWait(400)
        restored=ProxyPreferences(self.store);self.assertEqual(restored.config.password,'local-secret');self.assertEqual(restored.config.port,'3128')
        stored=self.store.all("SELECT data FROM workspace_state WHERE key='proxy'")[0][0]
        if sys.platform=='win32':self.assertNotIn('local-secret',stored);self.assertIn('dpapi:',stored)
        self.assertEqual(QFontInfo(overlay.host.font()).family(),'Inter')
        page=self.window.current_page();self.assertEqual(QFontInfo(page.message.font()).family(),'JetBrains Mono')
        self.assertEqual(page.message.font().pixelSize(),13);self.assertTrue(QRawFont.fromFont(page.message.font()).supportsCharacter(0x0416))
        self.assertTrue(QRawFont.fromFont(overlay.host.font()).supportsCharacter(0x0416))
        self.app.processEvents();Path('build').mkdir(exist_ok=True);self.window.grab().save('build/proxy-settings-overlay.png')
        overlay.host.setFocus();QTest.keyClick(overlay.host,Qt.Key.Key_Escape);self.assertFalse(overlay.isVisible());self.assertTrue(self.window.workbench.isEnabled())
        self.window.show_settings();self.assertEqual(overlay.host.text(),'127.0.0.1')
        self.assertNotIn('_proxy_config',page.collect())
    def test_unreadable_password_blocks_requests_until_preferences_are_repaired(self):
        prefs=ProxyPreferences(self.store);prefs.save(replace(DIRECT,custom=True,host='127.0.0.1',port='3128',password='secret'))
        with patch('proxy_support.protect_secret',side_effect=OSError('cannot decrypt')):broken=ProxyPreferences(self.store)
        self.assertTrue(broken.load_error)
        with self.assertRaisesRegex(ValueError,'Re-enter'):resolve_proxy('https://example.test',broken.config)
        broken.save(DIRECT);self.assertIsNone(resolve_proxy('https://example.test',broken.config))


if __name__=='__main__':unittest.main()
