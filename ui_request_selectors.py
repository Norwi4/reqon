"""Anchored request menus with semantic method colors and protocol icons."""
import re
from PySide6.QtCore import Qt,QPoint,QRect,QRectF
from PySide6.QtGui import QColor,QPainter,QPen
from PySide6.QtWidgets import QComboBox,QMenu,QToolButton,QWidgetAction,QLineEdit
from ui_brand import COLORS as C,METHOD_COLORS,PROTOCOL_COLORS
from ui_components import icon


def popup_position(anchor,size,screen):
    """Keep the anchor visible, including near the bottom of a monitor."""
    bounds=screen.availableGeometry()
    x=max(bounds.left(),min(anchor.left(),bounds.right()-size.width()+1))
    below=anchor.bottom()+7
    y=below if below+size.height()<=bounds.bottom()+1 else anchor.top()-size.height()-6
    return QPoint(x,max(bounds.top(),y))


class RequestMenu(QMenu):
    def __init__(self,parent,methods=False):
        super().__init__(parent);self.methods=methods
        self.setObjectName('requestSelectorMenu');self.setMinimumWidth(156 if methods else 170)
        self.setStyleSheet(f'''
            QMenu#requestSelectorMenu {{background:{C['canvas']};border:1px solid {C['border']};border-radius:4px;padding:7px;}}
            QMenu#requestSelectorMenu::item {{padding:8px 8px;min-height:16px;border:0;}}
            QMenu#requestSelectorMenu::separator {{height:1px;margin:8px;background:{C['border']};}}
        ''')
        self.aboutToShow.connect(self.update);self.hovered.connect(lambda *_:self.update())
    def paintEvent(self,event):
        painter=QPainter(self);painter.setRenderHint(QPainter.RenderHint.Antialiasing)
        painter.setPen(QColor(C['border']));painter.setBrush(QColor(C['canvas']))
        painter.drawRoundedRect(QRectF(self.rect()).adjusted(.5,.5,-.5,-.5),4,4)
        for action in self.actions():
            rect=self.actionGeometry(action)
            if isinstance(action,QWidgetAction):continue
            if action.isSeparator():
                painter.setPen(QColor(C['border']));painter.drawLine(rect.left()+8,rect.center().y(),rect.right()-8,rect.center().y());continue
            if action==self.activeAction():
                painter.setPen(Qt.PenStyle.NoPen);painter.setBrush(QColor(C['selected']));painter.drawRoundedRect(QRectF(rect),3,3)
            font=self.font();font.setPixelSize(12);font.setWeight(font.Weight.DemiBold if self.methods else font.Weight.Medium);painter.setFont(font)
            left=rect.left()+8
            if self.methods:
                painter.setPen(QColor(METHOD_COLORS.get(action.text(),C['text'])))
            else:
                left+=8;action.icon().paint(painter,QRect(left,rect.center().y()-8,16,16));left+=24
                painter.setPen(QColor(C['text'] if action.isEnabled() else C['subtle']))
            painter.drawText(QRect(left,rect.top(),rect.right()-left,rect.height()),Qt.AlignmentFlag.AlignVCenter|Qt.AlignmentFlag.AlignLeft,action.text())
    def open_under(self,widget,current=None):
        anchor=QRect(widget.mapToGlobal(QPoint()),widget.size())
        self.ensurePolished();self.adjustSize()
        self.popup(popup_position(anchor,self.sizeHint(),widget.screen()))
        if current is not None:self.setActiveAction(current)


