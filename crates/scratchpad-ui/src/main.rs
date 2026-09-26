use gtk::gio::prelude::*;
use gtk::prelude::*;
use gtk::glib;
use scratchpad_core::*;
use std::{cell::RefCell, path::{Path, PathBuf}, rc::Rc, time::Duration};

#[cfg(feature = "layer-shell")]
use gtk4_layer_shell::{Edge, Layer, LayerShell};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PanelEdge {
    Left,
    Right,
    Top,
    Bottom,
}

impl PanelEdge {
    fn from_env() -> Self {
        match std::env::var("SCRATCHPAD_EDGE")
            .unwrap_or_else(|_| "right".into())
            .to_ascii_lowercase()
            .as_str()
        {
            "left" => Self::Left,
            "top" => Self::Top,
            "bottom" => Self::Bottom,
            _ => Self::Right,
        }
    }

    fn root_orientation(self) -> gtk::Orientation {
        match self {
            Self::Left | Self::Right => gtk::Orientation::Horizontal,
            Self::Top | Self::Bottom => gtk::Orientation::Vertical,
        }
    }

    fn hotspot_first(self) -> bool {
        matches!(self, Self::Left | Self::Top)
    }

    fn is_vertical_panel(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }
}

struct UiState {
    active: Option<uuid::Uuid>,
    signature: String,
    drag_active: bool,
    collapse_timer: Option<glib::SourceId>,
    panel_extent: i32,
}

impl UiState {
    fn new(edge: PanelEdge) -> Self {
        Self {
            active: None,
            signature: String::new(),
            drag_active: false,
            collapse_timer: None,
            panel_extent: load_panel_extent(edge),
        }
    }
}

fn main() {
    let app = gtk::Application::builder()
        .application_id("dev.systemscratchpad.Scratchpad")
        .build();
    app.connect_activate(build);
    app.run();
}

fn build(app: &gtk::Application) {
    let edge = PanelEdge::from_env();
    let state = Rc::new(RefCell::new(UiState::new(edge)));

    let win = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Scratchpad")
        .decorated(false)
        .resizable(false)
        .build();

    configure_shell(&win, edge);

    let root = gtk::Box::new(edge.root_orientation(), 0);
    root.add_css_class("scratchpad-root");

    let hotspot = gtk::Box::new(gtk::Orientation::Vertical, 0);
    hotspot.add_css_class("edge-hotspot");
    if edge.is_vertical_panel() {
        hotspot.set_width_request(edge_width());
        hotspot.set_vexpand(true);
    } else {
        hotspot.set_height_request(edge_width());
        hotspot.set_hexpand(true);
    }

    let panel = gtk::Box::new(edge.root_orientation(), 0);
    panel.add_css_class("scratchpad-panel");
    panel.set_hexpand(true);
    panel.set_vexpand(true);

    let panel_body = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    panel_body.set_hexpand(true);
    panel_body.set_vexpand(true);

    let resize_handle = gtk::Box::new(
        if edge.is_vertical_panel() {
            gtk::Orientation::Vertical
        } else {
            gtk::Orientation::Horizontal
        },
        0,
    );
    resize_handle.add_css_class("resize-handle");
    if edge.is_vertical_panel() {
        resize_handle.set_width_request(10);
        resize_handle.set_vexpand(true);
        resize_handle.set_cursor_from_name(Some("col-resize"));
    } else {
        resize_handle.set_height_request(10);
        resize_handle.set_hexpand(true);
        resize_handle.set_cursor_from_name(Some("row-resize"));
    }

    {
        let extent = state.borrow().panel_extent;
        if edge.is_vertical_panel() {
            panel.set_width_request(extent);
        } else {
            panel.set_height_request(extent);
        }
    }

    let rail = gtk::Box::new(gtk::Orientation::Vertical, 6);
    rail.set_width_request(58);
    rail.add_css_class("page-rail");

    let content = gtk::Box::new(gtk::Orientation::Vertical, 10);
    content.set_hexpand(true);
    content.set_vexpand(true);

    let header = gtk::Label::new(Some("Scratchpad"));
    header.set_xalign(0.0);
    header.add_css_class("title-2");
    content.append(&header);

    let hint = gtk::Label::new(Some("Drop text, URLs, or files anywhere"));
    hint.set_xalign(0.0);
    hint.add_css_class("dim-label");
    content.append(&hint);

    let scroll = gtk::ScrolledWindow::new();
    let flow = gtk::FlowBox::new();
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_max_children_per_line(1);
    flow.set_row_spacing(10);
    flow.set_column_spacing(10);
    flow.set_valign(gtk::Align::Start);
    flow.set_hexpand(true);
    scroll.set_child(Some(&flow));
    scroll.set_vexpand(true);
    content.append(&scroll);

    panel_body.append(&rail);
    panel_body.append(&content);

    match edge {
        PanelEdge::Right | PanelEdge::Bottom => {
            panel.append(&resize_handle);
            panel.append(&panel_body);
        }
        PanelEdge::Left | PanelEdge::Top => {
            panel.append(&panel_body);
            panel.append(&resize_handle);
        }
    }

    install_resize_handle(&win, &panel, &resize_handle, state.clone(), edge);

    if edge.hotspot_first() {
        root.append(&hotspot);
        root.append(&panel);
    } else {
        root.append(&panel);
        root.append(&hotspot);
    }

    win.set_child(Some(&root));

    install_css();
    install_panel_reveal(&win, &root, &panel, state.clone(), edge);
    install_drop_target(&root, &flow, &panel, &win, state.clone(), edge);
    load_pages_and_items(&rail, &flow, state.clone());
    install_refresh_timer(&flow, state.clone());

    panel.set_visible(false);
    set_window_revealed(&win, edge, false, state.borrow().panel_extent);
    win.present();

    if std::env::var_os("SCRATCHPAD_START_OPEN").is_some() {
        reveal_panel(&win, &panel, &state, edge);
    }
}

