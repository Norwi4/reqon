"""Live theme selection, with independent day and night palettes."""
from dataclasses import replace
from PySide6.QtCore import Qt,QRectF,QSize,QSignalBlocker
from PySide6.QtGui import QColor,QPainter,QPen
from PySide6.QtWidgets import (QWidget,QAbstractButton,QLabel,QVBoxLayout,QHBoxLayout,QGridLayout,
    QFrame,QButtonGroup,QStackedWidget,QSizePolicy)
from ui_components import icon
from ui_general_settings import SettingsRadio
from ui_editor_controls import EditorSelect
from ui_palettes import THEMES,current_theme
from ui_theming import set_style


def draw_preview(painter,rect,theme_id):
    """A small native rendition of the workspace in the preview's own palette."""
    c=THEMES[theme_id].colors;painter.save();painter.setClipRect(rect)
    painter.translate(rect.left(),rect.top());painter.scale(rect.width()/160,rect.height()/94)
    def box(x,y,w,h,color):painter.fillRect(QRectF(x,y,w,h),QColor(c[color]))
    box(0,0,160,94,'canvas');box(0,0,160,14,'panel');box(0,14,35,80,'panel')
    box(0,14,160,1,'border');box(34,14,1,80,'border');box(35,29,125,1,'border')
    for x,w in ((6,17),(28,14),(48,17)):box(x,6,w,2,'muted')
    box(139,3,15,8,'accent');box(42,21,5,2,'success');box(50,21,23,2,'text')
    box(40,35,113,15,'panel');box(44,41,12,2,'success');box(63,41,49,2,'muted');box(128,35,25,15,'action')
    for y,w in ((24,17),(35,22),(46,14),(57,20)):box(6,y,w,2,'muted')
    for x,w in ((42,16),(63,12),(80,18),(103,15)):box(x,57,w,2,'muted')
    box(79,63,19,2,'accent')
    for y,w in ((73,53),(80,77),(87,41)):box(46,y,w,2,'border')
    painter.restore()


class ThemePreview(QWidget):
    def __init__(self,theme_id,parent=None):
        super().__init__(parent);self.theme_id=theme_id;self.setFixedHeight(133)
    def paintEvent(self,event):
        painter=QPainter(self);draw_preview(painter,QRectF(self.rect()),self.theme_id)
        painter.setPen(QPen(QColor(current_theme().colors['border']),1));painter.drawRect(self.rect().adjusted(0,0,-1,-1))


class ThemeChoice(QAbstractButton):
    def __init__(self,theme_id):
        super().__init__();self.theme_id=theme_id;self.setText(THEMES[theme_id].name);self.setAccessibleName(self.text())
        self.setCheckable(True);self.setFocusPolicy(Qt.FocusPolicy.StrongFocus);self.setCursor(Qt.CursorShape.PointingHandCursor)
        self.setSizePolicy(QSizePolicy.Policy.Expanding,QSizePolicy.Policy.Fixed);self.setFixedHeight(125);self.setMinimumWidth(135)
    def sizeHint(self):return QSize(160,125)
    def paintEvent(self,event):
        painter=QPainter(self);painter.setRenderHint(QPainter.RenderHint.Antialiasing);c=current_theme().colors
        rect=QRectF(2,2,self.width()-4,94);draw_preview(painter,rect,self.theme_id)
        border=c['accent'] if self.isChecked() else c['muted'] if self.underMouse() else c['border']
        painter.setPen(QPen(QColor(border),2 if self.isChecked() else 1));painter.setBrush(Qt.BrushStyle.NoBrush);painter.drawRoundedRect(rect,3,3)
        painter.setPen(QPen(QColor(c['action'] if self.isChecked() else c['muted']),1))
        painter.setBrush(QColor(c['action'] if self.isChecked() else c['canvas']));painter.drawEllipse(QRectF(2,106,12,12))
        if self.isChecked():
            painter.setPen(Qt.PenStyle.NoPen);painter.setBrush(QColor(c['action_text']));painter.drawEllipse(QRectF(6,110,4,4))
        font=self.font();font.setPixelSize(12);painter.setFont(font);painter.setPen(QColor(c['text']))
        painter.drawText(self.rect().adjusted(22,102,-1,0),Qt.AlignmentFlag.AlignVCenter,self.text())
        if self.hasFocus():
            painter.setPen(QPen(QColor(c['focus']),1,Qt.PenStyle.DotLine));painter.setBrush(Qt.BrushStyle.NoBrush);painter.drawRect(self.rect().adjusted(0,0,-1,-1))


