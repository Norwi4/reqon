"""Render the real Message toolbar and a contract-generated template in a temporary workspace."""
import sys
import tempfile
import time
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from PySide6.QtWidgets import QApplication
from PySide6.QtTest import QTest
from main import MainWindow, Store
from ui_pages import Job
from ui_theme import STYLE
from grpc_support import compile_proto

app=QApplication([]); app.setStyle('Fusion'); app.setStyleSheet(STYLE)
temporary=tempfile.TemporaryDirectory(); store=Store(temporary.name)
source=Path(temporary.name)/'customer.proto'
source.write_text('''syntax="proto3"; package customer.v1;
message Address { string city=1; string street=2; }
message Customer { int64 customer_id=1; string name=2; string email=3; bool active=4;
  Address address=5; repeated string tags=6; map<string,string> attributes=7; }
service CustomerService { rpc CreateCustomer(Customer) returns(Customer); }
''',encoding='utf-8')
descriptor=compile_proto(source,Path(temporary.name)/'definitions')
cid=store.insert('collections',{'name':'Customer Service API','kind':'grpc','expanded':1})
w=MainWindow(store); w.resize(1370,990); w.show()
page=w.add_tree_request(cid,None,'grpc'); page.name.setText('Create customer')
page.server.setText('localhost:50051'); page.descriptor_file.setText(descriptor)
page.select_method('customer.v1.CustomerService','CreateCustomer'); page.example_button.click()
deadline=time.monotonic()+10
while (page._example_loading or Job.running) and time.monotonic()<deadline:QTest.qWait(20)
assert 'customer_id' in page.message.toPlainText()
QTest.qWait(400); app.processEvents()
out=Path('docs/ui'); out.mkdir(exist_ok=True,parents=True)
w.grab().save(str(out/'grpc-example-message.png'))
w.resize(1000,700); app.processEvents(); w.grab().save(str(out/'grpc-example-message-compact.png'))
w.close(); app.processEvents(); store.db.close(); temporary.cleanup()
print('Example message screenshots:',out.resolve())
