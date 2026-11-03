"""Measure actual GUI Send with and without native channel/reflection reuse."""
import argparse
import json
import os
from pathlib import Path
import statistics
import sys
import tempfile
import time
from unittest.mock import patch

sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
os.environ.setdefault('QT_QPA_PLATFORM','offscreen')
from PySide6.QtWidgets import QApplication
from PySide6.QtTest import QTest
from PySide6.QtCore import QCoreApplication,QEvent
from main import MainWindow,Store
import native_request
from reflection_fixture import ReflectionServer


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args();app=QApplication([]);app.setStyle('Fusion');server=ReflectionServer('cachebench')
    result=dict(engine=native_request.RUNNER_VERSION,transport='loopback plaintext; actual GUI Send; persistent process',runs=[])
    original=native_request.RequestProcess.send
    try:
        for enabled in (False,True):
            native_request.shutdown_idle()
            with tempfile.TemporaryDirectory() as directory:
                store=Store(directory);window=MainWindow(store);window.show()
                page=window.current_page();page.server.setText(server.address);page.select_method('cachebench.Customers','GetCustomer');page.message.setPlainText('{"name":"cache benchmark"}')
                app.processEvents();before=len(server.calls);samples=[]
                def send(process,value):original(process,dict(value,grpc_cache=enabled))
                with patch.object(native_request.RequestProcess,'send',send):
                    for _ in range(25):
                        start=time.perf_counter();page.send_button.click()
                        deadline=time.monotonic()+8
                        while window.workers and time.monotonic()<deadline:QTest.qWait(1)
                        assert not window.workers
                        assert json.loads(page.response.editor.toPlainText())=={'name':'cache benchmark'}
                        samples.append((time.perf_counter()-start)*1000)
                row=dict(cache=enabled,reflection_calls=len(server.calls)-before,cold_ms=round(samples[0],2),warm_p50_ms=round(statistics.median(samples[1:]),2),warm_p95_ms=round(sorted(samples[1:])[22],2),samples_ms=[round(n,2) for n in samples])
                result['runs'].append(row);print(json.dumps(row),flush=True)
                window.close();window.deleteLater();QCoreApplication.sendPostedEvents(None,QEvent.Type.DeferredDelete);app.processEvents();store.db.close()
        args.output.write_text(json.dumps(result,indent=2),encoding='utf-8')
    finally:server.close();native_request.shutdown_idle()

if __name__=='__main__':main()
