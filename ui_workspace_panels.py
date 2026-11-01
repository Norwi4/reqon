"""Workspace information, notification and docked request console."""
from PySide6.QtCore import Qt,QTimer,QEvent
from PySide6.QtWidgets import QDialog,QFrame,QWidget,QVBoxLayout,QHBoxLayout,QLabel,QPushButton,QLineEdit,QPlainTextEdit,QStackedWidget,QApplication
from ui_components import tool
from ui_brand import VERSION,brand_icon


class InfoDialog(QDialog):
    def __init__(self,parent):
        super().__init__(parent);self.setWindowTitle('Reqon Info');self.setModal(True);self.setAttribute(Qt.WidgetAttribute.WA_DeleteOnClose);self.setFixedWidth(440)
        layout=QVBoxLayout(self);layout.setContentsMargins(28,28,28,24);layout.setSpacing(18)
        logo=QLabel();logo.setPixmap(brand_icon().pixmap(48,48));layout.addWidget(logo)
        title=QLabel('Reqon');title.setStyleSheet('font-size:24px;font-weight:600;');layout.addWidget(title)
        self.version_label=QLabel('Version '+VERSION);self.version_label.setObjectName('muted');layout.addWidget(self.version_label)
        text=QLabel('Something is not working as expected? Report a bug or suggest an improvement in the project’s GitHub Issues.');text.setWordWrap(True);layout.addWidget(text)
        self.issues_link=QLabel('<a href="https://github.com/Norwi4/reqon/issues" style="color:#3b82f6;text-decoration:none">Open GitHub Issues ↗</a>');self.issues_link.setOpenExternalLinks(True);layout.addWidget(self.issues_link)
        bottom=QHBoxLayout();bottom.addStretch();close=QPushButton('Close');close.clicked.connect(self.accept);bottom.addWidget(close);layout.addLayout(bottom)


class Toast(QFrame):
    def __init__(self,parent,title,message):
        super().__init__(parent);self.setObjectName('requestToast');self.setStyleSheet('QFrame#requestToast {background:#2b2b2b;border:1px solid #484848;border-radius:6px;}')
        self.setFixedWidth(min(370,parent.width()-32));layout=QVBoxLayout(self);layout.setContentsMargins(16,12,12,14);layout.setSpacing(8)
        head=QHBoxLayout();label=QLabel(title);label.setStyleSheet('font-weight:600;');head.addWidget(label);head.addStretch();head.addWidget(tool('close','Dismiss',self.deleteLater));layout.addLayout(head)
        label=QLabel(message);label.setWordWrap(True);label.setObjectName('muted');layout.addWidget(label);self.adjustSize()
        parent.installEventFilter(self);self.place();self.show();self.raise_();QTimer.singleShot(6000,self.deleteLater)
    def place(self):self.move(self.parentWidget().width()-self.width()-20,self.parentWidget().height()-self.height()-42)
    def eventFilter(self,obj,event):
        if event.type()==QEvent.Type.Resize:self.place()
        return False


def show_toast(window,title,message):
    from shiboken6 import isValid
    old=getattr(window,'request_toast',None)
    if old is not None and isValid(old):old.hide();old.deleteLater()
    window.request_toast=Toast(window,title,message)


class ConsolePanel(QFrame):
    def __init__(self,window):
        super().__init__();self.window=window;self.setObjectName('requestConsole');self.setMinimumHeight(130)
        layout=QVBoxLayout(self);layout.setContentsMargins(0,0,0,0);layout.setSpacing(0)
        bar=QWidget();bar.setFixedHeight(40);line=QHBoxLayout(bar);line.setContentsMargins(14,0,8,0);line.setSpacing(12)
        line.addWidget(QLabel('Console'));line.addStretch();self.search=QLineEdit();self.search.setPlaceholderText('Filter logs');self.search.setFixedWidth(190);self.search.textChanged.connect(self.refresh);line.addWidget(self.search)
        clear=QPushButton('Clear');clear.setFlat(True);clear.clicked.connect(self.clear);line.addWidget(clear)
        line.addWidget(tool('copy','Copy console',lambda:QApplication.clipboard().setText(self.output.toPlainText())))
        self.expand=tool('maximize','Expand console',self.toggle_expand);self.expand.setCheckable(True);line.addWidget(self.expand);line.addWidget(tool('close','Close console',window.show_console));layout.addWidget(bar)
        self.stack=QStackedWidget();layout.addWidget(self.stack,1);empty=QWidget();center=QVBoxLayout(empty);center.addStretch()
        title=QLabel('No logs yet');title.setAlignment(Qt.AlignmentFlag.AlignCenter);title.setStyleSheet('font-size:18px;font-weight:600;');center.addWidget(title)
        hint=QLabel('Send a request to view its details in the console.');hint.setObjectName('muted');hint.setAlignment(Qt.AlignmentFlag.AlignCenter);center.addWidget(hint);center.addStretch();self.stack.addWidget(empty)
        self.output=QPlainTextEdit();self.output.setReadOnly(True);self.output.setStyleSheet('border:0;border-top:1px solid #363636;padding:8px;font-family:monospace;');self.stack.addWidget(self.output);self.hide()
    def refresh(self,*_):
        query=self.search.text().casefold();entries=self.window.console_entries
        text='\n'.join(entry for entry in entries if query in entry.casefold());bar=self.output.verticalScrollBar();at_end=bar.value()>=bar.maximum();position=bar.value()
        self.output.setPlainText(text or ('No matching logs.' if entries else ''));self.stack.setCurrentIndex(1 if entries else 0);bar.setValue(bar.maximum() if at_end else position)
    def clear(self):self.window.console_entries.clear();self.refresh()
    def toggle_expand(self):
        splitter=self.window.console_splitter
        if self.expand.isChecked():self._sizes=splitter.sizes();splitter.setSizes([200,max(300,splitter.height()-200)])
        else:splitter.setSizes(getattr(self,'_sizes',[600,280]))
