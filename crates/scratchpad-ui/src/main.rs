mod cli;
mod dnd;
mod panel;
mod service;

use anyhow::Result;
use clap::Parser;
use gtk::glib;
use gtk::prelude::*;
use scratchpad_core::*;
use std::{
    cell::RefCell,
    rc::Rc,
    time::Duration,
};

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

#[derive(Clone)]
struct Toast {
    revealer: gtk::Revealer,
    label: gtk::Label,
    undo: gtk::Button,
    pending: Rc<RefCell<Option<Rc<dyn Fn()>>>>,
    timer: Rc<RefCell<Option<glib::SourceId>>>,
}

impl Toast {
    fn new() -> Self {
        let revealer = gtk::Revealer::new();
        revealer.set_transition_type(gtk::RevealerTransitionType::SlideUp);
        revealer.set_transition_duration(180);
        revealer.set_halign(gtk::Align::Center);
        revealer.set_valign(gtk::Align::End);
        revealer.set_margin_bottom(22);
        revealer.set_margin_start(20);
        revealer.set_margin_end(20);
        revealer.add_css_class("undo-toast");

        let box_ = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        box_.add_css_class("undo-toast-body");

        let label = gtk::Label::new(None);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_max_width_chars(34);
        label.set_xalign(0.0);
        box_.append(&label);

        let undo = gtk::Button::with_label("Undo");
        undo.add_css_class("flat");
        undo.add_css_class("undo-button");
        box_.append(&undo);

        revealer.set_child(Some(&box_));

        let pending = Rc::new(RefCell::new(None::<Rc<dyn Fn()>>));
        let timer = Rc::new(RefCell::new(None::<glib::SourceId>));

        {
            let pending = pending.clone();
            let timer = timer.clone();
            let revealer = revealer.clone();

            undo.connect_clicked(move |_| {
                if let Some(timer) = timer.borrow_mut().take() {
                    timer.remove();
                }

                if let Some(action) = pending.borrow_mut().take() {
                    action();
                }

                revealer.set_reveal_child(false);
            });
        }

        Self {
            revealer,
            label,
            undo,
            pending,
            timer,
        }
    }

    fn widget(&self) -> gtk::Revealer {
        self.revealer.clone()
    }

    fn show(
        &self,
        message: impl AsRef<str>,
        undo_action: Option<Rc<dyn Fn()>>,
    ) {
        if let Some(timer) = self.timer.borrow_mut().take() {
            timer.remove();
        }

        self.label.set_text(message.as_ref());
        self.undo.set_visible(undo_action.is_some());
        *self.pending.borrow_mut() = undo_action;
        self.revealer.set_reveal_child(true);

        let revealer = self.revealer.clone();
        let pending = self.pending.clone();
        let timer_slot = self.timer.clone();

        *self.timer.borrow_mut() = Some(glib::timeout_add_local_once(
            Duration::from_secs(6),
            move || {
                revealer.set_reveal_child(false);
                pending.borrow_mut().take();
                timer_slot.borrow_mut().take();
            },
        ));
    }
}

struct Ui {
    tab_list: gtk::Box,
    grid: gtk::FlowBox,
    title: gtk::Label,
    count: gtk::Label,
    state: RefCell<ViewState>,
    panel: panel::PanelController,
    toast: Toast,
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

    let rail = gtk::Box::new(gtk::Orientation::Vertical, 8);
    rail.set_width_request(86);
    rail.set_vexpand(true);
    rail.add_css_class("page-rail");

    let tab_scroll = gtk::ScrolledWindow::new();
    tab_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    tab_scroll.set_vexpand(true);
    tab_scroll.set_propagate_natural_height(true);
    tab_scroll.add_css_class("tab-scroll");

    let tab_list = gtk::Box::new(gtk::Orientation::Vertical, 5);
    tab_list.set_vexpand(false);
    tab_scroll.set_child(Some(&tab_list));
    rail.append(&tab_scroll);

