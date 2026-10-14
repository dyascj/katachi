//! The window: a Figma-style tab strip over either the Home screen (recent files) or one open
//! file. Each tab is a whole `App`; session-level state (MCP server, appearance, crash recovery)
//! lives in whichever app is "holding" it and moves when tabs change.

use crate::app::{App, Want};
use egui::{Align2, Color32, CornerRadius, FontId, Key, Modifiers, Pos2, Rect, Sense, Stroke, TextureHandle, pos2, vec2};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

/// Matches where macOS centers the traffic lights (16 points down), so they sit mid-strip.
pub const STRIP_H: f32 = 32.0;

/// Katachi draws the window frame itself (Linux, unless the system title bar is chosen): window
/// buttons at the end of the tab strip, drag to move, edges to resize.
pub static OWN_FRAME: AtomicBool = AtomicBool::new(false);
const BUTTONS_W: f32 = 3.0 * 40.0;

fn own_frame() -> bool {
    OWN_FRAME.load(Ordering::Relaxed)
}

/// Whether this window was opened with the desktop's title bar (Linux setting).
pub fn uses_system_title_bar() -> bool {
    !own_frame()
}
const BLUE: Color32 = Color32::from_rgb(0x0D, 0x99, 0xFF);

pub struct Shell {
    pub tabs: Vec<App>,
    /// The tab on screen; None = Home.
    pub active: Option<usize>,
    /// Holds the session while no tab does. Never shown: if it gains a document (say, crash
    /// recovery or an agent opening a file), it becomes a tab.
    idle: App,
    /// The tab holding the session, if not `idle`.
    held: Option<usize>,
    last_active: Option<usize>,
    home: Home,
    /// The window title last sent (sent only on change; some desktops redraw it every time).
    title: String,
    pub update: crate::update::Updater,
}

impl Shell {
    pub fn new(path: Option<PathBuf>) -> Self {
        let mut s = Shell { tabs: vec![], active: None, idle: App::new(None), held: None, last_active: None, home: Home::default(), title: String::new(), update: crate::update::Updater::start() };
        if let Some(p) = path {
            s.open(p);
        }
        s
    }

    /// Open a file in a tab (or switch to the tab that already has it).
    pub fn open(&mut self, p: PathBuf) {
        if let Some(i) = self.tabs.iter().position(|t| t.path.as_deref() == Some(p.as_path())) {
            return self.activate(Some(i));
        }
        let mut app = App::new(None);
        app.recoverable.clear();
        app.open_path(p);
        // A file that failed to open leaves an empty, unnamed document behind: don't keep it.
        if app.path.is_none() && !app.dirty {
            if let Some((msg, _)) = app.toast.take() {
                self.home.notice = Some(msg);
            }
            return;
        }
        self.tabs.push(app);
        self.activate(Some(self.tabs.len() - 1));
    }

    pub fn new_tab(&mut self) {
        let mut app = App::new(None);
        app.recoverable.clear();
        self.tabs.push(app);
        self.activate(Some(self.tabs.len() - 1));
    }

    fn activate(&mut self, i: Option<usize>) {
        if i.is_none() {
            self.home.refresh = true;
        }
        if self.active.is_some() {
            self.last_active = self.active;
        }
        self.active = i;
    }

    pub fn close(&mut self, i: usize) {
        self.park();
        let app = self.tabs.remove(i);
        app.clear_recovery();
        let fix = |o: Option<usize>| o.and_then(|a| if a == i { None } else if a > i { Some(a - 1) } else { Some(a) });
        self.last_active = fix(self.last_active);
        self.active = match self.active {
            // Closing the tab on screen shows its neighbor, or Home when it was the last one.
            Some(a) if a == i => (!self.tabs.is_empty()).then(|| i.min(self.tabs.len() - 1)),
            a => fix(a),
        };
        if self.active.is_none() {
            self.home.refresh = true;
        }
    }

    /// Close a tab, asking about unsaved changes first.
    fn request_close(&mut self, i: usize) {
        if self.tabs[i].dirty {
            self.activate(Some(i));
            self.tabs[i].confirm = Some(crate::app::After::CloseTab);
        } else {
            self.close(i);
        }
    }

    /// Move the session back to `idle`.
    fn park(&mut self) {
        if let Some(i) = self.held.take() {
            let (idle, tab) = (&mut self.idle, &mut self.tabs[i]);
            idle.take_session(tab);
        }
    }

    /// Hand the session to the tab on screen (or the last one used, from Home).
    fn hold(&mut self) {
        let want = self.active.or(self.last_active).filter(|&i| i < self.tabs.len());
        if want != self.held {
            self.park();
            if let Some(i) = want {
                self.tabs[i].take_session(&mut self.idle);
                self.held = Some(i);
            }
        }
    }

