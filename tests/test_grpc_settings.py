import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from concurrent import futures
from unittest.mock import patch
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import grpc
from google.protobuf import descriptor_pb2,descriptor_pool,message_factory
from grpc_reflection.v1alpha import reflection
from PySide6.QtTest import QTest
from PySide6.QtWidgets import QApplication,QFileDialog
from code_snippets import command_groups
from grpc_support import channel_for,discover_contract,example_message,invoke,Cancellation,rpc_error
from main import MainWindow,Store
from request_transport import TLSOptions,expand
from ui_pages import GrpcPage,Job
from ui_theme import STYLE
from test_tls_snippets import credentials,FIXTURES,secure_data
from reflection_fixture import ReflectionServer


class StreamingServer:
    def __init__(self):
        descriptor=descriptor_pb2.FileDescriptorProto(name='settings_tests.proto',package='settings',syntax='proto3')
        message=descriptor.message_type.add(name='Message'); message.field.add(name='name',number=1,label=1,type=9)
        service=descriptor.service.add(name='API')
        for name,client,server in [('Echo',False,False),('Wait',False,False),('Download',False,True),('Upload',True,False),('Chat',True,True)]:
            service.method.add(name=name,input_type='.settings.Message',output_type='.settings.Message',client_streaming=client,server_streaming=server)
        pool=descriptor_pool.DescriptorPool();pool.Add(descriptor);cls=message_factory.GetMessageClass(pool.FindMessageTypeByName('settings.Message'))
        self.entered=threading.Event(); self.security=[]
        def echo(request,context):
            self.security.append(context.auth_context()['transport_security_type'])
            context.send_initial_metadata((('x-initial','present'),));context.set_trailing_metadata((('x-trailer','present'),))
            return request
        def wait(request,context):
            self.entered.set()
            while context.is_active():time.sleep(0.01)
            return request
        def download(request,context):
            for i in range(4):yield cls(name=request.name+str(i))
        def upload(requests,context):return cls(name=''.join(request.name for request in requests))
        def chat(requests,context):
            for request in requests:yield echo(request,context) if not self.security else request
        handlers={}
        for name,handler,factory in [('Echo',echo,grpc.unary_unary_rpc_method_handler),('Wait',wait,grpc.unary_unary_rpc_method_handler),
                ('Download',download,grpc.unary_stream_rpc_method_handler),('Upload',upload,grpc.stream_unary_rpc_method_handler),('Chat',chat,grpc.stream_stream_rpc_method_handler)]:
            handlers[name]=factory(handler,request_deserializer=cls.FromString,response_serializer=lambda response:response.SerializeToString())
        self.server=grpc.server(futures.ThreadPoolExecutor(max_workers=8),options=[('grpc.max_receive_message_length',-1)])
        self.server.add_generic_rpc_handlers((grpc.method_handlers_generic_handler('settings.API',handlers),))
        reflection.enable_server_reflection(('settings.API',reflection.SERVICE_NAME),self.server,pool=pool)
        self.port=self.server.add_secure_port('127.0.0.1:0',credentials());self.server.start()
        self.data={'kind':'grpc','server':f'127.0.0.1:{self.port}','tls':True,'tls_verify':False,
            'service':'settings.API','rpc_method':'Echo','message':'{}'}
    def close(self):self.server.stop(0).wait(5)