fn install_panel_reveal(
    win: &gtk::ApplicationWindow,
    root: &gtk::Box,
    panel: &gtk::Box,
    state: Rc<RefCell<UiState>>,
    edge: PanelEdge,
) {
    let motion = gtk::EventControllerMotion::new();

    {
        let win = win.clone();
        let panel = panel.clone();
        let state = state.clone();
        motion.connect_enter(move |_, _, _| {
            reveal_panel(&win, &panel, &state, edge);
        });
    }

    {
        let win = win.clone();
        let panel = panel.clone();
        let state = state.clone();
        motion.connect_leave(move |_| {
            schedule_collapse(&win, &panel, &state, edge);
        });
    }

    root.add_controller(motion);

    // Drag motion has its own event path. Keeping this controller on the same
    // persistent surface lets a foreign drag reveal the panel without remapping it.
    let drag_motion = gtk::DropControllerMotion::new();

    {
        let win = win.clone();
        let panel = panel.clone();
        let state = state.clone();
        drag_motion.connect_enter(move |_, _, _| {
            state.borrow_mut().drag_active = true;
            reveal_panel(&win, &panel, &state, edge);
        });
    }

    {
        let win = win.clone();
        let panel = panel.clone();
        let state = state.clone();
        drag_motion.connect_leave(move |_| {
            state.borrow_mut().drag_active = false;
            schedule_collapse(&win, &panel, &state, edge);
        });
    }

    root.add_controller(drag_motion);
}

fn reveal_panel(
    win: &gtk::ApplicationWindow,
    panel: &gtk::Box,
    state: &Rc<RefCell<UiState>>,
    edge: PanelEdge,
) {
    if let Some(timer) = state.borrow_mut().collapse_timer.take() {
        timer.remove();
    }

    if !panel.is_visible() {
        panel.set_visible(true);
        set_window_revealed(win, edge, true, state.borrow().panel_extent);
        win.queue_resize();
    }
}

fn schedule_collapse(
    win: &gtk::ApplicationWindow,
    panel: &gtk::Box,
    state: &Rc<RefCell<UiState>>,
    edge: PanelEdge,
) {
    if state.borrow().drag_active {
        return;
    }

    if let Some(timer) = state.borrow_mut().collapse_timer.take() {
        timer.remove();
    }

    let win = win.clone();
    let panel = panel.clone();
    let state_for_timer = state.clone();

    let id = glib::timeout_add_local_once(Duration::from_millis(collapse_ms()), move || {
        let mut state = state_for_timer.borrow_mut();
        state.collapse_timer = None;
        if state.drag_active {
            return;
        }
        panel.set_visible(false);
        set_window_revealed(&win, edge, false, state.panel_extent);
        win.queue_resize();
    });

    state.borrow_mut().collapse_timer = Some(id);
}

fn set_window_revealed(
    win: &gtk::ApplicationWindow,
    edge: PanelEdge,
    revealed: bool,
    panel_extent: i32,
) {
    match edge {
        PanelEdge::Left | PanelEdge::Right => {
            let width = if revealed {
                panel_extent + edge_width()
            } else {
                edge_width()
            };
            // For layer-shell, an axis anchored on both sides must request 0
            // to stretch to the compositor-provided output extent.
            win.set_default_size(width, 0);
        }
        PanelEdge::Top | PanelEdge::Bottom => {
            let height = if revealed {
                panel_extent + edge_width()
            } else {
                edge_width()
            };
            // For layer-shell, an axis anchored on both sides must request 0
            // to stretch to the compositor-provided output extent.
            win.set_default_size(0, height);
        }
    }
}


