"""Send a local POST from the installed X11 application in an isolated workspace."""
import json
import os
from pathlib import Path
import sqlite3
import ssl
import subprocess
import sys
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer

sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from main import Store
from PySide6.QtGui import QGuiApplication
from PySide6.QtTest import QTest

app=QGuiApplication([])

received=[];arrived=threading.Event()
class Handler(BaseHTTPRequestHandler):
    def log_message(self,*_):pass
    def do_POST(self):
        received.append(self.rfile.read(int(self.headers['Content-Length'])))
        body=b'{"linux_packaged":"ok"}'
        self.send_response(200);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body);arrived.set()

server=ThreadingHTTPServer(('127.0.0.1',0),Handler)
tls=os.environ.get('REQON_SMOKE_TLS')=='1'
certificates=Path(__file__).parent/'fixtures/tls'
if tls:
    context=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(str(certificates/'server.pem'),str(certificates/'server.key'))
    server.socket=context.wrap_socket(server.socket,server_side=True)
threading.Thread(target=server.serve_forever,kwargs={'poll_interval':.02},daemon=True).start()
try:
    with tempfile.TemporaryDirectory(prefix='reqon-linux-http-') as directory:
        store=Store(directory);cid=store.insert('collections',{'name':'Packaged Linux','kind':'http'})
        payload='{"source":"Linux EXE","unicode":"Привет"}'
        origin=('https://localhost' if tls else 'http://127.0.0.1')+f':{server.server_port}'
        rid=store.save_request(dict(name='Linux packaged POST',kind='http',method='POST',url=origin+'/verify',body_type='raw',body=payload,
            general_overrides=True,http_verify=True,http_ca_file=str(certificates/'ca.pem') if tls else ''),collection_id=cid)
        uid=store.entity('request',rid)['uid']
        store.db.execute("INSERT OR REPLACE INTO workspace_state(key,data) VALUES('session',?)",(json.dumps(dict(tabs=[dict(kind='request',uid=uid)],active=0)),));store.db.commit();store.db.close()
        for attempt in (1,2):
            arrived.clear()
            process=subprocess.Popen([os.environ.get('REQON_EXECUTABLE','/usr/bin/reqon'),'--data-dir',directory])
            try:
                deadline=time.monotonic()+30;window=None
                while not window and time.monotonic()<deadline:
                    listing=subprocess.run(['wmctrl','-lp'],text=True,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL)
                    for line in listing.stdout.splitlines():
                        fields=line.split(None,4)
                        if len(fields)>4 and fields[2]==str(process.pid) and 'Reqon' in fields[4]:window=fields[0];break
                    assert process.poll() is None
                    time.sleep(.1)
                assert window,'No packaged Linux window'
                subprocess.run(['wmctrl','-i','-r',window,'-e','0,100,100,1200,800'],check=True);time.sleep(.5)
                geometry=subprocess.check_output(['xdotool','getwindowgeometry','--shell',window],text=True)
                values=dict(line.split('=',1) for line in geometry.splitlines())
                subprocess.run(['xdotool','windowactivate','--sync',window,'mousemove','--window',window,str(int(values['WIDTH'])-100),'163','click','1'],check=True)
                assert arrived.wait(8),'Packaged Linux application did not send POST'
                assert json.loads(received[-1])==json.loads(payload)
                deadline=time.monotonic()+5
                while time.monotonic()<deadline:
                    with sqlite3.connect(Path(directory)/'reqon.sqlite3') as db:
                        if db.execute('SELECT COUNT(*) FROM history WHERE status=200').fetchone()[0]==attempt:break
                    time.sleep(.1)
                else:raise AssertionError('HTTP response was not saved')
                QTest.qWait(250)
                assert app.primaryScreen().grabWindow(int(window,16)).save('/out/linux-packaged-'+('https' if tls else 'http')+'.png')
                subprocess.run(['wmctrl','-i','-c',window],check=True);assert process.wait(15)==0
            finally:
                if process.poll() is None:process.terminate();process.wait(10)
        print(json.dumps(dict(launches=2,requests=len(received),tls=tls,payload='verified',history='verified',restoration='passed')))
finally:server.shutdown();server.server_close()