    let add_tab = gtk::Button::from_icon_name("list-add-symbolic");
    add_tab.set_tooltip_text(Some("New task tab"));
    add_tab.add_css_class("flat");
    add_tab.add_css_class("add-tab");
    rail.append(&add_tab);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_hexpand(true);
    content.set_vexpand(true);
    content.add_css_class("content-column");

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    header.add_css_class("workspace-header");

    let title = gtk::Label::new(Some("Scratchpad"));
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.add_css_class("workspace-title");
    header.append(&title);

    let count = gtk::Label::new(Some("0 items"));
    count.add_css_class("item-count");
    header.append(&count);

    content.append(&header);

    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_hexpand(true);
    scroll.set_vexpand(true);
    scroll.add_css_class("grid-scroll");

    let grid = gtk::FlowBox::new();
    grid.set_selection_mode(gtk::SelectionMode::None);
    grid.set_min_children_per_line(2);
    grid.set_max_children_per_line(4);
    grid.set_row_spacing(12);
    grid.set_column_spacing(12);
    grid.set_valign(gtk::Align::Start);
    grid.set_hexpand(true);
    grid.set_homogeneous(true);
    grid.add_css_class("stamp-grid");

    scroll.set_child(Some(&grid));
    content.append(&scroll);

    shell.body.append(&rail);
    shell.body.append(&content);

    let toast = Toast::new();
    shell.root.add_overlay(&toast.widget());

    window.set_child(Some(&shell.root));
    install_css();

    let ui = Rc::new(Ui {
        tab_list,
        grid,
        title,
        count,
        state: RefCell::new(ViewState {
            active: None,
            signature: String::new(),
        }),
        panel: panel_controller.clone(),
        toast,
    });

    {
        let ui = ui.clone();
        add_tab.connect_clicked(move |button| {
            let ui_for_submit = ui.clone();
            prompt_page_name(
                button,
                "New task",
                "",
                Rc::new(move |name| ui_for_submit.create_page(name)),
            );
        });
    }

    ui.reload_tabs();
    install_refresh_timer(ui.clone());

    let submit: dnd::SubmitFn = {
        let ui = ui.clone();
        Rc::new(move |object| ui.submit_object(object))
    };

    dnd::install_inbound(
        &shell.root,
        panel_controller,
        submit,
    );

    window.present();
}

impl Ui {
    fn reload_tabs(self: &Rc<Self>) {
        let pages = match ipc(&Paths::discover(), Request::ListPages) {
            Ok(Response::Pages(pages)) => pages,
            Ok(Response::Error { message }) => {
                self.toast.show(message, None);
                return;
            }
            Ok(_) | Err(_) => {
                self.toast.show("Could not load tabs", None);
                return;
            }
        };

        while let Some(child) = self.tab_list.first_child() {
            self.tab_list.remove(&child);
        }

        let previous = self.state.borrow().active;
        let active = previous
            .filter(|id| pages.iter().any(|page| page.id == *id))
            .or_else(|| pages.first().map(|page| page.id));

        self.state.borrow_mut().active = active;

        for page in pages {
            self.tab_list.append(&self.page_tag(page));
        }

        if active.is_some() {
            self.state.borrow_mut().signature.clear();
            self.refresh_page(true);
        }
    }

