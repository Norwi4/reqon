"""Theme persistence, live native rendering and request-state isolation."""
import json,os,sys,tempfile,unittest
from dataclasses import replace
from pathlib import Path
from unittest.mock import patch
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
os.environ.setdefault('QT_QPA_PLATFORM','offscreen')
from PySide6.QtCore import Qt,QCoreApplication,QEvent,QSize
from PySide6.QtGui import QRawFont,QPalette
from PySide6.QtWidgets import QApplication
from PySide6.QtTest import QTest,QSignalSpy
from main import MainWindow,Store
from ui_components import icon
from ui_brand import METHOD_COLORS
from theme_settings import ThemeConfig,ThemePreferences
from ui_palettes import THEMES,current_theme,activate_theme,theme_color,contrast


class ThemeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):cls.app=QApplication.instance() or QApplication([]);cls.app.setStyle('Fusion')
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.store=Store(self.temp.name)
        self.window=MainWindow(self.store);self.window.resize(1400,1000);self.window.show();self.app.processEvents()
    def tearDown(self):
        self.window.general_preferences.save(replace(self.window.general_preferences.config,autosave=True,ask_unsaved=False))
        self.window.close();self.window.deleteLater();QCoreApplication.sendPostedEvents(None,QEvent.Type.DeferredDelete)
        self.store.db.close();self.temp.cleanup();activate_theme('dark')
    def theme(self,**values):
        controller=self.window.theme_controller;controller.set_config(replace(controller.config,**values));self.app.processEvents()

    def test_preferences_survive_reopen_and_reject_invalid_values(self):
        self.assertEqual(ThemePreferences(self.store).config,ThemeConfig())
        preferences=ThemePreferences(self.store);preferences.save(ThemeConfig())
        self.assertEqual(json.loads(self.store.all("SELECT data FROM workspace_state WHERE key='theme'")[0][0])['manual'],'dark')
        changes=self.store.db.total_changes;preferences.save(ThemeConfig())
        self.assertEqual(self.store.db.total_changes,changes)
        config=ThemeConfig('system','dracula','ayu-light','solarized-dark');self.window.theme_controller.set_config(config)
        self.window.general_preferences.save(replace(self.window.general_preferences.config,font_size=16))
        self.assertEqual(ThemePreferences(self.store).config,config)
        reopened=Store(self.temp.name)
        try:self.assertEqual(ThemePreferences(reopened).config,config)
        finally:reopened.db.close()
        with self.assertRaises(ValueError):self.theme(day='missing-theme')
        self.assertEqual(ThemePreferences(self.store).config,config)
        self.store.db.execute("UPDATE workspace_state SET data=? WHERE key='theme'",('{"manual":[]}',));self.store.db.commit()
        fallback=ThemePreferences(self.store);self.assertEqual(fallback.config,ThemeConfig());self.assertTrue(fallback.error)

    def test_manual_grid_and_system_day_night_selection(self):
        self.window.show_settings();overlay=self.window.settings_overlay;overlay.navigation['Themes'].click();self.app.processEvents();page=overlay.themes
        self.assertTrue(page.isVisible());self.assertEqual(len(page.choices),12)
        page.choices['dracula'].click();self.assertEqual(current_theme().id,'dracula');self.assertTrue(page.choices['dracula'].isChecked())
        page.system.click();page.day.select.setCurrentIndex(page.day.select.findData('solarized-light'));page.night.select.setCurrentIndex(page.night.select.findData('night-owl-dark'))
        self.window.theme_controller.system_changed(Qt.ColorScheme.Light);QTest.qWait(10)
        self.assertEqual(current_theme().id,'solarized-light');self.assertTrue(page.day.active.isVisible());self.assertFalse(page.night.active.isVisible())
        self.window.theme_controller.system_changed(Qt.ColorScheme.Dark);QTest.qWait(10)
        self.assertEqual(current_theme().id,'night-owl-dark');self.assertTrue(page.night.active.isVisible())
        page.manual.click();self.assertEqual(current_theme().id,'dracula')
        self.window.theme_controller.system_changed(Qt.ColorScheme.Light);QTest.qWait(10);self.assertEqual(current_theme().id,'dracula')
        self.assertEqual(ThemePreferences(self.store).config.day,'solarized-light')

    def test_all_palettes_preserve_dirty_request_response_cursor_and_undo(self):
        page=self.window.current_page();self.window.general_preferences.save(replace(self.window.general_preferences.config,autosave=False));self.window.general_settings_changed()
        page.message.insertPlainText('{"draft":true}');page.service.setText('example.Users');page.rpc_method.setText('GetUser')
        page.response.editor.setPlainText('{"reply":42}')
        before=page.collect();cursor=page.message.textCursor().position();saved=self.store.all('SELECT data FROM local_drafts WHERE uid=?',(page._draft_id,))
        spy=QSignalSpy(page.message.textChanged);glyph=icon('settings');dark=glyph.pixmap(24,24).toImage()
        for theme in THEMES.values():
            self.theme(manual=theme.id)
            self.assertEqual(page.collect(),before,theme.id);self.assertEqual(page.response.editor.toPlainText(),'{"reply":42}')
            self.assertEqual(page.message.textCursor().position(),cursor);self.assertTrue(page.message.document().isUndoAvailable())
            self.assertEqual(self.app.palette().color(QPalette.ColorRole.Window).name(),theme.colors['canvas'])
            self.assertEqual(page.response.empty.illustration.variant,'dark' if theme.dark else 'light')
            self.assertGreaterEqual(contrast(theme.colors['text'],theme.colors['canvas']),7)
            self.assertGreaterEqual(contrast(theme.colors['muted'],theme.colors['panel']),4.5)
            if theme.id!='dark':self.assertGreaterEqual(contrast(theme.colors['action_text'],theme.colors['action']),4.5)
            for method,color in METHOD_COLORS.items():
                self.assertGreaterEqual(contrast(theme_color(color),theme.colors['canvas']),4.5,(theme.id,method))
        self.assertEqual(spy.count(),0);self.assertEqual(self.store.all('SELECT data FROM local_drafts WHERE uid=?',(page._draft_id,)),saved)
        self.theme(manual='light');self.assertNotEqual(glyph.pixmap(24,24).toImage(),dark)
        page.message.undo();self.assertEqual(page.message.toPlainText(),'')

    def test_collection_paint_host_and_service_use_inter_in_all_states(self):
        cid=self.store.insert('collections',{'name':'Коллекция API','kind':'grpc'})
        fid=self.store.insert('folders',{'collection_id':cid,'name':'Пользователи'})
        rid=self.store.save_request(dict(name='Получить пользователя',kind='grpc',server='localhost:50051',service='example.Users',rpc_method='GetUser'),collection_id=cid,folder_id=fid)
        self.window.refresh_tree();self.window.tree.expandAll();self.window.open_item(self.window.tree_item('request',rid));page=self.window.current_page()
        page.method_picker.set_methods([('example.Users','GetUser'),('example.Users','ListUsers')]);page.method_picker.show_popup();self.app.processEvents()
        from ui_windows_tree import paint_row
        seen=[]
        def observe(painter,option,index,tree):
            paint_row(painter,option,index,tree);seen.append(QRawFont.fromFont(painter.font()).familyName())
        for theme in ('light','dark','dracula'):
            self.theme(manual=theme)
            with patch('ui_windows_tree.paint_row',side_effect=observe):self.window.tree.viewport().grab()
            for field in (page.server,page.method_picker.editor,page.method_picker.list,self.window.tree):
                raw=QRawFont.fromFont(field.font());self.assertEqual(raw.familyName(),'Inter');self.assertTrue(raw.supportsCharacter(ord('Ж')))
            for index in range(page.method_picker.list.count()):self.assertEqual(QRawFont.fromFont(page.method_picker.list.item(index).font()).familyName(),'Inter')
            self.assertEqual(QRawFont.fromFont(page.message.document().defaultFont()).familyName(),'JetBrains Mono')
        self.assertTrue(seen);self.assertEqual(set(seen),{'Inter'})

    def test_new_widgets_and_editor_preferences_follow_active_palette(self):
        scaled=icon('settings').pixmap(QSize(24,24),2.0)
        self.assertEqual(scaled.size(),QSize(48,48));self.assertEqual(scaled.devicePixelRatio(),2.0)
        self.theme(manual='light');page=self.window.new_request({'name':'New request','url':'https://example.test','body_type':'raw','body':'{"ready":true}'})
        self.window.general_preferences.save(replace(self.window.general_preferences.config,font_size=17));self.window.general_settings_changed()
        for theme in ('dracula','light','dark'):
            self.theme(manual=theme);self.assertEqual(page.body.font().pixelSize(),17)
            self.assertEqual(QRawFont.fromFont(page.url.font()).familyName(),'Inter')
            self.assertIn(current_theme().colors['canvas'],page.method.popup_menu.styleSheet())
        self.theme(manual='light');page.method.showPopup();self.app.processEvents()
        self.assertIn('#ffffff',page.method.popup_menu.styleSheet());self.assertNotIn('#1f1f1f',page.method.popup_menu.styleSheet());page.method.hidePopup()

    def test_hidden_tabs_and_settings_receive_latest_theme_before_showing(self):
        first=self.window.current_page();first.message.insertPlainText('{"draft":true}')
        before=first.collect();spy=QSignalSpy(first.message.textChanged)
        second=self.window.new_grpc_request()
        self.assertFalse(first.isVisible())
        for theme_id in ('light','dracula','high-contrast-light','dark'):
            self.theme(manual=theme_id)
            self.window.workbench.setCurrentWidget(first);first.tabs.setCurrentIndex(1);self.app.processEvents()
            self.assertEqual(first.message.palette().color(QPalette.ColorRole.Base).name(),current_theme().colors['canvas'])
            self.assertEqual(first.message.palette().color(QPalette.ColorRole.Text).name(),current_theme().colors['text'])
            first.tabs.setCurrentIndex(first.tabs.count()-1);self.app.processEvents()
            self.assertEqual(first.tls_server_name.palette().color(QPalette.ColorRole.Base).name(),current_theme().colors['canvas'])
            self.assertEqual(first.collect(),before)
            self.window.workbench.setCurrentWidget(second);self.app.processEvents()
        self.assertEqual(spy.count(),0)
        self.assertTrue(first.message.document().isUndoAvailable())


if __name__=='__main__':unittest.main()
