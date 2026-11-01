import json
import re
import sqlite3
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path
from grpc_support import invoke, Cancellation, rpc_error
from ui_pages import RequestPage, GrpcPage, Job
from ui_shell import build_shell
from ui_components import icon, tool
from ui_theme import STYLE
from collection_store import CollectionStore
from collection_actions import CollectionActions
from ui_collections import CollectionPage, ExamplePage
from autosave import Autosave
from request_transport import prepare_http,open_http,decode_http_content
from code_snippets import generate_snippet
from response_errors import failure_result
from ui_brand import USE_REQON_UI,DISPLAY_NAME,VERSION,brand_icon,windows_identity
from app_paths import workspace_database

from PySide6.QtCore import Qt, QStandardPaths, QThread, Signal, Slot
from PySide6.QtGui import QAction, QColor, QFont, QKeySequence, QSyntaxHighlighter, QTextCharFormat
from PySide6.QtWidgets import (QApplication, QComboBox, QDialog, QFileDialog, QFormLayout,
    QHBoxLayout, QInputDialog, QLabel, QLineEdit, QMainWindow, QMenu, QMessageBox,
    QPlainTextEdit, QPushButton, QSplitter, QTabWidget, QTableWidget, QTableWidgetItem,
    QTreeWidget, QTreeWidgetItem, QVBoxLayout, QWidget, QAbstractItemView, QFrame, QTabBar)

METHODS = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS']
DEFAULT = {'name': 'Untitled Request', 'method': 'GET', 'url': '', 'params': [],
           'headers': [], 'auth_type': 'No Auth', 'auth_value': '', 'body_type': 'none',
           'body': '', 'form': []}
GRPC_DEFAULT = {'kind':'grpc', 'name':'Untitled gRPC Request', 'server':'', 'tls':False,
                'service':'', 'rpc_method':'', 'message':'', 'metadata':[], 'descriptor_file':''}


class Store(CollectionStore):
    def __init__(self, directory=None):
        if USE_REQON_UI:
            self.path = workspace_database(directory)
        else:
            root = Path(directory or QStandardPaths.writableLocation(QStandardPaths.StandardLocation.AppDataLocation))
            root.mkdir(parents=True, exist_ok=True)
            self.path = root / 'getman.sqlite3'
        self.db = sqlite3.connect(self.path)
        self.db.execute('PRAGMA foreign_keys=ON')
        self.db.executescript('''
        CREATE TABLE IF NOT EXISTS collections(id INTEGER PRIMARY KEY, name TEXT NOT NULL, kind TEXT NOT NULL DEFAULT 'http');
        CREATE TABLE IF NOT EXISTS folders(id INTEGER PRIMARY KEY, collection_id INTEGER NOT NULL REFERENCES collections(id) ON DELETE CASCADE, name TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS requests(id INTEGER PRIMARY KEY, collection_id INTEGER NOT NULL REFERENCES collections(id) ON DELETE CASCADE, folder_id INTEGER REFERENCES folders(id) ON DELETE SET NULL, name TEXT NOT NULL, data TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS environments(id INTEGER PRIMARY KEY, name TEXT NOT NULL, variables TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS history(id INTEGER PRIMARY KEY, created_at TEXT DEFAULT CURRENT_TIMESTAMP, method TEXT, url TEXT, status INTEGER, duration INTEGER, data TEXT);
        ''')
        columns = [row[1] for row in self.db.execute('PRAGMA table_info(collections)')]
        if 'kind' not in columns:
            self.db.execute("ALTER TABLE collections ADD COLUMN kind TEXT NOT NULL DEFAULT 'http'")
        self.db.commit()
        self.migrate_collections()

    def all(self, sql, args=()):
        return self.db.execute(sql, args).fetchall()

    def save_request(self, data, request_id=None, collection_id=None, folder_id=None):
        with self.atomic():
            if request_id:
                row = self.entity('request', request_id)
                self.db.execute('UPDATE requests SET name=?,data=? WHERE id=?', (data['name'], json.dumps(data), request_id))
                collection_id = row['collection_id']; result = request_id
            else:
                self.entity('collection', collection_id)
                if folder_id is not None and self.entity('folder', folder_id)['collection_id'] != collection_id:
                    raise ValueError('The folder is in another collection.')
                result = self.insert('requests', {'collection_id': collection_id, 'folder_id': folder_id,
                                    'name': data['name'], 'data': json.dumps(data)})
            self.reconcile_kind(collection_id)
            return result

    def insert(self, table, values):
        values = self.insertion_values(table, values)
        keys = list(values)
        cur = self.db.execute(f"INSERT INTO {table}({','.join(keys)}) VALUES({','.join('?' for _ in keys)})", tuple(values.values()))
        if not self._transaction_depth: self.db.commit()
        return cur.lastrowid

    def delete(self, table, id):
        self.db.execute(f'DELETE FROM {table} WHERE id=?', (id,))
        self.db.commit()


