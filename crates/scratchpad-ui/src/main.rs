use gtk::gio::prelude::*;
use gtk::prelude::*;
use gtk::glib;
use scratchpad_core::*;
use std::{cell::RefCell, path::Path, rc::Rc, time::Duration};

#[cfg(feature = "layer-shell")]
use gtk4_layer_shell::{Edge, Layer, LayerShell};

fn main() {
    let app = gtk::Application::builder()
        .application_id("dev.systemscratchpad.Scratchpad")
        .build();
    app.connect_activate(build);
    app.run();
}

fn build(app: &gtk::Application) {
    let state = Rc::new(RefCell::new(UiState::default()));
    let win = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Scratchpad")
        .default_width(panel_width())
        .default_height(760)
        .build();

    configure_shell(&win);

    let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    root.add_css_class("scratchpad");

    let rail = gtk::Box::new(gtk::Orientation::Vertical, 6);
    rail.set_width_request(58);
    rail.add_css_class("page-rail");

    let content = gtk::Box::new(gtk::Orientation::Vertical, 10);
    content.set_hexpand(true);

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
    scroll.set_child(Some(&flow));
    scroll.set_vexpand(true);
    content.append(&scroll);

    root.append(&rail);
    root.append(&content);
    win.set_child(Some(&root));

    install_css();
    install_drop_targets(&root, &flow, state.clone());
    load_pages_and_items(&rail, &flow, state.clone());
    install_refresh_timer(&flow, state);

    win.present();
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

fn install_drop_targets(
    root: &gtk::Box,
    flow: &gtk::FlowBox,
    state: Rc<RefCell<UiState>>,
) {
    let text_target = gtk::DropTarget::new(String::static_type(), gtk::gdk::DragAction::COPY);
    {
        let flow = flow.clone();
        let state = state.clone();
        text_target.connect_drop(move |_, value, _, _| {
            let Ok(text) = value.get::<String>() else {
                return false;
            };
            accept_text_drop(text, &flow, &state)
        });
    }
    root.add_controller(text_target);

    let file_target =
        gtk::DropTarget::new(gtk::gdk::FileList::static_type(), gtk::gdk::DragAction::COPY);
    {
        let flow = flow.clone();
        let state = state.clone();
        file_target.connect_drop(move |_, value, _, _| {
            let Ok(files) = value.get::<gtk::gdk::FileList>() else {
                return false;
            };

            let mut accepted = false;
            for file in files.files() {
                let object = if let Some(path) = file.path() {
                    object_from_path(&path)
                } else {
                    object_from_uri(file.uri().as_str())
                };

                if let Some(object) = object {
                    accepted |= submit_object(object, &flow, &state);
                }
            }
            accepted
        });
    }
    root.add_controller(file_target);
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
        _ => false,
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

    if !matches!(object.lifecycle, Lifecycle::Available) {
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

    outer.upcast()
}

fn text_preview(object: &Object) -> Option<String> {
    object.representations.iter().find_map(|representation| {
        if let StorageRef::InlineText { text } = &representation.storage {
            let mut preview = text.chars().take(420).collect::<String>();
            if text.chars().count() > 420 {
                preview.push('…');
            }
            Some(preview)
        } else {
            None
        }
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

#[derive(Default)]
struct UiState {
    active: Option<uuid::Uuid>,
    signature: String,
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
fn configure_shell(window: &gtk::ApplicationWindow) {
    if std::env::var_os("SCRATCHPAD_NO_LAYER_SHELL").is_some()
        || !gtk4_layer_shell::is_supported()
    {
        return;
    }

    window.init_layer_shell();
    window.set_namespace(Some("system-scratchpad"));
    window.set_layer(Layer::Overlay);

    match std::env::var("SCRATCHPAD_EDGE")
        .unwrap_or_else(|_| "right".into())
        .as_str()
    {
        "left" => window.set_anchor(Edge::Left, true),
        "top" => window.set_anchor(Edge::Top, true),
        "bottom" => window.set_anchor(Edge::Bottom, true),
        _ => window.set_anchor(Edge::Right, true),
    }

    window.set_anchor(Edge::Top, true);
    window.set_anchor(Edge::Bottom, true);
    window.set_exclusive_zone(0);
}

#[cfg(not(feature = "layer-shell"))]
fn configure_shell(_: &gtk::ApplicationWindow) {}

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

fn panel_width() -> i32 {
    std::env::var("SCRATCHPAD_PANEL_WIDTH")
        .ok()
        .and_then(|x| x.parse().ok())
        .unwrap_or(430)
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
        ".scratchpad{background:#121419;padding:12px}         .page-rail{padding:4px 8px 4px 2px}         .page-dot{min-width:40px;min-height:40px;border-radius:12px;background:#252a33}         .object-card{background:#1d2129;border:1px solid #2b313d;border-radius:16px;padding:14px;margin:2px;box-shadow:0 3px 10px rgba(0,0,0,.22)}         .object-card:hover{background:#222731;border-color:#3a4352}         .type-badge{font-size:10px;font-weight:700;letter-spacing:1px;opacity:.62}         .state-badge{font-size:9px;font-weight:700;opacity:.68}         .text-preview{font-size:16px;line-height:1.35;color:#eef1f6}         .url-domain{font-size:16px;font-weight:700;color:#eef1f6}         .url-preview{font-size:12px;opacity:.68}         .compact-icon{font-size:23px;min-width:34px;opacity:.78}         .object-title{font-size:15px;font-weight:650;color:#eef1f6}         .secondary-text{font-size:11px;opacity:.58}         .dim-label{opacity:.58}",
    );
    gtk::style_context_add_provider_for_display(
        &gtk::gdk::Display::default().unwrap(),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
