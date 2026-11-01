"""Native Qt controls shared by the request editors and workspace shell."""
import json
import re
from PySide6.QtCore import Qt, QSize, QRect, QByteArray, Signal
from PySide6.QtGui import QColor, QIcon, QPixmap, QPainter, QFont, QSyntaxHighlighter, QTextCharFormat, QTextOption
from PySide6.QtSvg import QSvgRenderer
from PySide6.QtWidgets import (QWidget, QPushButton, QToolButton, QPlainTextEdit, QVBoxLayout,
    QHBoxLayout, QLabel, QTableWidget, QTableWidgetItem, QHeaderView, QAbstractItemView,
    QDialog, QLineEdit, QMessageBox, QStyledItemDelegate, QComboBox)
from ui_brand import USE_REQON_UI,COLORS as C,color


PATHS = {
    'proxy': '<rect x="3" y="4" width="18" height="6" rx="1"/><rect x="3" y="14" width="18" height="6" rx="1"/><path d="M7 7h1M7 17h1M12 10v4M16 7h2M16 17h2"/>',
    'eye': '<path d="M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12Z"/><circle cx="12" cy="12" r="3"/>',
    'collapse': '<path d="m7 7 5-4 5 4M7 17l5 4 5-4M4 12h16"/>',
    'more': '<circle cx="5" cy="12" r="1"/><circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/>',
    'star': '<path d="m12 3 2.8 5.7 6.2.9-4.5 4.4 1.1 6.2L12 17.3l-5.6 2.9 1.1-6.2L3 9.6l6.2-.9Z"/>',
    'trash': '<path d="M3 6h18M9 6V3h6v3M5 6l1 15h12l1-15M10 10v7M14 10v7"/>',
    'example': '<path d="M6 3h9l4 4v14H6ZM15 3v5h4M9 12h7M9 16h7"/>',
    'rpc': '<path d="M7 21V3m-4 4 4-4 4 4M17 3v18m-4-4 4 4 4-4"/>',
    'reflection': '<path d="M7 17H5a4 4 0 0 1-1-8 6 6 0 0 1 11-4 5 5 0 0 1 4 9M9 13v8m-3-3 3 3 3-3M16 21v-8m-3 3 3-3 3 3"/>',
    'refresh': '<path d="M20 9a8 8 0 1 0 0 7M20 3v6h-6"/>',
    'menu': '<path d="M4 6h16M4 12h16M4 18h16"/>',
    'back': '<path d="m13 5-7 7 7 7M6 12h14"/>',
    'forward': '<path d="m11 5 7 7-7 7M4 12h14"/>',
    'search': '<circle cx="10" cy="10" r="6.5"/><path d="m15 15 5 5"/>',
    'plus': '<path d="M12 4v16M4 12h16"/>',
    'close': '<path d="m6 6 12 12M18 6 6 18"/>',
    'minimize': '<path d="M5 12h14"/>',
    'maximize': '<rect x="6" y="6" width="12" height="12"/>',
    'chevron': '<path d="m7 9 5 5 5-5"/>',
    'collection': '<path d="M4 6h16l-2 14H6L4 6ZM3 3h18"/>',
    'folder': '<path d="M3 5h7l2 3h9v12H3Z"/>',
    'environment': '<rect x="4" y="3" width="14" height="18" rx="1"/><path d="M8 7h5M8 11h5M8 17h2M21 7v10"/>',
    'history': '<path d="M3 8a9 9 0 1 1 0 9M3 3v5h5M12 7v6l4 2"/>',
    'workspace': '<circle cx="8" cy="7" r="3"/><circle cx="18" cy="7" r="2"/><path d="M2 21v-3a6 6 0 0 1 12 0v3M16 14a5 5 0 0 1 6 5"/>',
    'grpc': '<path d="M7 4h12M7 2 5 4l2 2M17 8h-10M17 6l2 2-2 2M8 13h8v6H8ZM16 18v3H9"/>',
    'http': '<path d="M3 7h18M3 17h18M7 4v16M17 4v16M11 9v6M13 9v6"/>',
    'save': '<path d="M4 3h13l4 4v14H3V3ZM7 3v7h9V3M7 21v-7h10v7"/>',
    'lock': '<rect x="5" y="10" width="14" height="11" rx="1"/><path d="M8 10V6a4 4 0 0 1 8 0v4M12 14v3"/>',
    'unlock': '<rect x="5" y="10" width="14" height="11" rx="1"/><path d="M8 10V6a4 4 0 0 1 8 0M12 14v3"/>',
    'lock_off': '<path d="m3 3 18 18M8 8V6a4 4 0 0 1 8 0v4M5 10v11h14v-5M11 10h8v3M12 14v3"/>',
    'code': '<path d="m7 6-5 6 5 6M17 6l5 6-5 6M14 3l-4 18"/>',
    'docs': '<path d="M4 4h16M4 9h16M4 14h10M4 19h10"/>',
    'settings': '<path d="M12 3v3M12 18v3M3 12h3M18 12h3M6 6l2 2M16 16l2 2M6 18l2-2M16 8l2-2"/><circle cx="12" cy="12" r="6"/><circle cx="12" cy="12" r="2"/>',
    'console': '<rect x="3" y="4" width="18" height="16" rx="1"/><path d="m7 9 3 3-3 3M13 16h4"/>',
    'format': '<path d="M5 3H3v18h2M19 3h2v18h-2M8 7h8M8 12h8M8 17h5"/>',
    'wrap': '<path d="M3 5h18M3 10h14a4 4 0 0 1 0 8h-5M3 15h4M14 15l-3 3 3 3"/>',
    'copy': '<rect x="8" y="7" width="12" height="14" rx="1"/><path d="M16 7V3H3v14h5"/>',
    'download': '<path d="M12 3v12m-5-5 5 5 5-5M4 16v5h16v-5"/>',
    'check': '<path d="m5 12 4 4L19 6"/>',
    'split': '<rect x="3" y="4" width="18" height="16"/><path d="M9 4v16"/>',
}


