# Feasibility gates

The architecture intentionally keeps uncertain compositor/toolkit behavior isolated behind spikes.

## Gate A — foreign DnD survives edge expansion
Test Firefox, Chromium, Dolphin and Nautilus dragging into a 2 px persistent layer-shell surface while it expands to the full panel width. Required on Hyprland first, then Sway and KDE Plasma.

## Gate B — multi-representation inbound reads
For browser text, browser image and file-manager sources, record advertised MIME types and attempt bounded sequential reads before finalizing the drop. Never fetch every large representation blindly.

## Gate C — external MOVE interoperability
Do not ship destructive external-reference MOVE until Dolphin, Nautilus, Thunar, generic GTK, generic Qt and XWayland receivers have been tested. COPY is the current safe default.

## Gate D — portal persistence
Files received through XDG FileTransfer or transient FUSE paths must be imported into managed storage if the object is meant to survive a session.

## Gate E — performance
The canvas must remain responsive with hundreds of rich cards. If ordinary GTK child widgets become expensive, move card painting/hit-testing into a custom `DrawingArea`/snapshot-based canvas while keeping accessibility proxies.
