import json
import re
from pathlib import Path
from PySide6.QtCore import Qt, Signal, QThread, QSize, Slot, QObject, QTimer, QStandardPaths
from PySide6.QtGui import QFont
from PySide6.QtWidgets import (QWidget, QFrame, QVBoxLayout, QHBoxLayout, QLabel, QLineEdit,
    QPushButton, QToolButton, QMenu, QComboBox, QTabWidget, QSplitter, QFormLayout,
    QFileDialog, QApplication, QDialog, QListWidget, QMessageBox, QScrollArea,
    QTableWidget, QTableWidgetItem, QHeaderView, QAbstractItemView, QSizePolicy,QStackedWidget)
from ui_components import icon, tool, CodeEditor, KeyValueTable, Select as QComboBox, MethodButton, ProtocolButton
from shiboken6 import isValid
from grpc_support import discover_contract, describe_method, reflection_error, compile_proto, example_message
from grpc_picker import MethodSelector
from ui_tls import TLSButton
from ui_grpc_settings import settings_tab
from request_transport import TLSOptions, grpc_metadata
from ui_response_error import StatusBadge,ErrorPanel

HTTP_DEFAULT = {'name':'Untitled Request','method':'GET','url':'','params':[], 'headers':[],
    'auth_type':'No Auth','auth_value':'','body_type':'none','body':'','form':[]}
GRPC_DEFAULT = {'kind':'grpc','name':'Untitled gRPC Request','server':'','tls':False,'tls_verify':False,
    'service':'','rpc_method':'','message':'{}','metadata':[],'descriptor_file':''}


def row(parent=None, margins=(0,0,0,0), spacing=8):
    layout=QHBoxLayout(parent); layout.setContentsMargins(*margins); layout.setSpacing(spacing); return layout


class Job(QThread):
    result=Signal(object)
    failed=Signal(str)
    running=[]
    def __init__(self,fn): super().__init__(); self.fn=fn
    def run(self):
        try: self.result.emit(self.fn())
        except Exception as error: self.failed.emit(str(error))
    def launch(self):
        Job.running.append(self)
        self.finished.connect(self.cleanup,Qt.ConnectionType.QueuedConnection)
        self.start()
    @Slot()
    def cleanup(self):
        if self in Job.running:Job.running.remove(self)
        self.deleteLater()


class DialogCallbacks(QObject):
    def __init__(self,parent,ready,failed):super().__init__(parent);self.ready=ready;self.error=failed
    @Slot(object)
    def result(self,value):self.ready(value)
    @Slot(str)
    def failed(self,error):self.error(error)


