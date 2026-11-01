import json
import sqlite3
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from PySide6.QtCore import Qt, QPoint, QEvent
from PySide6.QtGui import QMouseEvent
from PySide6.QtTest import QTest
from PySide6.QtWidgets import QApplication, QLineEdit, QMessageBox
from main import Store, MainWindow
from ui_pages import HTTP_DEFAULT, GRPC_DEFAULT
from ui_collections import CollectionPage, ExamplePage, ROLE
from ui_theme import STYLE


class CollectionDataTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.store = Store(self.temp.name)
        self.cid = self.store.insert('collections', {'name': 'API', 'kind': 'grpc'})
        self.fid = self.store.insert('folders', {'collection_id': self.cid, 'name': 'Service'})
        self.nested = self.store.insert('folders', {'collection_id': self.cid, 'parent_id': self.fid, 'name': 'Nested'})
        self.rid = self.store.save_request(dict(GRPC_DEFAULT, name='Create', metadata=[['token', 'local']]), collection_id=self.cid, folder_id=self.nested)
        self.eid = self.store.insert('examples', {'request_id': self.rid, 'name': 'Success', 'data': json.dumps({
            'request': dict(GRPC_DEFAULT, name='Create', message='{"id":1}'),
            'response': {'body': '{"ok":true}', 'status': 0, 'reason': 'OK', 'duration': 5, 'size': 11, 'trailers': [['trace', '42']]}})})

    def tearDown(self): self.store.db.close(); self.temp.cleanup()

    def test_duplicate_copies_nested_structure_examples_and_independent_ids(self):
        copy = self.store.duplicate_entity('collection', self.cid)
        self.assertNotEqual(self.store.local_link('collection', copy), self.store.local_link('collection', self.cid))
        folder = self.store.children(copy)[0][1]; nested = self.store.children(copy, folder)[0][1]
        request = self.store.children(copy, nested)[0][1]
        self.assertNotEqual(request, self.rid)
        example = self.store.children(request_id=request)[0][1]
        self.assertEqual(self.store.entity('example', example)['data'], self.store.entity('example', self.eid)['data'])
        self.store.rename_entity('request', request, 'Changed')
        self.assertEqual(json.loads(self.store.entity('request', request)['data'])['name'], 'Changed')
        self.assertEqual(self.store.entity('request', self.rid)['name'], 'Create')

    def test_move_branch_and_reject_cycles_without_partial_changes(self):
        target = self.store.insert('collections', {'name': 'Target', 'kind': 'http'})
        http = self.store.save_request(dict(HTTP_DEFAULT, name='Ping'), collection_id=target)
        with self.assertRaises(ValueError): self.store.move_entity('folder', self.fid, self.cid, self.nested)
        self.assertIsNone(self.store.entity('folder', self.fid)['parent_id'])
        self.store.move_entity('folder', self.fid, target)
        self.assertEqual(self.store.entity('request', self.rid)['collection_id'], target)
        self.assertEqual(self.store.entity('folder', self.nested)['collection_id'], target)
        self.assertEqual(self.store.entity('collection', target)['kind'], 'mixed')
        self.assertEqual(self.store.entity('example', self.eid)['request_id'], self.rid)
        with self.assertRaises(ValueError): self.store.move_entity('request', http, self.cid, self.nested)
        self.assertEqual(self.store.entity('request', http)['collection_id'], target)

    def test_delete_folder_removes_branch_and_examples_only(self):
        survivor = self.store.save_request(dict(GRPC_DEFAULT, name='Keep'), collection_id=self.cid)
        self.store.delete_entity('folder', self.fid)
        self.assertEqual(self.store.all('SELECT id FROM requests'), [(survivor,)])
        self.assertEqual(self.store.all('SELECT id FROM examples'), [])
        self.assertEqual(self.store.all('PRAGMA foreign_key_check'), [])

    def test_sort_reorder_and_local_links_survive_reopen(self):
        a = self.store.save_request(dict(GRPC_DEFAULT, name='Alpha'), collection_id=self.cid)
        self.store.sort_children('collection', self.cid)
        self.assertEqual([r[2] for r in self.store.children(self.cid)], ['Alpha', 'Service'])
        self.store.move_entity('request', a, self.cid)
        self.assertEqual([r[2] for r in self.store.children(self.cid)], ['Service', 'Alpha'])
        link = self.store.local_link('request', a)
        self.store.db.close(); self.store = Store(self.temp.name)
        self.assertEqual(self.store.resolve_link(link), ('request', a))
        self.assertEqual([r[2] for r in self.store.children(self.cid)], ['Service', 'Alpha'])
        self.store.delete_entity('request', a)
        self.store.save_request(dict(GRPC_DEFAULT, name='Replacement'), collection_id=self.cid)
        with self.assertRaises(ValueError): self.store.resolve_link(link)

    def test_mixed_roundtrip_preserves_nested_examples_descriptors_variables(self):
        descriptor = Path(self.temp.name)/'test.protoset'; descriptor.write_bytes(b'local descriptor fixture')
        data = json.loads(self.store.entity('request', self.rid)['data']); data['descriptor_file'] = str(descriptor)
        self.store.save_request(data, self.rid)
        self.store.save_request(dict(HTTP_DEFAULT, name='HTTP', param_rows=[['x', '1', '', False]]), collection_id=self.cid)
        self.store.db.execute('UPDATE collections SET description=?,variables=? WHERE id=?', ('Summary', '{"host":"localhost"}', self.cid)); self.store.db.commit()
        exported = self.store.export_document(self.cid); imported = self.store.import_document(exported)
        self.assertEqual(self.store.export_document(imported), exported)
        self.assertEqual(self.store.entity('collection', imported)['kind'], 'mixed')
        self.assertEqual(self.store.all('PRAGMA foreign_key_check'), [])

    def test_bad_import_rolls_back_entire_collection(self):
        document = self.store.export_document(self.cid); document['items'].append({'type': 'request', 'data': {'kind': 'unsupported'}})
        count = self.store.all('SELECT COUNT(*) FROM collections')[0][0]
        with self.assertRaises(ValueError): self.store.import_document(document)
        self.assertEqual(self.store.all('SELECT COUNT(*) FROM collections')[0][0], count)
        self.assertEqual(len(self.store.all('SELECT id FROM requests')), 1)

    def test_postman_nested_folder_and_example_roundtrip(self):
        original = {'info': {'name': 'HTTP'}, 'variable': [{'key': 'base', 'value': 'http://localhost'}], 'item': [
            {'name': 'Outer', 'item': [{'name': 'Inner', 'item': [{'name': 'Get', 'request': {
                'method': 'GET', 'url': {'raw': '{{base}}/items', 'query': [{'key': 'q', 'value': 'x', 'disabled': True}]},
                'header': [{'key': 'hidden', 'value': 'v', 'disabled': True}]}, 'response': [
                    {'name': 'Example', 'code': 200, 'status': 'OK', 'body': '{"items":[]}', 'header': []}]}]}]}]}
        cid = self.store.import_document(original); exported = self.store.export_document(cid)
        entry = exported['item'][0]['item'][0]['item'][0]
        self.assertTrue(entry['request']['url']['query'][0]['disabled'])
        self.assertEqual(entry['response'][0]['body'], '{"items":[]}')
        imported = self.store.import_document(exported)
        self.assertEqual(self.store.export_document(imported), exported)

    def test_old_database_migration_preserves_requests_and_backups(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'getman.sqlite3'; db = sqlite3.connect(path)
            db.executescript('''CREATE TABLE collections(id INTEGER PRIMARY KEY,name TEXT NOT NULL,kind TEXT NOT NULL);
                CREATE TABLE folders(id INTEGER PRIMARY KEY,collection_id INTEGER,name TEXT);
                CREATE TABLE requests(id INTEGER PRIMARY KEY,collection_id INTEGER,folder_id INTEGER,name TEXT,data TEXT);
                INSERT INTO collections VALUES(1,'Old','grpc'); INSERT INTO folders VALUES(2,1,'Service');''')
            db.execute('INSERT INTO requests VALUES(3,1,2,?,?)', ('Original', json.dumps(dict(GRPC_DEFAULT, name='Original')))); db.commit(); db.close()
            migrated = Store(directory)
            try:
                self.assertEqual(migrated.entity('request', 3)['name'], 'Original')
                self.assertIsNone(migrated.entity('folder', 2)['parent_id'])
                self.assertTrue((Path(directory)/'reqon.before-0.6.sqlite3').is_file())
                self.assertEqual(migrated.children(1, 2)[0][2], 'Original')
            finally: migrated.db.close()


class CollectionUITests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([]); cls.app.setStyle('Fusion'); cls.app.setStyleSheet(STYLE)
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.store = Store(self.temp.name)
        self.cid = self.store.insert('collections', {'name': 'API', 'kind': 'http'})
        self.rid = self.store.save_request(dict(HTTP_DEFAULT, name='Get customer'), collection_id=self.cid)
        self.window = MainWindow(self.store); self.window.show(); self.window.activateWindow(); self.window.raise_()
        self.window.select_tree_item('request', self.rid); QTest.qWait(80); self.app.processEvents()
    def tearDown(self):
        if getattr(self.window, '_tree_menu', None): self.window._tree_menu.close()
        for i in range(self.window.workbench.count()):
            page = self.window.workbench.widget(i); page._saved = page.collect()
        self.window.close(); self.window.deleteLater(); self.app.processEvents(); self.store.db.close(); self.temp.cleanup()

    def test_hover_menu_star_inline_rename_and_keyboard_duplicate(self):
        tree = self.window.tree; root = self.window.tree_item('collection', self.cid)
        point = tree.visualItemRect(root).center()
        # Deliver to the viewport directly; OS cursor movement may go to another running Getman window.
        event = QMouseEvent(QEvent.Type.MouseMove, point, tree.viewport().mapToGlobal(point),
                            Qt.MouseButton.NoButton, Qt.MouseButton.NoButton, Qt.KeyboardModifier.NoModifier)
        self.app.sendEvent(tree.viewport(), event)
        self.assertTrue(tree.more.isVisible()); self.assertTrue(tree.star.isVisible())
        tree.star.click(); self.assertEqual(self.store.entity('collection', self.cid)['starred'], 1)
        tree.more.click(); self.app.processEvents(); self.assertTrue(self.window._tree_menu.isVisible()); self.window._tree_menu.close()
        item = self.window.tree_item('request', self.rid); self.window.open_item(item)
        tree.setCurrentItem(item); tree.setFocus(); QTest.keyClick(tree, Qt.Key.Key_E, Qt.KeyboardModifier.ControlModifier)
        editor = self.app.focusWidget(); self.assertIsInstance(editor, QLineEdit)
        QTest.keyClick(editor, Qt.Key.Key_A, Qt.KeyboardModifier.ControlModifier); QTest.keyClicks(editor, 'Renamed'); QTest.keyClick(editor, Qt.Key.Key_Return)
        QTest.qWait(30)  # Qt's delegate queues commitData/closeEditor after Enter.
        self.assertEqual(self.store.entity('request', self.rid)['name'], 'Renamed')
        self.assertEqual(self.window.current_page().name.text(), 'Renamed')
        QTest.keyClick(tree, Qt.Key.Key_D, Qt.KeyboardModifier.ControlModifier)
        self.assertEqual([r[0] for r in self.store.all('SELECT name FROM requests ORDER BY id')], ['Renamed', 'Renamed Copy'])

    def test_add_protocol_submenu_and_overview_variables(self):
        root = self.window.tree_item('collection', self.cid); menu = self.window.collection_menu(root)
        submenu = menu.actions()[0].menu(); self.assertEqual([a.text() for a in submenu.actions()], ['HTTP', 'gRPC'])
        submenu.actions()[1].trigger()
        self.assertEqual(self.window.current_page().kind, 'grpc'); self.assertEqual(self.store.entity('collection', self.cid)['kind'], 'mixed')
        self.window.open_item(self.window.tree_item('collection', self.cid)); page = self.window.workbench.currentWidget()
        self.assertIsInstance(page, CollectionPage); self.assertIn('2 requests', page.count.text())
        page.variables.set_rows([['host', 'localhost', '']]); page.save_variables()
        env = self.store.insert('environments', {'name': 'Dev', 'variables': '{"host":"override","token":"secret"}'})
        self.window.refresh_environments(); self.window.env_combo.setCurrentIndex(self.window.env_combo.findData(env))
        self.assertEqual(self.window.variables(self.cid), {'host': 'override', 'token': 'secret'})

    def test_move_rebinds_open_request_and_delete_preserves_unsaved_tab(self):
        item = self.window.tree_item('request', self.rid); page = self.window.open_item(item)
        page.url.setText('http://localhost/unsaved')
        target = self.store.insert('collections', {'name': 'Target', 'kind': 'http'})
        self.window.refresh_tree(); self.window.move_tree_entity('request', self.rid, target)
        self.assertEqual(page.collection_id, target); self.assertEqual(page.url.text(), 'http://localhost/unsaved')
        with patch.object(QMessageBox, 'question', return_value=QMessageBox.StandardButton.Yes):
            self.window.delete_item('collection', target, 'Target')
        self.assertIsNone(page.request_id); self.assertIsNone(page.collection_id)
        self.assertEqual(page.url.text(), 'http://localhost/unsaved')
        self.assertNotEqual(page.collect(), page._saved)

    def test_example_captures_response_and_saves_edits(self):
        page = self.window.open_item(self.window.tree_item('request', self.rid))
        page.show_response({'status': 201, 'reason': 'Created', 'duration': 8, 'size': 8, 'body': '{"id":7}', 'headers': {'trace': '42'}})
        example = self.window.add_example(self.rid); self.assertIsInstance(example, ExamplePage)
        self.assertEqual(example.response.toPlainText(), '{"id":7}')
        example.name.setText('Created customer'); example.response.setPlainText('{"id":8}'); self.window.save_example(example)
        data = json.loads(self.store.entity('example', example.entity_key[1])['data'])
        self.assertEqual(data['response']['body'], '{"id":8}'); self.assertEqual(data['response']['headers'], [['trace', '42']])
        self.assertEqual(example.collect(), example._saved)

    def test_expansion_search_and_drag_destinations_persist(self):
        fid = self.store.insert('folders', {'collection_id': self.cid, 'name': 'Service'})
        nested = self.store.insert('folders', {'collection_id': self.cid, 'parent_id': fid, 'name': 'Nested'})
        self.window.refresh_tree(); folder = self.window.tree_item('folder', fid); folder.setExpanded(True)
        self.window.drop_tree_item(self.window.tree_item('request', self.rid), self.window.tree_item('folder', nested), 'inside')
        self.assertEqual(self.store.entity('request', self.rid)['folder_id'], nested)
        self.window.tree_item('folder', fid).setExpanded(False)
        self.window.search.setText('customer'); self.assertTrue(self.window.tree_item('folder', fid).isExpanded())
        self.window.search.clear(); self.assertFalse(self.window.tree_item('folder', fid).isExpanded())
        self.window.refresh_tree(); self.assertFalse(self.window.tree_item('folder', fid).isExpanded())
        self.window.search.setText('API'); self.assertFalse(self.window.tree_item('request', self.rid).isHidden())


if __name__ == '__main__': unittest.main(verbosity=2)
