"""Reqon v2 transparent PNG states shared by the Windows and Linux UI."""
import json
import sys
from functools import lru_cache
from pathlib import Path
from PySide6.QtCore import Qt,QRectF,QSize,QCoreApplication
from PySide6.QtGui import QPainter,QPixmap
from PySide6.QtWidgets import QWidget,QVBoxLayout,QLabel,QSizePolicy
from ui_brand import asset_path


def state_root():return Path(getattr(sys,'_MEIPASS',Path(__file__).resolve().parent))/'reqon-ui-states v2'


# Keep existing UI state names and saved response examples compatible. Asset
# identifiers and paths come from the v2 manifest, captions remain native UI.
STATE_ASSETS={'activity-no-requests':'history-no-activity','response-no-content':'request-completed',
    'error-connection-refused':'error-service-unavailable','data-empty':'view-no-data','response-success':'request-completed'}
STATE_COPY={
    'response-ready-to-send':('Ready to send','Send a request to inspect the response.'),
    'request-create-new':('Create a request','Create a request to start working with your API.'),
    'activity-no-requests':('No activity','Run a request to see activity here.'),
    'response-no-content':('No response body','The request completed without a response body.'),
    'collections-empty':('No collections','Create a collection to organize your requests.'),
    'search-no-results':('No results','Try another search query.'),
}


@lru_cache(maxsize=1)
def state_manifest():return {item['id']:item for item in json.loads((state_root()/'manifest.json').read_text(encoding='utf-8'))['items']}


class StateIllustration(QWidget):
    def __init__(self,state='response-ready-to-send',size=128):
        super().__init__();self.setFixedSize(size,size);self.setFocusPolicy(Qt.FocusPolicy.NoFocus)
        self.pixmap=QPixmap();self.set_state(state)
    def set_state(self,state):
        if state is None:
            self.state=None;self.asset_id=None;self.pixmap=QPixmap(str(asset_path('png/transparent/reqon-512.png')));self.update();return
        self.asset_id=STATE_ASSETS.get(state,state)
        if self.asset_id not in state_manifest():raise ValueError('Unknown UI state: '+state)
        self.state=state;self.pixmap=QPixmap(str(state_root()/state_manifest()[self.asset_id]['dark']))
        if self.pixmap.isNull():raise ValueError('Cannot load UI state: '+state)
        self.update()
    def paintEvent(self,event):
        painter=QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.SmoothPixmapTransform)
        size=self.pixmap.size().scaled(self.size(),Qt.AspectRatioMode.KeepAspectRatio)
        target=QRectF((self.width()-size.width())/2,(self.height()-size.height())/2,size.width(),size.height())
        painter.drawPixmap(target,self.pixmap,QRectF(self.pixmap.rect()))


class StateWidget(QWidget):
    def __init__(self,state='response-ready-to-send',parent=None):
        super().__init__(parent);self.setMinimumSize(0,0)
        self.setSizePolicy(QSizePolicy.Policy.Expanding,QSizePolicy.Policy.Ignored)
        self.content_layout=QVBoxLayout(self);self.content_layout.setContentsMargins(16,12,16,12);self.content_layout.setSpacing(8)
        self.content_layout.addStretch();self.illustration=StateIllustration(state)
        self.content_layout.addWidget(self.illustration,0,Qt.AlignmentFlag.AlignHCenter)
        self.title=QLabel();self.title.setObjectName('emptyTitle');self.title.setAlignment(Qt.AlignmentFlag.AlignCenter);self.title.setWordWrap(True)
        self.description=QLabel();self.description.setObjectName('emptyDescription');self.description.setAlignment(Qt.AlignmentFlag.AlignCenter);self.description.setWordWrap(True)
        for label in (self.title,self.description):
            label.setTextFormat(Qt.TextFormat.PlainText);label.setSizePolicy(QSizePolicy.Policy.Ignored,QSizePolicy.Policy.Preferred);self.content_layout.addWidget(label)
        self.content_layout.addStretch();self.set_state(state)
    def set_state(self,state):
        self.state=state;self.illustration.set_state(state);title,description=STATE_COPY[state]
        self.title.setText(QCoreApplication.translate('UIStates',title))
        self.description.setText(QCoreApplication.translate('UIStates',description))
    def minimumSizeHint(self):return QSize(0,72)
    def resizeEvent(self,event):
        super().resizeEvent(event);self.illustration.setVisible(self.height()>=240 and self.width()>=180)


class ResponseEmpty(StateWidget):
    def set_state(self,state):self._pending=False;super().set_state(state)
    def pending(self):
        self._pending=True;self.illustration.hide();self.title.setText('Sending request');self.description.setText('Waiting for the server response')
    def resizeEvent(self,event):
        super().resizeEvent(event)
        if self._pending:self.illustration.hide()
