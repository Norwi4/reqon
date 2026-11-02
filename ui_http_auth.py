"""Auth type navigation and typed credential forms."""
from ui_theming import set_style
from PySide6.QtCore import Signal,Qt
from PySide6.QtWidgets import QWidget,QVBoxLayout,QHBoxLayout,QGridLayout,QLabel,QLineEdit,QCheckBox,QStackedWidget,QScrollArea,QFrame,QPlainTextEdit,QPushButton
from ui_components import CodeEditor
from ui_editor_controls import EditorSelect
from http_features import JWT_ALGORITHMS


class AuthForm(QGridLayout):
    def __init__(self,parent):
        super().__init__(parent);self._row=0;self.setColumnStretch(0,1);self.setColumnStretch(1,1);self.setAlignment(Qt.AlignmentFlag.AlignTop)
    def addRow(self,*args):
        if len(args)==1:self.addWidget(args[0],self._row,0,1,2)
        else:
            label,field=args
            if isinstance(label,str):label=QLabel(label)
            label.setWordWrap(True);self.addWidget(label,self._row,0,alignment=Qt.AlignmentFlag.AlignTop);self.addWidget(field,self._row,1)
        self._row+=1


class AuthPanel(QWidget):
    changed=Signal()
    def __init__(self,data):
        super().__init__();self._syncing=False
        outer=QHBoxLayout(self);outer.setContentsMargins(0,0,0,0);outer.setSpacing(0)
        left=QFrame();left.setFixedWidth(212);left.setObjectName('authNavigation')
        set_style(left,'QFrame#authNavigation {border-right:1px solid #363636;}')
        nav=QVBoxLayout(left);nav.setContentsMargins(12,16,12,16);nav.setSpacing(10)
        label=QLabel('Auth Type');label.setObjectName('muted');nav.addWidget(label)
        self.auth_type=EditorSelect(['No Auth','Basic Auth','Bearer Token','JWT Bearer','API Key'],data.get('auth_type','No Auth'));nav.addWidget(self.auth_type)
        self.description=QLabel('The authorization header will be automatically generated when you send the request.');self.description.setWordWrap(True);self.description.setObjectName('muted');self.description.setMargin(2);nav.addSpacing(12);nav.addWidget(self.description)
        self.jwt_location_label=QLabel('Add JWT token to');nav.addWidget(self.jwt_location_label)
        self.jwt_location=EditorSelect(['Request Header','Query Param'],data.get('jwt_location','Request Header'));nav.addWidget(self.jwt_location)
        nav.addStretch();outer.addWidget(left)
        self.pages=QStackedWidget();outer.addWidget(self.pages,1)
        none=QWidget();empty=QVBoxLayout(none);empty.setContentsMargins(16,42,16,16);empty.setSpacing(12)
        symbol=QLabel('−');symbol.setAlignment(Qt.AlignmentFlag.AlignCenter);symbol.setFixedSize(48,48);set_style(symbol,'background:#2b2b2b;border-radius:16px;color:#888;font-size:18px;')
        empty.addWidget(symbol,alignment=Qt.AlignmentFlag.AlignHCenter);heading=QLabel('No Auth');heading.setAlignment(Qt.AlignmentFlag.AlignCenter);set_style(heading,'font-weight:600;');empty.addWidget(heading)
        hint=QLabel('This request does not use any authorization.');hint.setObjectName('muted');hint.setAlignment(Qt.AlignmentFlag.AlignCenter);empty.addWidget(hint);empty.addStretch();self.pages.addWidget(none)
        self.auth_value=QLineEdit(data.get('auth_value',''),self);self.auth_value.hide()
        self.fields=[]
        def panel():
            content=QWidget();content.setObjectName('httpAuthForm');set_style(content,'QWidget#httpAuthForm {background:#1f1f1f;}');form=AuthForm(content);form.setContentsMargins(14,24,24,24);form.setHorizontalSpacing(24);form.setVerticalSpacing(18)
            scroll=QScrollArea();scroll.setWidgetResizable(True);scroll.setFrameShape(QFrame.Shape.NoFrame);scroll.setWidget(content);self.pages.addWidget(scroll);return form
        def line(placeholder='',password=False):
            field=QLineEdit();field.setPlaceholderText(placeholder);field.setMinimumHeight(30);field.setMinimumWidth(180);field.setMaximumWidth(320)
            if password:field.setEchoMode(QLineEdit.EchoMode.Password)
            self.fields.append(field);return field
        form=panel();self.username=line('Username');self.password=line('Password',True);form.addRow('Username',self.username);form.addRow('Password',self.password)
        form=panel();self.token=line('Token');form.addRow('Token',self.token)
        form=panel();self.jwt_algorithm=EditorSelect(JWT_ALGORITHMS,data.get('jwt_algorithm','HS256'));form.addRow('Algorithm',self.jwt_algorithm)
        self.jwt_secret=QPlainTextEdit(data.get('jwt_secret',''));self.jwt_secret.setFixedHeight(34 if self.jwt_algorithm.currentText().startswith('HS') else 100);self.jwt_secret.setPlaceholderText('Secret');set_style(self.jwt_secret,'QPlainTextEdit {border:1px solid #444;border-radius:3px;padding:2px;}');form.addRow('Secret',self.jwt_secret)
        self.jwt_secret_base64=QCheckBox('Secret Base64 encoded');self.jwt_secret_base64.setChecked(data.get('jwt_secret_base64',False));form.addRow('',self.jwt_secret_base64)
        self.jwt_payload=CodeEditor(data.get('jwt_payload','{}'));self.jwt_payload.setFixedHeight(138);form.addRow('Payload',self.jwt_payload)
        self.advanced_button=QPushButton('⌄ Advanced configuration');self.advanced_button.setFlat(True);self.advanced_button.setCheckable(True);self.advanced_button.setChecked(True);form.addRow(self.advanced_button)
        set_style(self.advanced_button,'QPushButton {background:transparent;border:0;text-align:left;padding:8px 0;} QPushButton:hover {color:white;}')
        self.advanced=QWidget();advanced=AuthForm(self.advanced);advanced.setContentsMargins(0,0,0,0);advanced.setHorizontalSpacing(24);advanced.setVerticalSpacing(18)
        self.jwt_prefix=line();self.jwt_prefix.setText(data.get('jwt_prefix','Bearer'));self.prefix_label=QLabel('Request header prefix');advanced.addRow(self.prefix_label,self.jwt_prefix)
        self.jwt_query_name=line();self.jwt_query_name.setText(data.get('jwt_query_name','token'));self.query_label=QLabel('Query param name');advanced.addRow(self.query_label,self.jwt_query_name)
        self.jwt_headers=CodeEditor(data.get('jwt_headers','{}'));self.jwt_headers.setFixedHeight(138);advanced.addRow('JWT headers',self.jwt_headers)
        for editor in (self.jwt_payload,self.jwt_headers):set_style(editor,'QPlainTextEdit#codeEditor {border:1px solid #363636;border-radius:3px;padding:2px;}')
        set_style(self.jwt_secret_base64,'QCheckBox::indicator {width:12px;height:12px;border:1px solid #666;border-radius:2px;background:transparent;} QCheckBox::indicator:checked {background:#3b82f6;}')
        hint=QLabel('Headers specific to the algorithm are added automatically.');hint.setObjectName('muted');hint.setWordWrap(True);advanced.addRow('',hint);form.addRow(self.advanced)
        self.advanced_button.toggled.connect(self.advanced.setVisible)
        form=panel();self.api_key_name=line('Key');self.api_key_value=line('Value');self.api_key_location=EditorSelect(['Header','Query Params'],data.get('api_key_location','Header'))
        form.addRow('Key',self.api_key_name);form.addRow('Value',self.api_key_value);form.addRow('Add to',self.api_key_location)
        self._from_value();self.auth_value.textChanged.connect(self._from_value)
        for field in (self.username,self.password,self.token,self.api_key_name,self.api_key_value):field.textChanged.connect(self._to_value)
        self.fields.extend([self.jwt_location,self.jwt_algorithm,self.jwt_secret,self.jwt_secret_base64,self.jwt_payload,self.jwt_headers,self.api_key_location])
        for field in self.fields:
            signal=field.textChanged if hasattr(field,'textChanged') else field.toggled if hasattr(field,'toggled') else field.currentTextChanged
            signal.connect(self.changed)
        self.auth_type.currentTextChanged.connect(self._mode);self.jwt_location.currentTextChanged.connect(self._jwt_location)
        self.jwt_algorithm.currentTextChanged.connect(lambda value:self.jwt_secret.setFixedHeight(34 if value.startswith('HS') else 100))
        self._mode(self.auth_type.currentText(),initial=True)
    def _from_value(self):
        if self._syncing:return
        self._syncing=True;value=self.auth_value.text();kind=self.auth_type.currentText()
        if kind=='Basic Auth':user,_,password=value.partition(':');self.username.setText(user);self.password.setText(password)
        elif kind=='Bearer Token':self.token.setText(value)
        elif kind=='API Key':key,_,secret=value.partition(':');self.api_key_name.setText(key);self.api_key_value.setText(secret)
        self._syncing=False
    def _to_value(self):
        if self._syncing:return
        kind=self.auth_type.currentText();value=None
        if kind=='Basic Auth':value=self.username.text()+':'+self.password.text()
        elif kind=='Bearer Token':value=self.token.text()
        elif kind=='API Key':value=self.api_key_name.text()+':'+self.api_key_value.text()
        if value is not None:
            self._syncing=True;self.auth_value.setText(value);self._syncing=False
        self.changed.emit()
    def _mode(self,kind,initial=False):
        self.pages.setCurrentIndex(self.auth_type.currentIndex());self.description.setVisible(kind!='No Auth')
        self.jwt_location_label.setVisible(kind=='JWT Bearer');self.jwt_location.setVisible(kind=='JWT Bearer');self._jwt_location()
        if not initial:self._to_value();self.changed.emit()
    def _jwt_location(self,*_):
        query=self.jwt_location.currentText()=='Query Param'
        self.prefix_label.setVisible(not query);self.jwt_prefix.setVisible(not query);self.query_label.setVisible(query);self.jwt_query_name.setVisible(query)
    def collect(self):
        return dict(auth_type=self.auth_type.currentText(),auth_value=self.auth_value.text(),api_key_location=self.api_key_location.currentText(),
            jwt_algorithm=self.jwt_algorithm.currentText(),jwt_secret=self.jwt_secret.toPlainText(),jwt_secret_base64=self.jwt_secret_base64.isChecked(),
            jwt_payload=self.jwt_payload.toPlainText(),jwt_headers=self.jwt_headers.toPlainText(),jwt_location=self.jwt_location.currentText(),
            jwt_prefix=self.jwt_prefix.text(),jwt_query_name=self.jwt_query_name.text())
