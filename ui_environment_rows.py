"""Native variable rows with a persistent blank tail and animated handle dragging."""
from PySide6.QtCore import Qt,QEvent,QPoint,QPropertyAnimation,QEasingCurve,QTimer,Signal
from PySide6.QtGui import QColor,QPainter,QPen,QCursor
from PySide6.QtWidgets import QWidget,QLineEdit,QCheckBox,QScrollArea,QVBoxLayout,QLabel,QApplication
from ui_components import tool,icon
from ui_brand import COLORS as C

HEIGHT=33


class EnabledCheck(QCheckBox):
    def paintEvent(self,event):
        painter=QPainter(self);painter.setPen(QPen(QColor(C['focus'] if self.hasFocus() else C['subtle']),1))
        painter.setBrush(QColor(C['selected'] if self.isChecked() else C['canvas']));painter.drawRect(1,3,13,13)
        if self.isChecked():icon('check',C['text'],12).paint(painter,2,4,12,12)


class DragHandle(QWidget):
    def __init__(self,row):
        super().__init__(row);self.row=row;self.start=None;self.setCursor(Qt.CursorShape.OpenHandCursor)
        self.setToolTip('Drag to move variable');self.setAccessibleName('Move variable')
    def paintEvent(self,event):
        painter=QPainter(self);painter.setPen(Qt.PenStyle.NoPen);painter.setBrush(QColor(C['muted']))
        for x in (8,12):
            for y in (12,16,20):painter.drawEllipse(QPoint(x,y),1,1)
    def mousePressEvent(self,event):
        if event.button()==Qt.MouseButton.LeftButton:self.start=event.globalPosition().toPoint();event.accept()
    def mouseMoveEvent(self,event):
        if self.start is None:return
        point=event.globalPosition().toPoint();canvas=self.row.canvas
        if canvas.dragged is None and (point-self.start).manhattanLength()>=QApplication.startDragDistance():canvas.begin_drag(self.row,self.start)
        if canvas.dragged:canvas.drag_to(point)
    def mouseReleaseEvent(self,event):
        if self.start is not None:self.row.canvas.end_drag();self.start=None;event.accept()


class VariableRow(QWidget):
    def __init__(self,canvas,data=None,placeholder=False):
        super().__init__(canvas);self.canvas=canvas;self.placeholder=placeholder;self.hovered=False
        self.setObjectName('environmentRow');self.setFixedHeight(HEIGHT);self.setMouseTracking(True)
        self.handle=DragHandle(self);self.enabled=EnabledCheck(self);self.enabled.setChecked((data or {}).get('enabled',True))
        self.enabled.setToolTip('Include variable');self.enabled.setAccessibleName('Include variable')
        self.key=QLineEdit((data or {}).get('key',''),self);self.value=QLineEdit((data or {}).get('value',''),self)
        for field,title in ((self.key,'Variable'),(self.value,'Value')):
            field.setObjectName('environmentCell');field.setAccessibleName(title);field.installEventFilter(self)
            field.textChanged.connect(self.edited)
            field.setStyleSheet(f'QLineEdit {{border:0;border-radius:0;background:transparent;padding:0 8px;color:{C["text"]};}} QLineEdit:focus {{background:{C["hover"]};}}')
        self.key.setPlaceholderText('Add variable' if placeholder else '')
        self.delete=tool('trash','Delete variable',lambda:canvas.remove_row(self),parent=self)
        self.delete.setAccessibleName('Delete variable');self.enabled.toggled.connect(self.edited)
        self.refresh_actions()
    def data(self):return {'key':self.key.text(),'value':self.value.text(),'enabled':self.enabled.isChecked()}
    def edited(self,*_):
        self.canvas.materialize(self);self.canvas.changed.emit();self.refresh_actions()
    def eventFilter(self,obj,event):
        if event.type()==QEvent.Type.FocusIn:
            self.canvas.materialize(self);self.canvas.active=self;self.canvas.refresh_actions();self.update()
        elif event.type()==QEvent.Type.FocusOut:QTimer.singleShot(0,self.canvas.refresh_actions)
        return super().eventFilter(obj,event)
    def enterEvent(self,event):self.hovered=True;self.refresh_actions();super().enterEvent(event)
    def leaveEvent(self,event):self.hovered=False;self.refresh_actions();super().leaveEvent(event)
    def refresh_actions(self):
        active=self.hovered or self.key.hasFocus() or self.value.hasFocus() or self.canvas.dragged is self
        self.handle.setVisible(active and not self.placeholder);self.delete.setVisible(active and not self.placeholder)
        self.enabled.setVisible(not self.placeholder and (active or not self.enabled.isChecked()));self.update()
    def resizeEvent(self,event):
        super().resizeEvent(event);middle=max(150,int(self.width()*.42))
        self.handle.setGeometry(0,0,20,HEIGHT);self.enabled.setGeometry(22,7,18,20)
        self.key.setGeometry(40,1,middle-40,HEIGHT-2);self.value.setGeometry(middle+1,1,max(1,self.width()-middle-33),HEIGHT-2)
        self.delete.setGeometry(max(0,self.width()-30),3,26,26)
    def paintEvent(self,event):
        painter=QPainter(self)
        active=self.hovered or self.key.hasFocus() or self.value.hasFocus() or self.canvas.dragged is self
        painter.fillRect(self.rect(),QColor(C['hover'] if active else C['canvas']))
        painter.setPen(QPen(QColor(C['border']),1));middle=max(150,int(self.width()*.42))
        painter.drawLine(20,0,20,self.height());painter.drawLine(middle,0,middle,self.height())
        painter.drawLine(20,self.height()-1,self.width(),self.height()-1);painter.drawLine(self.width()-1,0,self.width()-1,self.height())


