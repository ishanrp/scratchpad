use gtk::gdk::prelude::*;
use gtk::glib;
use gtk::prelude::*;
use scratchpad_core::Paths;
use std::{cell::RefCell, path::PathBuf, rc::Rc, time::Duration};

#[cfg(feature = "layer-shell")]
use gtk4_layer_shell::{Edge, Layer, LayerShell};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelEdge {
    Left,
    Right,
    Top,
    Bottom,
}

impl PanelEdge {
    pub fn from_env() -> Self {
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

    fn is_vertical(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }
}

struct PanelState {
    revealed: bool,
    pointer_inside: bool,
    drag_active: bool,
    collapse_timer: Option<glib::SourceId>,
    extent: i32,
}

#[derive(Clone)]
pub struct PanelController {
    window: gtk::ApplicationWindow,
    panel: gtk::Box,
    state: Rc<RefCell<PanelState>>,
    edge: PanelEdge,
}

pub struct PanelShell {
    pub root: gtk::Overlay,
    pub body: gtk::Box,
    pub controller: PanelController,
}

pub fn build(window: &gtk::ApplicationWindow) -> PanelShell {
    let edge = PanelEdge::from_env();
    configure_layer_shell(window, edge);

    window.add_css_class("scratchpad-window");
    window.set_decorated(false);
    window.set_resizable(false);

    // Keep one mapped surface for the lifetime of the UI. Hyprland reported a
    // 960x200 surface when we relied on opposite-edge stretching, so size the
    // long axis explicitly from the actual monitor geometry after mapping.
    // These fallback values avoid a tiny natural-size surface before the first
    // monitor geometry callback.
    if edge.is_vertical() {
        window.set_default_size(
            max_panel_extent(),
            fallback_monitor_height(),
        );
    } else {
        window.set_default_size(
            fallback_monitor_width(),
            max_panel_extent(),
        );
    }

    let root = gtk::Overlay::new();
    root.add_css_class("scratchpad-root");
    root.set_hexpand(true);
    root.set_vexpand(true);

    let base = gtk::Box::new(gtk::Orientation::Vertical, 0);
    base.set_hexpand(true);
    base.set_vexpand(true);
    root.set_child(Some(&base));

    let panel = gtk::Box::new(
        if edge.is_vertical() {
            gtk::Orientation::Horizontal
        } else {
            gtk::Orientation::Vertical
        },
        0,
    );
    panel.add_css_class("scratchpad-panel");
    panel.set_vexpand(true);
    panel.set_hexpand(true);

    let body = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    body.set_hexpand(true);
    body.set_vexpand(true);

    let resize_handle = gtk::Box::new(
        if edge.is_vertical() {
            gtk::Orientation::Vertical
        } else {
            gtk::Orientation::Horizontal
        },
        0,
    );
    resize_handle.add_css_class("resize-handle");

    if edge.is_vertical() {
        resize_handle.set_width_request(10);
        resize_handle.set_vexpand(true);
        resize_handle.set_cursor_from_name(Some("col-resize"));
    } else {
        resize_handle.set_height_request(10);
        resize_handle.set_hexpand(true);
        resize_handle.set_cursor_from_name(Some("row-resize"));
    }

    match edge {
        PanelEdge::Right | PanelEdge::Bottom => {
            panel.append(&resize_handle);
            panel.append(&body);
        }
        PanelEdge::Left | PanelEdge::Top => {
            panel.append(&body);
            panel.append(&resize_handle);
        }
    }

    let extent = load_panel_extent(edge);
    apply_panel_extent(&panel, edge, extent);
    align_to_edge(&panel, edge);

    let hotspot = gtk::Box::new(gtk::Orientation::Vertical, 0);
    hotspot.add_css_class("edge-hotspot");
    if edge.is_vertical() {
        hotspot.set_width_request(edge_width());
        hotspot.set_vexpand(true);
    } else {
        hotspot.set_height_request(edge_width());
        hotspot.set_hexpand(true);
    }
    align_to_edge(&hotspot, edge);

    root.add_overlay(&panel);
    root.add_overlay(&hotspot);
    panel.set_visible(false);

    let controller = PanelController {
        window: window.clone(),
        panel: panel.clone(),
        state: Rc::new(RefCell::new(PanelState {
            revealed: false,
            pointer_inside: false,
            drag_active: false,
            collapse_timer: None,
            extent,
        })),
        edge,
    };

    install_pointer_reveal(&root, &controller);
    install_drag_reveal(&root, &controller);
    install_resize(&resize_handle, &controller);

    {
        let controller = controller.clone();
        window.connect_map(move |_| {
            // Wait until the GdkSurface has a monitor, then explicitly match
            // the long axis to that output's application-pixel geometry.
            let controller = controller.clone();
            glib::idle_add_local_once(move || {
                controller.fit_surface_to_monitor();
                controller.apply_input_region();
            });
        });
    }

    PanelShell {
        root,
        body,
        controller,
    }
}

impl PanelController {
    pub fn reveal(&self) {
        if let Some(timer) = self.state.borrow_mut().collapse_timer.take() {
            timer.remove();
        }

        {
            let mut state = self.state.borrow_mut();
            if state.revealed {
                self.apply_input_region_for(true, state.extent);
                return;
            }
            state.revealed = true;
        }

        self.panel.set_visible(true);
        self.apply_input_region();
    }

