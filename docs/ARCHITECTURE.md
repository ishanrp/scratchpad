# Architecture

## Product model

Scratchpad is not clipboard history. A logical `Object` has one or more `Representation`s. A page merely places an object spatially through `PageItem`; it does not own the object.

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

## Process split

- `scratchpad-daemon`: owns persistence, managed blobs, jobs, resolver and IPC.
- `scratchpad-ui`: GTK4/layer-shell visual surface; it is intentionally thin and talks to the daemon.
- `scratchpad`: CLI using the same IPC contract.

## Interaction split

### External app ↔ Scratchpad
Standards-based DnD and MIME negotiation. The source does not infer external application identity or semantic intent.

### Scratchpad object ↔ Scratchpad target
The internal resolver can use source kind, target kind, page context, plugin capabilities and explicit user intent to choose actions.

## Edge reveal

On compatible compositors the UI keeps a tiny mapped layer-shell surface on the configured screen edge. Entering it can expand the same surface. This avoids relying on forbidden global pointer tracking.

## DnD safety

- External output advertises COPY by default.
- Cheap representations may be materialized before the drag begins.
- Expensive transforms are jobs, never synchronous DnD providers.
- External destructive MOVE remains gated behind an interoperability matrix.

## Storage

SQLite runs in WAL mode. The blob store is content-addressed by BLAKE3. Files imported from transient portal paths should be copied into managed storage before they are considered persistent.
