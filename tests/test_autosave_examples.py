import json
import sqlite3
import sys
import tempfile
import threading
import time
import unittest
from pathlib import Path
from unittest.mock import patch
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from PySide6.QtCore import Qt, QEvent, QPoint
from PySide6.QtGui import QHelpEvent
from PySide6.QtTest import QTest
from PySide6.QtWidgets import QApplication, QFileDialog, QMessageBox, QLineEdit
from google.protobuf import descriptor_pool, message_factory, json_format
from main import MainWindow, Store
from ui_pages import GrpcPage, RequestPage, Job
from ui_theme import STYLE
from grpc_support import compile_proto, example_message, _pool
from reflection_fixture import ReflectionServer


class AutosaveAndExamples(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app=QApplication.instance() or QApplication([]); cls.app.setStyle('Fusion'); cls.app.setStyleSheet(STYLE)
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory(); self.store=Store(self.temp.name); self.window=MainWindow(self.store)
        self.window.show(); self.app.processEvents(); self.server=None
    def tearDown(self):
        if self.server:self.server.gate.set()
        self.wait_for(lambda:not Job.running and not self.window.workers)
        self.window.close(); self.window.deleteLater(); self.app.processEvents()
        if self.server:self.server.close()
        self.store.db.close(); self.temp.cleanup()
    def wait_for(self,predicate):
        deadline=time.monotonic()+10
        while not predicate() and time.monotonic()<deadline:QTest.qWait(20)
        self.assertTrue(predicate())
    def grpc_request(self):
        cid=self.store.insert('collections',{'name':'Saved API','kind':'grpc'})
        page=self.window.add_tree_request(cid,None,'grpc')
        return cid,page
    def record(self,page):return json.loads(self.store.entity('request',page.request_id)['data'])
    def restart(self):
        with patch.object(QMessageBox,'question',side_effect=AssertionError('Unexpected save question')):
            self.assertTrue(self.window.close())
        self.window.deleteLater(); self.app.processEvents(); self.store.db.close()
        self.store=Store(self.temp.name); self.window=MainWindow(self.store); self.window.show(); self.app.processEvents()

    def test_autosave_updates_name_tree_and_all_request_fields(self):
        cid,page=self.grpc_request(); rid=page.request_id
        page.name.setText('Renamed from editor')
        self.assertEqual(self.window.tree_item('request',rid).text(0),'Renamed from editor')
        page.server.setText('localhost:28804'); page.select_method('example.Customers','Create')
        page.message.setPlainText('{"customer_id":"101"}'); page.auth_type.setCurrentText('Bearer Token'); page.auth_value.setText('local-token')
        page.metadata.set_rows([['source','getman','description',True],['disabled','keep','disabled row',False]])
        expected=page.collect(); self.wait_for(lambda:self.record(page)==expected)
        self.assertEqual(page.save_state.text(),'Saved locally')
        self.restart(); restored=self.window.current_page()
        self.assertEqual(restored.request_id,rid); self.assertEqual(restored.collect(),expected)
        self.assertEqual(restored.method_picker.selected,('example.Customers','Create'))

    def test_selected_method_renames_editor_tab_tree_and_saved_request(self):
        cid,page=self.grpc_request(); rid=page.request_id
        page.name.setText('Custom name'); page.method_picker.method_selected.emit('example.Customers','CreateCustomer')
        self.assertEqual(page.name.text(),'CreateCustomer')
        self.assertEqual(self.window.tree_item('request',rid).text(0),'CreateCustomer')
        self.assertEqual(self.window.workbench.tabText(self.window.workbench.indexOf(page)),'CreateCustomer')
        self.wait_for(lambda:self.record(page)['name']=='CreateCustomer')
        page.method_picker.method_selected.emit('example.Customers','GetCustomer')
        self.restart(); restored=self.window.current_page()
        self.assertEqual(restored.name.text(),'GetCustomer'); self.assertEqual(restored.rpc_method.text(),'GetCustomer')
        self.assertEqual(self.window.tree_item('request',rid).text(0),'GetCustomer')

    def test_export_flushes_debounce_and_import_restores_full_grpc_editor(self):
        cid,page=self.grpc_request()
        page.server.setText('grpc.internal.example:50051'); page.tls.setCurrentIndex(1)
        page.select_method('example.Accounts','FindAccount'); page.name.setText('Find account')
        page.message.setPlainText('{\n  "account_id": "42"\n}')
        page.auth_type.setCurrentText('Bearer Token'); page.auth_value.setText('{{token}}')
        page.metadata.set_rows([['x-tenant','local','Tenant',True],['x-disabled','still saved','',False]])
        expected=page.collect(); target=str(Path(self.temp.name)/'export.json')
        self.assertTrue(page._autosave_timer.isActive())
        with patch.object(QFileDialog,'getSaveFileName',return_value=(target,'')):self.window.export_collection(cid)
        exported=json.loads(Path(target).read_text(encoding='utf-8')); self.assertEqual(exported['items'][0]['data'],expected)
        with patch.object(QFileDialog,'getOpenFileName',return_value=(target,'')):self.window.import_collection()
        imported=self.store.all('SELECT MAX(id) FROM collections')[0][0]
        rid=self.store.all('SELECT id FROM requests WHERE collection_id=?',(imported,))[0][0]
        restored=self.window.open_item(self.window.tree_item('request',rid))
        self.assertEqual(restored.collect(),expected)
        self.assertEqual(restored.method_picker.selected,('example.Accounts','FindAccount'))

    def test_metadata_is_saved_while_cell_editor_still_has_focus(self):
        cid,page=self.grpc_request(); page.tabs.setCurrentIndex(3)
        page.metadata.set_rows([['token','','',True]]); self.app.processEvents()
        table=page.metadata.table; table.setCurrentCell(0,2); table.editItem(table.item(0,2)); self.app.processEvents()
        editor=self.app.focusWidget(); self.assertIsInstance(editor,QLineEdit)
        QTest.keyClicks(editor,'live-value')
        self.wait_for(lambda:self.record(page).get('metadata')==[['token','live-value','']])
        self.assertIs(self.app.focusWidget(),editor)
        QTest.keyClick(editor,Qt.Key.Key_Tab)

    def test_immediate_close_saves_http_and_drafts_and_restores_tabs(self):
        grpc=self.window.current_page(); grpc.server.setText('localhost:50051'); grpc.name.setText('gRPC draft')
        http=self.window.new_request(); http.name.setText('HTTP draft'); http.url.setText('http://localhost:8080/customers')
        http.body_type.setCurrentText('raw'); http.body.setPlainText('{"incomplete":')
        expected=grpc.collect(),http.collect()
        self.restart()
        self.assertEqual(self.window.workbench.count(),2)
        self.assertEqual(self.window.workbench.widget(0).collect(),expected[0]); self.assertEqual(self.window.current_page().collect(),expected[1])
        uid=self.window.current_page()._draft_id
        with patch.object(QMessageBox,'question',side_effect=AssertionError('Unexpected save question')):
            self.window.close_request_tab(1)
        self.assertEqual(self.window.open_draft(uid).collect(),expected[1])
        self.assertEqual(self.store.all('SELECT COUNT(*) FROM collections')[0][0],0)

    def test_linked_http_and_example_incomplete_json_are_autosaved(self):
        cid=self.store.insert('collections',{'name':'HTTP','kind':'http'}); page=self.window.add_tree_request(cid,None,'http')
        page.url.setText('http://localhost/new'); page.method.setCurrentText('POST'); page.params.set_rows([['q','value','',True]])
        expected=page.collect(); self.wait_for(lambda:self.record(page)==expected)
        example=self.window.add_example(page.request_id); example.name.setText('Edited example'); example.request.setPlainText('{"partial":')
        expected_example=example.collect(); self.restart()
        self.assertEqual(self.window.workbench.currentWidget().collect(),expected_example)

    def test_save_failure_does_not_claim_success_or_discard_edits(self):
        cid,page=self.grpc_request(); page.message.setPlainText('{"changed":true}')
        with patch.object(self.store,'save_request',side_effect=sqlite3.OperationalError('disk full')):
            self.assertFalse(self.window.flush_page(page))
            self.assertIn('Autosave failed',page.save_state.text()); self.assertNotEqual(page.collect(),page._saved)
        self.assertTrue(self.window.flush_page(page)); self.assertEqual(self.record(page)['message'],'{"changed":true}')

    def reflected_page(self):
        self.server=ReflectionServer('examples'); page=self.window.current_page(); page.server.setText(self.server.address)
        page.method_picker.show_popup(); self.wait_for(lambda:page.method_picker.state=='ready')
        page.method_picker.dismiss(); page.select_method('examples.Customers','GetCustomer')
        return page

    def test_example_uses_cached_contract_and_generates_invocable_payload(self):
        page=self.reflected_page(); calls=len(self.server.calls); self.server.disabled=True
        page.message.setPlainText('{"old":true}'); page.example_button.click()
        self.wait_for(lambda:not page._example_loading)
        self.assertEqual(json.loads(page.message.toPlainText()),{'name':'example'}); self.assertEqual(len(self.server.calls),calls)
        page.message.undo(); self.assertEqual(page.message.toPlainText(),'{"old":true}'); page.message.redo()
        self.server.disabled=False; page.send_button.click(); self.wait_for(lambda:not self.window.workers)
        self.assertEqual(json.loads(page.response_body.toPlainText()),{'name':'example'})

    def test_example_async_result_does_not_overwrite_user_typing_or_new_method(self):
        page=self.reflected_page(); gate=threading.Event()
        def delayed(*args,**kwargs):gate.wait(4); return '{"name":"generated"}'
        with patch('ui_pages.example_message',side_effect=delayed):
            page.example_button.click(); page.message.setPlainText('{"name":"typed"}'); gate.set()
            self.wait_for(lambda:not page._example_loading)
        self.assertEqual(page.message.toPlainText(),'{"name":"typed"}')
        gate.clear()
        with patch('ui_pages.example_message',side_effect=delayed):
            page.example_button.click(); page.select_method('examples.Orders','GetOrder'); gate.set(); self.wait_for(lambda:not Job.running)
        self.assertEqual(page.message.toPlainText(),'{"name":"typed"}')

    def test_expanded_service_method_picker_has_no_hover_tooltips(self):
        page=self.reflected_page(); picker=page.method_picker; picker.show_popup()
        for index in range(picker.list.count()):self.assertEqual(picker.list.item(index).toolTip(),'')
        point=QPoint(10,10); event=QHelpEvent(QEvent.Type.ToolTip,point,picker.list.viewport().mapToGlobal(point))
        self.assertTrue(picker.eventFilter(picker.list.viewport(),event)); picker.dismiss()

    def test_close_during_reflection_saves_without_modal_and_finishes_safely(self):
        self.server=ReflectionServer('closing'); self.server.gate.clear(); page=self.window.current_page(); page.server.setText(self.server.address)
        page.method_picker.show_popup(); self.wait_for(lambda:bool(self.server.calls))
        with patch.object(QMessageBox,'question',side_effect=AssertionError('Unexpected question')),patch.object(QMessageBox,'information',side_effect=AssertionError('Unexpected modal')):
            self.window.close(); self.assertFalse(self.window.isVisible()); self.server.gate.set()
            self.wait_for(lambda:not self.window._idle_close_timer.isActive())
        data=json.loads(self.store.all('SELECT data FROM local_drafts WHERE uid=?',(page._draft_id,))[0][0])
        self.assertEqual(data['server'],self.server.address)

    def test_proto_template_handles_nested_repeated_map_oneof_well_known_and_streaming(self):
        source=Path(self.temp.name)/'sample.proto'
        source.write_text('''syntax="proto3"; package sample;
            import "google/protobuf/timestamp.proto"; import "google/protobuf/duration.proto";
            import "google/protobuf/wrappers.proto"; import "google/protobuf/struct.proto";
            import "google/protobuf/any.proto"; import "google/protobuf/empty.proto";
            import "google/protobuf/field_mask.proto";
            message Child { string name=1; Child child=2; }
            enum State { UNSPECIFIED=0; ACTIVE=1; }
            message Request {
              int64 id=1; string name=2; bool enabled=3; double amount=4; bytes payload=5;
              repeated Child children=6; map<string,Child> users=7; State state=8;
              oneof choice { string first=9; int32 second=10; } optional string optional_name=11;
              google.protobuf.Timestamp time=12; google.protobuf.Duration duration=13;
              google.protobuf.Int64Value wrapper=14; google.protobuf.Struct object=15;
              google.protobuf.Any anything=16; google.protobuf.Empty empty=17;
              google.protobuf.FieldMask mask=18; google.protobuf.Value value=19;
              google.protobuf.ListValue list=20; map<bool,int64> flags=21;
            }
            service API { rpc Send(Request) returns(Request); rpc Stream(stream Request) returns(Request); }
        ''',encoding='utf-8')
        descriptor=compile_proto(source,Path(self.temp.name)/'definitions')
        payload=json.loads(example_message('',False,descriptor,'sample.API','Send'))
        pool=_pool(None,descriptor); cls=message_factory.GetMessageClass(pool.FindMessageTypeByName('sample.Request'))
        message=json_format.ParseDict(payload,cls(),descriptor_pool=pool); message.SerializeToString()
        self.assertEqual(len(payload),20); self.assertEqual(message.WhichOneof('choice'),'first')
        self.assertEqual(payload['id'],'1'); self.assertEqual(payload['children'][0]['name'],'example')
        self.assertEqual(payload['children'][0]['child'],{}); self.assertIn('key',payload['users'])
        self.assertEqual(payload['state'],'ACTIVE'); self.assertEqual(payload['wrapper'],'1')
        self.assertEqual(json.loads(example_message('',False,descriptor,'sample.API','Stream')),[payload])
        page=self.window.current_page(); page.server.clear(); page.descriptor_file.setText(descriptor); page.select_method('sample.API','Send')
        page.example_button.click(); self.wait_for(lambda:not page._example_loading)
        self.assertEqual(json.loads(page.message.toPlainText()),payload)


if __name__=='__main__':unittest.main(verbosity=2)