def icon(name, color='#a6a6a6', size=18):
    svg = f'<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"><g fill="none" stroke="{color}" stroke-width="1.35" stroke-linecap="round" stroke-linejoin="round">{PATHS.get(name, PATHS["docs"])}</g></svg>'
    pixmap = QPixmap(size * 2, size * 2); pixmap.fill(Qt.GlobalColor.transparent)
    painter = QPainter(pixmap); QSvgRenderer(QByteArray(svg.encode())).render(painter); painter.end()
    pixmap.setDevicePixelRatio(2)
    return QIcon(pixmap)


def tool(name, tooltip, callback=None, text='', parent=None):
    button = QToolButton(parent); button.setIcon(icon(name)); button.setIconSize(QSize(16,16))
    button.setText(text); button.setToolTip(tooltip); button.setObjectName('iconButton')
    if text: button.setToolButtonStyle(Qt.ToolButtonStyle.ToolButtonTextBesideIcon)
    if callback: button.clicked.connect(lambda checked=False:callback())
    return button


class Select(QComboBox):
    def paintEvent(self,event):
        super().paintEvent(event)
        painter=QPainter(self); icon('chevron',size=12).paint(painter,QRect(self.width()-19,(self.height()-12)//2,12,12))


class MethodButton(QToolButton):
    def paintEvent(self,event):
        painter=QPainter(self)
        if self.underMouse():painter.fillRect(self.rect(),QColor('#292929'))
        self.icon().paint(painter,QRect(8,(self.height()-16)//2,16,16))
        painter.setPen(QColor('#cccccc')); painter.setFont(self.font())
        text=self.fontMetrics().elidedText(self.text(),Qt.TextElideMode.ElideMiddle,self.width()-40)
        painter.drawText(self.rect().adjusted(30,0,-8,0),Qt.AlignmentFlag.AlignLeft|Qt.AlignmentFlag.AlignVCenter,text)


class ProtocolButton(QToolButton):
    def paintEvent(self,event):
        painter=QPainter(self); painter.fillRect(self.rect(),QColor('#383e47' if self.underMouse() else '#292d33'))
        self.icon().paint(painter,QRect(5,5,16,16)); icon('chevron',size=10).paint(painter,QRect(26,8,10,10))


class Syntax(QSyntaxHighlighter):
    pattern = re.compile(r'//[^\n]*|"(?:\\.|[^"\\])*"|\b(?:true|false|null)\b|(?<![\w])[-+]?\d+(?:\.\d+)?(?:[eE][-+]?\d+)?')
    def highlightBlock(self, text):
        for match in self.pattern.finditer(text):
            value = match.group()
            if value.startswith('//'): color = '#858575'
            elif value.startswith('"'): color = '#9cdcfe' if text[match.end():].lstrip().startswith(':') else '#ce9178'
            elif value in ('true','false','null'): color = '#569cd6'
            else: color = '#b5cea8'
            fmt = QTextCharFormat(); fmt.setForeground(QColor(color)); self.setFormat(match.start(), len(value), fmt)
        if USE_REQON_UI and text.lstrip().startswith('<'):
            fmt=QTextCharFormat();fmt.setForeground(QColor(C['key']))
            for match in re.finditer(r'</?[\w:.-]+|/?>',text):self.setFormat(match.start(),len(match.group()),fmt)
        if USE_REQON_UI:
            from json_comments import comment_ranges
            spans,in_block=comment_ranges(text,self.previousBlockState()==1,allow_incomplete=True)
            self.setCurrentBlockState(1 if in_block else 0)
            fmt=QTextCharFormat();fmt.setForeground(QColor(C['subtle']))
            for start,end in spans:
                self.setFormat(len(text[:start].encode('utf-16-le'))//2,len(text[start:end].encode('utf-16-le'))//2,fmt)


class Gutter(QWidget):
    def __init__(self, editor): super().__init__(editor); self.editor = editor
    def paintEvent(self, event): self.editor.paint_numbers(event)


class CodeEditor(QPlainTextEdit):
    def __init__(self, text='', readonly=False):
        super().__init__(); self.setObjectName('codeEditor')
        from ui_typography import load_fonts,CODE_FAMILY
        if USE_REQON_UI:load_fonts()
        font = QFont(CODE_FAMILY if USE_REQON_UI else 'Consolas'); font.setPixelSize(13 if USE_REQON_UI else 14); self.setFont(font)
        self.setLineWrapMode(QPlainTextEdit.LineWrapMode.NoWrap)
        if USE_REQON_UI:
            self.setWordWrapMode(QTextOption.WrapMode.NoWrap)
            self.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAsNeeded)
        self.setTabStopDistance(self.fontMetrics().horizontalAdvance(' ') * 4)
        self.document().setDocumentMargin(8)
        self.gutter = Gutter(self); self.highlighter = Syntax(self.document())
        self.blockCountChanged.connect(self.update_margin); self.updateRequest.connect(self.update_gutter)
        self.setPlainText(text); self.setReadOnly(readonly); self.update_margin()
        if USE_REQON_UI:
            from ui_windows_editor import EditorPresentation
            self.gutter_color=C['canvas'];self.presentation=EditorPresentation(self)
            from ui_typography import apply_editor_font
            from ui_editor_behavior import settings
            apply_editor_font(self,settings())

    def update_margin(self):
        presentation=getattr(self,'presentation',None)
        self.setViewportMargins(24+self.fontMetrics().horizontalAdvance(str(self.blockCount())),0,0,presentation.bottom_margin if presentation else 0)
    def setPlaceholderText(self,text):
        self._placeholder=text;self.viewport().update()
    def placeholderText(self):return getattr(self,'_placeholder','')
    def paintEvent(self,event):
        super().paintEvent(event)
        if self.document().isEmpty() and self.placeholderText():
            # Qt's native placeholder is covered by the full-width current-line
            # selection. Paint the hint last; it never becomes document text.
            painter=QPainter(self.viewport());painter.setFont(self.font());painter.setPen(QColor(C['subtle']))
            painter.drawText(int(self.document().documentMargin()),int(self.contentOffset().y())+self.fontMetrics().ascent(),self.placeholderText())
    def keyPressEvent(self,event):
        from ui_editor_behavior import editor_key
        if not editor_key(self,event):super().keyPressEvent(event)
    def contextMenuEvent(self,event):
        menu=self.createStandardContextMenu()
        link=self.link_at(event.pos())
        if link and hasattr(self,'link_callback'):menu.addSeparator();menu.addAction('Open link in new request',lambda:self.link_callback(link))
        menu.exec(event.globalPos());menu.deleteLater()
    def link_at(self,point):
        cursor=self.cursorForPosition(point);position=cursor.positionInBlock()
        for match in re.finditer(r'https?://[^\s\"<>]+',cursor.block().text()):
            if match.start()<=position<=match.end():return match.group().rstrip('.,;)')
    def mouseReleaseEvent(self,event):
        if event.modifiers()&Qt.KeyboardModifier.ControlModifier and hasattr(self,'link_callback'):
            link=self.link_at(event.pos())
            if link:self.link_callback(link);event.accept();return
        super().mouseReleaseEvent(event)
    def update_gutter(self, rect, dy):
        if dy: self.gutter.scroll(0,dy)
        else: self.gutter.update(0,rect.y(),self.gutter.width(),rect.height())
        if rect.contains(self.viewport().rect()): self.update_margin()
    def resizeEvent(self,event):
        super().resizeEvent(event)
        self.gutter.setGeometry(0,0,self.viewportMargins().left(),self.height())
        if USE_REQON_UI and hasattr(self,'presentation'):self.presentation.layout_banner()
    def paint_numbers(self,event):
        painter = QPainter(self.gutter); painter.fillRect(event.rect(),QColor(getattr(self,'gutter_color','#212121')))
        painter.setFont(self.font()); painter.setPen(QColor('#777777'))
        block = self.firstVisibleBlock(); number = block.blockNumber()
        top = int(self.blockBoundingGeometry(block).translated(self.contentOffset()).top())
        while block.isValid() and top <= event.rect().bottom():
            height = int(self.blockBoundingRect(block).height())
            if block.isVisible() and top + height >= event.rect().top():
                painter.drawText(0,top,self.gutter.width()-8,self.fontMetrics().height(),Qt.AlignmentFlag.AlignRight,str(number+1))
            block=block.next(); top+=height; number+=1
    def format_json(self):
        if USE_REQON_UI:return self.presentation.format_json()
        try: self.setPlainText(json.dumps(json.loads(self.toPlainText()),ensure_ascii=False,indent=2))
        except ValueError as error: QMessageBox.information(self,'Format JSON',str(error))
    def toggle_wrap(self):
        wrapping=self.lineWrapMode()==QPlainTextEdit.LineWrapMode.NoWrap
        if USE_REQON_UI:self.setWordWrapMode(QTextOption.WrapMode.WrapAtWordBoundaryOrAnywhere if wrapping else QTextOption.WrapMode.NoWrap)
        self.setLineWrapMode(QPlainTextEdit.LineWrapMode.WidgetWidth if wrapping else QPlainTextEdit.LineWrapMode.NoWrap)
        button=getattr(self,'wrap_button',None)
        if button:
            button.setChecked(wrapping);button.setToolTip('Wrap lines: '+('On' if wrapping else 'Off'))
    def wrap_control(self):
        button=tool('wrap','Toggle line wrapping',self.toggle_wrap)
        if USE_REQON_UI:
            button.setObjectName('wrapLines');button.setCheckable(True)
            button.setChecked(self.lineWrapMode()!=QPlainTextEdit.LineWrapMode.NoWrap)
            button.setToolTip('Wrap lines: '+('On' if button.isChecked() else 'Off'))
            button.setAccessibleName('Wrap lines');self.wrap_button=button
        return button


class PlaceholderDelegate(QStyledItemDelegate):
    def createEditor(self,parent,option,index):
        editor=super().createEditor(parent,option,index)
        # Keep the table model current while typing, without closing/recreating the editor.
        if isinstance(editor,QLineEdit):
            editor.textEdited.connect(lambda _:self.commitData.emit(editor))
            if hasattr(self.parent(),'variables_provider'):
                from ui_editor_behavior import VariableCompletion
                editor.completion=VariableCompletion(editor,self.parent().variables_provider)
        return editor
    def paint(self,painter,option,index):
        active=USE_REQON_UI and index.row()==self.parent().currentRow()
        if active:option.backgroundBrush=QColor(C['line'])
        if index.column()==0:
            painter.save()
            painter.fillRect(option.rect,QColor(C['line'] if active else color('#212121')))
            if index.data(Qt.ItemDataRole.CheckStateRole) is not None:
                box=QRect(option.rect.center().x()-7,option.rect.center().y()-7,14,14)
                checked=index.data(Qt.ItemDataRole.CheckStateRole)==Qt.CheckState.Checked.value
                painter.setPen(QColor('#bfbfbf')); painter.setBrush(QColor('#f5f5f5') if checked else QColor('#212121'))
                painter.drawRoundedRect(box,2,2)
                if checked:icon('check','#333333',14).paint(painter,box)
            painter.restore();return
        super().paint(painter,option,index)
        if index.column()>0 and not index.data() and index.row()==index.model().rowCount()-1:
            painter.save(); painter.setPen(QColor('#707070'))
            painter.drawText(option.rect.adjusted(8,0,0,0),Qt.AlignmentFlag.AlignVCenter,['','Key','Value','Description'][index.column()]); painter.restore()


class KeyValueTable(QWidget):
    changed = Signal()
    def __init__(self, data=None, readonly=False):
        super().__init__(); self.setObjectName('kvPanel'); self.readonly=readonly
        layout=QVBoxLayout(self); layout.setContentsMargins(16,8,16,0); layout.setSpacing(0)
        self.table=QTableWidget(0,4); self.table.setObjectName('kvTable')
        self.table.setHorizontalHeaderLabels(['','Key','Value','Description'])
        self.table.verticalHeader().hide(); self.table.verticalHeader().setDefaultSectionSize(33)
        self.table.setSelectionBehavior(QAbstractItemView.SelectionBehavior.SelectItems)
        self.table.setItemDelegate(PlaceholderDelegate(self.table)); self.table.setWordWrap(False)
        if USE_REQON_UI:self.table.currentCellChanged.connect(lambda *_:self.table.viewport().update())
        header=self.table.horizontalHeader(); header.setMinimumSectionSize(36)
        header.setFixedHeight(33); header.setDefaultAlignment(Qt.AlignmentFlag.AlignLeft|Qt.AlignmentFlag.AlignVCenter)
        header.setSectionResizeMode(0,QHeaderView.ResizeMode.Fixed); self.table.setColumnWidth(0,42)
        for col in (1,2,3): header.setSectionResizeMode(col,QHeaderView.ResizeMode.Stretch)
        self.table.setVerticalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAsNeeded)
        if readonly: self.table.setEditTriggers(QAbstractItemView.EditTrigger.NoEditTriggers)
        layout.addWidget(self.table); layout.addStretch(1)
        self.table.cellChanged.connect(self.on_change); self.set_rows(data or [])

    def set_rows(self,data):
        table=self.table; table.blockSignals(True); table.setRowCount(0)
        for values in list(data)+([] if self.readonly else [['','','',True]]):
            row=table.rowCount(); table.insertRow(row)
            check=QTableWidgetItem(); check.setFlags(Qt.ItemFlag.ItemIsEnabled | (Qt.ItemFlag.ItemIsUserCheckable if not self.readonly else Qt.ItemFlag.NoItemFlags))
            if values[0]:check.setCheckState(Qt.CheckState.Checked if len(values)<4 or values[3] else Qt.CheckState.Unchecked)
            table.setItem(row,0,check)
            for col in range(3): table.setItem(row,col+1,QTableWidgetItem(str(values[col]) if col<len(values) else ''))
        table.blockSignals(False); self.fit_height()
        self.changed.emit()
    def fit_height(self): self.table.setFixedHeight(min(340, self.table.rowCount()*33+35))
    def on_change(self,row,col):
        if not self.readonly and row==self.table.rowCount()-1 and col>0 and self.table.item(row,col).text():
            self.table.blockSignals(True)
            self.table.item(row,0).setCheckState(Qt.CheckState.Checked)
            new=row+1; self.table.insertRow(new)
            check=QTableWidgetItem(); check.setFlags(Qt.ItemFlag.ItemIsEnabled|Qt.ItemFlag.ItemIsUserCheckable); self.table.setItem(new,0,check)
            for c in (1,2,3):self.table.setItem(new,c,QTableWidgetItem(''))
            self.table.blockSignals(False); self.fit_height()
        self.changed.emit()
    def all_rows(self):
        result=[]
        for row in range(self.table.rowCount()):
            values=[self.table.item(row,col).text() if self.table.item(row,col) else '' for col in (1,2,3)]
            if values[0].strip(): result.append(values+[self.table.item(row,0).checkState()==Qt.CheckState.Checked])
        return result
    def active_rows(self): return [r[:3] for r in self.all_rows() if r[3]]
    def bulk_edit(self):
        dialog=QDialog(self); dialog.setWindowTitle('Bulk Edit'); dialog.resize(620,400)
        layout=QVBoxLayout(dialog); layout.addWidget(QLabel('One key:value pair per line'))
        editor=CodeEditor('\n'.join(f'{r[0]}:{r[1]}' for r in self.all_rows())); layout.addWidget(editor)
        apply=QPushButton('Apply'); apply.clicked.connect(dialog.accept); layout.addWidget(apply)
        if dialog.exec():
            data=[]
            for line in editor.toPlainText().splitlines():
                key,sep,value=line.partition(':')
                if key.strip(): data.append([key.strip(),value.strip(),''])
            self.set_rows(data); self.changed.emit()
