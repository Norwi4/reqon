"""Capture actual request menus in an isolated workspace at 100% or 200%."""
import argparse,os,sys,tempfile
from pathlib import Path
parser=argparse.ArgumentParser();parser.add_argument('--scale',default='1');options=parser.parse_args()
config=Path('build/request-menu-screen.json');config.parent.mkdir(exist_ok=True)
config.write_text('{"screens":[{"name":"Visual QA","width":3200,"height":2200}]}')
os.environ['QT_QPA_PLATFORM']='offscreen:configfile='+str(config);os.environ['QT_SCALE_FACTOR']=options.scale
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from PySide6.QtCore import QPoint
from PySide6.QtGui import QPainter
from PySide6.QtTest import QTest
from PySide6.QtWidgets import QApplication
from main import MainWindow,Store
from ui_theme import STYLE
from ui_typography import load_fonts

app=QApplication([]);load_fonts();app.setStyle('Fusion');app.setStyleSheet(STYLE)
output=Path('docs/ui');output.mkdir(parents=True,exist_ok=True)
with tempfile.TemporaryDirectory(prefix='reqon-menus-') as directory:
    store=Store(directory);window=MainWindow(store);window.resize(1200,820);window.move(0,0);window.show()
    page=window.new_request();QTest.qWait(80)
    def capture(name,menu):
        QTest.qWait(40);frame=window.grab();painter=QPainter(frame)
        painter.drawPixmap(window.mapFromGlobal(menu.mapToGlobal(QPoint())),menu.grab());painter.end()
        target=output/f'reqon-request-{name}-{round(float(options.scale)*100)}.png';frame.save(str(target));print(target)
    page.method.showPopup();capture('methods',page.method.popup_menu);page.method.hidePopup()
    page.protocol.open_menu();capture('protocols',page.protocol.menu());page.protocol.menu().hide()
    window.close();window.deleteLater();app.processEvents();store.db.close()
