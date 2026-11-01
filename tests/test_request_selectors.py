"""Exercise the actual popup, keyboard selection and saved request data."""
import os,sys,tempfile,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
os.environ.setdefault('QT_QPA_PLATFORM','offscreen')
from PySide6.QtCore import Qt,QPoint,QRect,QSize
from PySide6.QtGui import QColor
from PySide6.QtTest import QTest
from PySide6.QtWidgets import QApplication
from main import MainWindow,Store
from ui_pages import RequestPage,GrpcPage
from ui_brand import METHOD_COLORS
from ui_request_selectors import popup_position
from ui_theme import STYLE


class RequestSelectors(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app=QApplication.instance() or QApplication([]);cls.app.setStyle('Fusion');cls.app.setStyleSheet(STYLE)
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.store=Store(self.temp.name);self.window=MainWindow(self.store)
        self.window.resize(1000,700);self.window.move(0,0);self.window.show()
        self.page=self.window.new_request({'method':'GET','url':'http://localhost/api','body':'{"example":true}','body_type':'raw'})
        QTest.qWait(30)
    def tearDown(self):
        for widget in self.app.topLevelWidgets():
            if widget.windowType()==Qt.WindowType.Popup:widget.hide()
        self.window.close();self.window.deleteLater();self.app.processEvents();self.store.db.close();self.temp.cleanup()
    def test_popup_is_below_the_field_and_every_method_keeps_its_color(self):
        combo=self.page.method;combo.showPopup();self.app.processEvents();menu=combo.popup_menu
        self.assertGreater(menu.y(),combo.mapToGlobal(QPoint(0,combo.height())).y())
        self.assertEqual(menu.activeAction().text(),'GET')
        image=menu.grab().toImage()
        for action in menu.actions()[:7]:
            rect=menu.actionGeometry(action);target=QColor(METHOD_COLORS[action.text()])
            found=False
            for y in range(rect.top(),rect.bottom()+1):
                for x in range(rect.left()+8,min(rect.right(),rect.left()+90)):
                    color=image.pixelColor(x,y)
                    if max(abs(color.red()-target.red()),abs(color.green()-target.green()),abs(color.blue()-target.blue()))<18:
                        found=True;break
                if found:break
            self.assertTrue(found,action.text()+' lost its foreground color')
    def test_keyboard_selection_escape_and_repeated_open(self):
        combo=self.page.method;combo.showPopup();menu=combo.popup_menu
        QTest.keyClick(menu,Qt.Key.Key_Down);QTest.keyClick(menu,Qt.Key.Key_Return)
        self.assertEqual(self.page.collect()['method'],'POST');self.assertFalse(menu.isVisible())
        combo.showPopup();self.assertEqual(menu.activeAction().text(),'POST')
        QTest.keyClick(menu,Qt.Key.Key_Down);QTest.keyClick(menu,Qt.Key.Key_Escape)
        self.assertEqual(combo.currentText(),'POST');self.assertFalse(menu.isVisible())
        combo.showPopup();QTest.mouseClick(menu,Qt.MouseButton.LeftButton,pos=menu.actionGeometry(menu.actions()[4]).center())
        self.assertEqual(self.page.collect()['method'],'DELETE')
    def test_custom_method_validation_and_roundtrip(self):
        combo=self.page.method;combo.showPopup();combo.custom_method.setText('bad method')
        combo._add_custom();self.assertEqual(combo.currentText(),'GET');self.assertTrue(combo.popup_menu.isVisible())
        combo.custom_method.setText('propfind');combo._add_custom()
        self.assertEqual(self.page.collect()['method'],'PROPFIND')
        rebuilt=RequestPage(self.page.collect());self.assertEqual(rebuilt.method.currentText(),'PROPFIND');rebuilt.deleteLater()
    def test_protocol_menu_icons_position_and_switch_preserve_request(self):
        button=self.page.protocol;QTest.mouseClick(button,Qt.MouseButton.LeftButton);self.app.processEvents();menu=button.menu()
        self.assertTrue(menu.isVisible());self.assertGreater(menu.y(),button.mapToGlobal(QPoint(0,button.height())).y())
        self.assertEqual([a.text() for a in menu.actions()],['HTTP','gRPC'])
        self.assertTrue(all(not a.icon().isNull() for a in menu.actions()))
        self.assertNotEqual(menu.actions()[0].icon().pixmap(16,16).toImage(),menu.actions()[1].icon().pixmap(16,16).toImage())
        QTest.keyClick(menu,Qt.Key.Key_Down);QTest.keyClick(menu,Qt.Key.Key_Return)
        page=self.window.current_page();self.assertIsInstance(page,GrpcPage)
        self.assertEqual(page.message.toPlainText(),'{"example":true}')
        page.protocol.menu().actions()[0].trigger();page=self.window.current_page();self.assertIsInstance(page,RequestPage)
        self.assertEqual(page.url.text(),'http://localhost/api');self.assertEqual(page.body.toPlainText(),'{"example":true}')
    def test_popup_uses_space_above_at_screen_bottom(self):
        class Screen:
            def availableGeometry(self):return QRect(100,100,800,600)
        anchor=QRect(750,650,110,40);size=QSize(170,260)
        position=popup_position(anchor,size,Screen())
        self.assertLess(position.y()+size.height(),anchor.top())
        self.assertGreaterEqual(position.x(),100);self.assertLessEqual(position.x()+size.width(),900)


if __name__=='__main__':unittest.main(verbosity=2)
