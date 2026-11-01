"""Regression checks for saved request identity and selected collection export."""
import json
import os
from pathlib import Path
import sqlite3
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
os.environ.setdefault('QT_QPA_PLATFORM','offscreen')
from PySide6.QtCore import Qt
from PySide6.QtWidgets import QApplication,QInputDialog,QFileDialog,QDialog
from main import MainWindow,Store
from ui_theme import STYLE
from ui_collection_export import CollectionExportDialog
from request_transport import grpc_metadata


class SaveAndExport(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app=QApplication.instance() or QApplication([]);cls.app.setStyle('Fusion');cls.app.setStyleSheet(STYLE)
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.store=Store(self.temp.name);self.window=MainWindow(self.store)
    def tearDown(self):
        self.window.close();self.window.deleteLater();self.app.processEvents();self.store.db.close();self.temp.cleanup()
    def fill_grpc(self,page):
        page.server.setText('localhost:443');page.select_method('demo.Accounts','Create')
        page.message.setPlainText('{\n  "name":"saved"\n}');page.metadata.set_rows([['x-tenant','demo','',True],['off','keep','',False]])
        page.auth_type.setCurrentText('Bearer Token');page.auth_value.setText('{{token}}');page.tls.setCurrentIndex(1)
        page.grpc_timeout_ms.setValue(2500);page.tls_server_name.setText('local.test')
    def reopen(self,page):
        rid=page.request_id;self.window.close_request_tab(self.window.workbench.indexOf(page))
        return self.window.open_item(self.window.tree_item('request',rid))

    def test_switching_protocol_keeps_collection_identity_and_persists_all_fields(self):
        cid=self.store.insert('collections',{'name':'Saved','kind':'http'})
        http=self.window.add_tree_request(cid,None,'http');rid=http.request_id
        http.auth_type.setCurrentText('Bearer Token');http.auth_value.setText('{{token}}')
        self.window.switch_request_type(http,'grpc');page=self.window.current_page()
        self.assertEqual(page.request_id,rid);self.assertEqual(page.collection_id,cid)
        self.assertEqual(page.auth_value.text(),'{{token}}')
        self.fill_grpc(page);expected=page.collect();restored=self.reopen(page)
        self.assertEqual(restored.collect(),expected);self.assertEqual(restored.request_id,rid)
        self.assertEqual(self.store.all('SELECT COUNT(*) FROM requests')[0][0],1)

    def test_manual_save_then_immediate_close_restores_complete_request(self):
        page=self.window.current_page();self.fill_grpc(page)
        cid=self.store.insert('collections',{'name':'Saved','kind':'grpc'})
        with patch.object(QInputDialog,'getItem',return_value=(f'Saved  [{cid}]',True)):
            self.window.save_request(page)
        expected=page.collect();self.assertEqual(self.reopen(page).collect(),expected)

    def test_save_uses_values_after_the_destination_dialog(self):
        page=self.window.current_page();self.fill_grpc(page)
        cid=self.store.insert('collections',{'name':'Saved','kind':'grpc'})
        def choose(*args,**kwargs):
            page.message.setPlainText('{"name":"example completed while choosing collection"}')
            return f'Saved  [{cid}]',True
        with patch.object(QInputDialog,'getItem',side_effect=choose):self.window.save_request(page)
        self.assertEqual(json.loads(self.store.entity('request',page.request_id)['data']),page.collect())

    def test_switch_keeps_auth_templates_and_disabled_headers(self):
        page=self.window.new_request();page.url.setText('https://localhost:443')
        page.auth_type.setCurrentText('Basic Auth');page.auth_value.setText('user:{{password}}')
        page.headers.set_rows([['x-id','42','',True],['disabled','keep','',False]])
        self.window.switch_request_type(page,'grpc');grpc=self.window.current_page()
        self.assertEqual(grpc.collect()['auth_value'],'user:{{password}}')
        self.assertEqual(grpc_metadata(grpc.collect(),{'password':'secret'}),[('x-id','42'),('authorization','Basic dXNlcjpzZWNyZXQ=')])
        self.window.switch_request_type(grpc,'http');restored=self.window.current_page().collect()
        self.assertEqual(restored['auth_value'],'user:{{password}}');self.assertEqual(len(restored['header_rows']),2)

    def test_protocol_switch_of_draft_keeps_one_draft_uid(self):
        page=self.window.current_page();uid=page._draft_id;self.fill_grpc(page)
        self.window.switch_request_type(page,'http');http=self.window.current_page()
        self.assertEqual(http._draft_id,uid)
        self.assertEqual(self.store.all('SELECT COUNT(*) FROM local_drafts')[0][0],1)
        expected=http.collect();self.window.close_request_tab(self.window.workbench.indexOf(http))
        self.assertEqual(self.window.open_draft(uid).collect(),expected)

    def test_failed_manual_save_keeps_last_committed_snapshot_and_open_tab(self):
        cid=self.store.insert('collections',{'name':'Saved','kind':'grpc'});page=self.window.add_tree_request(cid,None,'grpc')
        before=page._saved;self.fill_grpc(page)
        with patch.object(self.store,'save_request',side_effect=sqlite3.OperationalError('disk full')):
            self.window.save_request(page);self.window.close_request_tab(self.window.workbench.indexOf(page))
            self.assertGreaterEqual(self.window.workbench.indexOf(page),0)
            self.assertEqual(page._saved,before);self.assertIn('Autosave failed',page.save_state.text())

    def seed_collection(self,name,kind):
        cid=self.store.insert('collections',{'name':name,'kind':kind,'description':'Local API','variables':json.dumps({'tenant':'test'})})
        fid=self.store.insert('folders',{'name':'Service','collection_id':cid})
        page=self.window.add_tree_request(cid,fid,kind)
        if kind=='grpc':self.fill_grpc(page)
        else:page.url.setText('https://example.test/items');page.body_type.setCurrentText('raw');page.body.setPlainText('{"value":42}')
        self.window.flush_page(page)
        self.store.insert('examples',{'request_id':page.request_id,'name':'Example','data':json.dumps({'request':page.collect(),'response':{'body':'{}','status':200,'reason':'OK'}})})
        return cid,page

    def test_tree_selects_entire_collections_and_bundle_restores_selected_only(self):
        http,http_page=self.seed_collection('HTTP','http');grpc,grpc_page=self.seed_collection('gRPC','grpc')
        omitted=self.store.insert('collections',{'name':'Omit','kind':'http'})
        dialog=CollectionExportDialog(self.window);self.assertEqual(dialog.selected_ids(),[http,grpc,omitted])
        dialog.select_all(False);self.assertFalse(dialog.export_button.isEnabled())
        for item in dialog.roots[:2]:item.setCheckState(0,Qt.CheckState.Checked)
        self.assertEqual(dialog.selected_ids(),[http,grpc]);self.assertTrue(dialog.export_button.isEnabled())
        self.assertEqual(dialog.roots[0].child(0).child(0).child(0).text(0),'Example')
        # Latest edits must be captured even before the 300 ms autosave timer.
        grpc_page.message.setPlainText('{"name":"latest"}')
        target=str(Path(self.temp.name)/'selected.json')
        def selection(instance):
            instance.roots[-1].setCheckState(0,Qt.CheckState.Unchecked)
            return QDialog.DialogCode.Accepted
        with patch.object(CollectionExportDialog,'exec',selection),patch.object(QFileDialog,'getSaveFileName',return_value=(target,'')):
            self.window.export_collections()
        document=json.loads(Path(target).read_text(encoding='utf-8'));self.assertEqual(len(document['collections']),2)
        with patch.object(QFileDialog,'getOpenFileName',return_value=(target,'')):self.window.import_collection()
        imported=self.store.all('SELECT id,name FROM collections WHERE id>? ORDER BY id',(omitted,))
        self.assertEqual([name for _,name in imported],['HTTP','gRPC'])
        for (cid,_),page in zip(imported,[http_page,grpc_page]):
            record=self.store.all('SELECT id,data FROM requests WHERE collection_id=?',(cid,))[0]
            self.assertEqual(json.loads(record[1]),page.collect())
            self.assertEqual(self.store.all('SELECT COUNT(*) FROM examples WHERE request_id=?',(record[0],))[0][0],1)
            self.assertEqual(json.loads(self.store.entity('collection',cid)['variables']),{'tenant':'test'})
        dialog.deleteLater()

    def test_bundle_failure_rolls_back_all_collections_and_empty_selection_is_rejected(self):
        cid,_=self.seed_collection('API','grpc');document=self.store.export_documents([cid])
        document['collections'].append({'format':'unknown'})
        before=self.store.all('SELECT COUNT(*) FROM collections')[0][0]
        with self.assertRaises(ValueError):self.store.import_documents(document)
        self.assertEqual(self.store.all('SELECT COUNT(*) FROM collections')[0][0],before)
        with self.assertRaises(ValueError):self.store.export_documents([])
        with self.assertRaises(ValueError):self.store.import_documents({'format':'reqon-collections-v1','collections':[]})

    def test_cancel_export_does_not_open_file_picker(self):
        with patch.object(CollectionExportDialog,'exec',return_value=QDialog.DialogCode.Rejected),patch.object(QFileDialog,'getSaveFileName') as choose:
            self.window.export_collections();choose.assert_not_called()


if __name__=='__main__':unittest.main(verbosity=2)
