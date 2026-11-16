# Reqon 0.20 — upgrade and recovery

Windows x64. A native API client for HTTP, gRPC and WebSocket.

## Upgrade from 0.19.0

Close the previous application, then run the current `Reqon-Setup-0.20.x-win64.exe` installer.
The installer uses the same application identifier and previous installation directory.
User data is never included in the installer and is never deleted by uninstalling.

On first launch, Reqon detects the previous `reqon.sqlite3` in `%APPDATA%\Reqon`
(including older `Getman\Getman` / `Getman` locations). It creates a consistent
SQLite backup, including committed WAL records, then converts the workspace
into `%APPDATA%\Reqon\native`. The original database is opened read-only and
remains available to 0.19.0. Subsequent launches use the native profile and do
not import duplicates or overwrite new work.

Collections, nested folders, HTTP/gRPC requests, metadata, disabled rows,
authorization, bodies, TLS settings, referenced managed files, environments,
globals, history and local drafts are retained. Request identity, tab order and
the active request are restored. Saved examples remain in collection exports;
all original columns and extended settings are also retained in the backup.
Unknown settings are preserved even where the new interface represents them differently.

The migration report is `native\upgrade-0.19-report.json`. Its `backup` field
points to the original SQLite snapshot and complete `records.json`.
If conversion fails, no partial native workspace is published. The original
database and the backup remain intact, and the application displays the error.

An existing native workspace always takes precedence. `--data-dir <folder>`
selects an isolated profile. If that folder contains a previous SQLite database,
the converted profile is created in its `native` subdirectory.

## cURL editor

Use the code icon in the request address bar to open the editable cURL/gRPCurl panel.
Valid edits update the active request automatically after 650 ms. Incomplete
commands remain as drafts and do not replace request fields. Each tab owns its draft.
Editing request fields refreshes a clean command; unfinished commands are kept.
Use Regenerate to replace a command draft from the request, or Copy to copy it.

Import accepts Bash, Windows CMD and PowerShell commands, with an explicit shell
selector when auto-detection is ambiguous. URL paste also accepts cURL.
Parsing commands never executes a shell or sends a request. Headers, query
parameters, JSON/text bodies, URL-encoded and multipart forms, binary files,
authentication, TLS and timeout options are supported. Unsupported shell
expressions or command options produce an error instead of being silently dropped.

## Recovery

The old database is unchanged, so reinstalling 0.19.0 restores access to its
original data. Edits made in 0.20 stay in the native profile; export collections
before moving those edits to another installation. Back up both profiles before
manually moving or deleting them.
