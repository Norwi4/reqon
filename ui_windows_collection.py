"""Windows collection overview: description and a compact facts sidebar."""
from ui_theming import set_style
import json
from PySide6.QtCore import Qt,QSize,QSignalBlocker
from PySide6.QtWidgets import QWidget,QVBoxLayout,QHBoxLayout,QLabel,QLineEdit,QPlainTextEdit,QTabWidget,QPushButton,QMenu,QSizePolicy,QFrame
from ui_components import KeyValueTable,tool,icon
from ui_brand import COLORS as C


class CollectionNameEdit(QLineEdit):
    def __init__(self):
        super().__init__();self.setObjectName('collectionName');self.setAccessibleName('Collection name')
        self.setMinimumWidth(100);self.setMaximumWidth(520);self.setFixedHeight(26)
        self.textChanged.connect(lambda *_:self.updateGeometry())
    def sizeHint(self):return QSize(min(520,max(100,self.fontMetrics().horizontalAdvance(self.text())+18)),26)


def build_collection_page(page,window,cid):
    layout=QVBoxLayout(page);layout.setContentsMargins(16,8,16,16);layout.setSpacing(4)
    head=QHBoxLayout();head.setContentsMargins(0,0,0,8);head.setSpacing(8)
    page.title=CollectionNameEdit()
    set_style(page.title,f'QLineEdit {{font-size:12px;font-weight:600;background:transparent;border:1px solid transparent;padding:0 6px;}} QLineEdit:focus {{border-color:{C["focus"]};}}')
    def rename(text):
        name=text.strip()
        if not name:return
        window.store.rename_entity('collection',cid,name)
        item=window.tree_item('collection',cid)
        if item:
            with QSignalBlocker(window.tree):item.setText(0,name);item.setToolTip(0,name)
        index=window.workbench.indexOf(page)
        if index>=0:window.workbench.setTabText(index,name)
        page.title.setToolTip(name)
    page.title.textEdited.connect(rename)
    def finish_name_edit():
        records=window.store.all('SELECT name FROM collections WHERE id=?',(cid,))
        if records and page.title.text()!=records[0][0]:page.title.setText(records[0][0])
    page.title.editingFinished.connect(finish_name_edit)
    head.addWidget(page.title);head.addStretch(1)
    export=QPushButton('Export');export.clicked.connect(lambda:window.export_collection(cid));head.addWidget(export)
    more=tool('more','Collection actions');menu=QMenu(more)
    for title,callback in [('Add HTTP request',lambda:window.add_tree_request(cid,None,'http')),
            ('Add gRPC request',lambda:window.add_tree_request(cid,None,'grpc')),('Add folder',lambda:window.add_folder(cid))]:menu.addAction(title,callback)
    more.setMenu(menu);more.setPopupMode(more.ToolButtonPopupMode.InstantPopup);head.addWidget(more);layout.addLayout(head)
    tabs=QTabWidget();tabs.setDocumentMode(True);tabs.setObjectName('editorTabs');tabs.tabBar().setDrawBase(False)
    set_style(tabs,'QTabWidget::pane {border:0;}');layout.addWidget(tabs,1)
    overview=QWidget();body=QHBoxLayout(overview);body.setContentsMargins(16,24,8,8);body.setSpacing(24)
    record=window.store.entity('collection',cid)
    from ui_documentation import DocumentationEditor
    page.description=DocumentationEditor(record['description']);page.description.setPlaceholderText('Add collection description')
    set_style(page.description,f'QPlainTextEdit {{border:0;background:{C["canvas"]};padding:0;color:{C["text"]};}}')
    page.description.setFrameShape(QFrame.Shape.NoFrame);page.description.textChanged.connect(page.save_description);body.addWidget(page.description,1)
    info=QWidget();info.setFixedWidth(190);facts=QVBoxLayout(info);facts.setContentsMargins(0,4,0,0);facts.setSpacing(12)
    page.count=QLabel();set_style(page.count,'font-size:12px;');facts.addWidget(page.count)
    page.protocol_counts=QLabel();page.protocol_counts.setWordWrap(True);page.protocol_counts.setObjectName('muted');facts.addWidget(page.protocol_counts)
    line=QFrame();line.setFrameShape(QFrame.Shape.HLine);set_style(line,f'color:{C["border"]};');facts.addWidget(line)
    label=QLabel('Summary');label.setObjectName('muted');facts.addWidget(label)
    page.summary=QLabel();page.summary.setTextFormat(Qt.TextFormat.PlainText);page.summary.setWordWrap(True);page.summary.setObjectName('muted');facts.addWidget(page.summary)
    def update_summary():
        text=page.description.toPlainText().strip();page.summary.setText((text[:160]+'…' if len(text)>160 else text) or 'Add a description to summarize this collection.')
    page.description.textChanged.connect(update_summary);update_summary();facts.addStretch();body.addWidget(info)
    tabs.addTab(overview,'Overview')
    page.variables=KeyValueTable();page.variables.set_rows([[key,str(value),''] for key,value in json.loads(record['variables']).items()])
    page.variables.changed.connect(page.save_variables);tabs.addTab(page.variables,'Variables')
    page.refresh();page._saved=page.collect()
