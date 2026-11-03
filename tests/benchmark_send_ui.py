"""Repeatable local response rendering measurements; no external endpoints."""
import argparse
import json
import os
from pathlib import Path
import statistics
import sys
import time
os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from PySide6.QtCore import QTimer
from PySide6.QtTest import QTest
from PySide6.QtWidgets import QApplication
from ui_pages import ResponsePanel
from ui_theme import STYLE


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--prepared', action='store_true')
    args = parser.parse_args()
    app = QApplication.instance() or QApplication([]); app.setStyleSheet(STYLE)
    panel = ResponsePanel(); panel.resize(1100, 600); panel.show()
    intervals = []; last = time.perf_counter()
    def tick():
        nonlocal last
        now = time.perf_counter(); intervals.append((now-last)*1000); last = now
    timer = QTimer(); timer.setInterval(10); timer.timeout.connect(tick); timer.start()
    results = []
    for size in (1024, 100*1024, 1024*1024, 10*1024*1024):
        item = dict(id=42, name='response item', values=[True, False, 12.5], text='x'*40)
        body = json.dumps([item]*(size//len(json.dumps(item))), ensure_ascii=False)
        response = dict(status=200, reason='OK', headers={'Content-Type':'application/json'},
            body=body, size=len(body), duration=1)
        if args.prepared:
            from response_format import prepare_response
            from general_settings import GeneralConfig
            prepare_response(response, False, GeneralConfig())
        QTest.qWait(60); intervals.clear(); last = time.perf_counter()
        start = time.perf_counter(); panel.show_response(response); render = (time.perf_counter()-start)*1000
        QTest.qWait(350)
        values = sorted(intervals)
        result = dict(bytes=len(body), render_ms=round(render,2),
            heartbeat_p50_ms=round(statistics.median(values),2),
            heartbeat_p95_ms=round(values[min(len(values)-1, int(len(values)*.95))],2),
            heartbeat_max_ms=round(max(values),2), full_copy=bool(panel._copy_text))
        results.append(result); print(json.dumps(result), flush=True)
    timer.stop(); panel.close()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(dict(prepared=args.prepared, results=results), indent=2)+'\n', encoding='utf-8')


if __name__ == '__main__': main()
