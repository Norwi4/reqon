from ui_theming import set_style
from ui_brand import PROTOCOL_COLORS
from PySide6.QtCore import Qt, QSize, QEvent, QObject, QUrl, QRectF
from PySide6.QtGui import QDesktopServices, QFont, QPainterPath, QRegion
from PySide6.QtWidgets import (QWidget,QFrame,QVBoxLayout,QHBoxLayout,QLabel,QLineEdit,
    QPushButton,QToolButton,QMenu,QSplitter,QTabWidget,QTreeWidget,QComboBox,QButtonGroup,
    QSizePolicy,QStyle,QDialog,QPlainTextEdit)
from ui_components import icon,tool,Select as QComboBox
from ui_collections import CollectionTree
from ui_snippets import CodeSnippetPanel
from ui_window import WindowResize
from ui_brand import USE_REQON_UI,DISPLAY_NAME,TAB_HEIGHT,brand_icon,stylesheet


class TitleBar(QFrame):
    def __init__(self,window): super().__init__(window); self.window=window; self.setObjectName('topbar'); self.setFixedHeight(48)
    def mousePressEvent(self,event):
        if event.button()==Qt.MouseButton.LeftButton: self.window.windowHandle().startSystemMove()
    def mouseDoubleClickEvent(self,event):
        if event.button()==Qt.MouseButton.LeftButton: self.toggle_maximize()
    def toggle_maximize(self):
        self.window.showNormal() if self.window.isMaximized() else self.window.showMaximized()


class WindowCorners(QObject):
    """Small native window outline; square edges when maximized or full screen."""
    def __init__(self,window):
        super().__init__(window); self.window=window; self.radius=8
        window.installEventFilter(self)
    def update_shape(self):
        window=self.window
        if window.isMaximized() or window.isFullScreen():
            window.clearMask(); return
        path=QPainterPath(); path.addRoundedRect(QRectF(window.rect()),self.radius,self.radius)
        window.setMask(QRegion(path.toFillPolygon().toPolygon()))
    def eventFilter(self,watched,event):
        if event.type() in (QEvent.Type.Show,QEvent.Type.Resize,QEvent.Type.WindowStateChange):self.update_shape()
        return False