    fn holder(&mut self) -> &mut App {
        match self.held {
            Some(i) => &mut self.tabs[i],
            None => &mut self.idle,
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let (close, next, prev) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::COMMAND, Key::W),
                i.consume_key(Modifiers::CTRL, Key::Tab),
                i.consume_key(Modifiers::CTRL | Modifiers::SHIFT, Key::Tab),
            )
        });
        if close && let Some(i) = self.active {
            self.request_close(i);
        }
        // Ctrl Q quits on Windows and Linux (macOS gets Cmd Q from its app menu). Quitting asks
        // about unsaved work first, as closing the window does.
        if !cfg!(target_os = "macos") && ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Q)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        // Ctrl+Tab cycles Home and the tabs.
        let n = self.tabs.len() as i64 + 1;
        let at = self.active.map_or(0, |a| a as i64 + 1);
        if next || prev {
            let to = (at + if next { 1 } else { -1 }).rem_euclid(n);
            self.activate((to > 0).then(|| to as usize - 1));
        }
        if self.active.is_none() {
            let (new, open, settings) = ctx.input_mut(|i| (i.consume_key(Modifiers::COMMAND, Key::N), i.consume_key(Modifiers::COMMAND, Key::O), i.consume_key(Modifiers::COMMAND, Key::Comma)));
            if settings {
                self.holder().open_settings(crate::settings::Tab::Account);
            }
            if new {
                self.new_tab();
            }
            if open {
                self.open_dialog();
            }
        }
    }

    fn open_dialog(&mut self) {
        if let Some(files) = rfd::FileDialog::new().add_filter("Katachi or Figma", &["katachi", "fig"]).pick_files() {
            for p in files {
                self.open(p);
            }
        }
    }

    /// What the tab on screen asked for this frame.
    fn wants(&mut self) {
        let Some(i) = self.active else { return };
        match self.tabs[i].want.take() {
            Some(Want::NewTab) => self.new_tab(),
            Some(Want::Open(p)) => self.open(p),
            Some(Want::OpenDialog) => self.open_dialog(),
            Some(Want::CloseTab) => self.close(i),
            Some(Want::Home) => self.activate(None),
            Some(Want::KeepOpen) => self.tabs.iter_mut().for_each(|t| t.allow_close = false),
            None => {}
        }
    }

    /// Whether quitting has to wait for a tab's unsaved changes.
    fn close_guard(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.viewport().close_requested()) {
            return;
        }
        if let Some(i) = self.tabs.iter().position(|t| t.dirty && !t.allow_close) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.activate(Some(i));
            self.tabs[i].confirm = Some(crate::app::After::Close);
        }
    }

    // ---------- the tab strip ----------

    fn strip(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let dark = ui.visuals().dark_mode;
        let glass = self.holder().appearance.glass_on();
        let bar = if dark { Color32::from_gray(0x1A) } else { Color32::from_gray(0xE6) };
        let bar = if glass { bar.gamma_multiply(0.55) } else { bar };
        let face = ui.visuals().panel_fill;
        let line = crate::appearance::border(&ctx);
        let ink = crate::appearance::ink(&ctx);
        let muted = crate::appearance::muted(&ctx);
        let mut action: Option<StripAction> = None;
        egui::Panel::top("tabstrip").exact_size(STRIP_H).resizable(false).show_separator_line(false).frame(egui::Frame::NONE.fill(bar)).show(ui, |ui| {
            let r = ui.max_rect();
            // Empty strip: drag to move the window, double-click to zoom (no system title bar).
            if cfg!(target_os = "macos") || own_frame() {
                let resp = ui.interact(r, egui::Id::new("strip-drag"), Sense::click_and_drag());
                if resp.drag_started() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                if resp.double_clicked() {
                    let max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
                }
            }
            let p = ui.painter().clone();
            let mut x = r.min.x + if cfg!(target_os = "macos") { 78.0 } else { 8.0 };
            // Update now.
            if let Some(label) = self.update.button_label() {
                let g = p.layout_no_wrap(label.into(), FontId::proportional(13.0), Color32::WHITE);
                let b = Rect::from_min_size(pos2(x, r.center().y - 11.0), vec2(g.size().x + 20.0, 22.0));
                let resp = ui.interact(b, egui::Id::new("update-now"), Sense::click());
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
                let busy = self.update.busy();
                p.rect_filled(b, 6.0, if busy { BLUE.gamma_multiply(0.6) } else if resp.hovered() { Color32::from_rgb(0x0B, 0x8A, 0xE6) } else { BLUE });
                p.galley(b.center() - g.size() / 2.0, g, Color32::WHITE);
                if resp.clicked() && !busy {
                    action = Some(StripAction::Update);
                }
                let tip = self.update.tooltip();
                resp.on_hover_text(tip);
                x = b.max.x + 10.0;
            }
            // Home.
            p.vline(x, r.y_range(), Stroke::new(1.0, line));
            let home = Rect::from_min_size(pos2(x, r.min.y), vec2(46.0, r.height()));
            let resp = ui.interact(home, egui::Id::new("tab-home"), Sense::click());
            resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, self.active.is_none(), "Home"));
            if self.active.is_none() {
                p.rect_filled(home, 0.0, face);
            } else if resp.hovered() {
                p.rect_filled(home, 0.0, bar.gamma_multiply(0.92));
            }
            house(&p, home.center(), if self.active.is_none() { ink } else { muted });
            if resp.clicked() {
                action = Some(StripAction::Activate(None));
            }
            resp.on_hover_text("Home");
            x = home.max.x;
            p.vline(x, r.y_range(), Stroke::new(1.0, line));
            // File tabs share what's left, between 110 and 220 points each.
            let end = if own_frame() { r.max.x - BUTTONS_W } else { r.max.x };
            let room = (end - x - 44.0).max(0.0);
            let w = if self.tabs.is_empty() { 0.0 } else { (room / self.tabs.len() as f32).clamp(110.0, 220.0) };
            for (i, t) in self.tabs.iter().enumerate() {
                let tr = Rect::from_min_size(pos2(x, r.min.y), vec2(w, r.height()));
                if tr.min.x > r.max.x {
                    break;
                }
                let on = self.active == Some(i);
                let resp = ui.interact(tr, egui::Id::new(("tab", i)), Sense::click());
                resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, on, t.display_name()));
                if on {
                    p.rect_filled(tr, 0.0, face);
                } else if resp.hovered() {
                    p.rect_filled(tr, 0.0, bar.gamma_multiply(0.92));
                }
                let xr = Rect::from_center_size(pos2(tr.max.x - 18.0, tr.center().y), vec2(20.0, 20.0));
                let name = t.display_name();
                let font = FontId::proportional(13.0);
                let room = tr.width() - 46.0;
                let label = elide(&p, &name, font.clone(), room);
                p.text(pos2(tr.min.x + 14.0, tr.center().y), Align2::LEFT_CENTER, label, font, if on { ink } else { muted });
                let x_hover = resp.hovered() && ctx.pointer_hover_pos().is_some_and(|q| xr.contains(q));
                if on || resp.hovered() {
                    if x_hover {
                        p.rect_filled(xr, 4.0, if dark { Color32::from_gray(0x3A) } else { Color32::from_gray(0xD6) });
                    }
                    let c = if x_hover { ink } else { muted };
                    let (a, b) = (xr.center() - vec2(4.0, 4.0), xr.center() + vec2(4.0, 4.0));
                    p.line_segment([a, b], Stroke::new(1.3, c));
                    p.line_segment([pos2(a.x, b.y), pos2(b.x, a.y)], Stroke::new(1.3, c));
                } else if t.dirty {
                    p.circle_filled(xr.center(), 3.5, muted);
                }
                if resp.clicked() {
                    action = Some(if x_hover { StripAction::Close(i) } else { StripAction::Activate(Some(i)) });
                }
                if resp.middle_clicked() {
                    action = Some(StripAction::Close(i));
                }
                if let Some(path) = &t.path {
                    resp.on_hover_text(path.display().to_string());
                }
                x = tr.max.x;
                p.vline(x, r.y_range(), Stroke::new(1.0, line));
            }
            // New tab.
            let plus = Rect::from_min_size(pos2(x + 6.0, r.center().y - 12.0), vec2(24.0, 24.0));
            if plus.max.x < end {
                let resp = ui.interact(plus, egui::Id::new("tab-new"), Sense::click());
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "New design file"));
                if resp.hovered() {
                    p.rect_filled(plus, 6.0, bar.gamma_multiply(0.9));
                }
                let c = plus.center();
                p.line_segment([c - vec2(5.5, 0.0), c + vec2(5.5, 0.0)], Stroke::new(1.3, muted));
                p.line_segment([c - vec2(0.0, 5.5), c + vec2(0.0, 5.5)], Stroke::new(1.3, muted));
                if resp.clicked() {
                    action = Some(StripAction::New);
                }
                resp.on_hover_text("New design file");
            }
            if own_frame() {
                if let Some(a) = window_buttons(ui, Rect::from_min_max(pos2(end, r.min.y), r.max)) {
                    action = Some(a);
                }
            }
            // The line under everything except the tab on screen, which runs into the content.
            let under = r.max.y - 0.5;
            let gap = match self.active {
                None => Some(home.x_range()),
                Some(i) => {
                    let x0 = home.max.x + w * i as f32;
                    Some(egui::Rangef::new(x0, x0 + w))
                }
            };
            match gap {
                Some(g) => {
                    p.hline(egui::Rangef::new(r.min.x, g.min), under, Stroke::new(1.0, line));
                    p.hline(egui::Rangef::new(g.max, r.max.x), under, Stroke::new(1.0, line));
                }
                None => {
                    p.hline(r.x_range(), under, Stroke::new(1.0, line));
                }
            }
        });
        match action {
            Some(StripAction::Window(cmd)) => ctx.send_viewport_cmd(cmd),
            Some(StripAction::Activate(i)) => self.activate(i),
            Some(StripAction::Close(i)) => self.request_close(i),
            Some(StripAction::New) => self.new_tab(),
            Some(StripAction::Update) => self.start_update(),
            None => {}
        }
    }

    fn start_update(&mut self) {
        // Quitting checks unsaved work, so install only once everything is saved.
        if let Some(i) = self.tabs.iter().position(|t| t.dirty) {
            self.activate(Some(i));
            let name = self.tabs[i].display_name();
            self.tabs[i].toast(format!("Save {name} before updating"));
            return;
        }
        self.update.install();
    }
}