class ResponsePanel(QWidget):
    def __init__(self,grpc=False):
        super().__init__(); self.grpc=grpc
        layout=QVBoxLayout(self); layout.setContentsMargins(0,0,0,0); layout.setSpacing(0)
        self.tabs=QTabWidget(); self.tabs.setObjectName('responseTabs')
        self.tabs.tabBar().setDrawBase(False)
        self.status_header=QWidget();self.status_header.setFixedHeight(38)
        status_row=row(self.status_header,(0,7,8,7),8)
        self.status=StatusBadge();status_row.addWidget(self.status,1)
        self.metrics=QLabel();self.metrics.setStyleSheet('font-size:11px;color:#999;')
        self.metrics.setSizePolicy(QSizePolicy.Policy.Preferred,QSizePolicy.Policy.Preferred);self.metrics.setMaximumWidth(130)
        status_row.addWidget(self.metrics)
        self.copy_button=tool('copy','Copy response',self.copy_response);self.copy_button.setFixedSize(24,24);self.copy_button.hide();status_row.addWidget(self.copy_button)
        self.tabs.setCornerWidget(self.status_header,Qt.Corner.TopRightCorner)
        body=QWidget(); body_layout=QVBoxLayout(body); body_layout.setContentsMargins(8,0,8,0); body_layout.setSpacing(0)
        toolbar=QWidget(); toolbar.setFixedHeight(36); controls=row(toolbar,spacing=8)
        self.editor=CodeEditor(readonly=True)
        self.editor.setPlaceholderText('Click Invoke to get a response' if grpc else 'Click Send to get a response')
        controls.addWidget(tool('wrap','Toggle line wrapping',self.editor.toggle_wrap))
        controls.addStretch()
        self.search=QLineEdit(); self.search.setPlaceholderText('Find in response'); self.search.setFixedWidth(190); self.search.hide()
        self.search.returnPressed.connect(self.find_next); controls.addWidget(self.search)
        controls.addWidget(tool('search','Find in response',self.toggle_search))
        controls.addWidget(tool('copy','Copy response',lambda: QApplication.clipboard().setText(self.editor.toPlainText())))
        controls.addWidget(tool('download','Save response',self.save_response))
        body_layout.addWidget(toolbar); body_layout.addWidget(self.editor,1)
        self.body_stack=QStackedWidget();self.body_stack.addWidget(body)
        self.error_panel=ErrorPanel();self.body_stack.addWidget(self.error_panel)
        self.tabs.addTab(self.body_stack,'Response' if grpc else 'Body')
        self.headers=KeyValueTable(readonly=True); self.tabs.addTab(self.headers,'Metadata' if grpc else 'Headers')
        self.trailers=KeyValueTable(readonly=True)
        if grpc: self.tabs.addTab(self.trailers,'Trailers')
        layout.addWidget(self.tabs)
        self._copy_text=''
    def copy_response(self):QApplication.clipboard().setText(self._copy_text)
    def set_pending(self):
        self.status.setText('Sending…');self.metrics.clear();self.copy_button.hide()
    def resizeEvent(self,event):
        super().resizeEvent(event);self.fit_status()
    def fit_status(self):
        self.status_header.setMaximumWidth(max(130,min(420,self.width()-self.tabs.tabBar().sizeHint().width()-16)))
    def toggle_search(self):
        self.search.setVisible(not self.search.isVisible()); self.search.setFocus()
    def find_next(self):
        if not self.editor.find(self.search.text()):
            cursor=self.editor.textCursor(); cursor.movePosition(cursor.MoveOperation.Start); self.editor.setTextCursor(cursor)
            self.editor.find(self.search.text())
    def save_response(self):
        path,_=QFileDialog.getSaveFileName(self,'Save response','response.json','JSON (*.json);;Text (*.txt);;All files (*)')
        if path: Path(path).write_text(self.editor.toPlainText(),encoding='utf-8')
    def show_response(self,result):
        self.copy_button.show()
        headers=result.get('headers',{})
        entries=list(headers.items()) if isinstance(headers,dict) else headers
        self.headers.set_rows([[str(k),str(v),''] for k,v in entries])
        self.tabs.setTabText(1,('Metadata' if self.grpc else 'Headers')+(f' ({len(entries)})' if entries else ''))
        trailers=result.get('trailers',[])
        self.trailers.set_rows([[str(k),str(v),''] for k,v in trailers])
        if self.grpc:self.tabs.setTabText(2,'Trailers'+(f' ({len(trailers)})' if trailers else ''))
        duration=result.get('duration',0)
        elapsed=f'{duration:g} ms' if duration<1000 else f'{duration/1000:.2f} s'
        if 'error' in result:
            info=result.get('error_info') or {'title':'Request failed','hint':'Review the error details below.',
                'details':result['error'],'badge':'REQUEST_ERROR','tone':'error'}
            self.status.setText(info['badge']);self.status.set_tone(info.get('tone','error'))
            self.metrics.setText(elapsed);self.metrics.setToolTip(elapsed)
            self.editor.setPlainText(result['error']);self.error_panel.set_error(info)
            self.body_stack.setCurrentIndex(1);self.tabs.setCurrentIndex(0)
            self._copy_text=info['badge']+'\n'+info['title']+'\n\n'+info.get('details',result['error']);self.copy_button.setToolTip('Copy error details')
            self.fit_status();return
        self.body_stack.setCurrentIndex(0)
        self.status.setText(f"{result['status']} {result['reason']}")
        tone='success' if self.grpc or 200<=result['status']<300 else 'error' if result['status']>=400 else 'warning'
        self.status.set_tone(tone)
        self.metrics.setText(f"{elapsed}  ·  {result['size']:,} B");self.metrics.setToolTip(self.metrics.text())
        text=result['body']
        try: text=json.dumps(json.loads(text),ensure_ascii=False,indent=2)
        except (ValueError,TypeError): pass
        self.editor.setPlainText(text)
        self._copy_text=text;self.copy_button.setToolTip('Copy response');self.fit_status()


