"""Shared Reqon presentation and application identity."""
import re
import sys
from pathlib import Path

IS_WINDOWS=sys.platform=='win32'
USE_REQON_UI=sys.platform in ('win32','linux')
DISPLAY_NAME='Reqon' if USE_REQON_UI else 'Getman'
VERSION='0.13.4' if sys.platform=='linux' else '0.13.8' if IS_WINDOWS else '0.11.0'
TAB_HEIGHT=36  # Total logical height, including the one-pixel active underline.
COLORS={
    'canvas':'#1f1f1f','panel':'#252525','hover':'#2b2b2b','selected':'#343434',
    'border':'#363636','accent':'#ff8319','action':'#3b82f6','action_hover':'#5593fa',
    'text':'#f5f7fa','muted':'#aab2bd','subtle':'#808994','focus':'#629af3',
    'line':'#24282d','error':'#f29a97','error_bg':'#432724','success':'#89d6ae',
    'match':'#62401f','number':'#b5cea8','string':'#ce9178','key':'#9cdcfe',
    'success_bg':'#1e3a2d','warning_bg':'#463b28','warning':'#e7bf74',
}
METHOD_COLORS={'GET':'#89d6ae','POST':'#e7bf74','PUT':'#8db7f7','PATCH':'#c5a6ee',
    'DELETE':'#f29a97','HEAD':'#89d5d5','OPTIONS':'#b6b8ec','gRPC':'#8db7f7'}
_LEGACY={'#212121':COLORS['canvas'],'#262626':COLORS['panel'],'#e6e6e6':COLORS['text'],
    '#ababab':COLORS['muted'],'#a6a6a6':COLORS['muted'],'#303030':COLORS['hover'],
    '#353535':COLORS['hover'],'#ff6c37':COLORS['accent'],'#0668d2':COLORS['action'],
    '#157ce6':COLORS['action_hover'],'#333333':COLORS['border'],'#333':COLORS['border']}


def color(value):return _LEGACY.get(value.lower(),value) if USE_REQON_UI else value


def stylesheet(value):
    return re.sub(r'#[0-9a-fA-F]{3,8}\b',lambda m:color(m.group()),value) if USE_REQON_UI else value


def asset_path(relative):
    root=Path(getattr(sys,'_MEIPASS',Path(__file__).resolve().parent))
    return root/'Reqon_Icon_Pack_Approved'/relative


def brand_icon(system=False):
    from PySide6.QtGui import QIcon
    if system:return QIcon(str(asset_path('windows/reqon.ico' if IS_WINDOWS else 'linux/reqon.png')))
    result=QIcon()
    # Use the approved raster at its actual pixel size, including high-DPI UI.
    for size in (16,20,24,32,40,48,64,72,96,128,144,180,192,256,384,512):
        result.addFile(str(asset_path(f'png/transparent/reqon-{size}.png')))
    return result


def windows_identity():
    if IS_WINDOWS:
        import ctypes
        setter=ctypes.windll.shell32.SetCurrentProcessExplicitAppUserModelID
        setter.argtypes=[ctypes.c_wchar_p];setter.restype=ctypes.c_long;setter('Norwi4.Reqon')