class HttpMethodSelect(QComboBox):
    def __init__(self,parent=None):
        super().__init__(parent);self.setObjectName('httpMethod');self.setAccessibleName('HTTP method')
        self.setFixedSize(110,40);self.setCursor(Qt.CursorShape.PointingHandCursor)
        self.addItems(['GET','POST','PUT','PATCH','DELETE','HEAD','OPTIONS'])
        self.popup_menu=RequestMenu(self,methods=True);self.popup_menu.aboutToHide.connect(self._closed)
        self.custom_method=None
        self.currentTextChanged.connect(lambda *_:self.update())
    def paintEvent(self,event):
        painter=QPainter(self);painter.setRenderHint(QPainter.RenderHint.Antialiasing)
        if self.hasFocus() or self.popup_menu.isVisible():
            painter.setPen(QPen(QColor(C['action']),2));painter.setBrush(Qt.BrushStyle.NoBrush)
            painter.drawRoundedRect(QRectF(self.rect()).adjusted(1,1,-1,-1),4,4)
        font=self.font();font.setPixelSize(12);font.setWeight(font.Weight.DemiBold);painter.setFont(font)
        painter.setPen(QColor(METHOD_COLORS.get(self.currentText(),C['text'])))
        label=painter.fontMetrics().elidedText(self.currentText(),Qt.TextElideMode.ElideRight,self.width()-40)
        painter.drawText(self.rect().adjusted(12,0,-26,0),Qt.AlignmentFlag.AlignVCenter|Qt.AlignmentFlag.AlignLeft,label)
        icon('chevron',C['muted'],12).paint(painter,QRect(self.width()-27,(self.height()-12)//2,12,12))
    def showPopup(self):
        self.popup_menu.clear();current=None
        for index in range(self.count()):
            action=self.popup_menu.addAction(self.itemText(index));action.setData(index)
            action.triggered.connect(lambda checked=False,index=index:self._select(index))
            if index==self.currentIndex():current=action
        self.popup_menu.addSeparator()
        self.custom_method=QLineEdit(self.popup_menu);self.custom_method.setAccessibleName('Custom HTTP method')
        self.custom_method.setPlaceholderText('Type a new method');self.custom_method.setFixedHeight(30)
        self.custom_method.setStyleSheet(f'QLineEdit {{border:0;background:transparent;padding:0 8px;font-size:11px;color:{C["text"]};}}')
        self.custom_method.returnPressed.connect(self._add_custom)
        footer=QWidgetAction(self.popup_menu);footer.setDefaultWidget(self.custom_method);self.popup_menu.addAction(footer)
        self.popup_menu.open_under(self,current);self.update()
    def _select(self,index):
        self.setCurrentIndex(index);self.hidePopup();self.setFocus(Qt.FocusReason.PopupFocusReason)
        self.activated.emit(index);self.textActivated.emit(self.currentText())
    def _add_custom(self):
        value=self.custom_method.text().strip().upper()
        if not re.fullmatch(r"[!#$%&'*+.^_`|~0-9A-Z-]+",value):
            self.custom_method.setToolTip('Enter an HTTP method without spaces or control characters.');return
        index=self.findText(value)
        if index<0:self.addItem(value);index=self.count()-1
        self._select(index)
    def hidePopup(self):
        self.popup_menu.hide();super().hidePopup();self.update()
    def _closed(self):
        super().hidePopup();self.update()


class RequestProtocolButton(QToolButton):
    def __init__(self,kind,parent=None):
        super().__init__(parent);self.kind=kind;self.setObjectName('requestProtocol')
        self.setFixedSize(26,26);self.setIcon(icon(kind,PROTOCOL_COLORS[kind],18))
        self.setAccessibleName('Request type: '+('gRPC' if kind=='grpc' else 'HTTP'))
        self.setToolTip('Switch request type: HTTP / gRPC');self.setCursor(Qt.CursorShape.PointingHandCursor)
        self.setPopupMode(QToolButton.ToolButtonPopupMode.DelayedPopup)
        self.clicked.connect(self.open_menu)
    def open_menu(self):
        menu=self.menu()
        if menu is None:return
        current=next((action for action in menu.actions() if action.data()==self.kind),None)
        menu.open_under(self,current);self.update()
    def mousePressEvent(self,event):
        if event.button()==Qt.MouseButton.LeftButton:
            self.setFocus(Qt.FocusReason.MouseFocusReason);self.open_menu();event.accept()
        else:super().mousePressEvent(event)
    def keyPressEvent(self,event):
        if event.key() in (Qt.Key.Key_Space,Qt.Key.Key_Return,Qt.Key.Key_Enter,Qt.Key.Key_Down,Qt.Key.Key_F4):
            self.open_menu();event.accept()
        else:super().keyPressEvent(event)
    def paintEvent(self,event):
        painter=QPainter(self);painter.setRenderHint(QPainter.RenderHint.Antialiasing)
        painter.setPen(Qt.PenStyle.NoPen);painter.setBrush(QColor(C['selected'] if self.underMouse() or self.menu() and self.menu().isVisible() else C['panel']))
        painter.drawRoundedRect(QRectF(self.rect()),4,4)
        self.icon().paint(painter,QRect(4,4,18,18))
