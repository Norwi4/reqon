"""Windows presentation regressions; all requests and workspaces are local fixtures."""
import json
import socket
import ssl
import subprocess
import sys
import tempfile
import unittest
import urllib.error
from pathlib import Path
from unittest.mock import patch
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from PySide6.QtCore import Qt
from PySide6.QtWidgets import QApplication,QMessageBox
from PySide6.QtTest import QTest
from main import MainWindow,Store
from ui_brand import USE_REQON_UI,brand_icon
from ui_components import CodeEditor
from ui_pages import ResponsePanel
from ui_theme import STYLE
from ui_error_states import decorate_failure
from response_errors import failure_result
from ui_state_widgets import state_manifest,StateIllustration
from ui_collections import COUNT,METHOD


@unittest.skipUnless(USE_REQON_UI,'Reqon presentation')
class WindowsDesign(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app=QApplication.instance() or QApplication([]);cls.app.setStyle('Fusion');cls.app.setStyleSheet(STYLE)
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.store=Store(self.temp.name)
        self.window=MainWindow(self.store);self.window.resize(1370,990);self.window.show();QTest.qWait(40)
    def tearDown(self):
        self.window.search_palette.hide();self.window.close();self.window.deleteLater();self.app.processEvents()
        self.store.db.close();self.temp.cleanup()
    def test_brand_assets_and_home_preserve_request(self):
        self.assertEqual(self.window.titlebar.height(),48);self.assertTrue(self.window.windowTitle().startswith('Reqon'))
        self.assertFalse(brand_icon(True).isNull());self.assertFalse(self.window.brand_home.icon().isNull())
        self.assertEqual(self.window.brand_home.iconSize().width(),24)
        page=self.window.current_page();page.message.setPlainText('{"draft":true}')
        before=page.collect();count=self.window.workbench.count()
        self.window.select_nav('History');self.window.brand_home.click()
        self.assertEqual(self.window.workbench.count(),count);self.assertEqual(page.collect(),before)
        self.assertIs(self.window.side_tabs.currentWidget(),self.window.tree)
        for state in state_manifest():
            art=StateIllustration(state);self.assertFalse(art.pixmap.isNull(),state)
            self.assertTrue(art.pixmap.hasAlphaChannel(),state);self.assertEqual(art.asset_id,state);art.deleteLater()
    def test_empty_payload_and_http_error_do_not_replace_data(self):
        panel=ResponsePanel();panel.resize(800,400);panel.show();self.app.processEvents()
        self.assertEqual(panel.empty.state,'response-ready-to-send');self.assertIs(panel.body_stack.currentWidget(),panel.empty)
        response={'status':204,'reason':'No Content','body':'','headers':{}}
        panel.show_response(response)
        self.assertEqual(panel.empty.state,'response-no-content');self.assertEqual(panel.metrics.text(),'')
        self.assertEqual(panel.empty.illustration.asset_id,'request-completed')
        panel.show_response(dict(response,status=503,reason='Service Unavailable',body='{"reason":"maintenance"}',duration=142,size=2400))
        self.assertEqual(panel.body_stack.currentIndex(),0)
        self.assertEqual(json.loads(panel.editor.toPlainText()),{'reason':'maintenance'})
        self.assertEqual(panel.http_error.illustration.state,'error-service-unavailable')
        self.assertTrue(panel.tabs.isTabVisible(panel.http_error_index));self.assertIn('2.3 KB',panel.metrics.text())
        panel.show_response(dict(response,status=200,reason='OK',body='{}'))
        self.assertEqual(panel.body_stack.currentIndex(),0);self.assertFalse(panel.tabs.isTabVisible(panel.http_error_index))
        panel.close();panel.deleteLater()
    def test_error_assets_follow_typed_causes_not_message_substrings(self):
        for error,expected in [(socket.gaierror(-2,'name lookup'),'error-dns-resolution'),(ConnectionRefusedError(10061,'refused'),'error-connection-refused'),(TimeoutError('wait'),'error-request-timeout'),(ssl.SSLError('handshake'),'error-tls-handshake')]:
            wrapped=urllib.error.URLError(error);result=decorate_failure(failure_result(wrapped,'http',2),wrapped)
            self.assertEqual(result['error_info']['asset'],expected)
        result=failure_result(ValueError('DNS TLS timeout is part of a user message'),'grpc',2)
        self.assertIsNone(decorate_failure(result)['error_info']['asset'])
        result={'status':14,'error_info':{'category':'connection'}}
        self.assertEqual(decorate_failure(result)['error_info']['asset'],'error-service-unavailable')
        error=ssl.SSLError('handshake');result=decorate_failure(failure_result(error,'grpc',2),error)
        self.assertEqual(result['error_info']['title'],'TLS error')
        self.assertEqual(decorate_failure(result)['error_info']['asset'],'error-tls-handshake')
        misleading=ConnectionError('TLS certificate verify failed is text supplied by the peer')
        result=decorate_failure(failure_result(misleading,'http',2),misleading)
        self.assertIsNone(result['error_info']['asset'])
    def test_search_opens_existing_request_and_escape_closes(self):
        cid=self.store.insert('collections',{'name':'Customers','kind':'http'})
        rid=self.store.save_request({'name':'Find customer','method':'GET','url':'http://localhost/customers'},collection_id=cid)
        self.window.refresh_tree();palette=self.window.search_palette
        self.window.activateWindow();self.window.raise_();QTest.qWait(30)
        QTest.keyClick(self.window.current_page().message,Qt.Key.Key_K,Qt.KeyboardModifier.ControlModifier)
        self.assertTrue(palette.isVisible())
        palette.input.setText('Find customer');QTest.qWait(120)
        self.assertEqual(palette.results.count(),1)
        QTest.keyClick(palette.input,Qt.Key.Key_Return);self.assertFalse(palette.isVisible())
        self.assertEqual(self.window.current_page().request_id,rid)
        count=self.window.workbench.count();palette.open();palette.input.setText('Find customer')
        # Enter before debounce must open the matching item, never a stale result.
        QTest.keyClick(palette.input,Qt.Key.Key_Return);self.assertEqual(self.window.workbench.count(),count)
        palette.open();palette.input.setText('missing-query');QTest.qWait(120)
        self.assertFalse(palette.empty.isHidden());self.assertEqual(palette.empty.state,'search-no-results')
        QTest.keyClick(palette.input,Qt.Key.Key_Escape);self.assertFalse(palette.isVisible())
    def test_collection_counts_methods_search_and_collapse(self):
        cid=self.store.insert('collections',{'name':'Customer API','kind':'http'})
        self.store.save_request({'name':'Create customer','method':'POST','url':'http://localhost/'},collection_id=cid)
        self.window.refresh_tree();root=self.window.tree.topLevelItem(0)
        self.assertEqual(root.data(0,COUNT),1);self.assertEqual(root.child(0).data(0,METHOD),'POST')
        self.window.search.setText('customer');self.assertEqual(self.window.tree.search_query,'customer')
        self.assertEqual(root.child(0).text(0),'Create customer')
        self.window.search.clear();root.setExpanded(True);self.window.tree.collapseAll()
        self.assertFalse(root.isExpanded());self.assertEqual(self.store.entity('collection',cid)['expanded'],0)
    def test_tab_dirty_indicator_and_close_hover_keep_names(self):
        page=self.window.current_page();page.name.setText('Working request');page.message.insertPlainText(' ')
        self.assertTrue(page._presentation_dirty)
        self.assertEqual(self.window.workbench.tabText(self.window.workbench.indexOf(page)),'Working request')
        QTest.qWait(380);self.assertFalse(page._presentation_dirty)
        self.window.new_request();QTest.qWait(20);bar=self.window.workbench.tabBar()
        self.assertEqual(bar.tabButton(0,bar.ButtonPosition.RightSide).graphicsEffect().opacity(),0)
        self.assertEqual(bar.tabButton(1,bar.ButtonPosition.RightSide).graphicsEffect().opacity(),1)
        QTest.mouseMove(bar,bar.tabRect(0).center());QTest.qWait(20)
        self.assertEqual(bar.tabButton(0,bar.ButtonPosition.RightSide).graphicsEffect().opacity(),1)
    def test_splitter_reset_and_json_validation_are_non_destructive(self):
        page=self.window.current_page();page.message.setPlainText('{"broken":')
        QTest.qWait(350);self.assertIsNotNone(page.message.presentation.error)
        with patch.object(QMessageBox,'information',side_effect=AssertionError('Modal validation')):
            self.assertFalse(page.message.format_json())
        self.assertEqual(page.message.toPlainText(),'{"broken":')
        original='{"name":"sample","value":1}';page.message.setPlainText(original)
        self.assertTrue(page.message.format_json());self.assertIn('\n',page.message.toPlainText())
        page.message.undo();self.assertEqual(page.message.toPlainText(),original)
        page.split.setSizes([300,300]);QTest.mouseDClick(page.split.handle(1),Qt.MouseButton.LeftButton)
        sizes=page.split.sizes();self.assertAlmostEqual(sizes[0]/sum(sizes),.6,delta=.02)
        raw=CodeEditor('<item value="test"/>');raw.presentation.validate();self.assertIsNone(raw.presentation.error);raw.deleteLater()
    def test_legacy_platform_path_is_unchanged(self):
        script=Path(__file__).with_name('check_legacy_presentation.py')
        result=subprocess.run([sys.executable,str(script)],capture_output=True,text=True,timeout=30)
        self.assertEqual(result.returncode,0,result.stdout+result.stderr)


if __name__=='__main__':unittest.main(verbosity=2)
