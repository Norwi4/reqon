"""Settings drawn inside the main widget: no dialog, native window or taskbar entry."""
from ui_theming import set_style,theme_qcolor
from PySide6.QtCore import Qt,QEvent,QTimer
from PySide6.QtGui import QPainter,QColor,QIntValidator,QPen,QPainterPath
from PySide6.QtWidgets import (QWidget,QFrame,QVBoxLayout,QHBoxLayout,QGridLayout,QLabel,
    QPushButton,QCheckBox,QLineEdit,QPlainTextEdit,QScrollArea,QComboBox,QApplication,QStackedWidget)
from shiboken6 import isValid
from proxy_support import ProxyConfig,PROTOCOLS
from ui_components import tool,icon
from ui_grpc_settings import ToggleSwitch


class ProxySwitch(ToggleSwitch):
    def __init__(self,label,checked):
        super().__init__(label,checked);self.setFixedSize(34,24)
    def paintEvent(self,event):
        painter=QPainter(self);painter.setRenderHint(QPainter.RenderHint.Antialiasing)
        painter.setPen(Qt.PenStyle.NoPen);painter.setBrush(theme_qcolor('#008cff' if self.isChecked() else '#444444'))
        from ui_palettes import current_theme
        painter.drawRoundedRect(3,6,28,14,7,7);painter.setBrush(QColor(current_theme().colors['action_text'] if self.isChecked() else '#ffffff'))
        painter.drawEllipse(18 if self.isChecked() else 5,8,10,10)
        if self.hasFocus():
            painter.setPen(theme_qcolor('#89c6ff'));painter.setBrush(Qt.BrushStyle.NoBrush);painter.drawRoundedRect(1,3,32,20,10,10)


class SettingsCheckBox(QCheckBox):
    def paintEvent(self,event):
        painter=QPainter(self);painter.setRenderHint(QPainter.RenderHint.Antialiasing)
        y=(self.height()-14)//2
        painter.setPen(QPen(theme_qcolor('#008cff' if self.isChecked() else '#777777'),1))
        painter.setBrush(theme_qcolor('#008cff' if self.isChecked() else '#212121'));painter.drawRoundedRect(1,y,13,13,2,2)
        if self.isChecked():
            path=QPainterPath();path.moveTo(4,y+6);path.lineTo(7,y+9);path.lineTo(11,y+4)
            from ui_palettes import current_theme
            painter.setPen(QPen(QColor(current_theme().colors['action_text']),1.6));painter.drawPath(path)
        painter.setFont(self.font());painter.setPen(theme_qcolor('#e6e6e6'))
        painter.drawText(self.rect().adjusted(22,0,0,0),Qt.AlignmentFlag.AlignVCenter,self.text())
        if self.hasFocus():
            painter.setPen(QPen(theme_qcolor('#89c6ff'),1,Qt.PenStyle.DotLine));painter.setBrush(Qt.BrushStyle.NoBrush);painter.drawRect(self.rect().adjusted(0,0,-1,-1))


