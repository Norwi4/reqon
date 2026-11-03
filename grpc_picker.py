"""Anchored native gRPC method picker: loading, grouped results, and retryable errors."""
from ui_theming import set_style,theme_qcolor,set_rich_text
from ui_brand import stylesheet
from PySide6.QtCore import Qt, Signal, QEvent, QPoint, QSize, QTimer
from PySide6.QtGui import QColor, QFont, QPainter, QPen
from PySide6.QtWidgets import (QApplication, QWidget, QFrame, QVBoxLayout, QHBoxLayout,
    QLabel, QLineEdit, QPushButton, QListWidget, QListWidgetItem, QPlainTextEdit,
    QGraphicsDropShadowEffect, QAbstractItemView, QToolTip)
from ui_components import icon, tool


class Spinner(QWidget):
    def __init__(self):
        super().__init__(); self.setFixedSize(18,18); self.angle=0
        self.timer=QTimer(self); self.timer.setInterval(70); self.timer.timeout.connect(self.tick)
    def tick(self):self.angle=(self.angle+30)%360; self.update()
    def showEvent(self,event):self.timer.start()
    def hideEvent(self,event):self.timer.stop()
    def paintEvent(self,event):
        painter=QPainter(self); painter.setRenderHint(QPainter.RenderHint.Antialiasing)
        painter.setPen(QPen(theme_qcolor('#777777'),2)); painter.drawEllipse(3,3,12,12)
        painter.setPen(QPen(theme_qcolor('#dddddd'),2)); painter.drawArc(3,3,12,12,self.angle*16,100*16)


