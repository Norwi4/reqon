"""Live native code-snippet sidebar."""
from ui_theming import set_style,theme_qcolor
from ui_brand import stylesheet
from PySide6.QtCore import Qt, QTimer, Signal
from PySide6.QtGui import QColor, QSyntaxHighlighter, QTextCharFormat, QTextOption
from PySide6.QtWidgets import QFrame, QVBoxLayout, QHBoxLayout, QLabel, QApplication, QPlainTextEdit
from ui_components import CodeEditor, tool
from code_snippets import generate_snippet


class ShellSyntax(QSyntaxHighlighter):
    def highlightBlock(self,text):
        quoted=self.previousBlockState()==1; start=0
        fmt=QTextCharFormat(); fmt.setForeground(theme_qcolor('#e6dc8e'))
        for i,char in enumerate(text):
            if char=="'":
                if quoted:self.setFormat(start,i-start+1,fmt)
                else:start=i
                quoted=not quoted
        if quoted:self.setFormat(start,len(text)-start,fmt)
        self.setCurrentBlockState(1 if quoted else 0)


class CodeSnippetPanel(QFrame):
    close_requested=Signal()

    def __init__(self,window):
        super().__init__(window); self.window=window; self.setObjectName('snippetPanel')
        self.setMinimumWidth(300); self.setMaximumWidth(560)
        layout=QVBoxLayout(self); layout.setContentsMargins(16,0,10,12); layout.setSpacing(0)
        head=QHBoxLayout(); title=QLabel('Code snippet'); title.setObjectName('snippetTitle')
        head.addWidget(title); head.addStretch(); self.close_button=tool('close','Close code snippet',self.close_requested.emit)
        head.addWidget(self.close_button); head.setContentsMargins(0,8,0,8); layout.addLayout(head)
        bar=QHBoxLayout(); bar.setContentsMargins(12,9,4,9)
        self.language=QLabel('gRPCurl'); self.language.setObjectName('muted'); bar.addWidget(self.language); bar.addStretch()
        self.copy_button=tool('copy','Copy code snippet',self.copy); bar.addWidget(self.copy_button); layout.addLayout(bar)
        self.editor=CodeEditor(); self.editor.setLineWrapMode(QPlainTextEdit.LineWrapMode.WidgetWidth)
        if hasattr(self.editor,'presentation'):self.editor.presentation.mode='text'
        self.editor.setWordWrapMode(QTextOption.WrapMode.WrapAnywhere)
        self.editor.highlighter.setDocument(None); self.editor.highlighter=ShellSyntax(self.editor.document())
        set_style(self.editor,stylesheet('QPlainTextEdit#codeEditor { font-size:12px; background:#262626; }'))
        self.editor.gutter_color='#262626'
        layout.addWidget(self.editor); layout.addStretch(1)
        self.message=QLabel(); self.message.setWordWrap(True); self.message.setObjectName('muted'); layout.addWidget(self.message)
        self.timer=QTimer(self); self.timer.setSingleShot(True); self.timer.setInterval(180); self.timer.timeout.connect(self.refresh)
        self.fit_timer=QTimer(self); self.fit_timer.setSingleShot(True); self.fit_timer.timeout.connect(self.fit_editor)
        self.command='';self._setting_text=False;self._applying=False;self._dirty=False;self._editing_page=None;self._drafts={}
        self.apply_timer=QTimer(self);self.apply_timer.setSingleShot(True);self.apply_timer.setInterval(650);self.apply_timer.timeout.connect(self.apply_edit)
        self.editor.textChanged.connect(self.text_edited);self.hide()

    def schedule(self,*_):
        if self.isVisible() and not self._applying and not getattr(self.window,'_applying_command',False):self.timer.start()

    def text_edited(self):
        if self._setting_text:return
        self._dirty=True;self._editing_page=self.window.current_page();self.command=self.editor.toPlainText();self.copy_button.setEnabled(bool(self.command));self.message.setText('Editing…');self.apply_timer.start();self.fit_timer.start(0)

    def apply_edit(self):
        page=self._editing_page
        if not self._dirty or page is None or page is not self.window.current_page():return
        try:
            from command_import import parse_command
            imported=parse_command(self.editor.toPlainText())
            self._applying=True;self._editing_page=self.window.apply_command_to_page(page,imported);self._dirty=False
            self.language.setText('gRPCurl' if imported.data.get('kind')=='grpc' else 'cURL');self.message.setText('Changes applied'+(' · '+' '.join(imported.notes) if imported.notes else ''))
            self._drafts.pop(self._editing_page._draft_id,None);self.timer.stop()
        except ValueError as error:self.message.setText(str(error))
        finally:self._applying=False

    def refresh(self,*_):
        self.timer.stop()
        if not self.isVisible():return
        page=self.window.current_page()
        if self._applying:return
        if self._dirty and page is self._editing_page:return
        if self._dirty and self._editing_page is not None:
            self._drafts[getattr(self._editing_page,'_draft_id','')]=self.editor.toPlainText()
        self.apply_timer.stop();self._editing_page=page;self._dirty=False
        self._setting_text=True
        try:self.display_page(page)
        finally:self._setting_text=False

    def display_page(self,page):
        if page is None:
            self.command=''; self.editor.clear(); self.message.setText('Open an HTTP or gRPC request to generate a command.'); self.copy_button.setEnabled(False); return
        self.language.setText('gRPCurl' if page.kind=='grpc' else 'cURL')
        draft=self._drafts.get(getattr(page,'_draft_id',''))
        if draft is not None:
            self.editor.setPlainText(draft);self.command=draft;self._dirty=True;self.message.setText('Complete the command to apply your changes.');self.copy_button.setEnabled(bool(draft));return
        try:
            label,command=generate_snippet(page.runtime_data_provider(),self.window.variables(page.collection_id))
            self.language.setText(label); self.command=command
            if self.editor.toPlainText()!=command:self.editor.setPlainText(command)
            self.fit_timer.start(0)
            self.message.setText('Edit the command to update this request automatically.'); self.copy_button.setEnabled(True)
        except ValueError as error:
            self.command=''; self.editor.clear(); self.message.setText(str(error)); self.copy_button.setEnabled(False)

    def fit_editor(self):
        height=16; block=self.editor.document().firstBlock()
        while block.isValid():
            height+=self.editor.blockBoundingRect(block).height();block=block.next()
        self.editor.setFixedHeight(min(max(80,int(height)),max(80,self.height()-130)))

    def resizeEvent(self,event):
        super().resizeEvent(event)
        if hasattr(self,'fit_timer'):self.fit_timer.start(0)

    def copy(self):
        if self.editor.toPlainText():
            QApplication.clipboard().setText(self.editor.toPlainText()); self.message.setText('Copied to clipboard')
