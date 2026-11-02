"""The General settings page, shared with the native Settings overlay."""
from ui_theming import theme_qcolor
from dataclasses import asdict,replace
from pathlib import Path
from PySide6.QtCore import QTimer,Qt
from PySide6.QtGui import QPainter,QColor,QPen
from PySide6.QtWidgets import QWidget,QVBoxLayout,QHBoxLayout,QLabel,QComboBox,QLineEdit,QSpinBox,QPushButton,QFileDialog,QRadioButton,QButtonGroup
from general_settings import GeneralConfig
from ui_settings import ProxySwitch


class SettingsRadio(QRadioButton):
    def paintEvent(self,event):
        painter=QPainter(self);painter.setRenderHint(QPainter.RenderHint.Antialiasing);y=(self.height()-14)//2
        painter.setPen(QPen(theme_qcolor('#008cff' if self.isChecked() else '#777777'),1));painter.setBrush(theme_qcolor('#008cff' if self.isChecked() else '#212121'));painter.drawEllipse(1,y,13,13)
        if self.isChecked():
            from ui_palettes import current_theme
            painter.setPen(Qt.PenStyle.NoPen);painter.setBrush(QColor(current_theme().colors['action_text']));painter.drawEllipse(5,y+4,5,5)
        painter.setPen(theme_qcolor('#e6e6e6'));painter.setFont(self.font());painter.drawText(self.rect().adjusted(22,0,0,0),Qt.AlignmentFlag.AlignVCenter,self.text())


