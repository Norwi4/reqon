"""Reveal generated headers while keeping them separate from editable user rows."""
from PySide6.QtCore import Qt,Signal,QTimer
from PySide6.QtWidgets import QWidget,QVBoxLayout,QHBoxLayout,QLabel,QTableWidget,QTableWidgetItem,QHeaderView,QAbstractItemView
from ui_components import KeyValueTable,tool
from http_features import preview_headers
from shiboken6 import isValid


class HeadersPanel(QWidget):
    changed=Signal()
    def __init__(self,page,data):
        super().__init__();self.page=page;self.disabled=set(data.get('disabled_headers',[]));self.expanded=False
        layout=QVBoxLayout(self);layout.setContentsMargins(16,8,16,0);layout.setSpacing(0)
        bar=QHBoxLayout();bar.setContentsMargins(0,0,0,8);bar.addWidget(QLabel('Headers'))
        self.toggle=tool('eye','Show auto-generated headers',self.toggle_generated);bar.addWidget(self.toggle);bar.addStretch()
        self.toggle.setToolButtonStyle(Qt.ToolButtonStyle.ToolButtonTextBesideIcon)
        self.headers=KeyValueTable(data.get('header_rows',data.get('headers',[])));self.headers.layout().setContentsMargins(0,0,0,0)
        bar.addWidget(tool('docs','Bulk edit headers',self.headers.bulk_edit,'Bulk Edit'));layout.addLayout(bar)
        self.generated=QTableWidget(0,4);self.generated.setObjectName('kvTable');self.generated.setHorizontalHeaderLabels(['','Key','Value','Description']);self.generated.verticalHeader().hide();self.generated.verticalHeader().setDefaultSectionSize(33)
        self.generated.setEditTriggers(QAbstractItemView.EditTrigger.NoEditTriggers);self.generated.setWordWrap(False)
        header=self.generated.horizontalHeader();header.setFixedHeight(33);header.setDefaultAlignment(Qt.AlignmentFlag.AlignLeft|Qt.AlignmentFlag.AlignVCenter);header.setSectionResizeMode(0,QHeaderView.ResizeMode.Fixed);self.generated.setColumnWidth(0,42)
        for col in (1,2,3):header.setSectionResizeMode(col,QHeaderView.ResizeMode.Stretch)
        layout.addWidget(self.generated);layout.addWidget(self.headers,1);self.generated.hide()
        self.generated.cellChanged.connect(self.toggle_header);self.headers.changed.connect(self.changed)
        self.timer=QTimer(self);self.timer.setSingleShot(True);self.timer.setInterval(180);self.timer.timeout.connect(self.refresh)
    def schedule(self,*_):self.timer.start()
    def toggle_generated(self):
        self.expanded=not self.expanded;self.refresh()
    def toggle_header(self,row,column):
        if column!=0:return
        name=self.generated.item(row,1).text().lower()
        if self.generated.item(row,0).checkState()==Qt.CheckState.Checked:self.disabled.discard(name)
        else:self.disabled.add(name)
        self.changed.emit()
    def refresh(self):
        if not isValid(self.page):return
        if not hasattr(self.page,'http_schema'):return
        if hasattr(self.page.window(),'store') and not self.page.window().isVisible():return
        data=self.page.runtime_data_provider() if hasattr(self.page,'runtime_data_provider') else self.page.collect()
        values=preview_headers(data,self.page.variables_provider() if hasattr(self.page,'variables_provider') else {})
        table=self.generated;table.blockSignals(True);table.setRowCount(len(values))
        for row,(name,value) in enumerate(values.items()):
            check=QTableWidgetItem();check.setFlags(Qt.ItemFlag.ItemIsEnabled|(Qt.ItemFlag.NoItemFlags if name.lower() in ('host','content-length') else Qt.ItemFlag.ItemIsUserCheckable));check.setCheckState(Qt.CheckState.Unchecked if name.lower() in self.disabled else Qt.CheckState.Checked);table.setItem(row,0,check)
            for col,text in [(1,name),(2,value),(3,'Calculated automatically')]:
                item=QTableWidgetItem(text);item.setToolTip(text);table.setItem(row,col,item)
        table.blockSignals(False);table.setFixedHeight(35+33*len(values));table.setVisible(self.expanded and bool(values))
        self.headers.table.horizontalHeader().setVisible(not self.expanded);self.headers.fit_height()
        self.toggle.setText('Hide auto-generated headers' if self.expanded else f'{len(values)} hidden')
        self.page.tabs.setTabText(2,f'Headers ({len(self.headers.active_rows())+sum(name.lower() not in self.disabled for name in values)})')
