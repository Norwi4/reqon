"""Apply the selected palette to native widgets, custom painting and live editors."""
from contextlib import ExitStack
from functools import lru_cache
import re
from PySide6.QtCore import QObject,Signal,Qt,QTimer,QSignalBlocker,QEvent,QCoreApplication
from PySide6.QtGui import QColor,QPalette,QSyntaxHighlighter
from PySide6.QtWidgets import QApplication,QPlainTextEdit,QTextEdit,QWidget
from ui_palettes import THEMES,current_theme,activate_theme,theme_color,render_style
from theme_settings import ThemePreferences


def theme_qcolor(value):return QColor(theme_color(value))


def set_style(widget,source,literals=()):
    widget._theme_style_source=source;widget._theme_style_literals=literals
    rendered=render_style(source,literals)
    if widget.styleSheet()!=rendered:widget.setStyleSheet(rendered)


def set_rich_text(label,source):
    label._theme_rich_text=source;label.setText(render_rich_text(source))


def render_rich_text(source):
    return re.sub(r'style="([^"]*)"',lambda match:'style="'+render_style(match[1])+'"',source)


def application_palette():
    c=current_theme().colors;palette=QPalette();roles={
        'Window':c['canvas'],'WindowText':c['text'],'Base':c['canvas'],'AlternateBase':c['panel'],
        'Text':c['text'],'Button':c['panel'],'ButtonText':c['text'],'ToolTipBase':c['panel'],'ToolTipText':c['text'],
        'Highlight':c['selected'],'HighlightedText':c['text'],'PlaceholderText':c['subtle'],
        'Link':c['action'],'LinkVisited':c['accent'],'Light':c['border'],'Midlight':c['border'],'Mid':c['border'],'Dark':c['border'],
    }
    for name,value in roles.items():palette.setColor(getattr(QPalette.ColorRole,name),QColor(value))
    for name in ('WindowText','Text','ButtonText'):palette.setColor(QPalette.ColorGroup.Disabled,getattr(QPalette.ColorRole,name),QColor(c['subtle']))
    return palette


@lru_cache(maxsize=1)
def theme_stylesheet():
    """Compile once: changing a palette must not rebuild the whole widget tree."""
    from ui_theme import STYLE
    sheets=[]
    for theme_id in THEMES:
        root=f'QWidget[reqonTheme="{theme_id}"]'
        def scope(match):
            selectors=[]
            for selector in match[1].split(','):
                selector=selector.strip()
                selectors.append(root+' '+selector)
                if selector in ('QWidget','QMainWindow','QDialog'):
                    selectors.append(selector+f'[reqonTheme="{theme_id}"]')
            return ', '.join(selectors)+' {'
        sheets.append(re.sub(r'([^{}]+)\{',scope,render_style(STYLE,theme_id=theme_id)))
    return '\n'.join(sheets)


def polish_theme(widget):
    # Public Qt polish invalidates this widget's cached selectors. The stylesheet
    # itself stays unchanged, avoiding QApplication's recursive repolish of every
    # cached ancestor and descendant. Formatting is never a user edit.
    with QSignalBlocker(widget):
        widget.style().polish(widget)
        QCoreApplication.sendEvent(widget,QEvent(QEvent.Type.StyleChange))
    widget._reqon_theme_id=current_theme().id
    widget.update()


class ThemeShowFilter(QObject):
    def eventFilter(self,widget,event):
        kind=event.type()
        if kind==QEvent.Type.Polish and isinstance(widget,QWidget):
            widget._reqon_theme_id=current_theme().id
        elif kind==QEvent.Type.Show and isinstance(widget,QWidget):
            if getattr(widget,'_reqon_theme_id',None)!=current_theme().id:polish_theme(widget)
        return False


class ThemeController(QObject):
    changed=Signal()
    def __init__(self,window):
        super().__init__(window);self.window=window;self.preferences=ThemePreferences(window.store);self._applying=False;self._applied_id=None
        # Application-wide QSS repolishes every cached widget AND its subtree.
        # A window-scoped sheet has the same inheritance without that repeated walk.
        app=QApplication.instance()
        if app.styleSheet():app.setStyleSheet('')
        if not hasattr(app,'reqon_theme_filter'):
            app.reqon_theme_filter=ThemeShowFilter(app);app.installEventFilter(app.reqon_theme_filter)
        self.hints=QApplication.styleHints();self.system_dark=self.hints.colorScheme()==Qt.ColorScheme.Dark
        self.hints.colorSchemeChanged.connect(self.system_changed)
        self.apply()
    @property
    def config(self):return self.preferences.config
    def set_config(self,config):
        self.preferences.save(config);self.apply()
    def system_changed(self,scheme):
        self.system_dark=scheme==Qt.ColorScheme.Dark
        # Qt updates its system palette after this notification. Apply on the next turn.
        QTimer.singleShot(0,self.restore_system_palette)
    def restore_system_palette(self):self.apply(force=True)
    def apply(self,force=False):
        if self._applying:return
        theme_id=self.config.effective(self.system_dark)
        if not force and theme_id==self._applied_id:self.changed.emit();return
        self._applying=True
        enabled=self.window.updatesEnabled();self.window.setUpdatesEnabled(False)
        try:
            activate_theme(theme_id);app=QApplication.instance()
            from ui_state_widgets import StateIllustration
            widgets=app.allWidgets()
            # Syntax and font reformatting must not mark requests as edited.
            with ExitStack() as stack:
                for widget in widgets:
                    if isinstance(widget,(QPlainTextEdit,QTextEdit)):stack.enter_context(QSignalBlocker(widget))
                palette=application_palette()
                if app.palette()!=palette:app.setPalette(palette)
                rendered=theme_stylesheet()
                roots={self.window,*(widget for widget in app.topLevelWidgets() if hasattr(widget,'theme_controller'))}
                for root in roots:
                    root.setProperty('reqonTheme',theme_id)
                    if root.styleSheet()!=rendered:root.setStyleSheet(rendered)
                # Children are not walked recursively here. Hidden controls pick up
                # the current palette before their next Show event and first paint.
                for widget in widgets:
                    if widget.isVisible():polish_theme(widget)
                for widget in widgets:
                    if hasattr(widget,'_theme_style_source'):
                        rendered=render_style(widget._theme_style_source,widget._theme_style_literals)
                        if widget.styleSheet()!=rendered:widget.setStyleSheet(rendered)
                    if hasattr(widget,'_theme_rich_text'):widget.setText(render_rich_text(widget._theme_rich_text))
                    if isinstance(widget,StateIllustration):widget.set_state(widget.state)
                    refresh=getattr(widget,'refresh_theme',None)
                    if refresh is not None:refresh()
                    presentation=getattr(widget,'presentation',None)
                    if presentation is not None:presentation.highlight()
                    highlighter=getattr(widget,'highlighter',None)
                    if isinstance(highlighter,QSyntaxHighlighter):
                        if hasattr(widget,'refresh_syntax_theme'):widget.refresh_syntax_theme()
                        else:highlighter.rehighlight()
            if hasattr(self.window,'environments'):self.window.environments.selection_changed()
            self._applied_id=theme_id
            self.changed.emit()
        finally:self._applying=False;self.window.setUpdatesEnabled(enabled)