class RowsCanvas(QWidget):
    changed=Signal()
    def __init__(self,scroll,rows):
        super().__init__();self.scroll=scroll;self.items=[];self.dragged=None;self.active=None;self.animations={};self.offset=0
        self.scroll_timer=QTimer(self);self.scroll_timer.setInterval(30);self.scroll_timer.timeout.connect(self.auto_scroll)
        for data in rows:self.items.append(VariableRow(self,data))
        self.items.append(VariableRow(self,placeholder=True));self.reposition()
    def data(self):return [row.data() for row in self.items if not row.placeholder]
    def materialize(self,row):
        if row.placeholder:
            row.placeholder=False;row.key.setPlaceholderText('')
            tail=VariableRow(self,placeholder=True);self.items.append(tail);tail.show();self.reposition();self.changed.emit()
    def remove_row(self,row):
        if row.placeholder:return
        if self.dragged is row:self.end_drag()
        self.stop_animation(row);self.items.remove(row);row.hide();row.deleteLater();self.reposition(True);self.changed.emit()
    def stop_animation(self,row):
        animation=self.animations.pop(row,None)
        if animation:animation.stop();animation.deleteLater()
    def reposition(self,animate=False):
        self.setFixedHeight(len(self.items)*HEIGHT)
        for index,row in enumerate(self.items):
            row.resize(self.width(),HEIGHT)
            if row is self.dragged:continue
            target=QPoint(0,index*HEIGHT)
            if self.animations.get(row) and self.animations[row].endValue()==target:continue
            self.stop_animation(row)
            if animate and row.pos()!=target:
                animation=QPropertyAnimation(row,b'pos',self);animation.setDuration(160);animation.setEasingCurve(QEasingCurve.Type.OutCubic)
                animation.setStartValue(row.pos());animation.setEndValue(target);self.animations[row]=animation;animation.start()
            else:row.move(target)
    def resizeEvent(self,event):super().resizeEvent(event);self.reposition()
    def refresh_actions(self):
        for row in self.items:row.refresh_actions()
    def move_row(self,source,target):
        if source==target:return
        row=self.items.pop(source);self.items.insert(max(0,min(target,len(self.items)-1)),row)
        self.reposition(True);self.changed.emit()
    def begin_drag(self,row,point):
        if row.placeholder:return
        self.dragged=row;self.offset=row.mapFromGlobal(point).y();self.stop_animation(row);row.raise_()
        row.handle.setCursor(Qt.CursorShape.ClosedHandCursor);self.scroll_timer.start();self.refresh_actions()
    def drag_to(self,point):
        if not self.dragged:return
        y=max(0,min(self.mapFromGlobal(point).y()-self.offset,(len(self.items)-2)*HEIGHT))
        self.dragged.move(0,y);target=max(0,min((y+HEIGHT//2)//HEIGHT,len(self.items)-2))
        source=self.items.index(self.dragged)
        if target!=source:self.move_row(source,target)
    def auto_scroll(self):
        point=QCursor.pos();local=self.scroll.viewport().mapFromGlobal(point);bar=self.scroll.verticalScrollBar()
        if local.y()<24:bar.setValue(bar.value()-12)
        elif local.y()>self.scroll.viewport().height()-24:bar.setValue(bar.value()+12)
        self.drag_to(point)
    def end_drag(self):
        self.scroll_timer.stop()
        if self.dragged:
            self.dragged.handle.setCursor(Qt.CursorShape.OpenHandCursor);self.dragged=None;self.reposition(True);self.refresh_actions();self.changed.emit()


class VariablesEditor(QWidget):
    changed=Signal()
    def __init__(self,rows=None):
        super().__init__();layout=QVBoxLayout(self);layout.setContentsMargins(0,0,0,0);layout.setSpacing(0)
        self.header=QWidget();self.header.setFixedHeight(HEIGHT);self.header.installEventFilter(self)
        self.variable_label=QLabel('Variable',self.header);self.value_label=QLabel('Value',self.header)
        for label in (self.variable_label,self.value_label):label.setStyleSheet(f'color:{C["muted"]};font-weight:600;')
        layout.addWidget(self.header)
        self.scroll=QScrollArea();self.scroll.setWidgetResizable(True);self.scroll.setFrameShape(QScrollArea.Shape.NoFrame)
        self.scroll.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff);self.scroll.setAlignment(Qt.AlignmentFlag.AlignTop)
        self.canvas=RowsCanvas(self.scroll,rows or []);self.scroll.setWidget(self.canvas);layout.addWidget(self.scroll,1)
        self.canvas.changed.connect(self.changed)
    def data(self):return self.canvas.data()
    def eventFilter(self,obj,event):
        if obj is self.header and event.type()==QEvent.Type.Resize:
            width=self.header.width();middle=max(150,int(width*.42))
            self.variable_label.setGeometry(48,0,middle-48,HEIGHT);self.value_label.setGeometry(middle+9,0,width-middle-10,HEIGHT)
        if obj is self.header and event.type()==QEvent.Type.Paint:
            painter=QPainter(self.header);painter.fillRect(self.header.rect().adjusted(20,0,0,0),QColor(C['panel']))
            painter.setPen(QColor(C['border']));painter.drawRect(self.header.rect().adjusted(20,0,-1,-1))
            middle=max(150,int(self.header.width()*.42));painter.drawLine(middle,0,middle,HEIGHT);return True
        return super().eventFilter(obj,event)
