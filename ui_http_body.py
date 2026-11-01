"""Body modes, multipart fields, files and GraphQL query/variables editors."""
from pathlib import Path
from PySide6.QtCore import Qt,Signal
from PySide6.QtWidgets import (QWidget,QVBoxLayout,QHBoxLayout,QLabel,QLineEdit,QPushButton,QStackedWidget,
    QSplitter,QTableWidgetItem,QHeaderView,QStyledItemDelegate,QFileDialog)
from ui_components import CodeEditor,KeyValueTable,tool
from ui_editor_controls import EditorSelect
from http_features import RAW_TYPES


def choose_file(parent,callback):
    from ui_editor_behavior import settings
    root=Path(settings().working_directory).resolve();path,_=QFileDialog.getOpenFileName(parent,'Select file',str(root))
    if path:
        selected=Path(path).resolve();callback(str(selected.relative_to(root) if selected.is_relative_to(root) else selected))


class MultipartTable(KeyValueTable):
    def bulk_edit(self):
        self._bulk_rows={row[0]:row for row in self.all_rows()}
        try:super().bulk_edit()
        finally:self._bulk_rows=None
    def set_rows(self,data):
        if getattr(self,'_bulk_rows',None):data=[[r[0],r[1],*self._bulk_rows[r[0]][2:]] if r[0] in self._bulk_rows else r for r in data]
        table=self.table;table.blockSignals(True);table.setColumnCount(5);table.setRowCount(0)
        table.setHorizontalHeaderLabels(['','Key','Type','Value','Description']);table.setItemDelegate(QStyledItemDelegate(table))
        table.horizontalHeader().setSectionResizeMode(2,QHeaderView.ResizeMode.Fixed);table.setColumnWidth(2,90)
        for column in (1,3,4):table.horizontalHeader().setSectionResizeMode(column,QHeaderView.ResizeMode.Stretch)
        for values in list(data)+[['','','',True,'Text']]:self._row(values)
        table.blockSignals(False);self.fit_height();self.changed.emit()
    def _row(self,values):
        table=self.table;row=table.rowCount();table.insertRow(row)
        check=QTableWidgetItem();check.setFlags(Qt.ItemFlag.ItemIsEnabled|Qt.ItemFlag.ItemIsUserCheckable)
        if values[0]:check.setCheckState(Qt.CheckState.Checked if len(values)<4 or values[3] else Qt.CheckState.Unchecked)
        table.setItem(row,0,check)
        for column,value in [(1,values[0]),(3,values[1]),(4,values[2] if len(values)>2 else '')]:table.setItem(row,column,QTableWidgetItem(str(value)))
        kind=EditorSelect(['Text','File'],values[4] if len(values)>4 else 'Text');kind.setMinimumWidth(0);kind.setStyleSheet('QComboBox {border:0;background:#1f1f1f;padding:0 8px;}');table.setCellWidget(row,2,kind)
        kind.currentTextChanged.connect(lambda *_:self._kind(row));self._kind(row,notify=False)
    def _kind(self,row,notify=True):
        table=self.table;kind=table.cellWidget(row,2).currentText()
        if kind=='File':
            panel=QWidget();panel.setAutoFillBackground(True);panel.setStyleSheet('background:#1f1f1f;');layout=QHBoxLayout(panel);layout.setContentsMargins(4,0,2,0);layout.setSpacing(2)
            field=QLineEdit(table.item(row,3).text());field.setPlaceholderText('Select file');field.setStyleSheet('border:0;background:transparent;');layout.addWidget(field,1)
            field.textChanged.connect(lambda text:table.item(row,3).setText(text));layout.addWidget(tool('folder','Select file',lambda:choose_file(self,field.setText)));table.setCellWidget(row,3,panel)
        else:table.removeCellWidget(row,3)
        if notify:self.changed.emit()
    def on_change(self,row,col):
        if row==self.table.rowCount()-1 and col in (1,3,4) and self.table.item(row,col).text():
            self.table.blockSignals(True);self.table.item(row,0).setCheckState(Qt.CheckState.Checked);self._row(['','','',True,'Text']);self.table.blockSignals(False);self.fit_height()
        self.changed.emit()
    def all_rows(self):
        return [[self.table.item(r,1).text(),self.table.item(r,3).text(),self.table.item(r,4).text(),
            self.table.item(r,0).checkState()==Qt.CheckState.Checked,self.table.cellWidget(r,2).currentText()]
            for r in range(self.table.rowCount()) if self.table.item(r,1).text().strip()]


