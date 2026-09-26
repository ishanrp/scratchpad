# Implementation status

## Implemented

- Core object / representation / page / placement model
- SQLite migration and repository
- BLAKE3 blob store
- One `scratchpad` executable containing UI, embedded service, and CLI modes
- Optional `scratchpad serve` headless mode
- Unix socket JSON IPC
- GTK4/layer-shell panel with GDK input-region hide/reveal
- Runtime monitor-geometry sizing
- Persistent manual panel width
- Browser text/URL inbound DnD
- File/URI inbound DnD
- Outbound semantic-vs-URI drag affordances
- Page hover switching
- Basic text/URL/path ingestion
- Plugin manifest/capability model
- GitHub Actions release build/tests/artifact

## Runtime validated on Hyprland so far

- Chromium selected-text drop reaches byte transfer and SQLite persistence
- browser image/link drag is captured as a URL when the browser exposes only text/URL representations
- outbound URI drag reaches file-manager drop negotiation
- panel GDK input regions correctly change between hidden and revealed widths

## Still requires runtime validation

- monitor-sized side surface on the user's actual output after explicit `GdkMonitor::geometry()` sizing
- new card-body plain-text drag into text editors
- LINK/FILE drag affordances into Dolphin
- inbound native files from Dolphin
- portal-backed transient-file import detection
- compositor-specific fullscreen/edge conflicts
- KDE/Sway interoperability matrix

## Deliberately deferred

- GNOME Shell extension
- destructive external file MOVE
- untrusted plugin sandbox enforcement
- expensive transforms in the DnD path
