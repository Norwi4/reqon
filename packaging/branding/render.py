"""Render the light installer panel from the existing Reqon vector logo.

Run: python packaging/branding/render.py
Build-time dependencies: PySide6. The committed PNG is used directly by Inno
Setup, so compiling an installer does not require Python or Qt.
"""
import os
from pathlib import Path

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from PySide6.QtCore import QByteArray, QRectF
from PySide6.QtGui import QFontDatabase, QGuiApplication, QImage, QPainter
from PySide6.QtSvg import QSvgRenderer

DIRECTORY = Path(__file__).resolve().parent
ROOT = DIRECTORY.parents[1]
logo = (ROOT / "assets/brand/circular-v1/svg/reqon-circle.svg").read_text(encoding="utf-8")
shapes = logo[logo.index(">") + 1:logo.rindex("</svg>")]
svg = f'''<svg xmlns="http://www.w3.org/2000/svg" width="492" height="942" viewBox="0 0 164 314">
  <rect width="164" height="314" fill="#F5F3EE"/>
  <rect x="27" y="30" width="30" height="3" rx="1.5" fill="#E9AE40"/>
  <g transform="translate(30 63) scale(0.203125)">{shapes}</g>
  <text x="82" y="191" text-anchor="middle" font-family="Inter" font-size="28" font-weight="600" fill="#191919">Reqon</text>
  <text x="82" y="211" text-anchor="middle" font-family="Inter" font-size="9" letter-spacing="1.8" fill="#986511">API CLIENT</text>
  <path d="M29 254 H69 Q76 254 76 261 V268 Q76 275 83 275 H135" fill="none" stroke="#D8D1C4" stroke-width="1"/>
  <circle cx="29" cy="254" r="3" fill="#E9AE40"/>
  <circle cx="135" cy="275" r="3" fill="#F5F3EE" stroke="#E9AE40" stroke-width="1"/>
  <text x="27" y="294" font-family="Inter" font-size="9" fill="#67645E">reqon.ru</text>
</svg>
'''
(DIRECTORY / "wizard-light.svg").write_text(svg, encoding="utf-8", newline="\n")
app = QGuiApplication([])
for filename in ("Inter-Regular.ttf", "Inter-SemiBold.ttf"):
    if QFontDatabase.addApplicationFont(str(ROOT / "assets/fonts" / filename)) < 0:
        raise RuntimeError(f"Could not load {filename}")
renderer = QSvgRenderer(QByteArray(svg.encode("utf-8")))
if not renderer.isValid():
    raise RuntimeError("Invalid installer artwork")
image = QImage(492, 942, QImage.Format.Format_ARGB32_Premultiplied)
image.fill(0)
painter = QPainter(image)
painter.setRenderHint(QPainter.RenderHint.Antialiasing)
renderer.render(painter, QRectF(0, 0, 492, 942))
painter.end()
if not image.save(str(DIRECTORY / "wizard-light.png")):
    raise RuntimeError("Could not save installer artwork")
print("Rendered packaging/branding/wizard-light.png (492 x 942)")
