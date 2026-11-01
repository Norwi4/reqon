"""Render Windows Qt UI with only local fixture data; optional DPI checks."""
import argparse,json,os,sys,tempfile,ssl
from pathlib import Path
args=argparse.ArgumentParser();args.add_argument('--scale',default='1');args.add_argument('--compact-only',action='store_true');options=args.parse_args()
os.environ['QT_SCALE_FACTOR']=options.scale
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from PySide6.QtWidgets import QApplication
from PySide6.QtTest import QTest
from main import MainWindow,Store,Worker
from ui_theme import STYLE
from ui_brand import IS_WINDOWS,VERSION
from ui_error_states import decorate_failure
from response_errors import failure_result
from error_fixture import ErrorServer

assert IS_WINDOWS
app=QApplication([]);app.setStyle('Fusion');app.setStyleSheet(STYLE)
out=Path('docs/ui');out.mkdir(parents=True,exist_ok=True)
prefix='reqon-windows-'+VERSION+'-'+str(round(float(options.scale)*100))
with tempfile.TemporaryDirectory(prefix='reqon-visual-') as directory:
    server=ErrorServer();store=Store(directory)
    grpc=store.insert('collections',{'name':'Customer services','kind':'grpc','expanded':1})
    http=store.insert('collections',{'name':'Customer REST API','kind':'http','expanded':1})
    for name,method,url in [('List customers','GET','customers'),('Create customer','POST','customers'),('Update profile','PATCH','customers/101'),('Delete customer','DELETE','customers/101')]:
        store.save_request({'name':name,'method':method,'url':'http://localhost:8080/'+url},collection_id=http)
    data=dict(server.data('Echo'),name='Customer details');rid=store.save_request(data,collection_id=grpc)
    window=MainWindow(store);window.resize(1370,990)
    window.open_item(window.tree_item('request',rid));page=window.current_page()
    page.message.setPlainText('{\n  "name": "customer_101"\n}');window.show();QTest.qWait(380)
    def capture(name):
        QTest.qWait(90);window.grab().save(str(out/f'{prefix}-{name}.png'))
    def result(method):
        worker=Worker(server.data(method),{});values=[];worker.done.connect(values.append);worker.run();return values[0]
    capture('ready')
    if not options.compact_only:
        page.message.setPlainText('{\n  // Customer name\n  "name": "customer_101" /* preserved in the editor */\n}')
        QTest.qWait(320)
        worker=Worker(page.collect(),{});values=[];worker.done.connect(values.append);worker.run()
        page.show_response(values[0]);assert 'error' not in values[0];assert page.message.presentation.error is None
        capture('comments');page.message.setPlainText('{\n  "name": "customer_101"\n}')
        page.show_response(result('Echo'));capture('response')
        page.show_response(result('Reject'));capture('server-error')
        page.show_response(result('Unavailable'));capture('unavailable')
        error=ssl.SSLError('The local test certificate could not be verified.')
        page.show_response(decorate_failure(failure_result(error,'grpc',35),error));capture('tls')
        page.message.setPlainText('{\n  "name":\n}');QTest.qWait(320);capture('validation')
        palette=window.search_palette;palette.open();palette.input.setText('customer');QTest.qWait(150)
        palette.grab().save(str(out/f'{prefix}-search.png'));palette.hide()
        page.message.setPlainText('{}')
        page.show_response(result('Unavailable'))
    window.resize(1000,700);capture('compact')
    assert window.width()==1000 and window.height()==700,window.size()
    assert window.titlebar.height()==48
    print(json.dumps({'scale':options.scale,'device_pixel_ratio':window.devicePixelRatioF(),'size':[window.width(),window.height()],'window_icon_sizes':[s.width() for s in window.windowIcon().availableSizes()]}))
    window.close();app.processEvents();server.close();store.db.close()
