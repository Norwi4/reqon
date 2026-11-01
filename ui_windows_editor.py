"""Non-destructive inline JSON feedback for the existing native text editor."""
import json
from json_comments import loads,looks_like_json,format_json
from PySide6.QtCore import QObject,QTimer,Qt
from PySide6.QtGui import QColor,QTextCursor,QTextFormat,QTextCharFormat
from PySide6.QtWidgets import QLabel,QTextEdit
from ui_brand import COLORS as C


class EditorPresentation(QObject):
    def __init__(self,editor):
        super().__init__(editor);self.editor=editor;self.mode='auto';self.error=None
        self.banner=QLabel(editor);self.banner.setObjectName('editorValidation');self.banner.setTextFormat(Qt.TextFormat.PlainText);self.banner.hide()
        self.timer=QTimer(self);self.timer.setInterval(280);self.timer.setSingleShot(True);self.timer.timeout.connect(self.validate)
        editor.textChanged.connect(self.changed);editor.cursorPositionChanged.connect(self.highlight)
        self.highlight();self.timer.start()
    @property
    def bottom_margin(self):return 28 if self.error else 0
    def changed(self):
        self.error=None;self.banner.hide();self.editor.update_margin();self.highlight();self.timer.start()
    def validate(self,force=False):
        text=self.editor.toPlainText();self.error=None
        from ui_editor_behavior import settings
        if text.strip() and not self.editor.isReadOnly() and (force or settings().request_validation) and (force or self.mode=='json' or self.mode!='text' and looks_like_json(text)):
            try:loads(text)
            except json.JSONDecodeError as error:self.error=error
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
            line.format.setBackground(QColor(C['line']));line.format.setProperty(QTextFormat.Property.FullWidthSelection,True);selections.append(line)
        if self.error:
            mark=QTextEdit.ExtraSelection();mark.cursor=QTextCursor(editor.document())
            position=len(editor.toPlainText()[:self.error.pos].encode('utf-16-le'))//2
            mark.cursor.setPosition(max(0,min(position,editor.document().characterCount()-2)))
            mark.cursor.movePosition(QTextCursor.MoveOperation.Right,QTextCursor.MoveMode.KeepAnchor)
            mark.format.setUnderlineStyle(QTextCharFormat.UnderlineStyle.WaveUnderline);mark.format.setUnderlineColor(QColor(C['error']));selections.append(mark)
        editor.setExtraSelections(selections)
    def format_json(self):
        self.timer.stop()
        from ui_editor_behavior import settings
        try:formatted=format_json(self.editor.toPlainText(),settings().indent)
        except json.JSONDecodeError:self.validate(True);return False
        cursor=self.editor.textCursor();cursor.beginEditBlock();cursor.select(QTextCursor.SelectionType.Document);cursor.insertText(formatted);cursor.endEditBlock()
        self.editor.setTextCursor(cursor);self.validate();return True