    fn page_tag(self: &Rc<Self>, page: Page) -> gtk::Widget {
        let tag = gtk::Box::new(gtk::Orientation::Horizontal, 1);
        tag.add_css_class("page-tag");
        tag.add_css_class(page_color_class(page.id));

        if self.state.borrow().active == Some(page.id) {
            tag.add_css_class("active");
        }

        let label = gtk::Label::new(Some(&page.name));
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_max_width_chars(8);
        label.set_xalign(0.0);

        let button = gtk::Button::new();
        button.set_child(Some(&label));
        button.set_hexpand(true);
        button.set_tooltip_text(Some(&page.name));
        button.add_css_class("flat");
        button.add_css_class("page-tag-main");
        tag.append(&button);

        let close = gtk::Button::from_icon_name("window-close-symbolic");
        close.set_tooltip_text(Some("Delete tab"));
        close.add_css_class("flat");
        close.add_css_class("tab-close");
        tag.append(&close);

        {
            let ui = self.clone();
            let page_id = page.id;
            button.connect_clicked(move |_| ui.select_page(page_id));
        }

        {
            let ui = self.clone();
            let page_for_delete = page.clone();
            close.connect_clicked(move |_| {
                ui.delete_page(page_for_delete.clone());
            });
        }

        {
            let ui = self.clone();
            let page_for_rename = page.clone();
            let gesture = gtk::GestureClick::new();
            gesture.set_button(1);
            gesture.connect_pressed(move |gesture, presses, _, _| {
                if presses != 2 {
                    return;
                }

                gesture.set_state(gtk::EventSequenceState::Claimed);

                let Some(widget) = gesture.widget() else {
                    return;
                };

                let ui_for_submit = ui.clone();
                let page_id = page_for_rename.id;
                prompt_page_name(
                    &widget,
                    "Rename task",
                    &page_for_rename.name,
                    Rc::new(move |name| {
                        ui_for_submit.rename_page(page_id, name)
                    }),
                );
            });
            button.add_controller(gesture);
        }

        install_tab_drag_hover(
            &tag,
            page.id,
            self.clone(),
        );

        tag.upcast()
    }

    fn select_page(self: &Rc<Self>, page_id: uuid::Uuid) {
        if self.state.borrow().active == Some(page_id) {
            return;
        }

        {
            let mut state = self.state.borrow_mut();
            state.active = Some(page_id);
            state.signature.clear();
        }

        self.reload_tabs();
    }

    fn create_page(self: &Rc<Self>, name: String) {
        match ipc(
            &Paths::discover(),
            Request::CreatePage { name },
        ) {
            Ok(Response::Created { id }) => {
                self.state.borrow_mut().active = Some(id);
                self.reload_tabs();
            }
            Ok(Response::Error { message }) => self.toast.show(message, None),
            Ok(_) => self.toast.show("Could not create tab", None),
            Err(err) => self.toast.show(format!("Could not create tab: {err}"), None),
        }
    }

    fn rename_page(self: &Rc<Self>, page_id: uuid::Uuid, name: String) {
        match ipc(
            &Paths::discover(),
            Request::RenamePage { page_id, name },
        ) {
            Ok(Response::Updated) => self.reload_tabs(),
            Ok(Response::Error { message }) => self.toast.show(message, None),
            Ok(_) => self.toast.show("Could not rename tab", None),
            Err(err) => self.toast.show(format!("Could not rename tab: {err}"), None),
        }
    }

    fn delete_page(self: &Rc<Self>, page: Page) {
        let snapshot = match ipc(
            &Paths::discover(),
            Request::GetPage { page_id: page.id },
        ) {
            Ok(Response::Page(snapshot)) => snapshot,
            Ok(Response::Error { message }) => {
                self.toast.show(message, None);
                return;
            }
            Ok(_) | Err(_) => {
                self.toast.show("Could not read tab before deleting it", None);
                return;
            }
        };

        match ipc(
            &Paths::discover(),
            Request::DeletePage { page_id: page.id },
        ) {
            Ok(Response::Removed) => {
                if self.state.borrow().active == Some(page.id) {
                    self.state.borrow_mut().active = None;
                }
                self.reload_tabs();

                let ui = self.clone();
                let snapshot_for_undo = snapshot.clone();
                self.toast.show(
                    format!("Deleted “{}”", page.name),
                    Some(Rc::new(move || {
                        match ipc(
                            &Paths::discover(),
                            Request::RestorePage {
                                snapshot: snapshot_for_undo.clone(),
                            },
                        ) {
                            Ok(Response::Created { .. }) => {
                                ui.state.borrow_mut().active =
                                    Some(snapshot_for_undo.page.id);
                                ui.reload_tabs();
                            }
                            Ok(Response::Error { message }) => {
                                ui.toast.show(message, None);
                            }
                            Ok(_) => {
                                ui.toast.show("Could not restore tab", None);
                            }
                            Err(err) => {
                                ui.toast.show(
                                    format!("Could not restore tab: {err}"),
                                    None,
                                );
                            }
                        }
                    })),
                );
            }
            Ok(Response::Error { message }) => self.toast.show(message, None),
            Ok(_) => self.toast.show("Could not delete tab", None),
            Err(err) => self.toast.show(format!("Could not delete tab: {err}"), None),
        }
    }

