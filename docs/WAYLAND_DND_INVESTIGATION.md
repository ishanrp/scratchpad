# Wayland / GTK4 DnD investigation

This document records the failure analysis behind the September 2026 UI/DnD rewrite.

## Symptoms

The first implementation repeatedly reached `GtkDropTargetAsync::accept` but never reached the data read. Later attempts produced:

```
Gdk-CRITICAL: gdk_drop_status: assertion '(preferred & actions) == preferred' failed
```

The layer panel also remained a small natural-height widget while the layer surface itself occupied a larger area, and dynamically changing the toplevel size made DnD behavior harder to reason about.

## Root causes

### 1. We were overriding GTK's action negotiation unnecessarily

`GtkDropTargetAsync` already has default `accept`, `drag-enter`, and `drag-motion` handlers. GTK's implementation chooses a unique preferred action and calls `gdk_drop_status()` on behalf of the controller.

The previous UI overrode `drag-enter` and `drag-motion`, derived the preferred action from `GdkDrop::actions()`, and then changed the target action mask over several iterations. This duplicated GTK's state machine and made it easy to return a preferred action that was not a member of the controller's current action mask.

Relevant upstream sources:

- https://docs.gtk.org/gtk4/class.DropTargetAsync.html
- https://codebrowser.dev/gtk/gtk/gtk/gtkdroptargetasync.c.html
- https://docs.gtk.org/gdk4/method.Drop.status.html

GDK explicitly documents that a destination must not restrict the actions it advertises to the current value of `GdkDrop::actions()`; that value can change during negotiation.

### 2. Format policy and action policy need to be separated

Scratchpad has two different safety policies:

- URI/file drops: COPY only. Never acknowledge a MOVE-only file drop.
- text-like drops: prefer COPY, but permit MOVE because Chromium on native Wayland has been observed to advertise selected text as MOVE-only.

The rewrite applies that policy in `DropTargetAsync::accept` by changing the target's supported action mask *before* GTK's built-in enter/motion handlers negotiate the actual action. Scratchpad no longer calls or emulates `gdk_drop_status()` itself.

### 3. Hide/reveal should not resize/remap the layer surface

Layer-shell stretches a surface automatically when it is anchored to opposite edges. GTK/GDK also supports an input region on a surface; pointer events outside that region pass to the surface below.

Relevant upstream docs:

- https://wmww.github.io/gtk4-layer-shell/gtk4-layer-shell-GTK4-Layer-Shell.html
- https://docs.gtk.org/gdk4/method.Surface.set_input_region.html

The rewritten panel therefore keeps one fixed, mapped, full-height layer surface. When hidden, only a thin edge strip is part of its input region. When revealed, the input region expands to the visible panel width. The remainder of the transparent surface is click-through.

This avoids changing Wayland surface geometry during an active foreign drag and preserves the same mapped surface from edge activation through drop.

## Current DnD policy

Inbound:

- `text/uri-list`: COPY only.
- `text/plain;charset=utf-8`, `text/plain`, `text/x-moz-url`: COPY or MOVE, with COPY preferred by GTK when available.
- Data transfer is asynchronous and bounded by `SCRATCHPAD_MAX_DROP_BYTES`.

Outbound:

- Scratchpad only advertises COPY.
- Text exposes plain text plus a lazily materialized temporary text file URI.
- Existing file/path and URL objects expose `text/uri-list`.

## Runtime diagnostics

```bash
SCRATCHPAD_DND_DEBUG=1 SCRATCHPAD_PANEL_DEBUG=1 ./scratchpad-ui
```

DnD diagnostics report accepted format class, source actions, target actions, the negotiated action at drop time, bytes read, persistence, and outbound source lifecycle.

Panel diagnostics report the actual GDK surface dimensions and input-region rectangle. This lets runtime tests distinguish compositor allocation issues from GTK child-layout issues.
