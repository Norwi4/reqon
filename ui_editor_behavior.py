"""Native editor keyboard behavior and {{variable}} completion."""
import re
from PySide6.QtCore import QObject,QEvent,QTimer,Qt,QStringListModel
from PySide6.QtGui import QTextCursor
from PySide6.QtWidgets import QApplication,QCompleter,QLineEdit
from general_settings import GeneralConfig


def settings():return getattr(QApplication.instance(),'reqon_general',GeneralConfig())


class VariableCompletion(QObject):
    def __init__(self,editor,provider):
        super().__init__(editor);self.editor=editor;self.provider=provider
        self.completer=QCompleter(self);self.model=QStringListModel(self);self.completer.setModel(self.model)
        self.completer.setWidget(editor);self.completer.setCaseSensitivity(Qt.CaseSensitivity.CaseInsensitive)
        self.completer.setCompletionMode(QCompleter.CompletionMode.PopupCompletion);self.completer.activated[str].connect(self.insert)
        editor.installEventFilter(self)
    def prefix(self):
        editor=self.editor
        if isinstance(editor,QLineEdit):text=editor.text()[:editor.cursorPosition()]
        else:
            cursor=editor.textCursor();cursor.movePosition(QTextCursor.MoveOperation.Start,QTextCursor.MoveMode.KeepAnchor);text=cursor.selectedText()
        match=re.search(r'\{\{\s*([^{}\s]*)$',text)
        return match.group(1) if match else None
    def refresh(self):
        prefix=self.prefix()
        if not settings().autocomplete or prefix is None or not self.editor.hasFocus():self.completer.popup().hide();return
        self.model.setStringList(sorted(self.provider()));self.completer.setCompletionPrefix(prefix)
        if self.completer.completionCount():self.completer.complete()
        else:self.completer.popup().hide()
    def insert(self,value):
        prefix=self.prefix()
        if prefix is None:return
        editor=self.editor
        if isinstance(editor,QLineEdit):
            position=editor.cursorPosition();after=editor.text()[position:];editor.setSelection(position-len(prefix),len(prefix));editor.insert(value+('' if after.startswith('}}') else '}}'))
        else:
            cursor=editor.textCursor();after=editor.toPlainText().encode('utf-16-le')[cursor.position()*2:].decode('utf-16-le')
            cursor.movePosition(QTextCursor.MoveOperation.Left,QTextCursor.MoveMode.KeepAnchor,len(prefix));cursor.insertText(value+('' if after.startswith('}}') else '}}'));editor.setTextCursor(cursor)
    def eventFilter(self,watched,event):
        if event.type()==QEvent.Type.KeyPress:
            if self.completer.popup().isVisible() and event.key() in (Qt.Key.Key_Enter,Qt.Key.Key_Return,Qt.Key.Key_Tab,Qt.Key.Key_Escape):
                event.ignore();return False
            QTimer.singleShot(0,self.refresh)
        return False


def editor_key(editor,event):
    if editor.isReadOnly():return False
    config=settings();cursor=editor.textCursor();key=event.key();text=event.text()
    if event.modifiers() & (Qt.KeyboardModifier.ControlModifier|Qt.KeyboardModifier.AltModifier):return False
    if key in (Qt.Key.Key_Tab,Qt.Key.Key_Backtab):
        if getattr(editor,'completion',None) and editor.completion.completer.popup().isVisible():return False
        unindent=key==Qt.Key.Key_Backtab or bool(event.modifiers()&Qt.KeyboardModifier.ShiftModifier)
        if cursor.hasSelection() or unindent:
            start,end=cursor.selectionStart(),cursor.selectionEnd();document=editor.document()
            first=document.findBlock(start).blockNumber();last=document.findBlock(max(start,end-1)).blockNumber()
            cursor.beginEditBlock()
            for number in range(last,first-1,-1):
                block=document.findBlockByNumber(number);line=QTextCursor(block)
                if unindent:
                    length=1 if block.text().startswith('\t') else min(config.indent_count,len(block.text())-len(block.text().lstrip(' ')))
                    line.movePosition(QTextCursor.MoveOperation.Right,QTextCursor.MoveMode.KeepAnchor,length);line.removeSelectedText()
                else:line.insertText(config.indent)
            cursor.endEditBlock()
        else:cursor.insertText(config.indent)
        return True
    pairs={**({'{':'}','[':']','(':')'} if config.close_brackets else {}),**({'"':'"',"'":"'"} if config.close_quotes else {})}
    position=cursor.position();document=editor.document();next_char=document.characterAt(position);previous=document.characterAt(position-1) if position else ''
    if text in ('}',']',')','"',"'") and text==next_char and ((text in '}])' and config.close_brackets) or (text in '\"\'' and config.close_quotes)):
        cursor.movePosition(QTextCursor.MoveOperation.Right);editor.setTextCursor(cursor);return True
    if text in pairs:
        if text in ('"',"'") and (previous=='\\' or previous.isalnum()):return False
        selected=cursor.selectedText();cursor.beginEditBlock();cursor.insertText(text+selected+pairs[text]);cursor.movePosition(QTextCursor.MoveOperation.Left);cursor.endEditBlock();editor.setTextCursor(cursor);return True
    if key in (Qt.Key.Key_Return,Qt.Key.Key_Enter):
        before=cursor.block().text()[:cursor.positionInBlock()];indent=re.match(r'[ \t]*',before).group();extra=config.indent if before.rstrip().endswith(('{','[','(')) else ''
        cursor.beginEditBlock();cursor.insertText('\n'+indent+extra)
        if extra and next_char in '}])':
            save=cursor.position();cursor.insertText('\n'+indent);cursor.setPosition(save)
        cursor.endEditBlock();editor.setTextCursor(cursor);return True
    return False