class BasePage(QWidget):
    send_requested=Signal(object)
    save_requested=Signal(object)
    switch_requested=Signal(object,str)
    def __init__(self,data,request_id,kind):
        super().__init__(); self.setObjectName('page'); self.kind=kind; self.data=data
        self.request_id=request_id; self.collection_id=None; self.folder_id=None; self.busy=False
        self.layout_root=QVBoxLayout(self); self.layout_root.setContentsMargins(0,0,0,0); self.layout_root.setSpacing(0)
        title=QWidget(); title.setFixedHeight(48); title_row=row(title,(16,0,16,0),8)
        self.protocol=ProtocolButton(); self.protocol.setToolTip('Switch request type: HTTP / gRPC'); self.protocol.setObjectName('protocolButton')
        self.protocol.setIcon(icon(kind,'#7ca8d9' if kind=='grpc' else '#79cdb4')); self.protocol.setFixedSize(40,26)
        menu=QMenu(self.protocol)
        for label,value in [('HTTP','http'),('gRPC','grpc')]:
            action=menu.addAction(icon(value,'#7ca8d9' if value=='grpc' else '#79cdb4'),label)
            action.triggered.connect(lambda checked=False, value=value: self.switch_requested.emit(self,value) if self.kind!=value else None)
        self.protocol.setMenu(menu); self.protocol.setPopupMode(QToolButton.ToolButtonPopupMode.InstantPopup)
        title_row.addWidget(self.protocol)
        self.name=QLineEdit(data['name']); self.name.setObjectName('requestName'); title_row.addWidget(self.name,1)
        self.save_state=QLabel('Saved locally'); title_row.addWidget(self.save_state)
        save=tool('save','Save request (Ctrl+S)',lambda:self.save_requested.emit(self),'Save'); title_row.addWidget(save)
        self.layout_root.addWidget(title)
        address=QWidget(); address_row=row(address,(16,8,16,8),8)
        self.address_frame=QFrame(); self.address_frame.setObjectName('requestBar'); self.address_frame.setFixedHeight(42)
        self.address_layout=row(self.address_frame,(10,0,4,0),0)
        address_row.addWidget(self.address_frame,1)
        self.send_button=QPushButton('Invoke' if kind=='grpc' else 'Send'); self.send_button.setObjectName('sendButton'); self.send_button.setFixedSize(70,40)
        self.send_button.clicked.connect(lambda:self.send_requested.emit(self)); address_row.addWidget(self.send_button)
        self.layout_root.addWidget(address)
        self.split=QSplitter(Qt.Orientation.Vertical); self.split.setHandleWidth(1); self.split.setChildrenCollapsible(False)
        self.tabs=QTabWidget(); self.tabs.setObjectName('editorTabs'); self.tabs.setDocumentMode(True)
        self.tabs.tabBar().setDrawBase(False)
        self.split.addWidget(self.tabs)
        self.response=ResponsePanel(kind=='grpc'); self.split.addWidget(self.response)
        self.split.setStretchFactor(0,1); self.split.setStretchFactor(1,1); self.split.setSizes([385,390])
        self.layout_root.addWidget(self.split,1)
        self.status=self.response.status; self.response_body=self.response.editor
        self.response_headers=self.response.headers
    def set_busy(self,busy):
        self.busy=busy; self.send_button.setEnabled(not busy or self.kind=='grpc'); self.protocol.setEnabled(not busy)
        self.send_button.setText(('Cancel' if self.kind=='grpc' else 'Sending…') if busy else 'Invoke' if self.kind=='grpc' else 'Send')
        if busy:self.response.set_pending()
    def show_response(self,result):
        self.last_response=result; self.set_busy(False); self.response.show_response(result)
    def divider(self):
        line=QFrame(); line.setObjectName('divider'); line.setFixedSize(1,24); self.address_layout.addWidget(line)
    def add_editor(self,editor):
        widget=QWidget(); layout=QVBoxLayout(widget); layout.setContentsMargins(8,0,8,0); layout.setSpacing(0)
        layout.addWidget(editor,1)
        bottom=QFrame(); bottom.setFixedHeight(42); bottom.setStyleSheet('QFrame { border-top:1px solid #333; }')
        controls=row(bottom,(4,0,0,0),8)
        controls.addWidget(tool('format','Format JSON',editor.format_json,'Format JSON'))
        controls.addWidget(tool('wrap','Toggle line wrapping',editor.toggle_wrap)); controls.addStretch()
        if self.kind=='grpc':
            self.example_button=tool('example','Generate test values from the selected method contract',self.use_example_message,'Use Example Message')
            self.example_button.setEnabled(False); controls.insertWidget(1,self.example_button)
            self.example_status=QLabel(''); self.example_status.setObjectName('muted'); controls.addWidget(self.example_status)
        layout.addWidget(bottom); return widget


