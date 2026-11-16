"""Export the archived first Reqon SVG mark and its visual contact sheet.

The active Circular Icon Pack lives in assets/brand/circular-v1 and is imported
unchanged from the supplied ZIP. This legacy generator does not modify it.

Development-only dependencies: PySide6 and Pillow. No runtime dependencies.
The SVG is the editable master. Small raster exports are rendered separately.
"""
from io import BytesIO
from pathlib import Path
import json
import struct

from PIL import Image, ImageDraw, ImageFont
from PySide6.QtCore import QByteArray, QRectF
from PySide6.QtGui import QImage, QPainter
from PySide6.QtSvg import QSvgRenderer

ROOT = Path(__file__).resolve().parents[1]
BRAND = ROOT / "assets" / "brand"
GOLD = "#e4ad49"
DARK = "#242424"
MASTER = (BRAND / "reqon-mark.svg").read_text(encoding="utf-8")
SHAPES = MASTER[MASTER.index("  <path"):MASTER.rindex("</svg>")]


def svg_document(body, viewbox="0 0 64 64"):
    return f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{viewbox}" width="64" height="64">\n{body}\n</svg>\n'


APP = svg_document(
    f'<rect x="2" y="2" width="60" height="60" rx="14" fill="{DARK}"/>'
    '<g transform="translate(.64 -.34) scale(.98)">'
    + SHAPES.replace("#fff", GOLD) + '</g>'
)
SMALL_APP = APP.replace('translate(.64 -.34) scale(.98)', 'translate(-2.56 -3.64) scale(1.08)')
GOLD_MARK = MASTER.replace("#fff", GOLD)
LIGHT_MARK = MASTER.replace("#fff", "#9b650e")


def render(svg, size):
    image = QImage(size, size, QImage.Format.Format_ARGB32_Premultiplied)
    image.fill(0)
    painter = QPainter(image)
    painter.setRenderHint(QPainter.RenderHint.Antialiasing)
    renderer = QSvgRenderer(QByteArray(svg.encode("utf-8")))
    assert renderer.isValid()
    renderer.render(painter, QRectF(0, 0, size, size))
    painter.end()
    rgba = image.convertToFormat(QImage.Format.Format_RGBA8888)
    return Image.frombytes("RGBA", (size, size), rgba.bits().tobytes())


def raster(svg, size):
    # Supersampling keeps the same rounded silhouette on 100–200% displays.
    result = render(svg, size * 4).resize((size, size), Image.Resampling.LANCZOS)
    if size <= 24:
        # Remove invisible resampling ringing outside the rounded tile.
        result.putalpha(result.getchannel("A").point(lambda alpha: 0 if alpha <= 2 else alpha))
    return result


def write_changed(path, content):
    if not path.exists() or path.read_bytes() != content:
        path.write_bytes(content)


def save_png(path, image):
    stream = BytesIO()
    image.save(stream, format="PNG")
    write_changed(path, stream.getvalue())


def save_ico(path, frames):
    # Explicit frames: never let an ICO encoder derive small sizes from 256px.
    payloads = []
    for size, image in frames:
        stream = BytesIO()
        image.save(stream, format="PNG")
        payloads.append((size, stream.getvalue()))
    offset = 6 + len(payloads) * 16
    directory = bytearray(struct.pack("<HHH", 0, 1, len(payloads)))
    for size, data in payloads:
        directory.extend(struct.pack("<BBBBHHII", size % 256, size % 256, 0, 0,
                                     1, 32, len(data), offset))
        offset += len(data)
    write_changed(path, bytes(directory) + b"".join(data for _, data in payloads))


def main():
    for name, svg in [("reqon-app.svg", APP), ("reqon-app-small.svg", SMALL_APP),
                      ("reqon-mark-gold.svg", GOLD_MARK), ("reqon-mark-light.svg", LIGHT_MARK)]:
        write_changed(BRAND / name, svg.encode("utf-8"))
    sizes = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256]
    frames = []
    for size in sizes + [512, 1024]:
        frame = raster(SMALL_APP if size <= 24 else APP, size)
        save_png(BRAND / f"reqon-{size}.png", frame)
        if size <= 256:
            frames.append((size, frame))
    save_ico(BRAND / "reqon.ico", frames)

    # A review sheet at actual UI sizes as well as large-scale geometry.
    sheet = Image.new("RGB", (1200, 760), "#151515")
    draw = ImageDraw.Draw(sheet)
    regular = ImageFont.truetype("C:/Windows/Fonts/segoeui.ttf", 18)
    small = ImageFont.truetype("C:/Windows/Fonts/segoeui.ttf", 14)
    heading = ImageFont.truetype("C:/Windows/Fonts/seguisb.ttf", 30)
    draw.text((48, 35), "Reqon / raccoon", font=heading, fill="#ededed")
    draw.text((48, 82), "One mark. Three surfaces.", font=regular, fill="#959595")
    for x, title, svg, bg in [
        (48, "Desktop / app", APP, "#1d1d1d"),
        (422, "Dark interface", GOLD_MARK, "#1d1d1d"),
        (796, "Light interface", LIGHT_MARK, "#f6f5f2"),
    ]:
        draw.rounded_rectangle((x, 135, x + 352, 483), radius=16, fill=bg)
        icon = raster(svg, 208)
        sheet.paste(icon, (x + 72, 181), icon)
        draw.text((x + 22, 441), title, font=regular, fill="#959595" if x < 796 else "#666666")
    draw.text((48, 526), "Taskbar / actual pixels", font=regular, fill="#ededed")
    x = 48
    for size in [16, 20, 24, 32, 40, 48, 64]:
        icon = raster(SMALL_APP if size <= 24 else APP, size)
        sheet.paste(icon, (x, 578 + (64 - size) // 2), icon)
        draw.text((x, 660), str(size), font=small, fill="#959595")
        x += size + 34
    draw.text((760, 526), "Title bar / 18 px", font=regular, fill="#ededed")
    draw.rounded_rectangle((760, 578, 1148, 614), radius=6, fill="#1a1a1a")
    icon = raster(GOLD_MARK, 18)
    sheet.paste(icon, (907, 587), icon)
    draw.text((933, 585), "Reqon", font=small, fill="#dddddd")
    draw.text((48, 718), "Original vector master · transparent mark · multi-size Windows ICO", font=small, fill="#777777")
    save_png(BRAND / "preview.png", sheet)
    print(json.dumps({"brand": str(BRAND), "ico_sizes": sizes, "png_max": 1024}))


if __name__ == "__main__":
    main()