    fn refresh_page(self: &Rc<Self>, force: bool) {
        let Some(page_id) = self.state.borrow().active else {
            self.title.set_text("Scratchpad");
            self.count.set_text("0 items");
            self.clear_grid();
            return;
        };

        let snapshot = match ipc(
            &Paths::discover(),
            Request::GetPage { page_id },
        ) {
            Ok(Response::Page(snapshot)) => snapshot,
            _ => return,
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

        if !force && self.state.borrow().signature == signature {
            return;
        }

        self.title.set_text(&snapshot.page.name);
        self.count.set_text(&item_count(snapshot.items.len()));
        self.clear_grid();

        for (object, placement) in snapshot.items {
            self.grid.insert(
                &self.card(object, placement),
                -1,
            );
        }

        self.state.borrow_mut().signature = signature;
    }

    fn clear_grid(&self) {
        while let Some(child) = self.grid.first_child() {
            self.grid.remove(&child);
        }
    }

    fn submit_object(self: &Rc<Self>, object: Object) -> bool {
        let Some(page_id) = self.state.borrow().active else {
            return false;
        };

        match ipc(
            &Paths::discover(),
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

                self.state.borrow_mut().signature.clear();
                self.refresh_page(true);
                true
            }
            Ok(Response::Error { message }) => {
                self.toast.show(message, None);
                false
            }
            Ok(_) => false,
            Err(err) => {
                self.toast.show(
                    format!("Could not save dropped object: {err}"),
                    None,
                );
                false
            }
        }
    }

    fn remove_item(
        self: &Rc<Self>,
        object: Object,
        placement: Placement,
    ) {
        match ipc(
            &Paths::discover(),
            Request::RemovePlacement {
                page_id: placement.page_id,
                object_id: placement.object_id,
            },
        ) {
            Ok(Response::Removed) => {
                self.state.borrow_mut().signature.clear();
                self.refresh_page(true);

                let label = object
                    .title
                    .clone()
                    .unwrap_or_else(|| kind_name(&object.kind).into());
                let ui = self.clone();
                let placement_for_undo = placement.clone();

                self.toast.show(
                    format!("Removed “{}”", label),
                    Some(Rc::new(move || {
                        match ipc(
                            &Paths::discover(),
                            Request::RestorePlacement {
                                placement: placement_for_undo.clone(),
                            },
                        ) {
                            Ok(Response::Updated) => {
                                if ui.state.borrow().active
                                    == Some(placement_for_undo.page_id)
                                {
                                    ui.state.borrow_mut().signature.clear();
                                    ui.refresh_page(true);
                                }
                            }
                            Ok(Response::Error { message }) => {
                                ui.toast.show(message, None);
                            }
                            Ok(_) => {
                                ui.toast.show("Could not restore item", None);
                            }
                            Err(err) => {
                                ui.toast.show(
                                    format!("Could not restore item: {err}"),
                                    None,
                                );
                            }
                        }
                    })),
                );
            }
            Ok(Response::Error { message }) => self.toast.show(message, None),
            Ok(_) => self.toast.show("Could not remove item", None),
            Err(err) => self.toast.show(format!("Could not remove item: {err}"), None),
        }
    }

    fn card(
        self: &Rc<Self>,
        object: Object,
        placement: Placement,
    ) -> gtk::Widget {
        let outer = gtk::Box::new(gtk::Orientation::Vertical, 8);
        outer.set_size_request(196, 152);
        outer.set_hexpand(true);
        outer.set_vexpand(true);
        outer.add_css_class("object-card");
        outer.add_css_class(type_class(&object.kind));

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);

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
            export.set_tooltip_text(Some(
                if matches!(object.kind, ObjectKind::Url) {
                    "Drag as link / URI"
                } else {
                    "Drag as a text file"
                },
            ));

            dnd::install_outbound(
                &export,
                object.clone(),
                self.panel.clone(),
                dnd::OutboundFlavor::Uri,
            );