fn install_resize_handle(
    win: &gtk::ApplicationWindow,
    panel: &gtk::Box,
    handle: &gtk::Box,
    state: Rc<RefCell<UiState>>,
    edge: PanelEdge,
) {
    let gesture = gtk::GestureDrag::new();
    let start_extent = Rc::new(RefCell::new(0_i32));

    {
        let start_extent = start_extent.clone();
        let state = state.clone();
        gesture.connect_drag_begin(move |_, _, _| {
            *start_extent.borrow_mut() = state.borrow().panel_extent;
        });
    }

    {
        let start_extent = start_extent.clone();
        let state = state.clone();
        let panel = panel.clone();
        let win = win.clone();

        gesture.connect_drag_update(move |_, dx, dy| {
            let start = *start_extent.borrow();
            let delta = match edge {
                PanelEdge::Right => -dx,
                PanelEdge::Left => dx,
                PanelEdge::Bottom => -dy,
                PanelEdge::Top => dy,
            };

            let extent = (start as f64 + delta)
                .round()
                .clamp(min_panel_extent() as f64, max_panel_extent() as f64)
                as i32;

            state.borrow_mut().panel_extent = extent;

            if edge.is_vertical_panel() {
                panel.set_width_request(extent);
            } else {
                panel.set_height_request(extent);
            }

            set_window_revealed(&win, edge, true, extent);
            win.queue_resize();
        });
    }

    {
        let state = state.clone();
        gesture.connect_drag_end(move |_, _, _| {
            save_panel_extent(edge, state.borrow().panel_extent);
        });
    }

    handle.add_controller(gesture);
}

fn panel_extent_path(edge: PanelEdge) -> PathBuf {
    let name = if edge.is_vertical_panel() {
        "panel-width"
    } else {
        "panel-height"
    };
    Paths::discover().data_dir.join(name)
}

fn load_panel_extent(edge: PanelEdge) -> i32 {
    let env_value = if edge.is_vertical_panel() {
        std::env::var("SCRATCHPAD_PANEL_WIDTH").ok()
    } else {
        std::env::var("SCRATCHPAD_PANEL_HEIGHT").ok()
    };

    if let Some(value) = env_value.and_then(|x| x.parse::<i32>().ok()) {
        return value.clamp(min_panel_extent(), max_panel_extent());
    }

    std::fs::read_to_string(panel_extent_path(edge))
        .ok()
        .and_then(|s| s.trim().parse::<i32>().ok())
        .unwrap_or(if edge.is_vertical_panel() { 430 } else { 360 })
        .clamp(min_panel_extent(), max_panel_extent())
}

fn save_panel_extent(edge: PanelEdge, extent: i32) {
    let path = panel_extent_path(edge);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, extent.to_string());
}

fn min_panel_extent() -> i32 {
    280
}

fn max_panel_extent() -> i32 {
    960
}

fn load_pages_and_items(
    rail: &gtk::Box,
    flow: &gtk::FlowBox,
    state: Rc<RefCell<UiState>>,
) {
    let paths = Paths::discover();
    match ipc(&paths, Request::ListPages) {
        Ok(Response::Pages(pages)) => {
            for page in pages {
                let button = gtk::Button::with_label(&page_glyph(&page.name));
                button.set_tooltip_text(Some(&page.name));
                button.add_css_class("page-dot");

                let page_id = page.id;
                let click_flow = flow.clone();
                let click_state = state.clone();
                button.connect_clicked(move |_| {
                    {
                        let mut s = click_state.borrow_mut();
                        s.active = Some(page_id);
                        s.signature.clear();
                    }
                    refresh_page(page_id, &click_flow, &click_state, true);
                });

                install_hover_switch(&button, page_id, flow.clone(), state.clone());
                rail.append(&button);

                if state.borrow().active.is_none() {
                    state.borrow_mut().active = Some(page_id);
                    refresh_page(page_id, flow, &state, true);
                }
            }
        }
        _ => {
            let label = gtk::Label::new(Some("Daemon offline"));
            rail.append(&label);
        }
    }
}

fn install_refresh_timer(flow: &gtk::FlowBox, state: Rc<RefCell<UiState>>) {
    let flow = flow.clone();
    glib::timeout_add_local(Duration::from_millis(refresh_ms()), move || {
        if let Some(page_id) = state.borrow().active {
            refresh_page(page_id, &flow, &state, false);
        }
        glib::ControlFlow::Continue
    });
}

fn refresh_page(
    page_id: uuid::Uuid,
    flow: &gtk::FlowBox,
    state: &Rc<RefCell<UiState>>,
    force: bool,
) {
    let paths = Paths::discover();
    let Ok(Response::Page(snapshot)) = ipc(&paths, Request::GetPage { page_id }) else {
        return;
    };

    let signature = snapshot
        .items
        .iter()
        .map(|(object, placement)| {
            format!(
                "{}:{}:{}:{}:{}",
                object.id,
                object.updated_at.timestamp_millis(),
                placement.x,
                placement.y,
                placement.z_index
            )
        })
        .collect::<Vec<_>>()
        .join("|");

    if !force && state.borrow().signature == signature {
        return;
    }

    while let Some(child) = flow.first_child() {
        flow.remove(&child);
    }

    for (object, _) in snapshot.items {
        flow.insert(&card(&object), -1);
    }

    state.borrow_mut().signature = signature;
}

