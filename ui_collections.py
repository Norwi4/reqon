"""Native collection tree, hover actions and local collection/example editors."""
from ui_theming import theme_qcolor
import json
from PySide6.QtCore import Qt, QSize, QRect, Signal, QPoint
from PySide6.QtGui import QColor, QPainter, QPen, QKeySequence
from PySide6.QtWidgets import (QAbstractItemView, QTreeWidget, QStyledItemDelegate, QStyleOptionViewItem, QMenu,
    QStyle, QWidget, QVBoxLayout, QHBoxLayout, QLabel, QLineEdit, QPlainTextEdit, QTabWidget,
    QPushButton, QSpinBox)
from ui_components import icon, tool, KeyValueTable, CodeEditor
from ui_brand import USE_REQON_UI

ROLE = Qt.ItemDataRole.UserRole
STAR = int(ROLE) + 1
PROTOCOL = int(ROLE) + 2
METHOD = int(ROLE) + 3
COUNT = int(ROLE) + 4


class CollectionMenu(QMenu):
    """Use the native menu's hit testing and keyboard handling, with a red delete label."""
    def paintEvent(self, event):
        super().paintEvent(event)
        action = getattr(self, 'delete_action', None)
        if not action: return
        rect = self.actionGeometry(action); painter = QPainter(self)
        painter.fillRect(rect, theme_qcolor('#363636' if self.activeAction() is action else '#242424'))
        painter.setFont(self.font()); painter.setPen(theme_qcolor('#ef9a9a'))
        painter.drawText(rect.adjusted(8, 0, -8, 0), Qt.AlignmentFlag.AlignLeft | Qt.AlignmentFlag.AlignVCenter, 'Delete')
        painter.setPen(theme_qcolor('#858585'))
        painter.drawText(rect.adjusted(8, 0, -24, 0), Qt.AlignmentFlag.AlignRight | Qt.AlignmentFlag.AlignVCenter, 'Del')


class TreeDelegate(QStyledItemDelegate):
    def paint(self, painter, option, index):
        tree = self.parent()
        option = QStyleOptionViewItem(option); self.initStyleOption(option, index)
        if USE_REQON_UI:
            from ui_windows_tree import paint_row
            painter.save();paint_row(painter,option,index,tree);painter.restore();return
        item = tree.itemFromIndex(index)
        hovered = item is tree.hover_item
        value = index.data(ROLE)
        reserve = (52 if value[0] == 'collection' else 28) if hovered else (26 if index.data(STAR) else 4)
        full_rect = QRect(option.rect)
        # Paint the row background across its width, then the elided label with room for actions.
        if option.state & QStyle.StateFlag.State_Selected: painter.fillRect(full_rect, theme_qcolor('#343434'))
        elif hovered: painter.fillRect(full_rect, theme_qcolor('#303030'))
        inset = 11 if value[0] == 'collection' else 26 if value[0] == 'folder' else 18
        label_rect = full_rect.adjusted(inset, 0, -reserve, 0)
        painter.setFont(option.font); painter.setPen(theme_qcolor('#e6e6e6'))
        if not option.icon.isNull():
            option.icon.paint(painter, QRect(label_rect.left(), full_rect.y()+6, 16, 16))
            label_rect.adjust(22, 0, 0, 0)
        text = option.fontMetrics.elidedText(option.text, Qt.TextElideMode.ElideRight, max(0, label_rect.width()))
        painter.drawText(label_rect, Qt.AlignmentFlag.AlignLeft | Qt.AlignmentFlag.AlignVCenter, text)
        if index.data(STAR) and not hovered:
            icon('star', '#ffca80', 14).paint(painter, QRect(full_rect.right()-22, full_rect.y()+7, 14, 14))
        if item is tree.currentItem() and tree.hasFocus():
            painter.setPen(QPen(theme_qcolor('#6e9ecf'), 1)); painter.drawRect(full_rect.adjusted(0, 0, -1, -1))

    def sizeHint(self, option, index): return QSize(80, 30 if USE_REQON_UI else 28)