class RequestPage(BasePage):
    def __init__(self,data=None,request_id=None):
        d=dict(HTTP_DEFAULT,**(data or {})); super().__init__(d,request_id,'http')
        self.method=QComboBox(); self.method.addItems(['GET','POST','PUT','PATCH','DELETE','HEAD','OPTIONS'])
        self.method.setCurrentText(d['method']); self.method.setFixedWidth(100); self.method.setStyleSheet('color:#8cd4a6;font-weight:600;')
        self.address_layout.addWidget(self.method); self.divider()
        self.url=QLineEdit(d['url']); self.url.setPlaceholderText('Enter URL or paste text'); self.address_layout.addWidget(self.url,1)
        self.url.returnPressed.connect(lambda:self.send_requested.emit(self))
        self.params=KeyValueTable(d.get('param_rows',d['params'])); self.tabs.addTab(self.params,'Params')
        auth=QWidget(); form=QFormLayout(auth); form.setContentsMargins(24,24,24,24); form.setVerticalSpacing(20)
        self.auth_type=QComboBox(); self.auth_type.addItems(['No Auth','Bearer Token','Basic Auth','API Key']); self.auth_type.setCurrentText(d['auth_type'])
        self.auth_value=QLineEdit(d['auth_value']); self.auth_value.setPlaceholderText('Token, username:password, or header:value')
        form.addRow('Auth Type',self.auth_type); form.addRow('Value',self.auth_value); self.tabs.addTab(auth,'Authorization')
        self.headers=KeyValueTable(d.get('header_rows',d['headers'])); self.tabs.addTab(self.headers,'Headers')
        body_widget=QWidget(); body_layout=QVBoxLayout(body_widget); body_layout.setContentsMargins(0,0,0,0); body_layout.setSpacing(0)
        self.body_type=QComboBox(); self.body_type.addItems(['none','raw','x-www-form-urlencoded']); self.body_type.setCurrentText(d['body_type'])
        options=row(margins=(16,8,16,8)); options.addWidget(self.body_type); options.addStretch(); body_layout.addLayout(options)
        self.body=CodeEditor(d['body']); self.form=KeyValueTable(d.get('form_rows',d['form']))
        self.body_panel=self.add_editor(self.body)
        body_layout.addWidget(self.body_panel,1); body_layout.addWidget(self.form,1)
        self.body_type.currentTextChanged.connect(self._body_mode); self._body_mode(self.body_type.currentText())
        self.tabs.addTab(body_widget,'Body')
        self.bulk=tool('docs','Bulk edit parameters',lambda:self.current_table().bulk_edit() if self.current_table() else None,'Bulk Edit')
        self.tabs.setCornerWidget(self.bulk)
        self.tabs.currentChanged.connect(lambda i:self.bulk.setVisible(self.current_table() is not None))
        self.headers.changed.connect(lambda:self.tabs.setTabText(2,f'Headers ({len(self.headers.active_rows())})'))
    def current_table(self):
        current=self.tabs.currentWidget(); return current if isinstance(current,KeyValueTable) else None
    def _body_mode(self,mode): self.body_panel.setVisible(mode=='raw'); self.form.setVisible(mode=='x-www-form-urlencoded')
    def collect(self):
        return {'name':self.name.text().strip() or 'Untitled Request','method':self.method.currentText(),
            'url':self.url.text().strip(),'params':self.params.active_rows(),'param_rows':self.params.all_rows(),
            'headers':self.headers.active_rows(),'header_rows':self.headers.all_rows(),
            'auth_type':self.auth_type.currentText(),'auth_value':self.auth_value.text(),
            'body_type':self.body_type.currentText(),'body':self.body.toPlainText(),
            'form':self.form.active_rows(),'form_rows':self.form.all_rows()}


