"""Commented JSON through the Windows editor, persistence and local transports."""
import json
from http.server import BaseHTTPRequestHandler,HTTPServer
from pathlib import Path
import sys
import tempfile
import threading
import unittest
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from PySide6.QtWidgets import QApplication
from PySide6.QtTest import QTest
from main import MainWindow,Store,Worker
from ui_brand import USE_REQON_UI
from ui_theme import STYLE
from json_comments import loads,normalize,format_json,comment_ranges
from request_transport import prepare_http,parse_message
from code_snippets import command_groups
from error_fixture import ErrorServer

SAMPLE='''{
    "data_okonchaniya_sroka_vineseniya_resheniya_date": "2026-11-05T08:05:41+03:00",
    //"data_otpravki_resheniya_strahovatelu_date": "",
    "dic_status_decision_code": "26", //3-подписано 8-отправлено 26-удалено
    "priznak_dosrochki": false,
    /* "disabled": "do not send", */
}'''


class JSONComments(unittest.TestCase):
    def test_comments_preserve_strings_positions_and_enabled_fields(self):
        self.assertEqual(loads(SAMPLE),{'data_okonchaniya_sroka_vineseniya_resheniya_date':'2026-11-05T08:05:41+03:00','dic_status_decision_code':'26','priznak_dosrochki':False})
        strings={'url':'https://example.test/a/*b*/?q=//','quoted':'say "hello" // keep','path':'C:\\folder\\','unicode':'😀 русский'}
        text='// introductory comment\r\n'+json.dumps(strings,ensure_ascii=False) + '\r\n/* ending */'
        self.assertEqual(loads(text),strings)
        self.assertEqual(len(normalize(text)),len(text));self.assertEqual(normalize(text).count('\n'),text.count('\n'))
    def test_invalid_json_and_unclosed_comments_retain_editor_location(self):
        text='{\n // some comment\n "value": ???\n}'
        with self.assertRaises(json.JSONDecodeError) as result:loads(text)
        self.assertEqual((result.exception.lineno,result.exception.colno),(3,11))
        with self.assertRaises(json.JSONDecodeError) as result:loads('{\n /* not closed')
        self.assertEqual((result.exception.lineno,result.exception.colno),(2,2))
        self.assertIn('Unterminated block comment',str(result.exception))
    def test_format_preserves_comment_tokens_values_and_is_idempotent(self):
        cases=[SAMPLE,'{"a":true// { text , :\n}',
            '{/*first*/"name"/*key*/:/*value*/"https://example.test/", "items":[1,2,]//last\n}',
            '[//start\n{"empty":{/*nested*/},"values":[/*nothing*/]},/*tail*/]',
            '// start\n{"x":1, /* multiple\n lines */ "y":2}']
        for text in cases:
            with self.subTest(text=text):
                formatted=format_json(text)
                self.assertEqual(loads(formatted),loads(text));self.assertEqual(format_json(formatted),formatted)
                self.assertEqual([text[a:b] for a,b in comment_ranges(text)[0]],[formatted[a:b] for a,b in comment_ranges(formatted)[0]])


@unittest.skipUnless(USE_REQON_UI,'Reqon release')
class WindowsJSONComments(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app=QApplication.instance() or QApplication([]);cls.app.setStyle('Fusion');cls.app.setStyleSheet(STYLE)
    def run_request(self,data,variables=None):
        worker=Worker(data,variables or {});results=[];worker.done.connect(results.append);worker.run()
        self.assertNotIn('error',results[0],results[0]);return results[0]
    def test_http_server_and_curl_receive_the_same_clean_json(self):
        captured=[]
        class Handler(BaseHTTPRequestHandler):
            def do_POST(self):
                body=self.rfile.read(int(self.headers['Content-Length']));captured.append((body,self.headers['Content-Type']))
                json.loads(body)  # A strict JSON server must accept the transmitted body.
                self.send_response(200);self.end_headers();self.wfile.write(b'{}')
            def log_message(self,*args):pass
        server=HTTPServer(('127.0.0.1',0),Handler);thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
        try:
            for headers in ([],[['Content-Type','application/json']], [['Content-Type','application/problem+json'],['Content-Length',str(len(SAMPLE.encode()))]]):
                data={'method':'POST','url':f'http://127.0.0.1:{server.server_port}/','body_type':'raw','body':'// payload\n'+SAMPLE,'headers':headers}
                self.assertEqual(self.run_request(data)['status'],200)
                self.assertEqual(json.loads(captured[-1][0]),loads(SAMPLE));self.assertNotIn(b'//',captured[-1][0])
                _,groups=command_groups(data);payload=next(group[1] for group in groups if group[0]=='--data-raw')
                self.assertEqual(payload.encode(),captured[-1][0]);self.assertEqual(data['body'],'// payload\n'+SAMPLE)
        finally:server.shutdown();server.server_close();thread.join(3)
    def test_non_json_raw_and_form_bodies_are_preserved(self):
        for text,content_type in [('https://example.test/a','text/plain'),(SAMPLE,'text/plain'),('<value>/* keep */ // keep</value>','application/xml')]:
            data={'url':'http://localhost/','method':'POST','body_type':'raw','body':text,'headers':[['Content-Type',content_type]]}
            self.assertEqual(prepare_http(data,{})[3],text.encode())
        data={'url':'http://localhost/','method':'POST','body_type':'x-www-form-urlencoded','form':[['value','// keep /* this */']]}
        self.assertIn(b'%2F%2F+keep',prepare_http(data,{})[3])
    def test_grpc_execution_and_grpcurl_ignore_comments(self):
        server=ErrorServer()
        try:
            text='// lead\n{\n//"disabled":"ignore",\n"name": "{{value}}" /* keep this comment in editor */,\n}'
            data=dict(server.data('Echo'),message=text);value='https://example.test/a/*b*/'
            result=self.run_request(data,{'value':value})
            self.assertEqual(json.loads(result['body']),{'name':value});self.assertEqual(data['message'],text)
            _,groups=command_groups(data,{'value':value});payload=next(group[1] for group in groups if group[0]=='-d')
            self.assertEqual(json.loads(payload),{'name':value});self.assertEqual(parse_message(text)['name'],'{{value}}')
        finally:server.close()
    def test_editor_validation_format_undo_autosave_and_collection_roundtrip(self):
        with tempfile.TemporaryDirectory() as directory:
            store=Store(directory);window=MainWindow(store);window.show()
            try:
                cid=store.insert('collections',{'name':'Commented JSON','kind':'grpc'})
                for kind in ('grpc','http'):
                    page=window.add_tree_request(cid,None,kind)
                    editor=page.message if kind=='grpc' else page.body
                    editor.setPlainText(SAMPLE);QTest.qWait(380)
                    self.assertIsNone(editor.presentation.error);self.assertEqual(editor.toPlainText(),SAMPLE)
                    self.assertTrue(editor.format_json());self.assertEqual(loads(editor.toPlainText()),loads(SAMPLE))
                    editor.undo();self.assertEqual(editor.toPlainText(),SAMPLE)
                window.flush_all();document=store.export_document(cid)
                for item in document['items']:
                    data=item['data'];self.assertEqual(data['message' if data.get('kind')=='grpc' else 'body'],SAMPLE)
                imported=store.import_document(document)
                for (data,) in store.all('SELECT data FROM requests WHERE collection_id=?',(imported,)):
                    data=json.loads(data);self.assertEqual(data['message' if data.get('kind')=='grpc' else 'body'],SAMPLE)
            finally:window.close();window.deleteLater();self.app.processEvents();store.db.close()


if __name__=='__main__':unittest.main(verbosity=2)
