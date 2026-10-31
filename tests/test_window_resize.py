"""Frameless resize regression: all directions, size limits and native hit tests."""
import sys,tempfile,unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from PySide6.QtCore import Qt,QPoint,QPointF,QEvent,QRect
from PySide6.QtGui import QMouseEvent
from PySide6.QtWidgets import QApplication
from main import MainWindow,Store
from ui_window import LEFT,RIGHT,TOP,BOTTOM,CURSORS,WindowsFrame
from ui_theme import STYLE


class WindowResizeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app=QApplication.instance() or QApplication([]);cls.app.setStyle('Fusion');cls.app.setStyleSheet(STYLE)
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.store=Store(self.temp.name);self.window=MainWindow(self.store)
        self.window.setGeometry(150,130,1100,760);self.window.show();self.app.processEvents()
    def tearDown(self):
        self.window.close();self.window.deleteLater();self.app.processEvents();self.store.db.close();self.temp.cleanup()
    def drag(self,grip,dx,dy):
        local=grip.rect().center();start=grip.mapToGlobal(local)
        with patch.object(grip,'start_system_resize',return_value=False):
            QApplication.sendEvent(grip,QMouseEvent(QEvent.Type.MouseButtonPress,QPointF(local),QPointF(start),Qt.MouseButton.LeftButton,Qt.MouseButton.LeftButton,Qt.KeyboardModifier.NoModifier))
            QApplication.sendEvent(grip,QMouseEvent(QEvent.Type.MouseMove,QPointF(local),QPointF(start+QPoint(dx,dy)),Qt.MouseButton.NoButton,Qt.MouseButton.LeftButton,Qt.KeyboardModifier.NoModifier))
            QApplication.sendEvent(grip,QMouseEvent(QEvent.Type.MouseButtonRelease,QPointF(local),QPointF(start+QPoint(dx,dy)),Qt.MouseButton.LeftButton,Qt.MouseButton.NoButton,Qt.KeyboardModifier.NoModifier))
        self.app.processEvents()
    def test_all_sides_and_corners_resize_and_keep_the_opposite_edge_anchored(self):
        for edges in CURSORS:
            with self.subTest(edges=edges):
                self.window.setGeometry(150,130,1100,760);self.app.processEvents();before=QRect(self.window.geometry())
                grip=self.window._resize_controller.grips[edges]
                self.assertTrue(grip.isVisible());self.assertEqual(grip.cursor().shape(),CURSORS[edges])
                dx=-70 if edges&LEFT else 70 if edges&RIGHT else 0
                dy=-50 if edges&TOP else 50 if edges&BOTTOM else 0
                self.drag(grip,dx,dy);after=self.window.geometry()
                self.assertEqual(after.width(),before.width()+abs(dx));self.assertEqual(after.height(),before.height()+abs(dy))
                self.assertEqual(after.right() if edges&LEFT else after.left(),before.right() if edges&LEFT else before.left())
                self.assertEqual(after.bottom() if edges&TOP else after.top(),before.bottom() if edges&TOP else before.top())
                self.assertIsNone(grip.origin)
    def test_minimum_and_maximum_sizes_do_not_move_the_fixed_corner(self):
        before=QRect(self.window.geometry());grip=self.window._resize_controller.grips[LEFT|TOP]
        self.drag(grip,3000,3000)
        self.assertEqual(self.window.size(),self.window.minimumSize());self.assertEqual(self.window.geometry().bottomRight(),before.bottomRight())
        self.window.setMaximumSize(1200,850);self.drag(grip,-3000,-3000)
        self.assertEqual(self.window.size(),self.window.maximumSize());self.assertEqual(self.window.geometry().bottomRight(),before.bottomRight())
    def test_grips_hide_when_maximized_and_return_with_rounded_corners(self):
        before=QRect(self.window.geometry());self.window.showMaximized();self.app.processEvents()
        self.assertTrue(self.window.isMaximized());self.assertTrue(self.window.mask().isEmpty())
        self.assertTrue(all(not grip.isVisible() for grip in self.window._resize_controller.grips.values()))
        self.window.showNormal();self.app.processEvents()
        self.assertEqual(self.window.geometry(),before)
        self.assertTrue(all(grip.isVisible() for grip in self.window._resize_controller.grips.values()))
        self.assertFalse(self.window.mask().contains(QPoint(0,0)))
        self.assertFalse(self.window.current_page().collect()['tls_verify'])


@unittest.skipUnless(sys.platform=='win32','Windows message layout')
class NativeHitTests(unittest.TestCase):
    def test_all_native_edges_at_multiple_dpi_and_negative_monitor_coordinates(self):
        import ctypes
        from ctypes import wintypes
        for scale in (1,1.25,1.5,2):
            window=SimpleNamespace(isMaximized=lambda:False,isFullScreen=lambda:False,devicePixelRatioF=lambda:scale)
            native=WindowsFrame(window);native.enabled=True
            def rectangle(hwnd,pointer):
                rect=ctypes.cast(pointer,ctypes.POINTER(wintypes.RECT)).contents
                rect.left=-1800;rect.top=-900;rect.right=-600;rect.bottom=-100;return True
            native.api=SimpleNamespace(GetWindowRect=rectangle)
            for x,y,expected in [(4,400,10),(1196,400,11),(600,4,12),(600,796,15),(5,5,13),(1195,5,14),(5,795,16),(1195,795,17)]:
                msg=wintypes.MSG();msg.message=0x0084;msg.lParam=((x-1800)&0xffff)|(((y-900)&0xffff)<<16)
                self.assertEqual(native.event(ctypes.addressof(msg)),(True,expected))
            msg.lParam=((-1200)&0xffff)|(((-500)&0xffff)<<16)
            self.assertIsNone(native.event(ctypes.addressof(msg)))
            for distance,expected in ((round(7*scale),(True,10)),(round(9*scale),None)):
                msg.lParam=((distance-1800)&0xffff)|(((-500)&0xffff)<<16)
                self.assertEqual(native.event(ctypes.addressof(msg)),expected)


if __name__=='__main__':unittest.main(verbosity=2)
