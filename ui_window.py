"""Resize a frameless native window without sacrificing the custom title bar."""
import sys
from PySide6.QtCore import Qt,QObject,QEvent,QRect
from PySide6.QtWidgets import QWidget,QApplication


LEFT=Qt.Edge.LeftEdge; RIGHT=Qt.Edge.RightEdge; TOP=Qt.Edge.TopEdge; BOTTOM=Qt.Edge.BottomEdge
CURSORS={LEFT:Qt.CursorShape.SizeHorCursor,RIGHT:Qt.CursorShape.SizeHorCursor,
    TOP:Qt.CursorShape.SizeVerCursor,BOTTOM:Qt.CursorShape.SizeVerCursor,
    TOP|LEFT:Qt.CursorShape.SizeFDiagCursor,BOTTOM|RIGHT:Qt.CursorShape.SizeFDiagCursor,
    TOP|RIGHT:Qt.CursorShape.SizeBDiagCursor,BOTTOM|LEFT:Qt.CursorShape.SizeBDiagCursor}


class ResizeGrip(QWidget):
    def __init__(self,window,edges):
        super().__init__(window); self.target=window; self.edges=edges; self.origin=None
        self.setCursor(CURSORS[edges]); self.setFocusPolicy(Qt.FocusPolicy.NoFocus)
        self.setAttribute(Qt.WidgetAttribute.WA_NoSystemBackground)
        self.setAccessibleName('Resize window')
    def start_system_resize(self):
        handle=self.target.windowHandle()
        return bool(handle and handle.startSystemResize(self.edges))
    def mousePressEvent(self,event):
        if event.button()!=Qt.MouseButton.LeftButton:return
        if self.target.isMaximized() or self.target.isFullScreen():return
        event.accept()
        if not self.start_system_resize():
            self.origin=event.globalPosition().toPoint(); self.initial=QRect(self.target.geometry())
            self.grabMouse()
    def mouseMoveEvent(self,event):
        if self.origin is None:return
        if not event.buttons() & Qt.MouseButton.LeftButton:self.finish(); return
        delta=event.globalPosition().toPoint()-self.origin
        rect=QRect(self.initial); minimum=self.target.minimumSize(); maximum=self.target.maximumSize()
        if self.edges & LEFT:
            width=max(minimum.width(),min(maximum.width(),self.initial.width()-delta.x()))
            rect.setLeft(self.initial.right()-width+1)
        elif self.edges & RIGHT:
            rect.setWidth(max(minimum.width(),min(maximum.width(),self.initial.width()+delta.x())))
        if self.edges & TOP:
            height=max(minimum.height(),min(maximum.height(),self.initial.height()-delta.y()))
            rect.setTop(self.initial.bottom()-height+1)
        elif self.edges & BOTTOM:
            rect.setHeight(max(minimum.height(),min(maximum.height(),self.initial.height()+delta.y())))
        self.target.setGeometry(rect); event.accept()
    def finish(self):
        self.origin=None
        if QWidget.mouseGrabber() is self:self.releaseMouse()
    def mouseReleaseEvent(self,event):self.finish(); event.accept()
    def hideEvent(self,event):self.finish(); super().hideEvent(event)


class WindowResize(QObject):
    def __init__(self,window):
        super().__init__(window); self.window=window
        self.grips={edge:ResizeGrip(window,edge) for edge in CURSORS}
        self.native=WindowsFrame(window) if sys.platform=='win32' and QApplication.platformName()=='windows' else None
        window.installEventFilter(self)
    def update(self):
        window=self.window
        enabled=window.isVisible() and not (window.isMaximized() or window.isFullScreen() or window.isMinimized())
        w,h=window.width(),window.height(); border=8; corner=16
        areas={LEFT:QRect(0,corner,border,h-2*corner),RIGHT:QRect(w-border,corner,border,h-2*corner),
            TOP:QRect(corner,0,w-2*corner,border),BOTTOM:QRect(corner,h-border,w-2*corner,border),
            TOP|LEFT:QRect(0,0,corner,corner),TOP|RIGHT:QRect(w-corner,0,corner,corner),
            BOTTOM|LEFT:QRect(0,h-corner,corner,corner),BOTTOM|RIGHT:QRect(w-corner,h-corner,corner,corner)}
        for edge,grip in self.grips.items():
            grip.setGeometry(areas[edge]); grip.setVisible(enabled)
            if enabled:grip.raise_()
    def eventFilter(self,watched,event):
        if event.type() in (QEvent.Type.Show,QEvent.Type.WinIdChange):
            if self.native and self.window.windowHandle():self.native.enable()
        if event.type() in (QEvent.Type.Show,QEvent.Type.Resize,QEvent.Type.WindowStateChange,QEvent.Type.Hide):
            self.update()
        return False


