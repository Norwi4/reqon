"""Run existing transport contracts against the actual Rust executable."""
import functools
import json
import os
from pathlib import Path
import sys
import threading
import time
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')
import test_general_settings as general
import test_http_editor as editor
import test_grpc_settings as settings
import test_proxy_settings as proxy_tests
import native_request


class NativeGeneralTests(general.GeneralNetworkTests):
    def setUp(self):
        self.addCleanup(patch.stopall)
        patch.object(general, 'send_http', functools.partial(general.send_http, native=True)).start()
        patch.object(general, 'invoke', native_request.invoke).start()

    def test_http2_auto_keylog_and_disabled_accept_use_negotiated_protocol(self):
        send=general.send_http
        config=self.config
        with patch.object(general, 'send_http', side_effect=lambda data,*args,**kwargs:send(dict(data,disabled_headers=['Accept']),*args,**kwargs)), \
                patch.object(self,'config',side_effect=lambda **kwargs:config(tls_keylog=True,**kwargs)):
            super().test_http2_and_auto_negotiate_real_h2_tls_server()

    def test_truncated_content_length_over_tls_is_an_error(self):
        import ssl
        from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
        class Handler(BaseHTTPRequestHandler):
            def log_message(self,*args):pass
            def do_GET(self):
                self.send_response(200);self.send_header('Content-Length','100');self.end_headers();self.wfile.write(b'short')
        server=ThreadingHTTPServer(('127.0.0.1',0),Handler)
        context=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER);context.load_cert_chain(str(self.root/'server.pem'),str(self.root/'server.key'))
        server.socket=context.wrap_socket(server.socket,server_side=True)
        threading.Thread(target=server.serve_forever,kwargs={'poll_interval':.02},daemon=True).start()
        try:
            data=general.effective_request(dict(url=f'https://localhost:{server.server_port}/'),self.config(),self.root)
            with self.assertRaises(native_request.NativeError):general.send_http(data,{},general.DIRECT,None,general.Cancellation())
        finally:server.shutdown();server.server_close()


class NativeProxyTests(proxy_tests.ProxyTransportTests):
    def setUp(self):
        self.addCleanup(patch.stopall)
        patch.object(proxy_tests, 'invoke', native_request.invoke).start()

    def fetch(self,url,config,**data):
        from http_runtime import send_http
        from general_settings import GeneralConfig,effective_request
        from grpc_support import Cancellation
        request=dict(url=url,headers=[['Authorization','Bearer origin']],http_timeout=3,general_overrides=True,**data)
        result=send_http(effective_request(request,GeneralConfig(),self.certs),{},config,None,Cancellation(),native=True)
        if result['status']>=400:raise ValueError(str(result['status']))
        return result['body'].encode()


class NativeEditorTests(editor.EditorNetwork):
    def setUp(self):
        super().setUp()
        self.addCleanup(patch.stopall)
        patch.object(editor, 'send_http', functools.partial(editor.send_http, native=True)).start()


class NativeGrpcTests(settings.SettingsTransportTests):
    def setUp(self):
        self.addCleanup(patch.stopall)
        patch.object(settings, 'invoke', native_request.invoke).start()


