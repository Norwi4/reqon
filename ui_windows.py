"""Optional Windows widgets; no network or persistence format changes."""
from ui_theming import theme_qcolor
import json
from PySide6.QtCore import Qt,QEvent,QTimer,QPoint,QSize,QObject
from PySide6.QtGui import QPainter,QColor,QKeySequence,QShortcut
from PySide6.QtWidgets import (QWidget,QVBoxLayout,QHBoxLayout,QLabel,QFrame,QDialog,QLineEdit,
    QListWidget,QListWidgetItem,QTabBar,QGraphicsOpacityEffect,QSplitter,QSplitterHandle,QSizePolicy,QPushButton)
from ui_brand import COLORS as C,brand_icon
from ui_components import icon,tool
from ui_state_widgets import ResponseEmpty,StateWidget


class TabCorner(QWidget):
    def sizeHint(self):
        # QTabWidget positions corners using this hint before fixed-size clamping.
        hint=super().sizeHint();hint.setHeight(self.height());return hint


class RequestTabs(QTabBar):
    def __init__(self,parent=None):
        super().__init__(parent);self.setMouseTracking(True);self.hovered=-1
        self.timer=QTimer(self);self.timer.setSingleShot(True);self.timer.timeout.connect(self.sync_buttons)
        self.currentChanged.connect(self.schedule);self.tabMoved.connect(self.schedule)
        self.add_button=None;self.last_request_kind='grpc'
    def schedule(self,*_):self.timer.start(0)
    def install_add_button(self,http,grpc):
        self.new_requests={'http':http,'grpc':grpc}
        self.add_button=tool('plus','New request',self.add_request,parent=self.parentWidget())
        self.add_button.setObjectName('newRequestTab');self.add_button.setFixedSize(28,28)
        self.add_button.setAccessibleName('New request tab')
        self.parentWidget().installEventFilter(self)
        self.parentWidget().currentChanged.connect(self.remember_request_kind);self.schedule()
    def remember_request_kind(self,*_):
        kind=getattr(self.parentWidget().currentWidget(),'kind',None)
        if kind in ('http','grpc'):self.last_request_kind=kind
    def add_request(self):
        self.remember_request_kind();self.new_requests[self.last_request_kind]()
    def eventFilter(self,obj,event):
        if obj is self.parentWidget() and event.type()==QEvent.Type.Resize:self.position_add_button()
        if obj is self.parentWidget() and event.type() in (QEvent.Type.Resize,QEvent.Type.LayoutRequest):self.schedule()
        return super().eventFilter(obj,event)
    def resizeEvent(self,event):
        super().resizeEvent(event)
        self.position_add_button()
        if hasattr(self,'timer'):self.schedule()
    def tabLayoutChange(self):
        super().tabLayoutChange()
        if hasattr(self,'timer'):self.schedule()
    def position_add_button(self):
        if getattr(self,'add_button',None) is None or getattr(self,'_placing_add_button',False):return
        self._placing_add_button=True
        workbench=self.parentWidget();corner=workbench.cornerWidget(Qt.Corner.TopRightCorner)
        limit=max(50,workbench.width()-(corner.width() if corner else 0)-self.x()-44)
        if self.maximumWidth()!=limit:self.setMaximumWidth(limit)
        end=min(self.width(),max(0,self.tabRect(self.count()-1).right()+1)) if self.count() else 0
        if self.sizeHint().width()>self.width():end=self.width()
        self.add_button.move(self.x()+end+8,self.y()+max(0,(self.height()-28)//2))
        self.add_button.setToolTip('New '+('gRPC' if self.last_request_kind=='grpc' else 'HTTP')+' request')
        self.add_button.show();self.add_button.raise_()
        self._placing_add_button=False
    def setTabButton(self,index,side,button):
        if button and side==self.ButtonPosition.RightSide:
            button.setFixedSize(26,26);button.setObjectName('tabClose')
        super().setTabButton(index,side,button);self.schedule()
    def tabInserted(self,index):super().tabInserted(index);self.schedule()
    def tabRemoved(self,index):super().tabRemoved(index);self.schedule()
    def mouseMoveEvent(self,event):
        self.hovered=self.tabAt(event.position().toPoint());self.sync_buttons();super().mouseMoveEvent(event)
    def leaveEvent(self,event):self.hovered=-1;self.sync_buttons();super().leaveEvent(event)
    def sync_buttons(self):
        self.position_add_button()
        for i in range(self.count()):
            button=self.tabButton(i,self.ButtonPosition.RightSide)
            if not button:continue
            effect=button.graphicsEffect()
            if not isinstance(effect,QGraphicsOpacityEffect):effect=QGraphicsOpacityEffect(button);button.setGraphicsEffect(effect)
            visible=i in (self.currentIndex(),self.hovered)
            effect.setOpacity(1.0 if visible else 0.0);button.setEnabled(visible)
            button.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents,not visible)
            page=self.parent().widget(i)
            self.setTabToolTip(i,self.tabText(i)+(' · Unsaved changes' if getattr(page,'_presentation_dirty',False) else ''))
        self.update()
    def paintEvent(self,event):
        super().paintEvent(event);painter=QPainter(self);painter.setRenderHint(QPainter.RenderHint.Antialiasing)
        painter.setPen(theme_qcolor(C['border']))
        for i in range(self.count()):
            rect=self.tabRect(i);painter.drawLine(rect.right(),rect.top()+9,rect.right(),rect.bottom()-9)
        painter.setPen(Qt.PenStyle.NoPen);painter.setBrush(theme_qcolor(C['accent']))
        for i in range(self.count()):
            if getattr(self.parent().widget(i),'_presentation_dirty',False):
                rect=self.tabRect(i);painter.drawEllipse(QPoint(rect.left()+23,rect.center().y()+7),3,3)


class SplitHandle(QSplitterHandle):
    def __init__(self,orientation,parent):
        super().__init__(orientation,parent);self.setCursor(Qt.CursorShape.SplitVCursor)
        self.setToolTip('Drag to resize · Double-click to reset to 60 / 40')
    def mouseDoubleClickEvent(self,event):
        if event.button()==Qt.MouseButton.LeftButton:self.splitter().reset_ratio();event.accept()
        else:super().mouseDoubleClickEvent(event)


class RequestSplitter(QSplitter):
    def __init__(self):
        # Qt keeps a wider overlapping grab area for a one-pixel divider.
        super().__init__(Qt.Orientation.Vertical);self.setObjectName('requestSplitter');self.setHandleWidth(1)
    def createHandle(self):return SplitHandle(self.orientation(),self)
    def reset_ratio(self):
        extent=max(1,(self.width() if self.orientation()==Qt.Orientation.Horizontal else self.height())-self.handleWidth());self.setSizes([round(extent*.6),round(extent*.4)])


class SearchPalette(QDialog):
    """Search the existing tree, drafts and history; rebuild from current data when opened."""
    def __init__(self,window):
        super().__init__(window,Qt.WindowType.Popup|Qt.WindowType.FramelessWindowHint)
        self.window=window;self.entries=[];self.setWindowTitle('Search local workspace')
        layout=QVBoxLayout(self);layout.setContentsMargins(0,0,0,0)
        frame=QFrame();frame.setObjectName('quickSearch');layout.addWidget(frame)
        body=QVBoxLayout(frame);body.setContentsMargins(12,12,12,8);body.setSpacing(8)
        self.input=QLineEdit();self.input.setPlaceholderText('Search requests, collections, drafts and history')
        self.input.setAccessibleName('Search local workspace');self.input.addAction(icon('search'),QLineEdit.ActionPosition.LeadingPosition)
        body.addWidget(self.input);self.results=QListWidget();self.results.setObjectName('quickResults')
        self.results.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff);body.addWidget(self.results,1)
        self.empty=StateWidget('search-no-results');self.empty.hide();body.addWidget(self.empty,1)
        self.hint=QLabel('↑ ↓ Navigate    Enter Open    Esc Close');self.hint.setObjectName('searchHint');body.addWidget(self.hint)
        self.input.installEventFilter(self);self.results.itemActivated.connect(self.open_result);self.results.itemClicked.connect(self.open_result)
        self.debounce=QTimer(self);self.debounce.setSingleShot(True);self.debounce.setInterval(80)
        self.debounce.timeout.connect(self.filter);self.input.textChanged.connect(lambda:self.debounce.start())
        self.input.returnPressed.connect(self.accept_current)
    def accept_current(self):
        if self.debounce.isActive():self.debounce.stop();self.filter()
        self.open_result(self.results.currentItem())
    def open(self):
        w=self.window;self.entries=[]
        requests={id:json.loads(data) for id,data in w.store.all('SELECT id,data FROM requests')}
        for item in w.tree_items():
            kind,id,*_=item.data(0,Qt.ItemDataRole.UserRole)
            path=[];parent=item.parent()
            while parent:path.insert(0,parent.text(0));parent=parent.parent()
            data=requests.get(id,{}) if kind=='request' else {}
            detail=' / '.join(path) or kind.title()
            if kind=='request':detail+=' · '+('gRPC '+data.get('server','') if data.get('kind')=='grpc' else data.get('method','GET')+' '+data.get('url',''))
            self.entries.append((kind,id,item.text(0),detail))
        for uid,name,kind in w.store.all('SELECT uid,name,kind FROM local_drafts ORDER BY updated_at DESC'):
            self.entries.append(('draft',uid,name,'Local draft · '+kind))
        for id,method,url in w.store.all('SELECT id,method,url FROM history ORDER BY id DESC LIMIT 200'):
            self.entries.append(('history',id,method+' '+url,'History'))
        for data in w.environments.data.all():self.entries.append(('environment',data['id'],data['name'],'Environment'))
        self.input.clear();self.filter();self.resize(min(640,w.width()-48),min(440,w.height()-100))
        self.move(w.mapToGlobal(QPoint((w.width()-self.width())//2,64)));self.show();self.input.setFocus()
    def filter(self):
        query=self.input.text().casefold().strip();self.results.clear()
        matches=[entry for entry in self.entries if query in (entry[2]+' '+entry[3]).casefold()]
        matches.sort(key=lambda entry:(not entry[2].casefold().startswith(query),entry[0]=='history'))
        for entry in matches[:100]:
            row=QListWidgetItem(entry[2]+'\n'+entry[3]);row.setData(Qt.ItemDataRole.UserRole,entry)
            row.setToolTip(entry[2]+'\n'+entry[3]);row.setIcon(icon('history' if entry[0]=='history' else 'collection' if entry[0]=='collection' else 'folder' if entry[0]=='folder' else 'docs'))
            self.results.addItem(row)
        self.results.setCurrentRow(0)
        self.results.setVisible(bool(matches));self.empty.setVisible(not matches)
        self.hint.setText(('No results · ' if not matches else f'{len(matches)} results · ')+('Showing first 100 · ' if len(matches)>100 else '')+'↑ ↓ Navigate    Enter Open    Esc Close')
    def open_result(self,item):
        if not item:return
        kind,id,*_=item.data(Qt.ItemDataRole.UserRole);self.hide();w=self.window
        if kind=='draft':w.open_draft(id)
        elif kind=='environment':w.environments.open(id)
        elif kind=='history':
            for i in range(w.history.topLevelItemCount()):
                candidate=w.history.topLevelItem(i)
                if candidate.data(0,Qt.ItemDataRole.UserRole)==id:w.open_history(candidate);break
        else:
            w.search.clear();w.select_nav('Collections');w.select_tree_item(kind,id)
            node=w.tree_item(kind,id)
            if node:
                if kind=='folder':node.setExpanded(True)
                else:w.open_item(node)
    def eventFilter(self,obj,event):
        if obj is self.input and event.type()==QEvent.Type.KeyPress:
            if event.key() in (Qt.Key.Key_Down,Qt.Key.Key_Up):
                if self.debounce.isActive():self.debounce.stop();self.filter()
                if self.results.count():self.results.setCurrentRow(max(0,min(self.results.count()-1,self.results.currentRow()+(1 if event.key()==Qt.Key.Key_Down else -1))))
                return True
            if event.key()==Qt.Key.Key_Escape:self.hide();return True
        if obj is self.window.global_search and (event.type()==QEvent.Type.MouseButtonPress or event.type()==QEvent.Type.KeyPress and event.key() in (Qt.Key.Key_Return,Qt.Key.Key_Space)):
            self.open();return True
        return super().eventFilter(obj,event)


def wire_search(window):
    window.search_palette=SearchPalette(window);window.global_search.setReadOnly(True)
    window.global_search.setPlaceholderText('Search  Ctrl+K');window.global_search.setAccessibleName('Search workspace (Ctrl+K)')
    window.global_search.setToolTip('Search local workspace · Ctrl+K')
    window.global_search.installEventFilter(window.search_palette)
    window.search_shortcut=QShortcut(QKeySequence('Ctrl+K'),window);window.search_shortcut.activated.connect(window.search_palette.open)


class WorkspaceStates(QObject):
    def __init__(self,window):
        super().__init__(window);self.window=window
        self.collections=StateWidget('collections-empty',window.tree.viewport())
        self.history=StateWidget('activity-no-requests',window.history.viewport())
        self.requests=StateWidget('request-create-new',window.workbench)
        create=QPushButton('Create collection');create.setFixedSize(160,32);create.clicked.connect(window.new_collection)
        self.collections.content_layout.insertWidget(4,create,0,Qt.AlignmentFlag.AlignHCenter);self.create_collection=create
        choices=QWidget();buttons=QHBoxLayout(choices);buttons.setContentsMargins(0,0,0,0);buttons.addStretch()
        for title,callback in [('HTTP request',window.new_request),('gRPC request',window.new_grpc_request)]:
            button=QPushButton(title);button.clicked.connect(lambda checked=False,callback=callback:callback());buttons.addWidget(button)
        buttons.addStretch();self.requests.content_layout.insertWidget(4,choices)
        self.timer=QTimer(self);self.timer.setSingleShot(True);self.timer.timeout.connect(self.refresh)
        for view in (window.tree,window.history):
            view.viewport().installEventFilter(self)
            for signal in (view.model().rowsInserted,view.model().rowsRemoved,view.model().modelReset):signal.connect(self.schedule)
        window.workbench.installEventFilter(self);window.workbench.currentChanged.connect(self.schedule);self.schedule()
    def schedule(self,*_):self.timer.start(0)
    def eventFilter(self,obj,event):
        if event.type() in (QEvent.Type.Resize,QEvent.Type.Show):self.schedule()
        return False
    def refresh(self):
        w=self.window;empty=w.tree.topLevelItemCount()==0
        no_matches=not empty and all(w.tree.topLevelItem(i).isHidden() for i in range(w.tree.topLevelItemCount()))
        state='collections-empty' if empty else 'search-no-results'
        if self.collections.state!=state:self.collections.set_state(state)
        self.collections.setGeometry(w.tree.viewport().rect());self.collections.setVisible(empty or no_matches)
        self.create_collection.setVisible(empty)
        self.history.setGeometry(w.history.viewport().rect());self.history.setVisible(w.history.topLevelItemCount()==0)
        self.requests.setGeometry(w.workbench.rect().adjusted(12,44,-12,-12));self.requests.setVisible(w.workbench.count()==0)


def response_metrics(result):
    metrics=[]
    if result.get('duration') is not None:
        duration=result['duration'];metrics.append(f'{duration:g} ms' if duration<1000 else f'{duration/1000:.2f} s')
    if result.get('size') is not None and 'error' not in result:
        size=result['size'];metrics.append(f'{size:,} B' if size<1024 else f'{size/1024:.1f} KB' if size<1024**2 else f'{size/1024**2:.1f} MB')
    return '  ·  '.join(metrics)
