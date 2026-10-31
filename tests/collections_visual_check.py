"""Capture real native widgets and menus using an isolated workspace."""
import json
import sys
import tempfile
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from PySide6.QtCore import QPoint
from PySide6.QtGui import QPainter
from PySide6.QtTest import QTest
from PySide6.QtWidgets import QApplication
from main import MainWindow, Store
from ui_pages import HTTP_DEFAULT, GRPC_DEFAULT
from ui_theme import STYLE

app = QApplication([]); app.setStyle('Fusion'); app.setStyleSheet(STYLE)
temporary = tempfile.TemporaryDirectory(); store = Store(temporary.name)
http = store.insert('collections', {'name': 'REST API basics: CRUD, test & variable', 'kind': 'http'})
store.save_request(dict(HTTP_DEFAULT, name='Get customers', url='http://localhost:8080/customers'), collection_id=http)
cid = store.insert('collections', {'name': 'ru.gov.pfr.ecp.uos.ozv.summaVozvrataKapitalizirovannihPlatezheyService', 'kind': 'grpc', 'expanded': 1})
names = ['SummaVozvrataKapitalizirovannihPlatezheyService', 'SvedeniyaObIzlishnemVziskaniiService', 'SvedeniyaUtochneniyaService', 'DeloService',
         'DokumentDelaService', 'DokumentResheniyaService', 'UvedomlenieZayavitelyuService', 'UvedomlenieOPereplateIzlishnihSredstv',
         'DeloCommonService', 'ReshenieService', 'RaschotProcentovService', 'OperaciyaResheniyaService', 'ZayavlenieService', 'SvedeniyaUplatiService', 'SvedeniyaOPereplateService', 'GrpcGeneralDataService']
first = None
for index, name in enumerate(names):
    fid = store.insert('folders', {'collection_id': cid, 'name': name, 'expanded': int(index == 0)})
    for method in ['Create', 'GetById']:
        rid = store.save_request(dict(GRPC_DEFAULT, name=method, server='localhost:28804', service=name, rpc_method=method,
                    message=json.dumps({'date_info': '2026-10-30', 'dic_source_info_code': '7', 'dic_stage_code': '9', 'organization_id': '371850'}, indent=2)), collection_id=cid, folder_id=fid)
        if first is None: first = rid
w = MainWindow(store); w.resize(1370, 990); w.show(); QTest.qWait(200)
w.open_item(w.tree_item('request', first)); QTest.qWait(100)
out = Path('docs/ui'); out.mkdir(parents=True, exist_ok=True)
def capture(name):
    app.processEvents(); QTest.qWait(120)
    # Capture only this isolated window, even if another application covers it.
    pixmap = w.grab(); painter = QPainter(pixmap)
    menu = getattr(w, '_tree_menu', None)
    if menu and menu.isVisible():
        painter.drawPixmap(w.mapFromGlobal(menu.pos()), menu.grab())
        for action in menu.actions():
            child = action.menu()
            if child and child.isVisible(): painter.drawPixmap(w.mapFromGlobal(child.pos()), child.grab())
    painter.end(); pixmap.save(str(out/name))
def menu_for(item):
    w.tree.update_hover(item); rect = w.tree.visualItemRect(item)
    w.show_tree_menu(item, w.tree.viewport().mapToGlobal(QPoint(w.tree.viewport().width()-24, rect.bottom()+2)))
    w.tree.update_hover(item)
    QTest.qWait(100)
    return w._tree_menu
capture('collections-tree.png')
menu = menu_for(w.tree_item('collection', cid)); capture('collections-menu.png'); menu.close()
menu = menu_for(w.tree_item('request', first)); capture('collections-request-menu.png'); menu.close()
w.open_item(w.tree_item('collection', cid)); capture('collections-overview.png')
menu = menu_for(w.tree_item('collection', cid)); submenu = menu.actions()[0].menu()
submenu.popup(menu.mapToGlobal(QPoint(menu.width()-3, menu.actionGeometry(menu.actions()[0]).top())))
capture('collections-add-request.png'); submenu.close(); menu.close()
for i in range(w.workbench.count()):
    page = w.workbench.widget(i); page._saved = page.collect()
w.close(); app.processEvents(); store.db.close(); temporary.cleanup()
print('Collection screenshots:', out.resolve())