            header.append(&export);
        }

        let delete = gtk::Button::from_icon_name("window-close-symbolic");
        delete.set_tooltip_text(Some("Remove from this tab"));
        delete.add_css_class("flat");
        delete.add_css_class("card-delete");
        header.append(&delete);

        {
            let ui = self.clone();
            let object_for_delete = object.clone();
            let placement_for_delete = placement.clone();
            delete.connect_clicked(move |_| {
                ui.remove_item(
                    object_for_delete.clone(),
                    placement_for_delete.clone(),
                );
            });
        }

        outer.append(&header);

        match object.kind {
            ObjectKind::Text => {
                let body = gtk::Label::new(Some(
                    text_preview(&object)
                        .as_deref()
                        .unwrap_or(
                            object.title.as_deref().unwrap_or("Empty text"),
                        ),
                ));
                body.set_wrap(true);
                body.set_lines(4);
                body.set_ellipsize(gtk::pango::EllipsizeMode::End);
                body.set_xalign(0.0);
                body.set_yalign(0.0);
                body.add_css_class("text-preview");
                outer.append(&body);
            }
            ObjectKind::Url => {
                let url_text = primary_uri(&object)
                    .or_else(|| text_preview(&object))
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
                domain_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                domain_label.set_xalign(0.0);
                domain_label.add_css_class("url-domain");
                outer.append(&domain_label);

                let url_label = gtk::Label::new(Some(&url_text));
                url_label.set_wrap(true);
                url_label.set_lines(2);
                url_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                url_label.set_xalign(0.0);
                url_label.add_css_class("url-preview");
                outer.append(&url_label);
            }
            _ => {
                let preview = gtk::Box::new(gtk::Orientation::Vertical, 8);
                preview.set_vexpand(true);
                preview.set_valign(gtk::Align::Center);

                let icon = gtk::Label::new(Some(icon_for(&object.kind)));
                icon.add_css_class("stamp-icon");
                preview.append(&icon);

                let title = gtk::Label::new(Some(
                    object.title.as_deref().unwrap_or("Untitled"),
                ));
                title.set_wrap(true);
                title.set_lines(2);
                title.set_ellipsize(gtk::pango::EllipsizeMode::End);
                title.set_xalign(0.5);
                title.add_css_class("object-title");
                preview.append(&title);

                if let Some(path) = external_path(&object) {
                    let secondary = gtk::Label::new(Some(&path));
                    secondary.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
                    secondary.set_max_width_chars(22);
                    secondary.set_xalign(0.5);
                    secondary.add_css_class("secondary-text");
                    preview.append(&secondary);
                }

                outer.append(&preview);
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
            object,
            self.panel.clone(),
            dnd::OutboundFlavor::Content,
        );

        outer.upcast()
    }
}

fn install_refresh_timer(ui: Rc<Ui>) {
    glib::timeout_add_local(Duration::from_millis(refresh_ms()), move || {
        ui.refresh_page(false);
        glib::ControlFlow::Continue
    });
}