def build_shell(window):
    window.setWindowFlags(Qt.WindowType.Window | Qt.WindowType.FramelessWindowHint)
    window._corners=WindowCorners(window)
    window.setWindowIcon(brand_icon(True) if USE_REQON_UI else icon('grpc','#ff6c37',32)); window.setMinimumSize(960,640)
    window._resize_controller=WindowResize(window)
    center=QWidget(); center.setObjectName('shell'); window.setCentralWidget(center)
    outer=QVBoxLayout(center); outer.setContentsMargins(1,1,1,1); outer.setSpacing(0)
    titlebar=TitleBar(window); window.titlebar=titlebar
    top=QHBoxLayout(titlebar); top.setContentsMargins(8,0,0,0); top.setSpacing(8)
    if USE_REQON_UI:
        window.brand_home=QToolButton();window.brand_home.setObjectName('brandHome')
        window.brand_home.setIcon(brand_icon());window.brand_home.setIconSize(QSize(24,24));window.brand_home.setFixedSize(36,36)
        window.brand_home.setToolTip('Reqon — Home');window.brand_home.setAccessibleName('Reqon — Home')
        window.brand_home.setFocusPolicy(Qt.FocusPolicy.StrongFocus)
        window.brand_home.clicked.connect(lambda:window.select_nav('Collections'));top.addWidget(window.brand_home)
    window.menu_button=tool('menu','Application menu'); window.menu_button.setFixedSize(28,30)
    window.menu_button.setPopupMode(QToolButton.ToolButtonPopupMode.InstantPopup); top.addWidget(window.menu_button)
    back=tool('back','Previous request',lambda:window.workbench.setCurrentIndex(max(0,window.workbench.currentIndex()-1)))
    forward=tool('forward','Next request',lambda:window.workbench.setCurrentIndex(min(window.workbench.count()-1,window.workbench.currentIndex()+1)))
    top.addWidget(back); top.addWidget(forward)
    home=QToolButton(); home.setText('Home'); home.setObjectName('navButton'); home.clicked.connect(lambda:window.select_nav('Collections')); top.addWidget(home)
    workspaces=QToolButton(); workspaces.setText('Workspaces'); workspaces.setObjectName('navButton')
    workspaces.setPopupMode(QToolButton.ToolButtonPopupMode.InstantPopup)
    workspace_menu=QMenu(workspaces); workspace_menu.addAction(icon('workspace'),'Local workspace',lambda:window.select_nav('Collections'))
    workspace_menu.addAction(icon('folder'),'Open local data folder',lambda:QDesktopServices.openUrl(QUrl.fromLocalFile(str(window.store.path.parent))))
    workspaces.setMenu(workspace_menu); top.addWidget(workspaces)
    top.addStretch(1)
    window.global_search=QLineEdit(); window.global_search.setObjectName('search'); window.global_search.setPlaceholderText('Search '+DISPLAY_NAME); window.global_search.setFixedSize(240,32)
    window.global_search.addAction(icon('search'),QLineEdit.ActionPosition.LeadingPosition)
    if USE_REQON_UI:
        window.global_search.setMinimumWidth(150);window.global_search.setMaximumWidth(280)
        window.global_search.setSizePolicy(QSizePolicy.Policy.Expanding,QSizePolicy.Policy.Fixed)
    if not USE_REQON_UI:window.global_search.textChanged.connect(lambda text:window.search.setText(text))
    top.addWidget(window.global_search)
    top.addStretch(1)
    local=QLabel('Local workspace'); local.setObjectName('localBadge'); top.addWidget(local)
    window.settings_button=tool('settings','Settings',window.show_settings);window.settings_button.setIconSize(QSize(21,21));window.settings_button.setFixedSize(36,36);top.addWidget(window.settings_button)
    window.info_button=QPushButton('Info');window.info_button.setFixedSize(72,30);window.info_button.setCursor(Qt.CursorShape.PointingHandCursor)
    set_style(window.info_button,'QPushButton {background:#ff6c37;color:white;border:0;border-radius:4px;font-weight:600;} QPushButton:hover {background:#ff824f;}');window.info_button.clicked.connect(window.show_info);top.addWidget(window.info_button);top.addSpacing(8)
    for image,tip,fn in [('minimize','Minimize',window.showMinimized),('maximize','Maximize / Restore',titlebar.toggle_maximize),('close','Close',window.close)]:
        button=tool(image,tip,fn); button.setFixedSize(46,46)
        if image=='close':button.setObjectName('windowClose')
        top.addWidget(button)
    outer.addWidget(titlebar)
    content=QHBoxLayout(); content.setContentsMargins(0,0,0,0); content.setSpacing(0)
    splitter=QSplitter(); splitter.setHandleWidth(1); splitter.setChildrenCollapsible(False); window.workspace_splitter=splitter
    left=QWidget(); left.setObjectName('sidebar'); left.setMinimumWidth(290)
    left_layout=QVBoxLayout(left); left_layout.setContentsMargins(0,0,0,0); left_layout.setSpacing(0)
    workspace_head=QFrame(); workspace_head.setObjectName('workspaceHead'); workspace_head.setFixedHeight(TAB_HEIGHT if USE_REQON_UI else 40)
    head=QHBoxLayout(workspace_head); head.setContentsMargins(14,0,8,0); head.setSpacing(4)
    head.addWidget(tool('workspace','Local workspace')); workspace_name=QLabel('My Workspace'); set_style(workspace_name,stylesheet('font-weight:600')); head.addWidget(workspace_name); head.addStretch()
    window.new_menu=QMenu(window)
    window.new_menu.addAction(icon('http',PROTOCOL_COLORS['http']),'HTTP request',lambda:window.new_request())
    window.new_menu.addAction(icon('grpc',PROTOCOL_COLORS['grpc']),'gRPC request',lambda:window.new_grpc_request())
    window.new_menu.addSeparator(); window.new_menu.addAction(icon('collection'),'Collection',window.new_collection)
    new=QPushButton('New'); new.setObjectName('smallButton'); new.clicked.connect(lambda:window.new_menu.exec(new.mapToGlobal(new.rect().bottomLeft()))); head.addWidget(new)
    imp=QPushButton('Import'); imp.setObjectName('smallButton'); imp.clicked.connect(lambda:window.environments.import_file() if USE_REQON_UI and window.side_tabs.currentWidget() is window.environments.sidebar else window.show_import_dialog()); head.addWidget(imp)
    left_layout.addWidget(workspace_head)
    left_content=QHBoxLayout(); left_content.setContentsMargins(0,0,0,0); left_content.setSpacing(0)
    rail=QFrame(); rail.setObjectName('rail'); rail.setFixedWidth(76)
    rail_layout=QVBoxLayout(rail); rail_layout.setContentsMargins(3,5,3,0); rail_layout.setSpacing(4)
    window.rail_group=QButtonGroup(window); window.rail_buttons={}
    for name,image in [('Collections','collection'),('Environments','environment'),('History','history')]:
        button=tool(image,name,lambda checked=False,name=name:window.select_nav(name,toggle=True),name)
        button.setObjectName('railButton'); button.setIconSize(QSize(18,18)); button.setToolButtonStyle(Qt.ToolButtonStyle.ToolButtonTextUnderIcon)
        button.setFixedSize(70,52); button.setCheckable(True); window.rail_group.addButton(button); window.rail_buttons[name]=button; rail_layout.addWidget(button)
    window.rail_buttons['Collections'].setChecked(True); rail_layout.addStretch(); left_content.addWidget(rail)
    sidebar=QWidget(); sidebar_layout=QVBoxLayout(sidebar); sidebar_layout.setContentsMargins(8,8,8,0); sidebar_layout.setSpacing(6)
    searchbar=QHBoxLayout(); searchbar.setContentsMargins(0,0,0,0); searchbar.setSpacing(4)
    searchbar.addWidget(tool('plus','Create collection',window.new_collection))
    window.search=QLineEdit(); window.search.setObjectName('search'); window.search.setPlaceholderText('Search collections'); window.search.setFixedHeight(26)
    window.search.addAction(icon('search',size=14),QLineEdit.ActionPosition.LeadingPosition); window.search.textChanged.connect(window.filter_tree)
    searchbar.addWidget(window.search,1)
    export=tool('download','Export collections',window.export_collections);export.setFixedSize(30,30);export.setAccessibleName('Export collections');searchbar.addWidget(export)
    if USE_REQON_UI:
        collapse=tool('collapse','Collapse all collections',lambda:(window.search.clear(),window.tree.collapseAll()))
        collapse.setFixedSize(30,30);collapse.setAccessibleName('Collapse all collections');searchbar.addWidget(collapse)
    if USE_REQON_UI:
        window.collection_searchbar=QWidget();window.collection_searchbar.setLayout(searchbar);sidebar_layout.addWidget(window.collection_searchbar)
        sidebar_layout.setContentsMargins(0,0,0,0);searchbar.setContentsMargins(8,8,8,0)
    else:sidebar_layout.addLayout(searchbar)
    window.side_tabs=QTabWidget(); window.side_tabs.tabBar().hide()
    window.tree=CollectionTree()
    window.tree.setUniformRowHeights(True); window.tree.itemClicked.connect(window.open_item)
    window.tree.setContextMenuPolicy(Qt.ContextMenuPolicy.CustomContextMenu); window.tree.customContextMenuRequested.connect(window.tree_menu)
    window.history=QTreeWidget(); window.history.setHeaderHidden(True); window.history.itemDoubleClicked.connect(window.open_history)
    window.side_tabs.addTab(window.tree,'Collections'); window.side_tabs.addTab(window.history,'History'); sidebar_layout.addWidget(window.side_tabs)
    left_content.addWidget(sidebar,1); left_layout.addLayout(left_content,1); splitter.addWidget(left)
    window.sidebar_container=left;window.sidebar_content=sidebar;window.sidebar_head=workspace_head;window._sidebar_collapsed=False;window._sidebar_width=378;window._active_nav='Collections'
    window.workbench=QTabWidget(); window.workbench.setObjectName('workbench'); window.workbench.setDocumentMode(True)
    if USE_REQON_UI:
        from ui_windows import RequestTabs,TabCorner
        window.workbench.setTabBar(RequestTabs(window.workbench))
    window.workbench.setMovable(True); window.workbench.setTabsClosable(True)
    window.workbench.tabCloseRequested.connect(window.close_request_tab)
    window.workbench.tabBar().setElideMode(Qt.TextElideMode.ElideRight); window.workbench.tabBar().setExpanding(False)
    window.workbench.tabBar().setDrawBase(False)
    corner=TabCorner() if USE_REQON_UI else QWidget()
    corner.setFixedHeight(TAB_HEIGHT if USE_REQON_UI else 40); corner_layout=QHBoxLayout(corner); corner_layout.setContentsMargins(4,0,10,0); corner_layout.setSpacing(6)
    if USE_REQON_UI:
        window.workbench.tabBar().install_add_button(window.new_request,window.new_grpc_request)
        window.tab_plus=window.workbench.tabBar().add_button
    else:
        plus=tool('plus','New request'); plus.setMenu(window.new_menu); plus.setPopupMode(QToolButton.ToolButtonPopupMode.InstantPopup); corner_layout.addWidget(plus)
    if USE_REQON_UI:
        from ui_environments import EnvironmentCombo
        window.env_combo=EnvironmentCombo()
    else:window.env_combo=QComboBox()
    window.env_combo.setFixedWidth(164); set_style(window.env_combo,stylesheet('border:0;background:#212121;')); window.env_combo.addItem('No environment',None)
    corner_layout.addWidget(window.env_combo); window.workbench.setCornerWidget(corner,Qt.Corner.TopRightCorner)
    splitter.addWidget(window.workbench); splitter.setSizes([378,942]); splitter.setStretchFactor(0,0); splitter.setStretchFactor(1,1); content.addWidget(splitter,1)
    utilities=QFrame(); utilities.setObjectName('utilityRail'); utilities.setFixedWidth(48); utility_layout=QVBoxLayout(utilities); utility_layout.setContentsMargins(8,8,8,0); utility_layout.setSpacing(16)
    utility_layout.addWidget(tool('environment','Manage environments',window.manage_environment))
    utility_layout.addWidget(tool('docs','Request documentation',window.show_docs))
    window.code_button=tool('code','Code snippet',window.toggle_code_snippet); window.code_button.setCheckable(True)
    utility_layout.addWidget(window.code_button); utility_layout.addStretch(); content.addWidget(utilities)
    window.snippet_panel=CodeSnippetPanel(window); window.snippet_panel.setFixedWidth(380)
    window.snippet_panel.close_requested.connect(window.hide_code_snippet); content.addWidget(window.snippet_panel)
    window.workbench.currentChanged.connect(window.snippet_panel.schedule)
    window.env_combo.currentIndexChanged.connect(window.snippet_panel.schedule)
    from ui_workspace_panels import ConsolePanel
    work_area=QWidget();work_area.setLayout(content);work_area.setMinimumHeight(200)
    window.console_splitter=QSplitter(Qt.Orientation.Vertical);window.console_splitter.setChildrenCollapsible(False);window.console_splitter.setHandleWidth(3)
    window.console_splitter.addWidget(work_area);window.console_panel=ConsolePanel(window);window.console_splitter.addWidget(window.console_panel);window.console_splitter.setStretchFactor(0,1);window.console_splitter.setStretchFactor(1,0)
    outer.addWidget(window.console_splitter,1)
    footer=QFrame(); footer.setObjectName('footer'); footer.setFixedHeight(25)
    bottom=QHBoxLayout(footer); bottom.setContentsMargins(10,0,12,0); bottom.setSpacing(12)
    bottom.addWidget(tool('split','Toggle collections sidebar',window.toggle_sidebar))
    local=tool('check','All data is stored on this computer',text='Local'); bottom.addWidget(local)
    bottom.addWidget(tool('search','Find a request',lambda:window.search_palette.open() if USE_REQON_UI else window.global_search.setFocus(),text='Find'))
    window.console_button=tool('console','Request console',window.show_console,text='Console');window.console_button.setCheckable(True);bottom.addWidget(window.console_button);bottom.addStretch()
    bottom.addWidget(tool('download','Export collection',window.export_collection,text='Export'))
    bottom.addWidget(tool('folder','Open local data folder',lambda:QDesktopServices.openUrl(QUrl.fromLocalFile(str(window.store.path.parent))),text='Local data'))
    for button in footer.findChildren(QToolButton):set_style(button,stylesheet('font-size:11px;padding:0;'));button.setIconSize(QSize(13,13))
    outer.addWidget(footer)
    if USE_REQON_UI:
        from ui_environments import EnvironmentController
        window.environments=EnvironmentController(window)
        window.new_menu.addAction(icon('environment'),'Environment',window.environments.create)
        from ui_windows import wire_search,WorkspaceStates
        wire_search(window)
        window.empty_states=WorkspaceStates(window)
