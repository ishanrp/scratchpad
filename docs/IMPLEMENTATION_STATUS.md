# Implementation status

## Implemented in this rebuild

- Workspace and crate boundaries
- Core object/representation/page/placement model
- External offer builder vs internal resolver split
- SQLite migration and repository
- BLAKE3 blob store
- Unix socket JSON protocol
- Daemon request dispatch
- CLI
- GTK4 edge panel shell
- Page hover switching controller
- Rich preview card taxonomy
- Basic text/URL/path ingestion
- Plugin manifest/capability model
- Tests for core resolver/offer rules and repository basics

## Requires real Wayland runtime validation

- foreign DnD + layer-shell expansion continuity
- multiple reads from one incoming foreign drop across GTK backends
- drag-out union providers to real GTK/Qt/XWayland apps
- portal-backed transient-file import detection
- compositor-specific fullscreen/edge conflicts

## Deliberately deferred

- GNOME Shell extension
- destructive external MOVE
- untrusted plugin sandbox enforcement
- expensive transform plugins in the drag path
- CRIU/session-resume ideas from the broader task-layer research
