"""Exercise JWT, multipart and workspace restoration through the built Windows EXE.

Mouse messages target only the test's child window and its known Send control.
The only request destination is a local fixture server in a temporary workspace.
"""
import ctypes,json,os,sqlite3,subprocess,sys,tempfile,threading,time
from ctypes import wintypes
from pathlib import Path
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
from contextlib import closing
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from PySide6.QtGui import QGuiApplication
from PySide6.QtTest import QTest
from cryptography.hazmat.primitives.asymmetric import rsa
from cryptography.hazmat.primitives import serialization
import jwt
from main import Store
from general_settings import GeneralPreferences,GeneralConfig

assert sys.platform=='win32'
root=Path(__file__).resolve().parents[1];exe=Path(sys.argv[1] if len(sys.argv)>1 else root/'dist/Reqon/Reqon.exe').resolve();assert exe.is_file()
api=ctypes.windll.user32;callback=ctypes.WINFUNCTYPE(wintypes.BOOL,wintypes.HWND,wintypes.LPARAM)
api.EnumWindows.argtypes=[callback,wintypes.LPARAM];api.GetWindowThreadProcessId.argtypes=[wintypes.HWND,ctypes.POINTER(wintypes.DWORD)]
api.GetWindowTextW.argtypes=[wintypes.HWND,wintypes.LPWSTR,ctypes.c_int];api.IsWindowVisible.argtypes=[wintypes.HWND]
api.PostMessageW.argtypes=[wintypes.HWND,wintypes.UINT,wintypes.WPARAM,wintypes.LPARAM];api.GetClientRect.argtypes=[wintypes.HWND,ctypes.POINTER(wintypes.RECT)];api.GetDpiForWindow.argtypes=[wintypes.HWND]
app=QGuiApplication([]);received=[];arrived=threading.Event();key=rsa.generate_private_key(65537,2048)
class Handler(BaseHTTPRequestHandler):
    def log_message(self,*_):pass
    def do_POST(self):
        body=self.rfile.read(int(self.headers['Content-Length']));received.append((dict(self.headers),body));arrived.set()
        response=b'{"packaged_http":"ok"}';self.send_response(200);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(response)));self.end_headers();self.wfile.write(response)
server=ThreadingHTTPServer(('127.0.0.1',0),Handler);threading.Thread(target=server.serve_forever,kwargs={'poll_interval':.02},daemon=True).start()
def own_window(pid):
    found=[]
    @callback
    def inspect(hwnd,_):
        owner=wintypes.DWORD();api.GetWindowThreadProcessId(hwnd,ctypes.byref(owner));title=ctypes.create_unicode_buffer(256)
        if owner.value==pid and api.IsWindowVisible(hwnd):
            api.GetWindowTextW(hwnd,title,256)
            if title.value.startswith('Reqon — Local API Client'):found.append(hwnd)
        return True
    api.EnumWindows(inspect,0);return found[0] if found else None
try:
    with tempfile.TemporaryDirectory(prefix='reqon-packaged-http-') as directory:
        workspace=Path(directory);(workspace/'upload.bin').write_bytes(b'packaged\x00\xfffixture')
        store=Store(workspace);GeneralPreferences(store).save(GeneralConfig(working_directory=directory,timeout_ms=3000,postman_token=False))
        cid=store.insert('collections',{'name':'Packaged HTTP fixture','kind':'http'})
        data=dict(name='Packaged JWT and multipart',method='POST',url=f'http://127.0.0.1:{server.server_port}/verify',auth_type='JWT Bearer',jwt_algorithm='RS256',jwt_payload='{"sub":"packaged-test"}',
            jwt_secret=key.private_bytes(serialization.Encoding.PEM,serialization.PrivateFormat.PKCS8,serialization.NoEncryption()).decode(),
            body_type='form-data',multipart_rows=[['file','upload.bin','',True,'File'],['name','frozen application','',True,'Text']])
        rid=store.save_request(data,collection_id=cid);uid=store.entity('request',rid)['uid'];session={'tabs':[{'kind':'request','uid':uid}],'active':0,'environment':None}
        store.db.execute("INSERT OR REPLACE INTO workspace_state(key,data) VALUES('session',?)",(json.dumps(session),));store.db.commit();store.db.close()
        for attempt in (1,2):
            arrived.clear();process=subprocess.Popen([str(exe),'--data-dir',directory],cwd=exe.parent)
            try:
                deadline=time.monotonic()+30;hwnd=None
                while time.monotonic()<deadline and not hwnd:
                    assert process.poll() is None,'Packaged application exited early';hwnd=own_window(process.pid);QTest.qWait(100)
                assert hwnd,'No packaged window';QTest.qWait(500)
                rect=wintypes.RECT();assert api.GetClientRect(hwnd,ctypes.byref(rect));scale=api.GetDpiForWindow(hwnd)/96
                point=(int(163*scale)<<16)|int(rect.right-100*scale)
                assert api.PostMessageW(hwnd,0x0201,1,point);assert api.PostMessageW(hwnd,0x0202,0,point)
                deadline=time.monotonic()+8
                while time.monotonic()<deadline and not arrived.is_set():QTest.qWait(100)
                output=root/'docs/ui/reqon-packaged-http.png';output.parent.mkdir(parents=True,exist_ok=True);QTest.qWait(300);app.primaryScreen().grabWindow(hwnd).save(str(output))
                assert arrived.is_set(),'The packaged EXE did not send the local HTTP request'
                headers,body=received[-1];lower={k.lower():v for k,v in headers.items()}
                assert jwt.decode(lower['authorization'].split(' ',1)[1],key.public_key(),algorithms=['RS256'])=={'sub':'packaged-test'}
                assert b'packaged\x00\xfffixture' in body and b'frozen application' in body
                assert 'boundary=' in lower['content-type'] and lower['user-agent'].startswith('Reqon/0.14.')
                assert api.PostMessageW(hwnd,0x0010,0,0);assert process.wait(15)==0
                with closing(sqlite3.connect(workspace/'reqon.sqlite3')) as db:
                    assert db.execute('PRAGMA integrity_check').fetchone()[0]=='ok'
                    assert db.execute('SELECT COUNT(*) FROM history WHERE status=200').fetchone()[0]==attempt
                    restored=json.loads(db.execute('SELECT data FROM requests WHERE uid=?',(uid,)).fetchone()[0]);assert restored['jwt_algorithm']=='RS256';assert restored['multipart_rows']==data['multipart_rows']
            finally:
                if process.poll() is None:process.terminate();process.wait(10)
        print(json.dumps({'executable':str(exe),'launches':2,'requests':len(received),'jwt':'RS256 verified','multipart':'binary bytes verified','restoration':'passed'}))
finally:server.shutdown();server.server_close()
