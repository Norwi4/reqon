"""Collection actions shared by context menus, hover controls and keyboard shortcuts."""
from ui_brand import PROTOCOL_COLORS
import json
from pathlib import Path
from PySide6.QtCore import Qt, QPoint, QSaveFile, QIODevice
from PySide6.QtGui import QFont, QCursor
from PySide6.QtWidgets import (QApplication, QTreeWidgetItem, QMenu, QInputDialog, QMessageBox,
    QDialog, QVBoxLayout, QLabel, QTreeWidget, QDialogButtonBox, QFileDialog, QTabBar, QToolTip)
from collection_store import TABLES
from ui_components import icon, tool
from ui_collections import ROLE, STAR, PROTOCOL, CollectionPage, ExamplePage, CollectionMenu
from ui_brand import USE_REQON_UI
from ui_collections import METHOD,COUNT


class CollectionActions:
    def connect_collection_tree(self):
        self.tree.menu_requested.connect(self.show_tree_menu)
        self.tree.star_requested.connect(self.toggle_star)
        self.tree.operation_requested.connect(self.tree_operation)
        self.tree.move_requested.connect(self.drop_tree_item)
        self.tree.itemChanged.connect(self.finish_tree_rename)
        self.tree.itemExpanded.connect(lambda item: self.persist_expansion(item, True))
        self.tree.itemCollapsed.connect(lambda item: self.persist_expansion(item, False))

    def tree_items(self):
        def branch(item):
            yield item
            for i in range(item.childCount()): yield from branch(item.child(i))
        for i in range(self.tree.topLevelItemCount()): yield from branch(self.tree.topLevelItem(i))

    def tree_item(self, kind, id):
        if hasattr(self,'_tree_index'):return self._tree_index.get((kind,id))
        return next((item for item in self.tree_items() if item.data(0, ROLE)[:2] == (kind, id)), None)

    def select_tree_item(self, kind, id):
        item = self.tree_item(kind, id)
        if not item: return
        parent = item.parent()
        while parent: parent.setExpanded(True); parent = parent.parent()
        self.tree.setCurrentItem(item); self.tree.scrollToItem(item)

    def refresh_tree(self):
        from collections import defaultdict
        from ui_typography import ui_font
        snapshot=self.store.tree_snapshot()
        if self.update_tree_values(snapshot):return
        children=defaultdict(list);counts=defaultdict(int)
        for kind,rows in snapshot.items():
            for row in rows:
                if kind=='collection':parent=('root',None)
                elif kind=='example':parent=('request',row['request_id'])
                else:
                    folder=row['parent_id' if kind=='folder' else 'folder_id']
                    parent=('folder',folder) if folder is not None else ('collection',row['collection_id'])
                    if kind=='request':counts[row['collection_id']]+=1
                children[parent].append((kind,row))
        for rows in children.values():rows.sort(key=lambda value:(value[1]['sort_order'],value[0],value[1]['id']))
        current=self.tree.currentItem();selected=current.data(0,ROLE)[:2] if current else None
        scroll=self.tree.verticalScrollBar().value()
        font=ui_font(12) if USE_REQON_UI else QFont('Segoe UI');font.setPixelSize(12)
        font.setWeight(QFont.Weight.Normal if USE_REQON_UI else QFont.Weight.DemiBold)
        index={};expanded={};icons={}
        def tree_icon(name,color='#a6a6a6'):
            key=(name,color)
            if key not in icons:icons[key]=icon(name,color)
            return icons[key]
        def node(kind,row):
            id=row['id'];name=row['name'];item=QTreeWidgetItem([name]);key=(kind,id)
            index[key]=item;expanded[key]=bool(row.get('expanded',False))
            value=(kind,id,row['kind']) if kind=='collection' else (kind,id,row['collection_id']) if kind=='folder' else key
            item.setData(0,ROLE,value)
            item.setFlags(item.flags() | Qt.ItemFlag.ItemIsEditable | Qt.ItemFlag.ItemIsDragEnabled)
            if kind in ('request','example'):item.setFlags(item.flags() & ~Qt.ItemFlag.ItemIsDropEnabled)
            item.setToolTip(0,name);item.setFont(0,font)
            if kind=='collection':
                item.setData(0,STAR,row['starred'])
                if USE_REQON_UI:item.setData(0,COUNT,counts[id])
            elif kind=='folder':item.setIcon(0,tree_icon('folder'))
            elif kind=='example':item.setIcon(0,tree_icon('example'))
            else:
                protocol=row['protocol'] or 'http';item.setData(0,PROTOCOL,protocol)
                item.setIcon(0,tree_icon(protocol,PROTOCOL_COLORS[protocol]))
                if USE_REQON_UI:item.setData(0,METHOD,'gRPC' if protocol=='grpc' else row['method'] or 'GET')
            item.addChildren([node(child_kind,child) for child_kind,child in children[key]])
            return item
        roots=[node(kind,row) for kind,row in children[('root',None)]]
        blocked=self.tree.blockSignals(True);updates=self.tree.updatesEnabled();self.tree.setUpdatesEnabled(False)
        try:
            self._tree_index={};self.tree.clear();self.tree.addTopLevelItems(roots)
            # Keep Python QIconEngine owners alive while Qt items share the icons.
            self._tree_index=index;self._tree_expanded=expanded;self._tree_icons=icons
            self._tree_snapshot=snapshot
            if selected in index:self.tree.setCurrentItem(index[selected])
            self.filter_tree(self.search.text());self.sync_tree_tabs()
            self.tree.verticalScrollBar().setValue(scroll)
        finally:
            self.tree.blockSignals(blocked);self.tree.setUpdatesEnabled(updates)

    def update_tree_values(self,snapshot):
        """Keep existing Qt nodes for metadata-only edits and unchanged refreshes."""
        previous=getattr(self,'_tree_snapshot',None)
        if previous is None:return False
        structure=('id','sort_order','collection_id','folder_id','parent_id','request_id')
        changes=[]
        for kind,rows in snapshot.items():
            old=previous.get(kind,[])
            if len(rows)!=len(old):return False
            for before,after in zip(old,rows):
                if any(before.get(key)!=after.get(key) for key in structure):return False
                if before!=after:changes.append((kind,after))
        blocked=self.tree.blockSignals(True)
        try:
            for kind,row in changes:
                item=self._tree_index[(kind,row['id'])]
                item.setText(0,row['name']);item.setToolTip(0,row['name'])
                if kind=='collection':
                    item.setData(0,ROLE,(kind,row['id'],row['kind']))
                    item.setData(0,STAR,row['starred'])
                elif kind=='request':
                    protocol=row['protocol'] or 'http'
                    item.setData(0,PROTOCOL,protocol)
                    key=(protocol,PROTOCOL_COLORS[protocol])
                    if key not in self._tree_icons:self._tree_icons[key]=icon(*key)
                    item.setIcon(0,self._tree_icons[key])
                    if USE_REQON_UI:item.setData(0,METHOD,'gRPC' if protocol=='grpc' else row['method'] or 'GET')
                if kind!='example':self._tree_expanded[(kind,row['id'])]=bool(row.get('expanded',False))
            self._tree_snapshot=snapshot
            if changes:self.filter_tree(self.search.text())
            self.sync_tree_tabs()
        finally:self.tree.blockSignals(blocked)
        return True

    def persist_expansion(self, item, expanded):
        if getattr(self, '_filtering_tree', False) or self.search.text().strip(): return
        kind, id, *_ = item.data(0, ROLE)
        if kind != 'example':
            self._tree_expanded[(kind,id)]=bool(expanded)
            self.store.db.execute(f'UPDATE {TABLES[kind]} SET expanded=? WHERE id=?', (int(expanded), id)); self.store.db.commit()

    def filter_tree(self, text):
        if USE_REQON_UI:self.tree.search_query=text.strip();self.tree.viewport().update()
        query = text.casefold().strip(); self._filtering_tree = True
        def match(item, ancestor_match=False):
            direct = query in item.text(0).casefold()
            descendants = False
            for i in range(item.childCount()): descendants = match(item.child(i), ancestor_match or direct) or descendants
            found = direct or descendants or ancestor_match; item.setHidden(not found)
            kind, id, *_ = item.data(0, ROLE)
            if query:
                if found and item.childCount(): item.setExpanded(True)
            elif kind != 'example' and item.isExpanded()!=self._tree_expanded.get((kind,id),False):
                item.setExpanded(self._tree_expanded.get((kind,id),False))
            return found
        try:
            for i in range(self.tree.topLevelItemCount()): match(self.tree.topLevelItem(i))
        finally: self._filtering_tree = False
        if USE_REQON_UI and hasattr(self,'empty_states'):self.empty_states.schedule()

    def sync_tree_tabs(self):
        if not hasattr(self, 'workbench'): return
        for index in reversed(range(self.workbench.count())):
            page = self.workbench.widget(index)
            from deferred_page import DeferredPage
            if isinstance(page,DeferredPage):
                if page.request_id:
                    rows=self.store.all('SELECT collection_id,folder_id,name FROM requests WHERE id=?',(page.request_id,))
                    if rows:
                        page.collection_id,page.folder_id,page.title=rows[0]
                        self.workbench.setTabText(index,page.title)
                    else:self.workbench.removeTab(index);page.deleteLater()
                continue
            if getattr(page, 'request_id', None):
                rows = self.store.all('SELECT collection_id,folder_id,name FROM requests WHERE id=?', (page.request_id,))
                if not rows:
                    page.request_id = None; page.collection_id = None; page.folder_id = None
                    # Keep unsaved edits in an unbound tab; Ctrl+S will ask for a collection.
                    page._saved = {}
                    if hasattr(page,'_autosave_timer'):self.queue_autosave(page)
                else:
                    page.collection_id, page.folder_id, name = rows[0]
                    if page._saved.get('name') != name:
                        page.name.setText(name); page._saved['name'] = name
                    self.update_tree_name(page)
            elif isinstance(page, (CollectionPage, ExamplePage)):
                kind, id = page.entity_key
                if isinstance(page,ExamplePage) and id is None:continue
                if not self.store.all(f'SELECT id FROM {TABLES[kind]} WHERE id=?', (id,)):
                    if isinstance(page, ExamplePage) and page.collect() != page._saved:
                        page.entity_key = ('example', None)
                        if hasattr(page,'_autosave_timer'):self.queue_autosave(page)
                        continue
                    self.workbench.removeTab(index); page.deleteLater()
                elif isinstance(page, CollectionPage): page.refresh()
                else:
                    name = self.store.entity(kind, id)['name']
                    if page._saved['name'] != name: page.name.setText(name); page._saved['name'] = name

    def open_item(self, item):
        kind, id, *_ = item.data(0, ROLE)
        if USE_REQON_UI and kind=='collection':
            if self.tree._branch_click:return
            item.setExpanded(True);self.tree.setCurrentItem(item)
        if kind == 'folder': return
        for index in range(self.workbench.count()):
            page = self.workbench.widget(index)
            if (kind == 'request' and getattr(page, 'request_id', None) == id) or getattr(page, 'entity_key', None) == (kind, id):
                self.workbench.setCurrentIndex(index); return self.materialize_tab(index)
        if kind == 'collection': page = CollectionPage(self, id)
        elif kind == 'example': page = ExamplePage(self, id)
        else:
            row = self.store.entity(kind, id); data = json.loads(row['data'])
            return self.finish_sidebar_open((self.new_grpc_request if data.get('kind') == 'grpc' else self.new_request)(data, id, row['collection_id'], row['folder_id']))
        index = self.workbench.addTab(page, self.store.entity(kind, id)['name']); self.workbench.setCurrentIndex(index)
        self.workbench.setTabIcon(index, icon(('collection' if USE_REQON_UI else 'folder') if kind == 'collection' else 'example'))
        close = tool('close', 'Close tab', lambda: self.close_request_tab(self.workbench.indexOf(page))); close.setFixedSize(18, 18)
        self.workbench.tabBar().setTabButton(index, QTabBar.ButtonPosition.RightSide, close)
        self.attach_autosave(page)
        return self.finish_sidebar_open(page)

    def open_example_draft(self,data):
        page=ExamplePage(self,None,data)
        index=self.workbench.addTab(page,page.name.text()); self.workbench.setCurrentIndex(index)
        self.workbench.setTabIcon(index,icon('example'))
        close=tool('close','Close tab',lambda:self.close_request_tab(self.workbench.indexOf(page))); close.setFixedSize(18,18)
        self.workbench.tabBar().setTabButton(index,QTabBar.ButtonPosition.RightSide,close)
        self.attach_autosave(page)
        return page

    def tree_operation(self, operation, item):
        kind, id, *_ = item.data(0, ROLE)
        if operation == 'rename': self.start_tree_rename(item)
        elif operation == 'duplicate': self.duplicate_item(kind, id)
        elif operation == 'delete': self.delete_item(kind, id, item.text(0))
        elif operation == 'open': self.open_item(item)

    def tree_menu(self, pos):
        item = self.tree.itemAt(pos)
        if item: self.show_tree_menu(item, self.tree.viewport().mapToGlobal(pos))
        else:
            menu = QMenu(self); menu.addAction('New collection', self.new_collection); menu.addAction('Import', self.import_collection)
            menu.addAction('Export collections…',self.export_collections)
            menu.popup(self.tree.viewport().mapToGlobal(pos)); self._tree_menu = menu

    def collection_menu(self, item):
        kind, id, *_ = item.data(0, ROLE); row = self.store.entity(kind, id)
        menu = CollectionMenu(self); menu.setObjectName('collectionMenu'); menu.setMinimumWidth(190)
        def action(label, callback, shortcut=''):
            result = menu.addAction(label + ('\t' + shortcut if shortcut else ''))
            result.triggered.connect(lambda checked=False: callback()); return result
        if kind in ('collection', 'folder'):
            cid = id if kind == 'collection' else row['collection_id']; fid = id if kind == 'folder' else None
            add = menu.addMenu('Add request'); add.setObjectName('collectionSubmenu'); add.setMinimumWidth(180)
            for label, protocol in [('HTTP', 'http'), ('gRPC', 'grpc')]:
                act = add.addAction(icon(protocol, PROTOCOL_COLORS[protocol]), label)
                act.triggered.connect(lambda checked=False, protocol=protocol: self.add_tree_request(cid, fid, protocol))
            action('Add folder', lambda: self.add_folder(cid, fid))
        elif kind == 'request': action('Add example', lambda: self.add_example(id))
        if kind != 'example':
            menu.addSeparator(); action('Share / Export…', lambda: self.export_item(kind, id))
        action('Copy link', lambda: self.copy_local_link(kind, id))
        if kind != 'example': action('Move', lambda: self.move_item_dialog(kind, id))
        if kind == 'collection':
            menu.addSeparator(); act = action('Fork locally', lambda: self.duplicate_item(kind, id, ' (fork)'))
            act.setToolTip('Create an independent local copy of this collection')
        menu.addSeparator(); action('Rename', lambda: self.start_tree_rename(item), 'Ctrl+E')
        action('Duplicate', lambda: self.duplicate_item(kind, id), 'Ctrl+D')
        if kind in ('collection', 'folder'):
            sort = menu.addMenu('Sort'); sort.setObjectName('collectionSubmenu'); sort.setMinimumWidth(170)
            for label, reverse in [('Name A → Z', False), ('Name Z → A', True)]:
                sort.addAction(label).triggered.connect(lambda checked=False, reverse=reverse: self.sort_tree_items(kind, id, reverse))
        menu.delete_action = action('Delete', lambda: self.delete_item(kind, id, row['name']), 'Del')
        return menu

    def show_tree_menu(self, item, global_pos):
        self.tree.setCurrentItem(item); self._tree_menu = self.collection_menu(item); self._tree_menu.popup(global_pos)

    def start_tree_rename(self, item):
        self.tree.setCurrentItem(item); self.tree.editItem(item, 0)

    def finish_tree_rename(self, item, column):
        if column: return
        kind, id, *_ = item.data(0, ROLE); old = self.store.entity(kind, id)['name']; name = item.text(0).strip()
        if name and name != old: self.store.rename_entity(kind, id, name)
        self.tree.blockSignals(True); item.setText(0, name or old); item.setToolTip(0, name or old); self.tree.blockSignals(False)
        self.sync_tree_tabs()

    def toggle_star(self, item):
        kind, id, *_ = item.data(0, ROLE)
        if kind != 'collection': return
        starred = not bool(self.store.entity(kind, id)['starred'])
        self.store.db.execute('UPDATE collections SET starred=? WHERE id=?', (int(starred), id)); self.store.db.commit()
        self.tree.blockSignals(True); item.setData(0, STAR, starred); self.tree.blockSignals(False); self.tree.update_hover(item)

    def duplicate_item(self, kind, id, suffix=' Copy'):
        if not self.flush_all():return
        result = self.store.duplicate_entity(kind, id, suffix); self.refresh_tree(); self.select_tree_item(kind, result)
        return result

    def delete_item(self, kind, id, name):
        if not self.flush_all():return
        detail = '\nAll nested folders, requests and examples will also be deleted.' if kind in ('collection', 'folder') else '\nSaved examples will also be deleted.' if kind == 'request' else ''
        if QMessageBox.question(self, 'Delete '+kind, f'Delete “{name}”?{detail}',
                QMessageBox.StandardButton.Yes | QMessageBox.StandardButton.Cancel, QMessageBox.StandardButton.Cancel) != QMessageBox.StandardButton.Yes: return
        self.store.delete_entity(kind, id); self.refresh_tree()
        if not self.workbench.count(): self.new_request()

    def new_collection(self): return self.create_collection('http')

    def create_collection(self, kind):
        if USE_REQON_UI:
            name,ok='New Collection',True
            self.select_nav('Collections');self.search.clear()
        else:name, ok = QInputDialog.getText(self, 'New collection', 'Collection name:', text='New Collection')
        if ok and name.strip():
            id = self.store.insert('collections', {'name': name.strip(), 'kind': kind})
            self.refresh_tree(); self.select_tree_item('collection', id); page=self.open_item(self.tree_item('collection', id))
            if USE_REQON_UI:
                page.title.setFocus(Qt.FocusReason.MouseFocusReason);page.title.selectAll()
            return id

    def add_folder(self, cid, parent_id=None):
        id = self.store.insert('folders', {'collection_id': cid, 'parent_id': parent_id, 'name': 'New Folder'})
        self.refresh_tree(); self.select_tree_item('folder', id); self.start_tree_rename(self.tree_item('folder', id))
        return id

    def add_tree_request(self, cid, fid, protocol):
        from ui_pages import HTTP_DEFAULT, GRPC_DEFAULT
        data = dict(GRPC_DEFAULT if protocol == 'grpc' else HTTP_DEFAULT)
        rid = self.store.save_request(data, collection_id=cid, folder_id=fid)
        self.refresh_tree(); self.select_tree_item('request', rid)
        return self.open_item(self.tree_item('request', rid))

    def sort_tree_items(self, kind, id, reverse=False):
        self.store.sort_children(kind, id, reverse); self.refresh_tree()

    def move_item_dialog(self, kind, id):
        dialog = QDialog(self); dialog.setWindowTitle('Move '+kind); dialog.resize(480, 460)
        layout = QVBoxLayout(dialog); layout.addWidget(QLabel('Place before collection:' if kind == 'collection' else 'Choose destination collection or folder:'))
        tree = QTreeWidget(); tree.setHeaderHidden(True); layout.addWidget(tree)
        blocked = set(self.store.folder_ids(id)) if kind == 'folder' else set()
        def folders(cid, parent, fid=None):
            for typ, child, name, _ in self.store.children(cid, fid):
                if typ != 'folder' or child in blocked: continue
                node = QTreeWidgetItem(parent, [name]); node.setIcon(0, icon('folder')); node.setData(0, ROLE, (cid, child))
                folders(cid, node, child)
        for _, cid, name, _ in self.store.children():
            if kind == 'collection' and cid == id: continue
            node = QTreeWidgetItem(tree, [name]); node.setIcon(0, icon('collection')); node.setData(0, ROLE, (cid, None))
            if kind != 'collection': folders(cid, node); node.setExpanded(True)
        if kind == 'collection':
            node = QTreeWidgetItem(tree, ['At the end']); node.setData(0, ROLE, (None, None))
        buttons = QDialogButtonBox(QDialogButtonBox.StandardButton.Ok | QDialogButtonBox.StandardButton.Cancel)
        buttons.button(QDialogButtonBox.StandardButton.Ok).setText('Move'); buttons.button(QDialogButtonBox.StandardButton.Ok).setEnabled(False)
        tree.currentItemChanged.connect(lambda item, old: buttons.button(QDialogButtonBox.StandardButton.Ok).setEnabled(item is not None))
        buttons.accepted.connect(dialog.accept); buttons.rejected.connect(dialog.reject); layout.addWidget(buttons)
        if dialog.exec() == QDialog.DialogCode.Accepted and tree.currentItem():
            cid, fid = tree.currentItem().data(0, ROLE)
            self.move_tree_entity(kind, id, cid if kind != 'collection' else None, fid, ('collection', cid) if kind == 'collection' and cid else None)

    def move_tree_entity(self, kind, id, cid=None, fid=None, before=None):
        if not self.flush_all():return False
        try: self.store.move_entity(kind, id, cid, fid, before)
        except ValueError as error: QMessageBox.warning(self, 'Cannot move item', str(error)); return False
        self.refresh_tree(); self.select_tree_item(kind, id); return True

    def drop_tree_item(self, source, target, position):
        kind, id, *_ = source.data(0, ROLE); before = None; cid = fid = None
        if kind == 'example': return
        if kind == 'collection':
            if target is not None:
                while target.parent(): target = target.parent()
                target_key = target.data(0, ROLE)[:2]
                if target_key == (kind, id): return
                siblings = [(k, i) for k, i, *_ in self.store.children() if (k, i) != (kind, id)]
                n = siblings.index(target_key) + (position == 'after')
                before = siblings[n] if n < len(siblings) else None
        else:
            if target is None: return
            typ, tid, *_ = target.data(0, ROLE)
            if position == 'inside' and typ in ('collection', 'folder'):
                cid = tid if typ == 'collection' else self.store.entity(typ, tid)['collection_id']; fid = tid if typ == 'folder' else None
            else:
                if typ in ('collection', 'example'): return
                row = self.store.entity(typ, tid); cid = row['collection_id']; fid = row.get('parent_id' if typ == 'folder' else 'folder_id')
                siblings = [(k, i) for k, i, *_ in self.store.children(cid, fid) if (k, i) != (kind, id)]
                n = siblings.index((typ, tid)) + (position == 'after')
                before = siblings[n] if n < len(siblings) else None
        self.move_tree_entity(kind, id, cid, fid, before)

    def copy_local_link(self, kind, id):
        QApplication.clipboard().setText(self.store.local_link(kind, id))
        QToolTip.showText(QCursor.pos(), 'Local link copied · Open with Ctrl+L', self)

    def open_local_link(self):
        clipboard = QApplication.clipboard().text()
        text, ok = QInputDialog.getText(self, 'Open local link', 'reqon:// link from this workspace:', text=clipboard if clipboard.startswith(('reqon://','getman://')) else '')
        if not ok: return
        try:
            key = self.store.resolve_link(text); self.search.clear(); self.select_nav('Collections'); self.select_tree_item(*key); self.open_item(self.tree_item(*key))
        except ValueError as error: QMessageBox.warning(self, 'Local link', str(error))

    def add_example(self, rid):
        row = self.store.entity('request', rid); data = json.loads(row['data']); response = None
        for index in range(self.workbench.count()):
            page = self.workbench.widget(index)
            if getattr(page, 'request_id', None) == rid:
                data = page.collect(); response = getattr(page, 'last_response', None); break
        response = response or {'status': 0 if data.get('kind') == 'grpc' else 200, 'reason': 'OK', 'body': '', 'headers': [], 'trailers': [], 'duration': 0, 'size': 0}
        id = self.store.insert('examples', {'request_id': rid, 'name': 'New Example', 'data': json.dumps({'request': data, 'response': response})})
        self.refresh_tree(); self.select_tree_item('example', id); return self.open_item(self.tree_item('example', id))

    def save_example(self, page):
        if self.flush_page(page):self.refresh_tree(); self.save_workspace()

    def invalidate_collection_variables(self, cid):
        from ui_pages import GrpcPage
        for i in range(self.workbench.count()):
            page = self.workbench.widget(i)
            if isinstance(page, GrpcPage) and page.collection_id == cid: page.source_changed()

    def import_collection(self):
        path, _ = QFileDialog.getOpenFileName(self, 'Import collections', '', 'JSON (*.json)')
        if not path: return
        try:
            ids = self.store.import_documents(json.loads(Path(path).read_text(encoding='utf-8-sig')), Path(path).stem)
            self.refresh_tree(); self.search.clear(); self.select_nav('Collections'); self.select_tree_item('collection', ids[0])
            self.statusBar().showMessage(f'Imported {len(ids)} collection(s)',4000)
        except Exception as error: QMessageBox.critical(self, 'Import failed', str(error))

    def export_collection(self, cid=None):
        if cid is None:
            collections = self.store.all('SELECT id,name FROM collections ORDER BY sort_order,id')
            if not collections: return
            labels = [f'{name}  [{id}]' for id, name in collections]
            chosen, ok = QInputDialog.getItem(self, 'Export collection', 'Collection:', labels, 0, False)
            if not ok: return
            cid = collections[labels.index(chosen)][0]
        self.export_item('collection', cid)

    def export_collections(self):
        from ui_collection_export import CollectionExportDialog
        if not self.flush_all():return
        dialog=CollectionExportDialog(self)
        if not dialog.exec():return
        if not self.flush_all():return
        try:
            document=self.store.export_documents(dialog.selected_ids())
            path,_=QFileDialog.getSaveFileName(self,'Export collections','Reqon.reqon_collections.json','JSON (*.json)')
            if not path:return
            payload=json.dumps(document,ensure_ascii=False,indent=2).encode('utf-8')
            file=QSaveFile(path)
            if not file.open(QIODevice.OpenModeFlag.WriteOnly):raise OSError(file.errorString())
            if file.write(payload)!=len(payload):
                error=file.errorString();file.cancelWriting();raise OSError(error)
            if not file.commit():raise OSError(file.errorString())
            self.statusBar().showMessage(f'Exported {len(document["collections"])} collection(s)',4000)
        except Exception as error:QMessageBox.critical(self,'Export failed',str(error))

    def export_item(self, kind, id):
        if not self.flush_all():return
        row = self.store.entity(kind, id); cid = id if kind == 'collection' else row['collection_id']
        try:
            document = self.store.export_document(cid); native = 'format' in document; key = 'items' if native else 'item'
            if kind != 'collection':
                def find(items, fid=None):
                    for (typ, child, *_), entry in zip(self.store.children(cid, fid), items):
                        if (typ, child) == (kind, id): return entry
                        if typ == 'folder':
                            found = find(entry[key], child)
                            if found: return found
                entry = find(document[key]); document[key] = [entry]
                if native: document['name'] = row['name']
                else: document['info']['name'] = row['name']
            filename = ''.join('_' if c in '<>:"/\\|?*' else c for c in row['name'])
            suffix = '.reqon_collection.json' if native else '.postman_collection.json'
            path, _ = QFileDialog.getSaveFileName(self, 'Export '+kind, filename+suffix, 'JSON (*.json)')
            if path: Path(path).write_text(json.dumps(document, ensure_ascii=False, indent=2), encoding='utf-8')
        except Exception as error: QMessageBox.critical(self, 'Export failed', str(error))
