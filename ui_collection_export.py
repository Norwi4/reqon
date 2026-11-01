"""Select entire collections for one portable export, with a hierarchy preview."""
from PySide6.QtCore import Qt
from PySide6.QtWidgets import (QDialog,QVBoxLayout,QHBoxLayout,QLabel,QPushButton,QTreeWidget,
                               QTreeWidgetItem,QDialogButtonBox,QAbstractItemView)
from ui_components import icon


class CollectionExportDialog(QDialog):
    def __init__(self,window):
        super().__init__(window);self.setWindowTitle('Export collections');self.resize(640,540);self.setMinimumSize(440,350)
        layout=QVBoxLayout(self);layout.setContentsMargins(24,20,24,20);layout.setSpacing(14)
        title=QLabel('Export collections');title.setObjectName('docsTitle');layout.addWidget(title)
        help_text=QLabel('Select collections to include in one file. All folders, requests and examples in each selected collection will be exported.')
        help_text.setWordWrap(True);help_text.setObjectName('muted');layout.addWidget(help_text)
        controls=QHBoxLayout()
        all_button=QPushButton('Select all');all_button.clicked.connect(lambda:self.select_all(True));controls.addWidget(all_button)
        none_button=QPushButton('Clear selection');none_button.clicked.connect(lambda:self.select_all(False));controls.addWidget(none_button)
        controls.addStretch();layout.addLayout(controls)
        self.tree=QTreeWidget();self.tree.setHeaderHidden(True);self.tree.setUniformRowHeights(True)
        self.tree.setEditTriggers(QAbstractItemView.EditTrigger.NoEditTriggers);layout.addWidget(self.tree,1)
        self.roots=[]
        def children(parent,cid,fid=None,request_id=None):
            for kind,id,name,*_ in window.store.children(cid,fid,request_id):
                item=QTreeWidgetItem(parent,[name]);item.setFlags(Qt.ItemFlag.ItemIsEnabled|Qt.ItemFlag.ItemIsSelectable)
                if kind=='request':
                    import json
                    data=json.loads(window.store.entity(kind,id)['data']);glyph='grpc' if data.get('kind')=='grpc' else 'http'
                else:glyph=kind
                item.setIcon(0,icon(glyph))
                if kind=='folder':children(item,cid,id)
                elif kind=='request':children(item,cid,request_id=id)
        for _,cid,name,*_ in window.store.children():
            item=QTreeWidgetItem(self.tree,[name]);item.setIcon(0,icon('collection'));item.setData(0,Qt.ItemDataRole.UserRole,cid)
            item.setFlags(Qt.ItemFlag.ItemIsEnabled|Qt.ItemFlag.ItemIsSelectable|Qt.ItemFlag.ItemIsUserCheckable)
            item.setCheckState(0,Qt.CheckState.Checked);self.roots.append(item);children(item,cid)
        self.summary=QLabel();self.summary.setObjectName('muted');layout.addWidget(self.summary)
        buttons=QDialogButtonBox(QDialogButtonBox.StandardButton.Cancel)
        self.export_button=buttons.addButton('Export selected',QDialogButtonBox.ButtonRole.AcceptRole)
        self.export_button.setObjectName('sendButton');self.export_button.setMinimumSize(130,32)
        buttons.accepted.connect(self.accept);buttons.rejected.connect(self.reject);layout.addWidget(buttons)
        self.tree.itemChanged.connect(self.update_selection);self.update_selection()

    def selected_ids(self):
        return [item.data(0,Qt.ItemDataRole.UserRole) for item in self.roots if item.checkState(0)==Qt.CheckState.Checked]

    def select_all(self,checked):
        self.tree.blockSignals(True)
        for item in self.roots:item.setCheckState(0,Qt.CheckState.Checked if checked else Qt.CheckState.Unchecked)
        self.tree.blockSignals(False);self.update_selection()

    def update_selection(self,*_):
        count=len(self.selected_ids());self.export_button.setEnabled(bool(count))
        self.summary.setText(f'{count} of {len(self.roots)} collections selected')

    def accept(self):
        if self.selected_ids():super().accept()