class SettingsTransportTests(unittest.TestCase):
    def tearDown(self):
        self.assertFalse([thread for thread in threading.enumerate() if thread.name=='Getman TLS relay'])

    def test_new_and_legacy_requests_default_to_off_and_explicit_on_is_retained(self):
        self.assertFalse(TLSOptions(True).verify_server)
        self.assertFalse(TLSOptions.from_data({'tls':True}).verify_server)
        self.assertTrue(TLSOptions.from_data({'tls':True,'tls_verify':True}).verify_server)
        server=ReflectionServer('secure',credentials())
        try:
            data=secure_data(server);data.pop('tls_verify');data['tls_ca_file']=''
            self.assertEqual(invoke(data,{},expand)['status'],0)
            args=[value for group in command_groups(data)[1] for value in group]
            self.assertIn('-insecure',args);self.assertNotIn('-plaintext',args)
        finally:server.close()

    def test_verification_off_uses_tls_for_reflection_and_mtls_without_trusting_ca(self):
        for mutual in (False,True):
            server=ReflectionServer('secure',credentials(mutual))
            try:
                data=dict(secure_data(server,mutual),tls_verify=False,tls_ca_file='missing-ignored-ca.pem')
                for host in ('localhost','127.0.0.1'):
                    data['server']=f'{host}:{server.port}'
                    methods,contract=discover_contract(data['server'],TLSOptions.from_data(data))
                    self.assertIn(('secure.Customers','GetCustomer'),methods)
                    self.assertEqual(json.loads(invoke(data,{},expand)['body']),{'name':'TLS request'})
                    with patch('grpc_support.channel_for',side_effect=AssertionError('Offline template must not open a channel')):
                        self.assertIn('name',json.loads(example_message('',TLSOptions.from_data(data),'','secure.Customers','GetCustomer',schema_bytes=contract)))
                # The same request still fails after the user re-enables validation.
                data.update(tls_verify=True,tls_ca_file='',grpc_timeout_ms=1500)
                with self.assertRaises(grpc.RpcError) as failure:invoke(data,{},expand)
                self.assertIn('Enable server certificate verification',rpc_error(failure.exception))
            finally:server.close()

    def test_server_name_override_and_grpcurl_insecure_match_settings(self):
        server=ReflectionServer('secure',credentials())
        try:
            data=dict(secure_data(server),server=f'127.0.0.1:{server.port}',tls_server_name='localhost')
            self.assertEqual(invoke(data,{},expand)['status'],0)
            data.update(tls_verify=False,tls_ca_file='nonexistent',grpc_timeout_ms=4500,grpc_max_response_mb=0,grpc_emit_defaults=False)
            _,groups=command_groups(data); args=[value for group in groups for value in group]
            self.assertIn('-insecure',args); self.assertNotIn('-plaintext',args); self.assertNotIn('-cacert',args);self.assertNotIn('-emit-defaults',args)
            self.assertIn(['-authority','localhost'],groups);self.assertIn(['-max-time','4.5'],groups)
            self.assertIn(['-max-msg-sz','2147483647'],groups)
            binary=os.environ.get('GETMAN_GRPCURL') or str(Path(__file__).resolve().parents[1]/'build/test-tools/grpcurl.exe')
            if Path(binary).exists():
                args[0]=binary; result=subprocess.run(args,capture_output=True,text=True,timeout=15)
                self.assertEqual(result.returncode,0,result.stderr);self.assertEqual(json.loads(result.stdout),{'name':'TLS request'})
        finally:server.close()

    def test_streaming_defaults_message_limits_metadata_and_deadline(self):
        server=StreamingServer()
        try:
            for emit,expected in ((True,{'name':''}),(False,{})):
                result=invoke(dict(server.data,grpc_emit_defaults=emit),{},expand)
                self.assertEqual(json.loads(result['body']),expected)
                self.assertIn(('x-initial','present'),result['headers']);self.assertIn(('x-trailer','present'),result['trailers'])
            for method,payload,expected in [('Download',{'name':'chunk'},[{'name':'chunk'+str(i)} for i in range(4)]),
                    ('Upload',[{'name':'a'},{'name':'b'}],{'name':'ab'}),('Chat',[{'name':'a'},{'name':'b'}],[{'name':'a'},{'name':'b'}])]:
                result=invoke(dict(server.data,rpc_method=method,message=json.dumps(payload)),{},expand)
                self.assertEqual(json.loads(result['body']),expected)
            big='x'*(1200*1024)
            data=dict(server.data,message=json.dumps({'name':big}),grpc_max_response_mb=1)
            with self.assertRaises(grpc.RpcError) as failure:invoke(data,{},expand)
            self.assertEqual(failure.exception.code(),grpc.StatusCode.RESOURCE_EXHAUSTED)
            self.assertEqual(json.loads(invoke(dict(data,grpc_max_response_mb=0),{},expand)['body'])['name'],big)
            self.assertTrue(all(value==[b'ssl'] for value in server.security))
            started=time.monotonic()
            with self.assertRaises(grpc.RpcError) as failure:invoke(dict(server.data,rpc_method='Wait',grpc_timeout_ms=200),{},expand)
            self.assertEqual(failure.exception.code(),grpc.StatusCode.DEADLINE_EXCEEDED)
            self.assertLess(time.monotonic()-started,2)
        finally:server.close()

    def test_unlimited_request_can_be_cancelled_and_relay_is_closed(self):
        server=StreamingServer(); cancellation=Cancellation(); failures=[]
        def call():
            try:invoke(dict(server.data,rpc_method='Wait',grpc_timeout_ms=0),{},expand,cancellation)
            except Exception as error:failures.append(error)
        thread=threading.Thread(target=call)
        try:
            thread.start();self.assertTrue(server.entered.wait(5));cancellation.cancel();thread.join(5)
            self.assertFalse(thread.is_alive());self.assertTrue(failures)
        finally:cancellation.cancel();server.close();thread.join(5)


