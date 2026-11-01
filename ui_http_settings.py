"""Editable HTTP transport options, including settings imported from curl."""
from PySide6.QtWidgets import QWidget,QFormLayout,QCheckBox,QDoubleSpinBox,QLineEdit,QLabel,QScrollArea,QFrame,QComboBox


def settings_tab(page,data):
    widget=QWidget(); form=QFormLayout(widget); form.setContentsMargins(24,20,24,20); form.setVerticalSpacing(14)
    page.general_overrides=QCheckBox('Override General settings');form.addRow(page.general_overrides)
    page.http_version=QComboBox();page.http_version.addItems(['Auto','HTTP/1.x','HTTP/2']);page.http_version.setCurrentText(data.get('http_version','HTTP/1.x'));form.addRow('HTTP version',page.http_version)
    page.http_verify=QCheckBox('Enable server certificate verification'); page.http_verify.setChecked(data.get('http_verify',True))
    page.http_follow_redirects=QCheckBox('Automatically follow redirects'); page.http_follow_redirects.setChecked(data.get('http_follow_redirects',True))
    page.http_timeout=QDoubleSpinBox(); page.http_timeout.setDecimals(3); page.http_timeout.setRange(0,2147483.647)
    page.http_timeout.setValue(data.get('http_timeout',30)); page.http_timeout.setSuffix(' s'); page.http_timeout.setSpecialValueText('No timeout')
    form.addRow(page.http_verify); form.addRow(page.http_follow_redirects); form.addRow('Request timeout',page.http_timeout)
    for key,label in [('http_ca_file','CA certificate'),('http_cert_file','Client certificate'),('http_key_file','Client private key')]:
        field=QLineEdit(data.get(key,'')); field.setPlaceholderText('Local PEM file or {{variable}}'); setattr(page,key,field); form.addRow(label,field)
    page.http_schema=QLineEdit(data.get('http_schema',''));page.http_schema.setPlaceholderText('Optional local JSON Schema or OpenAPI JSON file');form.addRow('Validation schema',page.http_schema)
    note=QLabel('Certificates are read when you send the request. Use PEM files and an unencrypted private key.'); note.setWordWrap(True); note.setObjectName('muted'); form.addRow(note)
    scroll=QScrollArea(); scroll.setWidgetResizable(True); scroll.setFrameShape(QFrame.Shape.NoFrame); scroll.setWidget(widget)
    return scroll