def expand(value, variables):
    return re.sub(r'\{\{\s*([^{}]+?)\s*\}\}', lambda m: str(variables.get(m.group(1), m.group(0))), str(value))


def rows(table):
    result = []
    for r in range(table.rowCount()):
        vals = [(table.item(r, c).text() if table.item(r, c) else '').strip() for c in range(table.columnCount())]
        if vals[0]: result.append(vals)
    return result


def set_rows(table, data):
    table.setRowCount(0)
    for values in list(data) + [[''] * table.columnCount()]:
        r = table.rowCount()
        table.insertRow(r)
        for c, value in enumerate(values[:table.columnCount()]):
            table.setItem(r, c, QTableWidgetItem(str(value)))


def make_table(headers):
    table = QTableWidget(0, len(headers))
    table.setHorizontalHeaderLabels(headers)
    table.horizontalHeader().setStretchLastSection(True)
    table.verticalHeader().hide()
    table.setAlternatingRowColors(True)
    table.cellChanged.connect(lambda r, c: table.insertRow(table.rowCount()) if r == table.rowCount()-1 and table.item(r, c) and table.item(r, c).text().strip() else None)
    set_rows(table, [])
    return table


class Worker(QThread):
    done = Signal(object)
    def __init__(self, data, variables, proxy_config=None,general_config=None,storage=None,cookies=None):
        super().__init__()
        self.data, self.variables = data, variables
        self.proxy_config=proxy_config
        self.general_config=general_config;self.storage=storage;self.cookies=cookies
        self.cancellation=Cancellation()

    def cancel(self):self.cancellation.cancel()

    def run(self):
        t = time.monotonic()
        try:
            d, v = self.data, self.variables
            if self.general_config:
                from general_settings import effective_request
                d=effective_request(d,self.general_config,self.storage)
            if d.get('kind') == 'grpc':
                self.done.emit(invoke(dict(d,_proxy_config=self.proxy_config), v, expand,self.cancellation))
                return
            if self.general_config:
                from http_runtime import send_http
                self.done.emit(send_http(d,v,self.proxy_config,self.cookies,self.cancellation));return
            url,method,headers,body=prepare_http(d,v)
            req = urllib.request.Request(url, data=body, headers=headers, method=method)
            try: response = open_http(req,d,v,self.proxy_config)
            except urllib.error.HTTPError as e: response = e
            with response:
                content = response.read(); wire_size=len(content)
                content = decode_http_content(content,response.headers.get('Content-Encoding',''))
                result = {'status': response.status, 'reason': response.reason, 'headers': dict(response.headers),
                          'body': content.decode(response.headers.get_content_charset() or 'utf-8', errors='replace'),
                          'size': wire_size, 'url': url, 'duration': round((time.monotonic()-t)*1000)}
            self.done.emit(result)
        except Exception as e:
            message='Request cancelled.' if self.cancellation.cancelled else rpc_error(e) if self.data.get('kind')=='grpc' else str(e)
            result=failure_result(e,self.data.get('kind','http'),round((time.monotonic()-t)*1000,2),message,self.cancellation.cancelled)
            if USE_REQON_UI:
                from ui_error_states import decorate_failure
                result=decorate_failure(result,e)
            self.done.emit(result)


