"""Render the HTTP editor with fictional local fixtures, never the user workspace."""
import argparse,os,sys,tempfile
from pathlib import Path
args=argparse.ArgumentParser();args.add_argument('--scale',default='1');options=args.parse_args()
os.environ['QT_SCALE_FACTOR']=options.scale;os.environ.setdefault('QT_QPA_PLATFORM','offscreen')
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from PySide6.QtWidgets import QApplication
from PySide6.QtTest import QTest
from main import MainWindow,Store
from ui_theme import STYLE
from ui_brand import VERSION
from ui_typography import load_fonts

app=QApplication([]);load_fonts();app.setStyle('Fusion');app.setStyleSheet(STYLE)
output=Path('docs/ui');output.mkdir(parents=True,exist_ok=True);prefix=f'http-{VERSION}-{options.scale}'
with tempfile.TemporaryDirectory() as directory:
    store=Store(directory);cid=store.insert('collections',{'name':'Example API','kind':'http'});store.save_request({'name':'List users','url':'https://api.example.test/users','method':'GET'},collection_id=cid)
    window=MainWindow(store);window.resize(1213,900);window.show();page=window.new_request({'name':'Untitled Request','url':'https://api.example.test/users'})
    def capture(name):QTest.qWait(300);window.grab().save(str(output/f'{prefix}-{name}.png'))
    page.tabs.setCurrentIndex(1);capture('auth-none');page.auth_type.setCurrentText('Basic Auth');capture('auth-basic')
    page.auth_type.setCurrentText('JWT Bearer');page.auth_panel.jwt_secret.setPlainText('example-local-secret');page.split.setSizes([600,160]);capture('auth-jwt')
    page.auth_panel.jwt_location.setCurrentText('Query Param');capture('auth-jwt-query');page.auth_type.setCurrentText('No Auth');page.split.setSizes([440,300])
    page.tabs.setCurrentIndex(2);capture('headers-hidden');page.headers_panel.toggle.click();capture('headers-expanded')
    page.tabs.setCurrentIndex(3);page.body_type.setCurrentText('form-data');page.body_modes.multipart.set_rows([['name','Example','',True,'Text'],['document','report.pdf','',True,'File']]);capture('body-form')
    page.body_type.setCurrentText('raw');page.body.setPlainText('{\n  "name": "Example",\n  "active": true\n}');capture('body-raw')
    page.body_type.setCurrentText('GraphQL');page.body_modes.graphql_query.setPlainText('query GetUser($id: ID!) {\n  user(id: $id) {\n    id\n    name\n  }\n}');page.body_modes.graphql_variables.setPlainText('{\n  "id": "42"\n}');capture('body-graphql')
    page.tabs.setCurrentIndex(4);page.split.setSizes([650,140]);capture('settings');window.resize(1213,1700);page.split.setSizes([1400,140]);capture('settings-full')
    window.resize(1213,900);page.split.setSizes([400,300]);page.tabs.setCurrentIndex(0);window.show_code_snippet();capture('snippet');window.hide_code_snippet()
    window.show_console();capture('console');window.show_console();window.select_nav('Collections',toggle=True);capture('collapsed');window.info_button.click();QTest.qWait(100);window.info_dialog.grab().save(str(output/f'{prefix}-info.png'));window.info_dialog.accept()
    window.resize(960,640);capture('compact');window.close();window.deleteLater();app.processEvents();store.db.close()
print('Saved HTTP editor visual checks to',output)
