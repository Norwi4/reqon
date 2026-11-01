"""Start and gracefully close the built EXE twice, using an isolated workspace.

Only windows belonging to the child process are inspected or closed. No
installed application, user workspace, registry entry or remote API is touched.
"""
import ctypes
import argparse
from ctypes import wintypes
from contextlib import closing
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import time

if sys.platform!='win32':raise SystemExit('Windows only')
from PySide6.QtGui import QGuiApplication
from PySide6.QtTest import QTest

root=Path(__file__).resolve().parents[1]
parser=argparse.ArgumentParser();parser.add_argument('executable',nargs='?',default=str(root/'dist/Reqon/Reqon.exe'))
parser.add_argument('--environment',action='store_true')
parser.add_argument('--legacy-workspace',action='store_true');options=parser.parse_args()
if options.legacy_workspace:options.environment=True
exe=Path(options.executable).resolve()
assert exe.is_file(),exe
api=ctypes.windll.user32
callback_type=ctypes.WINFUNCTYPE(wintypes.BOOL,wintypes.HWND,wintypes.LPARAM)
api.EnumWindows.argtypes=[callback_type,wintypes.LPARAM]
api.GetWindowThreadProcessId.argtypes=[wintypes.HWND,ctypes.POINTER(wintypes.DWORD)]
api.IsWindowVisible.argtypes=[wintypes.HWND]
api.GetWindowTextW.argtypes=[wintypes.HWND,wintypes.LPWSTR,ctypes.c_int]
api.PostMessageW.argtypes=[wintypes.HWND,wintypes.UINT,wintypes.WPARAM,wintypes.LPARAM]
app=QGuiApplication([])


def own_window(pid):
    matches=[]
    @callback_type
    def inspect(hwnd,_):
        owner=wintypes.DWORD();api.GetWindowThreadProcessId(hwnd,ctypes.byref(owner))
        if owner.value==pid and api.IsWindowVisible(hwnd):
            title=ctypes.create_unicode_buffer(512);api.GetWindowTextW(hwnd,title,512)
            # Qt may append applicationDisplayName to the native caption.
            if title.value.startswith('Reqon — Local API Client'):matches.append(hwnd)
        return True
    api.EnumWindows(inspect,0)
    return matches[0] if matches else None


with tempfile.TemporaryDirectory(prefix='reqon-packaged-smoke-') as workspace:
    workspace_path=Path(workspace)
    seed_directory=workspace_path/'Getman/Getman' if options.legacy_workspace else workspace_path
    database=workspace_path/'Reqon/reqon.sqlite3' if options.legacy_workspace else workspace_path/'reqon.sqlite3'
    if options.environment:
        sys.path.insert(0,str(root))
        from main import Store
        from environment_store import EnvironmentStore
        store=Store(seed_directory);environments=EnvironmentStore(store)
        selected=environments.create('Build smoke environment',[{'key':'base_url','value':'http://localhost:8080','enabled':True}],color='#b3261e')
        session={'tabs':[{'kind':'environment','uid':environments.get(selected)['uid']}],'active':0,'environment':selected}
        store.db.execute("INSERT OR REPLACE INTO workspace_state(key,data) VALUES('session',?)",(json.dumps(session),));store.db.commit();store.db.close()
    if options.legacy_workspace:
        legacy=seed_directory/'getman.sqlite3';(seed_directory/'reqon.sqlite3').rename(legacy);legacy_bytes=legacy.read_bytes()
    child_environment=os.environ.copy()
    if options.legacy_workspace:child_environment['APPDATA']=str(workspace_path)
    runs=[]
    for attempt in (1,2):
        arguments=[str(exe)] if options.legacy_workspace else [str(exe),'--data-dir',workspace]
        process=subprocess.Popen(arguments,cwd=exe.parent,env=child_environment)
        try:
            deadline=time.monotonic()+30;hwnd=None
            while time.monotonic()<deadline:
                assert process.poll() is None,f'EXE exited early: {process.returncode}'
                hwnd=own_window(process.pid)
                if hwnd:break
                QTest.qWait(100)
            assert hwnd,'Main window did not appear'
            QTest.qWait(750)
            if attempt==1:
                output=root/('docs/ui/reqon-windows-packaged-environment.png' if options.environment else 'docs/ui/reqon-windows-packaged.png');output.parent.mkdir(parents=True,exist_ok=True)
                pixmap=app.primaryScreen().grabWindow(hwnd)
                assert not pixmap.isNull() and pixmap.save(str(output)),'Could not capture the child window'
            assert api.PostMessageW(hwnd,0x0010,0,0),'WM_CLOSE failed'
            code=process.wait(timeout=15);assert code==0,f'EXE exit: {code}'
            assert database.is_file(),database
            if options.legacy_workspace:assert legacy.read_bytes()==legacy_bytes,'Legacy database changed'
            with closing(sqlite3.connect(database)) as db:
                assert db.execute('PRAGMA integrity_check').fetchone()[0]=='ok'
                session=json.loads(db.execute("SELECT data FROM workspace_state WHERE key='session'").fetchone()[0])
                assert session['tabs'],'Workspace tabs were not saved'
                if options.environment:
                    assert session['tabs'][0]['kind']=='environment' and session['environment']==selected
                    assert db.execute('SELECT color FROM environment_details WHERE environment_id=?',(selected,)).fetchone()[0]=='#b3261e'
                assert db.execute('SELECT COUNT(*) FROM history').fetchone()[0]==0
            runs.append({'attempt':attempt,'exit_code':code,'saved_tabs':len(session['tabs'])})
        finally:
            if process.poll() is None:
                # Cleanup is restricted to this test's own child process.
                process.terminate();process.wait(timeout=10)
    print(json.dumps({'executable':str(exe),'runs':runs,'isolated_workspace':True,'environment':options.environment,'legacy_migration':options.legacy_workspace}))
