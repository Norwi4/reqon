"""Repeatable local UI timings; never opens the user's workspace or sends requests.

python tests/benchmark_interface.py --output build/interface-after.json
Use the same Qt platform, counts and repeats for before/after comparisons.
"""
import argparse
from dataclasses import replace
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
from PySide6 import __version__ as qt_version
from PySide6.QtCore import QCoreApplication, QEvent
from PySide6.QtWidgets import QApplication
from main import MainWindow, Store


def measure(app, operation):
    start = time.perf_counter()
    operation()
    applied = time.perf_counter()
    app.processEvents()
    return {'call_ms': round((applied - start) * 1000, 2),
            'paint_ms': round((time.perf_counter() - start) * 1000, 2)}


def dispose(app, window, store):
    window.close()
    window.deleteLater()
    QCoreApplication.sendPostedEvents(None, QEvent.Type.DeferredDelete)
    app.processEvents()
    store.db.close()
    gc.collect()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--tabs', type=int, nargs='*', default=[1, 6])
    parser.add_argument('--requests', type=int, nargs='*', default=[100, 1000, 10000])
    parser.add_argument('--repeats', type=int, default=3)
    parser.add_argument('--full-rebuild',action='store_true',help='Use the previous complete tree rebuild for comparison')
    parser.add_argument('--editor-items', type=int, default=0,
                        help='Fill each gRPC editor with this many JSON objects')
    args = parser.parse_args()
    app = QApplication([])
    app.setStyle('Fusion')
    result = dict(platform=platform.platform(), python=platform.python_version(),
                  qt=qt_version, qt_platform=app.platformName(), repeats=args.repeats,
                  editor_items=args.editor_items,
                  full_rebuild=args.full_rebuild,
                  themes=[], collections=[])

    def save():
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(result, indent=2), encoding='utf-8')

    for count in args.tabs:
        with tempfile.TemporaryDirectory(prefix='reqon-ui-benchmark-') as directory:
            store = Store(directory)
            start = time.perf_counter()
            window = MainWindow(store)
            for index in range(count - 1):
                window.new_grpc_request({'name': f'Tab {index}', 'kind': 'grpc'})
            if args.editor_items:
                from PySide6.QtTest import QTest
                body=json.dumps([{'name':'item','value':i} for i in range(args.editor_items)],indent=2)
                for index in range(window.workbench.count()):window.workbench.widget(index).message.setPlainText(body)
                QTest.qWait(400)
            window.show()
            window.show_settings()
            window.settings_overlay.navigation['Themes'].click()
            app.processEvents()
            setup = (time.perf_counter() - start) * 1000
            samples = []
            for _ in range(args.repeats):
                for theme in ('light', 'dracula', 'dark'):
                    samples.append(dict(theme=theme, **measure(app, lambda: window.theme_controller.set_config(
                        replace(window.theme_controller.config, manual=theme)))))
            item = dict(tabs=count, widgets=len(app.allWidgets()), setup_ms=round(setup, 2),
                        samples=samples, median_paint_ms=statistics.median(s['paint_ms'] for s in samples),
                        max_paint_ms=max(s['paint_ms'] for s in samples))
            result['themes'].append(item)
            save()
            print(json.dumps(item), flush=True)
            dispose(app, window, store)

    for count in args.requests:
        with tempfile.TemporaryDirectory(prefix='reqon-tree-benchmark-') as directory:
            store = Store(directory)
            cid = store.insert('collections', {'name': 'Benchmark', 'kind': 'http', 'expanded': 1})
            store.db.executemany('INSERT INTO requests(collection_id,name,data,uid,sort_order) VALUES(?,?,?,?,?)',
                [(cid, f'Request {i:05}', json.dumps({'name': f'Request {i:05}', 'kind': 'http',
                    'method': 'GET', 'url': 'http://127.0.0.1/unused'}), f'benchmark-{i}', i)
                 for i in range(count)])
            store.db.commit()
            start = time.perf_counter()
            window = MainWindow(store)
            if args.full_rebuild:window.update_tree_values=lambda snapshot:False
            window.show()
            app.processEvents()
            startup = (time.perf_counter() - start) * 1000
            queries = []
            store.db.set_trace_callback(queries.append)
            refresh = measure(app, window.refresh_tree)
            refresh_queries = len(queries)
            queries.clear()
            search = measure(app, lambda: window.search.setText('Request 000'))
            clear = measure(app, lambda: window.search.clear())
            search_queries = len(queries)
            store.db.set_trace_callback(None)
            item = dict(requests=count, startup_ms=round(startup, 2), refresh=refresh, search=search,
                        clear=clear, refresh_sql=refresh_queries, search_sql=search_queries)
            result['collections'].append(item)
            save()
            print(json.dumps(item), flush=True)
            dispose(app, window, store)
    return 0


if __name__ == '__main__':
    sys.exit(main())
