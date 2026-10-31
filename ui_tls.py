"""Native TLS toggle and the interactive hover card shown beside the server URL."""
from PySide6.QtCore import Qt, QEvent, QTimer, QPoint, Signal
from PySide6.QtGui import QCursor
from PySide6.QtWidgets import QFrame, QVBoxLayout, QLabel, QToolButton
from ui_components import icon


class TLSButton(QToolButton):
    configure_requested=Signal()

    def __init__(self,parent=None):
        super().__init__(parent)
        self.setObjectName('tlsButton'); self.setCheckable(True); self.setFixedSize(30,30)
        self.setAccessibleName('Enable TLS'); self.setCursor(Qt.CursorShape.PointingHandCursor)
        self.card=QFrame(self,Qt.WindowType.ToolTip|Qt.WindowType.FramelessWindowHint)
        self.card.setObjectName('tlsCard'); self.card.setAttribute(Qt.WidgetAttribute.WA_ShowWithoutActivating)
        self.card.setFixedWidth(302)
        layout=QVBoxLayout(self.card); layout.setContentsMargins(16,14,16,14); layout.setSpacing(7)
        self.heading=QLabel(); self.heading.setObjectName('tlsHeading'); layout.addWidget(self.heading)
        self.state=QLabel(); self.state.setObjectName('tlsState'); layout.addWidget(self.state)
        self.description=QLabel(); self.description.setWordWrap(True); self.description.setObjectName('muted'); layout.addWidget(self.description)
        layout.addSpacing(7)
        self.link=QLabel('<a href="certificates" style="color:#b5b5b5">Configure additional CA and client certificates</a>')
        self.link.setTextInteractionFlags(Qt.TextInteractionFlag.LinksAccessibleByMouse|Qt.TextInteractionFlag.LinksAccessibleByKeyboard)
        self.link.linkActivated.connect(self.configure); layout.addWidget(self.link)
        self.show_timer=QTimer(self); self.show_timer.setSingleShot(True); self.show_timer.setInterval(250); self.show_timer.timeout.connect(self.show_card)
        self.hide_timer=QTimer(self); self.hide_timer.setSingleShot(True); self.hide_timer.setInterval(160); self.hide_timer.timeout.connect(self.hide_if_outside)
        self.card.installEventFilter(self); self.clicked.connect(self.update_state)
        self.update_state(False)

    def update_state(self,enabled):
        self.setChecked(bool(enabled)); self.setIcon(icon('lock' if enabled else 'lock_off','#c5c5c5'))
        action='Disable TLS' if enabled else 'Enable TLS'
        self.setAccessibleName(action); self.heading.setText(action)
        self.state.setText('Currently using a secured connection.' if enabled else 'Currently using an unsecured connection')
        self.description.setText('Disable TLS to invoke the method over an unsecured connection.' if enabled else 'Enable TLS to invoke the method over a secured connection.')
        self.card.adjustSize()

    def configure(self,*_):
        self.card.hide(); self.configure_requested.emit()

    def show_card(self):
        if not self.isVisible():return
        self.card.adjustSize(); point=self.mapToGlobal(QPoint(0,self.height()+6))
        screen=self.screen().availableGeometry()
        point.setX(max(screen.left(),min(point.x(),screen.right()-self.card.width())))
        if point.y()+self.card.height()>screen.bottom():point.setY(self.mapToGlobal(QPoint(0,0)).y()-self.card.height()-6)
        self.card.move(point); self.card.show()

    def hide_if_outside(self):
        point=QCursor.pos()
        if not self.rect().contains(self.mapFromGlobal(point)) and not self.card.rect().contains(self.card.mapFromGlobal(point)):
            self.card.hide()

    def enterEvent(self,event):
        self.hide_timer.stop(); self.show_timer.start(); super().enterEvent(event)

    def leaveEvent(self,event):
        self.show_timer.stop(); self.hide_timer.start(); super().leaveEvent(event)

    def hideEvent(self,event):
        self.show_timer.stop(); self.hide_timer.stop(); self.card.hide(); super().hideEvent(event)

    def event(self,event):
        if event.type()==QEvent.Type.ToolTip:return True
        return super().event(event)

    def eventFilter(self,watched,event):
        if event.type()==QEvent.Type.Enter:self.hide_timer.stop()
        elif event.type()==QEvent.Type.Leave:self.hide_timer.start()
        return False
