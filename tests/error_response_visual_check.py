"""Render native error layouts using only an isolated loopback fixture."""
import sys
import tempfile
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from PySide6.QtWidgets import QApplication
from PySide6.QtTest import QTest
from main import MainWindow,Store,Worker
from ui_theme import STYLE
from error_fixture import ErrorServer

app=QApplication([]);app.setStyle('Fusion');app.setStyleSheet(STYLE)
with tempfile.TemporaryDirectory(prefix='getman-error-visual-') as directory:
    server=ErrorServer();store=Store(directory);window=MainWindow(store);window.resize(1370,990)
    cid=store.insert('collections',{'name':'Customer Service API','kind':'grpc'})
    data=dict(server.data(),name='GetCustomer')
    rid=store.save_request(data,collection_id=cid);window.refresh_tree()
    page=window.new_grpc_request(data,rid);page.collection_id=cid
    window.show();QTest.qWait(250)
    out=Path('docs/ui');out.mkdir(parents=True,exist_ok=True)
    for method,name in [('Reject','server'),('Unavailable','connection')]:
        worker=Worker(server.data(method),{});results=[];worker.done.connect(results.append);worker.run()
        page.show_response(results[0]);QTest.qWait(100);window.grab().save(str(out/f'error-{name}.png'))
    window.resize(1000,700);QTest.qWait(100);window.grab().save(str(out/'error-compact.png'))
    for i in range(window.workbench.count()):
        draft=window.workbench.widget(i);draft._saved=draft.collect()
    window.close();app.processEvents();server.close();store.db.close()
print('Rendered server, connection and compact error views in docs/ui.')
