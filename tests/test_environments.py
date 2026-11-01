"""Windows environment workflows on isolated databases and loopback servers."""
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from PySide6.QtCore import Qt,QPoint,QPointF,QEvent
from PySide6.QtGui import QMouseEvent,QColor
from PySide6.QtWidgets import QApplication,QInputDialog,QMessageBox,QFileDialog
from PySide6.QtTest import QTest
from main import MainWindow,Store,Worker
from ui_theme import STYLE
from ui_brand import USE_REQON_UI,COLORS as C
from environment_store import EnvironmentStore
from ui_environments import EnvironmentPage,ROLE
from ui_collections import CollectionPage
from error_fixture import ErrorServer


@unittest.skipUnless(USE_REQON_UI,'Reqon environments')
class EnvironmentMigration(unittest.TestCase):
    def test_existing_environment_migration_and_atomic_invalid_import(self):
        with tempfile.TemporaryDirectory() as directory:
            store=Store(directory)
            try:
                id=store.insert('environments',{'name':'Existing','variables':'{"host":"localhost","port":8080}'})
                env=EnvironmentStore(store);original=env.get(id)
                self.assertEqual(original['rows'][1]['value'],'8080');self.assertEqual(original['color'],'')
                self.assertTrue((Path(directory)/'reqon.before-0.13.1.sqlite3').exists())
                env.save(id,'Existing',original['rows'],'#b3261e')
                self.assertEqual(EnvironmentStore(store).get(id)['uid'],original['uid'])
                before=len(env.all())
                with self.assertRaises(ValueError):env.import_document({'name':'Bad','values':[{'key':'x'}],'_reqon':{'color':'bad'}})
                self.assertEqual(len(env.all()),before)
                self.assertEqual(store.all('PRAGMA integrity_check')[0][0],'ok')
            finally:store.db.close()


