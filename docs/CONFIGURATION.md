# Configuration

Environment variables:

- `SCRATCHPAD_DATA_DIR` — overrides the XDG data directory used for DB/blobs.
- `SCRATCHPAD_RUNTIME_DIR` — overrides runtime socket/export cache directory.
- `SCRATCHPAD_SOCKET` — exact Unix socket path.
- `SCRATCHPAD_EDGE` — `left`, `right`, `top`, `bottom`; default `right`.
- `SCRATCHPAD_EDGE_WIDTH` — hidden activation width in application pixels; default `3`.
- `SCRATCHPAD_PANEL_WIDTH` — initial/pinned width for left/right panels; default `560`.
- `SCRATCHPAD_PANEL_HEIGHT` — initial/pinned height for top/bottom panels; default `420`.
- `SCRATCHPAD_MAX_PANEL_EXTENT` — maximum resizable panel extent; default `960`.
- `SCRATCHPAD_COLLAPSE_MS` — delay before collapsing after pointer/drag leave; default `350`.
- `SCRATCHPAD_HOVER_MS` — page-hover switch delay; default `400`.
- `SCRATCHPAD_REFRESH_MS` — UI persistence refresh interval; default `500`.
- `SCRATCHPAD_MAX_DROP_BYTES` — maximum streamed text/URI payload from a foreign drop; default 8 MiB.
- `SCRATCHPAD_DND_DEBUG=1` — log DnD formats/actions/transfers.
- `SCRATCHPAD_PANEL_DEBUG=1` — log monitor, GDK surface and input-region geometry.
- `SCRATCHPAD_FALLBACK_MONITOR_WIDTH` — pre-map width fallback; default `1920`.
- `SCRATCHPAD_FALLBACK_MONITOR_HEIGHT` — pre-map height fallback; default `1080`.
- `SCRATCHPAD_NO_LAYER_SHELL=1` — force ordinary GTK toplevel mode.

The panel width/height changed with the resize grip is also persisted under the Scratchpad data directory.
