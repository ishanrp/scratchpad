mod cli;
mod dnd;
mod panel;
mod service;

use anyhow::Result;
use clap::Parser;
use gtk::glib;
use gtk::prelude::*;
use scratchpad_core::*;
use std::{cell::RefCell, rc::Rc, time::Duration};

#[derive(Parser, Debug)]
#[command(name = "scratchpad")]
#[command(about = "Persistent Wayland scratchpad")]
struct Args {
    #[command(subcommand)]
    command: Option<cli::Command>,
}

struct ViewState {
    active: Option<uuid::Uuid>,
    signature: String,
}

fn main() -> Result<()> {
    let args = Args::parse();

    match args.command {
        Some(cli::Command::Serve) => service::run_headless(),
        Some(command) => {
            let request = cli::request(command)?;
            let response = ipc(&Paths::discover(), request)?;
            println!("{}", serde_json::to_string_pretty(&response)?);
            Ok(())
        }
        None => run_ui(),
    }
}

fn run_ui() -> Result<()> {
    let paths = Paths::discover();
    let _service = if service::socket_is_live(&paths.socket) {
        None
    } else {
        Some(service::start_embedded()?)
    };

    let app = gtk::Application::builder()
        .application_id("dev.systemscratchpad.Scratchpad")
        .build();

    app.connect_activate(build);
    app.run();
    Ok(())
}

fn build(app: &gtk::Application) {
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Scratchpad")
        .build();

    let shell = panel::build(&window);
    let panel_controller = shell.controller.clone();

    let rail = gtk::Box::new(gtk::Orientation::Vertical, 6);
    rail.set_width_request(58);
    rail.set_vexpand(true);
    rail.add_css_class("page-rail");

    let content = gtk::Box::new(gtk::Orientation::Vertical, 10);
    content.set_hexpand(true);
    content.set_vexpand(true);
    content.add_css_class("content-column");

    let header = gtk::Label::new(Some("Scratchpad"));
    header.set_xalign(0.0);
    header.add_css_class("title-2");
    content.append(&header);

    let hint = gtk::Label::new(Some("Drop text, URLs, or files anywhere"));
    hint.set_xalign(0.0);
    hint.add_css_class("dim-label");
    content.append(&hint);

    let scroll = gtk::ScrolledWindow::new();
    scroll.set_hexpand(true);
    scroll.set_vexpand(true);

    let flow = gtk::FlowBox::new();
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_max_children_per_line(1);
    flow.set_row_spacing(10);
    flow.set_column_spacing(10);
    flow.set_valign(gtk::Align::Start);
    flow.set_hexpand(true);
    scroll.set_child(Some(&flow));

    content.append(&scroll);
    shell.body.append(&rail);
    shell.body.append(&content);

    window.set_child(Some(&shell.root));
    install_css();

    let state = Rc::new(RefCell::new(ViewState {
        active: None,
        signature: String::new(),
    }));

    load_pages_and_items(
        &rail,
        &flow,
        state.clone(),
        panel_controller.clone(),
    );

    install_refresh_timer(
        &flow,
        state.clone(),
        panel_controller.clone(),
    );

    let submit: dnd::SubmitFn = {
        let flow = flow.clone();
        let state = state.clone();
        let panel_controller = panel_controller.clone();

        Rc::new(move |object| {
            submit_object(
                object,
                &flow,
                &state,
                &panel_controller,
            )
        })
    };

    dnd::install_inbound(
        &shell.root,
        panel_controller,
        submit,
    );

    window.present();
}

fn load_pages_and_items(
    rail: &gtk::Box,
    flow: &gtk::FlowBox,
    state: Rc<RefCell<ViewState>>,
    panel: panel::PanelController,
) {
    let paths = Paths::discover();

    match ipc(&paths, Request::ListPages) {
        Ok(Response::Pages(pages)) => {
            for page in pages {
                let button = gtk::Button::with_label(&page_glyph(&page.name));
                button.set_tooltip_text(Some(&page.name));
                button.add_css_class("page-dot");

                let page_id = page.id;

                {
                    let flow = flow.clone();
                    let state = state.clone();
                    let panel = panel.clone();

                    button.connect_clicked(move |_| {
                        {
                            let mut state = state.borrow_mut();
                            state.active = Some(page_id);
                            state.signature.clear();
                        }

                        refresh_page(
                            page_id,
                            &flow,
                            &state,
                            &panel,
                            true,
                        );
                    });
                }

                install_hover_switch(
                    &button,
                    page_id,
                    flow.clone(),
                    state.clone(),
                    panel.clone(),
                );

                rail.append(&button);

                if state.borrow().active.is_none() {
                    state.borrow_mut().active = Some(page_id);
                    refresh_page(
                        page_id,
                        flow,
                        &state,
                        &panel,
                        true,
                    );
                }
            }
        }
        _ => {
            let label = gtk::Label::new(Some("Service unavailable"));
            label.add_css_class("warning-label");
            rail.append(&label);
        }
    }
}

