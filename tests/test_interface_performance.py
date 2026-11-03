"""Bounded UI work and preservation of data while caches/themes change."""
import json
import os
from pathlib import Path
import sys
import tempfile
import time
import unittest
import gc
import threading
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')
from PySide6.QtCore import QCoreApplication, QEvent
from PySide6.QtTest import QSignalSpy, QTest
from PySide6.QtWidgets import QApplication
from main import MainWindow, Store
from ui_components import CodeEditor
from ui_palettes import activate_theme, theme_color
from deferred_page import DeferredPage


class InterfacePerformanceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])
        cls.app.setStyle('Fusion')

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.store = Store(self.temp.name)
        self.window = MainWindow(self.store)
        self.window.show()
        self.app.processEvents()

    def tearDown(self):
        self.window.close()
        self.window.deleteLater()
        QCoreApplication.sendPostedEvents(None, QEvent.Type.DeferredDelete)
        self.store.db.close()
        self.temp.cleanup()
        activate_theme('dark')

    def test_large_tree_uses_bounded_queries_and_search_restores_expansion(self):
        cid = self.store.insert('collections', {'name': 'Collection', 'kind': 'http'})
        fid = self.store.insert('folders', {'name': 'Folder', 'collection_id': cid})
        self.store.db.executemany('INSERT INTO requests(collection_id,folder_id,name,data,uid,sort_order) VALUES(?,?,?,?,?,?)',
            [(cid, fid, f'Request {i}', json.dumps({'name': f'Request {i}', 'method': 'POST'}),
              f'fixture-{i}', i) for i in range(600)])
        self.store.db.commit()
        statements = []
        self.store.db.set_trace_callback(statements.append)
        self.window.refresh_tree()
        self.assertLessEqual(len(statements), 8)
        folder = self.window.tree_item('folder', fid)
        self.assertEqual(folder.childCount(), 600)
        self.assertFalse(folder.isExpanded())
        statements.clear()
        self.window.search.setText('Request 599')
        self.assertTrue(folder.isExpanded())
        self.assertFalse(folder.child(599).isHidden())
        self.window.search.clear()
        self.assertFalse(folder.isExpanded())
        self.assertEqual(statements, [])
        self.store.db.set_trace_callback(None)
        selected = folder.child(599)
        self.window.tree.setCurrentItem(selected)
        self.window.refresh_tree()
        self.assertEqual(self.window.tree.currentItem().text(0), 'Request 599')

    def test_unchanged_drafts_and_sessions_do_not_write_again(self):
        page = self.window.current_page()
        self.window.flush_page(page, force=True)
        self.window.save_workspace()
        before = self.store.db.total_changes
        for _ in range(3):
            self.assertTrue(self.window.flush_page(page, force=True))
            self.assertTrue(self.window.save_workspace())
        self.assertEqual(self.store.db.total_changes, before)
        page.message.setPlainText('{"changed":true}')
        self.assertTrue(self.window.flush_page(page, force=True))
        saved = json.loads(self.store.all('SELECT data FROM local_drafts WHERE uid=?', (page._draft_id,))[0][0])
        self.assertEqual(saved['message'], '{"changed":true}')

    def test_restored_tabs_stay_saved_until_selected_and_keep_order(self):
        first=self.window.current_page();first.message.setPlainText('{"first":1}')
        second=self.window.new_request({'name':'Second','url':'http://localhost/unused'})
        third=self.window.new_grpc_request({'name':'Third','message':'{"third":3}'})
        expected=[self.window.workbench.widget(i).collect() for i in range(3)]
        self.window.workbench.setCurrentIndex(1)
        self.window.close();self.window.deleteLater()
        QCoreApplication.sendPostedEvents(None,QEvent.Type.DeferredDelete)
        self.window=MainWindow(self.store);self.window.show();self.app.processEvents()
        self.assertIsInstance(self.window.workbench.widget(0),DeferredPage)
        self.assertIsInstance(self.window.workbench.widget(2),DeferredPage)
        self.assertEqual(self.window.current_page().collect(),expected[1])
        self.assertTrue(self.window.flush_all())
        self.assertIsInstance(self.window.workbench.widget(2),DeferredPage)
        self.window.workbench.tabBar().moveTab(2,0)
        self.window.workbench.setCurrentIndex(0)
        loaded=self.window.current_page()
        self.assertEqual(loaded.collect(),expected[2])
        loaded.message.insertPlainText(' ');self.assertTrue(loaded.message.document().isUndoAvailable())
        loaded.message.undo();self.assertEqual(loaded.collect(),expected[2])
        self.window.close_request_tab(1)
        self.assertEqual(self.window.workbench.count(),2)
        self.assertTrue(self.window.save_workspace())

    def test_large_validation_ignores_stale_result_and_preserves_undo(self):
        editor=self.window.current_page().message
        editor.setPlainText('["'+('x'*100000)+'",]')
        editor.presentation.validate()
        editor.insertPlainText('broken')
        editor.presentation.validate()
        deadline=time.monotonic()+3
        while editor.presentation.error is None and time.monotonic()<deadline:QTest.qWait(10)
        self.assertIsNotNone(editor.presentation.error)
        editor.undo();editor.presentation.validate()
        deadline=time.monotonic()+3
        while (editor.presentation._future is not None or editor.presentation.error is not None) and time.monotonic()<deadline:QTest.qWait(10)
        self.assertIsNone(editor.presentation.error)

    def test_automatic_cycle_collection_stays_on_gui_thread(self):
        observed=[];owner=threading.get_ident()
        def callback(phase,info):
            if phase=='start':observed.append(threading.get_ident())
        gc.callbacks.append(callback)
        try:
            def allocate():
                for _ in range(3000):
                    cycle=[];cycle.append(cycle)
            worker=threading.Thread(target=allocate);worker.start();worker.join()
            self.assertFalse(gc.isenabled())
            self.assertNotIn(worker.ident,observed)
            self.app.reqon_collector.collect_if_needed()
            self.assertIn(owner,observed)
            self.assertEqual(set(observed),{owner})
        finally:gc.callbacks.remove(callback)

    def test_editing_body_does_not_rescan_collection_but_protocol_change_does(self):
        cid = self.store.insert('collections', {'name': 'Mixed', 'kind': 'http'})
        data = {'name': 'Request', 'kind': 'http', 'body': 'old'}
        rid = self.store.save_request(data, collection_id=cid)
        self.store.save_request(dict(data, name='Other'), collection_id=cid)
        with patch.object(self.store, 'reconcile_kind', wraps=self.store.reconcile_kind) as reconcile:
            self.store.save_request(dict(data, body='new'), request_id=rid)
            reconcile.assert_not_called()
            self.store.save_request(dict(data, kind='grpc', message='{}'), request_id=rid)
            reconcile.assert_called_once_with(cid)
        self.assertEqual(self.store.entity('collection', cid)['kind'], 'mixed')
        self.assertEqual(json.loads(self.store.entity('request', rid)['data'])['kind'], 'grpc')

    def test_metadata_tree_refresh_preserves_nodes_and_structural_move_rebuilds(self):
        cid=self.store.insert('collections',{'name':'Tree','kind':'http'})
        rid=self.store.save_request({'name':'Old','kind':'http','method':'GET'},collection_id=cid)
        self.window.refresh_tree();item=self.window.tree_item('request',rid)
        self.window.tree.setCurrentItem(item)
        self.store.save_request({'name':'Renamed','kind':'http','method':'POST'},request_id=rid)
        self.window.refresh_tree()
        self.assertIs(self.window.tree_item('request',rid),item)
        self.assertEqual(item.text(0),'Renamed')
        self.assertIs(self.window.tree.currentItem(),item)
        fid=self.store.insert('folders',{'name':'Nested','collection_id':cid})
        self.store.move_entity('request',rid,cid,fid)
        self.window.refresh_tree()
        self.assertEqual(self.window.tree_item('request',rid).parent(),self.window.tree_item('folder',fid))

    def test_large_hidden_editor_recolours_on_show_without_edits_or_lost_undo(self):
        editor = CodeEditor()
        self.addCleanup(editor.deleteLater)
        text = '\n'.join('{"item":"a reasonably long value","number":123}' for _ in range(900))
        editor.insertPlainText(text)
        self.app.processEvents()
        spy = QSignalSpy(editor.textChanged)
        activate_theme('light')
        editor.refresh_syntax_theme()
        expected = theme_color('#9cdcfe')
        first = editor.document().firstBlock()
        self.assertNotIn(expected, [entry.format.foreground().color().name() for entry in first.layout().formats()])
        editor.resize(600, 300)
        editor.show()
        self.app.processEvents()
        self.assertIn(expected, [entry.format.foreground().color().name() for entry in first.layout().formats()])
        deadline = time.monotonic() + 3
        last = editor.document().lastBlock()
        while expected not in [entry.format.foreground().color().name() for entry in last.layout().formats()] and time.monotonic() < deadline:
            QTest.qWait(10)
        self.assertIn(expected, [entry.format.foreground().color().name() for entry in last.layout().formats()])
        self.assertEqual(editor.toPlainText(), text)
        self.assertEqual(spy.count(), 0)
        self.assertTrue(editor.document().isUndoAvailable())
        editor.undo()
        self.assertEqual(editor.toPlainText(), '')
        editor.close()


if __name__ == '__main__':
    unittest.main()
