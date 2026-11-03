"""Isolated startup, tab, input, scroll, resize, heartbeat and RSS measurements."""
import argparse
import gc
import json
import os
from pathlib import Path
import platform
import statistics
import sys
import tempfile
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')
import psutil
from PySide6 import __version__ as qt_version
from PySide6.QtCore import QCoreApplication, QEvent, QTimer, Qt
from PySide6.QtGui import QTextCursor
from PySide6.QtTest import QTest
from PySide6.QtWidgets import QApplication
from main import MainWindow, Store


def summary(samples):
    ordered = sorted(samples)
    return dict(p50=round(statistics.median(ordered), 2),
                p95=round(ordered[min(len(ordered)-1, int(len(ordered)*.95))], 2),
                max=round(max(ordered), 2), samples=len(samples))


def seed(store, tabs, body):
    entries=[]
    for i in range(tabs):
        data=dict(name=f'Tab {i}', kind='grpc', message=body)
        uid=f'benchmark-{i}'
        store.db.execute('INSERT INTO local_drafts(uid,kind,name,data) VALUES(?,?,?,?)',
                         (uid,'grpc',data['name'],json.dumps(data)))
        entries.append(dict(kind='draft',uid=uid))
    store.db.execute("INSERT OR REPLACE INTO workspace_state(key,data) VALUES('session',?)",
                     (json.dumps(dict(tabs=entries,active=tabs-1)),))
    store.db.commit()


def run(app, count, body):
    with tempfile.TemporaryDirectory(prefix='reqon-workspace-benchmark-') as directory:
        store=Store(directory)
        seed(store,count,body)
        process=psutil.Process()
        rss_before=process.memory_info().rss
        start=time.perf_counter()
        window=MainWindow(store)
        constructed=time.perf_counter()
        window.show();app.processEvents()
        ready=time.perf_counter()
        result=dict(tabs=count, body_bytes=len(body.encode()),
                    construct_ms=round((constructed-start)*1000,2),
                    first_paint_ms=round((ready-start)*1000,2),
                    rss_ready_mib=round(process.memory_info().rss/1024**2,2),
                    rss_delta_mib=round((process.memory_info().rss-rss_before)/1024**2,2))
        beats=[];last=time.perf_counter()
        def beat():
            nonlocal last
            now=time.perf_counter();beats.append((now-last)*1000);last=now
        timer=QTimer();timer.setInterval(10);timer.timeout.connect(beat);timer.start()
        QTest.qWait(400)
        def measure(operation):
            begin=time.perf_counter();operation();app.processEvents()
            return (time.perf_counter()-begin)*1000
        result['open_ms']=summary([measure(lambda:window.new_grpc_request({'kind':'grpc','name':'New'})) for _ in range(3)])
        result['switch_ms']=summary([measure(lambda i=i:window.workbench.setCurrentIndex(i%count)) for i in range(max(20,count))])
        result['warm_switch_ms']=summary([measure(lambda i=i:window.workbench.setCurrentIndex(i%count)) for i in range(max(20,count))])
        window.workbench.setCurrentIndex(count-1);app.processEvents()
        editor=window.current_page().message
        editor.setFocus();app.processEvents()
        result['input_editor_focused']=editor.hasFocus()
        editor.moveCursor(QTextCursor.MoveOperation.End)
        result['input_ms']=summary([measure(lambda:QTest.keyClick(editor,Qt.Key.Key_Space)) for _ in range(30)])
        QTest.qWait(400)
        scroll=editor.verticalScrollBar()
        result['scroll_ms']=summary([measure(lambda i=i:scroll.setValue(int(scroll.maximum()*(i%10)/9))) for i in range(30)])
        result['resize_ms']=summary([measure(lambda i=i:window.resize(1000+(i%2)*120,750+(i%2)*60)) for i in range(20)])
        QTest.qWait(300);timer.stop()
        result['heartbeat_ms']=summary(beats)
        result['rss_exercised_mib']=round(process.memory_info().rss/1024**2,2)
        result['close_ms']=measure(window.close)
        window.deleteLater();QCoreApplication.sendPostedEvents(None,QEvent.Type.DeferredDelete)
        app.processEvents();store.db.close();gc.collect()
        result['rss_after_dispose_mib']=round(process.memory_info().rss/1024**2,2)
        return result


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--tabs',type=int,nargs='+',default=[1,6,20])
    parser.add_argument('--items',type=int,default=4000)
    parser.add_argument('--repeats',type=int,default=3)
    args=parser.parse_args()
    app=QApplication([]);app.setStyle('Fusion')
    body=json.dumps([{'name':'item','value':i} for i in range(args.items)],indent=2)
    result=dict(platform=platform.platform(),python=platform.python_version(),qt=qt_version,
                qt_platform=app.platformName(),repeats=args.repeats,items=args.items,
                conditions='Fresh temporary workspace; warm process imports; no network; 10 ms heartbeat; RSS includes allocator retention.',runs=[])
    for count in args.tabs:
        for repeat in range(args.repeats):
            row=dict(repeat=repeat,**run(app,count,body));result['runs'].append(row)
            args.output.parent.mkdir(parents=True,exist_ok=True)
            args.output.write_text(json.dumps(result,indent=2),encoding='utf-8')
            print(json.dumps(row),flush=True)


if __name__=='__main__':main()
