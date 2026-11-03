"""Native gRPC settings, laid out like the Postman request Settings tab."""
from ui_theming import set_style,theme_qcolor
from ui_brand import stylesheet
from PySide6.QtCore import Qt, QRectF
from PySide6.QtGui import QColor, QPainter, QPen, QIntValidator
from PySide6.QtWidgets import (QAbstractButton,QWidget,QScrollArea,QVBoxLayout,
    QGridLayout,QLabel,QLineEdit,QCheckBox)


class ToggleSwitch(QAbstractButton):
    def __init__(self,label,checked,parent=None):
        super().__init__(parent)
        self.setCheckable(True); self.setChecked(checked); self.setAccessibleName(label)
        self.setFixedSize(68,24); self.setCursor(Qt.CursorShape.PointingHandCursor)
        self.setFocusPolicy(Qt.FocusPolicy.StrongFocus)
        self.toggled.connect(self.update)
    def paintEvent(self,event):
        painter=QPainter(self); painter.setRenderHint(QPainter.RenderHint.Antialiasing)
        painter.setPen(Qt.PenStyle.NoPen)
        painter.setBrush(theme_qcolor('#0983ef' if self.isChecked() else '#424242'))
        painter.drawRoundedRect(QRectF(0,4,32,16),8,8)
        from ui_palettes import current_theme
        painter.setBrush(QColor(current_theme().colors['action_text'] if self.isChecked() else '#ffffff')); painter.drawEllipse(QRectF(18 if self.isChecked() else 2,6,12,12))
        painter.setPen(theme_qcolor('#e6e6e6' if self.isChecked() else '#a8a8a8'))
        painter.drawText(QRectF(37,0,31,24),Qt.AlignmentFlag.AlignVCenter,'ON' if self.isChecked() else 'OFF')
        if self.hasFocus():
            painter.setPen(QPen(theme_qcolor('#8dbcf0'),1,Qt.PenStyle.DotLine)); painter.setBrush(Qt.BrushStyle.NoBrush)
            painter.drawRoundedRect(QRectF(0.5,0.5,66,23),3,3)


class NumberInput(QLineEdit):
    def __init__(self,value,maximum):
        super().__init__(); self.maximum=maximum
        self.setValidator(QIntValidator(0,maximum,self)); self.setValue(value)
        self.setFixedSize(80,32)
        self.editingFinished.connect(lambda:self.setValue(self.value()))
    def value(self):return min(self.maximum,max(0,int(self.text() or '0')))
    def setValue(self,value):self.setText(str(min(self.maximum,max(0,int(value)))))


def settings_tab(page,data):
    scroll=QScrollArea(); scroll.setWidgetResizable(True); scroll.setFrameShape(QScrollArea.Shape.NoFrame)
    scroll.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
    content=QWidget(); content.setObjectName('grpcSettings'); scroll.setWidget(content)
    layout=QGridLayout(content); layout.setContentsMargins(30,14,24,28)
    layout.setHorizontalSpacing(30); layout.setVerticalSpacing(20)
    layout.setColumnStretch(0,2); layout.setColumnStretch(1,1)
    page.tls_verify=ToggleSwitch('Enable server certificate verification',data.get('tls_verify',False))
    page.tls_verify.setToolTip('OFF keeps TLS encryption but does not verify the server certificate or its name.')
    page.tls_server_name=QLineEdit(data.get('tls_server_name','')); page.tls_server_name.setFixedSize(80,32)
    page.grpc_emit_defaults=ToggleSwitch('Include fields with default values in the response',data.get('grpc_emit_defaults',True))
    def number(key,default,maximum):
        return NumberInput(data.get(key,default),maximum)
    page.grpc_max_response_mb=number('grpc_max_response_mb',10,2047)
    page.grpc_timeout_ms=number('grpc_timeout_ms',0,2147483647)
    entries=[
        ('Enable server certificate verification','Verify server certificate when invoking a method over a secure connection.',page.tls_verify),
        ('Overwrite server name for certificate verification','Configure the value used to validate the common name in the server certificate.',page.tls_server_name),
        ('Include fields with default values in the response','View response fields with default values. Turning this OFF will omit such fields from the response.',page.grpc_emit_defaults),
        ('Maximum response message size','Maximum allowed message size in MB. To receive messages of any size, set this value to 0.',page.grpc_max_response_mb),
        ('Connection Timeout','Configure the deadline to invoke and receive the response, in milliseconds. To keep the connection open indefinitely, set this value to 0.',page.grpc_timeout_ms),
    ]
    page.general_overrides=QCheckBox('Override General settings');layout.addWidget(page.general_overrides,0,0,1,2)
    for index,(title,description,field) in enumerate(entries):
        text=QWidget(); labels=QVBoxLayout(text); labels.setContentsMargins(0,0,0,0); labels.setSpacing(4)
        heading=QLabel(title); heading.setObjectName('settingTitle'); heading.setWordWrap(True)
        details=QLabel(description); details.setWordWrap(True)
        labels.addWidget(heading); labels.addWidget(details)
        heading.setBuddy(field); field.setAccessibleName(title); field.setAccessibleDescription(description)
        layout.addWidget(text,index+1,0,Qt.AlignmentFlag.AlignTop)
        layout.addWidget(field,index+1,1,Qt.AlignmentFlag.AlignTop|Qt.AlignmentFlag.AlignLeft)
    layout.setRowStretch(len(entries)+1,1)
    return scroll
