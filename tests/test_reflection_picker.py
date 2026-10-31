import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest.mock import patch
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from PySide6.QtCore import Qt
from PySide6.QtWidgets import QApplication,QFileDialog,QInputDialog
from PySide6.QtTest import QTest
from main import MainWindow,Store
from ui_pages import Job
from ui_theme import STYLE
from reflection_fixture import ReflectionServer


class ReflectionPickerTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app=QApplication.instance() or QApplication([]);cls.app.setStyle('Fusion');cls.app.setStyleSheet(STYLE)
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.store=Store(self.temp.name);self.window=MainWindow(self.store)
        self.server=ReflectionServer();self.window.show();self.app.processEvents()
        self.page=self.window.current_page();self.page.server.setText(self.server.address);self.picker=self.page.method_picker
    def wait_for(self,predicate):
        deadline=time.monotonic()+10
        while not predicate() and time.monotonic()<deadline:QTest.qWait(15)
        self.assertTrue(predicate())
    def tearDown(self):
        self.server.gate.set();self.picker.dismiss();self.wait_for(lambda:not Job.running)
        for index in range(self.window.workbench.count()):
            page=self.window.workbench.widget(index);page._saved=page.collect()
        self.window.close();self.window.deleteLater();self.app.processEvents();self.server.close();self.store.db.close();self.temp.cleanup()
    def open_picker(self):QTest.mouseClick(self.picker.editor,Qt.MouseButton.LeftButton)
    def visible_methods(self):
        return [self.picker.list.item(i).data(Qt.ItemDataRole.UserRole) for i in range(self.picker.list.count()) if self.picker.list.item(i).data(Qt.ItemDataRole.UserRole)]
    def test_auto_load_group_search_and_keyboard_selection(self):
        self.server.gate.clear();self.open_picker()
        self.assertTrue(self.picker.popup.isVisible());self.assertEqual(self.picker.state,'loading')
        self.assertTrue(self.picker.spinner.isVisible())
        # Typing remains responsive while a real reflection request is pending.
        QTest.keyClicks(self.picker.editor,'Orders');self.server.gate.set()
        self.wait_for(lambda:self.picker.state=='ready')
        self.assertEqual(self.visible_methods(),[('picker.Orders','GetOrder'),('picker.Orders','ListOrders')])
        self.picker.editor.setText('GETCUSTOMER');self.assertEqual(len(self.visible_methods()),2)
        self.picker.editor.setText('Customers Get');self.assertEqual(self.visible_methods(),[('picker.Customers','GetCustomer')])
        QTest.keyClick(self.picker.editor,Qt.Key.Key_Return)
        self.assertFalse(self.picker.popup.isVisible());self.assertEqual(self.page.service.text(),'picker.Customers');self.assertEqual(self.page.rpc_method.text(),'GetCustomer')
        count=len(self.server.calls);self.open_picker();self.assertEqual(self.picker.state,'ready');QTest.qWait(80);self.assertEqual(len(self.server.calls),count)
        self.picker.editor.setText('NoSuchMethod');self.assertTrue(self.picker.empty.isVisible())
        QTest.keyClick(self.picker.editor,Qt.Key.Key_Escape);self.assertFalse(self.picker._expanded)
    def test_error_details_retry_and_outside_click(self):
        self.server.disabled=True;self.open_picker();self.wait_for(lambda:self.picker.state=='error')
        self.assertTrue(self.picker.error_banner.isVisible());self.assertIn('UNIMPLEMENTED',self.picker.details)
        self.picker.error_label.linkActivated.emit('details');self.assertTrue(self.picker.error_details.isVisible())
        self.server.disabled=False;self.picker.retry.click();self.assertEqual(self.picker.state,'loading')
        self.wait_for(lambda:self.picker.state=='ready');self.assertEqual(len(self.picker.methods),5)
        QTest.mouseClick(self.page.name,Qt.MouseButton.LeftButton);self.assertFalse(self.picker._expanded)
    def test_stale_server_result_does_not_replace_new_host(self):
        other=ReflectionServer('newhost')
        try:
            self.server.gate.clear();self.open_picker();self.wait_for(lambda:bool(self.server.calls))
            self.page.server.setText(other.address);self.wait_for(lambda:self.picker.state=='ready')
            self.assertTrue(all(s.startswith('newhost.') for s,m in self.picker.methods))
            self.server.gate.set();self.wait_for(lambda:not Job.running)
            self.assertTrue(all(s.startswith('newhost.') for s,m in self.picker.methods))
        finally:other.close()
    def test_reflection_uses_environment_and_authorization(self):
        self.server.required_token='Bearer local-secret'
        self.page.variables_provider=lambda:{'host':self.server.address,'token':'local-secret'}
        self.page.server.setText('{{host}}');self.page.auth_type.setCurrentText('Bearer Token');self.page.auth_value.setText('{{token}}')
        self.open_picker();self.wait_for(lambda:self.picker.state=='ready')
        self.assertTrue(all(dict(call).get('authorization')=='Bearer local-secret' for call in self.server.calls))
    def test_proto_import_and_generate_collection(self):
        source=Path(self.temp.name)/'local.proto'
        source.write_text('syntax="proto3"; package offline; import "google/protobuf/empty.proto"; service Local { rpc Ping(google.protobuf.Empty) returns (google.protobuf.Empty); }',encoding='utf-8')
        self.page.server.clear()
        with patch.object(QFileDialog,'getOpenFileName',return_value=(str(source),'')):self.page.choose_descriptor()
        self.wait_for(lambda:self.picker.state in ('ready','error'))
        self.assertEqual(self.picker.state,'ready',self.picker.details);self.assertEqual(self.picker.methods,[('offline.Local','Ping')])
        self.assertTrue(Path(self.page.descriptor_file.text()).is_file())
        with patch.object(QInputDialog,'getText',return_value=('Generated API',True)):self.picker.generate_requested.emit()
        collections=self.store.all("SELECT id FROM collections WHERE name='Generated API'")
        self.assertEqual(len(collections),1);self.assertEqual(self.store.all('SELECT name FROM requests WHERE collection_id=?',(collections[0][0],))[0][0],'Ping')


if __name__=='__main__':unittest.main(verbosity=2)