class GeneralPage(QWidget):
    def __init__(self,overlay):
        super().__init__();self.overlay=overlay;self.window=overlay.window;self.setObjectName('generalSettings');self.controls={}
        self.config=self.window.general_preferences.config;self.dirty=False
        self.form=QVBoxLayout(self);self.form.setContentsMargins(46,36,46,24);self.form.setSpacing(0)
        title=QLabel('General');title.setObjectName('settingsHeading');self.form.addWidget(title);self.form.addSpacing(24)
        self.section('Request')
        self.choice('http_version','HTTP version',['Auto','HTTP/1.x','HTTP/2'],'Select the HTTP version to use for sending the request.')
        self.number('timeout_ms','Request timeout',0,2147483647,' ms','Maximum request duration. Set to 0 to wait without a timeout.')
        self.number('max_response_mb','Max response size',0,2047,' MB','Maximum response size to download and decode. Set to 0 for no limit.')
        self.toggle('verify_ssl','SSL certificate verification')
        self.toggle('tls_keylog','SSL/TLS key log','Save TLS session keys locally for debugging encrypted connections.')
        log=QPushButton('Export TLS key log');log.clicked.connect(self.export_keylog);log.setMaximumWidth(190);self.form.addWidget(log,alignment=Qt.AlignmentFlag.AlignRight)
        self.toggle('disable_cookies','Disable cookies','Disable the local cookie jar for all requests.')
        self.toggle('request_validation','Request validation','Validate JSON and linked local JSON Schema / OpenAPI request bodies before sending.')
        self.choice('response_format','Response format detection',['Auto','JSON'],radio=True)
        self.section('Working directory','Resolve request body files and local validation schemas relative to this directory.')
        location=QWidget();line=QHBoxLayout(location);line.setContentsMargins(0,0,0,0)
        field=QLineEdit(self.config.working_directory);field.setReadOnly(True);self.controls['working_directory']=field
        choose=QPushButton('Choose');choose.clicked.connect(self.choose_directory);line.addWidget(field,1);line.addWidget(choose)
        self.form.addWidget(QLabel('Location'));self.form.addSpacing(6);self.form.addWidget(location)
        self.toggle('read_outside','Read files outside working directory','Allow request body and schema files outside the selected directory.')
        self.section('Headers')
        self.toggle('no_cache','Send no-cache header')
        self.toggle('postman_token','Send Postman Token header','Send a fresh Postman-Token UUID with each HTTP request.')
        self.toggle('retain_headers','Retain headers when clicking on links')
        self.toggle('follow_redirects','Automatically follow redirects')
        self.section('User interface')
        self.toggle('new_tab','Always open sidebar item in new tab')
        self.toggle('ask_unsaved','Always ask when closing unsaved tabs','Applies to unsaved changes when autosave is off.')
        self.toggle('tab_icons','Show icons with tab names')
        self.toggle('two_pane','Two-pane view','Show the request and response side by side.')
        self.toggle('autocomplete','Variable autocomplete')
        self.choice('documentation_editor','Default documentation editor',['Reqon editor','Markdown editor'],radio=True)
        self.section('Editor settings')
        reset=QPushButton('Reset');reset.clicked.connect(self.reset_editor);reset.setFixedWidth(70);self.form.addWidget(reset,alignment=Qt.AlignmentFlag.AlignRight)
        field=QLineEdit(self.config.font_family);field.setFixedWidth(250);self.controls['font_family']=field;field.textChanged.connect(self.changed)
        self.setting('Font Family',field,'Comma-separated family names. First available font is used.')
        self.number('font_size','Font Size (px)',8,40,' px')
        self.number('indent_count','Indentation count',1,16,'','Number of spaces or tab width per code level.')
        self.choice('indent_type','Indentation type',['Space','Tab'],radio=True)
        self.toggle('close_brackets','Auto close brackets');self.toggle('close_quotes','Auto close quotes')
        from ui_components import CodeEditor
        self.preview=CodeEditor('{\n    "font": "JetBrains Mono",\n    "sample": "Aa 0123 • Кириллица"\n}',readonly=True);self.preview.setFixedHeight(110)
        self.form.addSpacing(12);self.form.addWidget(self.preview)
        self.font_info=QLabel();self.font_info.setObjectName('settingsDescription');self.font_info.setWordWrap(True);self.form.addWidget(self.font_info)
        self.section('Application')
        self.choice('language','Language',['English','Русский'],radio=True)
        self.toggle('autosave','Autosave','Automatically save changes to requests on this computer.')
        note=QLabel('Reqon works locally. No cloud accounts or usage telemetry.');note.setWordWrap(True);note.setObjectName('settingsDescription');self.form.addSpacing(18);self.form.addWidget(note)
        self.status=QLabel(self.window.general_preferences.error);self.status.setObjectName('settingsDescription');self.status.setWordWrap(True);self.form.addSpacing(12);self.form.addWidget(self.status);self.form.addStretch()
        self.timer=QTimer(self);self.timer.setSingleShot(True);self.timer.setInterval(300);self.timer.timeout.connect(self.save)
        self.update_preview()

    def setting(self,title,control,description=''):self.overlay.setting(title,control,description,self.form)
    def section(self,title,description=''):
        if self.form.count()>2:self.form.addSpacing(30)
        label=QLabel(title);label.setObjectName('settingsSection');self.form.addWidget(label);self.form.addSpacing(14)
        if description:
            label=QLabel(description);label.setWordWrap(True);label.setObjectName('settingsDescription');self.form.addWidget(label);self.form.addSpacing(12)
    def toggle(self,key,title,description=''):
        control=ProxySwitch(title,getattr(self.config,key));self.controls[key]=control;control.toggled.connect(self.changed);self.setting(title,control,description)
    def number(self,key,title,low,high,suffix,description=''):
        control=QSpinBox();control.setRange(low,high);control.setValue(getattr(self.config,key));control.setSuffix(suffix);control.setFixedWidth(114)
        self.controls[key]=control;control.valueChanged.connect(self.changed);self.setting(title,control,description)
    def choice(self,key,title,values,description='',radio=False):
        if radio:
            control=QWidget();layout=QHBoxLayout(control);layout.setContentsMargins(0,0,0,0);layout.setSpacing(16)
            group=QButtonGroup(control);group.values=values;self.controls[key]=group
            for i,value in enumerate(values):
                button=SettingsRadio(value);group.addButton(button,i);layout.addWidget(button);button.setChecked(value==getattr(self.config,key))
            group.idClicked.connect(self.changed)
        else:
            control=QComboBox();control.addItems(values);control.setCurrentText(getattr(self.config,key));control.setFixedWidth(114);self.controls[key]=control;control.currentIndexChanged.connect(self.changed)
        self.setting(title,control,description)
    def collect(self):
        values={}
        for key,control in self.controls.items():
            if isinstance(control,QButtonGroup):value=control.values[control.checkedId()]
            elif isinstance(control,QSpinBox):value=control.value()
            elif isinstance(control,QComboBox):value=control.currentText()
            elif isinstance(control,QLineEdit):value=control.text()
            else:value=control.isChecked()
            values[key]=value
        return replace(self.config,**values)
    def changed(self,*_):
        if not hasattr(self,'timer'):return
        self.dirty=True;self.status.setText('Saving…');self.timer.start();self.update_preview()
    def update_preview(self):
        from ui_typography import apply_editor_font
        config=self.collect();actual=apply_editor_font(self.preview,config)
        self.font_info.setText('Interface: Inter · Editor: '+actual)
    def save(self):
        self.timer.stop()
        try:
            config=self.collect();self.window.general_preferences.save(config);self.window.general_settings_changed();self.config=config
            self.dirty=False;self.status.setText('Saved locally');return True
        except Exception as error:self.status.setText(str(error));return False
    def reset_editor(self):
        defaults=GeneralConfig()
        for key in ('font_family','font_size','indent_count','indent_type','close_brackets','close_quotes'):
            control=self.controls[key];value=getattr(defaults,key)
            if isinstance(control,QButtonGroup):control.button(control.values.index(value)).setChecked(True)
            elif isinstance(control,QSpinBox):control.setValue(value)
            elif isinstance(control,QLineEdit):control.setText(value)
            else:control.setChecked(value)
        self.changed()
    def choose_directory(self):
        directory=QFileDialog.getExistingDirectory(self,'Working directory',self.controls['working_directory'].text())
        if directory:self.controls['working_directory'].setText(directory);self.changed()
    def export_keylog(self):
        source=self.window.store.path.parent/'logs/tls-keys.log'
        if not source.exists():self.status.setText('Enable TLS key logging and send a TLS request first.');return
        target,_=QFileDialog.getSaveFileName(self,'Export TLS key log','tls-keys.log','Key log (*.log);;All files (*)')
        if target:
            try:Path(target).write_bytes(source.read_bytes());self.status.setText('TLS key log exported')
            except OSError as error:self.status.setText(str(error))
