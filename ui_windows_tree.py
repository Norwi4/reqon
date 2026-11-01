"""Tree presentation only; existing QTreeWidget owns editing, drag/drop and actions."""
import re
from PySide6.QtCore import Qt,QRect
from PySide6.QtGui import QColor,QPen
from PySide6.QtWidgets import QStyle
from ui_brand import COLORS as C,METHOD_COLORS
from ui_components import icon


def paint_row(painter,option,index,tree):
    option.font.setWeight(option.font.Weight.Medium if option.state & QStyle.StateFlag.State_Selected else option.font.Weight.Normal)
    role=int(Qt.ItemDataRole.UserRole);kind=index.data(role)[0];hovered=tree.itemFromIndex(index) is tree.hover_item
    starred=index.data(role+1);method=index.data(role+3);count=index.data(role+4)
    rect=QRect(option.rect);reserve=(52 if kind=='collection' else 28) if hovered else (26 if starred else 4)
    count_width=24 if kind=='collection' and count is not None and not hovered else 0
    if option.state & QStyle.StateFlag.State_Selected:painter.fillRect(rect,QColor(C['selected']))
    elif hovered:painter.fillRect(rect,QColor(C['hover']))
    label=rect.adjusted(8,0,-reserve-count_width,0);painter.setFont(option.font)
    if method:
        font=painter.font();font.setPixelSize(10);font.setWeight(font.Weight.DemiBold);painter.setFont(font)
        width=max(36,painter.fontMetrics().horizontalAdvance(method)+8)
        painter.setPen(QColor(METHOD_COLORS.get(method,C['muted'])))
        painter.drawText(QRect(label.left(),label.top(),width,label.height()),Qt.AlignmentFlag.AlignVCenter,method)
        label.adjust(width,0,0,0);painter.setFont(option.font)
    elif not option.icon.isNull() and kind!='collection':
        glyph=option.icon
        glyph.paint(painter,QRect(label.left(),rect.center().y()-8,16,16));label.adjust(22,0,0,0)
    text=painter.fontMetrics().elidedText(option.text,Qt.TextElideMode.ElideRight,max(0,label.width()))
    query=getattr(tree,'search_query','')
    if query:
        for match in re.finditer(re.escape(query),text,re.IGNORECASE):
            start=painter.fontMetrics().horizontalAdvance(text[:match.start()]);width=painter.fontMetrics().horizontalAdvance(match.group())
            painter.fillRect(QRect(label.left()+start,rect.center().y()-9,width,18),QColor(C['match']))
    painter.setPen(QColor(C['text']));painter.drawText(label,Qt.AlignmentFlag.AlignLeft|Qt.AlignmentFlag.AlignVCenter,text)
    if count_width:
        painter.setPen(QColor(C['muted']));painter.drawText(QRect(rect.right()-reserve-count_width,rect.top(),count_width,rect.height()),Qt.AlignmentFlag.AlignCenter,str(count))
    if starred and not hovered:icon('star',C['accent'],14).paint(painter,QRect(rect.right()-22,rect.center().y()-7,14,14))
