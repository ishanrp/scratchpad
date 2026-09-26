# Configuration

Environment variables:

- `SCRATCHPAD_DATA_DIR` — overrides the XDG data directory used for DB/blobs.
- `SCRATCHPAD_RUNTIME_DIR` — overrides runtime socket/export cache directory.
- `SCRATCHPAD_SOCKET` — exact Unix socket path.
- `SCRATCHPAD_EDGE` — `left`, `right`, `top`, `bottom`; default `right`.
- `SCRATCHPAD_EDGE_WIDTH` — hidden activation width in px; default `2`.
- `SCRATCHPAD_PANEL_WIDTH` — expanded panel width in px; default `430`.
- `SCRATCHPAD_HOVER_MS` — page-hover switch delay; default `400`.
- `SCRATCHPAD_NO_LAYER_SHELL=1` — force ordinary GTK toplevel mode.


## Edge panel and DnD

- `SCRATCHPAD_PANEL_HEIGHT` — expanded height for top/bottom edge mode; default `360`.
- `SCRATCHPAD_COLLAPSE_MS` — delay before the panel collapses after pointer/drag leave; default `300`.
- `SCRATCHPAD_START_OPEN=1` — start expanded instead of as the edge activation strip.
- `SCRATCHPAD_MAX_DROP_BYTES` — maximum streamed text/URI payload accepted from a foreign drop; default `8388608` (8 MiB).
- `SCRATCHPAD_DND_DEBUG=1` — print offered/selected DnD MIME formats and transfer diagnostics to stderr.