enum StripAction {
    Window(egui::ViewportCommand),
    Activate(Option<usize>),
    Close(usize),
    New,
    Update,
}

/// Minimize, maximize and close, drawn at the end of the tab strip.
fn window_buttons(ui: &mut egui::Ui, area: Rect) -> Option<StripAction> {
    let ctx = ui.ctx().clone();
    let max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
    let ink = crate::appearance::ink(&ctx);
    let dark = ui.visuals().dark_mode;
    let p = ui.painter().clone();
    let mut action = None;
    for (k, label) in ["Minimize", if max { "Restore" } else { "Maximize" }, "Close"].into_iter().enumerate() {
        let b = Rect::from_min_size(pos2(area.min.x + k as f32 * 40.0, area.min.y), vec2(40.0, area.height()));
        let resp = ui.interact(b, egui::Id::new(("window-button", k)), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
        let close = k == 2;
        let hot = resp.hovered();
        if hot {
            p.rect_filled(b, 0.0, if close { Color32::from_rgb(0xE8, 0x11, 0x23) } else if dark { Color32::from_gray(0x33) } else { Color32::from_gray(0xD4) });
        }
        let c = b.center();
        let s = Stroke::new(1.0, if hot && close { Color32::WHITE } else { ink });
        match k {
            0 => {
                p.hline(egui::Rangef::new(c.x - 5.0, c.x + 5.0), c.y, s);
            }
            1 if max => {
                p.rect_stroke(Rect::from_center_size(c + vec2(-1.0, 1.0), vec2(8.0, 8.0)), 1.0, s, egui::StrokeKind::Middle);
                p.line_segment([c + vec2(-3.0, -4.0), c + vec2(5.0, -4.0)], s);
                p.line_segment([c + vec2(5.0, -4.0), c + vec2(5.0, 4.0)], s);
            }
            1 => {
                p.rect_stroke(Rect::from_center_size(c, vec2(10.0, 10.0)), 1.0, s, egui::StrokeKind::Middle);
            }
            _ => {
                p.line_segment([c - vec2(5.0, 5.0), c + vec2(5.0, 5.0)], s);
                p.line_segment([c + vec2(-5.0, 5.0), c + vec2(5.0, -5.0)], s);
            }
        }
        if resp.clicked() {
            action = Some(StripAction::Window(match k {
                0 => egui::ViewportCommand::Minimized(true),
                1 => egui::ViewportCommand::Maximized(!max),
                _ => egui::ViewportCommand::Close,
            }));
        }
    }
    action
}

/// With Katachi drawing the frame: a thin border, and pressing near an edge or corner resizes
/// the window (the window manager takes over from there).
fn resize_edges(ctx: &egui::Context) {
    use egui::viewport::ResizeDirection as D;
    let (max, full) = ctx.input(|i| (i.viewport().maximized.unwrap_or(false), i.viewport().fullscreen.unwrap_or(false)));
    if max || full {
        return;
    }
    let r = ctx.content_rect();
    let line = crate::appearance::border(ctx);
    ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("window-border"))).rect_stroke(r, 0.0, Stroke::new(1.0, line), egui::StrokeKind::Inside);
    let (e, c) = (5.0, 14.0);
    let zones = [
        (D::NorthWest, Rect::from_min_size(r.min, vec2(c, c))),
        (D::NorthEast, Rect::from_min_size(pos2(r.max.x - c, r.min.y), vec2(c, c))),
        (D::SouthWest, Rect::from_min_size(pos2(r.min.x, r.max.y - c), vec2(c, c))),
        (D::SouthEast, Rect::from_min_size(r.max - vec2(c, c), vec2(c, c))),
        (D::North, Rect::from_min_max(r.min, pos2(r.max.x, r.min.y + e))),
        (D::South, Rect::from_min_max(pos2(r.min.x, r.max.y - e), r.max)),
        (D::West, Rect::from_min_max(r.min, pos2(r.min.x + e, r.max.y))),
        (D::East, Rect::from_min_max(pos2(r.max.x - e, r.min.y), r.max)),
    ];
    for (k, (dir, zone)) in zones.into_iter().enumerate() {
        let resp = egui::Area::new(egui::Id::new(("resize-edge", k)))
            .order(egui::Order::Foreground)
            .fixed_pos(zone.min)
            .show(ctx, |ui| ui.allocate_exact_size(zone.size(), Sense::drag()).1)
            .inner;
        if resp.hovered() || resp.dragged() {
            ctx.set_cursor_icon(match dir {
                D::North | D::South => egui::CursorIcon::ResizeVertical,
                D::East | D::West => egui::CursorIcon::ResizeHorizontal,
                D::NorthWest | D::SouthEast => egui::CursorIcon::ResizeNwSe,
                D::NorthEast | D::SouthWest => egui::CursorIcon::ResizeNeSw,
            });
        }
        if resp.drag_started() {
            ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(dir));
        }
    }
}