from general_controller import GeneralController


class MainWindow(GeneralController, Autosave, CollectionActions, QMainWindow):
    def nativeEvent(self,event_type,message):
        controller=getattr(self,'_resize_controller',None)
        if controller and controller.native:
            result=controller.native.event(message)
            if result is not None:return result
        return super().nativeEvent(event_type,message)

    def __init__(self, store=None):
        from ui_typography import load_fonts
        if USE_REQON_UI:load_fonts()
        super().__init__()
        self.store = store or Store()
        from proxy_support import ProxyPreferences
        self.proxy_preferences=ProxyPreferences(self.store)
        self.init_general_settings()
        self.workers = []
        self.console_entries = []
        self.init_autosave()
        self.setWindowTitle(DISPLAY_NAME+' — Local API Client')
        available = QApplication.primaryScreen().availableGeometry()
        self.resize(min(1370, available.width()-40), min(990, available.height()-40))
        build_shell(self)
        self.connect_collection_tree()
        self.make_menu(); self.menu_button.setMenu(self.file_menu); self.menuBar().hide()
        self.statusBar().hide()
        self.refresh_tree(); self.refresh_history(); self.refresh_environments(); self.restore_workspace()
        self.general_settings_changed()

    def make_menu(self):
        file = self.menuBar().addMenu('&File')
        self.file_menu = file
        actions = [('New HTTP Request', self.new_request, 'Ctrl+N'), ('New gRPC Request', self.new_grpc_request, 'Ctrl+Shift+N'), ('New Collection', self.new_collection, ''),
                   ('Import cURL / gRPCurl', self.show_import_dialog, 'Ctrl+Shift+I'),
                   ('Import Postman Collection', self.import_collection, 'Ctrl+O'), ('Export Collection', self.export_collection, ''),
                   ('Export collections…', self.export_collections, 'Ctrl+Shift+E'),
                   ('Open local link', self.open_local_link, 'Ctrl+L'),
                   ('Open local draft', self.open_draft_dialog, ''),
                   ('Code snippet', self.show_code_snippet, ''), ('Copy as cURL / gRPCurl', self.copy_curl, ''), ('Quit', self.close, 'Ctrl+Q')]
        actions.insert(-1,('Settings',self.show_settings,'Ctrl+,'))
        for label, callback, shortcut in actions:
            action = QAction(label, self); action.triggered.connect(lambda checked=False,callback=callback:callback())
            if shortcut: action.setShortcut(QKeySequence(shortcut))
            file.addAction(action)
            self.addAction(action)
        view = self.menuBar().addMenu('&View')
        action = QAction('Send Request', self); action.setShortcut(QKeySequence('Ctrl+Return')); action.triggered.connect(lambda: self.send_request(self.current_page()) if self.current_page() else None); view.addAction(action)
        self.addAction(action)
        save = QAction('Save Request', self); save.setShortcut(QKeySequence('Ctrl+S'))
        save.triggered.connect(lambda:self.save_request(self.workbench.currentWidget()) if self.workbench.currentWidget() else None)
        self.addAction(save)

    def select_nav(self, name, toggle=False):
        if toggle and name==self._active_nav and not self._sidebar_collapsed:
            self.set_sidebar_collapsed(True);return
        self._active_nav=name;self.set_sidebar_collapsed(False)
        if USE_REQON_UI:
            if name=='Environments':self.environments.navigate();return
            self.collection_searchbar.setVisible(name=='Collections')
        if name == 'Environments':
            self.manage_environment()
            self.rail_buttons['Collections' if self.side_tabs.currentWidget() is self.tree else 'History'].setChecked(True)
            return
        self.side_tabs.setCurrentWidget(self.history if name == 'History' else self.tree)
        self.rail_buttons[name].setChecked(True)

    def show_docs(self):
        page=self.current_page()
        if isinstance(page,GrpcPage):page.tabs.setCurrentIndex(0)

    def toggle_sidebar(self):self.set_sidebar_collapsed(not self._sidebar_collapsed)

    def set_sidebar_collapsed(self,collapsed):
        if self._sidebar_collapsed==collapsed:return
        left=self.sidebar_container;sizes=self.workspace_splitter.sizes()
        if collapsed:self._sidebar_width=sizes[0]
        self._sidebar_collapsed=collapsed;self.sidebar_head.setVisible(not collapsed);self.sidebar_content.setVisible(not collapsed)
        left.setMinimumWidth(76 if collapsed else 290);left.setMaximumWidth(76 if collapsed else 16777215)
        width=76 if collapsed else self._sidebar_width;self.workspace_splitter.setSizes([width,max(200,sum(sizes)-width)])

    def show_console(self):
        visible=not self.console_panel.isVisible();self.console_panel.setVisible(visible);self.console_button.setChecked(visible)
        if visible:self.console_panel.refresh();self.console_splitter.setSizes([max(200,self.console_splitter.height()-280),280])

    def show_info(self):
        from ui_workspace_panels import InfoDialog
        self.info_dialog=InfoDialog(self);self.info_dialog.open()

    def configure_tab(self, page, draft_id=None):
        self.configure_general_page(page)
        # A replacement tab may have a new protocol. Compare with the actual
        # stored request, not a UI snapshot that has never been written.
        page._saved=json.loads(self.store.entity('request',page.request_id)['data']) if page.request_id else page.collect()
        index=self.workbench.indexOf(page)
        self.set_request_tab_icon(page)
        close=tool('close','Close request',lambda:self.close_request_tab(self.workbench.indexOf(page)))
        close.setFixedSize(18,18)
        self.workbench.tabBar().setTabButton(index,QTabBar.ButtonPosition.RightSide,close)
        self.attach_autosave(page,draft_id)

    def confirm_close(self,page):
        if page.busy:
            self.set_save_status(page,'Request in progress'); return False
        return self.close_general_page(page)

    def close_request_tab(self,index):
        page=self.workbench.widget(index)
        if page is None or not self.confirm_close(page):return
        self.workbench.removeTab(index); page.deleteLater()
        if self.workbench.count()==0:self.new_request()
        self.save_workspace()

    def closeEvent(self,event):
        settings=getattr(self,'settings_overlay',None)
        if settings and not settings.save_pending():event.ignore();return
        if not all(self.close_general_page(self.workbench.widget(i)) for i in range(self.workbench.count())) or not self.save_workspace():
            event.ignore(); self.show(); return
        if self.workers or Job.running:
            for worker in self.workers:worker.cancel()
            self.hide(); self._idle_close_timer.start()
            event.ignore(); return
        self._session_timer.stop(); self._idle_close_timer.stop()
        event.accept()

    def current_page(self):
        page = self.workbench.currentWidget()
        return page if isinstance(page, (RequestPage, GrpcPage)) else None

    def new_request(self, data=None, request_id=None, collection_id=None, folder_id=None, draft_id=None):
        page = RequestPage(data, request_id)
        page.collection_id, page.folder_id = collection_id, folder_id
        page.send_requested.connect(self.send_request); page.save_requested.connect(self.save_request)
        page.switch_requested.connect(self.switch_request_type)
        idx = self.workbench.addTab(page, page.data['name']); self.workbench.setCurrentIndex(idx)
        self.configure_tab(page,draft_id)
        page.name.textChanged.connect(lambda name: self.workbench.setTabText(self.workbench.indexOf(page), name or 'Untitled Request'))
        return page

    def new_grpc_request(self, data=None, request_id=None, collection_id=None, folder_id=None, draft_id=None):
        page = GrpcPage(data, request_id)
        page.variables_provider=lambda:self.variables(page.collection_id)
        page.proxy_provider=lambda:self.proxy_preferences.config
        page.definition_directory=self.store.path.parent/'definitions'
        self.env_combo.currentIndexChanged.connect(page.source_changed)
        page.generate_collection.connect(self.generate_grpc_collection)
        page.collection_id, page.folder_id = collection_id, folder_id
        page.send_requested.connect(self.send_request); page.save_requested.connect(self.save_request)
        page.switch_requested.connect(self.switch_request_type)
        idx = self.workbench.addTab(page, page.data['name']); self.workbench.setCurrentIndex(idx)
        self.configure_tab(page,draft_id)
        page.name.textChanged.connect(lambda name: self.workbench.setTabText(self.workbench.indexOf(page), name or 'Untitled gRPC Request'))
        return page

    @Slot(object,object)
    def generate_grpc_collection(self,data,methods):
        name,ok=QInputDialog.getText(self,'Generate gRPC collection','Collection name:',text=data.get('server') or 'gRPC services')
        if not ok or not name.strip():return
        cid=self.store.insert('collections',{'name':name.strip(),'kind':'grpc'})
        folders={}
        for service,method in methods:
            if service not in folders:
                folders[service]=self.store.insert('folders',{'collection_id':cid,'name':service})
            request=dict(data,name=method,service=service,rpc_method=method,message='')
            self.store.save_request(request,collection_id=cid,folder_id=folders[service])
        self.refresh_tree(); self.select_nav('Collections')

    def switch_request_type(self, page, kind):
        if page.busy:return
        old_index = self.workbench.indexOf(page)
        if old_index < 0: return
        if not self.flush_page(page):return
        old = page.collect()
        name = old['name']
        if name.startswith('Untitled '): name = 'Untitled Request' if kind == 'http' else 'Untitled gRPC Request'
        if kind == 'http':
            address=old.get('server','')
            if address.startswith(('grpc://','grpcs://','http://','https://')):
                address=('https://' if old.get('tls') else 'http://')+address.split('://',1)[1]
            elif address:address=('https://' if old.get('tls') else 'http://')+address
            data = dict(DEFAULT, name=name, url=address,
                        headers=[list(row) for row in old.get('metadata',[])],
                        header_rows=[list(row) for row in old.get('metadata_rows',old.get('metadata',[]))],
                        auth_type=old.get('auth_type','No Auth'),auth_value=old.get('auth_value',''),
                        body_type='raw', body=old.get('message',''),http_verify=old.get('tls_verify',False),
                        http_ca_file=old.get('tls_ca_file',''),http_cert_file=old.get('tls_cert_file',''),http_key_file=old.get('tls_key_file',''))
        else:
            data = dict(GRPC_DEFAULT, name=name, server=old.get('url',''),
                        metadata=[list(row[:2]) for row in old.get('headers',[])],
                        metadata_rows=[list(row) for row in old.get('header_rows',old.get('headers',[]))],
                        message=old.get('body',''),tls_verify=old.get('http_verify',True),
                        tls_ca_file=old.get('http_ca_file',''),tls_cert_file=old.get('http_cert_file',''),tls_key_file=old.get('http_key_file',''))
            data.update(auth_type=old.get('auth_type','No Auth'),auth_value=old.get('auth_value',''))
        self.workbench.removeTab(old_index)
        page.deleteLater()
        create=self.new_request if kind=='http' else self.new_grpc_request
        new_page=create(data,page.request_id,page.collection_id,page.folder_id,page._draft_id)
        self.workbench.tabBar().moveTab(self.workbench.indexOf(new_page), old_index)
        self.workbench.setCurrentWidget(new_page)
        self.refresh_tree();self.save_workspace()

    def save_request(self, page):
        if USE_REQON_UI and getattr(page,'is_environment_page',False):return page.flush()
        if isinstance(page, ExamplePage): return self.save_example(page)
        if isinstance(page, CollectionPage): return
        if not self.flush_page(page,force=True):return
        collection_id=page.collection_id
        if not collection_id:
            collections = self.store.all('SELECT id,name FROM collections ORDER BY sort_order,id')
            names = ['+ New collection'] + [f'{name}  [{cid}]' for cid,name in collections]
            chosen, ok = QInputDialog.getItem(self, 'Save request', 'Collection:', names, 0, False)
            if not ok: return
            if chosen == names[0]:
                name, ok = QInputDialog.getText(self, 'New collection', 'Collection name:')
                if not ok or not name.strip(): return
                collection_id = self.store.insert('collections', {'name':name.strip(), 'kind':page.kind})
            else: collection_id = collections[names.index(chosen)-1][0]
        existing_kind = self.store.all('SELECT kind FROM collections WHERE id=?', (collection_id,))
        if not existing_kind:
            QMessageBox.warning(self, 'Collection missing', 'Select a collection for this request')
            page.collection_id = None
            page.folder_id = None; page.request_id = None
            return
        page.collection_id=collection_id
        # Read widgets after the destination dialog, including any completed
        # asynchronous example generation, and acknowledge only committed data.
        if not self.flush_page(page,force=True):return
        self.save_workspace()
        self.refresh_tree(); self.statusBar().showMessage('Request saved locally', 4000)

    def variables(self, collection_id=None):
        result = self.environments.values(None) if USE_REQON_UI else {}
        if collection_id:
            collection = self.store.all('SELECT variables FROM collections WHERE id=?', (collection_id,))
            if collection: result.update(json.loads(collection[0][0]))
        id = self.env_combo.currentData()
        if not id: return result
        if USE_REQON_UI:
            result.update(self.environments.values(id));return result
        row = self.store.all('SELECT variables FROM environments WHERE id=?', (id,))
        if row: result.update(json.loads(row[0][0]))
        return result

    def refresh_environments(self):
        if USE_REQON_UI:return self.environments.refresh()
        selected = self.env_combo.currentData()
        self.env_combo.clear(); self.env_combo.addItem('No Environment', None)
        for id, name in self.store.all('SELECT id,name FROM environments ORDER BY name'): self.env_combo.addItem(name, id)
        index = self.env_combo.findData(selected)
        if index >= 0: self.env_combo.setCurrentIndex(index)

    def manage_environment(self):
        if USE_REQON_UI:return self.environments.navigate()
        names = ['+ New environment'] + [n for _,n in self.store.all('SELECT id,name FROM environments ORDER BY name')]
        chosen, ok = QInputDialog.getItem(self, 'Environments', 'Select environment to edit:', names, 0, False)
        if not ok: return
        if chosen == names[0]:
            name, ok = QInputDialog.getText(self, 'New environment', 'Name:')
            if not ok or not name.strip(): return
            id = self.store.insert('environments', {'name':name.strip(), 'variables':'{}'})
        else: id = self.store.all('SELECT id FROM environments WHERE name=?', (chosen,))[0][0]
        dialog = QDialog(self); dialog.setWindowTitle('Environment variables'); dialog.resize(560,400)
        layout = QVBoxLayout(dialog); table = make_table(['Variable', 'Value']); layout.addWidget(table)
        current = self.store.all('SELECT variables FROM environments WHERE id=?', (id,))[0][0]
        set_rows(table, json.loads(current).items())
        buttons = QHBoxLayout(); delete = QPushButton('Delete environment'); save = QPushButton('Save')
        buttons.addWidget(delete); buttons.addStretch(); buttons.addWidget(save); layout.addLayout(buttons)
        delete.clicked.connect(lambda: dialog.done(2)); save.clicked.connect(dialog.accept)
        result = dialog.exec()
        if result == QDialog.DialogCode.Accepted:
            variables = {k:v for k,v in rows(table)}
            self.store.db.execute('UPDATE environments SET variables=? WHERE id=?', (json.dumps(variables), id)); self.store.db.commit()
        elif result == 2: self.store.delete('environments', id)
        self.refresh_environments()

    def send_request(self, page):
        if not page:return
        if page.busy:
            for worker in self.workers:
                if worker.page is page:worker.cancel()
            return
        if not self.flush_page(page):return
        data = page.collect()
        if not (data.get('server') if data.get('kind') == 'grpc' else data.get('url')):
            page.status.setText('Enter a server URL'); return
        page.status.setText('Sending…')
        page.set_busy(True)
        self.console_entries.append(f"{time.strftime('%H:%M:%S')}  {data.get('method','gRPC')}  {data.get('url',data.get('server',''))}  Sending…")
        self.console_entries[:]=self.console_entries[-1000:];self.console_panel.refresh()
        worker = Worker(data, self.variables(page.collection_id),self.proxy_preferences.config,self.general_preferences.config,self.store.path.parent,self.cookie_store); self.workers.append(worker)
        worker.page=page
        worker.done.connect(self.on_request_result,Qt.ConnectionType.QueuedConnection)
        worker.finished.connect(self.on_worker_finished,Qt.ConnectionType.QueuedConnection)
        worker.start()

    @Slot(object)
    def on_request_result(self,result):
        worker=self.sender()
        self.request_finished(worker.page,worker.data,result,worker)

    @Slot()
    def on_worker_finished(self):
        worker=self.sender()
        if worker in self.workers:self.workers.remove(worker)
        worker.deleteLater()

    def request_finished(self, page, data, result, worker):
        page.show_response(result)
        self.console_entries.append(f"{time.strftime('%H:%M:%S')}  {data.get('method','gRPC')}  {data.get('url',data.get('server',''))}  {result.get('status','ERROR')}  {result['duration']} ms")
        if result.get('error'):self.console_entries[-1]+='\n  '+result['error']
        self.console_entries[:]=self.console_entries[-1000:];self.console_panel.refresh()
        self.store.insert('history', {'method':data.get('method', 'gRPC'), 'url':result.get('url',data.get('url',data.get('server',''))),
            'status':result.get('status',-1), 'duration':result['duration'], 'data':json.dumps(data)})
        self.refresh_history()

    def refresh_history(self):
        self.history.clear()
        for id, method, url, status in self.store.all('SELECT id,method,url,status FROM history ORDER BY id DESC LIMIT 200'):
            display='OK' if method=='gRPC' and status==0 else str(status) if status>0 else 'Error'
            item = QTreeWidgetItem(self.history, [f'{method}  {url}', display]); item.setData(0, Qt.ItemDataRole.UserRole, id)

    def open_history(self, item):
        row = self.store.all('SELECT data FROM history WHERE id=?', (item.data(0, Qt.ItemDataRole.UserRole),))
        if row:
            data = json.loads(row[0][0]); (self.new_grpc_request if data.get('kind') == 'grpc' else self.new_request)(data)

    def copy_curl(self):
        page = self.current_page()
        if not page: return
        try:
            label,command=generate_snippet(page.runtime_data_provider(),self.variables(page.collection_id))
            QApplication.clipboard().setText(command); self.statusBar().showMessage(label+' copied',4000)
        except ValueError:self.show_code_snippet()

    def show_import_dialog(self):
        from ui_command_import import CommandImportDialog
        dialog=CommandImportDialog(self)
        if dialog.exec() and dialog.imported:
            self.import_command_request(dialog.imported)

    def show_settings(self):
        from ui_settings import SettingsOverlay
        if not hasattr(self,'settings_overlay'):self.settings_overlay=SettingsOverlay(self)
        self.settings_overlay.present()

    def proxy_settings_changed(self):
        for index in range(self.workbench.count()):
            page=self.workbench.widget(index)
            if isinstance(page,GrpcPage):page.source_changed()

    def import_command_request(self,imported):
        data=dict(imported.data,general_overrides=True)
        page=(self.new_grpc_request if data.get('kind')=='grpc' else self.new_request)(data)
        page.tabs.setCurrentIndex(1 if data.get('kind')=='grpc' else 3 if data.get('body_type')!='none' else 0)
        self.flush_page(page);self.save_workspace()
        return page

    def apply_command_to_page(self,page,imported):
        """Apply a parsed command without changing the request's persistent identity."""
        index=self.workbench.indexOf(page)
        if index<0 or page is not self.current_page():raise ValueError('Select the original request tab to apply this command.')
        if page.busy:raise ValueError('Wait for the current request to finish before editing its command.')
        data=page.collect();saved=page._saved;response=getattr(page,'last_response',None);editor_tab=page.tabs.currentIndex()
        for key in ('param_rows','header_rows','form_rows','metadata_rows','raw_language'):data.pop(key,None)
        data.update(imported.data);data['name']=page.name.text();data['general_overrides']=True
        page._autosave_timer.stop();self._applying_command=True
        try:
            factory=self.new_grpc_request if data.get('kind')=='grpc' else self.new_request
            replacement=factory(data,page.request_id,page.collection_id,page.folder_id,page._draft_id)
            replacement._saved=saved;replacement._preview_pinned=True
            replacement.tabs.setCurrentIndex(min(editor_tab,replacement.tabs.count()-1))
            if response:replacement.show_response(response)
            self.workbench.removeTab(index);self.workbench.tabBar().moveTab(self.workbench.indexOf(replacement),index);self.workbench.setCurrentWidget(replacement);page.deleteLater()
        finally:self._applying_command=False
        self.queue_autosave(replacement);return replacement

    def show_code_snippet(self):
        if self.width()<1240 and not self._sidebar_collapsed:
            self._snippet_restore_sidebar=True;self.set_sidebar_collapsed(True)
        self.snippet_panel.show(); self.code_button.setChecked(True); self.snippet_panel.refresh()

    def hide_code_snippet(self):
        self.snippet_panel.hide(); self.code_button.setChecked(False)
        if getattr(self,'_snippet_restore_sidebar',False):
            self.set_sidebar_collapsed(False); self._snippet_restore_sidebar=False

    def toggle_code_snippet(self):
        if self.snippet_panel.isVisible():self.hide_code_snippet()
        else:self.show_code_snippet()




def main():
    import argparse
    parser = argparse.ArgumentParser(description=DISPLAY_NAME+' local native API client')
    parser.add_argument('--data-dir', type=Path, help='Use a separate local workspace directory')
    options, qt_arguments = parser.parse_known_args()
    windows_identity()
    app = QApplication([sys.argv[0], *qt_arguments])
    from ui_typography import load_fonts
    if USE_REQON_UI:load_fonts()
    app.setApplicationName(DISPLAY_NAME); app.setOrganizationName(DISPLAY_NAME)
    app.setApplicationVersion(VERSION)
    if USE_REQON_UI:
        app.setApplicationDisplayName(DISPLAY_NAME);app.setWindowIcon(brand_icon(True))
    app.setStyle('Fusion'); app.setStyleSheet(STYLE)
    try:
        store = Store(options.data_dir)
    except (OSError, sqlite3.Error) as error:
        QMessageBox.critical(None, DISPLAY_NAME, 'Could not open the local workspace.\n\n'+str(error))
        return 1
    window = MainWindow(store); window.show()
    sys.exit(app.exec())


if __name__ == '__main__': sys.exit(main())