fn install_refresh_timer(
    flow: &gtk::FlowBox,
    state: Rc<RefCell<ViewState>>,
    panel: panel::PanelController,
) {
    let flow = flow.clone();

    glib::timeout_add_local(Duration::from_millis(refresh_ms()), move || {
        if let Some(page_id) = state.borrow().active {
            refresh_page(
                page_id,
                &flow,
                &state,
                &panel,
                false,
            );
        }

        glib::ControlFlow::Continue
    });
}

fn refresh_page(
    page_id: uuid::Uuid,
    flow: &gtk::FlowBox,
    state: &Rc<RefCell<ViewState>>,
    panel: &panel::PanelController,
    force: bool,
) {
    let paths = Paths::discover();

    let Ok(Response::Page(snapshot)) = ipc(
        &paths,
        Request::GetPage { page_id },
    ) else {
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
        flow.insert(&card(&object, panel.clone()), -1);
    }

    state.borrow_mut().signature = signature;
}

fn submit_object(
    object: Object,
    flow: &gtk::FlowBox,
    state: &Rc<RefCell<ViewState>>,
    panel: &panel::PanelController,
) -> bool {
    let Some(page_id) = state.borrow().active else {
        return false;
    };

    let paths = Paths::discover();

    match ipc(
        &paths,
        Request::AddObject {
            object,
            page_id: Some(page_id),
            placement: None,
        },
    ) {
        Ok(Response::Created { id }) => {
            if dnd_debug() {
                eprintln!("[scratchpad-dnd] persisted object={id}");
            }

            state.borrow_mut().signature.clear();
            refresh_page(
                page_id,
                flow,
                state,
                panel,
                true,
            );
            true
        }
        Ok(response) => {
            eprintln!(
                "[scratchpad-ui] service rejected object: {response:?}"
            );
            false
        }
        Err(err) => {
            eprintln!("[scratchpad-ui] service error: {err:#}");
            false
        }
    }
}

