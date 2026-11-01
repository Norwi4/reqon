"""Local rich-text / Markdown documentation backed by the same Markdown string."""
from PySide6.QtCore import Signal,QSignalBlocker
from PySide6.QtGui import QTextDocument,QTextCharFormat,QFont,QTextListFormat
from PySide6.QtWidgets import QWidget,QVBoxLayout,QHBoxLayout,QStackedWidget,QTextEdit,QToolButton
from ui_components import CodeEditor


class LocalDocument(QTextDocument):
    def loadResource(self,kind,url):return None  # Descriptions never fetch external resources.


class DocumentationEditor(QWidget):
    textChanged=Signal()
    def __init__(self,text):
        super().__init__();self.mode='';self.markdown=text;self.loading=False
        layout=QVBoxLayout(self);layout.setContentsMargins(0,0,0,0);layout.setSpacing(8)
        self.toolbar=QWidget();tools=QHBoxLayout(self.toolbar);tools.setContentsMargins(0,0,0,0)
        for label,callback in [('Bold',self.bold),('Italic',self.italic),('Bullet list',self.bullets)]:
            button=QToolButton();button.setText(label);button.clicked.connect(callback);tools.addWidget(button)
        tools.addStretch();layout.addWidget(self.toolbar)
        self.rich=QTextEdit();self.rich.setDocument(LocalDocument(self.rich));self.rich.setAcceptRichText(False)
        self.source=CodeEditor(text);self.source.presentation.mode='text'
        self.stack=QStackedWidget();self.stack.addWidget(self.rich);self.stack.addWidget(self.source);layout.addWidget(self.stack)
        self.rich.textChanged.connect(lambda:self.changed(True));self.source.textChanged.connect(lambda:self.changed(False))
        self.set_mode('Reqon editor')
    def setPlaceholderText(self,text):self.rich.setPlaceholderText(text);self.source.setPlaceholderText(text)
    def setFrameShape(self,shape):self.rich.setFrameShape(shape);self.source.setFrameShape(shape)
    def toPlainText(self):return self.markdown
    def setPlainText(self,text):
        self.markdown=text;mode=self.mode;self.mode='';self.set_mode(mode)
    def set_mode(self,mode):
        if mode==self.mode:return
        self.loading=True;self.mode=mode;rich=mode=='Reqon editor'
        self.rich.setMarkdown(self.markdown);self.source.setPlainText(self.markdown);self.stack.setCurrentIndex(0 if rich else 1);self.toolbar.setVisible(rich);self.loading=False
    def changed(self,rich):
        if self.loading:return
        self.markdown=self.rich.toMarkdown() if rich else self.source.toPlainText();self.textChanged.emit()
    def bold(self):
        format=QTextCharFormat();format.setFontWeight(QFont.Weight.Normal if self.rich.fontWeight()>=QFont.Weight.Bold else QFont.Weight.Bold);self.rich.mergeCurrentCharFormat(format)
    def italic(self):
        format=QTextCharFormat();format.setFontItalic(not self.rich.fontItalic());self.rich.mergeCurrentCharFormat(format)
    def bullets(self):self.rich.textCursor().createList(QTextListFormat.Style.ListDisc)
