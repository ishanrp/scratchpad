# System Scratchpad — Rust rebuild

A Linux/Wayland-first, persistent visual staging surface for moving *objects* between applications and workflows. This repository is a clean Rust rebuild based on the original Scratchpad engineering design plus the adversarial feasibility review.

## What is implemented

- Rust workspace split into core model, SQLite storage, daemon, CLI, and GTK4 UI.
- Polymorphic objects with multiple representations (text, URI, file, directory, blob, URL, app/tool targets).
- Correct `page_items` placement model: objects are independent from pages and may be placed on multiple pages.
- SQLite WAL persistence, schema migrations, object/page CRUD, placement ordering and geometry.
- BLAKE3-addressed managed blob store.
- Unix-socket JSON IPC with request IDs and a line-delimited framing protocol.
- CLI for adding/listing/showing/removing objects and managing pages.
- GTK4 edge panel with a persistent tiny activation surface when layer-shell is available.
- Page rail with hover activation state and a 400 ms page switch timer during drag motion.
- Rich card rendering for text, URL, image, video, audio, PDF, file, folder, app/tool and unknown MIME objects.
- Outbound external drag policy separated from the internal target resolver.
- COPY-first external DnD policy. Destructive external MOVE is intentionally not enabled.
- Cheap virtual-file materialization support in the core offer builder.
- Plugin manifest model and deterministic internal capability resolver skeleton.
- Native-first packaging/build instructions and a containerized build recipe.

## Deliberate limits

Wayland does **not** let a normal client discover arbitrary external target app identity, target filesystem location, or global pointer coordinates. Therefore external drops use MIME/action negotiation only. Rich semantic resolution is reserved for Scratchpad-owned targets. GNOME shell integration is deferred; the project targets Hyprland/wlroots-style layer-shell first.

## Build on CachyOS / Arch

```bash
sudo pacman -S --needed base-devel rust gtk4 gtk4-layer-shell sqlite
cargo build --workspace --release
```

Run the daemon and UI in separate terminals:

```bash
./target/release/scratchpad-daemon
./target/release/scratchpad-ui
```

Then add something:

```bash
echo 'hello from scratchpad' | ./target/release/scratchpad add-text --stdin
./target/release/scratchpad add-url https://example.com
./target/release/scratchpad add-path ~/Downloads/example.pdf
./target/release/scratchpad list
```

## Data locations

Defaults follow XDG directories:

- database: `$XDG_DATA_HOME/system-scratchpad/scratchpad.db`
- blobs: `$XDG_DATA_HOME/system-scratchpad/blobs/`
- export cache: `$XDG_RUNTIME_DIR/system-scratchpad/exports/`
- socket: `$XDG_RUNTIME_DIR/system-scratchpad/scratchpad.sock`

All paths can be overridden with environment variables documented in `docs/CONFIGURATION.md`.

## Architecture

See `docs/ARCHITECTURE.md`, `docs/FEASIBILITY_GATES.md`, and `docs/IMPLEMENTATION_STATUS.md`.

## Build without Rust on the host

If you keep Rust/Cargo out of your host system, build in Docker and export the native binaries:

```bash
./scripts/export-docker-build.sh
```

This creates:

```text
dist/scratchpad-daemon
dist/scratchpad-cli
dist/scratchpad-ui
```

Run those binaries natively in your Wayland session. The host still needs the runtime libraries (`gtk4` and `gtk4-layer-shell`).

GitHub Actions also performs a clean Arch Linux release build and test pass on every push/PR and uploads the same three binaries as an artifact.