fn card(
    object: &Object,
    panel: panel::PanelController,
) -> gtk::Widget {
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 10);
    outer.set_width_request(360);
    outer.set_hexpand(true);
    outer.add_css_class("object-card");

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);

    let badge = gtk::Label::new(Some(
        &kind_name(&object.kind).to_ascii_uppercase(),
    ));
    badge.add_css_class("type-badge");
    header.append(&badge);

    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    header.append(&spacer);

    if !matches!(&object.lifecycle, Lifecycle::Available) {
        let lifecycle = gtk::Label::new(Some(
            lifecycle_name(&object.lifecycle),
        ));
        lifecycle.add_css_class("state-badge");
        header.append(&lifecycle);
    }

    if matches!(object.kind, ObjectKind::Text | ObjectKind::Url) {
        let label = if matches!(object.kind, ObjectKind::Url) {
            "LINK"
        } else {
            "FILE"
        };
        let export = gtk::Label::new(Some(label));
        export.add_css_class("drag-export-chip");
        export.set_tooltip_text(Some(if matches!(object.kind, ObjectKind::Url) {
            "Drag this handle when the destination expects a link/URI"
        } else {
            "Drag this handle when the destination expects a file"
        }));
        dnd::install_outbound(
            &export,
            object.clone(),
            panel.clone(),
            dnd::OutboundFlavor::Uri,
        );
        header.append(&export);
    }

    outer.append(&header);

    match object.kind {
        ObjectKind::Text => {
            let body = gtk::Label::new(Some(
                text_preview(object)
                    .as_deref()
                    .unwrap_or(
                        object.title.as_deref().unwrap_or("Empty text"),
                    ),
            ));
            body.set_wrap(true);
            body.set_lines(8);
            body.set_xalign(0.0);
            body.set_yalign(0.0);
            body.add_css_class("text-preview");
            outer.append(&body);
        }
        ObjectKind::Url => {
            let url_text = primary_uri(object)
                .or_else(|| text_preview(object))
                .unwrap_or_else(|| {
                    object.title.clone().unwrap_or_default()
                });

            let domain = url::Url::parse(&url_text)
                .ok()
                .and_then(|url| {
                    url.host_str().map(ToOwned::to_owned)
                })
                .unwrap_or_else(|| "Link".into());

            let domain_label = gtk::Label::new(Some(&domain));
            domain_label.set_xalign(0.0);
            domain_label.add_css_class("url-domain");
            outer.append(&domain_label);

            let url_label = gtk::Label::new(Some(&url_text));
            url_label.set_wrap(true);
            url_label.set_lines(4);
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

            let title = gtk::Label::new(Some(
                object.title.as_deref().unwrap_or("Untitled"),
            ));
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
        .map(|representation| representation.mime_type.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    if !tooltip.is_empty() {
        outer.set_tooltip_text(Some(&tooltip));
    }

    dnd::install_outbound(
        &outer,
        object.clone(),
        panel,
        dnd::OutboundFlavor::Content,
    );

    outer.upcast()
}

fn install_hover_switch(
    button: &gtk::Button,
    page: uuid::Uuid,
    flow: gtk::FlowBox,
    state: Rc<RefCell<ViewState>>,
    panel: panel::PanelController,
) {
    let motion = gtk::DropControllerMotion::new();
    let pending = Rc::new(RefCell::new(None::<glib::SourceId>));

    {
        let pending = pending.clone();
        let state = state.clone();
        let flow = flow.clone();
        let panel = panel.clone();

        motion.connect_enter(move |_, _, _| {
            if let Some(old) = pending.borrow_mut().take() {
                old.remove();
            }

            let state = state.clone();
            let flow = flow.clone();
            let panel = panel.clone();

            *pending.borrow_mut() = Some(
                glib::timeout_add_local_once(
                    Duration::from_millis(hover_ms()),
                    move || {
                        {
                            let mut state = state.borrow_mut();
                            state.active = Some(page);
                            state.signature.clear();
                        }

                        refresh_page(
                            page,
                            &flow,
                            &state,
                            &panel,
                            true,
                        );
                    },
                ),
            );
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

fn ipc(
    paths: &Paths,
    request: Request,
) -> anyhow::Result<Response> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;

    let mut stream = UnixStream::connect(&paths.socket)?;
    let envelope = RequestEnvelope::new(request);

    stream.write_all(serde_json::to_string(&envelope)?.as_bytes())?;
    stream.write_all(b"\n")?;

    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;

    Ok(
        serde_json::from_str::<ResponseEnvelope>(&line)?
            .response,
    )
}

fn text_preview(object: &Object) -> Option<String> {
    object.representations.iter().find_map(|representation| {
        if let StorageRef::InlineText { text } = &representation.storage {
            let mut preview =
                text.chars().take(560).collect::<String>();

            if text.chars().count() > 560 {
                preview.push('…');
            }

            Some(preview)
        } else {
            None
        }
    })
}

fn primary_uri(object: &Object) -> Option<String> {
    object.representations.iter().find_map(|representation| {
        match &representation.storage {
            StorageRef::Uri { uri } => Some(uri.clone()),
            _ => None,
        }
    })
}

fn external_path(object: &Object) -> Option<String> {
    object.representations.iter().find_map(|representation| {
        match &representation.storage {
            StorageRef::ExternalPath { path } => Some(path.clone()),
            _ => None,
        }
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
        .map(|character| character.to_uppercase().collect())
        .unwrap_or_else(|| "•".into())
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

fn dnd_debug() -> bool {
    std::env::var_os("SCRATCHPAD_DND_DEBUG").is_some()
}

fn install_css() {
    let provider = gtk::CssProvider::new();

    provider.load_from_data(
        r#"
        .scratchpad-window,
        .scratchpad-root {
            background: transparent;
        }

        .scratchpad-panel {
            background: #121419;
            padding: 14px;
        }

        .edge-hotspot {
            background: rgba(115, 145, 255, 0.22);
        }

        .resize-handle {
            background: rgba(115, 145, 255, 0.10);
            border-radius: 4px;
        }

        .resize-handle:hover {
            background: rgba(115, 145, 255, 0.55);
        }

        .page-rail {
            padding: 4px 10px 4px 2px;
        }

        .page-dot {
            min-width: 42px;
            min-height: 42px;
            border-radius: 12px;
            background: #252a33;
        }

        .content-column {
            padding-left: 6px;
        }

        .object-card {
            background: #1d2129;
            border: 1px solid #2b313d;
            border-radius: 16px;
            padding: 16px;
            margin: 2px;
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

        .drag-export-chip {
            font-size: 10px;
            font-weight: 700;
            padding: 3px 7px;
            border-radius: 8px;
            background: rgba(115, 145, 255, 0.14);
        }

        .drag-export-chip:hover {
            background: rgba(115, 145, 255, 0.28);
        }

        .text-preview {
            font-size: 16px;
            color: #eef1f6;
        }

        .url-domain {
            font-size: 17px;
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

        .secondary-text,
        .dim-label {
            opacity: 0.58;
        }

        .warning-label {
            opacity: 0.72;
        }
        "#,
    );

    gtk::style_context_add_provider_for_display(
        &gtk::gdk::Display::default().expect("GTK display"),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