fn install_tab_drag_hover<W: IsA<gtk::Widget>>(
    widget: &W,
    page_id: uuid::Uuid,
    ui: Rc<Ui>,
) {
    let motion = gtk::DropControllerMotion::new();
    let pending = Rc::new(RefCell::new(None::<glib::SourceId>));

    {
        let pending = pending.clone();
        motion.connect_enter(move |_, _, _| {
            if let Some(old) = pending.borrow_mut().take() {
                old.remove();
            }

            let ui = ui.clone();
            *pending.borrow_mut() = Some(glib::timeout_add_local_once(
                Duration::from_millis(hover_ms()),
                move || ui.select_page(page_id),
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

    widget.add_controller(motion);
}

fn prompt_page_name<W: IsA<gtk::Widget>>(
    anchor: &W,
    title: &str,
    initial: &str,
    on_submit: Rc<dyn Fn(String)>,
) {
    let popover = gtk::Popover::new();
    popover.set_has_arrow(true);
    popover.set_autohide(true);
    popover.set_position(gtk::PositionType::Right);
    popover.set_parent(anchor);

    let box_ = gtk::Box::new(gtk::Orientation::Vertical, 8);
    box_.set_margin_top(10);
    box_.set_margin_bottom(10);
    box_.set_margin_start(10);
    box_.set_margin_end(10);

    let heading = gtk::Label::new(Some(title));
    heading.set_xalign(0.0);
    heading.add_css_class("popover-title");
    box_.append(&heading);

    let entry = gtk::Entry::new();
    entry.set_text(initial);
    entry.set_placeholder_text(Some("Task name"));
    entry.set_width_chars(20);
    box_.append(&entry);

    let save = gtk::Button::with_label(if initial.is_empty() {
        "Create"
    } else {
        "Rename"
    });
    save.add_css_class("suggested-action");
    box_.append(&save);

    popover.set_child(Some(&box_));

    let commit: Rc<dyn Fn()> = {
        let entry = entry.clone();
        let popover = popover.clone();
        Rc::new(move || {
            let value = entry.text().trim().to_string();
            if value.is_empty() {
                return;
            }

            on_submit(value);
            popover.popdown();
        })
    };

    {
        let commit = commit.clone();
        entry.connect_activate(move |_| commit());
    }

    {
        let commit = commit.clone();
        save.connect_clicked(move |_| commit());
    }

    popover.popup();
    entry.grab_focus();
    entry.select_region(0, -1);
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

fn item_count(count: usize) -> String {
    match count {
        1 => "1 item".into(),
        count => format!("{count} items"),
    }
}

fn page_color_class(id: uuid::Uuid) -> &'static str {
    const COLORS: [&str; 8] = [
        "tab-coral",
        "tab-amber",
        "tab-lime",
        "tab-teal",
        "tab-sky",
        "tab-indigo",
        "tab-violet",
        "tab-rose",
    ];

    COLORS[id.as_bytes()[0] as usize % COLORS.len()]
}

fn type_class(kind: &ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Text => "type-text",
        ObjectKind::Url => "type-url",
        ObjectKind::Image => "type-image",
        ObjectKind::Video => "type-video",
        ObjectKind::Audio => "type-audio",
        ObjectKind::Pdf => "type-pdf",
        ObjectKind::File => "type-file",
        ObjectKind::Directory => "type-folder",
        ObjectKind::App => "type-app",
        ObjectKind::Tool => "type-tool",
        ObjectKind::Unknown => "type-unknown",
    }
}

fn text_preview(object: &Object) -> Option<String> {
    object.representations.iter().find_map(|representation| {
        if let StorageRef::InlineText { text } = &representation.storage {
            let mut preview =
                text.chars().take(280).collect::<String>();

            if text.chars().count() > 280 {
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

fn hover_ms() -> u64 {
    std::env::var("SCRATCHPAD_HOVER_MS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(400)
}

fn refresh_ms() -> u64 {
    std::env::var("SCRATCHPAD_REFRESH_MS")
        .ok()
        .and_then(|value| value.parse().ok())
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
            background: #101218;
            padding: 12px;
        }

        .edge-hotspot {
            background: rgba(115, 145, 255, 0.22);
        }

        .resize-handle {
            background: rgba(115, 145, 255, 0.08);
            border-radius: 4px;
        }

        .resize-handle:hover {
            background: rgba(115, 145, 255, 0.52);
        }

        .page-rail {
            background: #0c0e13;
            border-radius: 14px;
            padding: 8px 6px;
        }

        .tab-scroll,
        .grid-scroll {
            background: transparent;
        }

        .page-tag {
            min-height: 38px;
            border-left-width: 4px;
            border-left-style: solid;
            border-radius: 4px 11px 11px 4px;
            background: #171a22;
            transition: 120ms ease;
        }

        .page-tag:hover {
            background: #20242e;
        }

        .page-tag.active {
            background: #272c38;
            margin-right: 0;
        }

        .page-tag-main {
            min-height: 36px;
            padding: 0 4px 0 7px;
            font-size: 12px;
            font-weight: 650;
        }

        .tab-close {
            min-width: 20px;
            min-height: 20px;
            padding: 0;
            opacity: 0.12;
        }

        .page-tag:hover .tab-close,
        .page-tag.active .tab-close {
            opacity: 0.82;
        }

        .add-tab {
            min-width: 34px;
            min-height: 34px;
            border-radius: 11px;
            opacity: 0.72;
        }

        .add-tab:hover {
            opacity: 1;
            background: #222733;
        }

        .tab-coral { border-left-color: #ff7b72; }
        .tab-amber { border-left-color: #d6a94f; }
        .tab-lime { border-left-color: #8fbd5f; }
        .tab-teal { border-left-color: #4fb6a8; }
        .tab-sky { border-left-color: #62aee8; }
        .tab-indigo { border-left-color: #7d8cf5; }
        .tab-violet { border-left-color: #a67af4; }
        .tab-rose { border-left-color: #e778a7; }

        .content-column {
            padding: 4px 2px 2px 10px;
        }

        .workspace-header {
            min-height: 38px;
            padding: 2px 4px 2px 2px;
        }

        .workspace-title {
            font-size: 20px;
            font-weight: 760;
            color: #edf0f6;
        }

        .item-count {
            font-size: 11px;
            opacity: 0.48;
            padding-right: 4px;
        }

        .stamp-grid {
            padding: 2px 4px 14px 2px;
        }

        .object-card {
            min-width: 196px;
            min-height: 152px;
            background: #1a1e27;
            border: 1px solid #2a303c;
            border-top-width: 3px;
            border-radius: 13px;
            padding: 11px 12px;
            margin: 1px;
            transition: 120ms ease;
        }

        .object-card:hover {
            background: #202530;
            border-color: #3a4352;
        }

        .type-text { border-top-color: #8a96a8; }
        .type-url { border-top-color: #67aaf9; background: #19202b; }
        .type-image { border-top-color: #ad7cf6; background: #201b29; }
        .type-video { border-top-color: #ec945c; background: #241d1a; }
        .type-audio { border-top-color: #61bd82; background: #19241f; }
        .type-pdf { border-top-color: #e86f6f; background: #261b1d; }
        .type-file { border-top-color: #9ba4b4; }
        .type-folder { border-top-color: #d6a94f; background: #252118; }
        .type-app { border-top-color: #4fb6a8; background: #172421; }
        .type-tool { border-top-color: #64c2c8; background: #172326; }
        .type-unknown { border-top-color: #7f8795; }

        .type-badge {
            font-size: 9px;
            font-weight: 760;
            letter-spacing: 0.8px;
            opacity: 0.62;
        }

        .state-badge {
            font-size: 9px;
            font-weight: 700;
            opacity: 0.64;
        }

        .drag-export-chip {
            font-size: 9px;
            font-weight: 760;
            padding: 2px 6px;
            border-radius: 7px;
            background: rgba(115, 145, 255, 0.13);
            opacity: 0.46;
        }

        .object-card:hover .drag-export-chip {
            opacity: 0.9;
        }

        .card-delete {
            min-width: 20px;
            min-height: 20px;
            padding: 0;
            opacity: 0.08;
        }

        .object-card:hover .card-delete {
            opacity: 0.78;
        }

        .text-preview {
            font-size: 14px;
            color: #edf0f5;
        }

        .url-domain {
            font-size: 15px;
            font-weight: 760;
            color: #f1f4f8;
        }

        .url-preview {
            font-size: 11px;
            opacity: 0.62;
        }

        .stamp-icon {
            font-size: 28px;
            opacity: 0.72;
        }

        .object-title {
            font-size: 13px;
            font-weight: 680;
            color: #edf0f5;
        }

        .secondary-text {
            font-size: 10px;
            opacity: 0.46;
        }

        .undo-toast-body {
            background: #292f3a;
            border: 1px solid #414a5b;
            border-radius: 12px;
            padding: 8px 10px 8px 13px;
        }

        .undo-button {
            font-weight: 740;
            color: #9eb8ff;
        }

        .popover-title {
            font-weight: 720;
        }
        "#,
    );

    gtk::style_context_add_provider_for_display(
        &gtk::gdk::Display::default().expect("GTK display"),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
