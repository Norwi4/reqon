# Reqon UI States — Screenshot Style v2

This pack uses **the exact illustrations from the approved reference screenshot**, individually cropped and separated from the dark background. It does **not** substitute newly invented drawings.

## Contents

- `dark/`: 14 transparent PNG illustrations for dark surfaces.
- `light/`: 14 recolored transparent PNG illustrations for light surfaces.
- `previews/`: overview sheets for visual review.
- `manifest.json`: machine-readable mapping of each illustration to its UI state.

## When to use each image

| Filename (without extension) | Trigger / screen |
|---|---|
| `response-ready-to-send` | Response panel before a request has been executed |
| `request-create-new` | New request creation / no request selected |
| `history-no-activity` | No recent request activity |
| `response-no-response` | Request has no response yet |
| `error-service-unavailable` | Server unavailable / connection refused |
| `error-no-connection` | Network is offline or connection unavailable |
| `error-dns-resolution` | Hostname DNS lookup failed |
| `error-tls-handshake` | TLS handshake or certificate failure |
| `error-request-timeout` | Request exceeded configured timeout |
| `collections-empty` | No collections in workspace |
| `search-no-results` | Search returned no matches |
| `view-no-data` | Current panel has no data |
| `folder-empty` | Folder contains no requests |
| `request-completed` | Request completed successfully |

## Implementation guidance for Codex CLI

1. Use `manifest.json` IDs to select the appropriate illustration; load `dark/` or `light/` according to the current theme.
2. Keep headings, descriptions, retry actions and diagnostics in native UI components, not inside images.
3. Render illustrations using `object-fit: contain`; suggested size 120–180 CSS px, with responsive constraints.
4. Do not show a success illustration instead of a real response body; it is suitable only for a successful operation without data or as transient feedback.
5. Show detailed error information and preserve existing request/response behavior.
6. Use descriptive alt text or mark decorative illustrations `aria-hidden=true` when nearby text communicates the state.

## Important asset limitations

The approved reference is a single **raster** composite, not a set of original vectors. These are extracted **PNG images with transparency**, not true SVG vector paths. Some antialiased edges and very dark details may show extraction artifacts; light-theme versions are recolored adaptations, not separately hand-drawn originals. Inspect at actual display size before shipping. For production-grade SVGs, each illustration must be redrawn/traced manually.
