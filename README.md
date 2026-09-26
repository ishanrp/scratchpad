# System Scratchpad — Rust rebuild

A Linux/Wayland-first persistent visual staging surface for moving objects between applications and workflows.

## Runtime model

Scratchpad now ships as **one executable**.

```text
scratchpad
├─ GTK4/layer-shell UI
├─ embedded persistence/service thread
├─ SQLite + blob store
└─ Unix socket API for CLI calls and future integrations
```

Launching `scratchpad` starts the UI and its embedded service. The socket boundary remains internal because it is useful for CLI commands and future plugins, but there is no separate daemon binary to launch.

For a deliberately headless session:

```bash
scratchpad serve
```

CLI commands use the same executable and connect to the running service:

```bash
scratchpad list
scratchpad add-text "hello from Scratchpad"
scratchpad add-url https://example.com
scratchpad add-path ~/Downloads/example.pdf
```

## What is implemented

- Polymorphic objects with multiple representations: text, URI, file, directory, blob, URL, app/tool targets.
- Correct `page_items` placement model: objects are independent from pages and may be placed on multiple pages.
- SQLite WAL persistence and BLAKE3-addressed managed blob storage.
- Embedded service plus Unix-socket JSON IPC.
- Single-binary CLI/UI/headless modes.
- GTK4 + layer-shell edge surface with click-through input regions.
- Browser/file-manager inbound Wayland DnD.
- COPY-only file/URI imports; Chromium MOVE-only text drags are accepted without deleting the source.
- Explicit outbound representation drags: card content for text semantics, `LINK`/`FILE` handles for URI/file semantics.
- Page rail and drag-hover page switching.
- Plugin manifest/capability model.

## DnD representation rule

A Wayland drag source cannot reliably identify the target application. A destination that accepts both file/URI and plain-text formats may choose either one.

Scratchpad therefore does not advertise ambiguous representations from the same drag gesture for text/URL objects:

- drag a text/URL **card body** → plain text
- drag the **LINK** handle → URI/link
- drag the **FILE** handle on text → temporary text file URI
- drag a file/folder object → file URI

This avoids editors treating a URL as a file-open request while preserving file-manager link/download behavior.

## Build on CachyOS / Arch

```bash
sudo pacman -S --needed base-devel rust gtk4 gtk4-layer-shell sqlite
cargo build --workspace --release
./target/release/scratchpad
```

## Build without Rust on the host

```bash
./scripts/export-docker-build.sh
./dist/scratchpad
```

The host only needs the GTK4 / gtk4-layer-shell runtime libraries.

GitHub Actions performs a clean Arch Linux release build and test pass and uploads `scratchpad-linux-x86_64`.

## Data locations

Defaults follow XDG directories:

- database: `$XDG_DATA_HOME/system-scratchpad/scratchpad.db`
- blobs: `$XDG_DATA_HOME/system-scratchpad/blobs/`
- export cache: `$XDG_RUNTIME_DIR/system-scratchpad/exports/`
- socket: `$XDG_RUNTIME_DIR/system-scratchpad/scratchpad.sock`

See `docs/CONFIGURATION.md`, `docs/ARCHITECTURE.md`, and `docs/WAYLAND_DND_INVESTIGATION.md`.