class MethodSelector(QFrame):
    opened=Signal()
    refresh_requested=Signal()
    reflection_requested=Signal()
    import_requested=Signal()
    generate_requested=Signal()
    method_selected=Signal(str,str)

    def __init__(self,parent=None):
        super().__init__(parent); self.setObjectName('methodSelector'); self.setMinimumWidth(200)
        set_style(self,stylesheet('QFrame#methodSelector {border:0;} QFrame#methodSelector[expanded="true"] {border:2px solid #1685ec;border-radius:3px;}'))
        self.state='idle'; self.methods=[]; self.selected=None; self.display=''; self.source='reflection'; self.details=''
        self.popup=None; self._expanded=False
        layout=QHBoxLayout(self); layout.setContentsMargins(6,0,2,0); layout.setSpacing(0)
        self.symbol=QLabel(); self.symbol.setPixmap(icon('rpc','#8ab2e0').pixmap(16,16)); layout.addWidget(self.symbol)
        self.editor=QLineEdit(); self.editor.setPlaceholderText('Select a method'); self.editor.setReadOnly(True)
        from ui_typography import ui_font
        self.editor.setFont(ui_font())
        set_style(self.editor,stylesheet('border:0;background:transparent;padding:0 8px;')); layout.addWidget(self.editor,1)
        self.arrow=tool('chevron','Select a service and method',self.toggle); layout.addWidget(self.arrow)
        self.editor.installEventFilter(self); self.editor.textChanged.connect(self.filter_methods)

    def _create_popup(self):
        if self.popup is not None:return
        self.popup=QFrame(self.window()); self.popup.setObjectName('methodPopup')
        self.destroyed.connect(self.popup.deleteLater)
        set_style(self.popup,stylesheet('''
            QFrame#methodPopup {background:#242424;border:1px solid #373737;border-radius:4px;}
            QFrame#reflectionError {background:#5a1d08;border:0;border-radius:3px;}
            QListWidget {background:transparent;border:0;outline:none;}
            QListWidget::item {padding:0 12px;border:0;border-radius:3px;}
            QListWidget::item:selected {background:#363636;color:#f4f4f4;}
            QListWidget::item:hover {background:#303030;}
            QLabel#hint {color:#a5a5a5;font-size:12px;}
            QFrame#sourceFooter {border:0;border-top:1px solid #383838;}
            QPushButton#importChoice,QPushButton#reflectionChoice {text-align:left;border:0;background:transparent;padding:5px 0;font-weight:600;}
            QPushButton#importChoice:hover,QPushButton#reflectionChoice:hover {color:white;background:#2d2d2d;}
        '''))
        shadow=QGraphicsDropShadowEffect(self.popup); shadow.setBlurRadius(18); shadow.setOffset(0,5); shadow.setColor(QColor(0,0,0,130)); self.popup.setGraphicsEffect(shadow)
        root=QVBoxLayout(self.popup); root.setContentsMargins(14,12,14,0); root.setSpacing(10)
        self.error_banner=QFrame(); self.error_banner.setObjectName('reflectionError')
        self.error_banner.setFixedHeight(48)
        error_row=QHBoxLayout(self.error_banner); error_row.setContentsMargins(12,8,10,8); error_row.setSpacing(8)
        self.error_label=QLabel(); self.error_label.setWordWrap(True); self.error_label.setTextFormat(Qt.TextFormat.RichText)
        self.error_label.linkActivated.connect(self.toggle_details); error_row.addWidget(self.error_label,1)
        self.retry=QPushButton('Try Again'); set_style(self.retry,stylesheet('background:transparent;border:1px solid #926047;padding:6px 12px;')); self.retry.clicked.connect(self.refresh_requested.emit); error_row.addWidget(self.retry)
        root.addWidget(self.error_banner)
        self.error_details=QPlainTextEdit(); self.error_details.setReadOnly(True); self.error_details.setFixedHeight(104); root.addWidget(self.error_details)
        self.choices=QWidget(); choices=QVBoxLayout(self.choices); choices.setContentsMargins(0,0,0,8); choices.setSpacing(8)
        self.hint=QLabel('Use a Protobuf schema to load the services and methods.'); self.hint.setObjectName('hint'); self.hint.setWordWrap(True); choices.addWidget(self.hint)
        self.no_schema=QLabel('No Protobuf schemas available.'); self.no_schema.setObjectName('hint'); choices.addWidget(self.no_schema)
        self.import_button=QPushButton('  Import a .proto file'); self.import_button.setObjectName('importChoice'); self.import_button.setIcon(icon('download')); self.import_button.clicked.connect(self.import_requested.emit); choices.addWidget(self.import_button)
        description=QLabel('      Select a .proto file or descriptor set from your local system.'); description.setObjectName('hint'); description.setWordWrap(True); choices.addWidget(description)
        separator=QFrame(); separator.setFixedHeight(1); set_style(separator,stylesheet('background:#3b3b3b;')); choices.addWidget(separator)
        reflect_row=QHBoxLayout(); reflect_row.setContentsMargins(0,0,0,0); reflect_row.setSpacing(6)
        self.spinner=Spinner(); reflect_row.addWidget(self.spinner)
        self.reflection_symbol=QLabel(); self.reflection_symbol.setPixmap(icon('reflection').pixmap(18,18)); reflect_row.addWidget(self.reflection_symbol)
        self.reflection_button=QPushButton('Use Server Reflection'); self.reflection_button.setObjectName('reflectionChoice'); self.reflection_button.clicked.connect(self.reflection_requested.emit)
        reflect_row.addWidget(self.reflection_button,1); choices.addLayout(reflect_row)
        self.reflection_note=QLabel('      Load methods using server reflection.'); self.reflection_note.setObjectName('hint'); self.reflection_note.setWordWrap(True); choices.addWidget(self.reflection_note); choices.addStretch(1)
        root.addWidget(self.choices)
        self.list=QListWidget(); self.list.setSpacing(0); self.list.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
        self.list.setIconSize(QSize(16,16))
        self.list.setTextElideMode(Qt.TextElideMode.ElideRight); self.list.setVerticalScrollMode(QAbstractItemView.ScrollMode.ScrollPerPixel)
        self.list.itemClicked.connect(self.choose_item); self.list.itemActivated.connect(self.choose_item); self.list.installEventFilter(self); root.addWidget(self.list,1)
        self.empty=QLabel('No services or methods match your search.'); self.empty.setObjectName('hint'); self.empty.setAlignment(Qt.AlignmentFlag.AlignCenter); self.empty.setWordWrap(True); root.addWidget(self.empty,1)
        self.footer=QFrame(); self.footer.setObjectName('sourceFooter'); self.footer.setFixedHeight(34)
        footer=QHBoxLayout(self.footer); footer.setContentsMargins(0,0,0,0); footer.setSpacing(6)
        self.source_label=QLabel(); self.source_label.setObjectName('hint'); footer.addWidget(self.source_label)
        self.refresh=tool('refresh','Reload services and methods',self.refresh_requested.emit); footer.addWidget(self.refresh)
        footer.addStretch(); footer.addWidget(tool('collection','Generate a collection from these services',self.generate_requested.emit,'Generate Collection')); root.addWidget(self.footer)
        self.popup.hide(); self.update_state()

    def setText(self,text):
        self.display=text
        if not self._expanded:self.editor.setText(text)
    def refresh_theme(self):
        self.symbol.setPixmap(icon('rpc','#8ab2e0').pixmap(16,16))
        if self.popup is not None:
            self.reflection_symbol.setPixmap(icon('reflection').pixmap(18,18))
            for index in range(self.list.count()):
                item=self.list.item(index)
                if not item.data(Qt.ItemDataRole.UserRole):item.setForeground(theme_qcolor('#aaaaaa'))
    def text(self):return self.display
    def set_selected(self,service,method):
        self.selected=(service,method)
        self.setText(f'{service.rsplit(".",1)[-1]} / {method}' if service and method else '')
        self.setToolTip(f'{service} / {method}' if service and method else 'Select a service and method')
    def toggle(self):self.dismiss() if self._expanded else self.show_popup()
    def show_popup(self):
        if self._expanded:return
        QToolTip.hideText()
        self._create_popup(); self._expanded=True; self.setProperty('expanded',True); self.style().unpolish(self); self.style().polish(self)
        QApplication.instance().installEventFilter(self)
        self.editor.setReadOnly(False); self.editor.clear(); self.editor.setPlaceholderText('Search services and methods'); self.editor.setFocus()
        self.update_state(); self.reposition(); self.popup.show(); self.popup.raise_(); self.opened.emit()
    def dismiss(self):
        if not self._expanded:return
        QApplication.instance().removeEventFilter(self)
        self._expanded=False; self.popup.hide(); self.editor.setReadOnly(True); self.editor.setText(self.display); self.editor.setPlaceholderText('Select a method')
        self.setProperty('expanded',False); self.style().unpolish(self); self.style().polish(self)
    def reposition(self):
        if self.popup is None:return
        parent=self.window(); pos=self.mapTo(parent,QPoint(0,self.height()+4))
        width=min(max(390,self.width()),parent.width()-20)
        height=440 if self.state=='ready' else 280 if self.state=='error' else 185
        if self.error_details.isVisible():height+=110
        height=min(height,max(160,parent.height()-pos.y()-30))
        x=max(10,min(pos.x(),parent.width()-width-10)); self.popup.setGeometry(x,pos.y(),width,height)
    def eventFilter(self,watched,event):
        if self._expanded and event.type()==QEvent.Type.ToolTip and isinstance(watched,QWidget):
            if watched is self or self.isAncestorOf(watched) or watched is self.popup or self.popup.isAncestorOf(watched):
                QToolTip.hideText(); return True
        if watched==self.editor and event.type()==QEvent.Type.MouseButtonPress and not self._expanded:self.show_popup()
        if event.type()==QEvent.Type.KeyPress and watched in (self.editor,getattr(self,'list',None)):
            key=event.key()
            if key==Qt.Key.Key_Escape and self._expanded:self.dismiss(); return True
            if key in (Qt.Key.Key_Return,Qt.Key.Key_Enter) and self._expanded:
                if self.state=='ready':self.choose_item(self.list.currentItem())
                return True
            if key in (Qt.Key.Key_Down,Qt.Key.Key_Up):
                if not self._expanded:self.show_popup(); return True
                self.navigate(1 if key==Qt.Key.Key_Down else -1); return True
            if watched==self.editor and not self._expanded and event.text():self.show_popup()
        if self._expanded:
            if event.type()==QEvent.Type.MouseButtonPress and hasattr(event,'globalPosition'):
                point=event.globalPosition().toPoint()
                if not self.popup.rect().contains(self.popup.mapFromGlobal(point)) and not self.rect().contains(self.mapFromGlobal(point)):self.dismiss()
            elif event.type()==QEvent.Type.WindowDeactivate and watched==self.window():self.dismiss()
            elif event.type() in (QEvent.Type.Resize,QEvent.Type.Move) and watched in (self,self.window()):self.reposition()
            elif event.type()==QEvent.Type.Hide and watched==self:self.dismiss()
        return super().eventFilter(watched,event)
    def hideEvent(self,event):self.dismiss(); super().hideEvent(event)
    def navigate(self,direction):
        if self.state!='ready':return
        rows=[i for i in range(self.list.count()) if self.list.item(i).data(Qt.ItemDataRole.UserRole)]
        if not rows:return
        current=self.list.currentRow(); index=rows.index(current) if current in rows else (-1 if direction>0 else 0)
        target=rows[(index+direction)%len(rows)]; self.list.setCurrentRow(target); self.list.scrollToItem(self.list.item(target))
    def choose_item(self,item):
        if item is None:return
        data=item.data(Qt.ItemDataRole.UserRole)
        if data:
            self.set_selected(*data); self.dismiss(); self.method_selected.emit(*data)
    def set_loading(self,source='reflection'):
        self.source=source; self.state='loading'; self.details=''; self.update_state()
    def set_error(self,details,source='reflection'):
        self.source=source; self.state='error'; self.details=details; self.update_state()
    def set_idle(self,hint='Enter a server URL to load services and methods.'):
        self.state='idle'; self.details=hint; self.methods=[]; self.update_state()
    def set_methods(self,methods,source='reflection'):
        self.methods=list(methods); self.source=source; self.state='ready'; self.details=''; self.update_state()
    def update_state(self):
        if self.popup is None:return
        failed=self.state=='error'; ready=self.state=='ready'; loading=self.state=='loading'
        self.error_banner.setVisible(failed); self.error_details.hide(); self.choices.setVisible(not ready)
        self.no_schema.setVisible(failed); self.spinner.setVisible(loading); self.reflection_button.setEnabled(not loading)
        self.reflection_symbol.setVisible(not loading)
        set_rich_text(self.error_label,('Could not load server reflection.' if self.source=='reflection' else 'Could not load service definition.')+' <a href="details" style="color:#eeeeee;text-decoration:underline">View error details</a>')
        self.error_details.setPlainText(self.details)
        self.reflection_note.setText('      Loading server reflection.' if loading and self.source=='reflection' else '      Loading service definition.' if loading else '      '+self.details if self.state=='idle' else '      Load methods using server reflection.')
        self.list.setVisible(ready); self.footer.setVisible(ready); self.empty.hide()
        self.source_label.setText('Using server reflection.' if self.source=='reflection' else 'Using local service definition.')
        if ready:self.filter_methods(self.editor.text() if self._expanded else '')
        self.reposition()
    def toggle_details(self,*args):
        self.error_details.setVisible(not self.error_details.isVisible()); self.reposition()
    def filter_methods(self,text):
        if self.popup is None or self.state!='ready':return
        if not self._expanded:text=''
        terms=text.casefold().split(); groups={}
        for service,method in self.methods:
            if all(term in f'{service} {method}'.casefold() for term in terms):groups.setdefault(service,[]).append(method)
        self.list.clear(); first=None; selected=None
        for service,methods in sorted(groups.items()):
            header=QListWidgetItem(service); header.setFlags(Qt.ItemFlag.ItemIsEnabled); header.setForeground(theme_qcolor('#aaaaaa'))
            from ui_typography import ui_font
            header.setFont(ui_font(11,QFont.Weight.DemiBold)); header.setSizeHint(QSize(0,30)); self.list.addItem(header)
            for method in sorted(methods):
                item=QListWidgetItem(icon('rpc','#80b3d5'),'  '+method);item.setFont(ui_font()); item.setSizeHint(QSize(0,29)); item.setData(Qt.ItemDataRole.UserRole,(service,method)); self.list.addItem(item)
                if first is None:first=item
                if self.selected==(service,method):selected=item
        self.empty.setText('No services or methods match your search.' if self.methods else 'The server returned no services or methods.')
        self.empty.setVisible(not groups); self.list.setVisible(bool(groups))
        if selected is not None or first is not None:self.list.setCurrentItem(selected if selected is not None else first)