class NativeLifecycleTests(unittest.TestCase):
    def test_reflection_cache_isolated_by_metadata_and_refresh(self):
        from reflection_fixture import ReflectionServer
        from request_transport import expand
        native_request.shutdown_idle();server=ReflectionServer('cached')
        data=dict(server=server.address,service='cached.Customers',rpc_method='GetCustomer',message='{"name":"cached"}')
        try:
            native_request.invoke(data,{},expand);first=len(server.calls)
            self.assertGreater(first,0)
            for _ in range(3):self.assertEqual(json.loads(native_request.invoke(data,{},expand)['body']),{'name':'cached'})
            self.assertEqual(len(server.calls),first)
            native_request.invoke(dict(data,metadata=[['x-tenant','other']]),{},expand)
            self.assertGreater(len(server.calls),first)
            before=len(server.calls);native_request.shutdown_idle()
            native_request.invoke(data,{},expand);self.assertGreater(len(server.calls),before)
        finally:server.close();native_request.shutdown_idle()

    def test_released_wrapper_cannot_cancel_another_lease(self):
        native_request.shutdown_idle()
        old=native_request.RequestProcess()
        with old:
            old.hello();old.completed=True
        with native_request.RequestProcess() as new:
            self.assertEqual(old.process.pid,new.process.pid)
            old.close()
            self.assertIsNone(new.process.poll())
            new.hello()

    def test_unavailable_server_discards_cached_contract_before_restart(self):
        from reflection_fixture import ReflectionServer
        from request_transport import expand
        native_request.shutdown_idle();server=ReflectionServer('restartcache')
        data=dict(server=server.address,service='restartcache.Customers',rpc_method='GetCustomer',message='{}',grpc_timeout_ms=300)
        try:
            native_request.invoke(data,{},expand)
            server.close()
            with self.assertRaises(Exception):native_request.invoke(data,{},expand)
            self.assertEqual(len(native_request._idle),0)
        finally:server.close();native_request.shutdown_idle()

    def test_tls_cache_rechecks_changed_certificate_content(self):
        import tempfile
        from test_tls_snippets import credentials,secure_data,FIXTURES
        from reflection_fixture import ReflectionServer
        from request_transport import expand
        native_request.shutdown_idle();server=ReflectionServer('secure',credentials())
        try:
            with tempfile.TemporaryDirectory() as directory:
                ca=Path(directory)/'ca.pem';ca.write_bytes((FIXTURES/'ca.pem').read_bytes())
                data=dict(secure_data(server),tls_ca_file=str(ca))
                native_request.invoke(data,{},expand);before=len(server.calls)
                native_request.invoke(data,{},expand);self.assertEqual(len(server.calls),before)
                ca.write_text('invalid certificate',encoding='ascii')
                with self.assertRaises(Exception):native_request.invoke(data,{},expand)
        finally:server.close();native_request.shutdown_idle()

    def test_warm_process_deadline_is_not_a_user_cancellation(self):
        import grpc
        from error_fixture import ErrorServer
        from request_transport import expand
        server=ErrorServer()
        try:
            for _ in range(4):
                native_request.invoke(server.data('Echo'),{},expand)
                with self.assertRaises(native_request.GrpcError) as failure:
                    native_request.invoke(dict(server.data('Slow'),grpc_timeout_ms=80),{},expand)
                self.assertEqual(failure.exception.code(),grpc.StatusCode.DEADLINE_EXCEEDED)
        finally:server.close();native_request.shutdown_idle()

    def test_reflection_has_its_own_budget_when_rpc_timeout_is_unlimited(self):
        import grpc
        from concurrent.futures import ThreadPoolExecutor
        from request_transport import expand
        entered=threading.Event()
        def stalled(requests,context):
            entered.set()
            while context.is_active():time.sleep(.01)
            yield b''
        server=grpc.server(ThreadPoolExecutor(max_workers=2))
        handler=grpc.stream_stream_rpc_method_handler(stalled,request_deserializer=lambda value:value,response_serializer=lambda value:value)
        server.add_generic_rpc_handlers((grpc.method_handlers_generic_handler('grpc.reflection.v1.ServerReflection',{'ServerReflectionInfo':handler}),))
        port=server.add_insecure_port('127.0.0.1:0');server.start()
        try:
            started=time.monotonic()
            with self.assertRaises(native_request.GrpcError) as failure:
                native_request.invoke(dict(server=f'127.0.0.1:{port}',service='test.API',rpc_method='Echo',grpc_timeout_ms=0),{},expand)
            self.assertTrue(entered.is_set())
            self.assertEqual(failure.exception.code(),grpc.StatusCode.DEADLINE_EXCEEDED)
            self.assertIn('reflection',failure.exception.details())
            self.assertLess(time.monotonic()-started,13)
            entered.clear()
            with native_request.RequestProcess() as process:
                process.hello();process.send(dict(kind='grpc',url=f'http://127.0.0.1:{port}',method='test.API/Echo',payload={},timeout_ms=0,transport=dict(allow_plaintext_remote=True)))
                self.assertTrue(entered.wait(3));process.process.stdin.close()
                self.assertEqual(process.process.wait(timeout=3),0)
        finally:server.stop(0).wait(5)

    def test_process_reuse_and_cancellation_are_isolated_between_requests(self):
        from grpc_support import Cancellation
        from request_transport import expand
        native_request.shutdown_idle()
        server=settings.StreamingServer();cancellation=Cancellation();errors=[]
        try:
            with patch.object(native_request.subprocess,'Popen',wraps=native_request.subprocess.Popen) as spawn:
                for _ in range(2):self.assertEqual(native_request.invoke(server.data,{},expand)['status'],0)
                self.assertEqual(spawn.call_count,1)
            def waiting():
                try:native_request.invoke(dict(server.data,rpc_method='Wait'),{},expand,cancellation)
                except Exception as error:errors.append(error)
            thread=threading.Thread(target=waiting);thread.start();self.assertTrue(server.entered.wait(3))
            reply=native_request.invoke(dict(server.data,message='{"name":"independent"}'),{},expand)
            self.assertEqual(json.loads(reply['body']),{'name':'independent'})
            cancellation.cancel();thread.join(3)
            self.assertFalse(thread.is_alive());self.assertTrue(errors)
            self.assertEqual(native_request.invoke(server.data,{},expand)['status'],0)
        finally:
            cancellation.cancel();server.close();native_request.shutdown_idle()

    def test_version_mismatch_and_crash_are_not_successes(self):
        with patch.object(native_request, 'RUNNER_VERSION', 'invalid'):
            with native_request.RequestProcess() as process:
                with self.assertRaisesRegex(ValueError, 'version mismatch'): process.hello()
        with native_request.RequestProcess() as process:
            process.hello(); process.process.kill(); process.process.wait(5)
            with self.assertRaisesRegex(ValueError, 'stopped before'): process.read()

    def test_cancel_before_start_does_not_leave_a_process(self):
        from grpc_support import Cancellation
        cancellation = Cancellation(); cancellation.cancel()
        process = native_request.RequestProcess()
        with self.assertRaisesRegex(ValueError, 'cancelled'):
            with cancellation.use(process): process.hello()
        self.assertIsNotNone(process.process.poll())


if __name__ == '__main__': unittest.main()