class SettingsOverlay(QWidget):
    def __init__(self,window):
        super().__init__(window.centralWidget());self.window=window;self.setObjectName('settingsOverlay')
        self.setFocusPolicy(Qt.FocusPolicy.StrongFocus);self.hide();self._background=[];self._actions=[];self._previous_focus=None
        self.parentWidget().installEventFilter(self)
        self.panel=QFrame(self);self.panel.setObjectName('settingsCard')
        outer=QHBoxLayout(self.panel);outer.setContentsMargins(0,0,0,0);outer.setSpacing(0)
        self.nav=QFrame();self.nav.setObjectName('settingsNavigation');self.nav.setFixedWidth(226)
        nav=QVBoxLayout(self.nav);nav.setContentsMargins(24,28,24,24);nav.setSpacing(3);self.navigation={}
        for label,glyph in [('General','settings'),('Themes','palette'),('Shortcuts','console'),('Data','save'),
                            ('Add-ons','plus'),('Certificates','lock'),('Proxy','proxy'),('Update','download'),('About','docs')]:
            button=QPushButton(label);button.setIcon(icon(glyph));button.setFixedHeight(34);button.setCheckable(True)
            button.setAutoExclusive(True)
            button.setObjectName('settingsNavItem');button.setChecked(label=='General');button.setEnabled(label in ('General','Proxy','Themes'))
            if label not in ('General','Proxy','Themes'):button.setToolTip('Not available yet')
            else:button.clicked.connect(lambda checked=False,label=label:self.pages.setCurrentIndex({'General':0,'Proxy':1,'Themes':2}[label]))
            self.navigation[label]=button;nav.addWidget(button)
        nav.addStretch();outer.addWidget(self.nav)
        right=QWidget();right_layout=QVBoxLayout(right);right_layout.setContentsMargins(0,0,0,0);right_layout.setSpacing(0)
        self.scroll=QScrollArea();self.scroll.setWidgetResizable(True);self.scroll.setFrameShape(QFrame.Shape.NoFrame)
        self.scroll.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
        content=QWidget();content.setObjectName('proxySettings');self.form=QVBoxLayout(content)
        self.form.setContentsMargins(46,36,46,24);self.form.setSpacing(0)
        title=QLabel('Proxy');title.setObjectName('settingsHeading');self.form.addWidget(title);self.form.addSpacing(24)
        self.section('Default proxy configuration','Reqon uses your system’s proxy configuration by default to send API requests.')
        config=window.proxy_preferences.config
        self.system_auth=SettingsCheckBox('This proxy requires authentication');self.system_auth.setChecked(config.system_auth)
        self.form.addWidget(self.system_auth);self.form.addSpacing(14)
        self.system_credentials,self.system_username,self.system_password=self.credentials(config.system_username,config.system_password)
        self.form.addWidget(self.system_credentials);self.form.addSpacing(26)
        self.section('Proxy configurations for sending requests','Specify the proxy used to send HTTP and gRPC requests. Custom settings take priority over system settings.')
        self.form.addSpacing(8)
        self.system=ProxySwitch('Use system proxy',config.system);self.setting('Use system proxy',self.system)
        self.environment=ProxySwitch('Respect HTTP_PROXY, HTTPS_PROXY, and NO_PROXY environment variables',config.environment)
        self.setting('Respect HTTP_PROXY, HTTPS_PROXY, and NO_PROXY environment variables',self.environment)
        self.custom=ProxySwitch('Use custom proxy configuration',config.custom);self.setting('Use custom proxy configuration',self.custom)
        self.custom_panel=QWidget();self.custom_form=QVBoxLayout(self.custom_panel);self.custom_form.setContentsMargins(0,0,0,0);self.custom_form.setSpacing(0)
        types=QWidget();types_row=QHBoxLayout(types);types_row.setContentsMargins(0,0,0,0);types_row.setSpacing(16)
        self.http=SettingsCheckBox('HTTP');self.http.setChecked(config.http);self.https=SettingsCheckBox('HTTPS');self.https.setChecked(config.https)
        types_row.addWidget(self.http);types_row.addWidget(self.https)
        self.setting('Use proxy for',types,'Choose which request types use the proxy.',self.custom_form)
        server=QWidget();server_row=QHBoxLayout(server);server_row.setContentsMargins(0,0,0,0);server_row.setSpacing(7)
        self.protocol=QComboBox();self.protocol.addItems(PROTOCOLS);self.protocol.setCurrentText(config.protocol);self.protocol.setFixedWidth(89)
        self.host=QLineEdit(config.host);self.host.setPlaceholderText('127.0.0.1');self.host.setMinimumWidth(100)
        self.port=QLineEdit(str(config.port));self.port.setValidator(QIntValidator(1,65535,self.port));self.port.setFixedWidth(65)
        for widget in (self.protocol,QLabel('://'),self.host,QLabel(':'),self.port):server_row.addWidget(widget)
        server.setMaximumWidth(320);self.setting('Proxy server',server,layout=self.custom_form)
        self.validation=QLabel();self.validation.setObjectName('proxyValidation');self.validation.setWordWrap(True);self.custom_form.addWidget(self.validation)
        self.auth=ProxySwitch('Proxy auth',config.auth);self.setting('Proxy auth',self.auth,'Authenticate with the proxy server.',self.custom_form)
        self.custom_credentials,self.username,self.password=self.credentials(config.username,config.password)
        self.custom_form.addWidget(self.custom_credentials)
        self.socks_note=QLabel('SOCKS4 uses the username only; passwords are not part of that protocol.');self.socks_note.setWordWrap(True);self.socks_note.setObjectName('settingsDescription');self.custom_form.addWidget(self.socks_note)
        self.bypass=QPlainTextEdit(config.bypass);self.bypass.setPlaceholderText('E.g. 127.0.0.1, localhost,\n*.example.com');self.bypass.setFixedSize(220,78)
        self.setting('Proxy bypass',self.bypass,'Comma-separated hosts that connect directly.',self.custom_form)
        self.form.addWidget(self.custom_panel);self.form.addSpacing(12)
        self.status=QLabel();self.status.setObjectName('settingsDescription');self.status.setWordWrap(True);self.form.addWidget(self.status);self.form.addStretch(1)
        self.scroll.setWidget(content)
        from ui_general_settings import GeneralPage
        self.general=GeneralPage(self);self.general_scroll=QScrollArea();self.general_scroll.setWidgetResizable(True)
        self.general_scroll.setFrameShape(QFrame.Shape.NoFrame);self.general_scroll.setWidget(self.general)
        self.general_scroll.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
        self.pages=QStackedWidget();self.pages.addWidget(self.general_scroll);self.pages.addWidget(self.scroll)
        from ui_themes import ThemesPage
        self.themes=ThemesPage(self);self.themes_scroll=QScrollArea();self.themes_scroll.setWidgetResizable(True)
        self.themes_scroll.setFrameShape(QFrame.Shape.NoFrame);self.themes_scroll.setWidget(self.themes)
        self.themes_scroll.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff);self.pages.addWidget(self.themes_scroll)
        right_layout.addWidget(self.pages);outer.addWidget(right,1)
        self.close_button=tool('close','Close settings',self.dismiss,parent=self.panel);self.close_button.setFixedSize(30,30)
        self.close_button.setObjectName('settingsClose')
        self.timer=QTimer(self);self.timer.setSingleShot(True);self.timer.setInterval(300);self.timer.timeout.connect(self.save)
        for widget in (self.system,self.environment,self.custom,self.http,self.https,self.system_auth,self.auth):widget.toggled.connect(self.changed)
        for widget in (self.host,self.port,self.username,self.password,self.system_username,self.system_password):widget.textChanged.connect(self.changed)
        self.protocol.currentIndexChanged.connect(self.changed);self.bypass.textChanged.connect(self.changed)
        for widget,label in [(self.host,'Proxy host'),(self.port,'Proxy port'),(self.protocol,'Proxy protocol'),(self.bypass,'Proxy bypass')]:widget.setAccessibleName(label)
        self.update_fields();self.status.setText(window.proxy_preferences.load_error)

    def section(self,title,description):
        heading=QLabel(title);heading.setObjectName('settingsSection');self.form.addWidget(heading);self.form.addSpacing(6)
        text=QLabel(description);text.setObjectName('settingsDescription');text.setWordWrap(True);self.form.addWidget(text);self.form.addSpacing(16)

    def setting(self,title,control,description='',layout=None):
        frame=QFrame();frame.setObjectName('proxySetting');row=QHBoxLayout(frame);row.setContentsMargins(0,13,0,13);row.setSpacing(12)
        labels=QVBoxLayout();labels.setContentsMargins(0,0,0,0);labels.setSpacing(4)
        labels.setAlignment(Qt.AlignmentFlag.AlignVCenter)
        name=QLabel(title);name.setWordWrap(True);set_style(name,'font-size:12px;font-weight:500;');labels.addWidget(name)
        if description:
            text=QLabel(description);text.setWordWrap(True);text.setObjectName('settingsDescription');labels.addWidget(text)
        row.addLayout(labels,1);row.addWidget(control);(layout or self.form).addWidget(frame)

    def credentials(self,user,secret):
        widget=QWidget();grid=QGridLayout(widget);grid.setContentsMargins(0,12,0,16);grid.setHorizontalSpacing(12)
        username=QLineEdit(user);password=QLineEdit(secret);password.setEchoMode(QLineEdit.EchoMode.Password)
        username.setAccessibleName('Proxy username');password.setAccessibleName('Proxy password')
        eye=password.addAction(icon('eye'),QLineEdit.ActionPosition.TrailingPosition);eye.setToolTip('Show / hide password')
        eye.triggered.connect(lambda:password.setEchoMode(QLineEdit.EchoMode.Normal if password.echoMode()==QLineEdit.EchoMode.Password else QLineEdit.EchoMode.Password))
        grid.addWidget(QLabel('Username'),0,0);grid.addWidget(QLabel('Password'),0,1);grid.addWidget(username,1,0);grid.addWidget(password,1,1)
        return widget,username,password

    def collect(self):
        return ProxyConfig(system=self.system.isChecked(),environment=self.environment.isChecked(),custom=self.custom.isChecked(),
            http=self.http.isChecked(),https=self.https.isChecked(),protocol=self.protocol.currentText(),host=self.host.text(),port=self.port.text(),
            auth=self.auth.isChecked(),username=self.username.text(),password=self.password.text(),bypass=self.bypass.toPlainText(),
            system_auth=self.system_auth.isChecked(),system_username=self.system_username.text(),system_password=self.system_password.text())

    def update_fields(self):
        self.system_credentials.setVisible(self.system_auth.isChecked());self.custom_panel.setVisible(self.custom.isChecked())
        self.custom_credentials.setEnabled(self.auth.isChecked())
        socks4=self.protocol.currentText() in ('socks4','socks4a');self.password.setEnabled(not socks4)
        self.socks_note.setVisible(socks4)
        error=self.collect().error();self.validation.setText(error);self.validation.setVisible(bool(error))
        set_style(self.host,'border-color:#d97b6c;' if error and 'host' in error.lower() else '')

    def changed(self,*_):
        self.update_fields();self.status.setText('Saving…');self.timer.start()

    def save(self):
        self.timer.stop()
        try:
            config=self.collect();self.window.proxy_preferences.save(config)
            self.window.proxy_settings_changed()
            self.status.setText('Saved locally. Fix the highlighted settings before sending requests.' if config.error() else 'Saved locally · Applies to new requests and reflection calls.')
            return True
        except Exception as error:
            self.status.setText('Could not save proxy settings. '+str(error));return False

    def present(self):
        if self.isVisible():self.raise_();return
        self._previous_focus=QApplication.focusWidget();self._background=[]
        for child in self.parentWidget().findChildren(QWidget,options=Qt.FindChildOption.FindDirectChildrenOnly):
            if child is not self:self._background.append((child,child.isEnabled()));child.setEnabled(False)
        self._actions=[(action,action.isEnabled()) for action in self.window.actions()]
        for action,_ in self._actions:action.setEnabled(False)
        if hasattr(self.window,'search_palette'):self.window.search_palette.hide()
        for i in range(self.window.workbench.count()):
            page=self.window.workbench.widget(i)
            if hasattr(page,'method_picker'):page.method_picker.dismiss();page.tls_button.card.hide()
        self.reposition();self.show();self.raise_();self.close_button.setFocus()

    def dismiss(self):
        if not self.save_pending():return
        self.hide()
        for widget,enabled in self._background:
            if isValid(widget):widget.setEnabled(enabled)
        for action,enabled in self._actions:
            if isValid(action):action.setEnabled(enabled)
        if self._previous_focus is not None and isValid(self._previous_focus):self._previous_focus.setFocus()

    def save_pending(self):
        if self.general.dirty and not self.general.save():return False
        return not self.timer.isActive() or self.save()

    def reposition(self):
        self.setGeometry(self.parentWidget().rect());width=min(844,self.width()-48);height=min(840,self.height()-72)
        self.panel.setGeometry((self.width()-width)//2,(self.height()-height)//2,width,height)
        self.close_button.move(width-45,15);self.close_button.raise_()

    def eventFilter(self,watched,event):
        if watched is self.parentWidget() and event.type()==QEvent.Type.Resize:self.reposition()
        return super().eventFilter(watched,event)

    def keyPressEvent(self,event):
        if event.key()==Qt.Key.Key_Escape:self.dismiss();event.accept()
        else:super().keyPressEvent(event)

    def paintEvent(self,event):
        painter=QPainter(self);painter.fillRect(self.rect(),QColor(0,0,0,115))