fn install_drop_target(
    root: &gtk::Box,
    flow: &gtk::FlowBox,
    panel: &gtk::Box,
    win: &gtk::ApplicationWindow,
    state: Rc<RefCell<UiState>>,
    edge: PanelEdge,
) {
    // MIME formats are the interoperable path for cross-process Wayland DnD.
    // GTypes are primarily useful for in-process transfers, so use DropTargetAsync.
    let formats = gtk::gdk::ContentFormats::new(&[
        "text/uri-list",
        "text/plain;charset=utf-8",
        "text/plain",
        "text/x-moz-url",
    ]);
    let target = gtk::DropTargetAsync::new(
        Some(formats),
        gtk::gdk::DragAction::COPY,
    );

    target.connect_accept(|_, drop| {
        let formats = drop.formats();
        let accepted = [
            "text/uri-list",
            "text/plain;charset=utf-8",
            "text/plain",
            "text/x-moz-url",
        ]
        .iter()
        .any(|mime| formats.contain_mime_type(mime));

        if dnd_debug() {
            eprintln!(
                "[scratchpad-dnd] inbound formats={} accepted={accepted}",
                formats.to_str()
            );
        }
        accepted
    });

    target.connect_drag_enter(|_, drop, _, _| {
        let action = negotiated_drop_action(drop);
        if dnd_debug() {
            eprintln!(
                "[scratchpad-dnd] drag-enter actions={:?} chosen={:?} formats={}",
                drop.actions(),
                action,
                drop.formats().to_str()
            );
        }
        action
    });

    target.connect_drag_motion(|_, drop, _, _| negotiated_drop_action(drop));

    target.connect_drag_leave(|_, _| {
        if dnd_debug() {
            eprintln!("[scratchpad-dnd] drag-leave");
        }
    });

    {
        let win = win.clone();
        let panel = panel.clone();
        let flow = flow.clone();
        let state = state.clone();

        target.connect_drop(move |_, drop, _, _| {
            let negotiated_action = negotiated_drop_action(drop);
            if negotiated_action.is_empty() {
                if dnd_debug() {
                    eprintln!("[scratchpad-dnd] drop rejected: no compatible action");
                }
                return false;
            }

            state.borrow_mut().drag_active = true;
            reveal_panel(&win, &panel, &state, edge);

            let drop = drop.clone();
            let flow = flow.clone();
            let state = state.clone();
            let win = win.clone();
            let panel = panel.clone();

            glib::MainContext::default().spawn_local(async move {
                let result = read_foreign_drop(&drop).await;
                let accepted = match result {
                    Ok((mime, bytes)) => {
                        if dnd_debug() {
                            eprintln!(
                                "[scratchpad-dnd] received mime={} bytes={}",
                                mime,
                                bytes.len()
                            );
                        }
                        handle_inbound_payload(&mime, &bytes, &flow, &state)
                    }
                    Err(err) => {
                        eprintln!("[scratchpad-dnd] read failed: {err:#}");
                        false
                    }
                };

                drop.finish(if accepted {
                    negotiated_action
                } else {
                    gtk::gdk::DragAction::empty()
                });

                state.borrow_mut().drag_active = false;
                schedule_collapse(&win, &panel, &state, edge);
            });

            true
        });
    }

    root.add_controller(target);
}

fn negotiated_drop_action(drop: &gtk::gdk::Drop) -> gtk::gdk::DragAction {
    let actions = drop.actions();
    if actions.contains(gtk::gdk::DragAction::COPY) {
        return gtk::gdk::DragAction::COPY;
    }

    // Chromium/Wayland commonly exposes selected text as MOVE-only. Treating
    // that as an inbound import is safe for Scratchpad's database because we
    // never delete or mutate the source ourselves. Do not accept MOVE-only
    // file URI drops here; those remain COPY-only until the file-manager
    // interoperability matrix is complete.
    let formats = drop.formats();
    let has_uri_list = formats.contain_mime_type("text/uri-list");
    let has_text = formats.contain_mime_type("text/plain;charset=utf-8")
        || formats.contain_mime_type("text/plain")
        || formats.contain_mime_type("text/html")
        || formats.contain_mime_type("text/x-moz-url");

    if actions.contains(gtk::gdk::DragAction::MOVE) && has_text && !has_uri_list {
        gtk::gdk::DragAction::MOVE
    } else {
        gtk::gdk::DragAction::empty()
    }
}

async fn read_foreign_drop(
    drop: &gtk::gdk::Drop,
) -> anyhow::Result<(String, Vec<u8>)> {
    let (stream, mime) = drop
        .read_future(
            &[
                "text/uri-list",
                "text/plain;charset=utf-8",
                "text/plain",
                "text/x-moz-url",
            ],
            glib::Priority::DEFAULT,
        )
        .await?;

    let mut out = Vec::new();
    loop {
        let chunk = stream
            .read_bytes_future(64 * 1024, glib::Priority::DEFAULT)
            .await?;
        if chunk.is_empty() {
            break;
        }
        if out.len() + chunk.len() > max_drop_bytes() {
            anyhow::bail!("drop exceeded {} bytes", max_drop_bytes());
        }
        out.extend_from_slice(chunk.as_ref());
    }

    Ok((mime.to_string(), out))
}

