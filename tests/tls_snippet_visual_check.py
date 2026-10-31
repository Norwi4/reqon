"""Render the native TLS hover cards and code snippet sidebar with isolated fixture data."""
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
    page=window.current_page();page.server.setText('localhost:28800');page.select_method('example.CustomerService','Update')
    page.message.setPlainText('{\n  // Fields from the service contract\n  "customer_id": "1689",\n  "name": "Example customer",\n  "active": true\n}')
    page.metadata.set_rows([['request_source','getman','']])
    cid=store.insert('collections',{'name':'Customer API','kind':'grpc'});page.collection_id=cid;window.flush_page(page);window.refresh_tree()
    out=Path('docs/ui');out.mkdir(parents=True,exist_ok=True)
    page.tls_button.show_card();QTest.qWait(100);page.tls_button.card.grab().save(str(out/'tls-disabled.png'))
    page.tls_button.click();QTest.qWait(100);page.tls_button.card.grab().save(str(out/'tls-enabled.png'))
    page.tls_button.card.hide();page.tls_button.click();window.show_code_snippet();QTest.qWait(250)
    window.grab().save(str(out/'grpc-code-snippet.png'))
    window.hide_code_snippet();window.resize(1000,700);app.processEvents();window.show_code_snippet();QTest.qWait(250)
    window.grab().save(str(out/'code-snippet-compact.png'))
    window.close();app.processEvents();store.db.close()
print('Rendered TLS cards and code snippet sidebar.')
