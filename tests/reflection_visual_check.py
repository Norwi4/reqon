"""Capture the three real reflection states using a local controlled gRPC server."""
import sys
import tempfile
import time
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from PySide6.QtWidgets import QApplication
from PySide6.QtTest import QTest
from main import MainWindow,Store
from ui_pages import Job
from ui_theme import STYLE
from reflection_fixture import ReflectionServer

app=QApplication([]);app.setStyle('Fusion');app.setStyleSheet(STYLE)
temp=tempfile.TemporaryDirectory();store=Store(temp.name);window=MainWindow(store);window.resize(1370,990)
server=ReflectionServer('demo.api.v1');page=window.current_page();page.server.setText(server.address);page.name.setText('Customer API')
out=Path('docs/ui');out.mkdir(parents=True,exist_ok=True)
def wait_for(predicate):
 deadline=time.monotonic()+10
 while not predicate() and time.monotonic()<deadline:QTest.qWait(20)
 if not predicate():raise RuntimeError('Reflection preview timed out')
def capture(name):
 app.processEvents();window.grab().save(str(out/name))
window.show();QTest.qWait(150)
try:
 server.gate.clear();page.discover_methods();capture('reflection-loading.png')
 server.gate.set();wait_for(lambda:page.method_picker.state=='ready');capture('reflection-services.png')
 page.method_picker.editor.setText('Orders');capture('reflection-search.png')
 page.method_picker.editor.clear();server.disabled=True;page.start_discovery(force=True);wait_for(lambda:page.method_picker.state=='error');capture('reflection-error.png')
 page.method_picker.toggle_details();capture('reflection-error-details.png')
 print('Captured real reflection loading, grouped services, search, error and details.')
finally:
 page.method_picker.dismiss();server.gate.set();wait_for(lambda:not Job.running)
 page._saved=page.collect();window.close();server.close();store.db.close();temp.cleanup()