fn handle_inbound_payload(
    mime: &str,
    bytes: &[u8],
    flow: &gtk::FlowBox,
    state: &Rc<RefCell<UiState>>,
) -> bool {
    if mime == "text/uri-list" {
        return handle_uri_list(bytes, flow, state);
    }

    let mut text = String::from_utf8_lossy(bytes).into_owned();

    // Firefox may expose text/x-moz-url as URL + title. Prefer the first line.
    if mime == "text/x-moz-url" {
        if text.as_bytes().windows(2).any(|w| w == [0, 0]) {
            let utf16 = bytes
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect::<Vec<_>>();
            text = String::from_utf16_lossy(&utf16);
        }
        text = text.lines().next().unwrap_or_default().to_string();
    }

    accept_text_drop(text, flow, state)
}

fn handle_uri_list(
    bytes: &[u8],
    flow: &gtk::FlowBox,
    state: &Rc<RefCell<UiState>>,
) -> bool {
    let text = String::from_utf8_lossy(bytes);
    let mut accepted = false;

    for line in text.lines() {
        let uri = line.trim();
        if uri.is_empty() || uri.starts_with('#') {
            continue;
        }

        let object = url::Url::parse(uri).ok().and_then(|parsed| {
            if parsed.scheme() == "file" {
                parsed
                    .to_file_path()
                    .ok()
                    .and_then(|path| object_from_path(&path))
            } else {
                object_from_uri(uri)
            }
        });

        if let Some(object) = object {
            accepted |= submit_object(object, flow, state);
        }
    }

    accepted
}

fn accept_text_drop(
    text: String,
    flow: &gtk::FlowBox,
    state: &Rc<RefCell<UiState>>,
) -> bool {
    let text = text.trim().to_string();
    if text.is_empty() {
        return false;
    }

    let source = SourceDescriptor::DragDrop {
        app_id: None,
        offered_mime_types: vec!["text/plain;charset=utf-8".into()],
    };

    let object = if url::Url::parse(&text).is_ok() {
        Object::new(ObjectKind::Url, Some(text.clone()), source)
            .with_representation(
                "text/uri-list",
                RepresentationRole::Primary,
                StorageRef::Uri { uri: text.clone() },
                Some(text.len() as u64),
            )
            .with_representation(
                "text/plain;charset=utf-8",
                RepresentationRole::Secondary,
                StorageRef::InlineText { text: text.clone() },
                Some(text.len() as u64),
            )
    } else {
        Object::new(ObjectKind::Text, first_line(&text), source).with_representation(
            "text/plain;charset=utf-8",
            RepresentationRole::Primary,
            StorageRef::InlineText { text: text.clone() },
            Some(text.len() as u64),
        )
    };

    submit_object(object, flow, state)
}

fn object_from_path(path: &Path) -> Option<Object> {
    let path = std::fs::canonicalize(path).ok()?;
    let metadata = std::fs::metadata(&path).ok()?;
    let kind = if metadata.is_dir() {
        ObjectKind::Directory
    } else {
        kind_for_path(&path)
    };
    let mime = if metadata.is_dir() {
        "inode/directory".to_string()
    } else {
        mime_for_path(&path)
    };

    let title = path.file_name().map(|s| s.to_string_lossy().into_owned());
    Some(
        Object::new(
            kind,
            title,
            SourceDescriptor::DragDrop {
                app_id: None,
                offered_mime_types: vec!["text/uri-list".into()],
            },
        )
        .with_representation(
            mime,
            RepresentationRole::Primary,
            StorageRef::ExternalPath {
                path: path.display().to_string(),
            },
            Some(metadata.len()),
        ),
    )
}

fn object_from_uri(uri: &str) -> Option<Object> {
    url::Url::parse(uri).ok()?;
    Some(
        Object::new(
            ObjectKind::Url,
            Some(uri.to_string()),
            SourceDescriptor::DragDrop {
                app_id: None,
                offered_mime_types: vec!["text/uri-list".into()],
            },
        )
        .with_representation(
            "text/uri-list",
            RepresentationRole::Primary,
            StorageRef::Uri {
                uri: uri.to_string(),
            },
            Some(uri.len() as u64),
        ),
    )
}

fn submit_object(
    object: Object,
    flow: &gtk::FlowBox,
    state: &Rc<RefCell<UiState>>,
) -> bool {
    let Some(page_id) = state.borrow().active else {
        return false;
    };
    let paths = Paths::discover();
    let request = Request::AddObject {
        object,
        page_id: Some(page_id),
        placement: None,
    };

    match ipc(&paths, request) {
        Ok(Response::Created { .. }) => {
            state.borrow_mut().signature.clear();
            refresh_page(page_id, flow, state, true);
            true
        }
        Ok(other) => {
            eprintln!("[scratchpad-dnd] daemon rejected drop: {other:?}");
            false
        }
        Err(err) => {
            eprintln!("[scratchpad-dnd] daemon error: {err:#}");
            false
        }
    }
}

