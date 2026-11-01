"""The desktop palette has priority over the website's marketing palette."""
from ui_brand import COLORS as C,TAB_HEIGHT

STYLE=f'''
QToolButton#brandHome {{padding:6px;border-radius:6px;}}
QToolButton#brandHome:hover, QToolButton#brandHome:focus {{background:{C['hover']};}}
QLineEdit, QComboBox, QPushButton {{border-radius:5px;}}
QLineEdit:focus, QComboBox:focus, QPushButton:focus {{border-color:{C['focus']};}}
QToolButton:focus {{background:{C['hover']};}}
QFrame#requestBar {{border:1px solid {C['border']};border-radius:6px;}}
QPushButton#sendButton {{background:{C['action']};border-radius:5px;}}
QPushButton#sendButton:hover {{background:{C['action_hover']};}}
QTabBar {{font-size:12px;}}
QTabBar::tab {{padding:0 12px;height:{TAB_HEIGHT-1}px;border-bottom:1px solid transparent;color:{C['muted']};}}
QTabBar::tab:selected {{color:{C['text']};border-bottom:1px solid {C['accent']};}}
QTabWidget#workbench > QTabBar::tab {{min-width:92px;max-width:204px;border-right:0;}}
QTabWidget#workbench > QTabBar::tab:selected {{background:{C['hover']};}}
QTabWidget#workbench::pane {{border:0;border-top:1px solid {C['border']};}}
QToolButton#newRequestTab {{padding:4px;border-radius:4px;}}
QToolButton#newRequestTab:hover {{background:{C['hover']};}}
QToolButton#tabClose {{padding:4px;border-radius:4px;}}
QTabWidget#editorTabs > QTabBar::tab {{height:{TAB_HEIGHT-1}px;}}
QTreeWidget::item {{height:30px;}}
QTableWidget {{selection-background-color:{C['selected']};alternate-background-color:{C['canvas']};}}
QTableWidget::item:selected {{background:{C['selected']};}}
QHeaderView::section {{color:{C['muted']};padding:8px;}}
QLabel#editorValidation {{color:{C['error']};background:{C['error_bg']};border-radius:4px;padding:4px 8px;font-size:11px;}}
QFrame#quickSearch {{background:{C['panel']};border:1px solid {C['border']};border-radius:8px;}}
QListWidget#quickResults {{background:{C['panel']};border:0;outline:none;}}
QListWidget#quickResults::item {{padding:10px 12px;border-radius:4px;}}
QListWidget#quickResults::item:selected {{background:{C['selected']};color:{C['text']};}}
QLabel#searchHint {{color:{C['muted']};font-size:11px;padding:4px;}}
QLabel#emptyTitle {{color:{C['text']};font-size:15px;font-weight:600;}}
QLabel#emptyDescription {{color:{C['muted']};font-size:12px;}}
QLabel#localBadge {{color:{C['muted']};font-size:11px;}}
QSplitter#requestSplitter::handle {{background:{C['border']};}}
QSplitter#requestSplitter::handle:hover {{background:{C['focus']};}}
QPlainTextEdit#codeEditor QScrollBar:horizontal {{height:12px;margin:0;background:{C['canvas']};}}
QPlainTextEdit#codeEditor QScrollBar::handle:horizontal {{background:#525252;min-width:36px;border-radius:0;}}
QPlainTextEdit#codeEditor QScrollBar::handle:horizontal:hover {{background:#707070;}}
QPlainTextEdit#codeEditor QScrollBar::handle:horizontal:pressed {{background:#808080;}}
QToolButton#wrapLines:checked {{background:{C['selected']};}}
'''
