# Reqon — developer icon pack v2

Minimal geometric raccoon icon, distinct from the detailed website mascot. Source SVGs are in `svg/` and all raster exports are generated from those exact SVGs.

## Recommended integration
- Custom title bar, top left before hamburger: `png/transparent/reqon-24.png` (24 logical px; click target 36 px).
- Windows EXE / shortcut / taskbar / Alt+Tab: `windows/reqon.ico` (multi-resolution). Configure executable/packager AND runtime window icon as applicable.
- macOS application bundle: `macos/reqon.icns`.
- Linux: `linux/hicolor/*/apps/reqon.png`, `linux/reqon.svg`, and `Icon=reqon` in desktop entry.
- Web/PWA: `web/` favicon and manifest.
- Dark UI: `png/dark`; light UI: `png/light`; transparent variants: `png/transparent`.

## Design tokens
Background #111B27; accent #FF7900; foreground #F4F7FA.

## Notes
At 16 px, fine details will naturally diminish. Review icons on actual taskbar and 125–200% DPI. The SVG is an original editable vector construction, not an automatic vector trace. No original website mascot is included in this pack.