fn card(object: &Object) -> gtk::Widget {
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 10);
    outer.set_width_request(300);
    outer.set_hexpand(true);
    outer.add_css_class("object-card");

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let badge = gtk::Label::new(Some(&kind_name(&object.kind).to_ascii_uppercase()));
    badge.add_css_class("type-badge");
    header.append(&badge);

    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    header.append(&spacer);

    if !matches!(&object.lifecycle, Lifecycle::Available) {
        let lifecycle = gtk::Label::new(Some(lifecycle_name(&object.lifecycle)));
        lifecycle.add_css_class("state-badge");
        header.append(&lifecycle);
    }

    outer.append(&header);

    match object.kind {
        ObjectKind::Text => {
            let body = gtk::Label::new(Some(
                text_preview(object)
                    .as_deref()
                    .unwrap_or(object.title.as_deref().unwrap_or("Empty text")),
            ));
            body.set_wrap(true);
            body.set_lines(6);
            body.set_xalign(0.0);
            body.set_yalign(0.0);
            body.add_css_class("text-preview");
            outer.append(&body);
        }
        ObjectKind::Url => {
            let url_text = primary_uri(object)
                .or_else(|| text_preview(object))
                .unwrap_or_else(|| object.title.clone().unwrap_or_default());
            let domain = url::Url::parse(&url_text)
                .ok()
                .and_then(|u| u.host_str().map(ToOwned::to_owned))
                .unwrap_or_else(|| "Link".into());

            let domain_label = gtk::Label::new(Some(&domain));
            domain_label.set_xalign(0.0);
            domain_label.add_css_class("url-domain");
            outer.append(&domain_label);

            let url_label = gtk::Label::new(Some(&url_text));
            url_label.set_wrap(true);
            url_label.set_lines(3);
            url_label.set_xalign(0.0);
            url_label.add_css_class("url-preview");
            outer.append(&url_label);
        }
        _ => {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);

            let icon = gtk::Label::new(Some(icon_for(&object.kind)));
            icon.add_css_class("compact-icon");
            row.append(&icon);

            let details = gtk::Box::new(gtk::Orientation::Vertical, 4);
            details.set_hexpand(true);

            let title = gtk::Label::new(Some(object.title.as_deref().unwrap_or("Untitled")));
            title.set_wrap(true);
            title.set_lines(3);
            title.set_xalign(0.0);
            title.add_css_class("object-title");
            details.append(&title);

            if let Some(path) = external_path(object) {
                let secondary = gtk::Label::new(Some(&path));
                secondary.set_wrap(true);
                secondary.set_lines(2);
                secondary.set_xalign(0.0);
                secondary.add_css_class("secondary-text");
                details.append(&secondary);
            }

            row.append(&details);
            outer.append(&row);
        }
    }

    let tooltip = object
        .representations
        .iter()
        .map(|r| r.mime_type.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if !tooltip.is_empty() {
        outer.set_tooltip_text(Some(&tooltip));
    }

    install_drag_source(&outer, object.clone());
    outer.upcast()
}

fn install_drag_source(widget: &gtk::Box, object: Object) {
    let source = gtk::DragSource::new();
    source.set_actions(gtk::gdk::DragAction::COPY);

    source.connect_prepare(move |_, _, _| {
        let provider = drag_provider(&object);
        if dnd_debug() {
            if let Some(provider) = &provider {
                eprintln!(
                    "[scratchpad-dnd] outbound prepare object={} formats={}",
                    object.id,
                    provider.formats().to_str()
                );
            } else {
                eprintln!(
                    "[scratchpad-dnd] outbound prepare object={} has no provider",
                    object.id
                );
            }
        }
        provider
    });

    source.connect_drag_begin(|_, drag| {
        if dnd_debug() {
            eprintln!(
                "[scratchpad-dnd] outbound drag-begin actions={:?}",
                drag.actions()
            );
        }
    });

    source.connect_drag_end(|_, drag, delete_data| {
        if dnd_debug() {
            eprintln!(
                "[scratchpad-dnd] outbound drag-end selected={:?} delete_data={}",
                drag.selected_action(),
                delete_data
            );
        }
    });

    widget.add_controller(source);
}

fn drag_provider(object: &Object) -> Option<gtk::gdk::ContentProvider> {
    let mut providers = Vec::new();

    if let Some(text) = full_text(object) {
        providers.push(bytes_provider(
            "text/plain;charset=utf-8",
            text.as_bytes().to_vec(),
        ));
        providers.push(bytes_provider("text/plain", text.as_bytes().to_vec()));
    }

    if let Some(uri) = outbound_uri(object) {
        let uri_list = format!("{uri}\r\n");
        providers.push(bytes_provider("text/uri-list", uri_list.into_bytes()));
    } else if let Some(text) = full_text(object) {
        // File managers typically do not accept text/plain drops. Materialize
        // text cheaply before the drag and offer both raw text and a file URI.
        if let Some(uri) = materialize_text_export(object, &text) {
            let uri_list = format!("{uri}\r\n");
            providers.push(bytes_provider("text/uri-list", uri_list.into_bytes()));
        }
    }

    match providers.len() {
        0 => None,
        1 => providers.pop(),
        _ => Some(gtk::gdk::ContentProvider::new_union(&providers)),
    }
}

fn bytes_provider(mime: &str, data: Vec<u8>) -> gtk::gdk::ContentProvider {
    let bytes = glib::Bytes::from_owned(data);
    gtk::gdk::ContentProvider::for_bytes(mime, &bytes)
}

