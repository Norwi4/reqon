import json
import socket
import sys
import threading
import unittest
import urllib.error
from http.server import BaseHTTPRequestHandler,HTTPServer
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import grpc
from PySide6.QtWidgets import QApplication
from PySide6.QtGui import QFontMetricsF
from PySide6.QtCore import Qt
from PySide6.QtTest import QTest
from main import Worker
from response_errors import failure_result
from ui_pages import ResponsePanel
from ui_theme import STYLE
from error_fixture import ErrorServer,SERVER_DETAILS


class ResponseErrors(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app=QApplication.instance() or QApplication([])
        cls.app.setStyle('Fusion');cls.app.setStyleSheet(STYLE)
        cls.server=ErrorServer()
    @classmethod
    def tearDownClass(cls):cls.server.close()
    def setUp(self):
        self.panel=ResponsePanel(True);self.panel.resize(940,430);self.panel.show();QTest.qWait(20)
    def tearDown(self):self.panel.close();self.panel.deleteLater();self.app.processEvents()
    def execute(self,data):
        results=[];worker=Worker(data,{});worker.done.connect(results.append);worker.run()
        self.assertEqual(len(results),1);return results[0]
    def show(self,result):
        self.panel.show_response(result);QTest.qWait(30)
    def test_server_status_details_metadata_and_copy(self):
        result=self.execute(self.server.data())
        self.assertEqual((result['status'],result['reason']),(9,'FAILED_PRECONDITION'))
        self.assertEqual(result['body'],SERVER_DETAILS)
        self.assertEqual(result['headers'],[['source','local-test']])
        self.assertIn(['diagnostic-bin','AP8='],result['trailers'])
        self.assertEqual(json.loads(json.dumps(result)),result)
        self.show(result)
        self.assertEqual(self.panel.status.text(),'9 FAILED_PRECONDITION')
        self.assertEqual(self.panel.status.tone,'error')
        badge=self.panel.status;available=badge.width()-14
        self.assertGreaterEqual(available,QFontMetricsF(badge.font()).horizontalAdvance(badge.text()))
        self.assertEqual(badge.fontMetrics().elidedText(badge.text(),Qt.TextElideMode.ElideRight,available),badge.text())
        self.assertEqual(self.panel.error_panel.title.text(),'Precondition failed')
        self.assertEqual(self.panel.error_panel.details.toPlainText(),SERVER_DETAILS)
        self.assertEqual(self.panel.tabs.tabText(1),'Metadata (1)')
        self.assertEqual(self.panel.tabs.tabText(2),'Trailers (2)')
        self.assertEqual(self.panel.body_stack.currentIndex(),1)
        self.assertTrue(self.panel.error_panel.illustration.isVisible())
        self.assertGreater(self.panel.metrics.width(),20)
        self.panel.copy_button.click()
        self.assertIn(SERVER_DETAILS,self.app.clipboard().text())
        self.assertTrue(self.app.clipboard().text().startswith('9 FAILED_PRECONDITION'))
    def test_connection_refused_and_server_unavailable(self):
        # A bound, non-listening local TCP port cannot be reused by another test.
        with socket.socket() as sock:
            sock.bind(('127.0.0.1',0))
            data=self.server.data();data['server']=f'127.0.0.1:{sock.getsockname()[1]}'
            # Windows may take a second to report ECONNREFUSED. Do not turn
            # this connection test into a deadline test on slower builders.
            data['grpc_timeout_ms']=8000
            result=self.execute(data)
        self.assertEqual(result['status'],14)
        self.assertEqual(result['error_info']['category'],'connection')
        self.show(result);self.assertEqual(self.panel.status.text(),'14 UNAVAILABLE')
        self.assertEqual(self.panel.error_panel.title.text(),'Service unavailable')
        self.assertTrue(self.panel.error_panel.details.toPlainText())
        result=self.execute(self.server.data('Unavailable'))
        self.assertEqual(result['status'],14)
        self.assertEqual(result['body'],'Service temporarily unavailable.')
    def test_timeout_and_cancellation_have_distinct_states(self):
        data=self.server.data('Slow');data['grpc_timeout_ms']=80
        result=self.execute(data)
        self.assertEqual(result['status'],4)
        self.assertEqual(result['error_info']['illustration'],'timeout')
        cancelled=failure_result(RuntimeError('closed'),'grpc',40,'Request cancelled.',True)
        self.show(cancelled)
        self.assertEqual(self.panel.status.text(),'1 CANCELLED')
        self.assertEqual(self.panel.status.tone,'neutral')
        self.assertEqual(self.panel.editor.toPlainText(),'Request cancelled.')
    def test_state_reset_and_compact_long_plain_text(self):
        self.show(self.execute(self.server.data()))
        success=self.execute(self.server.data('Echo'));self.show(success)
        self.assertEqual(self.panel.body_stack.currentIndex(),0)
        self.assertEqual(self.panel.status.text(),'0 OK')
        self.assertEqual(self.panel.status.tone,'success')
        self.assertEqual(self.panel.tabs.tabText(1),'Metadata')
        self.assertEqual(self.panel.tabs.tabText(2),'Trailers')
        text='<b>Server detail is plain text</b>\n'+('LONG_TOKEN_'*120)
        self.panel.resize(580,190)
        self.show(failure_result(ValueError(text),'grpc',1))
        self.assertEqual(self.panel.error_panel.details.toPlainText(),text)
        self.assertFalse(self.panel.error_panel.illustration.isVisible())
        self.assertLessEqual(self.panel.error_panel.card.width(),self.panel.error_panel.viewport().width())
        self.assertEqual(self.panel.error_panel.horizontalScrollBar().maximum(),0)
        self.assertEqual(self.panel.error_panel.details.horizontalScrollBar().maximum(),0)
        self.assertLessEqual(self.panel.error_panel.details.height(),200)
        self.assertEqual(self.panel.status.text(),'REQUEST_ERROR')
        self.assertEqual(self.panel.tabs.tabText(2),'Trailers')
    def test_tls_details_through_wrapped_rpc_cause(self):
        class Rpc(grpc.RpcError):
            def code(self):return grpc.StatusCode.UNAVAILABLE
            def details(self):return 'Transport closed'
        rpc=Rpc();wrapped=ConnectionError('TLS: CERTIFICATE_VERIFY_FAILED: unable to get local issuer certificate')
        wrapped.__cause__=rpc
        result=failure_result(wrapped,'grpc',11)
        self.assertEqual(result['status'],14)
        self.assertEqual(result['error_info']['title'],'Certificate verification failed')
        self.assertIn('issuer',result['body'])
    def test_http_transport_error_has_no_fake_http_status(self):
        result=failure_result(urllib.error.URLError(ConnectionRefusedError('Connection refused')),'http',12)
        self.assertEqual(result['status'],-1)
        self.assertEqual(result['error_info']['badge'],'CONNECTION_ERROR')
    def test_http_404_keeps_response_body(self):
        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                self.send_response(404);self.send_header('Content-Type','application/json');self.end_headers()
                self.wfile.write(b'{"error":"customer not found"}')
            def log_message(self,*args):pass
        server=HTTPServer(('127.0.0.1',0),Handler)
        thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
        try:
            result=self.execute({'method':'GET','url':f'http://127.0.0.1:{server.server_port}/missing'})
            self.assertNotIn('error',result);self.assertEqual(result['status'],404)
            panel=ResponsePanel();panel.show_response(result)
            self.assertEqual(panel.body_stack.currentIndex(),0)
            self.assertEqual(panel.status.text(),'404 Not Found');self.assertEqual(panel.status.tone,'error')
            self.assertIn('customer not found',panel.editor.toPlainText());panel.deleteLater()
        finally:server.shutdown();server.server_close();thread.join(3)


if __name__=='__main__':unittest.main(verbosity=2)
