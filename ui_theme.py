STYLE = '''
QWidget { color: #e6e6e6; font-family: "Segoe UI"; font-size: 13px; }
QMainWindow, QDialog { background: #212121; }
QWidget#shell, QWidget#page, QStackedWidget { background: #212121; }
QFrame#topbar, QFrame#workspaceHead, QWidget#sidebar, QFrame#rail, QFrame#utilityRail { background: #262626; }
QFrame#topbar { border-bottom: 1px solid #363636; }
QFrame#workspaceHead { border-bottom: 1px solid #333333; }
QFrame#rail { border-right: 1px solid #303030; }
QFrame#utilityRail { border-left: 1px solid #333333; background: #212121; }
QFrame#footer { background: #242424; border-top: 1px solid #383838; }
QFrame#requestBar { border: 1px solid #686868; border-radius: 4px; background: #212121; }
QFrame#requestBar QLineEdit, QFrame#requestBar QComboBox { border: 0; background: transparent; padding: 0 8px; }
QFrame#divider { background: #414141; }
QLineEdit { background: #262626; border: 1px solid #3d3d3d; border-radius: 3px; padding: 5px 9px; selection-background-color: #284c70; }
QLineEdit:focus { border-color: #6a6a6a; }
QLineEdit#requestName { border: 0; background: transparent; font-weight: 600; padding: 0; }
QLineEdit#search { font-size: 12px; background: #242424; }
QComboBox { background: #262626; border: 1px solid #3e3e3e; border-radius: 3px; padding: 5px 10px; }
QComboBox::drop-down { border: 0; width: 22px; }
QComboBox::down-arrow { image: none; }
QComboBox QAbstractItemView { background: #292929; border: 1px solid #444; selection-background-color: #3a3a3a; }
QPushButton { border: 1px solid #444; background: #2d2d2d; border-radius: 3px; padding: 6px 12px; }
QPushButton:hover { background: #383838; }
QToolButton { background: transparent; border: 0; border-radius: 3px; padding: 5px; color: #ababab; }
QToolButton:hover { background: #353535; color: #fff; }
QToolButton:checked { background: #3a3a3a; color: #fff; }
QToolButton::menu-indicator { image: none; width: 0; }
QToolButton#navButton { font-size: 14px; color: #efefef; padding: 6px; }
QToolButton#railButton { font-size: 10px; padding: 7px 0; }
QToolButton#windowClose:hover { background: #c42b1c; }
QToolButton#protocolButton { background: #292d33; padding: 4px; }
QToolButton#protocolButton:hover { background: #383e47; }
QPushButton#sendButton { background: #0668d2; border: 0; color: white; font-weight: 600; padding: 0; }
QPushButton#sendButton:hover { background: #157ce6; }
QPushButton#sendButton:disabled { background: #344d68; color: #b6b6b6; }
QPushButton#smallButton { padding: 4px 8px; background: #303030; border: none; font-size: 11px; font-weight: 600; }
QLabel#muted { color: #a0a0a0; }
QLabel#localBadge { color: #a6b7aa; font-size: 11px; }
QLabel#statusLabel { color: #74d99c; font-size: 12px; padding: 0 8px; }
QLabel#docsTitle { font-size: 14px; font-weight: 600; }
QTabWidget::pane { border: 0; }
QTabBar { background: #212121; }
QTabBar::tab { background: transparent; color: #ababab; padding: 0 12px; height: 38px; border-bottom: 1px solid transparent; }
QTabBar::tab:selected { color: #f5f5f5; border-bottom: 1px solid #ff6c37; }
QTabBar::tab:hover { color: #f5f5f5; }
QTabWidget#workbench > QTabBar::tab { border-right: 1px solid #303030; min-width: 80px; max-width: 180px; padding: 0 12px; }
QTabWidget#workbench > QTabBar::tab:selected { background: #272727; }
QTabWidget#editorTabs > QTabBar::tab { padding: 0 10px; height: 40px; }
QPlainTextEdit#codeEditor { background: #212121; border: 0; padding: 0; font-family: "Consolas"; font-size: 14px; selection-background-color: #284c70; }
QPlainTextEdit { background: #212121; border: 1px solid #333; selection-background-color: #284c70; }
QTreeWidget { background: #262626; border: none; outline: none; font-size: 12px; }
QTreeWidget::item { height: 28px; padding: 0 3px; }
QTreeWidget::item:selected { background: #393939; }
QTreeWidget::item:hover { background: #303030; }
QTableWidget { background: #212121; alternate-background-color: #212121; border: 1px solid #333; gridline-color: #333; selection-background-color: #30485b; outline: none; }
QTableWidget::item { padding: 0 8px; }
QTableWidget QLineEdit { padding: 0; border: none; background: #303030; }
QHeaderView::section { background: #212121; color: #aaa; font-weight: 600; border: none; border-right: 1px solid #333; border-bottom: 1px solid #333; padding: 8px; text-align: left; }
QTableWidget::indicator { width: 14px; height: 14px; }
QSplitter::handle { background: #343434; }
QMenu { background: #242424; border: 1px solid #373737; padding: 7px; }
QMenu::item { padding: 8px 24px 8px 8px; border-radius: 3px; }
QMenu::item:selected { background: #363636; }
QMenu::separator { height: 1px; background: #3b3b3b; margin: 5px; }
QMenu#collectionMenu, QMenu#collectionSubmenu { padding: 5px 7px; border-radius: 5px; }
QMenu#collectionMenu::item, QMenu#collectionSubmenu::item { padding: 5px 18px 5px 8px; font-size: 13px; }
QMenu#collectionMenu::separator, QMenu#collectionSubmenu::separator { margin: 7px 5px; }
QTreeWidget#collectionTree QLineEdit { border: 1px solid #6e9ecf; border-radius: 0; background: #212121; padding: 0 4px; }
QScrollArea { border: none; background: #212121; }
QScrollBar:vertical { background: transparent; width: 9px; margin: 2px; }
QScrollBar::handle:vertical { background: #454545; min-height: 24px; border-radius: 3px; }
QScrollBar:horizontal { background: transparent; height: 9px; margin: 2px; }
QScrollBar::handle:horizontal { background: #454545; min-width: 24px; border-radius: 3px; }
QScrollBar::add-line, QScrollBar::sub-line { width: 0; height: 0; }
QScrollBar::add-page, QScrollBar::sub-page { background: transparent; }
QToolTip { background: #353535; color: #eee; border: 1px solid #555; padding: 5px; }
QFrame#tlsCard { background:#212121; border:1px solid #3d3d3d; border-radius:5px; }
QFrame#tlsCard QLabel { background:transparent; border:0; font-size:12px; }
QFrame#tlsCard QLabel#tlsHeading { color:#f2f2f2; font-size:16px; font-weight:600; }
QFrame#tlsCard QLabel#tlsState { color:#aaa; font-weight:600; }
QFrame#snippetPanel { background:#212121; border-left:1px solid #333; }
QLabel#snippetTitle { color:#eee; font-size:14px; font-weight:600; padding:7px 0; }
'''

# Shared native presentation for Windows and Linux.
from ui_brand import USE_REQON_UI,stylesheet
if USE_REQON_UI:
    from ui_windows_theme import STYLE as WINDOWS_STYLE
    STYLE=stylesheet(STYLE)+WINDOWS_STYLE
    from ui_typography import STYLE as TYPOGRAPHY_STYLE
    STYLE+=TYPOGRAPHY_STYLE
    from ui_theme_rules import COMPONENT_STYLE
    STYLE+=COMPONENT_STYLE