@unittest.skipUnless(USE_REQON_UI,'Reqon environments')
class EnvironmentsUI(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app=QApplication.instance() or QApplication([]);cls.app.setStyle('Fusion');cls.app.setStyleSheet(STYLE)
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.store=Store(self.temp.name);self.window=MainWindow(self.store)
        self.window.resize(1370,990);self.window.show();self.window.activateWindow();self.window.raise_();QTest.qWait(40)
    def tearDown(self):
        for popup in (self.window.environments.popup,self.window.environments.menu):
            if popup:popup.close()
        self.window.close();self.window.deleteLater();self.app.processEvents();self.store.db.close();self.temp.cleanup()
    def add_row(self,page,key,value,enabled=True):
        row=page.rows.canvas.items[-1];row.key.setText(key);row.value.setText(value);row.enabled.setChecked(enabled);return row
    def restart(self):
        with patch.object(QMessageBox,'question',side_effect=AssertionError('Unexpected save prompt')):self.assertTrue(self.window.close())
        self.window.deleteLater();self.app.processEvents();self.store.db.close()
        self.store=Store(self.temp.name);self.window=MainWindow(self.store);self.window.show();QTest.qWait(50)
    def test_navigation_empty_row_and_focus_create_exactly_one_tail(self):
        with patch.object(QInputDialog,'getItem',side_effect=AssertionError('Modal environment editor')):self.window.select_nav('Environments')
        page=self.window.workbench.currentWidget();self.assertIsInstance(page,EnvironmentPage)
        self.assertIs(self.window.side_tabs.currentWidget(),self.window.environments.sidebar)
        self.assertEqual(len(page.rows.canvas.items),1);self.assertEqual(page.rows.data(),[])
        first=page.rows.canvas.items[0];QTest.mouseClick(first.key,Qt.MouseButton.LeftButton)
        self.assertEqual(len(page.rows.canvas.items),2)
        QTest.keyClicks(first.key,'base_url');QTest.keyClicks(first.value,'http://localhost')
        self.assertEqual(len(page.rows.canvas.items),2)
        count=self.window.workbench.count();self.window.select_nav('Collections');self.window.select_nav('Environments')
        self.assertEqual(self.window.workbench.count(),count);self.assertIs(self.window.workbench.currentWidget(),page)
    def test_live_rename_color_selection_and_hover_actions(self):
        page=self.window.environments.create();id=page.environment_id;page.name.setText('Development')
        self.window.environments.apply(id);self.window.environments.set_color(id,'#b3261e')
        combo=self.window.env_combo;self.assertEqual(combo.currentData(),id);self.assertEqual(combo.currentText(),'Development')
        self.assertIn('#b3261e',combo.styleSheet());self.assertEqual(page.color,'#b3261e')
        self.window.environments.set_color(id,'#008c45');self.assertIn('#008c45',combo.styleSheet())
        listing=self.window.environments.sidebar.list;listing.show_hover(listing.item(0))
        self.assertTrue(listing.color.isVisible());self.assertTrue(listing.apply.isVisible());self.assertTrue(listing.more.isVisible())
        listing.apply.click();self.assertIsNone(combo.currentData())
        listing.show_hover(listing.item(0));listing.apply.click();self.assertEqual(combo.currentData(),id)
        listing.show_hover(listing.item(0));listing.color.click();self.assertTrue(self.window.environments.popup.isVisible())
        self.window.environments.popup.choose('#0668d2');self.assertIn('#0668d2',combo.styleSheet())
        self.assertEqual([action.text() for action in self.window.environments.menu_for(id).actions() if not action.isSeparator()],['Export','Move','Rename','Duplicate','Delete'])
    def test_drag_handle_reorders_with_animation_and_deletion_persists(self):
        page=self.window.environments.create();canvas=page.rows.canvas
        for key in ('first','second','third'):self.add_row(page,key,key+' value')
        first=canvas.items[0];first.hovered=True;first.refresh_actions();handle=first.handle;start=handle.mapToGlobal(handle.rect().center())
        def mouse(kind,global_pos,buttons):
            local=handle.mapFromGlobal(global_pos)
            self.app.sendEvent(handle,QMouseEvent(kind,QPointF(local),QPointF(global_pos),Qt.MouseButton.LeftButton if kind!=QEvent.Type.MouseMove else Qt.MouseButton.NoButton,buttons,Qt.KeyboardModifier.NoModifier))
        mouse(QEvent.Type.MouseButtonPress,start,Qt.MouseButton.LeftButton)
        mouse(QEvent.Type.MouseMove,start+QPoint(0,66),Qt.MouseButton.LeftButton)
        self.assertIs(canvas.dragged,first);self.assertEqual([row['key'] for row in canvas.data()],['second','third','first'])
        self.assertTrue(any(animation.duration()==160 for animation in canvas.animations.values()))
        mouse(QEvent.Type.MouseButtonRelease,start+QPoint(0,66),Qt.MouseButton.NoButton);QTest.qWait(300)
        self.assertIsNone(canvas.dragged);self.assertEqual([row.y() for row in canvas.items],[0,33,66,99])
        canvas.items[1].delete.click();QTest.qWait(300)
        self.assertEqual([row['key'] for row in self.window.environments.data.get(page.environment_id)['rows']],['second','first'])
        self.assertTrue(canvas.items[-1].placeholder)
    def test_globals_collection_environment_precedence_and_live_grpc(self):
        globals_page=self.window.environments.open(None);self.add_row(globals_page,'user','global');self.add_row(globals_page,'global_only','yes')
        cid=self.store.insert('collections',{'name':'API','kind':'grpc','variables':'{"user":"collection"}'})
        page=self.window.environments.create();row=self.add_row(page,'{{user}}','active');self.window.environments.apply(page.environment_id)
        self.assertEqual(self.window.variables(cid)['user'],'active');row.enabled.setChecked(False)
        self.assertEqual(self.window.variables(cid)['user'],'collection');row.enabled.setChecked(True)
        server=ErrorServer()
        try:
            self.add_row(page,'grpc_host',f'127.0.0.1:{server.port}')
            request=dict(server.data('Echo'),server='{{grpc_host}}',message='{"name":"{{user}}"}')
            worker=Worker(request,self.window.variables(cid));results=[];worker.done.connect(results.append);worker.run()
            self.assertNotIn('error',results[0]);self.assertEqual(json.loads(results[0]['body']),{'name':'active'})
        finally:server.close()
        self.window.environments.apply(None);self.assertEqual(self.window.variables()['user'],'global')
    def test_autosave_and_restart_restore_tabs_rows_colors_and_active_environment(self):
        page=self.window.environments.create();id=page.environment_id;page.name.setText('Saved environment')
        self.add_row(page,'host','localhost');self.add_row(page,'disabled','keep',False)
        self.window.environments.set_color(id,'#a31578');self.window.environments.apply(id)
        self.restart();restored=self.window.workbench.currentWidget()
        self.assertIsInstance(restored,EnvironmentPage);self.assertEqual(restored.environment_id,id)
        self.assertEqual(restored.name.text(),'Saved environment');self.assertEqual(restored.color,'#a31578')
        self.assertEqual(len(restored.rows.canvas.items),3);self.assertFalse(restored.rows.data()[1]['enabled'])
        self.assertEqual(self.window.env_combo.currentData(),id);self.assertIn('#a31578',self.window.env_combo.styleSheet())
        self.assertEqual(self.window.variables(),{'host':'localhost'})
    def test_export_import_duplicate_move_and_delete_active_environment(self):
        controller=self.window.environments;page=controller.create();id=page.environment_id;page.name.setText('Source')
        self.add_row(page,'host','localhost');self.add_row(page,'disabled','retain',False);controller.set_color(id,'#c99a00')
        target=str(Path(self.temp.name)/'environment.json')
        with patch.object(QFileDialog,'getSaveFileName',return_value=(target,'')):controller.export(id)
        document=json.loads(Path(target).read_text(encoding='utf-8'));self.assertEqual(document['values'][1]['enabled'],False)
        with patch.object(QFileDialog,'getOpenFileName',return_value=(target,'')):controller.import_file()
        imported=self.window.workbench.currentWidget();self.assertNotEqual(imported.uid,page.uid);self.assertEqual(imported.collect(),page.collect())
        duplicate=controller.duplicate(id);self.assertEqual(duplicate.name.text(),'Source Copy');self.assertEqual(duplicate.rows.data(),page.rows.data())
        labels=[f'{i+1}. {record["name"]}' for i,record in enumerate(controller.data.all())]
        with patch.object(QInputDialog,'getItem',return_value=(labels[0],True)):controller.move(duplicate.environment_id)
        self.assertEqual(controller.data.all()[0]['id'],duplicate.environment_id)
        controller.apply(id);controller.delete(id,confirm=False);self.assertIsNone(self.window.env_combo.currentData())
        self.assertIsNone(controller.page(id));self.assertFalse(self.store.all('SELECT * FROM environment_details WHERE environment_id=?',(id,)))
    def test_collection_overview_and_selection_match_new_layout(self):
        cid=self.store.insert('collections',{'name':'Local service collection','kind':'http'})
        self.store.save_request({'name':'Example','url':'http://localhost','method':'GET'},collection_id=cid);self.window.refresh_tree()
        item=self.window.tree_item('collection',cid);item.setExpanded(False);self.window.open_item(item)
        page=self.window.workbench.currentWidget();self.assertIsInstance(page,CollectionPage);self.assertTrue(item.isExpanded())
        self.assertEqual(page.count.text(),'1 requests');page.description.rich.setFocus()
        QTest.keyClicks(page.description.rich,'Collection description')
        self.assertEqual(page.summary.text(),'Collection description');self.assertEqual(self.store.entity('collection',cid)['description'].strip(),'Collection description')
        self.window.tree.setFocus();QTest.qWait(50);rect=self.window.tree.visualItemRect(item)
        image=self.window.tree.viewport().grab().toImage();self.assertEqual(image.pixelColor(0,rect.center().y()),QColor(C['accent']))
        self.assertGreater(page.count.mapTo(page,QPoint()).x(),page.description.mapTo(page,QPoint()).x())
        # The disclosure arrow can still collapse the collection without reopening it.
        point=QPoint(rect.left()-5,rect.center().y());QTest.mouseClick(self.window.tree.viewport(),Qt.MouseButton.LeftButton,pos=point)
        self.assertFalse(item.isExpanded())


if __name__=='__main__':unittest.main(verbosity=2)