class CollectionTree(QTreeWidget):
    menu_requested = Signal(object, object)
    star_requested = Signal(object)
    operation_requested = Signal(str, object)
    move_requested = Signal(object, object, str)

    def __init__(self):
        super().__init__()
        self.setObjectName('collectionTree'); self.setHeaderHidden(True); self.setIndentation(16)
        self.setUniformRowHeights(True); self.setMouseTracking(True)
        self.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
        self.header().setStretchLastSection(True)
        self.setEditTriggers(QAbstractItemView.EditTrigger.EditKeyPressed)
        self.setDragDropMode(QAbstractItemView.DragDropMode.InternalMove)
        self.setDefaultDropAction(Qt.DropAction.MoveAction); self.setDropIndicatorShown(True)
        self.setAutoExpandDelay(650)
        self.setItemDelegate(TreeDelegate(self))
        self.hover_item = None
        self._branch_click=False
        self.more = tool('more', 'More actions', self.show_hover_menu, parent=self.viewport())
        self.star = tool('star', 'Add to favorites', self.toggle_hover_star, parent=self.viewport())
        for button in (self.more, self.star):
            button.setFixedSize(24, 24); button.hide()
        self.verticalScrollBar().valueChanged.connect(self.clear_hover)

    def clear_hover(self, *_):
        self.hover_item = None; self.more.hide(); self.star.hide(); self.viewport().update()

    def clear(self):
        self.clear_hover(); super().clear()

    def update_hover(self, item):
        self.hover_item = item; self.more.hide(); self.star.hide()
        if item:
            rect = self.visualItemRect(item); right = self.viewport().width()-3
            self.more.move(right-24, rect.y()+2); self.more.show(); self.more.raise_()
            if item.data(0, ROLE)[0] == 'collection':
                starred = bool(item.data(0, STAR))
                self.star.setIcon(icon('star', '#ffca80' if starred else '#a6a6a6'))
                self.star.setToolTip('Remove from favorites' if starred else 'Add to favorites')
                self.star.move(right-48, rect.y()+2); self.star.show(); self.star.raise_()
        self.viewport().update()

    def mouseMoveEvent(self, event):
        self.update_hover(self.itemAt(event.position().toPoint())); super().mouseMoveEvent(event)

    def mousePressEvent(self,event):
        item=self.itemAt(event.position().toPoint())
        self._branch_click=bool(USE_REQON_UI and item and event.position().x()<self.visualItemRect(item).left())
        super().mousePressEvent(event)

    def mouseReleaseEvent(self,event):
        super().mouseReleaseEvent(event);self._branch_click=False

    def paintEvent(self,event):
        super().paintEvent(event)
        if USE_REQON_UI:
            from ui_brand import COLORS
            # Paint after branches: Qt clips row delegates to the indented
            # content area, while the selected marker belongs at the rail edge.
            painter=QPainter(self.viewport())
            for item in self.selectedItems():
                rect=self.visualItemRect(item)
                if rect.isValid():painter.fillRect(QRect(0,rect.top(),2,rect.height()),theme_qcolor(COLORS['accent']))

    def leaveEvent(self, event):
        if not self.more.underMouse() and not self.star.underMouse(): self.clear_hover()
        super().leaveEvent(event)

    def resizeEvent(self, event):
        super().resizeEvent(event)
        if hasattr(self, 'hover_item'): self.update_hover(self.hover_item)

    def show_hover_menu(self):
        if self.hover_item:
            self.menu_requested.emit(self.hover_item, self.more.mapToGlobal(QPoint(0, self.more.height())))

    def toggle_hover_star(self):
        if self.hover_item: self.star_requested.emit(self.hover_item)

    def keyPressEvent(self, event):
        if self.state() != QAbstractItemView.State.EditingState and self.currentItem():
            operation = None
            if event.key() == Qt.Key.Key_Delete: operation = 'delete'
            elif event.key() == Qt.Key.Key_F2 or QKeySequence(event.keyCombination()) == QKeySequence('Ctrl+E'): operation = 'rename'
            elif QKeySequence(event.keyCombination()) == QKeySequence('Ctrl+D'): operation = 'duplicate'
            elif event.key() in (Qt.Key.Key_Return, Qt.Key.Key_Enter): operation = 'open'
            if operation:
                self.operation_requested.emit(operation, self.currentItem()); event.accept(); return
        super().keyPressEvent(event)

    def dropEvent(self, event):
        if event.source() is not self or not self.currentItem(): event.ignore(); return
        source = self.currentItem(); target = self.itemAt(event.position().toPoint())
        indicator = self.dropIndicatorPosition()
        position = {QAbstractItemView.DropIndicatorPosition.AboveItem: 'before',
                    QAbstractItemView.DropIndicatorPosition.BelowItem: 'after',
                    QAbstractItemView.DropIndicatorPosition.OnItem: 'inside'}.get(indicator, 'root')
        if source is target: event.ignore(); return
        # Database is authoritative; Qt must not perform a second in-memory move.
        self.move_requested.emit(source, target, position); event.ignore()

    def drawBranches(self, painter, rect, index):
        item = self.itemFromIndex(index)
        painter.save()
        if item.isSelected(): painter.fillRect(rect, theme_qcolor('#343434'))
        elif item is self.hover_item: painter.fillRect(rect, theme_qcolor('#303030'))
        painter.setPen(QPen(theme_qcolor('#383838'), 1))
        ancestor = item.parent(); x = rect.right()-self.indentation()-2
        while ancestor:
            painter.drawLine(x, rect.top(), x, rect.bottom()); x -= self.indentation(); ancestor = ancestor.parent()
        if item.childCount():
            x = rect.right()-3; y = rect.center().y(); painter.setPen(QPen(theme_qcolor('#c0c0c0'), 1.2))
            if item.isExpanded():
                painter.drawLine(x-3, y-1, x, y+2); painter.drawLine(x, y+2, x+3, y-1)
            else:
                painter.drawLine(x-1, y-3, x+2, y); painter.drawLine(x+2, y, x-1, y+3)
        painter.restore()