fn outbound_uri(object: &Object) -> Option<String> {
    if let Some(uri) = primary_uri(object) {
        return Some(uri);
    }

    external_path(object).and_then(|path| {
        url::Url::from_file_path(PathBuf::from(path))
            .ok()
            .map(|url| url.to_string())
    })
}

fn materialize_text_export(object: &Object, text: &str) -> Option<String> {
    let paths = Paths::discover();
    let export_dir = paths.exports();
    std::fs::create_dir_all(&export_dir).ok()?;

    let file = export_dir.join(format!("{}.txt", object.id));
    std::fs::write(&file, text).ok()?;

    url::Url::from_file_path(file)
        .ok()
        .map(|url| url.to_string())
}

fn full_text(object: &Object) -> Option<String> {
    object.representations.iter().find_map(|representation| {
        if let StorageRef::InlineText { text } = &representation.storage {
            Some(text.clone())
        } else {
            None
        }
    })
}

fn text_preview(object: &Object) -> Option<String> {
    full_text(object).map(|text| {
        let mut preview = text.chars().take(420).collect::<String>();
        if text.chars().count() > 420 {
            preview.push('…');
        }
        preview
    })
}

fn primary_uri(object: &Object) -> Option<String> {
    object.representations.iter().find_map(|representation| match &representation.storage {
        StorageRef::Uri { uri } => Some(uri.clone()),
        _ => None,
    })
}

fn external_path(object: &Object) -> Option<String> {
    object.representations.iter().find_map(|representation| match &representation.storage {
        StorageRef::ExternalPath { path } => Some(path.clone()),
        _ => None,
    })
}

fn lifecycle_name(lifecycle: &Lifecycle) -> &'static str {
    match lifecycle {
        Lifecycle::Available => "READY",
        Lifecycle::Changed => "CHANGED",
        Lifecycle::Missing => "MISSING",
        Lifecycle::PermissionDenied => "NO ACCESS",
        Lifecycle::Offline => "OFFLINE",
        Lifecycle::MovedKnown => "MOVED",
    }
}

fn install_hover_switch(
    button: &gtk::Button,
    page: uuid::Uuid,
    flow: gtk::FlowBox,
    state: Rc<RefCell<UiState>>,
) {
    let motion = gtk::DropControllerMotion::new();
    let pending = Rc::new(RefCell::new(None::<glib::SourceId>));

    {
        let pending = pending.clone();
        let state = state.clone();
        let flow = flow.clone();
        motion.connect_enter(move |_, _, _| {
            if let Some(old) = pending.borrow_mut().take() {
                old.remove();
            }
            let state = state.clone();
            let flow = flow.clone();
            *pending.borrow_mut() = Some(glib::timeout_add_local_once(
                Duration::from_millis(hover_ms()),
                move || {
                    {
                        let mut s = state.borrow_mut();
                        s.active = Some(page);
                        s.signature.clear();
                    }
                    refresh_page(page, &flow, &state, true);
                },
            ));
        });
    }

    {
        let pending = pending.clone();
        motion.connect_leave(move |_| {
            if let Some(old) = pending.borrow_mut().take() {
                old.remove();
            }
        });
    }

    button.add_controller(motion);
}

fn ipc(paths: &Paths, request: Request) -> anyhow::Result<Response> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;

    let mut stream = UnixStream::connect(&paths.socket)?;
    let envelope = RequestEnvelope::new(request);
    stream.write_all(serde_json::to_string(&envelope)?.as_bytes())?;
    stream.write_all(b"\n")?;

    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    Ok(serde_json::from_str::<ResponseEnvelope>(&line)?.response)
}

#[cfg(feature = "layer-shell")]
fn configure_shell(window: &gtk::ApplicationWindow, edge: PanelEdge) {
    if std::env::var_os("SCRATCHPAD_NO_LAYER_SHELL").is_some()
        || !gtk4_layer_shell::is_supported()
    {
        return;
    }

    window.init_layer_shell();
    window.set_namespace(Some("system-scratchpad"));
    window.set_layer(Layer::Top);
    window.set_exclusive_zone(0);

    match edge {
        PanelEdge::Left => {
            window.set_anchor(Edge::Left, true);
            window.set_anchor(Edge::Top, true);
            window.set_anchor(Edge::Bottom, true);
        }
        PanelEdge::Right => {
            window.set_anchor(Edge::Right, true);
            window.set_anchor(Edge::Top, true);
            window.set_anchor(Edge::Bottom, true);
        }
        PanelEdge::Top => {
            window.set_anchor(Edge::Top, true);
            window.set_anchor(Edge::Left, true);
            window.set_anchor(Edge::Right, true);
        }
        PanelEdge::Bottom => {
            window.set_anchor(Edge::Bottom, true);
            window.set_anchor(Edge::Left, true);
            window.set_anchor(Edge::Right, true);
        }
    }
}

#[cfg(not(feature = "layer-shell"))]
fn configure_shell(_: &gtk::ApplicationWindow, _: PanelEdge) {}

fn first_line(text: &str) -> Option<String> {
    text.lines()
        .next()
        .map(|line| line.chars().take(80).collect())
}