class WindowsFrame:
    """Keep WS_THICKFRAME for native sizing; draw the client area ourselves."""
    def __init__(self,window):
        import ctypes
        from ctypes import wintypes
        self.window=window; self.ctypes=ctypes; self.types=wintypes
        self.api=ctypes.WinDLL('user32',use_last_error=True)
        signatures={
            'GetWindowLongPtrW':([wintypes.HWND,ctypes.c_int],ctypes.c_ssize_t),
            'SetWindowLongPtrW':([wintypes.HWND,ctypes.c_int,ctypes.c_ssize_t],ctypes.c_ssize_t),
            'SetWindowPos':([wintypes.HWND,wintypes.HWND,ctypes.c_int,ctypes.c_int,ctypes.c_int,ctypes.c_int,wintypes.UINT],wintypes.BOOL),
            'GetWindowRect':([wintypes.HWND,ctypes.POINTER(wintypes.RECT)],wintypes.BOOL),
            'IsZoomed':([wintypes.HWND],wintypes.BOOL),
            'MonitorFromWindow':([wintypes.HWND,wintypes.DWORD],wintypes.HMONITOR),
            'GetMonitorInfoW':([wintypes.HMONITOR,wintypes.LPVOID],wintypes.BOOL),
        }
        for name,(args,result) in signatures.items():
            fn=getattr(self.api,name); fn.argtypes=args; fn.restype=result
        class MonitorInfo(ctypes.Structure):
            _fields_=[('size',wintypes.DWORD),('monitor',wintypes.RECT),('work',wintypes.RECT),('flags',wintypes.DWORD)]
        self.MonitorInfo=MonitorInfo; self.enabled=False
    def enable(self):
        hwnd=int(self.window.winId()); style=self.api.GetWindowLongPtrW(hwnd,-16)
        required=0x00040000|0x00010000|0x00020000  # WS_THICKFRAME | MAXIMIZEBOX | MINIMIZEBOX
        if style & required == required:self.enabled=True; return
        self.api.SetWindowLongPtrW(hwnd,-16,style|required)
        self.enabled=True
        self.api.SetWindowPos(hwnd,None,0,0,0,0,0x0020|0x0001|0x0002|0x0004|0x0010)
    def event(self,message):
        c=self.ctypes; t=self.types; msg=t.MSG.from_address(int(message))
        if msg.message==0x0083:  # WM_NCCALCSIZE: keep the custom borderless client area.
            if msg.wParam and self.api.IsZoomed(msg.hWnd) and not self.window.isFullScreen():
                info=self.MonitorInfo(); info.size=c.sizeof(info)
                monitor=self.api.MonitorFromWindow(msg.hWnd,2)
                if self.api.GetMonitorInfoW(monitor,c.byref(info)):
                    rect=c.cast(msg.lParam,c.POINTER(t.RECT)).contents
                    rect.left=info.work.left;rect.top=info.work.top;rect.right=info.work.right;rect.bottom=info.work.bottom
            return True,0
        if msg.message==0x0084 and self.enabled and not (self.window.isMaximized() or self.window.isFullScreen()):
            rect=t.RECT()
            if not self.api.GetWindowRect(msg.hWnd,c.byref(rect)):return None
            x=c.c_short(msg.lParam & 0xffff).value-rect.left
            y=c.c_short((msg.lParam>>16) & 0xffff).value-rect.top
            w,h=rect.right-rect.left,rect.bottom-rect.top
            if not (0<=x<w and 0<=y<h):return None
            scale=self.window.devicePixelRatioF(); border=round(8*scale); corner=round(16*scale)
            if x<corner and y<corner:return True,13  # HTTOPLEFT
            if x>=w-corner and y<corner:return True,14
            if x<corner and y>=h-corner:return True,16
            if x>=w-corner and y>=h-corner:return True,17
            if x<border:return True,10
            if x>=w-border:return True,11
            if y<border:return True,12
            if y>=h-border:return True,15
        return None