class CollectionPage(QWidget):
    def __init__(self, window, cid):
        super().__init__(); self.window = window; self.collection_id = cid
        self.entity_key = ('collection', cid); self.request_id = None; self.busy = False
        self.setObjectName('page')
        if USE_REQON_UI:
            from ui_windows_collection import build_collection_page
            build_collection_page(self,window,cid);return
        layout = QVBoxLayout(self); layout.setContentsMargins(16, 12, 16, 12); layout.setSpacing(12)
        head = QHBoxLayout(); self.title = QLabel(); self.title.setObjectName('docsTitle'); head.addWidget(self.title, 1)
        head.addWidget(tool('copy', 'Copy local link', lambda: window.copy_local_link('collection', cid)))
        export = QPushButton('Share / Export'); export.clicked.connect(lambda: window.export_collection(cid)); head.addWidget(export)
        layout.addLayout(head)
        tabs = QTabWidget(); tabs.setDocumentMode(True); tabs.setObjectName('editorTabs'); layout.addWidget(tabs, 1)
        overview = QWidget(); body = QVBoxLayout(overview); body.setContentsMargins(8, 24, 8, 8); body.setSpacing(16)
        self.count = QLabel(); self.count.setObjectName('docsTitle'); body.addWidget(self.count)
        body.addWidget(QLabel('Summary'))
        self.description = QPlainTextEdit(); self.description.setPlaceholderText('Describe this collection'); self.description.setMaximumHeight(150)
        row = window.store.entity('collection', cid); self.description.setPlainText(row['description']); body.addWidget(self.description)
        self.description.textChanged.connect(self.save_description)
        actions = QHBoxLayout()
        for label, callback in [('Add HTTP request', lambda: window.add_tree_request(cid, None, 'http')),
                                ('Add gRPC request', lambda: window.add_tree_request(cid, None, 'grpc')),
                                ('Add folder', lambda: window.add_folder(cid))]:
            button = QPushButton(label); button.clicked.connect(callback); actions.addWidget(button)
        actions.addStretch(); body.addLayout(actions)
        self.location = QLabel('Stored on this computer · Changes are saved automatically'); self.location.setObjectName('muted')
        body.addWidget(self.location); body.addStretch(); tabs.addTab(overview, 'Overview')
        self.variables = KeyValueTable(); self.variables.set_rows([[k, str(v), ''] for k, v in json.loads(row['variables']).items()])
        self.variables.changed.connect(self.save_variables); tabs.addTab(self.variables, 'Variables')
        self.refresh(); self._saved = self.collect()

    def collect(self): return {}
    def refresh(self):
        store = self.window.store; row = store.entity('collection', self.collection_id)
        if self.title.text()!=row['name']:self.title.setText(row['name'])
        requests = store.all('SELECT data FROM requests WHERE collection_id=?', (self.collection_id,))
        grpc = sum(json.loads(r[0]).get('kind') == 'grpc' for r in requests)
        folders = store.all('SELECT COUNT(*) FROM folders WHERE collection_id=?', (self.collection_id,))[0][0]
        if USE_REQON_UI:
            self.count.setText(f'{len(requests)} requests');self.title.setToolTip(row['name'])
            self.protocol_counts.setText(f'{len(requests)-grpc} HTTP · {grpc} gRPC\n{folders} folders')
        else:self.count.setText(f'{len(requests)} requests   ·   {len(requests)-grpc} HTTP   ·   {grpc} gRPC   ·   {folders} folders')
        index = self.window.workbench.indexOf(self)
        if index >= 0: self.window.workbench.setTabText(index, row['name'])
    def save_description(self):
        self.window.store.db.execute('UPDATE collections SET description=? WHERE id=?', (self.description.toPlainText(), self.collection_id))
        self.window.store.db.commit()
    def save_variables(self):
        variables = {k: v for k, v, *_ in self.variables.active_rows()}
        self.window.store.db.execute('UPDATE collections SET variables=? WHERE id=?', (json.dumps(variables), self.collection_id))
        self.window.store.db.commit(); self.window.invalidate_collection_variables(self.collection_id)