    pub fn set_drag_active(&self, active: bool) {
        {
            let mut state = self.state.borrow_mut();
            state.drag_active = active;
            if active {
                if let Some(timer) = state.collapse_timer.take() {
                    timer.remove();
                }
            }
        }

        if active {
            self.reveal();
        } else {
            self.maybe_schedule_collapse();
        }
    }

    pub fn maybe_schedule_collapse(&self) {
        let mut state = self.state.borrow_mut();
        if state.pointer_inside || state.drag_active || !state.revealed {
            return;
        }

        if let Some(timer) = state.collapse_timer.take() {
            timer.remove();
        }

        let controller = self.clone();
        state.collapse_timer = Some(glib::timeout_add_local_once(
            Duration::from_millis(collapse_ms()),
            move || controller.collapse_if_idle(),
        ));
    }

    fn collapse_if_idle(&self) {
        {
            let mut state = self.state.borrow_mut();
            state.collapse_timer = None;
            if state.pointer_inside || state.drag_active || !state.revealed {
                return;
            }
            state.revealed = false;
        }

        self.panel.set_visible(false);
        self.apply_input_region();
    }

    fn set_pointer_inside(&self, inside: bool) {
        self.state.borrow_mut().pointer_inside = inside;
        if inside {
            self.reveal();
        } else {
            self.maybe_schedule_collapse();
        }
    }

    fn extent(&self) -> i32 {
        self.state.borrow().extent
    }

    fn set_extent(&self, extent: i32) {
        let extent = extent.clamp(min_panel_extent(), max_panel_extent());
        self.state.borrow_mut().extent = extent;
        apply_panel_extent(&self.panel, self.edge, extent);
        save_panel_extent(self.edge, extent);
        self.apply_input_region();
    }

    fn fit_surface_to_monitor(&self) {
        let Some(surface) = self.window.surface() else {
            return;
        };
        let display = surface.display();
        let Some(monitor) = display.monitor_at_surface(&surface) else {
            return;
        };
        let geometry = monitor.geometry();

        if self.edge.is_vertical() {
            self.window.set_default_size(
                max_panel_extent(),
                geometry.height(),
            );
        } else {
            self.window.set_default_size(
                geometry.width(),
                max_panel_extent(),
            );
        }

        if panel_debug() {
            eprintln!(
                "[scratchpad-panel] monitor={}x{} scale={} requested_surface={}x{}",
                geometry.width(),
                geometry.height(),
                monitor.scale_factor(),
                if self.edge.is_vertical() {
                    max_panel_extent()
                } else {
                    geometry.width()
                },
                if self.edge.is_vertical() {
                    geometry.height()
                } else {
                    max_panel_extent()
                }
            );
        }
    }

