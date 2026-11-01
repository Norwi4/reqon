# Reqon — mascot web assets v1

A collection of **cropped illustrations from one concept sheet** for the Reqon website. All images retain their original dark backgrounds; these are raster PNGs, not transparent vector assets. The sheet is a concept, not a final production asset system.

## Directories
- `hero/`: main mascot, API workspace, success, explorer. Use in landing hero and section intros.
- `features/`: collections, documentation, download, teamwork, security, updates, debugging, ideas, settings, platforms, coding, coffee. Use on feature cards and editorial sections.
- `ui/`: strips of emotions and symbols. These are **not individual transparent stickers**.
- `overview.png`: full visual contact sheet.

## Integration notes
- Prefer `hero/hero-main.png` for the main landing area and `features/*` for feature sections.
- Avoid stretching or placing images over bright backgrounds; the source has a dark baked-in background.
- Use `object-fit: contain` and rounded dark containers.
- The images were cropped from a single 1536×1024 contact sheet, so they are limited resolution; do not upscale excessively.
- For true transparent assets or high-resolution illustrations, generate/export separate source images before production.
- Mascot identity: gray/white raccoon, orange goggles, charcoal hoodie with orange R, dark navy and orange accents.
- Use actual product screenshots for UI demonstrations; illustrations are decorative.
