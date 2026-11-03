"""Keep Python cycle finalizers on Qt's GUI thread, including during TLS work.

Reference counting remains enabled. Only automatic cyclic collection is moved
to a GUI-owned timer, because unreachable PySide wrappers may own GUI objects.
"""
import gc
import time
from PySide6.QtCore import QObject,QTimer,QThread
from PySide6.QtWidgets import QApplication


class GuiCollector(QObject):
    def __init__(self,app):
        super().__init__(app)
        self.last_full=time.monotonic()
        self.enabled=gc.isenabled()
        gc.disable()
        self.timer=QTimer(self);self.timer.setInterval(1000)
        self.timer.timeout.connect(self.collect_if_needed);self.timer.start()

    def collect_if_needed(self):
        if not self.enabled:return
        counts=gc.get_count();threshold=gc.get_threshold()
        full_due=time.monotonic()-self.last_full>=15
        if counts[0]<threshold[0] and not full_due:return
        # Frequent young collections avoid accumulating editor signal cycles.
        # Full collection is bounded in frequency, not moved into a worker.
        if full_due:
            gc.collect(2);self.last_full=time.monotonic()
        else:gc.collect(1)


def install_gui_collector():
    app=QApplication.instance()
    if app is None or QThread.currentThread()!=app.thread():
        raise RuntimeError('GUI lifetime management must start on the application thread')
    if not hasattr(app,'reqon_collector'):app.reqon_collector=GuiCollector(app)
