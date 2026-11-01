# Reqon UI States — developer asset pack v1.0

A cohesive collection of **14 original line-art illustrations** for Reqon desktop, supplied in **light** and **dark** theme variants. SVG artwork has transparent backgrounds, no embedded captions, and is scalable. PNG previews are supplied at 256×256 and 512×512. This is an original vector reinterpretation of the previously approved concept sheet, **not** a vector trace of the raster concept.

## File organization

```text
svg/light/<state>.svg
svg/dark/<state>.svg
png/light/<state>-256.png
png/light/<state>-512.png
png/dark/<state>-256.png
png/dark/<state>-512.png
README.md
manifest.json
```

## State → illustration mapping

| File stem | When to show | Suggested heading | Suggested description |
|---|---|---|---|
| `response-ready-to-send` | Before a request has been executed in the selected tab | Ready to send | Send a request to inspect the response. |
| `request-create-new` | No request is currently open; user is prompted to create one | Create a request | Create a request to start working with your API. |
| `activity-no-requests` | Request history has no entries | No activity | Run a request to see activity here. |
| `response-no-content` | Response completed but has no body (for example HTTP 204) | No response body | The request completed without a response body. |
| `error-service-unavailable` | Service is unavailable (gRPC UNAVAILABLE / HTTP 503), not a specific DNS or TLS error | Service unavailable | The service is currently unavailable. |
| `error-connection-refused` | TCP connection refused or target port unreachable | Connection failed | Could not connect to the target service. |
| `error-dns-resolution` | Hostname could not be resolved | DNS error | Check the server hostname and DNS configuration. |
| `error-tls-handshake` | TLS handshake, certificate, or trust validation failure | TLS error | A secure connection could not be established. |
| `error-request-timeout` | Deadline exceeded or request timed out | Request timed out | The request took too long to complete. |
| `collections-empty` | No collections exist in the active workspace | No collections | Create a collection to organize your requests. |
| `search-no-results` | Search returned zero matches | No results | Try another search query. |
| `data-empty` | A data-driven view has no records | No data | There is no data to display here. |
| `folder-empty` | Selected collection folder contains no requests or child items | Empty folder | This folder does not contain any requests yet. |
| `response-success` | Request completed successfully and success feedback is appropriate; do not replace an actual response body | Request completed | The request completed successfully. |

## Implementation rules

1. **Use the actual app state**, not the request method or a substring of the error message, to choose the image. Distinguish connection refusal, DNS failure, TLS failure, timeouts and service unavailability using structured error codes where possible.
2. Select `svg/dark/` for dark surfaces and `svg/light/` for light surfaces. Do not invert SVG colors using CSS filters.
3. The artwork is transparent. Use it at **96–160 CSS px** in empty/error states, typically 120–136 px. Maintain aspect ratio and allow a clear margin.
4. Keep all headings, descriptions, error details, and retry buttons in the UI, not baked into the artwork. Localize them normally.
5. Illustrations are decorative: use `alt=""` / `aria-hidden="true"` if a nearby heading describes the state. Preserve accessible error messages.
6. Do not replace a successful response payload with `response-success`; use this for optional confirmation or when there is no payload to inspect.
7. If there is an HTTP or gRPC error response body, preserve it. Error illustration may appear in a separate status panel, but must not hide technical diagnostics.
8. `response-no-content` is for an intentionally empty completed response; `response-ready-to-send` is for the pre-execution state.
9. Prefer SVG for UI and PNG only for frameworks that require raster assets. Avoid embedding these in button icons or the app title bar: those use the separate Reqon logo pack.
10. Images are original UI illustrations; they are not Postman assets. They complement the detailed marketing mascot and simplified app icon.

## Example (React)

```tsx
const asset = `/assets/reqon-ui-states/svg/${theme}/${state}.svg`;
<img src={asset} width={128} height={128} alt="" aria-hidden="true" />
```

Valid `state` values are the file stems in the table. For user-facing copy, use translation keys, not filenames.
