"""Process launch to first visible responsive window; OS disk cache is not cleared."""
import argparse
import ctypes
from ctypes import wintypes
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import psutil

sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from main import Store

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe',type=Path,default=Path('dist/Reqon/Reqon.exe'))
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--tabs',type=int,nargs='+',default=[1,20])
    parser.add_argument('--repeats',type=int,default=3)
    args=parser.parse_args();api=ctypes.windll.user32
    callback=ctypes.WINFUNCTYPE(wintypes.BOOL,wintypes.HWND,wintypes.LPARAM)
    api.EnumWindows.argtypes=[callback,wintypes.LPARAM]
    api.GetWindowThreadProcessId.argtypes=[wintypes.HWND,ctypes.POINTER(wintypes.DWORD)]
    api.GetWindowTextW.argtypes=[wintypes.HWND,wintypes.LPWSTR,ctypes.c_int]
    api.IsWindowVisible.argtypes=[wintypes.HWND]
    api.PostMessageW.argtypes=[wintypes.HWND,wintypes.UINT,wintypes.WPARAM,wintypes.LPARAM]
    api.SendMessageTimeoutW.argtypes=[wintypes.HWND,wintypes.UINT,wintypes.WPARAM,wintypes.LPARAM,wintypes.UINT,wintypes.UINT,ctypes.POINTER(ctypes.c_size_t)]
    result=dict(exe=str(args.exe.resolve()),conditions=__doc__,runs=[])
    body=json.dumps([{'name':'item','value':i} for i in range(4000)],indent=2)
    for count in args.tabs:
        with tempfile.TemporaryDirectory(prefix='reqon-launch-benchmark-') as directory:
            store=Store(directory);entries=[]
            for i in range(count):
                uid=f'tab-{i}';data=dict(name=uid,kind='grpc',message=body)
                store.db.execute('INSERT INTO local_drafts(uid,kind,name,data) VALUES(?,?,?,?)',(uid,'grpc',uid,json.dumps(data)))
                entries.append(dict(kind='draft',uid=uid))
            store.db.execute("INSERT OR REPLACE INTO workspace_state(key,data) VALUES('session',?)",(json.dumps(dict(tabs=entries,active=count-1)),))
            store.db.commit();store.db.close()
            for repeat in range(args.repeats):
                started=time.perf_counter();process=subprocess.Popen([str(args.exe.resolve()),'--data-dir',directory],cwd=args.exe.resolve().parent)
                found=[]
                @callback
                def inspect(hwnd,_):
                    owner=wintypes.DWORD();api.GetWindowThreadProcessId(hwnd,ctypes.byref(owner))
                    if owner.value==process.pid and api.IsWindowVisible(hwnd):
                        title=ctypes.create_unicode_buffer(512);api.GetWindowTextW(hwnd,title,512)
                        if title.value.startswith('Reqon — Local API Client'):found.append(hwnd)
                    return True
                try:
                    deadline=time.monotonic()+60
                    while not found and time.monotonic()<deadline:
                        assert process.poll() is None,process.returncode
                        api.EnumWindows(inspect,0);time.sleep(.01)
                    assert found,'No main window'
                    reply=ctypes.c_size_t()
                    while not api.SendMessageTimeoutW(found[0],0,0,0,2,250,ctypes.byref(reply)):
                        assert time.monotonic()<deadline,'Window did not respond'
                        assert process.poll() is None,process.returncode
                    row=dict(tabs=count,repeat=repeat,visible_responsive_ms=round((time.perf_counter()-started)*1000,2),rss_mib=round(psutil.Process(process.pid).memory_info().rss/1024**2,2))
                    result['runs'].append(row);print(json.dumps(row),flush=True)
                    assert api.PostMessageW(found[0],0x0010,0,0);assert process.wait(20)==0
                finally:
                    if process.poll() is None:process.terminate();process.wait(10)
                args.output.parent.mkdir(parents=True,exist_ok=True)
                args.output.write_text(json.dumps(result,indent=2),encoding='utf-8')

if __name__=='__main__':main()
