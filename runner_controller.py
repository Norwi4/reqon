"""Asynchronous Qt adapter for the same standalone Reqon Runner used by CI."""
import json
import os
from pathlib import Path
import shutil
import sys
import time
import uuid
from PySide6.QtCore import QObject, QProcess, QThread, QTimer, Signal

RUNNER_VERSION = '0.14.1'


def runner_binary():
    explicit = os.environ.get('REQON_RUNNER_BINARY')
    if explicit:
        return str(Path(explicit).resolve())
    name = 'reqon.exe' if os.name == 'nt' else 'reqon'
    root = Path(getattr(sys, '_MEIPASS', Path(__file__).resolve().parent))
    candidates = [root / 'runner' / name, root / 'prototypes/rust-runner/dist' / name]
    for candidate in candidates:
        if candidate.is_file():
            return str(candidate)
    found = shutil.which('reqon')
    if found:
        return found
    raise FileNotFoundError('Reqon CLI is not installed. Install the matching CLI or set REQON_RUNNER_BINARY.')


def read_json(path, limit):
    with Path(path).open('rb') as stream:
        data = stream.read(limit + 1)
    if len(data) > limit:
        raise ValueError('Runner artifact exceeds its size limit')
    return json.loads(data)


class ReportReader(QThread):
    def __init__(self, directory, exit_code, version, parent=None):
        super().__init__(parent)
        self.directory, self.exit_code, self.version = directory, exit_code, version
        self.result = None

    def run(self):
        try:
            manifest = read_json(self.directory / 'result.manifest.json', 1024 * 1024)
            report = read_json(self.directory / 'result.json', 64 * 1024 * 1024)
            if (manifest.get('state') != 'complete' or not manifest.get('run_id')
                    or manifest['run_id'] != report.get('run_id')
                    or manifest.get('exit_code') != self.exit_code
                    or report.get('engine_version') != self.version
                    or manifest.get('outcome') != report.get('outcome')):
                raise ValueError('Runner artifacts do not describe one completed run')
            expected = {'passed': 0, 'failed': 1, 'error': 2, 'cancelled': 130}
            if expected.get(report.get('outcome')) != self.exit_code:
                raise ValueError('Runner exit code does not match the report')
            cases = report.get('cases', [])
            if not isinstance(cases, list) or not cases:
                raise ValueError('Runner report has no cases')
            counts = {key: 0 for key in ('passed', 'failed', 'error', 'cancelled', 'skipped')}
            for case in cases:
                key = case.get('outcome', 'error')
                counts[key] = counts.get(key, 0) + 1
            if report['outcome'] == 'passed' and not counts['passed']:
                raise ValueError('Runner reported success without passed cases')
            # Return only a small summary to the UI; large response bodies stay in artifacts.
            self.result = {'outcome': report['outcome'], 'exit_code': self.exit_code,
                           'run_id': report['run_id'], 'counts': counts,
                           'duration_ms': report.get('duration_ms', 0), 'directory': str(self.directory)}
        except Exception as error:
            self.result = {'outcome': 'error', 'error': str(error), 'exit_code': self.exit_code,
                           'directory': str(self.directory)}