class DayNightCard(QWidget):
    def __init__(self,period):
        super().__init__();self.period=period;layout=QVBoxLayout(self);layout.setContentsMargins(0,0,0,0);layout.setSpacing(12)
        row=QHBoxLayout();row.setSpacing(7)
        self.glyph=QLabel();self.glyph.setFixedSize(18,18);row.addWidget(self.glyph)
        title=QLabel('Day Theme' if period=='day' else 'Night Theme');title.setObjectName('settingsSection');row.addWidget(title)
        self.active=QLabel('ACTIVE');self.active.setObjectName('themeActive');row.addWidget(self.active);row.addStretch();layout.addLayout(row)
        description=QLabel('Used when your system is in light mode.' if period=='day' else 'Used when your system is in dark mode.')
        description.setObjectName('settingsDescription');description.setWordWrap(True);layout.addWidget(description)
        self.preview=ThemePreview('light' if period=='day' else 'dark');layout.addWidget(self.preview)
        self.select=EditorSelect();self.select.setAccessibleName('Day theme' if period=='day' else 'Night theme')
        for theme in THEMES.values():self.select.addItem(theme.name,theme.id)
        layout.addWidget(self.select)
    def refresh(self,theme_id,active):
        with QSignalBlocker(self.select):self.select.setCurrentIndex(self.select.findData(theme_id))
        self.preview.theme_id=theme_id;self.preview.update();self.active.setVisible(active)
        self.glyph.setPixmap(icon('sun' if self.period=='day' else 'moon').pixmap(18,18))


class ThemesPage(QWidget):
    def __init__(self,overlay):
        super().__init__();self.controller=overlay.window.theme_controller;self.setObjectName('themesSettings')
        layout=QVBoxLayout(self);layout.setContentsMargins(46,36,46,24);layout.setSpacing(0)
        title=QLabel('Themes');title.setObjectName('settingsHeading');layout.addWidget(title);layout.addSpacing(14)
        description=QLabel('Choose a theme for your workspace. Sync with your system or select your preferred appearance.')
        description.setWordWrap(True);description.setObjectName('settingsDescription');layout.addWidget(description);layout.addSpacing(24)
        row=QHBoxLayout();row.setSpacing(20);label=QLabel('Theme selection');label.setObjectName('settingsSection');row.addWidget(label);row.addStretch()
        self.system=SettingsRadio('Sync with system');self.manual=SettingsRadio('Manual');self.modes=QButtonGroup(self)
        for mode,button in [('system',self.system),('manual',self.manual)]:
            self.modes.addButton(button);row.addWidget(button);button.clicked.connect(lambda checked,mode=mode:self.save(mode=mode))
        layout.addLayout(row);layout.addSpacing(18)
        line=QFrame();line.setFixedHeight(1);set_style(line,'background:#363636;border:0;');layout.addWidget(line);layout.addSpacing(24)
        self.pages=QStackedWidget();self.pages.setSizePolicy(QSizePolicy.Policy.Expanding,QSizePolicy.Policy.Maximum)
        self.manual_page=QWidget();grid=QGridLayout(self.manual_page);grid.setContentsMargins(0,0,0,0);grid.setHorizontalSpacing(20);grid.setVerticalSpacing(19)
        self.choices={};self.group=QButtonGroup(self)
        for index,theme in enumerate(THEMES.values()):
            button=ThemeChoice(theme.id);self.choices[theme.id]=button;self.group.addButton(button);grid.addWidget(button,index//3,index%3)
            button.clicked.connect(lambda checked,theme_id=theme.id:self.save(manual=theme_id))
        self.system_page=QWidget();cards=QHBoxLayout(self.system_page);cards.setContentsMargins(0,0,0,0);cards.setSpacing(24)
        self.day=DayNightCard('day');self.night=DayNightCard('night')
        for card in (self.day,self.night):
            cards.addWidget(card,1);card.select.currentIndexChanged.connect(lambda index,card=card:self.save(**{card.period:card.select.itemData(index)}))
        self.pages.addWidget(self.manual_page);self.pages.addWidget(self.system_page);layout.addWidget(self.pages)
        layout.addSpacing(12);self.status=QLabel(self.controller.preferences.error);self.status.setObjectName('proxyValidation');self.status.setWordWrap(True);layout.addWidget(self.status);layout.addStretch()
        self.controller.changed.connect(self.refresh);self.refresh()
    def save(self,**values):
        try:self.controller.set_config(replace(self.controller.config,**values));self.status.clear()
        except Exception as error:self.status.setText(str(error));self.refresh()
    def refresh(self):
        config=self.controller.config
        for button,checked in ((self.manual,config.mode=='manual'),(self.system,config.mode=='system')):
            with QSignalBlocker(button):button.setChecked(checked)
        for key,button in self.choices.items():
            with QSignalBlocker(button):button.setChecked(key==config.manual)
            button.update()
        self.pages.setCurrentIndex(0 if config.mode=='manual' else 1)
        self.manual_page.setSizePolicy(QSizePolicy.Policy.Expanding,QSizePolicy.Policy.Preferred if config.mode=='manual' else QSizePolicy.Policy.Ignored)
        self.system_page.setSizePolicy(QSizePolicy.Policy.Expanding,QSizePolicy.Policy.Preferred if config.mode=='system' else QSizePolicy.Policy.Ignored)
        self.day.refresh(config.day,not self.controller.system_dark);self.night.refresh(config.night,self.controller.system_dark)
        self.pages.updateGeometry()
