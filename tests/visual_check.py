"""Render actual Windows Qt widgets with isolated local fixture data."""
import json
import sys
import tempfile
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from PySide6.QtWidgets import QApplication
from PySide6.QtCore import QTimer
from main import MainWindow,Store
from ui_pages import GRPC_DEFAULT,HTTP_DEFAULT
from ui_theme import STYLE

app=QApplication([]); app.setStyle('Fusion'); app.setStyleSheet(STYLE)
temporary=tempfile.TemporaryDirectory(prefix='getman-visual-')
store=Store(temporary.name)
http=store.insert('collections',{'name':'REST API basics: CRUD, test & variables','kind':'http'})
grpc=store.insert('collections',{'name':'Customer Service API','kind':'grpc'})
message={'organization_id':'371850','case_id':'93227306854','decision_date':'2026-10-30','status_code':'9','source_code':'7'}
g=dict(GRPC_DEFAULT,name='localhost:50051',server='localhost:50051',service='CustomerService',rpc_method='GetCustomer',message=json.dumps(message,indent=2),metadata=[['request_source','getman',''],['userId','e1232bd4-86a4-4e51-82a9-8689a9347350','']])
store.save_request(g,collection_id=grpc)
store.save_request(dict(HTTP_DEFAULT,name='Get customers',url='https://api.example.com/customers'),collection_id=http)
w=MainWindow(store); w.resize(1370,990)
w.workbench.removeTab(0)
for name in ['List customers','Create customer','localhost:50051']:
 w.new_grpc_request(dict(g,name=name))
page=w.current_page()
page.show_response({'status':0,'reason':'OK','duration':7640,'size':528,'headers':[('content-type','application/grpc')],'trailers':[('request-id','req_019')],'body':json.dumps({'info':{'result':'OK','duration_ms':'7','message':'Customer loaded successfully','data':{'organization_id':'371850','status':'active'},'result_code':'0'},'customer_id':'customer_101'},indent=2)})
page._saved=page.collect()
for i in range(w.tree.topLevelItemCount()):w.tree.topLevelItem(i).setExpanded(False)
out=Path('docs/ui'); out.mkdir(parents=True,exist_ok=True)
def capture():
 app.processEvents(); w.grab().save(str(out/'grpc-message.png'))
 page.tabs.setCurrentIndex(3); app.processEvents(); w.grab().save(str(out/'grpc-metadata.png'))
 page._schema_key=page.schema_context()
 page.show_schema(('GetCustomerRequest',[('organization_id','Int64Value'),('case_id','Int64Value'),('decision_date','StringValue'),('status_code','StringValue'),('source_code','StringValue')],'GetCustomerResponse',[('info','ResponseInfo'),('customer_id','StringValue')]))
 page.tabs.setCurrentIndex(0); app.processEvents(); w.grab().save(str(out/'grpc-docs.png'))
 h=w.new_request(dict(HTTP_DEFAULT,name='Get customers',url='https://api.example.com/customers',params=[['page','1','Page number'],['limit','25','Results per page']]))
 h.show_response({'status':200,'reason':'OK','duration':128,'size':342,'headers':{'Content-Type':'application/json'},'body':json.dumps({'data':[{'id':101,'name':'Northwind','active':True}],'page':1,'total':1},indent=2)})
 app.processEvents(); w.grab().save(str(out/'http-request.png'))
 w.resize(1000,700); app.processEvents(); w.grab().save(str(out/'compact.png'))
 print('Rendered gRPC message, metadata, docs, HTTP and compact views:',out.resolve())
 app.quit()
w.show(); QTimer.singleShot(500,capture); app.exec()
store.db.close(); temporary.cleanup()
