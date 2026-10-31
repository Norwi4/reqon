"""Native response status and illustrated error states. No HTML/browser surface."""
import math
from PySide6.QtCore import Qt,QSize,QRectF,QByteArray
from PySide6.QtGui import QColor,QPainter,QTextOption,QFontMetricsF
from PySide6.QtSvg import QSvgRenderer
from PySide6.QtWidgets import (QWidget,QLabel,QVBoxLayout,QScrollArea,QTextEdit,QSizePolicy,QFrame)


class StatusBadge(QLabel):
    PALETTE={'error':('#67220e','#ffb39d'),'success':('#183e2d','#9bdfb1'),
        'warning':('#49381e','#edc584'),'neutral':('#343434','#bdbdbd')}
    def __init__(self):
        super().__init__('');self.tone='neutral';self.setFixedHeight(20);self.setMaximumWidth(260)
        self.setStyleSheet('font-size:12px;')
        self.setSizePolicy(QSizePolicy.Policy.Preferred,QSizePolicy.Policy.Fixed)
        self.setTextFormat(Qt.TextFormat.PlainText)
    def setText(self,value):
        super().setText(value);self.tone='neutral';self.setToolTip(value);self.setVisible(bool(value));self.updateGeometry();self.update()
    def set_tone(self,tone):self.tone=tone;self.update()
    def sizeHint(self):
        # Integer font metrics round down on some Linux fallback fonts; that
        # fraction of a pixel is enough for elidedText to remove the last word.
        width=math.ceil(QFontMetricsF(self.font()).horizontalAdvance(self.text()))+14
        return QSize(min(260,width),20)
    def minimumSizeHint(self):return QSize(44 if self.text() else 0,20)
    def paintEvent(self,event):
        if not self.text():return
        background,foreground=self.PALETTE[self.tone]
        painter=QPainter(self);painter.setRenderHint(QPainter.RenderHint.Antialiasing)
        painter.setPen(Qt.PenStyle.NoPen);painter.setBrush(QColor(background));painter.drawRoundedRect(QRectF(self.rect()),3,3)
        painter.setPen(QColor(foreground))
        text=self.fontMetrics().elidedText(self.text(),Qt.TextElideMode.ElideRight,max(0,self.width()-14))
        painter.drawText(self.rect().adjusted(7,0,-7,0),Qt.AlignmentFlag.AlignVCenter|Qt.AlignmentFlag.AlignLeft,text)


class ErrorIllustration(QWidget):
    """Small line-art explorer inspecting a failed request or connection."""
    def __init__(self):
        super().__init__();self.setFixedSize(124,124);self.renderer=QSvgRenderer(self);self.set_kind('request')
    def set_kind(self,kind):
        symbols={
            'connection':'<path d="M97 43l9 9m-6-12l9 9m-15-7l-4 4 10 10 4-4m-17 8l-5 5m27-32l5-5"/><path d="M85 33l-4-4m29 34l5 3" stroke="#d49a70"/>',
            'request':'<path d="M89 33h22v28H89zM94 39h12m-12 5h8"/><path d="M97 50l7 7m0-7l-7 7" stroke="#d49a70"/>',
            'timeout':'<circle cx="101" cy="47" r="13"/><path d="M101 38v10l7 4" stroke="#d49a70"/>',
            'cancelled':'<circle cx="101" cy="47" r="13"/><path d="M96 42l10 10m0-10l-10 10" stroke="#d49a70"/>',
        }
        svg=f'''<svg xmlns="http://www.w3.org/2000/svg" width="144" height="144" viewBox="0 0 144 144">
        <g fill="none" stroke="#747474" stroke-width="1.15" stroke-linecap="round" stroke-linejoin="round">
        <path d="M82 23h44v45H82zM86 27h36v37M90 75h28m-14-7v7"/>
        <circle cx="49" cy="55" r="20"/><path d="M42 36c18-3 25 20 17 34M35 42c-9 11-8 22 0 27"/>
        <circle cx="44" cy="55" r="3"/><path d="M37 73h22l5 10-3 35m-24-45l-9 9 3 38m3-23h26M43 98v25c0 7-12 8-13 0m15-24l6 26c2 8 14 5 12-2"/>
        <rect x="24" y="76" width="31" height="26" rx="6"/><path d="M25 91h28m-23 5h17"/>
        <circle cx="31" cy="86" r="2" stroke="#d49a70"/><circle cx="38" cy="86" r="2"/>
        <path d="M61 78c13-2 15-13 23-18l5 6c-6 13-14 20-25 20m16-25l5-9 8 4-4 10M50 35l-3-10"/>
        <circle cx="47" cy="23" r="2.5" fill="#d49a70" stroke="#d49a70"/>
        <path d="M87 110h37v23H87zM91 109v-9h23v9m-18-4h13m-10 16v7m-6-3h26"/>
        <circle cx="122" cy="121" r="10" stroke="#d49a70"/><path d="M122 115v7m0 4v.5" stroke="#d49a70"/>
        {symbols.get(kind,symbols['request'])}</g></svg>'''
        self.renderer.load(QByteArray(svg.encode('utf-8')));self.update()
    def paintEvent(self,event):
        painter=QPainter(self);self.renderer.render(painter,QRectF(self.rect()))


