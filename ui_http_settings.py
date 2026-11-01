"""Per-request HTTP settings with explicit General defaults."""
from PySide6.QtCore import Qt
from PySide6.QtWidgets import QWidget,QVBoxLayout,QHBoxLayout,QCheckBox,QDoubleSpinBox,QLineEdit,QLabel,QScrollArea,QFrame,QSpinBox
from ui_editor_controls import EditorSelect,CheckSelect
from ui_grpc_settings import ToggleSwitch
from http_features import REQUEST_DEFAULTS


def settings_tab(page,data):
    widget=QWidget();widget.setObjectName('httpSettingsContent');widget.setStyleSheet('QWidget#httpSettingsContent {background:#1f1f1f;} QSpinBox,QDoubleSpinBox {background:#252525;color:#f5f7fa;border:1px solid #444;border-radius:3px;padding:6px;}');layout=QVBoxLayout(widget);layout.setContentsMargins(16,8,24,20);layout.setSpacing(0)
    page.http_extra_fields={}
    def row(name,title,description,field,default=None):
        setattr(page,name,field)
        container=QWidget();line=QHBoxLayout(container);line.setContentsMargins(0,12,0,12);line.setSpacing(24)
        copy=QVBoxLayout();copy.setSpacing(5);heading=QLabel(title);heading.setStyleSheet('font-weight:600;');copy.addWidget(heading)
        hint=QLabel(description);hint.setWordWrap(True);hint.setObjectName('muted');copy.addWidget(hint);line.addLayout(copy,2)
        control=QVBoxLayout();control.setSpacing(5);control.addWidget(field,alignment=Qt.AlignmentFlag.AlignLeft)
        if default:
            link=QLabel('Default: <a href="settings" style="color:#3b82f6;text-decoration:none">'+default+'</a>');link.linkActivated.connect(lambda _:page.window().show_settings());control.addWidget(link)
        line.addLayout(control,1);layout.addWidget(container)
    def toggle(name,title,description,default=False,general=None):
        field=ToggleSwitch(title,data.get(name,default));row(name,title,description,field,general)
        if name in REQUEST_DEFAULTS:page.http_extra_fields[name]=field
    row('http_version','HTTP version','Select the HTTP version to use for sending the request.',EditorSelect(['Auto','HTTP/1.x','HTTP/2'],data.get('http_version','HTTP/1.x')),'Settings')
    toggle('http_verify','Enable SSL certificate verification','Verify SSL certificates when sending a request. Verification failures will abort the request.',False,'Settings')
    toggle('http_follow_redirects','Automatically follow redirects','Follow HTTP 3xx responses as redirects.',True,'Settings')
    toggle('http_follow_original_method','Follow original HTTP method','Keep the original HTTP method when following a redirect.')
    toggle('http_follow_auth','Follow Authorization header','Retain the Authorization header when redirecting to a different hostname.')
    toggle('http_remove_referer','Remove referer header on redirect','Remove the Referer header when a redirect occurs.')
    toggle('http_strict_parser','Enable strict HTTP parser','Reject folded response headers and invalid control characters in HTTP/1 responses.')
    toggle('http_encode_url','Encode URL automatically','Encode spaces and non-ASCII characters in the URL before sending.',True)
    toggle('http_disable_cookies','Disable cookie jar','Prevent cookies in the cookie jar from being stored or sent.',False,'Settings')
    toggle('http_server_cipher_order','Use server cipher suite during handshake','Set the OpenSSL cipher preference option for the TLS handshake.')
    count=QSpinBox();count.setRange(0,100);count.setValue(data.get('http_max_redirects',10));count.setFixedWidth(90)
    row('http_max_redirects','Maximum number of redirects','Set the maximum number of redirects to follow.',count);page.http_extra_fields['http_max_redirects']=count
    for name,title,description,placeholder in [
        ('http_disabled_tls','TLS/SSL protocols disabled during handshake','Comma-separated protocols to exclude from TLS negotiation.','TLSv1, TLSv1.1, TLSv1.2, TLSv1.3'),
        ('http_ciphers','Cipher suite selection','OpenSSL cipher list for TLS 1.2 and earlier. Leave empty to use the system defaults.','e.g. ECDHE+AESGCM')]:
        import ssl
        choices=['TLSv1','TLSv1.1','TLSv1.2','TLSv1.3'] if name=='http_disabled_tls' else [cipher['name'] for cipher in ssl.create_default_context().get_ciphers() if cipher['protocol']!='TLSv1.3']
        field=CheckSelect(choices,data.get(name,''),', ' if name=='http_disabled_tls' else ':','Select protocols' if name=='http_disabled_tls' else 'Select cipher suites');field.setToolTip(placeholder);field.setMinimumWidth(180)
        row(name,title,description,field);page.http_extra_fields[name]=field
    page.general_overrides=QCheckBox('Override General settings');page.general_overrides.setChecked(data.get('general_overrides',False));layout.addWidget(page.general_overrides)
    page.http_timeout=QDoubleSpinBox();page.http_timeout.setDecimals(3);page.http_timeout.setRange(0,2147483.647);page.http_timeout.setValue(data.get('http_timeout',0));page.http_timeout.setSuffix(' s');page.http_timeout.setSpecialValueText('No timeout')
    row('http_timeout','Request timeout','Limit the total duration of this request, including redirects.',page.http_timeout,'Settings')
    for key,label in [('http_ca_file','CA certificate'),('http_cert_file','Client certificate'),('http_key_file','Client private key'),('http_schema','Validation schema')]:
        field=QLineEdit(data.get(key,''));field.setPlaceholderText('Local file or {{variable}}')
        row(key,label,'JSON Schema or OpenAPI JSON file.' if key=='http_schema' else 'PEM file, read when sending the request.',field)
    layout.addStretch()
    scroll=QScrollArea();scroll.setWidgetResizable(True);scroll.setFrameShape(QFrame.Shape.NoFrame);scroll.setWidget(widget);return scroll


def collect_settings(page):
    return {key:field.isChecked() if hasattr(field,'isChecked') else field.value() if hasattr(field,'value') else field.text().strip()
            for key,field in page.http_extra_fields.items()}
