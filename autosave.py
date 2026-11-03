"""GUI-thread autosave, persistent local drafts and restoration of open tabs."""
from ui_theming import set_style
import json
import uuid
from PySide6.QtCore import QTimer,QSignalBlocker
from PySide6.QtWidgets import QInputDialog, QLabel, QLineEdit, QApplication, QAbstractItemView
from ui_components import KeyValueTable
from ui_pages import RequestPage, GrpcPage, Job
from ui_collections import CollectionPage, ExamplePage, ROLE
from collection_store import TABLES
from ui_brand import USE_REQON_UI,stylesheet
from deferred_page import DeferredPage


class Autosave:
    def init_autosave(self):
        self._restoring_workspace=True
        self._session_timer=QTimer(self); self._session_timer.setSingleShot(True); self._session_timer.setInterval(300)
        self._session_timer.timeout.connect(self.save_workspace)
        self._idle_close_timer=QTimer(self); self._idle_close_timer.setInterval(100)
        self._idle_close_timer.timeout.connect(self.close_when_idle)

    def attach_autosave(self,page,draft_id=None):
        if isinstance(page,CollectionPage):return
        page._draft_id=draft_id or uuid.uuid4().hex
        page._autosave_timer=QTimer(page); page._autosave_timer.setSingleShot(True); page._autosave_timer.setInterval(300)
        page._autosave_timer.timeout.connect(lambda:self.flush_page(page))
        if isinstance(page,RequestPage):
            fields=[page.name,page.url,page.method,page.auth_type,page.auth_value,page.body_type,page.body,
                    page.params,page.headers,page.form,page.http_verify,page.http_follow_redirects,page.http_timeout,
                    page.http_ca_file,page.http_cert_file,page.http_key_file]
            fields.extend([page.general_overrides,page.http_version,page.http_schema,page.body_file])
            fields.extend(page.http_extra_fields.values())
            for panel in (page.auth_panel,page.body_modes,page.headers_panel):panel.changed.connect(lambda page=page:self.queue_autosave(page))
        elif isinstance(page,GrpcPage):
            fields=[page.name,page.server,page.service,page.rpc_method,page.tls,page.auth_type,page.auth_value,
                    page.message,page.metadata,page.descriptor_file,page.tls_ca_file,page.tls_cert_file,page.tls_key_file,
                    page.tls_verify,page.tls_server_name,page.grpc_emit_defaults,page.grpc_max_response_mb,page.grpc_timeout_ms]
            fields.append(page.general_overrides)
        else:
            fields=[page.name,page.request,page.response,page.status,page.reason,page.headers,page.trailers]
        for field in fields:
            if isinstance(field,KeyValueTable):signal=field.changed
            elif hasattr(field,'textChanged'):signal=field.textChanged
            elif hasattr(field,'currentIndexChanged'):signal=field.currentIndexChanged
            elif hasattr(field,'toggled'):signal=field.toggled
            else:signal=field.valueChanged
            signal.connect(lambda *_,page=page:self.queue_autosave(page))
        if not getattr(self,'_applying_command',False):self.flush_page(page,force=True)

    def set_save_status(self,page,text,error=False):
        if USE_REQON_UI:
            page._presentation_dirty=error or text=='Saving…'
            self.workbench.tabBar().schedule()
        label=getattr(page,'save_state',None)
        if label:
            label.setText(text); set_style(label,stylesheet('color:#ef9a9a;font-size:11px;' if error else 'color:#969696;font-size:11px;'))

    def queue_autosave(self,page):
        if self.workbench.indexOf(page)<0:return
        page._preview_pinned=True
        self.snippet_panel.schedule()
        if not self.general_preferences.config.autosave:
            self.set_save_status(page,'Unsaved changes');page._presentation_dirty=True;self.workbench.tabBar().schedule();self.update_tree_name(page);return
        self.set_save_status(page,'Saving…'); page._autosave_timer.start()
        self.update_tree_name(page)
        if not self._restoring_workspace:self._session_timer.start()

    def update_tree_name(self,page):
        if getattr(page,'request_id',None):key=('request',page.request_id)
        else:key=getattr(page,'entity_key',None)
        if not key or key[1] is None:return
        item=self.tree_item(*key)
        if item:
            name=page.name.text().strip() or ('Untitled gRPC Request' if isinstance(page,GrpcPage) else 'Untitled Request' if isinstance(page,RequestPage) else 'Example')
            self.tree.blockSignals(True); item.setText(0,name); item.setToolTip(0,name); self.tree.blockSignals(False)
            if USE_REQON_UI and isinstance(page,(RequestPage,GrpcPage)):
                from ui_collections import METHOD
                item.setData(0,METHOD,'gRPC' if isinstance(page,GrpcPage) else page.method.currentText())

    def flush_page(self,page,force=False):
        if isinstance(page,DeferredPage):return True
        if USE_REQON_UI and getattr(page,'is_environment_page',False):return page.flush()
        if isinstance(page,CollectionPage):return True
        if not force and not self.general_preferences.config.autosave:return True
        timer=getattr(page,'_autosave_timer',None)
        if timer:timer.stop()
        try:
            data=page.collect(); draft_id=getattr(page,'_draft_id',None)
            if not draft_id:page._draft_id=draft_id=uuid.uuid4().hex
            with self.store.atomic():
                if isinstance(page,(RequestPage,GrpcPage)):
                    if page.request_id:
                        # A deleted request becomes a draft; it is never silently recreated in the tree.
                        record=self.store.all('SELECT collection_id,folder_id FROM requests WHERE id=?',(page.request_id,))
                        if record:page.collection_id,page.folder_id=record[0]
                        else:page.request_id=None; page.collection_id=None; page.folder_id=None
                    if page.collection_id and not self.store.all('SELECT id FROM collections WHERE id=?',(page.collection_id,)):
                        page.collection_id=None; page.folder_id=None; page.request_id=None
                    if page.collection_id:
                        if force or data!=page._saved or not page.request_id:
                            page.request_id=self.store.save_request(data,page.request_id,page.collection_id,page.folder_id)
                        self.store.db.execute('DELETE FROM local_drafts WHERE uid=?',(draft_id,))
                    else:self.write_draft(draft_id,data.get('kind','http'),data['name'],data)
                else:
                    snapshot=page.autosave_snapshot()
                    id=page.entity_key[1]
                    if id and self.store.all('SELECT id FROM examples WHERE id=?',(id,)):
                        self.store.db.execute('UPDATE examples SET name=?,data=? WHERE id=?',
                            (data['name'].strip() or 'Example',json.dumps(snapshot),id))
                        self.store.db.execute('DELETE FROM local_drafts WHERE uid=?',(draft_id,))
                    else:
                        page.entity_key=('example',None)
                        self.write_draft(draft_id,'example',data['name'] or 'Example',snapshot)
            page._saved=data
            self.set_save_status(page,'Draft saved locally' if not getattr(page,'request_id',None) and not getattr(page,'entity_key',(None,None))[1] else 'Saved locally')
            self.update_tree_name(page)
            return True
        except Exception as error:
            self.set_save_status(page,'Autosave failed: '+str(error),True)
            return False

    def write_draft(self,uid,kind,name,data):
        self.store.db.execute('''INSERT INTO local_drafts(uid,kind,name,data) VALUES(?,?,?,?)
            ON CONFLICT(uid) DO UPDATE SET kind=excluded.kind,name=excluded.name,data=excluded.data,updated_at=CURRENT_TIMESTAMP
            WHERE kind<>excluded.kind OR name<>excluded.name OR data<>excluded.data''',
            (uid,kind,name,json.dumps(data)))

    def flush_all(self):
        # Inline tree renaming may still have an editor open when Export/Close is invoked.
        editor=QApplication.focusWidget()
        if isinstance(editor,QLineEdit) and self.tree.isAncestorOf(editor) and self.tree.state()==QAbstractItemView.State.EditingState:
            self.tree.itemDelegate().commitData.emit(editor)
        success=True
        for index in range(self.workbench.count()):
            if not self.flush_page(self.workbench.widget(index),force=True):success=False
        return self.save_workspace() and success

    def save_workspace(self,*_):
        if self._restoring_workspace:return True
        try:
            entries=[]
            for index in range(self.workbench.count()):
                page=self.workbench.widget(index)
                if isinstance(page,DeferredPage):
                    entries.append(page.session_entry);continue
                if USE_REQON_UI and getattr(page,'is_environment_page',False):
                    entries.append({'kind':'environment','uid':page.uid});continue
                if getattr(page,'request_id',None):key=('request',page.request_id)
                else:key=getattr(page,'entity_key',None)
                if key and key[1]:
                    rows=self.store.all(f'SELECT uid FROM {TABLES[key[0]]} WHERE id=?',(key[1],))
                    if not rows:raise ValueError('This item no longer exists.')
                    entries.append({'kind':key[0],'uid':rows[0][0]})
                else:entries.append({'kind':'draft','uid':page._draft_id})
            data={'tabs':entries,'active':self.workbench.currentIndex(),'environment':self.env_combo.currentData()}
            serialized=json.dumps(data)
            if serialized==getattr(self,'_saved_session',None):return True
            with self.store.atomic():self.store.db.execute("INSERT OR REPLACE INTO workspace_state(key,data) VALUES('session',?)",(serialized,))
            self._saved_session=serialized
            return True
        except Exception as error:
            for index in range(self.workbench.count()):self.set_save_status(self.workbench.widget(index),'Autosave failed: '+str(error),True)
            return False

    def restore_workspace(self):
        rows=self.store.all("SELECT data FROM workspace_state WHERE key='session'")
        data=json.loads(rows[0][0]) if rows else {}
        for entry in data.get('tabs',[]):
            kind=entry.get('kind'); uid=entry.get('uid')
            if USE_REQON_UI and kind=='environment':
                id=self.environments.data.from_uid(uid)
                if id is not False:self.environments.open(id)
            elif kind=='draft':
                records=self.store.all('SELECT name,kind FROM local_drafts WHERE uid=?',(uid,))
                if records and records[0][1] in ('http','grpc'):
                    self.add_deferred_tab(entry,*records[0])
                else:self.open_draft(uid)
            elif kind in TABLES:
                rows=self.store.all(f'SELECT id FROM {TABLES[kind]} WHERE uid=?',(uid,))
                if rows:
                    item=self.tree_item(kind,rows[0][0])
                    if item:
                        if kind=='request':
                            record=self.store.entity(kind,rows[0][0]);payload=json.loads(record['data'])
                            self.add_deferred_tab(entry,record['name'],payload.get('kind','http'),record['id'],record['collection_id'],record['folder_id'])
                        else:self.open_item(item)
        if not self.workbench.count():self.new_grpc_request()
        self.workbench.setCurrentIndex(min(max(data.get('active',0),0),self.workbench.count()-1))
        self.materialize_tab(self.workbench.currentIndex())
        index=self.env_combo.findData(data.get('environment'))
        if index>=0:self.env_combo.setCurrentIndex(index)
        self._restoring_workspace=False
        self.workbench.currentChanged.connect(self.materialize_tab)
        self.workbench.currentChanged.connect(lambda _:self._session_timer.start())
        self.workbench.tabBar().tabMoved.connect(lambda *_:self._session_timer.start())
        self.env_combo.currentIndexChanged.connect(lambda _:self._session_timer.start())
        self.save_workspace()

    def add_deferred_tab(self,entry,name,kind,request_id=None,collection_id=None,folder_id=None):
        from ui_components import tool
        from PySide6.QtWidgets import QTabBar
        page=DeferredPage(self,entry,name,kind,request_id,collection_id,folder_id)
        index=self.workbench.addTab(page,name)
        self.set_request_tab_icon(page)
        close=tool('close','Close request',lambda:self.close_request_tab(self.workbench.indexOf(page)))
        self.workbench.tabBar().setTabButton(index,QTabBar.ButtonPosition.RightSide,close)
        return page

    def materialize_tab(self,index):
        page=self.workbench.widget(index)
        if not isinstance(page,DeferredPage) or getattr(self,'_materializing_tab',False):return page
        entry=page.session_entry
        self._materializing_tab=True
        restoring=self._restoring_workspace
        self._restoring_workspace=True
        try:
            # Remove the placeholder from lookup while the normal creation path
            # installs signals, preferences and autosave on the real editor.
            with QSignalBlocker(self.workbench):
                self.workbench.removeTab(index)
                if entry['kind']=='draft':loaded=self.open_draft(entry['uid'])
                else:
                    item=self.tree_item('request',page.request_id)
                    loaded=self.open_item(item) if item else None
                if loaded is None:
                    self.workbench.insertTab(index,page,page.title)
                    self.workbench.setCurrentIndex(index)
                    return page
                self.workbench.tabBar().moveTab(self.workbench.indexOf(loaded),index)
                self.workbench.setCurrentIndex(index)
            page.deleteLater()
        finally:
            self._materializing_tab=False
            self._restoring_workspace=restoring
        self.workbench.currentChanged.emit(index)
        return loaded

    def open_draft(self,uid):
        for index in range(self.workbench.count()):
            page=self.workbench.widget(index)
            if getattr(page,'_draft_id',None)==uid:
                self.workbench.setCurrentIndex(index);return self.materialize_tab(index)
        rows=self.store.all('SELECT kind,data FROM local_drafts WHERE uid=?',(uid,))
        if not rows:return
        kind,payload=rows[0]; data=json.loads(payload)
        if kind=='example':page=self.open_example_draft(data)
        else:page=(self.new_grpc_request if kind=='grpc' else self.new_request)(data)
        initial=page._draft_id; page._draft_id=uid
        with self.store.atomic():self.store.db.execute('DELETE FROM local_drafts WHERE uid=?',(initial,))
        return page

    def open_draft_dialog(self):
        rows=self.store.all('SELECT uid,name,kind FROM local_drafts ORDER BY updated_at DESC,rowid DESC')
        if not rows:return
        labels=[f'{name}  [{kind.upper()} · {uid[:8]}]' for uid,name,kind in rows]
        label,ok=QInputDialog.getItem(self,'Local drafts','Open a saved draft:',labels,0,False)
        if ok:self.open_draft(rows[labels.index(label)][0]); self.save_workspace()

    def close_when_idle(self):
        runner = getattr(self, 'runner_dialog', None)
        if not self.workers and not Job.running and not (runner and runner.controller.running):
            self._idle_close_timer.stop(); self.close()
