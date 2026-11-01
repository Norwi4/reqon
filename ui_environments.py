"""Local Windows environment tabs, sidebar actions and selection colors."""
import json
from pathlib import Path
from PySide6.QtCore import Qt,QSize,QPoint,QRect,QTimer,QObject,Signal
from PySide6.QtGui import QColor,QPainter,QPen,QPixmap,QIcon,QKeySequence
from PySide6.QtWidgets import (QWidget,QVBoxLayout,QHBoxLayout,QLabel,QLineEdit,QToolButton,QPushButton,
    QListWidget,QListWidgetItem,QStyledItemDelegate,QStyle,QDialog,QColorDialog,QMenu,QFileDialog,
    QInputDialog,QMessageBox,QTabBar,QSizePolicy,QStylePainter,QStyleOptionComboBox)
from ui_brand import COLORS as C
from ui_components import icon,tool,Select
from ui_environment_rows import VariablesEditor
from environment_store import EnvironmentStore,active_values

ROLE=Qt.ItemDataRole.UserRole
ENV_COLORS=('#b3261e','#008c45','#0668d2','#c99a00','#a31578','#008697')


class EnvironmentCombo(Select):
    def paintEvent(self,event):
        painter=QStylePainter(self);option=QStyleOptionComboBox();self.initStyleOption(option)
        field=self.style().subControlRect(QStyle.ComplexControl.CC_ComboBox,option,QStyle.SubControl.SC_ComboBoxEditField,self)
        width=field.width()-4-(option.iconSize.width()+5 if not option.currentIcon.isNull() else 0)
        option.currentText=self.fontMetrics().elidedText(option.currentText,Qt.TextElideMode.ElideRight,max(0,width))
        painter.drawComplexControl(QStyle.ComplexControl.CC_ComboBox,option);painter.drawControl(QStyle.ControlElement.CE_ComboBoxLabel,option)
        icon('chevron',size=12).paint(painter,QRect(self.width()-19,(self.height()-12)//2,12,12))


def color_icon(color,size=16):
    pixmap=QPixmap(size*2,size*2);pixmap.fill(Qt.GlobalColor.transparent);pixmap.setDevicePixelRatio(2)
    painter=QPainter(pixmap);painter.setRenderHint(QPainter.RenderHint.Antialiasing)
    painter.setPen(QPen(QColor(color or C['subtle']),1,Qt.PenStyle.SolidLine if color else Qt.PenStyle.DashLine))
    painter.setBrush(QColor(color) if color else Qt.BrushStyle.NoBrush)
    if color:painter.drawRoundedRect(3,3,size-6,size-6,2,2)
    else:painter.drawEllipse(3,3,size-6,size-6)
    painter.end();return QIcon(pixmap)


class ApplyButton(QToolButton):
    def __init__(self,parent=None):
        super().__init__(parent);self.applied=False;self.setFixedSize(26,26);self.setAccessibleName('Apply environment')
    def paintEvent(self,event):
        p=QPainter(self);p.setRenderHint(QPainter.RenderHint.Antialiasing)
        if self.underMouse():p.fillRect(self.rect(),QColor(C['hover']))
        p.setPen(QPen(QColor(C['muted']),1.2));p.setBrush(QColor(C['muted']) if self.applied else Qt.BrushStyle.NoBrush)
        p.drawEllipse(6,6,14,14);p.setPen(QPen(QColor(C['canvas'] if self.applied else C['muted']),1.4))
        p.drawLine(9,13,12,16);p.drawLine(12,16,17,10)


class ColorPopup(QDialog):
    selected=Signal(str)
    def __init__(self,parent,color):
        super().__init__(parent,Qt.WindowType.Popup|Qt.WindowType.FramelessWindowHint);self.setObjectName('quickSearch')
        layout=QVBoxLayout(self);layout.setContentsMargins(12,12,12,10);layout.setSpacing(8)
        colors=QHBoxLayout();colors.setSpacing(4)
        for value in ('',*ENV_COLORS):
            button=QToolButton();button.setFixedSize(24,24);button.setIcon(color_icon(value,20));button.setIconSize(QSize(20,20))
            button.setToolTip(value or 'No color');button.setAccessibleName(value or 'No color');button.setCheckable(True);button.setChecked(value==color)
            button.clicked.connect(lambda checked=False,value=value:self.choose(value));colors.addWidget(button)
        layout.addLayout(colors);custom=tool('plus','Custom color',self.custom);layout.addWidget(custom,0,Qt.AlignmentFlag.AlignLeft)
        self.setStyleSheet(f'QDialog {{background:{C["panel"]};border:1px solid {C["border"]};border-radius:5px;}} QToolButton:checked {{border:1px solid {C["text"]};}}')
    def choose(self,color):self.selected.emit(color);self.close()
    def custom(self):
        self.hide();color=QColorDialog.getColor(parent=self.parentWidget(),title='Environment color')
        if color.isValid():self.selected.emit(color.name())
        self.close()


class EnvironmentDelegate(QStyledItemDelegate):
    def paint(self,painter,option,index):
        owner=self.parent();id=index.data(ROLE);color=index.data(int(ROLE)+1);selected=bool(option.state&QStyle.StateFlag.State_Selected)
        hovered=owner.hover_id==id;painter.save()
        if selected:painter.fillRect(option.rect,QColor(C['selected']))
        elif hovered:painter.fillRect(option.rect,QColor(C['hover']))
        if color or hovered:color_icon(color).paint(painter,QRect(option.rect.left()+14,option.rect.center().y()-8,16,16))
        font=option.font;font.setWeight(font.Weight.Medium if selected else font.Weight.Normal);painter.setFont(font);painter.setPen(QColor(C['text']))
        reserve=65 if hovered else 38 if id==owner.controller.window.env_combo.currentData() else 10
        rect=option.rect.adjusted(36,0,-reserve,0);text=painter.fontMetrics().elidedText(index.data(),Qt.TextElideMode.ElideRight,max(0,rect.width()))
        painter.drawText(rect,Qt.AlignmentFlag.AlignVCenter,text)
        if id==owner.controller.window.env_combo.currentData() and not hovered:
            x=option.rect.right()-34;y=option.rect.center().y()-7
            painter.setRenderHint(QPainter.RenderHint.Antialiasing);painter.setPen(Qt.PenStyle.NoPen);painter.setBrush(QColor(C['muted']))
            painter.drawEllipse(x,y,14,14);painter.setPen(QPen(QColor(C['canvas']),1.4))
            painter.drawLine(x+3,y+7,x+6,y+10);painter.drawLine(x+6,y+10,x+11,y+4)
        painter.restore()
    def sizeHint(self,option,index):return QSize(100,30)


class EnvironmentList(QListWidget):
    def __init__(self,controller):
        super().__init__();self.controller=controller;self.hover_id=None;self.setObjectName('environmentList');self.setMouseTracking(True)
        self.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff);self.setItemDelegate(EnvironmentDelegate(self))
        self.setStyleSheet(f'QListWidget {{border:0;background:transparent;outline:none;}}')
        self.color=tool('environment','Set environment color',self.choose_color,parent=self.viewport());self.color.setFixedSize(26,26)
        self.apply=ApplyButton(self.viewport());self.apply.clicked.connect(self.toggle_apply)
        self.more=tool('more','More actions',self.show_menu,parent=self.viewport());self.more.setFixedSize(26,26)
        self.clear_hover();self.itemClicked.connect(lambda item:controller.open(item.data(ROLE)))
        self.setContextMenuPolicy(Qt.ContextMenuPolicy.CustomContextMenu);self.customContextMenuRequested.connect(self.context_menu)
        self.verticalScrollBar().valueChanged.connect(self.clear_hover)
    def current_id(self):return self.currentItem().data(ROLE) if self.currentItem() else None
    def clear_hover(self,*_):
        self.hover_id=None
        for button in (self.color,self.apply,self.more):button.hide()
        self.viewport().update()
    def mouseMoveEvent(self,event):
        self.show_hover(self.itemAt(event.position().toPoint()));super().mouseMoveEvent(event)
    def show_hover(self,item):
        self.clear_hover()
        if not item:return
        self.hover_id=item.data(ROLE);rect=self.visualItemRect(item)
        self.color.setIcon(color_icon(item.data(int(ROLE)+1)));self.color.move(9,rect.y()+2)
        self.apply.applied=self.controller.window.env_combo.currentData()==self.hover_id
        self.apply.setToolTip('Deactivate environment' if self.apply.applied else 'Set as active environment')
        self.apply.move(self.viewport().width()-57,rect.y()+2);self.more.move(self.viewport().width()-29,rect.y()+2)
        for button in (self.color,self.apply,self.more):button.show();button.raise_()
        self.viewport().update()
    def leaveEvent(self,event):
        if not any(button.underMouse() for button in (self.color,self.apply,self.more)):self.clear_hover()
        super().leaveEvent(event)
    def resizeEvent(self,event):super().resizeEvent(event);self.clear_hover()
    def choose_color(self):
        if self.hover_id is not None:self.controller.show_colors(self.hover_id,self.color)
    def toggle_apply(self):
        if self.hover_id is not None:self.controller.apply(None if self.apply.applied else self.hover_id)
    def show_menu(self):
        if self.hover_id is not None:self.controller.show_menu(self.hover_id,self.more.mapToGlobal(QPoint(0,26)))
    def context_menu(self,position):
        item=self.itemAt(position)
        if item:self.controller.show_menu(item.data(ROLE),self.viewport().mapToGlobal(position))
    def keyPressEvent(self,event):
        id=self.current_id()
        if id is not None:
            if event.key() in (Qt.Key.Key_Return,Qt.Key.Key_Enter):self.controller.open(id);return
            if event.key()==Qt.Key.Key_F2 or QKeySequence(event.keyCombination())==QKeySequence('Ctrl+E'):self.controller.rename(id);return
            if QKeySequence(event.keyCombination())==QKeySequence('Ctrl+D'):self.controller.duplicate(id);return
            if event.key()==Qt.Key.Key_Delete:self.controller.delete(id);return
        super().keyPressEvent(event)


class EnvironmentSidebar(QWidget):
    def __init__(self,controller):
        super().__init__();layout=QVBoxLayout(self);layout.setContentsMargins(0,0,0,0);layout.setSpacing(0)
        bar=QHBoxLayout();bar.setContentsMargins(8,8,8,8);bar.setSpacing(4)
        bar.addWidget(tool('plus','New environment',controller.create));self.search=QLineEdit();self.search.setObjectName('search')
        self.search.setPlaceholderText('Search environments');self.search.setFixedHeight(26);self.search.addAction(icon('search',size=14),QLineEdit.ActionPosition.LeadingPosition)
        bar.addWidget(self.search,1);layout.addLayout(bar)
        self.globals=QPushButton('Globals');self.globals.setFixedHeight(48);self.globals.clicked.connect(lambda:controller.open(None))
        self.globals.setStyleSheet(f'text-align:left;padding-left:36px;border:0;border-bottom:1px solid {C["border"]};border-radius:0;background:transparent;')
        layout.addWidget(self.globals);layout.addSpacing(8);self.list=EnvironmentList(controller);layout.addWidget(self.list,1)
        self.search.textChanged.connect(self.filter)
    def filter(self,text):
        for i in range(self.list.count()):self.list.item(i).setHidden(text.casefold() not in self.list.item(i).text().casefold())


class EnvironmentPage(QWidget):
    def __init__(self,controller,id):
        super().__init__();self.controller=controller;self.environment_id=id;self.is_environment_page=True;self.busy=False;self.deleted=False
        data=controller.data.get(id);self.uid=data['uid'];self.color=data['color'];self.setObjectName('page')
        layout=QVBoxLayout(self);layout.setContentsMargins(0,0,0,0);layout.setSpacing(0)
        head=QHBoxLayout();head.setContentsMargins(16,8,16,12);head.setSpacing(4)
        self.color_button=tool('environment','Set environment color',lambda:controller.show_colors(id,self.color_button));self.color_button.setFixedSize(24,28)
        self.color_button.setIcon(color_icon(self.color));self.color_button.setVisible(id is not None);head.addWidget(self.color_button)
        self.name=QLineEdit(data['name']);self.name.setObjectName('requestName');self.name.setReadOnly(id is None);head.addWidget(self.name,1)
        self.save_state=QLabel('Saved locally');self.save_state.setObjectName('muted');head.addWidget(self.save_state)
        self.more=tool('more','Environment actions',lambda:controller.show_menu(id,self.more.mapToGlobal(QPoint(0,28))));head.addWidget(self.more)
        layout.addLayout(head)
        self.rows=VariablesEditor(data['rows']);layout.addWidget(self.rows,1)
        layout.setContentsMargins(0,0,24,12)
        self._autosave_timer=QTimer(self);self._autosave_timer.setSingleShot(True);self._autosave_timer.setInterval(250);self._autosave_timer.timeout.connect(self.flush)
        self.name.textChanged.connect(self.changed);self.rows.changed.connect(self.changed)
    def collect(self):return {'name':self.name.text(),'rows':self.rows.data(),'color':self.color}
    def changed(self,*_):
        if self.deleted:return
        self.controller.window.set_save_status(self,'Saving…');self.controller.update_names(self);self._autosave_timer.start()
        if self.environment_id is None or self.environment_id==self.controller.window.env_combo.currentData():self.controller.invalidate()
    def flush(self):
        self._autosave_timer.stop()
        if self.deleted:return True
        try:
            self.controller.data.save(self.environment_id,self.name.text(),self.rows.data(),self.color)
            self.controller.window.set_save_status(self,'Saved locally');self.controller.update_names(self);return True
        except Exception as error:self.controller.window.set_save_status(self,'Autosave failed: '+str(error),True);return False


class EnvironmentController(QObject):
    def __init__(self,window):
        super().__init__(window);self.window=window;self.data=EnvironmentStore(window.store);self.sidebar=EnvironmentSidebar(self)
        window.side_tabs.addTab(self.sidebar,'Environments');self.popup=None;self.menu=None
        window.env_combo.currentIndexChanged.connect(self.selection_changed)
        window.workbench.currentChanged.connect(self.tab_changed)
    def pages(self):return [self.window.workbench.widget(i) for i in range(self.window.workbench.count()) if getattr(self.window.workbench.widget(i),'is_environment_page',False)]
    def page(self,id):return next((page for page in self.pages() if page.environment_id==id),None)
    def values(self,id):
        page=self.page(id);return active_values(page.rows.data() if page else self.data.get(id)['rows'])
    def show_sidebar(self):
        w=self.window;w.side_tabs.setCurrentWidget(self.sidebar);w.collection_searchbar.hide();w.rail_buttons['Environments'].setChecked(True)
    def navigate(self):
        self.show_sidebar();records=self.data.all();selected=self.sidebar.list.current_id()
        if selected is None:selected=self.window.env_combo.currentData()
        if selected is None and records:selected=records[0]['id']
        if selected is None:self.create()
        else:self.open(selected)
    def open(self,id):
        self.show_sidebar();page=self.page(id)
        if page is None:
            page=EnvironmentPage(self,id);w=self.window;index=w.workbench.addTab(page,page.name.text());w.workbench.setTabIcon(index,icon('environment'))
            close=tool('close','Close environment',lambda:w.close_request_tab(w.workbench.indexOf(page)))
            w.workbench.tabBar().setTabButton(index,QTabBar.ButtonPosition.RightSide,close)
        self.window.workbench.setCurrentWidget(page);self.select_item(id);self.window.save_workspace();return self.window.finish_sidebar_open(page)
    def select_item(self,id):
        listing=self.sidebar.list
        for i in range(listing.count()):
            if listing.item(i).data(ROLE)==id:listing.setCurrentItem(listing.item(i));return
        listing.clearSelection()
    def create(self):
        id=self.data.create();self.refresh();page=self.open(id);page.name.setFocus();page.name.selectAll();return page
    def refresh(self):
        w=self.window;combo=w.env_combo;selected=combo.currentData();current=self.sidebar.list.current_id();records=self.data.all()
        combo.blockSignals(True);combo.clear();combo.addItem('No Environment',None)
        self.sidebar.list.clear_hover();self.sidebar.list.clear()
        for data in records:
            combo.addItem(color_icon(data['color']),data['name'],data['id'])
            item=QListWidgetItem(data['name']);item.setData(ROLE,data['id']);item.setData(int(ROLE)+1,data['color']);item.setToolTip(data['name']);self.sidebar.list.addItem(item)
        combo.setCurrentIndex(max(0,combo.findData(selected)));combo.blockSignals(False)
        self.select_item(current);self.sidebar.filter(self.sidebar.search.text());self.selection_changed()
        if selected!=combo.currentData():combo.currentIndexChanged.emit(combo.currentIndex())
    def selection_changed(self,*_):
        combo=self.window.env_combo;id=combo.currentData();color=self.data.get(id)['color'] if id is not None else ''
        background=QColor(color).darker(145).name() if color else C['canvas']
        combo.setStyleSheet(f'QComboBox {{border:1px solid {color or "transparent"};border-radius:4px;background:{background};color:{C["text"]};font-size:12px;padding:5px 20px 5px 8px;}}')
        combo.setToolTip(combo.currentText());self.sidebar.list.clear_hover();self.sidebar.list.viewport().update()
    def apply(self,id):
        self.window.env_combo.setCurrentIndex(max(0,self.window.env_combo.findData(id)));self.window.save_workspace()
    def update_names(self,page):
        name=page.name.text().strip() or 'New Environment';w=self.window;index=w.workbench.indexOf(page)
        if index>=0:w.workbench.setTabText(index,name)
        combo_index=w.env_combo.findData(page.environment_id)
        if page.environment_id is not None and combo_index>=0:w.env_combo.setItemText(combo_index,name)
        for i in range(self.sidebar.list.count()):
            item=self.sidebar.list.item(i)
            if item.data(ROLE)==page.environment_id:item.setText(name);item.setToolTip(name)
        self.sidebar.filter(self.sidebar.search.text())
    def set_color(self,id,color):
        page=self.page(id)
        if page:
            page.color=color;page.color_button.setIcon(color_icon(color));page.changed();page.flush()
        else:
            data=self.data.get(id);self.data.save(id,data['name'],data['rows'],color)
        index=self.window.env_combo.findData(id)
        if index>=0:self.window.env_combo.setItemIcon(index,color_icon(color))
        for i in range(self.sidebar.list.count()):
            item=self.sidebar.list.item(i)
            if item.data(ROLE)==id:item.setData(int(ROLE)+1,color)
        self.selection_changed();self.sidebar.list.viewport().update()
    def show_colors(self,id,anchor):
        if id is None:return
        self.popup=ColorPopup(self.window,self.data.get(id)['color']);self.popup.selected.connect(lambda color:self.set_color(id,color))
        self.popup.move(anchor.mapToGlobal(QPoint(0,anchor.height())));self.popup.show()
    def invalidate(self):
        from ui_pages import GrpcPage
        for i in range(self.window.workbench.count()):
            page=self.window.workbench.widget(i)
            if isinstance(page,GrpcPage):page.source_changed()
            if hasattr(page,'headers_panel'):page.headers_panel.schedule()
        self.window.snippet_panel.schedule()
    def rename(self,id):
        page=self.open(id);page.name.setFocus();page.name.selectAll()
    def duplicate(self,id):
        page=self.page(id)
        if page and not page.flush():return
        new=self.data.duplicate(id);self.refresh();return self.open(new)
    def move(self,id):
        records=self.data.all();labels=[f'{i+1}. {item["name"]}' for i,item in enumerate(records)]
        current=next(i for i,item in enumerate(records) if item['id']==id)
        choice,ok=QInputDialog.getItem(self.window,'Move environment','Position in the local environment list:',labels,current,False)
        if ok:self.data.move(id,labels.index(choice));self.refresh();self.select_item(id)
    def delete(self,id,confirm=True):
        if id is None:return
        data=self.data.get(id)
        if confirm and QMessageBox.question(self.window,'Delete environment',f'Delete “{data["name"]}”?',QMessageBox.StandardButton.Yes|QMessageBox.StandardButton.No)!=QMessageBox.StandardButton.Yes:return
        for page in self.pages():
            if page.environment_id==id:
                page.deleted=True;page._autosave_timer.stop();self.window.workbench.removeTab(self.window.workbench.indexOf(page));page.deleteLater()
        self.data.delete(id);self.refresh();self.window.save_workspace()
    def menu_for(self,id):
        from ui_collections import CollectionMenu
        menu=CollectionMenu(self.window);menu.addAction('Export',lambda:self.export(id))
        if id is not None:
            menu.addSeparator();menu.addAction('Move',lambda:self.move(id));menu.addSeparator()
            rename=menu.addAction('Rename',lambda:self.rename(id));rename.setShortcut(QKeySequence('Ctrl+E'))
            duplicate=menu.addAction('Duplicate',lambda:self.duplicate(id));duplicate.setShortcut(QKeySequence('Ctrl+D'))
            delete=menu.addAction('Delete',lambda:self.delete(id));delete.setShortcut(QKeySequence('Delete'));menu.delete_action=delete
        return menu
    def show_menu(self,id,point):self.menu=self.menu_for(id);self.menu.popup(point)
    def export(self,id):
        page=self.page(id)
        if page and not page.flush():return
        data=self.data.get(id);path,_=QFileDialog.getSaveFileName(self.window,'Export environment',data['name']+'.postman_environment.json','JSON (*.json)')
        if not path:return
        try:Path(path).write_text(json.dumps(self.data.export_document(id),ensure_ascii=False,indent=2),encoding='utf-8')
        except Exception as error:QMessageBox.critical(self.window,'Export failed',str(error))
    def import_file(self):
        path,_=QFileDialog.getOpenFileName(self.window,'Import environment','','JSON (*.json)')
        if not path:return
        try:
            id=self.data.import_document(json.loads(Path(path).read_text(encoding='utf-8-sig')));self.refresh();self.open(id)
        except Exception as error:QMessageBox.critical(self.window,'Import failed',str(error))
    def tab_changed(self,*_):
        page=self.window.workbench.currentWidget()
        if getattr(page,'is_environment_page',False):self.show_sidebar();self.select_item(page.environment_id)
        elif getattr(page,'entity_key',(None,None))[0]=='collection':
            self.window.select_nav('Collections');self.window.select_tree_item(*page.entity_key)
