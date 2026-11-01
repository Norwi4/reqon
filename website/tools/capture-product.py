"""Render the actual Reqon widgets using an isolated workspace and local demo APIs.

Optional asset authoring helper; requires the desktop app's Python dependencies.
No user workspace or remote API is accessed.
"""
import json
import os
import sys
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

os.environ['QT_QPA_PLATFORM'] = 'offscreen'
os.environ['QT_SCALE_FACTOR'] = '1'
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
sys.path.insert(0, str(ROOT / 'tests'))
from PySide6.QtWidgets import QApplication
from PySide6.QtTest import QTest
from main import MainWindow, Store, Worker
from ui_pages import HTTP_DEFAULT
from ui_theme import STYLE
from ui_typography import load_fonts
from error_fixture import ErrorServer


class DemoAPI(BaseHTTPRequestHandler):
    def do_GET(self):
        body = json.dumps({
            'data': [
                {'id': 'cus_1001', 'name': 'Northwind', 'status': 'active'},
                {'id': 'cus_1002', 'name': 'Contoso', 'status': 'active'},
            ],
            'pagination': {'page': 1, 'limit': 20, 'total': 2},
        }).encode()
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.send_header('X-Request-Id', 'demo-request-001')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_args):
        pass


app = QApplication([])
load_fonts()
app.setStyle('Fusion')
app.setStyleSheet(STYLE)
output = ROOT / 'website/public/assets/screenshots'
output.mkdir(parents=True, exist_ok=True)
http = ThreadingHTTPServer(('127.0.0.1', 0), DemoAPI)
threading.Thread(target=http.serve_forever, daemon=True).start()
grpc = ErrorServer()

try:
    with tempfile.TemporaryDirectory(prefix='reqon-site-demo-') as directory:
        store = Store(directory)
        collection = store.insert('collections', {'name': 'Customer API', 'kind': 'http', 'expanded': 1})
        ids = []
        for name, method, endpoint in [('List customers', 'GET', '/v1/customers'), ('Create customer', 'POST', '/v1/customers'), ('Update customer', 'PATCH', '/v1/customers/cus_1001'), ('Delete customer', 'DELETE', '/v1/customers/cus_1001')]:
            request = dict(HTTP_DEFAULT, name=name, method=method,
                           url=f'http://127.0.0.1:{http.server_port}{endpoint}',
                           params=[['page', '1', 'Page number'], ['limit', '20', 'Customers per page']])
            ids.append(store.save_request(request, collection_id=collection))
        grpc_collection = store.insert('collections', {'name': 'Customer gRPC', 'kind': 'grpc', 'expanded': 1})
        grpc_data = dict(grpc.data('Echo'), name='Get customer', message=json.dumps({'name': 'Northwind'}, indent=2))
        grpc_id = store.save_request(grpc_data, collection_id=grpc_collection)
        window = MainWindow(store)
        window.resize(1370, 860)
        window.open_item(window.tree_item('request', ids[0]))
        window.show()
        QTest.qWait(200)

        def capture(name):
            page = window.current_page()
            values = []
            worker = Worker(page.collect(), {})
            worker.done.connect(values.append)
            worker.run()
            assert values and 'error' not in values[0], values
            page.show_response(values[0])
            QTest.qWait(250)
            assert window.grab().save(str(output / f'{name}.png'))

        capture('http-current')
        window.open_item(window.tree_item('request', grpc_id))
        capture('grpc-current')
        window.close()
        app.processEvents()
        store.db.close()
finally:
    http.shutdown()
    http.server_close()
    grpc.close()

print('Rendered current HTTP and gRPC views with local demo data.')