class ErrorDetails(QTextEdit):
    def __init__(self):
        super().__init__();self.setReadOnly(True);self.setFrameShape(QFrame.Shape.NoFrame)
        self.setLineWrapMode(QTextEdit.LineWrapMode.WidgetWidth)
        self.setWordWrapMode(QTextOption.WrapMode.WrapAtWordBoundaryOrAnywhere)
        self.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
        self.setMaximumWidth(552);self.setFixedHeight(48)
        self.setSizePolicy(QSizePolicy.Policy.Expanding,QSizePolicy.Policy.Fixed)
        self.document().setDocumentMargin(0)
        self.document().documentLayout().documentSizeChanged.connect(self.fit_height)
        self.set_tone('error')
    def set_tone(self,tone):
        background='#303030' if tone=='neutral' else '#5c1c06'
        self.setStyleSheet(f'QTextEdit {{background:{background};color:#f0dfd8;border:0;border-radius:3px;padding:10px 14px;font-size:12px;}}')
    def set_details(self,text):
        self.setPlainText(text);self.selectAll();self.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self.moveCursor(self.textCursor().MoveOperation.Start);self.fit_height()
    def fit_height(self,*_):
        height=max(40,min(200,math.ceil(self.document().size().height())+22))
        if self.height()!=height:self.setFixedHeight(height)
    def resizeEvent(self,event):super().resizeEvent(event);self.fit_height()


class ErrorPanel(QScrollArea):
    def __init__(self):
        super().__init__();self.setFrameShape(QFrame.Shape.NoFrame);self.setWidgetResizable(True)
        self.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
        content=QWidget();content.setObjectName('responseErrorContent');content.setStyleSheet('QWidget#responseErrorContent {background:#212121;}')
        self.setWidget(content);outer=QVBoxLayout(content);outer.setContentsMargins(20,20,20,24);outer.setSpacing(0)
        outer.addStretch(1)
        self.card=QWidget();self.card.setMaximumWidth(620);self.card.setSizePolicy(QSizePolicy.Policy.Expanding,QSizePolicy.Policy.Preferred)
        layout=QVBoxLayout(self.card);layout.setContentsMargins(0,0,0,0);layout.setSpacing(10)
        self.illustration=ErrorIllustration();layout.addWidget(self.illustration,0,Qt.AlignmentFlag.AlignHCenter)
        self.title=QLabel();self.title.setTextFormat(Qt.TextFormat.PlainText);self.title.setWordWrap(True)
        self.title.setAlignment(Qt.AlignmentFlag.AlignCenter);self.title.setStyleSheet('font-size:14px;font-weight:600;color:#f2f2f2;')
        self.title.setSizePolicy(QSizePolicy.Policy.Ignored,QSizePolicy.Policy.Preferred);layout.addWidget(self.title)
        self.hint=QLabel();self.hint.setTextFormat(Qt.TextFormat.PlainText);self.hint.setWordWrap(True)
        self.hint.setAlignment(Qt.AlignmentFlag.AlignCenter);self.hint.setStyleSheet('font-size:12px;color:#a6a6a6;')
        self.hint.setSizePolicy(QSizePolicy.Policy.Ignored,QSizePolicy.Policy.Preferred);layout.addWidget(self.hint)
        self.details=ErrorDetails();layout.addWidget(self.details,0,Qt.AlignmentFlag.AlignHCenter)
        outer.addWidget(self.card,0,Qt.AlignmentFlag.AlignHCenter);outer.addStretch(1)
    def set_error(self,info):
        self.title.setText(info['title']);self.hint.setText(info.get('hint',''))
        self.illustration.set_kind(info.get('illustration','request'))
        self.details.set_tone(info.get('tone','error'));self.details.set_details(info.get('details',''))
        self.details.setAccessibleName('Error details');self.verticalScrollBar().setValue(0)
    def resizeEvent(self,event):
        super().resizeEvent(event)
        self.card.setFixedWidth(max(100,min(620,self.viewport().width()-40)))
        self.details.setFixedWidth(max(80,min(552,self.card.width())))
        self.illustration.setVisible(self.viewport().height()>=240)
