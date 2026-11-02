"""Native command import with an editable source and a preview before creating a tab."""
from ui_theming import set_style
from PySide6.QtCore import Qt,QTimer
from PySide6.QtWidgets import (QDialog,QVBoxLayout,QHBoxLayout,QLabel,QPushButton,QComboBox,
                               QPlainTextEdit,QApplication,QDialogButtonBox)
from command_import import parse_command
from ui_components import CodeEditor
from ui_snippets import ShellSyntax


class CommandImportDialog(QDialog):
    def __init__(self,window):
        super().__init__(window); self.window=window; self.imported=None; self.candidate=None
        self.setWindowTitle('Import cURL / gRPCurl'); self.resize(740,580); self.setMinimumSize(520,430)
        layout=QVBoxLayout(self); layout.setContentsMargins(24,20,24,20); layout.setSpacing(14)
        title=QLabel('Import request'); title.setObjectName('docsTitle'); layout.addWidget(title)
        help_text=QLabel('Paste a cURL or gRPCurl command. The request will open in a new tab.'); help_text.setWordWrap(True)
        help_text.setObjectName('muted'); layout.addWidget(help_text)
        controls=QHBoxLayout(); self.shell=QComboBox()
        for label,value in [('Auto-detect shell','auto'),('Bash / Linux / macOS','bash'),('Windows CMD','cmd'),('PowerShell','powershell')]:self.shell.addItem(label,value)
        controls.addWidget(self.shell); controls.addStretch()
        paste=QPushButton('Paste from clipboard'); paste.clicked.connect(lambda:self.editor.setPlainText(QApplication.clipboard().text()))
        controls.addWidget(paste); layout.addLayout(controls)
        self.editor=CodeEditor(); self.editor.setPlaceholderText('curl --request POST https://api.example.com/items \\\n  --header "Content-Type: application/json" \\\n  --data-raw \'{"name": "Example"}\'')
        self.editor.setLineWrapMode(QPlainTextEdit.LineWrapMode.NoWrap)
        self.editor.highlighter.setDocument(None); self.editor.highlighter=ShellSyntax(self.editor.document())
        layout.addWidget(self.editor,1)
        self.preview=QLabel(); self.preview.setWordWrap(True); self.preview.setTextFormat(Qt.TextFormat.PlainText); layout.addWidget(self.preview)
        self.feedback=QLabel(); self.feedback.setWordWrap(True); self.feedback.setTextFormat(Qt.TextFormat.PlainText); layout.addWidget(self.feedback)
        footer=QHBoxLayout(); file=QPushButton('Import collection file…'); file.clicked.connect(self.choose_file); footer.addWidget(file); footer.addStretch()
        buttons=QDialogButtonBox(QDialogButtonBox.StandardButton.Cancel)
        self.import_button=buttons.addButton('Import request',QDialogButtonBox.ButtonRole.AcceptRole); self.import_button.setObjectName('sendButton')
        self.import_button.setMinimumSize(126,32)
        buttons.rejected.connect(self.reject); buttons.accepted.connect(self.apply); footer.addWidget(buttons); layout.addLayout(footer)
        self.timer=QTimer(self); self.timer.setSingleShot(True); self.timer.setInterval(160); self.timer.timeout.connect(self.refresh)
        self.editor.textChanged.connect(self.schedule); self.shell.currentIndexChanged.connect(self.schedule)
        self.refresh(); self.editor.setFocus()

    def schedule(self,*_):
        self.import_button.setEnabled(False); self.timer.start()

    def refresh(self):
        self.timer.stop(); self.candidate=None; self.import_button.setEnabled(False)
        text=self.editor.toPlainText()
        if not text.strip():
            self.preview.clear(); self.feedback.setText('The command is parsed locally. Import does not send a request.'); self.feedback.setObjectName('muted'); set_style(self.feedback,''); return
        try:self.candidate=parse_command(text,self.shell.currentData())
        except ValueError as error:
            self.preview.clear(); self.feedback.setText(str(error)); set_style(self.feedback,'color:#e79591;'); return
        data=self.candidate.data
        if data['kind']=='grpc':
            summary=f"gRPC · {data['service']}/{data['rpc_method']}\n{data['server']} · {len(data['metadata'])} metadata · {'TLS' if data['tls'] else 'Plaintext'}"
        else:
            summary=f"{data['method']} · {data['url']}\n{len(data['params'])} parameters · {len(data['headers'])} headers · Body: {data['body_type']}"
        self.preview.setText(summary)
        self.feedback.setText('\n'.join(self.candidate.notes) or 'Ready to import. The new request will be saved as a local draft.')
        set_style(self.feedback,'color:#d3b47b;' if self.candidate.notes else '')
        self.import_button.setEnabled(True)

    def apply(self):
        self.refresh()
        if self.candidate:self.imported=self.candidate;self.accept()

    def choose_file(self):
        self.reject(); self.window.import_collection()