class GrpcPage(BasePage):
    generate_collection=Signal(object,object)
    def __init__(self,data=None,request_id=None):
        d=dict(GRPC_DEFAULT,**(data or {})); super().__init__(d,request_id,'grpc')
        self.tls_button=TLSButton(self); self.address_layout.addWidget(self.tls_button)
        self.server=QLineEdit(d['server']); self.server.setPlaceholderText('Enter server URL'); self.server.setMinimumWidth(130)
        self.address_layout.addWidget(self.server,1); self.divider()
        self.service=QLineEdit(d['service'],self); self.rpc_method=QLineEdit(d['rpc_method'],self)
        # Keep the method fields as a model; the address bar renders one aligned picker.
        self.service.hide(); self.rpc_method.hide()
        self.method_picker=MethodSelector(self)
        self.method_picker.setSizePolicy(QSizePolicy.Policy.Expanding,QSizePolicy.Policy.Preferred)
        self.address_layout.addWidget(self.method_picker,2)
        self.docs_scroll=QScrollArea(); self.docs_scroll.setWidgetResizable(True)
        docs_outer=QWidget(); docs_row=row(docs_outer,(0,0,0,0),0); docs_row.addStretch()
        self.docs_content=QWidget(); self.docs_content.setMaximumWidth(568); self.docs_content.setMinimumWidth(250)
        docs_layout=QVBoxLayout(self.docs_content); docs_layout.setContentsMargins(0,0,0,12); docs_layout.setSpacing(12)
        self.docs_title=QLabel(); self.docs_title.setWordWrap(True); self.docs_title.setObjectName('docsTitle'); docs_layout.addWidget(self.docs_title)
        self.docs_request=QLabel('Select a method to see its request and response fields.'); self.docs_request.setWordWrap(True); docs_layout.addWidget(self.docs_request)
        self.docs_request_fields=self.schema_table(); docs_layout.addWidget(self.docs_request_fields)
        self.docs_response=QLabel(''); docs_layout.addWidget(self.docs_response)
        self.docs_response_fields=self.schema_table(); docs_layout.addWidget(self.docs_response_fields); docs_layout.addStretch()
        docs_row.addWidget(self.docs_content,1); docs_row.addStretch(); self.docs_scroll.setWidget(docs_outer)
        self.tabs.addTab(self.docs_scroll,'Docs'); self.tabs.setTabIcon(0,icon('docs'))
        self.message=CodeEditor(d['message']); self.tabs.addTab(self.add_editor(self.message),'Message')
        auth=QWidget(); auth_form=QFormLayout(auth); auth_form.setContentsMargins(24,24,24,24); auth_form.setVerticalSpacing(20)
        self.auth_type=QComboBox(); self.auth_type.addItems(['No Auth','Bearer Token']); self.auth_type.setCurrentText(d.get('auth_type','No Auth'))
        self.auth_value=QLineEdit(d.get('auth_value','')); self.auth_value.setPlaceholderText('Token or {{variable}}')
        auth_form.addRow('Auth Type',self.auth_type); auth_form.addRow('Token',self.auth_value); self.tabs.addTab(auth,'Authorization')
        self.metadata=KeyValueTable(d.get('metadata_rows',d['metadata'])); self.tabs.addTab(self.metadata,'Metadata')
        self.metadata.changed.connect(self.metadata_count); self.metadata_count()
        definition=QWidget(); definition_layout=QVBoxLayout(definition); definition_layout.setContentsMargins(24,24,24,24); definition_layout.setSpacing(16)
        heading=QLabel('Service definition'); heading.setObjectName('docsTitle'); definition_layout.addWidget(heading)
        description=QLabel('Use server reflection to discover services, or import a local protobuf descriptor set.'); description.setWordWrap(True); description.setObjectName('muted'); definition_layout.addWidget(description)
        source=row(); self.descriptor_file=QLineEdit(d['descriptor_file']); self.descriptor_file.setPlaceholderText('No local definition — use server reflection')
        source.addWidget(self.descriptor_file,1); browse=QPushButton('Import definition'); browse.clicked.connect(self.choose_descriptor); source.addWidget(browse)
        definition_layout.addLayout(source); definition_layout.addStretch(); self.tabs.addTab(definition,'Service definition')
        self.tls=QComboBox(self); self.tls.addItems(['Plaintext','TLS']); self.tls.setCurrentIndex(1 if d['tls'] or d['server'].strip().startswith(('grpcs://','https://')) else 0); self.tls.hide()
        self.certificates_dialog=QDialog(self); self.certificates_dialog.setWindowTitle('CA and client certificates'); self.certificates_dialog.resize(660,300)
        settings_form=QFormLayout(self.certificates_dialog); settings_form.setContentsMargins(24,24,24,24); settings_form.setVerticalSpacing(16)
        cert_heading=QLabel('CA and client certificates'); cert_heading.setObjectName('docsTitle'); settings_form.addRow(cert_heading)
        help_text=QLabel('TLS verifies the server using system trust roots and any additional CA certificate. For mutual TLS, select a client certificate and its private key.'); help_text.setWordWrap(True); help_text.setObjectName('muted'); settings_form.addRow(help_text)
        self.tls_ca_file=QLineEdit(d.get('tls_ca_file','')); self.tls_cert_file=QLineEdit(d.get('tls_cert_file','')); self.tls_key_file=QLineEdit(d.get('tls_key_file',''))
        for label,field in [('CA certificate',self.tls_ca_file),('Client certificate',self.tls_cert_file),('Client private key',self.tls_key_file)]:
            field.setPlaceholderText('Local PEM file or {{variable}}'); holder=QWidget(); line=row(holder)
            line.addWidget(field,1); browse=QPushButton('Browse…'); browse.clicked.connect(lambda checked=False,field=field:self.choose_tls_file(field)); line.addWidget(browse)
            settings_form.addRow(label,holder)
        note=QLabel('Certificate paths are saved locally with this request. Private key contents are never added to collection exports.'); note.setWordWrap(True); note.setObjectName('muted'); settings_form.addRow(note)
        close=QPushButton('Done'); close.clicked.connect(self.certificates_dialog.hide); settings_form.addRow('',close)
        self.tabs.addTab(settings_tab(self,d),'Settings')
        self.tabs.setCurrentIndex(1); self.tabs.currentChanged.connect(self.tab_changed)
        self.bulk=tool('docs','Bulk edit metadata',self.metadata.bulk_edit,'Bulk Edit'); self.tabs.setCornerWidget(self.bulk); self.bulk.hide()
        self.service.textChanged.connect(self.update_docs); self.rpc_method.textChanged.connect(self.update_docs)
        self.variables_provider=lambda:{}
        self.definition_directory=Path(QStandardPaths.writableLocation(QStandardPaths.StandardLocation.AppDataLocation))/'definitions'
        self._schema_key=None; self._discovery_key=None; self._inflight_key=None; self._methods=[]; self._generation=0; self._importing=False
        self._contract=None; self._contract_key=None; self._example_ticket=0; self._example_loading=False
        self._reload_timer=QTimer(self); self._reload_timer.setSingleShot(True); self._reload_timer.setInterval(300); self._reload_timer.timeout.connect(self.start_discovery)
        self.method_picker.opened.connect(self.start_discovery)
        self.method_picker.refresh_requested.connect(lambda:self.start_discovery(force=True))
        self.method_picker.reflection_requested.connect(self.use_reflection)
        self.method_picker.import_requested.connect(self.choose_descriptor)
        self.method_picker.method_selected.connect(self.select_method)
        self.method_picker.generate_requested.connect(self.generate_from_methods)
        self._tls_syncing=False
        self.tls_button.clicked.connect(lambda checked:self.tls.setCurrentIndex(int(checked)))
        self.tls_button.configure_requested.connect(self.show_certificates)
        self.server.textChanged.connect(self.server_changed); self.tls.currentIndexChanged.connect(self.tls_changed)
        for field in (self.tls_ca_file,self.tls_cert_file,self.tls_key_file):field.textChanged.connect(self.source_changed)
        self.tls_verify.toggled.connect(self.source_changed); self.tls_server_name.textChanged.connect(self.source_changed)
        self.tls_button.update_state(self.tls.currentIndex()==1)
        self.descriptor_file.textChanged.connect(self.source_changed); self.metadata.changed.connect(self.source_changed)
        self.auth_type.currentTextChanged.connect(self.source_changed); self.auth_value.textChanged.connect(self.source_changed)
        self.update_docs()
    def choose_tls_file(self,field):
        path,_=QFileDialog.getOpenFileName(self,'Select PEM certificate or key','','PEM certificates and keys (*.pem *.crt *.cer *.key);;All files (*)')
        if path:field.setText(path)
    def show_certificates(self):
        self.tabs.setCurrentIndex(5); self.certificates_dialog.show(); self.certificates_dialog.raise_(); self.tls_ca_file.setFocus()
    def tls_changed(self,*_):
        enabled=self.tls.currentIndex()==1
        self.tls_button.update_state(enabled)
        if not self._tls_syncing:
            value=self.server.text()
            for prefix in ('grpc://','grpcs://','http://','https://'):
                if value.strip().startswith(prefix):
                    self._tls_syncing=True
                    self.server.setText(('grpcs://' if enabled else 'grpc://')+value.strip()[len(prefix):])
                    self._tls_syncing=False; break
        self.source_changed()
    def server_changed(self,*_):
        if not self._tls_syncing:
            value=self.server.text().strip()
            if value.startswith(('grpc://','grpcs://','http://','https://')):
                self._tls_syncing=True; self.tls.setCurrentIndex(int(value.startswith(('grpcs://','https://')))); self._tls_syncing=False
        self.source_changed()
    def metadata_count(self):
        count=len(self.metadata.active_rows()); self.tabs.setTabText(3,f'Metadata ({count})' if count else 'Metadata')
    def schema_table(self):
        table=QTableWidget(0,2); table.setHorizontalHeaderLabels(['Field','Type']); table.verticalHeader().hide()
        table.horizontalHeader().setSectionResizeMode(QHeaderView.ResizeMode.Stretch); table.verticalHeader().setDefaultSectionSize(37)
        table.horizontalHeader().setDefaultAlignment(Qt.AlignmentFlag.AlignLeft|Qt.AlignmentFlag.AlignVCenter); table.horizontalHeader().setFixedHeight(35)
        table.setVerticalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
        table.setEditTriggers(QAbstractItemView.EditTrigger.NoEditTriggers); table.setFixedHeight(35); table.hide(); return table
    def tab_changed(self,index):
        self.bulk.setVisible(index==3)
        if index==0:self.load_docs()
    def update_docs(self):
        service=self.service.text().strip(); method=self.rpc_method.text().strip()
        title=f'{service} / {method}' if service and method else 'Select a method'
        self.method_picker.set_selected(service,method); self.docs_title.setText(title)
        self.example_button.setEnabled(bool(service and method) and not getattr(self,'_example_loading',False))
    def resizeEvent(self,event): super().resizeEvent(event); self.update_docs()
    def load_docs(self):
        args=self.schema_context()
        if (not args[0] and not args[2]) or not args[3] or not args[4] or args==self._schema_key:return
        self._schema_key=args; self.docs_request.setText('Loading service definition…')
        contract=self.current_contract()
        def fetch_schema():
            try:return args,describe_method(*args,schema_bytes=contract),None
            except Exception as error:return args,None,reflection_error(error)
        job=Job(fetch_schema); job.result.connect(self.schema_done,Qt.ConnectionType.QueuedConnection); job.launch()
    @Slot(object)
    def schema_done(self,result):
        args,schema,error=result
        if args!=self.schema_context() or args!=self._schema_key:return
        if error:self.schema_error(error)
        else:self.show_schema(schema)
    @Slot(str)
    def schema_error(self,error): self._schema_key=None; self.docs_request.setText('Unable to load schema. '+error)
    @Slot(object)
    def show_schema(self,result):
        request_name,request_fields,response_name,response_fields=result
        self.docs_request.setText('request   '+request_name); self.docs_response.setText('response   '+response_name)
        for table,values in [(self.docs_request_fields,request_fields),(self.docs_response_fields,response_fields)]:
            table.setRowCount(len(values))
            for row_index,values_row in enumerate(values):
                for column,value in enumerate(values_row):table.setItem(row_index,column,QTableWidgetItem(value))
            table.setFixedHeight(37+len(values)*37); table.show()
    def discovery_context(self):
        variables=self.variables_provider()
        def resolve(text):
            return re.sub(r'\{\{\s*([^{}]+?)\s*\}\}',lambda m:str(variables.get(m.group(1),m.group(0))),text)
        server=resolve(self.server.text().strip())
        data=self.collect(); metadata=grpc_metadata(data,variables)
        return server,TLSOptions.from_data(data,variables),resolve(self.descriptor_file.text().strip()),tuple(metadata)

    def schema_context(self):
        server,tls,path,metadata=self.discovery_context()
        return server,tls,path,self.service.text().strip(),self.rpc_method.text().strip(),metadata

    def source_changed(self,*args):
        self._generation+=1; self._discovery_key=None; self._inflight_key=None; self._schema_key=None; self._methods=[]; self._importing=False
        self.method_picker.set_idle()
        self._contract=None; self._contract_key=None
        self.cancel_example()
        if self.method_picker._expanded:self._reload_timer.start()

    def start_discovery(self,force=False):
        self._reload_timer.stop()
        if self._importing:
            self.method_picker.set_loading('local'); return
        key=self.discovery_context(); server,tls,path,metadata=key
        source='local' if path else 'reflection'
        if not server and not path:
            self.method_picker.set_idle(); return
        if '{{' in server:
            self.method_picker.set_error('The server URL contains an unresolved variable. Select an environment or enter a host and port.'); return
        if not force and self._discovery_key==key:
            self.method_picker.set_methods(self._methods,source); return
        if not force and self._inflight_key==key:
            self.method_picker.set_loading(source); return
        self._generation+=1; generation=self._generation
        self._inflight_key=key; self._discovery_key=None; self.method_picker.set_loading(source)
        def fetch():
            try:return generation,key,discover_contract(server,tls,path,metadata),None
            except Exception as error:return generation,key,None,reflection_error(error)
        job=Job(fetch); job.result.connect(self.discovery_done,Qt.ConnectionType.QueuedConnection); job.launch()

    @Slot(object)
    def discovery_done(self,result):
        generation,key,definition,error=result
        if generation!=self._generation or key!=self.discovery_context():return
        self._inflight_key=None; source='local' if key[2] else 'reflection'
        if error:
            self.method_picker.set_error(error,source); return
        methods,contract=definition; self._contract=contract; self._contract_key=key
        self._methods=list(methods); self._discovery_key=key
        self.method_picker.set_methods(methods,source)

    @Slot(str,str)
    def select_method(self,service,method):
        self.cancel_example()
        self.service.setText(service); self.rpc_method.setText(method); self._schema_key=None
        if method.strip():self.name.setText(method.strip())
        if self.tabs.currentIndex()==0:self.load_docs()

    def discover_methods(self):
        self.method_picker.show_popup()

    def current_contract(self):
        return self._contract if self._contract_key==self.discovery_context() else None

    def cancel_example(self):
        self._example_ticket+=1; self._example_loading=False
        self.example_button.setText('Use Example Message'); self.example_status.clear(); self.update_docs()

    def use_example_message(self):
        args=self.schema_context()
        if not args[3] or not args[4] or self._example_loading:return
        self._example_ticket+=1; ticket=self._example_ticket; original=self.message.toPlainText(); contract=self.current_contract()
        self._example_loading=True; self.example_button.setEnabled(False); self.example_button.setText('Loading example…'); self.example_status.clear()
        def generate():
            try:return ticket,args,original,example_message(*args,schema_bytes=contract),None
            except Exception as error:return ticket,args,original,None,reflection_error(error)
        job=Job(generate); job.result.connect(self.example_ready,Qt.ConnectionType.QueuedConnection); job.launch()

    @Slot(object)
    def example_ready(self,result):
        ticket,args,original,payload,error=result
        if ticket!=self._example_ticket:return
        self._example_loading=False; self.example_button.setText('Use Example Message'); self.update_docs()
        if args!=self.schema_context():return
        if error:self.example_status.setText('Could not generate example'); self.example_status.setToolTip(error); return
        if self.message.toPlainText()!=original:
            self.example_status.setText('Message changed; click again to replace it'); return
        cursor=self.message.textCursor(); cursor.beginEditBlock(); cursor.select(cursor.SelectionType.Document); cursor.insertText(payload); cursor.endEditBlock()
        self.message.setTextCursor(cursor); self.tabs.setCurrentIndex(1); self.example_status.clear()

    def use_reflection(self):
        if self.descriptor_file.text():self.descriptor_file.clear()
        self.start_discovery(force=True)

    def choose_descriptor(self):
        self.method_picker.dismiss()
        path,_=QFileDialog.getOpenFileName(self,'Import service definition','','Protobuf schemas (*.proto *.protoset *.pb *.desc);;All files (*)')
        if not path:return
        if Path(path).suffix.lower()!='.proto':
            self.descriptor_file.setText(path); self.method_picker.show_popup(); return
        self._generation+=1; generation=self._generation; self._importing=True
        self.method_picker.show_popup(); self.method_picker.set_loading('local')
        directory=self.definition_directory
        def compile_definition():
            try:return generation,compile_proto(path,directory),None
            except Exception as error:return generation,None,str(error)
        job=Job(compile_definition); job.result.connect(self.definition_imported,Qt.ConnectionType.QueuedConnection); job.launch()

    @Slot(object)
    def definition_imported(self,result):
        generation,path,error=result
        if generation!=self._generation:return
        self._importing=False
        if error:self.method_picker.set_error(error,'local'); return
        self.descriptor_file.setText(path); self.start_discovery(force=True)

    def generate_from_methods(self):
        if self._discovery_key!=self.discovery_context() or not self._methods:return
        self.method_picker.dismiss(); self.generate_collection.emit(self.collect(),list(self._methods))

    def collect(self):
        return {'kind':'grpc','name':self.name.text().strip() or 'Untitled gRPC Request',
            'server':self.server.text().strip(),'tls':self.tls.currentIndex()==1,
            'tls_ca_file':self.tls_ca_file.text().strip(),'tls_cert_file':self.tls_cert_file.text().strip(),'tls_key_file':self.tls_key_file.text().strip(),
            'tls_verify':self.tls_verify.isChecked(),'tls_server_name':self.tls_server_name.text().strip(),
            'grpc_emit_defaults':self.grpc_emit_defaults.isChecked(),'grpc_max_response_mb':self.grpc_max_response_mb.value(),'grpc_timeout_ms':self.grpc_timeout_ms.value(),
            'service':self.service.text().strip(),'rpc_method':self.rpc_method.text().strip(),
            'message':self.message.toPlainText(),'metadata':self.metadata.active_rows(),'metadata_rows':self.metadata.all_rows(),
            'descriptor_file':self.descriptor_file.text().strip(),'auth_type':self.auth_type.currentText(),'auth_value':self.auth_value.text()}