/// A house outline for the Home tab.
fn house(p: &egui::Painter, c: Pos2, color: Color32) {
    let s = Stroke::new(1.4, color);
    let pts = [c + vec2(-6.5, -0.5), c + vec2(0.0, -6.5), c + vec2(6.5, -0.5)];
    p.line_segment([pts[0], pts[1]], s);
    p.line_segment([pts[1], pts[2]], s);
    let body = Rect::from_min_max(c + vec2(-5.0, -2.0), c + vec2(5.0, 6.5));
    p.rect_stroke(body, CornerRadius::same(1), s, egui::StrokeKind::Middle);
    p.line_segment([c + vec2(-1.5, 6.5), c + vec2(-1.5, 2.5)], s);
    p.line_segment([c + vec2(1.5, 6.5), c + vec2(1.5, 2.5)], s);
    p.line_segment([c + vec2(-1.5, 2.5), c + vec2(1.5, 2.5)], s);
}

/// Cut `s` with an ellipsis to fit `width`.
fn elide(p: &egui::Painter, s: &str, font: FontId, width: f32) -> String {
    let fits = |t: &str| p.layout_no_wrap(t.to_string(), font.clone(), Color32::WHITE).size().x <= width;
    if fits(s) {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let (mut lo, mut hi) = (0, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let t: String = chars[..mid].iter().collect::<String>() + "…";
        if fits(&t) { lo = mid } else { hi = mid - 1 }
    }
    chars[..lo].iter().collect::<String>() + "…"
}

// ---------- Home ----------

#[derive(Default)]
struct Home {
    search: String,
    files: Vec<(PathBuf, SystemTime)>,
    thumbs: HashMap<PathBuf, (SystemTime, Option<TextureHandle>)>,
    refresh: bool,
    loaded: bool,
    /// Set by the thumbnail worker when it's written new thumbnails.
    fresh: Arc<AtomicBool>,
    working: Arc<Mutex<bool>>,
    notice: Option<String>,
}

/// What to do after drawing Home.
enum HomeAction {
    Open(PathBuf),
    New,
    OpenDialog,
    Forget(PathBuf),
    Reveal(PathBuf),
    Settings,
}

impl Home {
    fn reload(&mut self, ctx: &egui::Context) {
        self.files = crate::files::recent().into_iter().map(|p| {
            let t = std::fs::metadata(&p).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
            (p, t)
        }).collect();
        self.loaded = true;
        self.refresh = false;
        // Thumbnails missing or older than their file get rendered in the background.
        let stale: Vec<PathBuf> = self
            .files
            .iter()
            .filter(|(p, t)| std::fs::metadata(crate::files::thumb_path(p)).and_then(|m| m.modified()).map_or(true, |tt| tt < *t))
            .map(|(p, _)| p.clone())
            .collect();
        if !stale.is_empty() && !*self.working.lock().unwrap() {
            *self.working.lock().unwrap() = true;
            let (working, fresh, ctx) = (self.working.clone(), self.fresh.clone(), ctx.clone());
            std::thread::spawn(move || {
                for p in stale {
                    if write_thumb(&p).is_ok() {
                        fresh.store(true, Ordering::Relaxed);
                        ctx.request_repaint();
                    }
                }
                *working.lock().unwrap() = false;
            });
        }
    }

    fn thumb(&mut self, ctx: &egui::Context, file: &Path) -> Option<TextureHandle> {
        let tp = crate::files::thumb_path(file);
        let t = std::fs::metadata(&tp).and_then(|m| m.modified()).ok()?;
        if let Some((seen, tex)) = self.thumbs.get(file)
            && *seen == t
        {
            return tex.clone();
        }
        let tex = image::open(&tp).ok().map(|img| {
            let img = img.to_rgba8();
            let ci = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
            ctx.load_texture(format!("thumb:{}", file.display()), ci, egui::TextureOptions::LINEAR)
        });
        self.thumbs.insert(file.to_path_buf(), (t, tex.clone()));
        tex
    }

    fn ui(&mut self, ui: &mut egui::Ui) -> Option<HomeAction> {
        let ctx = ui.ctx().clone();
        if !self.loaded || self.refresh {
            self.reload(&ctx);
        }
        if self.fresh.swap(false, Ordering::Relaxed) {
            // Re-check thumbnail times.
            self.thumbs.clear();
        }
        let ink = crate::appearance::ink(&ctx);
        let muted = crate::appearance::muted(&ctx);
        let line = crate::appearance::border(&ctx);
        let dark = ui.visuals().dark_mode;
        let well = if dark { Color32::from_gray(0x2A) } else { Color32::from_gray(0xF3) };
        let mut action = None;
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(ui.visuals().panel_fill)).show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                let full = ui.available_width();
                let width = (full - 96.0).min(1240.0);
                let left = ((full - width) / 2.0).max(24.0);
                ui.add_space(36.0);
                ui.horizontal(|ui| {
                    ui.add_space(left);
                    ui.vertical(|ui| {
                        ui.set_width(width);
                        // Create.
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Recents").size(22.0).strong().color(ink));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if pill(ui, "Settings", false).clicked() {
                                    action = Some(HomeAction::Settings);
                                }
                                if pill(ui, "Open file…", false).clicked() {
                                    action = Some(HomeAction::OpenDialog);
                                }
                                if pill(ui, "New design file", true).clicked() {
                                    action = Some(HomeAction::New);
                                }
                            });
                        });
                        ui.add_space(18.0);
                        // Search.
                        let (sr, _) = ui.allocate_exact_size(vec2(width, 38.0), Sense::hover());
                        ui.painter().rect_filled(sr, 8.0, well);
                        let glass = sr.left_center() + vec2(18.0, 0.0);
                        ui.painter().circle_stroke(glass + vec2(-1.0, -1.0), 5.0, Stroke::new(1.4, muted));
                        ui.painter().line_segment([glass + vec2(2.6, 2.6), glass + vec2(6.0, 6.0)], Stroke::new(1.4, muted));
                        let field = egui::TextEdit::singleline(&mut self.search).hint_text("Search files").frame(egui::Frame::NONE).font(FontId::proportional(14.0));
                        ui.put(Rect::from_min_max(sr.min + vec2(36.0, 9.0), sr.max - vec2(10.0, 7.0)), field);
                        ui.add_space(22.0);
                        if let Some(n) = &self.notice {
                            ui.colored_label(Color32::from_rgb(0xE0, 0x4E, 0x3A), n);
                            ui.add_space(10.0);
                        }
                        let q = self.search.to_lowercase();
                        let files: Vec<(PathBuf, SystemTime)> = self.files.iter().filter(|(p, _)| q.is_empty() || name(p).to_lowercase().contains(&q)).cloned().collect();
                        if files.is_empty() {
                            ui.add_space(60.0);
                            ui.vertical_centered(|ui| {
                                let msg = if self.files.is_empty() { "Files you open or save show up here." } else { "No files match your search." };
                                ui.label(egui::RichText::new(msg).size(14.0).color(muted));
                            });
                            return;
                        }
                        // Cards: as many columns as fit at 240+ points.
                        let gap = 24.0;
                        let cols = ((width + gap) / (240.0 + gap)).floor().max(1.0) as usize;
                        let cw = (width - gap * (cols as f32 - 1.0)) / cols as f32;
                        let th = (cw * 0.6).round();
                        for row in files.chunks(cols) {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = gap;
                                for (path, when) in row {
                                    let (r, resp) = ui.allocate_exact_size(vec2(cw, th + 64.0), Sense::click());
                                    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name(path)));
                                    let p = ui.painter();
                                    let thumb = Rect::from_min_size(r.min, vec2(cw, th));
                                    p.rect_filled(r, 10.0, ui.visuals().panel_fill);
                                    p.rect_filled(thumb, CornerRadius { nw: 10, ne: 10, sw: 0, se: 0 }, well);
                                    if let Some(tex) = self.thumb(&ctx, path) {
                                        let size = tex.size_vec2();
                                        let fit = ((thumb.width() - 32.0) / size.x).min((thumb.height() - 24.0) / size.y).min(1.0);
                                        let ir = Rect::from_center_size(thumb.center(), size * fit);
                                        ui.painter().image(tex.id(), ir, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
                                    }
                                    let p = ui.painter();
                                    p.hline(thumb.x_range(), thumb.max.y, Stroke::new(1.0, line));
                                    p.rect_stroke(r, 10.0, Stroke::new(if resp.hovered() { 1.5 } else { 1.0 }, if resp.hovered() { BLUE } else { line }), egui::StrokeKind::Inside);
                                    // File mark, name, and when it was edited.
                                    let mark = Rect::from_min_size(pos2(r.min.x + 14.0, thumb.max.y + 15.0), vec2(20.0, 20.0));
                                    p.rect_filled(mark, 5.0, BLUE);
                                    p.text(mark.center(), Align2::CENTER_CENTER, "K", FontId::proportional(12.0), Color32::WHITE);
                                    let title = elide(p, &name(path), FontId::proportional(14.0), cw - 62.0);
                                    p.text(pos2(mark.max.x + 12.0, mark.min.y - 2.0), Align2::LEFT_TOP, title, FontId::proportional(14.0), ink);
                                    p.text(pos2(mark.max.x + 12.0, mark.min.y + 18.0), Align2::LEFT_TOP, format!("Edited {}", ago(*when)), FontId::proportional(12.0), muted);
                                    if resp.clicked() {
                                        action = Some(HomeAction::Open(path.clone()));
                                    }
                                    crate::chrome::context_menu(&resp, |ui| {
                                        if ui.button("Open").clicked() {
                                            action = Some(HomeAction::Open(path.clone()));
                                        }
                                        if ui.button(if cfg!(target_os = "macos") { "Show in Finder" } else { "Show in folder" }).clicked() {
                                            action = Some(HomeAction::Reveal(path.clone()));
                                        }
                                        if ui.button("Remove from recents").clicked() {
                                            action = Some(HomeAction::Forget(path.clone()));
                                        }
                                    });
                                }
                            });
                            ui.add_space(gap);
                        }
                    });
                });
            });
        });
        action
    }
}

