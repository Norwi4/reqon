"""Regression dialog using existing Qt controls and application theme."""
from pathlib import Path
from PySide6.QtCore import QUrl, Qt
from PySide6.QtGui import QDesktopServices
from PySide6.QtWidgets import (QDialog, QVBoxLayout, QHBoxLayout, QFormLayout, QLineEdit,
                              QPushButton, QLabel, QPlainTextEdit, QFileDialog, QProgressBar)
from runner_controller import RunnerController


class RunnerDialog(QDialog):
    def __init__(self, window):
        super().__init__(window)
        self.setWindowTitle('Reqon — Regression Runner')
        self.resize(780, 540)
        self.controller = RunnerController(window.store.path.parent / 'runner-artifacts', self)
        self.controller.event.connect(self.on_event)
        self.controller.finished.connect(self.on_finished)
        self.controller.state_changed.connect(self.on_state)
        self.close_pending = False
        self.directory = None
        outer = QVBoxLayout(self)
        form = QFormLayout()
        self.package = QLineEdit(); self.package.setPlaceholderText('catalog.json or Java scenario.yaml')
        self.environment = QLineEdit(); self.environment.setPlaceholderText('Optional environment.json')
        self.endpoint = QLineEdit(); self.endpoint.setPlaceholderText('Endpoint, if not supplied by the environment')
        self.suite = QLineEdit(); self.suite.setPlaceholderText('Default suite')
        for label, field in [('Package / scenario', self.package), ('Environment', self.environment)]:
            row = QHBoxLayout(); row.addWidget(field)
            choose = QPushButton('Browse…'); choose.clicked.connect(lambda checked=False, field=field: self.choose(field))
            row.addWidget(choose); form.addRow(label, row)
        form.addRow('Endpoint', self.endpoint); form.addRow('Suite', self.suite)
        outer.addLayout(form)
        self.status = QLabel('Select a package or scenario to run.'); self.status.setWordWrap(True)
        self.status.setTextFormat(Qt.TextFormat.PlainText); outer.addWidget(self.status)
        self.progress = QProgressBar(); self.progress.setRange(0, 1); self.progress.setValue(0); outer.addWidget(self.progress)
        self.log = QPlainTextEdit(); self.log.setReadOnly(True); self.log.setMaximumBlockCount(1000); outer.addWidget(self.log, 1)
        row = QHBoxLayout()
        self.open_report = QPushButton('Open HTML report'); self.open_report.setEnabled(False)
        self.open_report.clicked.connect(self.show_report); row.addWidget(self.open_report)
        self.open_folder = QPushButton('Open artifacts'); self.open_folder.setEnabled(False)
        self.open_folder.clicked.connect(self.show_folder); row.addWidget(self.open_folder)
        row.addStretch()
        self.run_button = QPushButton('Run'); self.run_button.clicked.connect(self.run); row.addWidget(self.run_button)
        self.cancel_button = QPushButton('Cancel run'); self.cancel_button.setEnabled(False)
        self.cancel_button.clicked.connect(self.controller.cancel); row.addWidget(self.cancel_button)
        outer.addLayout(row)

    def choose(self, field):
        path, _ = QFileDialog.getOpenFileName(self, 'Select file', field.text(), 'Regression files (*.json *.yaml *.yml);;All files (*)')
        if path:
            field.setText(path)

    def run(self):
        self.log.clear(); self.open_report.setEnabled(False); self.open_folder.setEnabled(False)
        try:
            self.controller.start(self.package.text().strip(), self.environment.text().strip(),
                                  self.endpoint.text().strip(), self.suite.text().strip())
        except Exception as error:
            self.status.setText(str(error))

    def on_state(self, state):
        running = self.controller.running
        self.run_button.setEnabled(not running); self.cancel_button.setEnabled(running and state != 'reading')
        for field in (self.package, self.environment, self.endpoint, self.suite):
            field.setEnabled(not running)
        self.progress.setRange(0, 0 if running else 1)
        self.status.setText({'checking': 'Checking Runner version…', 'running': 'Regression running…',
                             'cancelling': 'Cancelling…', 'reading': 'Reading completed report…'}.get(state, 'Ready'))

    def on_event(self, event):
        if event.get('type') in ('case_started', 'step_started', 'step_finished', 'case_finished', 'kafka_ready'):
            name = str(event.get('step_id') or event.get('case_id') or event.get('scenario_id') or '')[:160]
            self.log.appendPlainText(f"{event['type']}  {name}  {event.get('outcome', '')}")

    def on_finished(self, result):
        self.directory = Path(result['directory'])
        self.open_folder.setEnabled(True); self.open_report.setEnabled((self.directory / 'report.html').is_file())
        self.progress.setValue(1)
        counts = result.get('counts', {})
        summary = ', '.join(f'{key}: {value}' for key, value in counts.items() if value)
        self.status.setText(result.get('error') or f"{result['outcome'].upper()} — {summary}")
        if self.close_pending:
            self.close()

    def show_report(self):
        if self.directory:
            QDesktopServices.openUrl(QUrl.fromLocalFile(str(self.directory / 'report.html')))

    def show_folder(self):
        if self.directory:
            QDesktopServices.openUrl(QUrl.fromLocalFile(str(self.directory)))

    def closeEvent(self, event):
        if self.controller.running:
            self.close_pending = True
            self.controller.cancel(); self.hide(); event.ignore()
        else:
            self.close_pending = False
            event.accept()