class SettingsUITests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app=QApplication.instance() or QApplication([]);cls.app.setStyle('Fusion');cls.app.setStyleSheet(STYLE)
    def wait_for(self,predicate):
        deadline=time.monotonic()+15
        while not predicate() and time.monotonic()<deadline:QTest.qWait(20)
        self.assertTrue(predicate())
    def test_settings_autosave_export_import_and_restore(self):
        with tempfile.TemporaryDirectory() as directory:
            store=Store(directory);window=MainWindow(store);window.show();page=window.current_page()
            try:
                # New requests inherit the 50 MB response limit from General.
                self.assertFalse(page.tls_verify.isChecked());self.assertEqual(page.grpc_max_response_mb.value(),50);self.assertEqual(page.grpc_timeout_ms.value(),0)
                page.tls_verify.click();page.tls_server_name.setText('localhost');page.grpc_emit_defaults.click()
                page.grpc_max_response_mb.setValue(27);page.grpc_timeout_ms.setValue(1500)
                cid=store.insert('collections',{'name':'Settings','kind':'grpc'});page.collection_id=cid
                self.wait_for(lambda:page.save_state.text()=='Saved locally')
                expected={key:page.collect()[key] for key in ('tls_verify','tls_server_name','grpc_emit_defaults','grpc_max_response_mb','grpc_timeout_ms')}
                target=str(Path(directory)/'settings.json')
                with patch.object(QFileDialog,'getSaveFileName',return_value=(target,'')):window.export_collection(cid)
                with patch.object(QFileDialog,'getOpenFileName',return_value=(target,'')):window.import_collection()
                restored=json.loads(store.all('SELECT data FROM requests ORDER BY id DESC LIMIT 1')[0][0])
                self.assertEqual({key:restored[key] for key in expected},expected)
                rebuilt=GrpcPage(restored)
                self.assertEqual({key:rebuilt.collect()[key] for key in expected},expected);rebuilt.deleteLater()
            finally:window.close();window.deleteLater();self.app.processEvents();store.db.close()
    def test_cancel_button_and_window_close_stop_unlimited_tls_requests(self):
        server=StreamingServer()
        try:
            with tempfile.TemporaryDirectory() as directory:
                store=Store(directory);window=MainWindow(store);window.show()
                try:
                    page=window.new_grpc_request(dict(server.data,rpc_method='Wait'));page.send_button.click()
                    self.wait_for(server.entered.is_set);self.assertEqual(page.send_button.text(),'Cancel');page.send_button.click()
                    self.wait_for(lambda:not window.workers);self.assertEqual(page.response_body.toPlainText(),'Request cancelled.')
                    server.entered.clear();page.send_button.click();self.wait_for(server.entered.is_set);window.close()
                    self.wait_for(lambda:not window.workers and not Job.running);self.assertFalse(window.isVisible())
                finally:window.close();window.deleteLater();self.app.processEvents();store.db.close()
        finally:server.close()


if __name__=='__main__':unittest.main(verbosity=2)
