"""Non-destructive inline JSON feedback for the existing native text editor."""
from ui_theming import theme_qcolor
import json
from concurrent.futures import ThreadPoolExecutor
from json_comments import loads,looks_like_json,format_json
from PySide6.QtCore import QObject,QTimer,Qt
from PySide6.QtGui import QColor,QTextCursor,QTextFormat,QTextCharFormat
from PySide6.QtWidgets import QLabel,QTextEdit
from ui_brand import COLORS as C

_validation_pool=ThreadPoolExecutor(max_workers=1,thread_name_prefix='reqon-json')


def _validate_text(text):
    try:loads(text)
    except json.JSONDecodeError as error:return error
    return None


class EditorPresentation(QObject):
    def __init__(self,editor):
        super().__init__(editor);self.editor=editor;self.mode='auto';self.error=None
        self.banner=QLabel(editor);self.banner.setObjectName('editorValidation');self.banner.setTextFormat(Qt.TextFormat.PlainText);self.banner.hide()
        self.timer=QTimer(self);self.timer.setInterval(280);self.timer.setSingleShot(True);self.timer.timeout.connect(self.validate)
        self.poll=QTimer(self);self.poll.setInterval(25);self.poll.timeout.connect(self.validation_ready)
        self._future=None;self._validated_key=None;self._error_position=0
        editor.textChanged.connect(self.changed);editor.cursorPositionChanged.connect(self.highlight)
        self.highlight();self.timer.start()
    @property
    def bottom_margin(self):return 28 if self.error else 0
    def changed(self):
        if self._future:self._future.cancel();self._future=None
        self.poll.stop();self._validated_key=None
        self.error=None;self.banner.hide();self.editor.update_margin();self.highlight();self.timer.start()
    def validate(self,force=False):
        if self.editor.isReadOnly():return
        from ui_editor_behavior import settings
        key=(self.editor.document().revision(),self.mode,settings().request_validation)
        if not force and key==self._validated_key:return
        text=self.editor.toPlainText();self.error=None
        if text.strip() and (force or settings().request_validation) and (force or self.mode=='json' or self.mode!='text' and looks_like_json(text)):
            if not force and len(text)>65536:
                if self._future:self._future.cancel()
                self._future=_validation_pool.submit(_validate_text,text)
                self._pending_key=key;self.poll.start();return
            self.error=_validate_text(text)
        self._validated_key=key
        self.show_validation(text)
    def validation_ready(self):
        if self._future is None or not self._future.done():return
        future=self._future;self._future=None;self.poll.stop()
        from ui_editor_behavior import settings
        key=(self.editor.document().revision(),self.mode,settings().request_validation)
        if self.editor.isReadOnly() or key!=self._pending_key:return
        self.error=future.result();self._validated_key=key
        self.show_validation(self.editor.toPlainText())
    def show_validation(self,text):
        if self.error:self._error_position=len(text[:self.error.pos].encode('utf-16-le'))//2
        self.banner.setVisible(self.error is not None);self.editor.update_margin();self.layout_banner();self.highlight()
    def layout_banner(self):
        if not self.error:return
        editor=self.editor;scroll=editor.horizontalScrollBar();bottom=scroll.height() if scroll.isVisible() else 0
        self.banner.setGeometry(8,editor.height()-bottom-28,max(20,editor.width()-16),24)
        full=f'Invalid JSON · line {self.error.lineno}, column {self.error.colno}: {self.error.msg}'
        self.banner.setText(self.banner.fontMetrics().elidedText(full,Qt.TextElideMode.ElideRight,max(0,self.banner.width()-16)))
        self.banner.setToolTip(full);self.banner.setAccessibleName(full);self.banner.raise_()
    def highlight(self):
        editor=self.editor;selections=[]
        if not editor.isReadOnly():
            line=QTextEdit.ExtraSelection();line.cursor=editor.textCursor();line.cursor.clearSelection()
            line.format.setBackground(theme_qcolor(C['line']));line.format.setProperty(QTextFormat.Property.FullWidthSelection,True);selections.append(line)
        if self.error:
            mark=QTextEdit.ExtraSelection();mark.cursor=QTextCursor(editor.document())
            position=self._error_position
            mark.cursor.setPosition(max(0,min(position,editor.document().characterCount()-2)))
            mark.cursor.movePosition(QTextCursor.MoveOperation.Right,QTextCursor.MoveMode.KeepAnchor)
            mark.format.setUnderlineStyle(QTextCharFormat.UnderlineStyle.WaveUnderline);mark.format.setUnderlineColor(theme_qcolor(C['error']));selections.append(mark)
        editor.setExtraSelections(selections)
    def format_json(self):
        self.timer.stop()
        from ui_editor_behavior import settings
        try:formatted=format_json(self.editor.toPlainText(),settings().indent)
        except json.JSONDecodeError:self.validate(True);return False
        cursor=self.editor.textCursor();cursor.beginEditBlock();cursor.select(QTextCursor.SelectionType.Document);cursor.insertText(formatted);cursor.endEditBlock()
        self.editor.setTextCursor(cursor);self.validate();return True
