import json
import sys
import tempfile
import threading
import time
import unittest
from concurrent import futures
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path
from unittest.mock import patch
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import grpc
from google.protobuf import descriptor_pb2,descriptor_pool,message_factory
from grpc_reflection.v1alpha import reflection
from PySide6.QtCore import Qt,QThread
from PySide6.QtWidgets import QApplication,QFileDialog,QPushButton
from PySide6.QtTest import QTest
from main import MainWindow,Store
from ui_pages import GrpcPage,RequestPage
from ui_components import KeyValueTable
from ui_theme import STYLE


class UIIntegration(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app=QApplication.instance() or QApplication([])
        cls.app.setStyle('Fusion'); cls.app.setStyleSheet(STYLE)
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory(); self.store=Store(self.temp.name); self.window=MainWindow(self.store)
        self.window.show(); self.app.processEvents()
    def tearDown(self):
        for index in range(self.window.workbench.count()):
            page=self.window.workbench.widget(index); page._saved=page.collect()
        self.window.close(); self.window.deleteLater(); self.app.processEvents()
        self.store.db.close(); self.temp.cleanup()
    def wait_for(self,predicate):
        deadline=time.monotonic()+8
        while not predicate() and time.monotonic()<deadline:QTest.qWait(20)
        self.assertTrue(predicate())
    def test_protocol_menu_and_local_collection_roundtrip(self):
        page=self.window.current_page(); page.server.setText('localhost:50051'); page.message.setPlainText('{"hello":"world"}')
        page.protocol.menu().actions()[0].trigger()
        http=self.window.current_page(); self.assertIsInstance(http,RequestPage)
        self.assertEqual(http.url.text(),'localhost:50051'); self.assertEqual(http.body.toPlainText(),'{"hello":"world"}')
        http.protocol.menu().actions()[1].trigger(); page=self.window.current_page(); self.assertIsInstance(page,GrpcPage)
        cid=self.store.insert('collections',{'name':'Local test','kind':'grpc'})
        page.collection_id=cid; self.window.save_request(page)
        target=str(Path(self.temp.name)/'collection.json')
        with patch.object(QFileDialog,'getSaveFileName',return_value=(target,'')):self.window.export_collection(cid)
        with patch.object(QFileDialog,'getOpenFileName',return_value=(target,'')):self.window.import_collection()
        self.assertEqual(len(self.store.all("SELECT id FROM collections WHERE name='Local test'")),2)
    def test_table_typing_checkbox_and_reload(self):
        page=self.window.current_page(); page.tabs.setCurrentIndex(3); self.app.processEvents()
        table=page.metadata.table
        table.setCurrentCell(0,1); table.editItem(table.item(0,1)); self.app.processEvents()
        editor=self.app.focusWidget(); QTest.keyClicks(editor,'authorization'); QTest.keyClick(editor,Qt.Key.Key_Tab)
        self.assertEqual(table.item(0,1).text(),'authorization')
        self.assertEqual(table.rowCount(),2)
        table.item(0,2).setText('Bearer test'); table.item(0,0).setCheckState(Qt.CheckState.Unchecked)
        self.assertEqual(page.collect()['metadata'],[])
        restored=GrpcPage(page.collect()); self.assertEqual(restored.metadata.all_rows()[0],['authorization','Bearer test','',False]); restored.deleteLater()
    def test_http_request_through_send_button(self):
        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                self.send_response(200); self.send_header('Content-Type','application/json'); self.end_headers()
                self.wfile.write(json.dumps({'path':self.path}).encode())
            def log_message(self,*args):pass
        server=HTTPServer(('127.0.0.1',0),Handler); thread=threading.Thread(target=server.serve_forever,daemon=True); thread.start()
        try:
            page=self.window.new_request(); page.url.setText(f'http://127.0.0.1:{server.server_port}/customers')
            page.params.set_rows([['page','2',''],['ignored','no','',False]])
            page.send_button.click(); self.assertTrue(page.busy)
            self.wait_for(lambda:not self.window.workers)
            self.assertIn('/customers?page=2',page.response_body.toPlainText())
            self.assertIn('200 OK',page.status.text()); self.assertTrue(page.send_button.isEnabled())
            self.assertGreater(len(page.response.headers.all_rows()),0)
        finally:server.shutdown(); server.server_close()
    def test_grpc_metadata_trailers_and_schema(self):
        descriptor=descriptor_pb2.FileDescriptorProto(name='ui_echo.proto',package='ui',syntax='proto3')
        message=descriptor.message_type.add(name='Text'); message.field.add(name='value',number=1,label=1,type=9)
        service=descriptor.service.add(name='Echo'); service.method.add(name='Say',input_type='.ui.Text',output_type='.ui.Text')
        pool=descriptor_pool.Default(); pool.Add(descriptor); Text=message_factory.GetMessageClass(pool.FindMessageTypeByName('ui.Text'))
        def echo(request,context):
            context.send_initial_metadata((('source','test-server'),)); context.set_trailing_metadata((('request-id','42'),))
            return Text(value=request.value+'!')
        server=grpc.server(futures.ThreadPoolExecutor(max_workers=2))
        server.add_generic_rpc_handlers((grpc.method_handlers_generic_handler('ui.Echo',{'Say':grpc.unary_unary_rpc_method_handler(echo,request_deserializer=Text.FromString,response_serializer=lambda x:x.SerializeToString())}),))
        reflection.enable_server_reflection(('ui.Echo',reflection.SERVICE_NAME),server)
        port=server.add_insecure_port('127.0.0.1:0'); server.start()
        try:
            page=self.window.current_page(); page.server.setText(f'127.0.0.1:{port}'); page.service.setText('ui.Echo'); page.rpc_method.setText('Say')
            threads=[]; original=self.window.request_finished
            def record_thread(*args):
                threads.append(QThread.currentThread()==self.window.thread()); original(*args)
            self.window.request_finished=record_thread
            page.message.setPlainText('{"value":"hello"}'); page.send_button.click()
            self.wait_for(lambda:not self.window.workers)
            self.assertEqual(threads,[True])
            self.assertIn('hello!',page.response_body.toPlainText())
            self.assertEqual(page.response.headers.active_rows()[0][:2],['source','test-server'])
            self.assertEqual(page.response.trailers.active_rows()[0][:2],['request-id','42'])
            page.tabs.setCurrentIndex(0); self.wait_for(lambda:page.docs_request_fields.rowCount()==1)
            self.assertEqual(page.docs_request_fields.item(0,0).text(),'value')
            from ui_pages import Job
            self.wait_for(lambda:not Job.running)
        finally:server.stop(0)


if __name__=='__main__':unittest.main(verbosity=2)
