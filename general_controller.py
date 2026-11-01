"""Connect General preferences to existing tabs, editors and local persistence."""
from PySide6.QtCore import Qt,QSignalBlocker
from PySide6.QtGui import QIcon
from PySide6.QtWidgets import QApplication,QLineEdit,QMessageBox
from general_settings import GeneralPreferences,effective_request
from ui_typography import apply_editor_font


class GeneralController:
    def init_general_settings(self):
        from http_runtime import CookieStore
        self.general_preferences=GeneralPreferences(self.store);self.cookie_store=CookieStore(self.store.path.parent)
        QApplication.instance().reqon_general=self.general_preferences.config
        self._previous_general=self.general_preferences.config

    def set_request_tab_icon(self,page):
        from ui_components import icon
        index=self.workbench.indexOf(page)
        if index<0:return
        if not self.general_preferences.config.tab_icons:self.workbench.setTabIcon(index,QIcon());return
        kind=getattr(page,'kind',None) or ('environment' if getattr(page,'is_environment_page',False) else getattr(page,'entity_key',('http',))[0])
        self.workbench.setTabIcon(index,icon(kind,'#7ca8d9' if kind=='grpc' else '#79cdb4' if kind=='http' else '#aab2bd'))

    def configure_general_page(self,page):
        from ui_pages import GrpcPage
        from ui_components import CodeEditor,KeyValueTable
        from ui_editor_behavior import VariableCompletion
        data=page.data;legacy={'tls_verify':False,'grpc_timeout_ms':0,'grpc_max_response_mb':10} if isinstance(page,GrpcPage) else {'http_verify':True,'http_timeout':30,'http_follow_redirects':True}
        override=data.get('general_overrides',any(key in data and data[key]!=value for key,value in legacy.items()))
        page.general_overrides.setChecked(override)
        page.general_overrides.toggled.connect(lambda:self.apply_general_page(page))
        page.runtime_data_provider=lambda:effective_request(page.collect(),self.general_preferences.config,self.store.path.parent)
        provider=lambda:self.variables(page.collection_id)
        for editor in [*page.findChildren(CodeEditor),*[getattr(page,name) for name in ('url','server','auth_value') if hasattr(page,name)]]:
            if isinstance(editor,QLineEdit) or not editor.isReadOnly():editor.completion=VariableCompletion(editor,provider)
        for table in page.findChildren(KeyValueTable):table.variables_provider=provider
        page.response.editor.link_callback=lambda url:self.open_response_link(page,url)
        self.apply_general_page(page)
        names=('tls_verify','grpc_timeout_ms','grpc_max_response_mb') if isinstance(page,GrpcPage) else ('http_verify','http_timeout','http_follow_redirects','http_version')
        for name in names:
            field=getattr(page,name)
            signal=field.toggled if hasattr(field,'toggled') else field.currentIndexChanged if hasattr(field,'currentIndexChanged') else field.valueChanged if hasattr(field,'valueChanged') else field.textChanged
            signal.connect(lambda *_,page=page:page.general_overrides.setChecked(True))

    def apply_general_page(self,page):
        from ui_pages import RequestPage,GrpcPage
        config=self.general_preferences.config
        if isinstance(page,(RequestPage,GrpcPage)):
            if isinstance(page,RequestPage):
                fields=[(page.http_verify,config.verify_ssl),(page.http_timeout,config.timeout_ms/1000),(page.http_follow_redirects,config.follow_redirects),(page.http_version,config.http_version)]
            else:fields=[(page.tls_verify,config.verify_ssl),(page.grpc_timeout_ms,config.timeout_ms),(page.grpc_max_response_mb,config.max_response_mb)]
            override=page.general_overrides.isChecked()
            for field,value in fields:
                field.setEnabled(override)
                if not override:
                    with QSignalBlocker(field):
                        if hasattr(field,'setChecked'):field.setChecked(value)
                        elif hasattr(field,'setCurrentText'):field.setCurrentText(value)
                        else:field.setValue(value)
            orientation=Qt.Orientation.Horizontal if config.two_pane else Qt.Orientation.Vertical
            if page.split.orientation()!=orientation:page.split.setOrientation(orientation);page.split.setSizes([1,1])
            page.split.handle(1).setCursor(Qt.CursorShape.SplitHCursor if config.two_pane else Qt.CursorShape.SplitVCursor)
            page.response.format_config=config
            if hasattr(page,'last_response'):page.response.show_response(page.last_response)
        from ui_components import CodeEditor
        from ui_response_error import ErrorDetails
        for editor in [*page.findChildren(CodeEditor),*page.findChildren(ErrorDetails)]:
            apply_editor_font(editor,config)
            if hasattr(editor,'presentation'):editor.presentation.validate()
            if not config.autocomplete and hasattr(editor,'completion'):editor.completion.completer.popup().hide()
        if hasattr(page,'description') and hasattr(page.description,'set_mode'):page.description.set_mode(config.documentation_editor)
        self.set_request_tab_icon(page)

    def general_settings_changed(self):
        config=self.general_preferences.config;previous=self._previous_general;self._previous_general=config
        network_changed=any(getattr(config,key)!=getattr(previous,key) for key in ('verify_ssl','tls_keylog','timeout_ms','max_response_mb'))
        QApplication.instance().reqon_general=config
        for index in range(self.workbench.count()):
            page=self.workbench.widget(index);self.apply_general_page(page)
            if network_changed and hasattr(page,'source_changed'):page.source_changed()
            if config.autosave and not previous.autosave:self.flush_page(page,force=True)
        for editor in self.snippet_panel.findChildren(__import__('ui_components').CodeEditor):apply_editor_font(editor,config)
        from ui_language import apply_language
        apply_language(self,config.language)
        index=self.env_combo.findData(None)
        if index>=0:self.env_combo.setItemText(index,'Без окружения' if config.language=='Русский' else 'No Environment')

    def close_general_page(self,page):
        config=self.general_preferences.config
        if not hasattr(page,'_draft_id') or not hasattr(page,'collect'):return self.flush_page(page,force=True)
        dirty=page.collect()!=getattr(page,'_saved',None)
        if config.autosave:return self.flush_page(page,force=True)
        if dirty and config.ask_unsaved:
            answer=QMessageBox.question(self,'Unsaved changes','Save changes before closing this tab?',QMessageBox.StandardButton.Save|QMessageBox.StandardButton.Discard|QMessageBox.StandardButton.Cancel)
            if answer==QMessageBox.StandardButton.Cancel:return False
            if answer==QMessageBox.StandardButton.Save:return self.flush_page(page,force=True)
        if dirty:
            timer=getattr(page,'_autosave_timer',None)
            if timer:timer.stop()
            if getattr(page,'request_id',None):
                item=self.tree_item('request',page.request_id)
                record=self.store.all('SELECT name FROM requests WHERE id=?',(page.request_id,))
                if item and record:
                    with QSignalBlocker(self.tree):item.setText(0,record[0][0])
        return True

    def finish_sidebar_open(self,page):
        self.apply_general_page(page)
        if not getattr(page,'_preview_watched',False):
            page._preview_watched=True
            for name in ('description','title','name','variables'):
                control=getattr(page,name,None)
                signal=getattr(control,'textEdited',None) or getattr(control,'textChanged',None) or getattr(control,'changed',None)
                if signal is not None:signal.connect(lambda *_,page=page:setattr(page,'_preview_pinned',True))
        if self._restoring_workspace or self.general_preferences.config.new_tab:return page
        for index in range(self.workbench.count()-1,-1,-1):
            previous=self.workbench.widget(index)
            if previous is page or not getattr(previous,'_sidebar_preview',False) or getattr(previous,'_preview_pinned',False) or previous.busy:continue
            if self.confirm_close(previous):self.workbench.removeTab(index);previous.deleteLater()
        page._sidebar_preview=True;self.save_workspace();return page

    def open_response_link(self,page,url):
        from urllib.parse import urljoin,urlsplit
        url=urljoin(getattr(page,'last_response',{}).get('url',page.collect().get('url','')),url)
        if urlsplit(url).scheme not in ('http','https'):return
        data={'name':'Linked request','method':'GET','url':url}
        if self.general_preferences.config.retain_headers:
            old=page.collect();data.update(header_rows=old.get('header_rows',[]),auth_type=old.get('auth_type','No Auth'),auth_value=old.get('auth_value',''))
        return self.new_request(data)