fn pill(ui: &mut egui::Ui, text: &str, primary: bool) -> egui::Response {
    let ctx = ui.ctx().clone();
    let g = ui.painter().layout_no_wrap(text.to_string(), FontId::proportional(13.5), Color32::WHITE);
    let (r, resp) = ui.allocate_exact_size(vec2(g.size().x + 30.0, 34.0), Sense::click());
    let (fill, ink) = if primary {
        (if resp.hovered() { Color32::from_rgb(0x0B, 0x8A, 0xE6) } else { BLUE }, Color32::WHITE)
    } else {
        (if resp.hovered() { crate::appearance::subtle(&ctx) } else { ui.visuals().panel_fill }, crate::appearance::ink(&ctx))
    };
    ui.painter().rect_filled(r, 8.0, fill);
    if !primary {
        ui.painter().rect_stroke(r, 8.0, Stroke::new(1.0, crate::appearance::border(&ctx)), egui::StrokeKind::Inside);
    }
    ui.painter().galley_with_override_text_color(r.center() - g.size() / 2.0, g, ink);
    resp
}

fn name(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// "3 days ago", the way Figma's file browser says it.
pub fn ago(t: SystemTime) -> String {
    let s = t.elapsed().map_or(0, |d| d.as_secs());
    let plural = |n: u64, unit: &str| format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" });
    match s {
        0..60 => "just now".into(),
        60..3600 => plural(s / 60, "minute"),
        3600..86400 => plural(s / 3600, "hour"),
        86400..172800 => "yesterday".into(),
        172800..2592000 => plural(s / 86400, "day"),
        2592000..31536000 => plural(s / 2592000, "month"),
        _ => plural(s / 31536000, "year"),
    }
}

/// Render a file's first page to its Home thumbnail.
fn write_thumb(file: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
    let doc = crate::doc::parse_file(&text)?;
    let out = crate::files::thumb_path(file);
    let Some(page) = doc.pages.first() else { return Err("no pages".into()) };
    let ids = page.children.clone();
    let png = if ids.is_empty() {
        // An empty file still gets a (blank) thumbnail so it isn't re-rendered every visit.
        let img = image::RgbaImage::from_pixel(4, 3, image::Rgba([0, 0, 0, 0]));
        let mut bytes = vec![];
        img.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png).map_err(|e| e.to_string())?;
        bytes
    } else {
        let b = crate::export::export_bounds(&doc, &ids);
        let scale = (640.0 / b.width()).min(400.0 / b.height()).min(2.0);
        let svg = crate::export::to_svg(&doc, &ids);
        crate::export::svg_to_png(&doc, &svg, scale)?
    };
    crate::files::write_atomic(&out, &png).map_err(|e| e.to_string())
}