fn kind_for_path(path: &Path) -> ObjectKind {
    match path
        .extension()
        .and_then(|x| x.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" | "jpg" | "jpeg" | "webp" | "gif" => ObjectKind::Image,
        "mp4" | "mkv" | "webm" | "mov" => ObjectKind::Video,
        "mp3" | "flac" | "wav" | "ogg" => ObjectKind::Audio,
        "pdf" => ObjectKind::Pdf,
        _ => ObjectKind::File,
    }
}

fn mime_for_path(path: &Path) -> String {
    match kind_for_path(path) {
        ObjectKind::Image => "image/*",
        ObjectKind::Video => "video/*",
        ObjectKind::Audio => "audio/*",
        ObjectKind::Pdf => "application/pdf",
        _ => "application/octet-stream",
    }
    .into()
}

fn edge_width() -> i32 {
    std::env::var("SCRATCHPAD_EDGE_WIDTH")
        .ok()
        .and_then(|x| x.parse().ok())
        .unwrap_or(3)
        .max(1)
}

fn collapse_ms() -> u64 {
    std::env::var("SCRATCHPAD_COLLAPSE_MS")
        .ok()
        .and_then(|x| x.parse().ok())
        .unwrap_or(300)
}

fn hover_ms() -> u64 {
    std::env::var("SCRATCHPAD_HOVER_MS")
        .ok()
        .and_then(|x| x.parse().ok())
        .unwrap_or(400)
}

fn refresh_ms() -> u64 {
    std::env::var("SCRATCHPAD_REFRESH_MS")
        .ok()
        .and_then(|x| x.parse().ok())
        .unwrap_or(500)
}

fn max_drop_bytes() -> usize {
    std::env::var("SCRATCHPAD_MAX_DROP_BYTES")
        .ok()
        .and_then(|x| x.parse().ok())
        .unwrap_or(8 * 1024 * 1024)
}

fn dnd_debug() -> bool {
    std::env::var_os("SCRATCHPAD_DND_DEBUG").is_some()
}

fn kind_name(kind: &ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Text => "Text",
        ObjectKind::Url => "URL",
        ObjectKind::Image => "Image",
        ObjectKind::Video => "Video",
        ObjectKind::Audio => "Audio",
        ObjectKind::Pdf => "PDF",
        ObjectKind::File => "File",
        ObjectKind::Directory => "Folder",
        ObjectKind::App => "App",
        ObjectKind::Tool => "Tool",
        ObjectKind::Unknown => "Object",
    }
}

fn icon_for(kind: &ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Text => "≡",
        ObjectKind::Url => "↗",
        ObjectKind::Image => "▧",
        ObjectKind::Video => "▶",
        ObjectKind::Audio => "♫",
        ObjectKind::Pdf => "PDF",
        ObjectKind::File => "▤",
        ObjectKind::Directory => "▰",
        ObjectKind::App => "◈",
        ObjectKind::Tool => "⚙",
        ObjectKind::Unknown => "◇",
    }
}

fn page_glyph(name: &str) -> String {
    name.chars()
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_else(|| "•".into())
}

fn install_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(
        r#"
        .scratchpad-root {
            background: transparent;
        }
        .scratchpad-panel {
            background: #121419;
            padding: 12px;
        }
        .edge-hotspot {
            background: rgba(115, 145, 255, 0.24);
        }
        .resize-handle {
            background: rgba(115, 145, 255, 0.10);
            border-radius: 4px;
        }
        .resize-handle:hover {
            background: rgba(115, 145, 255, 0.55);
        }
        .scratchpad-root:drop(active) .scratchpad-panel {
            background: #161a21;
        }
        .page-rail {
            padding: 4px 8px 4px 2px;
        }
        .page-dot {
            min-width: 40px;
            min-height: 40px;
            border-radius: 12px;
            background: #252a33;
        }
        .object-card {
            background: #1d2129;
            border: 1px solid #2b313d;
            border-radius: 16px;
            padding: 14px;
            margin: 2px;
            box-shadow: 0 3px 10px rgba(0, 0, 0, 0.22);
        }
        .object-card:hover {
            background: #222731;
            border-color: #3a4352;
        }
        .type-badge {
            font-size: 10px;
            font-weight: 700;
            opacity: 0.62;
        }
        .state-badge {
            font-size: 9px;
            font-weight: 700;
            opacity: 0.68;
        }
        .text-preview {
            font-size: 16px;
            color: #eef1f6;
        }
        .url-domain {
            font-size: 16px;
            font-weight: 700;
            color: #eef1f6;
        }
        .url-preview {
            font-size: 12px;
            opacity: 0.68;
        }
        .compact-icon {
            font-size: 23px;
            min-width: 34px;
            opacity: 0.78;
        }
        .object-title {
            font-size: 15px;
            font-weight: 700;
            color: #eef1f6;
        }
        .secondary-text {
            font-size: 11px;
            opacity: 0.58;
        }
        .dim-label {
            opacity: 0.58;
        }
        "#,
    );
    gtk::style_context_add_provider_for_display(
        &gtk::gdk::Display::default().unwrap(),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
