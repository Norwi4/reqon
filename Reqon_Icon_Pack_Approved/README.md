# Reqon — approved desktop icon pack

This pack is generated **from the exact user-approved PNG**, not from a newly generated drawing. The face, goggles, proportions and palette are preserved.

## Files

- `source/reqon-original.png` — untouched uploaded image, 1254 × 1254.
- `png/dark/reqon-N.png` — original dark rounded-square tile, cropped to remove black canvas margins and resized.
- `png/transparent/reqon-N.png` — same artwork, with exterior black canvas made transparent (dark tile remains).
- `windows/reqon.ico` — Windows EXE, shortcut, taskbar, Alt+Tab, sizes 16/24/32/48/64/128/256.
- `macos/reqon.icns` — macOS app icon.
- `linux/reqon.png` — 512 px desktop entry icon.
- `web/` — favicon, 192/512 PWA icons, apple-touch-icon and web manifest.
- `preview/size-comparison.png` — preview of actual pixel-size artwork against dark/light surfaces.

## Suggested integration

- Desktop shortcut, Windows taskbar and executable: `windows/reqon.ico`.
- Title bar upper-left: `png/transparent/reqon-24.png` (or 20/32 for UI scaling).
- macOS: `macos/reqon.icns`.
- Linux: `linux/reqon.png`.
- Website: `web/favicon.ico`, `web/apple-touch-icon.png`, and PWA PNGs.

## Important

The source is raster, **not an editable vector**. Do not rename PNG to SVG or assume vector quality. The smallest icons are downsampled versions; test 16/24 px at actual scale. For a future vector master, manually redraw the approved artwork rather than generating a different mascot.