class RunnerController(QObject):
    event = Signal(object)
    finished = Signal(object)
    state_changed = Signal(str)

    def __init__(self, artifacts_root, parent=None, binary=None, expected_version=RUNNER_VERSION):
        super().__init__(parent)
        self.artifacts_root = Path(artifacts_root)
        self.binary, self.expected_version = binary, expected_version
        self.process = None
        self.reader = None
        self.running = False
        self.phase = 'idle'
        self.pending = bytearray()
        self.stderr = bytearray()
        self.cancel_requested = False
        self.drain_timer = QTimer(self)
        self.drain_timer.setSingleShot(True)
        self.drain_timer.timeout.connect(self._drain)
        self.kill_timer = QTimer(self)
        self.kill_timer.setSingleShot(True)
        self.kill_timer.timeout.connect(self._kill)
        self.probe_timer = QTimer(self)
        self.probe_timer.setSingleShot(True)
        self.probe_timer.timeout.connect(lambda: self._fail('Runner version check timed out'))
        self.pending_exit = None
        self.error_timer = QTimer(self)
        self.error_timer.setSingleShot(True)
        self.error_timer.timeout.connect(self._drain_error)

    def start(self, package, environment='', endpoint='', suite=''):
        if self.running:
            raise RuntimeError('A regression is already running')
        if not package:
            raise ValueError('Select a package or scenario first')
        source = Path(package).expanduser().resolve(strict=True)
        if not source.is_file():
            raise ValueError('Select a package catalog or scenario file')
        self.directory = self.artifacts_root / uuid.uuid4().hex
        self.directory.mkdir(parents=True, mode=0o700)
        self.command = ['run', str(source), '--control-stdin', '--json', str(self.directory / 'result.json'),
                        '--junit', str(self.directory / 'junit.xml'), '--html', str(self.directory / 'report.html')]
        if environment:
            self.command += ['--env', str(Path(environment).expanduser().resolve(strict=True))]
        if endpoint:
            self.command += ['--endpoint', endpoint]
        if suite:
            self.command += ['--suite', suite]
        self.executable = self.binary or runner_binary()
        self.running = True
        self.cancel_requested = False
        self.pending_exit = None
        self.failure = None
        self._launch(['--version'], 'checking')
        self.probe_timer.start(5000)

    def _launch(self, arguments, phase):
        if self.process is not None:
            self.process.deleteLater()
        self.pending.clear(); self.stderr.clear()
        self.phase = phase
        self.process = QProcess(self)
        self.process.setProgram(self.executable)
        self.process.setArguments(arguments)
        self.process.readyReadStandardOutput.connect(self._drain)
        self.process.readyReadStandardError.connect(self._drain_error)
        self.process.errorOccurred.connect(self._process_error)
        self.process.finished.connect(self._process_finished)
        self.state_changed.emit(phase)
        self.process.start()

    def _process_error(self, error):
        if error == QProcess.ProcessError.FailedToStart:
            self._fail('Cannot start Reqon CLI: ' + self.process.errorString())

    def _drain_error(self):
        if self.process is None:
            return
        self.process.setReadChannel(QProcess.ProcessChannel.StandardError)
        if self.process.bytesAvailable() > 2 * 1024 * 1024:
            self.process.setReadChannel(QProcess.ProcessChannel.StandardOutput)
            self._fail('Runner diagnostic output exceeded its buffer limit')
            return
        data = bytes(self.process.read(65536))
        if len(self.stderr) < 65536:
            self.stderr.extend(data[:65536 - len(self.stderr)])
        if self.process.bytesAvailable():
            self.error_timer.start(0)
        self.process.setReadChannel(QProcess.ProcessChannel.StandardOutput)

    def _drain(self):
        if not self.running or self.process is None:
            return
        self.process.setReadChannel(QProcess.ProcessChannel.StandardOutput)
        if self.process.bytesAvailable() > 2 * 1024 * 1024:
            self._fail('Runner progress output exceeded its buffer limit')
            return
        self.pending.extend(bytes(self.process.read(65536)))
        if self.phase == 'checking':
            if len(self.pending) > 8192:
                self._fail('Invalid Runner version response')
            return
        started = time.monotonic()
        handled = 0
        while b'\n' in self.pending and handled < 64 and time.monotonic() - started < .005:
            line, _, tail = self.pending.partition(b'\n')
            self.pending = bytearray(tail)
            handled += 1
            if len(line) > 65536:
                self._fail('Runner progress line exceeded its limit'); return
            try:
                value = json.loads(line)
                if isinstance(value, dict):
                    self.event.emit(value)
            except (ValueError, UnicodeError):
                self._fail('Invalid Runner progress message'); return
        if len(self.pending) > 131072:
            self._fail('Runner progress buffer exceeded its limit'); return
        if self.process.bytesAvailable() or b'\n' in self.pending:
            self.drain_timer.start(0)
        elif self.pending_exit is not None:
            self._read_report()

    def _process_finished(self, code, status):
        self.probe_timer.stop(); self.kill_timer.stop()
        if not self.running:
            return
        self._drain_error()
        if not self.running:
            return
        if self.failure:
            self._complete({'outcome': 'error', 'error': self.failure, 'directory': str(self.directory)})
        elif self.phase == 'checking':
            self._drain()
            if not self.running or self.failure:
                return
            actual = self.pending.decode('utf-8', errors='replace').strip()
            if code != 0 or actual != 'reqon-runner-prototype ' + self.expected_version:
                self._fail(f'Reqon CLI {self.expected_version} is required. Install the matching CLI build.')
            elif self.cancel_requested:
                self._complete({'outcome': 'cancelled', 'exit_code': 130, 'directory': str(self.directory)})
            else:
                self._launch(self.command, 'running')
        else:
            self.pending_exit = code
            self._drain()

    def _read_report(self):
        if self.reader is not None:
            return
        self.phase = 'reading'
        self.state_changed.emit(self.phase)
        self.reader = ReportReader(self.directory, self.pending_exit, self.expected_version, self)
        self.reader.finished.connect(self._report_ready)
        self.reader.start()

    def _report_ready(self):
        result = self.reader.result
        self.reader.deleteLater(); self.reader = None
        self._complete(result)

    def cancel(self):
        if not self.running or self.phase == 'reading' or self.cancel_requested:
            return
        self.cancel_requested = True
        if self.phase == 'running':
            self.process.write(b'{"type":"cancel"}\n')
        self.state_changed.emit('cancelling')
        self.kill_timer.start(3000)

    def _kill(self):
        if self.process and self.process.state() != QProcess.ProcessState.NotRunning:
            self.failure = self.failure or 'Runner stopped after the cancellation deadline; partial artifacts were retained'
            self.process.kill()

    def _fail(self, message):
        self.failure = message
        self.probe_timer.stop()
        if self.process and self.process.state() != QProcess.ProcessState.NotRunning:
            self.process.kill()
        else:
            self._complete({'outcome': 'error', 'error': message, 'directory': str(self.directory)})

    def _complete(self, result):
        if not self.running:
            return
        self.running = False
        self.drain_timer.stop(); self.probe_timer.stop(); self.kill_timer.stop(); self.error_timer.stop()
        self.phase = 'idle'
        self.state_changed.emit('idle')
        self.finished.emit(result)