class ExamplePage(QWidget):
    def __init__(self, window, id, draft=None):
        super().__init__(); self.window = window; self.entity_key = ('example', id)
        self.request_id = None; self.busy = False; self.setObjectName('page')
        row = window.store.entity('example', id) if id else {'name':draft.get('_editor_state',{}).get('name','Example'),'data':json.dumps(draft)}
        snapshot = json.loads(row['data'])
        self.original = snapshot; self.collection_id = None
        layout = QVBoxLayout(self); layout.setContentsMargins(16, 12, 16, 12)
        head = QHBoxLayout(); head.addWidget(QLabel('Example'))
        self.name = QLineEdit(row['name']); self.name.setObjectName('requestName'); head.addWidget(self.name, 1)
        self.save_state=QLabel('Saved locally'); head.addWidget(self.save_state)
        head.addWidget(tool('save', 'Save example (Ctrl+S)', lambda: window.save_example(self), 'Save'))
        layout.addLayout(head)
        hint = QLabel('Saved request and response · Editing this example does not send a request'); hint.setObjectName('muted'); layout.addWidget(hint)
        response = snapshot.get('response', {})
        status = QHBoxLayout(); status.addWidget(QLabel('Status'))
        self.status = QSpinBox(); self.status.setRange(-1, 999); self.status.setValue(int(response.get('status', 200)))
        status.addWidget(self.status); self.reason = QLineEdit(response.get('reason', 'OK')); status.addWidget(self.reason, 1); layout.addLayout(status)
        tabs = QTabWidget(); tabs.setDocumentMode(True); layout.addWidget(tabs, 1)
        self.request = CodeEditor(json.dumps(snapshot.get('request', {}), ensure_ascii=False, indent=2)); tabs.addTab(self.request, 'Request snapshot')
        self.response = CodeEditor(response.get('body', '')); tabs.addTab(self.response, 'Response')
        self.headers = KeyValueTable(); headers = response.get('headers', [])
        self.headers.set_rows([[k, v, ''] for k, v in (headers.items() if isinstance(headers, dict) else headers)]); tabs.addTab(self.headers, 'Headers / Metadata')
        self.trailers = KeyValueTable(); self.trailers.set_rows([[k, v, ''] for k, v in response.get('trailers', [])]); tabs.addTab(self.trailers, 'Trailers')
        state=snapshot.get('_editor_state')
        if state:
            self.name.setText(row['name']); self.request.setPlainText(state['request']); self.response.setPlainText(state['body'])
            self.status.setValue(state['status']); self.reason.setText(state['reason'])
            self.headers.set_rows(state['headers']); self.trailers.set_rows(state['trailers'])
        tabs.setCurrentIndex(1); self._saved = self.collect()
        self.name.textChanged.connect(lambda name: window.workbench.setTabText(window.workbench.indexOf(self), name))

    def collect(self):
        return {'name': self.name.text(), 'request': self.request.toPlainText(), 'body': self.response.toPlainText(),
                'status': self.status.value(), 'reason': self.reason.text(), 'headers': self.headers.all_rows(), 'trailers': self.trailers.all_rows()}

    def snapshot(self):
        request = json.loads(self.request.toPlainText())
        if not isinstance(request, dict) or not request.get('name'): raise ValueError('The request snapshot must be a JSON object with a name.')
        body = self.response.toPlainText()
        response = dict(self.original.get('response', {}), body=body, status=self.status.value(), reason=self.reason.text(),
                        size=len(body.encode()), duration=self.original.get('response', {}).get('duration', 0),
                        headers=[r[:2] for r in self.headers.active_rows()], trailers=[r[:2] for r in self.trailers.active_rows()])
        response.pop('error', None)
        return {'request': request, 'response': response}

    def autosave_snapshot(self):
        try:result=self.snapshot()
        except ValueError:result=dict(self.original)
        result['_editor_state']=self.collect()
        return result