impl eframe::App for Shell {
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        let holder = self.held.map_or(&self.idle, |i| &self.tabs[i]);
        if holder.appearance.glass_on() { [0.0; 4] } else { visuals.panel_fill.to_normalized_gamma_f32() }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.hold();
        // Files opened from Finder or the Dock, or dropped on Home, open as tabs.
        for p in crate::macos_open::take() {
            self.open(p);
        }
        if self.active.is_none() {
            let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
            for p in dropped {
                self.open(p);
            }
        }
        self.shortcuts(&ctx);
        self.hold();
        if own_frame() {
            resize_edges(&ctx);
        }
        let presenting = self.active.is_some_and(|i| self.tabs[i].presenting.is_some());
        if !presenting {
            self.strip(ui);
        }
        self.hold();
        match self.active {
            Some(i) => self.tabs[i].ui(ui, frame),
            None => {
                let action = self.home.ui(ui);
                // Home has no app on screen: the session's own windows and the MCP server run here.
                self.holder().session_frame(&ctx, frame);
                match action {
                    Some(HomeAction::Open(p)) => self.open(p),
                    Some(HomeAction::New) => self.new_tab(),
                    Some(HomeAction::OpenDialog) => self.open_dialog(),
                    Some(HomeAction::Forget(p)) => {
                        crate::files::forget_recent(&p);
                        self.home.refresh = true;
                    }
                    Some(HomeAction::Reveal(p)) => reveal(&p),
                    Some(HomeAction::Settings) => self.holder().open_settings(crate::settings::Tab::Account),
                    None => {}
                }
            }
        }
        // Anything that landed in `idle` (recovered work, a file an agent opened) becomes a tab.
        if self.held.is_none() && (self.idle.path.is_some() || self.idle.dirty) {
            let mut fresh = App::new(None);
            fresh.recoverable.clear();
            let app = std::mem::replace(&mut self.idle, fresh);
            self.tabs.push(app);
            let i = self.tabs.len() - 1;
            self.held = Some(i);
            self.activate(Some(i));
        }
        let title = self.active.and_then(|i| self.tabs.get(i)).map_or_else(|| "Katachi".to_string(), |t| t.window_title());
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
        self.wants();
        self.close_guard(&ctx);
        // Tabs in the background have unsaved work too.
        let now = ctx.input(|i| i.time);
        for t in &mut self.tabs {
            t.autosave(now);
        }
        if self.update.poll(&ctx) {
            ctx.request_repaint();
        }
    }

    /// The window closed: every tab's unsaved work was saved or let go, so drop the autosaves.
    fn on_exit(&mut self) {
        for t in &self.tabs {
            t.clear_recovery();
        }
    }
}

