"""Actual Qt window/QProcess integration, with local regression packages only."""
import json
import os
from pathlib import Path
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
sys.path.insert(0, str(ROOT / 'prototypes/rust-runner/tests'))
from PySide6.QtCore import QTimer
from PySide6.QtTest import QTest
from PySide6.QtWidgets import QApplication
from fixture import WorkflowServer
from test_e2e import BINARY
from runner_controller import RunnerController, ReportReader, RUNNER_VERSION
from main import MainWindow, Store


class RunnerIntegration(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='reqon-qt-runner-')
        self.root = Path(self.temp.name)
        self.controller = RunnerController(self.root / 'artifacts', binary=str(BINARY))
        self.results = []
        self.events = []
        self.controller.finished.connect(self.results.append)
        self.controller.event.connect(self.events.append)

    def tearDown(self):
        if self.controller.running:
            self.controller.cancel()
            self.wait(lambda: not self.controller.running)
        self.controller.deleteLater()
        self.app.processEvents()
        self.temp.cleanup()

    def wait(self, predicate, timeout=15):
        deadline = time.monotonic() + timeout
        while not predicate() and time.monotonic() < deadline:
            QTest.qWait(5)
        self.assertTrue(predicate(), 'Qt operation did not complete before deadline')

    def package(self, steps):
        case = {'schema_version': 'reqon-case/v1', 'id': 'ui-case', 'name': 'UI case', 'steps': steps}
        catalog = {'schema_version': 'reqon-package/v1', 'id': 'ui-package', 'name': 'UI package',
                   'requires': {'engine': 'reqon', 'version': '=' + RUNNER_VERSION, 'capabilities': ['pause', 'grpc.unary']},
                   'cases': ['case.json']}
        (self.root / 'case.json').write_text(json.dumps(case), encoding='utf8')
        path = self.root / 'catalog.json'
        path.write_text(json.dumps(catalog), encoding='utf8')
        return path

    def test_real_process_progress_and_cancel_then_restart(self):
        package = self.package([{'type': 'pause', 'id': 'waiting', 'duration_ms': 10000}])
        self.controller.start(package, endpoint='http://127.0.0.1:1')
        self.wait(lambda: any(e.get('type') == 'step_started' for e in self.events))
        self.controller.cancel(); self.controller.cancel()
        self.wait(lambda: bool(self.results))
        self.assertEqual(self.results[-1]['outcome'], 'cancelled', self.results)
        self.assertEqual(self.results[-1]['exit_code'], 130)
        directory = Path(self.results[-1]['directory'])
        self.assertTrue((directory / 'junit.xml').is_file())
        package = self.package([{'type': 'pause', 'id': 'done', 'duration_ms': 1}])
        self.controller.start(package, endpoint='http://127.0.0.1:1')
        self.wait(lambda: len(self.results) == 2)
        self.assertEqual(self.results[-1]['outcome'], 'passed', self.results)
        self.assertNotEqual(self.results[-1]['directory'], str(directory))

    def test_main_window_remains_responsive_and_log_is_bounded(self):
        package = self.package([{'type': 'pause', 'id': f'step-{i}', 'duration_ms': 1} for i in range(1000)])
        store = Store(str(self.root / 'workspace'))
        with patch.dict(os.environ, {'REQON_RUNNER_BINARY': str(BINARY)}):
            window = MainWindow(store); window.show(); window.show_runner()
            dialog = window.runner_dialog
            dialog.package.setText(str(package)); dialog.endpoint.setText('http://127.0.0.1:1')
            ticks = []
            timer = QTimer(); timer.timeout.connect(lambda: ticks.append(time.monotonic())); timer.start(5)
            try:
                dialog.run_button.click()
                self.wait(lambda: not dialog.controller.running, timeout=20)
                self.assertTrue(dialog.status.text().startswith('PASSED'), dialog.status.text())
                self.assertLessEqual(dialog.log.document().blockCount(), 1000)
                self.assertGreater(len(ticks), 20)
                gaps = [b - a for a, b in zip(ticks, ticks[1:])]
                self.assertLess(max(gaps), .3, f'UI heartbeat blocked: {max(gaps):.3f}s')
                print(f'Qt 1000 steps: max heartbeat gap {max(gaps)*1000:.1f} ms, {len(ticks)} ticks')
                self.assertTrue(dialog.open_report.isEnabled())
                package = self.package([{'type': 'pause', 'id': 'close-cancel', 'duration_ms': 10000}])
                dialog.run_button.click()
                self.wait(lambda: dialog.controller.phase == 'running')
                window.close()
                self.wait(lambda: not dialog.controller.running and not window.isVisible())
            finally:
                timer.stop()
                if dialog.controller.running:
                    dialog.controller.cancel(); self.wait(lambda: not dialog.controller.running)
                dialog.close(); window.close(); window.deleteLater(); self.app.processEvents()
                store.db.close()

    def test_real_grpc_verdict_and_worker_crash(self):
        server = WorkflowServer()
        try:
            package = self.package([{'type': 'grpc', 'id': 'echo', 'method': 'poc.Workflow/Echo',
                                     'expect': [{'path': '/count', 'equals': 99}]}])
            self.controller.start(package, endpoint=server.endpoint)
            self.wait(lambda: bool(self.results))
            self.assertEqual(self.results[-1]['outcome'], 'failed')
            package = self.package([{'type': 'pause', 'id': 'crash', 'duration_ms': 10000}])
            self.controller.start(package, endpoint=server.endpoint)
            self.wait(lambda: self.controller.phase == 'running')
            self.controller.process.kill()
            self.wait(lambda: len(self.results) == 2)
            self.assertEqual(self.results[-1]['outcome'], 'error')
        finally:
            server.close()

    def test_version_mismatch_is_rejected_before_execution(self):
        self.controller.expected_version = '0.0.1'
        package = self.package([{'type': 'pause', 'id': 'unused', 'duration_ms': 10000}])
        self.controller.start(package, endpoint='http://127.0.0.1:1')
        self.wait(lambda: bool(self.results))
        self.assertEqual(self.results[0]['outcome'], 'error')
        self.assertFalse(self.events)

    def test_large_artifact_read_and_manifest_mismatch(self):
        directory = self.root / 'large'; directory.mkdir()
        manifest = {'state': 'complete', 'run_id': 'same', 'exit_code': 0, 'outcome': 'passed'}
        report = {'run_id': 'same', 'engine_version': RUNNER_VERSION, 'outcome': 'passed',
                  'cases': [{'outcome': 'passed', 'response': 'x' * (24 * 1024 * 1024)}]}
        (directory / 'result.manifest.json').write_text(json.dumps(manifest))
        (directory / 'result.json').write_text(json.dumps(report))
        reader = ReportReader(directory, 0, RUNNER_VERSION)
        ticks = []; timer = QTimer(); timer.timeout.connect(lambda: ticks.append(time.monotonic())); timer.start(5)
        reader.start()
        try:
            self.wait(lambda: reader.isFinished())
            self.assertEqual(reader.result['outcome'], 'passed')
            self.assertLess(len(json.dumps(reader.result)), 1024)
            self.assertTrue(ticks)
            manifest['run_id'] = 'wrong'
            (directory / 'result.manifest.json').write_text(json.dumps(manifest))
            reader.start(); self.wait(lambda: reader.isFinished())
            self.assertEqual(reader.result['outcome'], 'error')
        finally:
            timer.stop(); reader.wait(5000); reader.deleteLater()


if __name__ == '__main__':
    unittest.main()
