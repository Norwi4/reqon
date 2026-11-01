"""Exercise the legacy presentation fallback without changing the host platform."""
import sys,tempfile
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import ui_brand
ui_brand.IS_WINDOWS=False;ui_brand.USE_REQON_UI=False;ui_brand.DISPLAY_NAME='Getman';ui_brand.VERSION='0.11.0'
from PySide6.QtWidgets import QApplication,QSplitter,QTabBar
from main import MainWindow,Store
from ui_theme import STYLE
from request_transport import prepare_http,parse_message
raw='{\n// unchanged HTTP body on other platforms\n"value":1\n}'
assert prepare_http({'url':'http://localhost/','body_type':'raw','body':raw},{})[3]==raw.encode()
assert parse_message(raw)=={'value':1}  # Existing gRPC support is preserved.
app=QApplication([]);app.setStyle('Fusion');app.setStyleSheet(STYLE)
with tempfile.TemporaryDirectory() as directory:
    store=Store(directory);window=MainWindow(store);page=window.current_page()
    assert window.windowTitle()=='Getman — Local API Client'
    assert not hasattr(window,'brand_home') and not hasattr(window,'search_palette')
    assert not hasattr(window,'environments')
    assert not store.all("SELECT name FROM sqlite_master WHERE name='environment_details'")
    assert type(window.workbench.tabBar()) is QTabBar
    assert type(page.split) is QSplitter and page.split.handleWidth()==1
    assert not hasattr(page.message,'presentation')
    assert page.response.body_stack.count()==2 and page.response.body_stack.currentIndex()==0
    assert window.global_search.placeholderText()=='Search Getman'
    assert '#ff6c37' in STYLE and '#brandHome' not in STYLE
    window.close();store.db.close()
print('Legacy presentation fallback preserved.')