/// Show a file in Finder / the file manager.
fn reveal(p: &Path) {
    if cfg!(target_os = "macos") {
        let _ = std::process::Command::new("open").arg("-R").arg(p).spawn();
    } else {
        // Linux file managers that implement FileManager1 open the folder with the file selected.
        let path: String = p.to_string_lossy().bytes().map(|b| if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect();
        let uri = format!("array:string:file://{path}");
        let shown = std::process::Command::new("dbus-send")
            .args(["--session", "--print-reply", "--dest=org.freedesktop.FileManager1", "/org/freedesktop/FileManager1", "org.freedesktop.FileManager1.ShowItems", &uri, "string:"])
            .output()
            .is_ok_and(|o| o.status.success());
        if !shown && let Some(dir) = p.parent() {
            crate::plugin_view::open_url(&dir.display().to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ago_reads_like_figma() {
        let now = SystemTime::now();
        let back = |s: u64| now - std::time::Duration::from_secs(s);
        assert_eq!(ago(back(5)), "just now");
        assert_eq!(ago(back(60)), "1 minute ago");
        assert_eq!(ago(back(3 * 3600)), "3 hours ago");
        assert_eq!(ago(back(30 * 3600)), "yesterday");
        assert_eq!(ago(back(4 * 86400)), "4 days ago");
        assert_eq!(ago(back(65 * 86400)), "2 months ago");
    }

    #[test]
    fn background_tabs_autosave_and_a_cancelled_quit_asks_again() {
        crate::tests::isolate_data();
        let tag = format!("Background {}", std::process::id());
        let page = tag.clone();
        // Each frame is 31 s, past the autosave interval.
        let mut h = egui_kittest::Harness::builder().with_step_dt(31.0).build_eframe(move |cc| {
            crate::setup_style(&cc.egui_ctx);
            let mut s = Shell::new(None);
            s.update = crate::update::Updater::off();
            s.new_tab();
            s.tabs[0].doc.pages[0].name = page.clone();
            s.tabs[0].dirty = true;
            s.new_tab();
            s
        });
        h.run_steps(3);
        let saved = (0..200).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(10));
            std::fs::read_dir(crate::files::recovery_dir()).into_iter().flatten().flatten().any(|e| std::fs::read_to_string(e.path()).is_ok_and(|s| s.contains(&tag)))
        });
        assert!(saved, "the tab that isn't on screen autosaved");
        // Quitting: tab 0 said "Don't save", then tab 1 cancelled. Both must ask again next time.
        let s = h.state_mut();
        s.tabs[0].allow_close = true;
        s.tabs[1].want = Some(Want::KeepOpen);
        s.wants();
        assert!(s.tabs.iter().all(|t| !t.allow_close));
        s.tabs[0].clear_recovery();
    }

    #[test]
    fn tabs_open_switch_and_close_and_the_session_follows() {
        crate::tests::isolate_data();
        let dir = std::env::temp_dir().join(format!("katachi-tabs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("A.katachi"), dir.join("B.katachi"));
        for p in [&a, &b] {
            let mut app = App::new(None);
            app.doc.pages[0].name = name(p);
            assert!(app.save_to(p.clone()));
        }
        let mut s = Shell::new(None);
        s.update = crate::update::Updater::off();
        assert!(s.active.is_none(), "starts on Home");
        s.open(a.clone());
        s.open(b.clone());
        s.open(a.clone());
        assert_eq!(s.tabs.len(), 2, "opening an open file switches to its tab");
        assert_eq!(s.active, Some(0));
        s.hold();
        assert_eq!(s.held, Some(0));
        s.activate(Some(1));
        s.hold();
        assert_eq!(s.held, Some(1), "the session moves with the tab on screen");
        s.tabs[1].dirty = true;
        s.request_close(1);
        assert_eq!(s.tabs.len(), 2, "unsaved work asks first");
        assert_eq!(s.tabs[1].confirm, Some(crate::app::After::CloseTab));
        s.close(1);
        assert_eq!((s.tabs.len(), s.active), (1, Some(0)));
        s.close(0);
        assert_eq!(s.active, None, "closing the last tab shows Home");
        s.hold();
        assert_eq!(s.held, None);
    }
}

#[cfg(test)]
mod look {
    use super::*;
    use egui_kittest::Harness;

    /// `cargo test --release shell_look -- --ignored` renders Home and the tab strip to target/.
    #[test]
    #[ignore]
    fn shell_look() {
        crate::tests::isolate_data();
        let dir = std::env::temp_dir().join(format!("katachi-look-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (i, name) in ["Lumen", "Weather app", "Smart home HUD", "Onboarding flow with a long file name"].iter().enumerate() {
            let mut app = App::new(None);
            if i % 2 == 0 {
                crate::plugins::run(&mut app.doc, 0, &[], include_str!("../assets/promo/showcase.js")).unwrap();
            }
            let p = dir.join(format!("{name}.katachi"));
            assert!(app.save_to(p.clone()));
            write_thumb(&p).unwrap();
        }
        let mut h = Harness::builder().with_size(egui::vec2(1440.0, 900.0)).with_pixels_per_point(2.0).wgpu().build_eframe(|cc| {
            crate::setup_style(&cc.egui_ctx);
            let mut s = Shell::new(None);
            s.update = crate::update::Updater::off();
            *s.update.state.lock().unwrap() = crate::update::State::Available { version: "0.2.0".into(), url: String::new(), sig: String::new() };
            s
        });
        h.run_steps(6);
        h.render().unwrap().save("target/look-home.png").unwrap();
        h.state_mut().open(dir.join("Lumen.katachi"));
        h.state_mut().open(dir.join("Weather app.katachi"));
        h.state_mut().activate(Some(0));
        h.run_steps(6);
        h.render().unwrap().save("target/look-tabs.png").unwrap();
    }
}
