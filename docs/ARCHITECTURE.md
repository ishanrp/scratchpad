# Architecture

## Product model

Scratchpad is not clipboard history. A logical `Object` has one or more `Representation`s. A page only places an object spatially through `PageItem`; it does not own the object.

```text
Object
 ├─ metadata / provenance / lifecycle
 ├─ Representation(text/plain)
 ├─ Representation(text/html)
 ├─ Representation(text/uri-list)
 └─ Representation(file/blob/...)

Page
 └─ PageItem(object_id, x, y, width, height, z)
```

## Single-process application

The initial prototype split the daemon, CLI, and UI into three binaries. That isolation was useful while persistence and IPC were being developed, but it added operational friction without providing a user-visible benefit.

The production direction is one `scratchpad` executable:

```text
scratchpad process
├─ GTK main thread
│  ├─ layer-shell panel
│  └─ DnD / presentation
└─ service thread
   ├─ SQLite repository
   ├─ jobs / managed blobs
   └─ Unix socket listener
```

The service boundary remains as an internal module and Unix socket. This preserves clean ownership of SQLite and provides an API for CLI calls or future integrations without requiring the user to manage a daemon.

`scratchpad serve` is available when a headless service is intentionally desired.

## External interaction

### Inbound DnD

GTK performs the actual Wayland action negotiation.

- file/URI input: COPY only
- text input: COPY preferred; MOVE accepted for sources such as Chromium that expose selected text as MOVE-only
- Scratchpad never deletes the external source as part of an inbound text MOVE

### Outbound DnD

Target identity is unavailable on normal Wayland DnD, so one ambiguous source gesture cannot reliably adapt itself based on whether the receiver is a text editor or file manager.

For text and URL objects, Scratchpad exposes separate drag affordances:

- object body: semantic/plain-text representation
- LINK/FILE affordance: URI/file representation

File and directory objects naturally drag as URIs.

## Edge panel

The layer surface stays mapped. Hiding does not destroy/remap it. Instead, GDK input regions make the transparent portion click-through.

The tested Hyprland setup produced a `960x200` surface when the prototype relied on opposite-edge stretching. The current implementation therefore anchors a side panel to a corner and explicitly sizes its long axis from `GdkMonitor::geometry()` after the surface is mapped. This keeps edge activation deterministic while retaining one surface through an active DnD session.

## Storage

SQLite runs in WAL mode. The blob store is content-addressed by BLAKE3. Files imported from transient portal paths should be copied into managed storage before they are considered persistent.