    fn apply_input_region(&self) {
        let state = self.state.borrow();
        self.apply_input_region_for(state.revealed, state.extent);
    }

    fn apply_input_region_for(&self, revealed: bool, extent: i32) {
        let Some(surface) = self.window.surface() else {
            return;
        };

        let width = surface.width();
        let height = surface.height();
        if width <= 0 || height <= 0 {
            return;
        }

        let active_extent = if revealed { extent } else { edge_width() };
        let active_extent = if self.edge.is_vertical() {
            active_extent.min(width)
        } else {
            active_extent.min(height)
        };

        let rectangle = match self.edge {
            PanelEdge::Right => gtk::cairo::RectangleInt::new(
                width - active_extent,
                0,
                active_extent,
                height,
            ),
            PanelEdge::Left => {
                gtk::cairo::RectangleInt::new(0, 0, active_extent, height)
            }
            PanelEdge::Top => {
                gtk::cairo::RectangleInt::new(0, 0, width, active_extent)
            }
            PanelEdge::Bottom => gtk::cairo::RectangleInt::new(
                0,
                height - active_extent,
                width,
                active_extent,
            ),
        };

        let region = gtk::cairo::Region::create_rectangle(&rectangle);
        surface.set_input_region(Some(&region));

        if panel_debug() {
            eprintln!(
                "[scratchpad-panel] region revealed={} surface={}x{} extent={} rect=({},{} {}x{})",
                revealed,
                width,
                height,
                active_extent,
                rectangle.x(),
                rectangle.y(),
                rectangle.width(),
                rectangle.height()
            );
        }
    }
}

fn install_pointer_reveal(root: &gtk::Overlay, controller: &PanelController) {
    let motion = gtk::EventControllerMotion::new();

    {
        let controller = controller.clone();
        motion.connect_enter(move |_, _, _| controller.set_pointer_inside(true));
    }

    {
        let controller = controller.clone();
        motion.connect_leave(move |_| controller.set_pointer_inside(false));
    }

    root.add_controller(motion);
}

fn install_drag_reveal(root: &gtk::Overlay, controller: &PanelController) {
    let motion = gtk::DropControllerMotion::new();

    {
        let controller = controller.clone();
        motion.connect_enter(move |_, _, _| controller.set_drag_active(true));
    }

    {
        let controller = controller.clone();
        motion.connect_leave(move |_| controller.set_drag_active(false));
    }

    root.add_controller(motion);
}

fn install_resize(handle: &gtk::Box, controller: &PanelController) {
    let gesture = gtk::GestureDrag::new();
    let start = Rc::new(RefCell::new(0_i32));

    {
        let start = start.clone();
        let controller = controller.clone();
        gesture.connect_drag_begin(move |_, _, _| {
            *start.borrow_mut() = controller.extent();
            controller.reveal();
        });
    }

    {
        let start = start.clone();
        let controller = controller.clone();
        gesture.connect_drag_update(move |_, dx, dy| {
            let delta = match controller.edge {
                PanelEdge::Right => -dx,
                PanelEdge::Left => dx,
                PanelEdge::Bottom => -dy,
                PanelEdge::Top => dy,
            };

            controller.set_extent((*start.borrow() as f64 + delta).round() as i32);
        });
    }

    handle.add_controller(gesture);
}

fn apply_panel_extent(panel: &gtk::Box, edge: PanelEdge, extent: i32) {
    if edge.is_vertical() {
        panel.set_width_request(extent);
        panel.set_height_request(-1);
    } else {
        panel.set_height_request(extent);
        panel.set_width_request(-1);
    }
}

fn align_to_edge<W: IsA<gtk::Widget>>(widget: &W, edge: PanelEdge) {
    match edge {
        PanelEdge::Right => {
            widget.set_halign(gtk::Align::End);
            widget.set_valign(gtk::Align::Fill);
        }
        PanelEdge::Left => {
            widget.set_halign(gtk::Align::Start);
            widget.set_valign(gtk::Align::Fill);
        }
        PanelEdge::Top => {
            widget.set_halign(gtk::Align::Fill);
            widget.set_valign(gtk::Align::Start);
        }
        PanelEdge::Bottom => {
            widget.set_halign(gtk::Align::Fill);
            widget.set_valign(gtk::Align::End);
        }
    }
}

#[cfg(feature = "layer-shell")]
fn configure_layer_shell(window: &gtk::ApplicationWindow, edge: PanelEdge) {
    if std::env::var_os("SCRATCHPAD_NO_LAYER_SHELL").is_some()
        || !gtk4_layer_shell::is_supported()
    {
        return;
    }

    window.init_layer_shell();
    window.set_namespace(Some("system-scratchpad"));
    window.set_layer(Layer::Top);
    window.set_exclusive_zone(0);

    // Anchor to one corner and size the long axis explicitly. The previous
    // opposite-edge stretch path produced a 200px-high layer surface on the
    // tested Hyprland setup despite the documented stretch semantics.
    match edge {
        PanelEdge::Left => {
            window.set_anchor(Edge::Left, true);
            window.set_anchor(Edge::Top, true);
        }
        PanelEdge::Right => {
            window.set_anchor(Edge::Right, true);
            window.set_anchor(Edge::Top, true);
        }
        PanelEdge::Top => {
            window.set_anchor(Edge::Top, true);
            window.set_anchor(Edge::Left, true);
        }
        PanelEdge::Bottom => {
            window.set_anchor(Edge::Bottom, true);
            window.set_anchor(Edge::Left, true);
        }
    }
}

#[cfg(not(feature = "layer-shell"))]
fn configure_layer_shell(_: &gtk::ApplicationWindow, _: PanelEdge) {}

fn panel_extent_path(edge: PanelEdge) -> PathBuf {
    let name = if edge.is_vertical() {
        "panel-width"
    } else {
        "panel-height"
    };
    Paths::discover().data_dir.join(name)
}

fn load_panel_extent(edge: PanelEdge) -> i32 {
    let env_value = if edge.is_vertical() {
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
        .unwrap_or(if edge.is_vertical() { 560 } else { 420 })
        .clamp(min_panel_extent(), max_panel_extent())
}

fn save_panel_extent(edge: PanelEdge, extent: i32) {
    let path = panel_extent_path(edge);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, extent.to_string());
}

fn edge_width() -> i32 {
    std::env::var("SCRATCHPAD_EDGE_WIDTH")
        .ok()
        .and_then(|x| x.parse().ok())
        .unwrap_or(3)
        .clamp(1, 32)
}

fn collapse_ms() -> u64 {
    std::env::var("SCRATCHPAD_COLLAPSE_MS")
        .ok()
        .and_then(|x| x.parse().ok())
        .unwrap_or(350)
}

fn fallback_monitor_width() -> i32 {
    std::env::var("SCRATCHPAD_FALLBACK_MONITOR_WIDTH")
        .ok()
        .and_then(|x| x.parse().ok())
        .unwrap_or(1920)
        .max(640)
}

fn fallback_monitor_height() -> i32 {
    std::env::var("SCRATCHPAD_FALLBACK_MONITOR_HEIGHT")
        .ok()
        .and_then(|x| x.parse().ok())
        .unwrap_or(1080)
        .max(480)
}

fn min_panel_extent() -> i32 {
    280
}

fn max_panel_extent() -> i32 {
    std::env::var("SCRATCHPAD_MAX_PANEL_EXTENT")
        .ok()
        .and_then(|x| x.parse().ok())
        .unwrap_or(960)
        .max(min_panel_extent())
}

fn panel_debug() -> bool {
    std::env::var_os("SCRATCHPAD_PANEL_DEBUG").is_some()
}
