"""Compact selectors used by HTTP editor panels."""
from PySide6.QtCore import QPoint,QRect,Qt
from PySide6.QtWidgets import QMenu,QLineEdit
from ui_components import Select,icon
from ui_request_selectors import popup_position


class EditorSelect(Select):
    def __init__(self,items=(),value='',parent=None):
        super().__init__(parent);self.addItems(items);self.setCurrentText(value);self.setMinimumHeight(32)
        self.popup_menu=QMenu(self)
        self.popup_menu.setStyleSheet('QMenu {background:#252525;border:1px solid #444;border-radius:4px;padding:7px;} QMenu::item {padding:7px 16px;min-height:16px;} QMenu::item:selected {background:#363636;border-radius:3px;}')
        self.popup_menu.aboutToHide.connect(lambda:super(EditorSelect,self).hidePopup())
    def showPopup(self):
        menu=self.popup_menu;menu.clear();menu.setMinimumWidth(max(170,self.width()));current=None
        for index in range(self.count()):
            action=menu.addAction(self.itemText(index));action.setCheckable(True);action.setChecked(index==self.currentIndex())
            action.triggered.connect(lambda checked=False,index=index:self.select(index))
            if index==self.currentIndex():current=action
        anchor=QRect(self.mapToGlobal(QPoint()),self.size());menu.ensurePolished();menu.adjustSize()
        menu.popup(popup_position(anchor,menu.sizeHint(),self.screen()));menu.setActiveAction(current)
    def select(self,index):
        self.setCurrentIndex(index);self.hidePopup();self.activated.emit(index);self.textActivated.emit(self.currentText())
    def hidePopup(self):self.popup_menu.hide();super().hidePopup()


class CheckSelect(QLineEdit):
    """Editable choice list; checked items are serialized as one settings value."""
    def __init__(self,choices,value='',separator=', ',placeholder='Select options'):
        super().__init__(value);self.choices=choices;self.separator=separator;self.setPlaceholderText(placeholder);self.setMinimumHeight(32)
        action=self.addAction(icon('chevron'),QLineEdit.ActionPosition.TrailingPosition);action.triggered.connect(self.show_choices)
        self.menu=QMenu(self);self.menu.setStyleSheet('QMenu {background:#252525;border:1px solid #444;padding:6px;} QMenu::item {padding:6px 18px;} QMenu::item:selected {background:#363636;}')
    def show_choices(self):
        self.menu.clear();selected=[part.strip() for part in self.text().split(self.separator.strip()) if part.strip()]
        for name in self.choices:
            action=self.menu.addAction(name);action.setCheckable(True);action.setChecked(name in selected)
            action.triggered.connect(lambda checked,name=name:self.choose(name,checked))
        self.menu.setMinimumWidth(self.width());self.menu.popup(popup_position(QRect(self.mapToGlobal(QPoint()),self.size()),self.menu.sizeHint(),self.screen()))
    def choose(self,name,checked):
        selected=[part.strip() for part in self.text().split(self.separator.strip()) if part.strip()]
        if checked and name not in selected:selected.append(name)
        elif not checked:selected=[value for value in selected if value!=name]
        self.setText(self.separator.join(selected))
    def keyPressEvent(self,event):
        if event.key()==Qt.Key.Key_Down and event.modifiers()==Qt.KeyboardModifier.AltModifier:self.show_choices();event.accept()
        else:super().keyPressEvent(event)
