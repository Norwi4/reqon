"""Capture Settings at the reference screenshot size, using only local fixtures."""
from pathlib import Path
import sys,tempfile
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from PySide6.QtWidgets import QApplication
from PySide6.QtTest import QTest
from main import MainWindow,Store
from ui_theme import STYLE

app=QApplication([]);app.setStyle('Fusion');app.setStyleSheet(STYLE)
with tempfile.TemporaryDirectory() as directory:
    store=Store(directory);window=MainWindow(store);window.resize(1370,990);window.show();app.processEvents()
    page=window.current_page();page.name.setText('Customer API');page.server.setText('localhost:28800')
    page.tls_button.click();page.tls_verify.setChecked(False);page.tabs.setCurrentIndex(5)
    page.split.setSizes([350,400])
    cid=store.insert('collections',{'name':'Customer API','kind':'grpc'});page.collection_id=cid;window.flush_page(page);window.refresh_tree()
    out=Path('docs/ui');out.mkdir(parents=True,exist_ok=True)
    QTest.qWait(200);window.grab().save(str(out/'grpc-settings.png'))
    window.resize(1000,700);QTest.qWait(150);window.grab().save(str(out/'grpc-settings-compact.png'))
    window.close();window.deleteLater();app.processEvents();store.db.close()
print('Rendered native Settings controls.')
