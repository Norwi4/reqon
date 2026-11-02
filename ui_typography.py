"""Bundled UI/editor fonts, loaded for this process without installing OS fonts."""
from ui_theming import set_style
import sys
from pathlib import Path
from PySide6.QtGui import QFont,QFontDatabase,QFontInfo
from PySide6.QtWidgets import QApplication

UI_FAMILY='Inter'
CODE_FAMILY='JetBrains Mono'


def ui_font(size=13,weight=QFont.Weight.Normal):
    font=QFont(UI_FAMILY);font.setPixelSize(size);font.setWeight(weight)
    font.setStyleStrategy(QFont.StyleStrategy.PreferAntialias)
    return font


def load_fonts():
    app=QApplication.instance()
    if app is None or app.property('reqonFontsLoaded'):return
    root=Path(getattr(sys,'_MEIPASS',Path(__file__).resolve().parent))/'assets/fonts'
    loaded=[]
    for path in sorted(root.glob('*.ttf')):
        identifier=QFontDatabase.addApplicationFont(str(path))
        if identifier<0:raise RuntimeError('Cannot load bundled font: '+str(path))
        loaded.extend(QFontDatabase.applicationFontFamilies(identifier))
    if not {UI_FAMILY,CODE_FAMILY}.issubset(loaded):raise RuntimeError('Required bundled Inter / JetBrains Mono fonts are missing.')
    app.setFont(ui_font());app.setProperty('reqonFontsLoaded',True)


def apply_editor_font(editor,config):
    load_fonts()
    families=[name.strip().strip('\"\'') for name in config.font_family.split(',') if name.strip()]
    available={name.casefold():name for name in QFontDatabase.families()};family=next((available[name.casefold()] for name in families if name.casefold() in available),CODE_FAMILY)
    font=QFont(family);font.setPixelSize(config.font_size);font.setWeight(QFont.Weight.Normal)
    font.setStyleStrategy(QFont.StyleStrategy.PreferAntialias)
    # A widget-local style wins over application QSS; set the document font too.
    if not hasattr(editor,'_reqon_font_base_style'):editor._reqon_font_base_style=getattr(editor,'_theme_style_source',editor.styleSheet())
    selector='QTextEdit#responseErrorDetails' if editor.inherits('QTextEdit') else 'QPlainTextEdit#codeEditor'
    set_style(editor,editor._reqon_font_base_style+'\n'+selector+' {font-family:"'+family.replace('"','')+'";font-size:'+str(config.font_size)+'px;font-weight:400;}')
    editor._reqon_last_font_style=editor.styleSheet()
    editor.setFont(font);editor.document().setDefaultFont(font)
    editor.setTabStopDistance(editor.fontMetrics().horizontalAdvance(' ')*config.indent_count)
    if hasattr(editor,'gutter'):editor.gutter.setFont(font);editor.update_margin();editor.gutter.update()
    return QFontInfo(font).family()


STYLE='''
QWidget {font-family:"Inter";font-size:13px;}
QToolButton#navButton {font-size:13px;font-weight:500;}
QTreeWidget,QTreeView {font-size:12px;font-weight:400;}
QTreeWidget#collectionTree,QTreeWidget#collectionTree QLineEdit {font-family:"Inter";font-size:12px;}
QFrame#requestBar QLineEdit,QFrame#methodPopup QLineEdit,QFrame#methodPopup QListWidget {font-family:"Inter";font-size:13px;}
QTabBar {font-size:12px;font-weight:500;}
QPushButton#sendButton,QToolButton#saveRequestButton {font-size:12px;font-weight:600;}
QTableWidget,QTableView,QHeaderView {font-size:12px;font-weight:400;}
QPlainTextEdit#codeEditor {font-family:"JetBrains Mono";font-size:13px;font-weight:400;}
QFrame#footer QLabel,QFrame#footer QToolButton {font-size:11px;font-weight:400;}
'''
