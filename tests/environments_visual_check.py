"""Native Windows environment and collection previews; no external requests."""
import argparse,json,os,sys,tempfile
from pathlib import Path
parser=argparse.ArgumentParser();parser.add_argument('--scale',default='1');options=parser.parse_args()
os.environ['QT_SCALE_FACTOR']=options.scale
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from PySide6.QtCore import QPoint
from PySide6.QtTest import QTest
from PySide6.QtWidgets import QApplication
from main import MainWindow,Store
from ui_theme import STYLE
from ui_brand import VERSION

app=QApplication([]);app.setStyle('Fusion');app.setStyleSheet(STYLE)
out=Path('docs/ui');out.mkdir(parents=True,exist_ok=True);prefix=f'reqon-{VERSION}-environments-{round(float(options.scale)*100)}'
with tempfile.TemporaryDirectory(prefix='reqon-environments-visual-') as directory:
    store=Store(directory);window=MainWindow(store);window.resize(1370,990);window.show();window.activateWindow();window.raise_()
    controller=window.environments;window.select_nav('Environments');page=window.workbench.currentWidget();page.name.setText('Local development')
    def capture(name,widget=window):
        QTest.qWait(80);widget.grab().save(str(out/f'{prefix}-{name}.png'))
    capture('new')
    for name,value in [('base_url','http://localhost:8080'),('access_token','local-token'),('user_id','101'),('tasks_id','42')]:
        row=page.rows.canvas.items[-1];row.key.setText(name);row.value.setText(value)
    QTest.qWait(300);page.rows.canvas.items[1].hovered=True;page.rows.canvas.items[1].refresh_actions();capture('row-hover')
    controller.sidebar.list.show_hover(controller.sidebar.list.item(0));capture('list-hover')
    controller.show_colors(page.environment_id,controller.sidebar.list.color);capture('palette',controller.popup);controller.popup.close()
    controller.set_color(page.environment_id,'#b3261e');controller.apply(page.environment_id);capture('active')
    controller.show_menu(page.environment_id,controller.sidebar.list.mapToGlobal(QPoint(130,30)));capture('menu',controller.menu);controller.menu.close()
    page.rows.canvas.move_row(0,3);QTest.qWait(200);capture('reordered')
    cid=store.insert('collections',{'name':'CustomerService','kind':'grpc','expanded':1})
    fid=store.insert('folders',{'collection_id':cid,'name':'Customers','expanded':1})
    for method in ('Create','GetById','Update'):
        store.save_request({'kind':'grpc','name':method,'server':'localhost:50051','service':'example.Customers','rpc_method':method,'message':'{}'},collection_id=cid,folder_id=fid)
    window.refresh_tree();window.open_item(window.tree_item('collection',cid));collection=window.workbench.currentWidget();capture('collection')
    window.resize(1000,700);capture('collection-compact');assert window.size().width()==1000
    controller.open(page.environment_id);capture('compact');assert window.size().width()==1000
    print(json.dumps({'scale':options.scale,'device_pixel_ratio':window.devicePixelRatioF(),'rows':len(page.rows.data()),'active_environment':window.env_combo.currentText(),'size':[window.width(),window.height()]}))
    window.close();app.processEvents();store.db.close()