class BodyPanel(QWidget):
    changed=Signal()
    def __init__(self,page,data):
        super().__init__();self.page=page;self._initializing=True
        layout=QVBoxLayout(self);layout.setContentsMargins(8,0,8,0);layout.setSpacing(0)
        toolbar=QWidget();controls=QHBoxLayout(toolbar);controls.setContentsMargins(8,8,8,8);controls.setSpacing(8)
        self.body_type=EditorSelect(['none','form-data','x-www-form-urlencoded','raw','binary','GraphQL'],'binary' if data.get('body_type')=='file' else data.get('body_type','none'))
        self.body_type.setMaximumWidth(205);controls.addWidget(self.body_type)
        media=next((r[1].split(';')[0].strip() for r in data.get('headers',[]) if r[0].lower()=='content-type'),'')
        inferred=next((name for name,value in RAW_TYPES.items() if value==media),None)
        if not inferred:inferred='JSON' if not data.get('body','').strip() or data.get('body','').lstrip().startswith(('{','[')) else 'Text'
        self.raw_language=EditorSelect(list(RAW_TYPES),data.get('raw_language',inferred));self.raw_language.setFixedWidth(110);controls.addWidget(self.raw_language)
        controls.addStretch();self.schema=tool('settings','Local request schema',self.choose_schema,'Schema');controls.addWidget(self.schema)
        self.beautify=QPushButton('Beautify');self.beautify.setFlat(True);self.beautify.setStyleSheet('color:#3b82f6;font-weight:600;');self.beautify.clicked.connect(self.format_body);controls.addWidget(self.beautify)
        self.bulk=tool('docs','Bulk edit body fields',self.bulk_edit,'Bulk Edit');controls.addWidget(self.bulk);layout.addWidget(toolbar)
        self.stack=QStackedWidget();layout.addWidget(self.stack,1)
        none=QWidget();empty=QVBoxLayout(none);label=QLabel('This request does not have a body.');label.setObjectName('muted');label.setAlignment(Qt.AlignmentFlag.AlignCenter);empty.addWidget(label);self.stack.addWidget(none)
        self.multipart=MultipartTable(data.get('multipart_rows',[]));self.stack.addWidget(self.multipart)
        self.form=KeyValueTable(data.get('form_rows',data.get('form',[])));self.stack.addWidget(self.form)
        self.body=CodeEditor(data.get('body',''));self.body.setPlaceholderText('Enter request body');self.stack.addWidget(self.body)
        self.body_file_panel=QWidget();file_layout=QVBoxLayout(self.body_file_panel);file_layout.setContentsMargins(8,0,8,0)
        line=QHBoxLayout();self.body_file=QLineEdit(data.get('body_file',''));self.body_file.setPlaceholderText('Select file');self.body_file.setFixedWidth(280);line.addWidget(self.body_file)
        line.addWidget(tool('folder','Select request body file',lambda:choose_file(self,self.body_file.setText)));line.addStretch();file_layout.addLayout(line);file_layout.addStretch();self.stack.addWidget(self.body_file_panel)
        graphql=QSplitter(Qt.Orientation.Horizontal);graphql.setChildrenCollapsible(False);graphql.setHandleWidth(5)
        self.graphql_query=CodeEditor(data.get('graphql_query',''));self.graphql_variables=CodeEditor(data.get('graphql_variables',''));self.graphql_variables.setPlaceholderText('{}')
        if hasattr(self.graphql_query,'presentation'):self.graphql_query.presentation.mode='text';self.graphql_variables.presentation.mode='json'
        for heading,editor in [('QUERY',self.graphql_query),('GRAPHQL VARIABLES',self.graphql_variables)]:
            pane=QWidget();section=QVBoxLayout(pane);section.setContentsMargins(0,0,0,0);section.setSpacing(6);label=QLabel(heading);label.setStyleSheet('font-size:11px;font-weight:600;');section.addWidget(label);section.addWidget(editor,1);graphql.addWidget(pane)
        graphql.setSizes([500,350]);self.stack.addWidget(graphql)
        self.body_type.currentTextChanged.connect(self.mode_changed)
        for field in (self.body,self.body_file,self.graphql_query,self.graphql_variables):field.textChanged.connect(self.changed)
        self.raw_language.currentTextChanged.connect(self.language_changed);self.form.changed.connect(self.changed);self.multipart.changed.connect(self.changed)
        self.language_changed()
        self.mode_changed(self.body_type.currentText());self._initializing=False
    def mode_changed(self,mode):
        self.stack.setCurrentIndex(self.body_type.currentIndex());raw=mode=='raw'
        self.raw_language.setVisible(raw);self.schema.setVisible(raw);self.beautify.setVisible(raw);self.bulk.setVisible(mode in ('form-data','x-www-form-urlencoded'))
        if mode=='GraphQL' and not self._initializing and self.page.method.currentText()!='POST':
            self.page.method.setCurrentText('POST')
            from ui_workspace_panels import show_toast
            show_toast(self.page.window(),'Request method changed','Request method has been changed to POST for the selected body type — GraphQL.')
        self.changed.emit()
    def bulk_edit(self):
        (self.multipart if self.body_type.currentText()=='form-data' else self.form).bulk_edit()
    def language_changed(self,*_):
        language=self.raw_language.currentText();self.beautify.setEnabled(language in ('JSON','XML'))
        if hasattr(self.body,'presentation'):self.body.presentation.mode='json' if language=='JSON' else 'text';self.body.presentation.validate()
        self.changed.emit()
    def choose_schema(self):choose_file(self,self.page.http_schema.setText)
    def format_body(self):
        if self.raw_language.currentText()=='XML':
            try:
                from xml.dom.minidom import parseString
                self.body.setPlainText(parseString(self.body.toPlainText()).toprettyxml(indent='  '))
            except Exception as error:
                from ui_workspace_panels import show_toast
                show_toast(self.window(),'Cannot format XML',str(error))
        else:self.body.format_json()
    def collect(self):
        return dict(body_type=self.body_type.currentText(),body=self.body.toPlainText(),raw_language=self.raw_language.currentText(),
            body_file=self.body_file.text(),form=self.form.active_rows(),form_rows=self.form.all_rows(),multipart_rows=self.multipart.all_rows(),
            graphql_query=self.graphql_query.toPlainText(),graphql_variables=self.graphql_variables.toPlainText())
