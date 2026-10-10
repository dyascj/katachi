//! Side panels (pages, layers, design properties), floating toolbar, and painted icons.

use crate::app::{Align, App, Tool};
use crate::canvas::{BLUE, num};
use crate::doc::{AutoLayout, AxisAlign, Constraint, Kind, LayoutMode, NodeId, Page, Paint, Rgba, SHAPE_GRAY, Shadow, Sizing, StrokeAlign};
use crate::paint_ui::{PaintEvent, hex_field, paint_row, shadow_row};
use egui::{
    Color32, CornerRadius, DragValue, FontId, Painter, Pos2, Rect, Response, RichText, Sense, Shape, Stroke, StrokeKind,
    Ui, pos2, vec2,
};

#[derive(Clone, Copy, PartialEq)]
pub enum Icon {
    Move,
    Frame,
    Rect,
    Ellipse,
    Line,
    Pen,
    Text,
    Hand,
    Group,
    Align(Align),
    Eye(bool),
    Lock(bool),
    Plus,
    Minus,
    Chevron(bool),
    Menu,
    ArrowDown,
    ArrowRight,
    Opacity,
    Radius,
    PadH,
    PadV,
    GapH,
    GapV,
    Rotate,
    Component,
    Instance,
}

const SUBTLE: Color32 = Color32::from_rgb(0xF5, 0xF5, 0xF5);
const SEL_BG: Color32 = Color32::from_rgb(0xE5, 0xF4, 0xFF);
const CHILD_BG: Color32 = Color32::from_rgb(0xF2, 0xF9, 0xFF);

/// Drag payload: a main component dragged from the Assets tab.
pub struct AssetDrag(pub NodeId);

pub fn paint_icon(p: &Painter, r: Rect, icon: Icon, color: Color32) {
    let s = Stroke::new(1.2, color);
    let c = r.center();
    let at = |x: f32, y: f32| c + vec2(x, y);
    match icon {
        Icon::Move => {
            let pts = [at(-4., -6.), at(-4., 5.), at(-1., 2.5), at(1.5, 7.), at(3., 6.2), at(0.6, 1.8), at(4.5, 1.8)];
            p.add(Shape::closed_line(pts.to_vec(), s));
        }
        Icon::Frame => {
            for d in [-2.5, 2.5] {
                p.line_segment([at(d, -6.), at(d, 6.)], s);
                p.line_segment([at(-6., d), at(6., d)], s);
            }
        }
        Icon::Rect => {
            p.rect_stroke(Rect::from_center_size(c, vec2(11., 11.)), 1.0, s, StrokeKind::Middle);
        }
        Icon::Ellipse => {
            p.circle_stroke(c, 5.5, s);
        }
        Icon::Line => {
            p.line_segment([at(-5., 5.), at(5., -5.)], s);
        }
        Icon::Pen => {
            let pts = [at(0., -6.), at(4.5, 1.), at(2., 6.), at(-2., 6.), at(-4.5, 1.)];
            p.add(Shape::closed_line(pts.to_vec(), s));
            p.line_segment([at(0., -6.), at(0., 0.5)], s);
            p.circle_filled(at(0., 1.5), 1.2, color);
        }
        Icon::Text => {
            p.line_segment([at(-5., -5.), at(5., -5.)], s);
            p.line_segment([at(0., -5.), at(0., 6.)], s);
        }
        Icon::Hand => {
            for (i, h) in [4.0, 6.0, 6.0, 5.0].iter().enumerate() {
                let x = -3.0 + i as f32 * 2.2;
                p.line_segment([at(x, 2.), at(x, 2. - h)], Stroke::new(1.6, color));
            }
            p.rect_stroke(Rect::from_min_max(at(-4., 1.), at(4.5, 6.5)), 2.0, s, StrokeKind::Middle);
            p.line_segment([at(-4., 3.), at(-6., 0.)], Stroke::new(1.6, color));
        }
        Icon::Group => {
            let b = Rect::from_center_size(c, vec2(10., 10.));
            for (a, d) in [(b.left_top(), vec2(3., 3.)), (b.right_top(), vec2(-3., 3.)), (b.left_bottom(), vec2(3., -3.)), (b.right_bottom(), vec2(-3., -3.))] {
                p.line_segment([a, a + vec2(d.x, 0.)], s);
                p.line_segment([a, a + vec2(0., d.y)], s);
            }
        }
        Icon::Align(a) => {
            let f = |r: Rect| p.rect_filled(r, 0.5, color);
            let bar = |x0: f32, y: f32, w: f32| Rect::from_min_size(at(x0, y), vec2(w, 3.));
            let col = |x: f32, y0: f32, h: f32| Rect::from_min_size(at(x, y0), vec2(3., h));
            match a {
                Align::Left => {
                    p.line_segment([at(-6., -6.), at(-6., 6.)], s);
                    f(bar(-4., -4., 9.));
                    f(bar(-4., 1., 5.));
                }
                Align::HCenter => {
                    p.line_segment([at(0., -6.), at(0., 6.)], s);
                    f(bar(-5., -4., 10.));
                    f(bar(-3., 1., 6.));
                }
                Align::Right => {
                    p.line_segment([at(6., -6.), at(6., 6.)], s);
                    f(bar(-5., -4., 9.));
                    f(bar(-1., 1., 5.));
                }
                Align::Top => {
                    p.line_segment([at(-6., -6.), at(6., -6.)], s);
                    f(col(-4., -4., 9.));
                    f(col(1., -4., 5.));
                }
                Align::VCenter => {
                    p.line_segment([at(-6., 0.), at(6., 0.)], s);
                    f(col(-4., -5., 10.));
                    f(col(1., -3., 6.));
                }
                Align::Bottom => {
                    p.line_segment([at(-6., 6.), at(6., 6.)], s);
                    f(col(-4., -5., 9.));
                    f(col(1., -1., 5.));
                }
            }
        }
        Icon::Eye(open) => {
            let pts: Vec<Pos2> = (0..=24)
                .map(|i| {
                    let t = i as f32 / 24.0 * std::f32::consts::TAU;
                    at(6.0 * t.cos(), 3.5 * t.sin())
                })
                .collect();
            p.add(Shape::line(pts, s));
            p.circle_filled(c, 1.8, color);
            if !open {
                p.line_segment([at(-6., 5.), at(6., -5.)], s);
            }
        }
        Icon::Lock(locked) => {
            p.rect_filled(Rect::from_min_max(at(-4., -0.5), at(4., 5.5)), 1.0, color);
            let shift = if locked { 0.0 } else { 3.0 };
            let pts: Vec<Pos2> = (0..=12)
                .map(|i| {
                    let t = std::f32::consts::PI * (1.0 + i as f32 / 12.0);
                    at(shift + 2.6 * t.cos(), -2.5 + 3.0 * t.sin())
                })
                .collect();
            p.add(Shape::line(pts, s));
            p.line_segment([at(shift - 2.6, -2.5), at(shift - 2.6, -0.5)], s);
            p.line_segment([at(shift + 2.6, -2.5), at(shift + 2.6, -0.5)], s);
        }
        Icon::Plus => {
            p.line_segment([at(-5., 0.), at(5., 0.)], s);
            p.line_segment([at(0., -5.), at(0., 5.)], s);
        }
        Icon::ArrowDown | Icon::ArrowRight => {
            let (a, b, l, r) = if icon == Icon::ArrowDown {
                (at(0., -6.), at(0., 6.), at(-4., 2.), at(4., 2.))
            } else {
                (at(-6., 0.), at(6., 0.), at(2., -4.), at(2., 4.))
            };
            p.line_segment([a, b], s);
            p.line_segment([l, b], s);
            p.line_segment([r, b], s);
        }
        Icon::Opacity => {
            p.circle_stroke(c, 5.0, s);
            let half: Vec<Pos2> = (0..=12).map(|i| {
                let t = std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / 12.0;
                at(5.0 * t.cos(), 5.0 * t.sin())
            }).collect();
            p.add(Shape::convex_polygon(half, color, Stroke::NONE));
        }
        Icon::Radius => {
            let pts: Vec<Pos2> = (0..=8).map(|i| {
                let t = std::f32::consts::PI + std::f32::consts::FRAC_PI_2 * i as f32 / 8.0;
                at(1.0 + 5.0 * t.cos(), 1.0 + 5.0 * t.sin())
            }).collect();
            p.line_segment([at(-4., 6.), at(-4., 1.)], s);
            p.line_segment([at(1., -4.), at(6., -4.)], s);
            p.add(Shape::line(pts, s));
        }
        Icon::PadH | Icon::PadV => {
            let h = icon == Icon::PadH;
            let f = |x: f32, y: f32| if h { at(x, y) } else { at(y, x) };
            p.line_segment([f(-6., -5.), f(-6., 5.)], s);
            p.line_segment([f(6., -5.), f(6., 5.)], s);
            p.rect_stroke(Rect::from_two_pos(f(-3., -3.), f(3., 3.)), 1.0, s, StrokeKind::Middle);
        }
        Icon::GapH | Icon::GapV => {
            let h = icon == Icon::GapH;
            let f = |x: f32, y: f32| if h { at(x, y) } else { at(y, x) };
            p.line_segment([f(-5., -5.), f(-5., 5.)], s);
            p.line_segment([f(5., -5.), f(5., 5.)], s);
            p.line_segment([f(-2., 0.), f(2., 0.)], s);
        }
        Icon::Rotate => {
            let pts: Vec<Pos2> = (0..=16).map(|i| {
                let t = -0.6 + 5.0 * i as f32 / 16.0;
                at(5.0 * t.cos(), 5.0 * t.sin())
            }).collect();
            let end = *pts.last().unwrap();
            p.add(Shape::line(pts, s));
            p.line_segment([end, end + vec2(3.0, 0.5)], s);
            p.line_segment([end, end + vec2(0.5, 3.0)], s);
        }
        Icon::Component => {
            // Four small diamonds.
            for (dx, dy) in [(0., -3.5), (3.5, 0.), (0., 3.5), (-3.5, 0.)] {
                let m = at(dx, dy);
                let d = 2.3;
                p.add(Shape::convex_polygon(vec![m + vec2(0., -d), m + vec2(d, 0.), m + vec2(0., d), m + vec2(-d, 0.)], color, Stroke::NONE));
            }
        }
        Icon::Instance => {
            let d = 5.5;
            p.add(Shape::closed_line(vec![at(0., -d), at(d, 0.), at(0., d), at(-d, 0.)], s));
        }
        Icon::Menu => {
            for y in [-4., 0., 4.] {
                p.line_segment([at(-6., y), at(6., y)], s);
            }
        }
        Icon::Minus => {
            p.line_segment([at(-5., 0.), at(5., 0.)], s);
        }
        Icon::Chevron(open) => {
            let pts = if open { vec![at(-3., -1.5), at(0., 1.5), at(3., -1.5)] } else { vec![at(-1.5, -3.), at(1.5, 0.), at(-1.5, 3.)] };
            p.add(Shape::line(pts, Stroke::new(1.0, color)));
        }
    }
}

pub fn icon_button(ui: &mut Ui, icon: Icon, selected: bool, size: f32, tip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    let (bg, fg) = if selected {
        (BLUE, Color32::WHITE)
    } else if resp.hovered() {
        (SUBTLE, Color32::from_gray(0x1E))
    } else {
        (Color32::TRANSPARENT, Color32::from_gray(0x1E))
    };
    ui.painter().rect_filled(rect, 5.0, bg);
    paint_icon(ui.painter(), rect, icon, fg);
    if tip.is_empty() { resp } else { resp.on_hover_text(tip) }
}

fn kind_icon(k: Kind) -> Icon {
    match k {
        Kind::Frame => Icon::Frame,
        Kind::Component => Icon::Component,
        Kind::Instance => Icon::Instance,
        Kind::Group => Icon::Group,
        Kind::Rectangle => Icon::Rect,
        Kind::Ellipse => Icon::Ellipse,
        Kind::Vector => Icon::Pen,
        Kind::Text => Icon::Text,
    }
}

/// Half the panel's content width, independent of what a row has already used.
fn half(ui: &Ui) -> f32 {
    (ui.max_rect().width() - 8.0) / 2.0
}

/// Figma-style number box with a label (or `@icon` name) inside on the left.
fn num_field(ui: &mut Ui, v: &mut f32, label: &str, min: f32, width: f32) -> bool {
    let icon = match label {
        "@opacity" => Some(Icon::Opacity),
        "@radius" => Some(Icon::Radius),
        "@padh" => Some(Icon::PadH),
        "@padv" => Some(Icon::PadV),
        "@gaph" => Some(Icon::GapH),
        "@gapv" => Some(Icon::GapV),
        "@rotate" => Some(Icon::Rotate),
        _ => None,
    };
    let prefix = match (icon, label.is_empty()) {
        (Some(_), _) => "     ".to_string(),
        (None, true) => String::new(),
        (None, false) => format!("{label}  "),
    };
    let unit = match label {
        "@opacity" => "%",
        "@rotate" => "°",
        _ => "",
    };
    let mut tmp = if v.is_nan() { 0.0 } else { *v };
    let mixed = v.is_nan();
    let resp = ui.add_sized(
        [width.max(30.0), 22.0],
        DragValue::new(&mut tmp).prefix(prefix).speed(1.0).range(min..=1e6).custom_formatter(move |x, _| {
            if mixed {
                "Mixed".into()
            } else {
                format!("{}{}", num(x as f32), unit)
            }
        }),
    );
    if let Some(i) = icon {
        let r = Rect::from_center_size(pos2(resp.rect.min.x + 13.0, resp.rect.center().y), vec2(14.0, 14.0));
        paint_icon(ui.painter(), r, i, Color32::from_gray(0x80));
    }
    if resp.changed() {
        *v = tmp;
    }
    resp.changed()
}

/// Compact chevron button opening a Fixed / Hug / Fill menu.
fn sizing_menu(ui: &mut Ui, id: egui::Id, mode: &mut Sizing, can_hug: bool, can_fill: bool) -> bool {
    let resp = icon_button(ui, Icon::Chevron(true), false, 18.0, "Resizing");
    let mut changed = false;
    egui::Popup::menu(&resp).id(id).show(|ui| {
        for (m, ok, label) in [(Sizing::Fixed, true, "Fixed"), (Sizing::Hug, can_hug, "Hug contents"), (Sizing::Fill, can_fill, "Fill container")] {
            if ok && ui.selectable_label(*mode == m, label).clicked() && *mode != m {
                *mode = m;
                changed = true;
            }
        }
    });
    changed
}

/// 3x3 alignment picker for auto layout (rows = vertical, columns = horizontal position).
fn align_grid(ui: &mut Ui, l: &mut AutoLayout) {
    let (rect, resp) = ui.allocate_exact_size(vec2(64.0, 64.0), Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, 4.0, SUBTLE);
    let h = l.mode == LayoutMode::Horizontal;
    let idx = |a: AxisAlign| match a {
        AxisAlign::Min => 0,
        AxisAlign::Center => 1,
        AxisAlign::Max => 2,
    };
    let (col, row) = if h { (idx(l.main), idx(l.cross)) } else { (idx(l.cross), idx(l.main)) };
    let cell = |c: usize, r: usize| pos2(rect.min.x + 12.0 + c as f32 * 20.0, rect.min.y + 12.0 + r as f32 * 20.0);
    for r in 0..3 {
        for c in 0..3 {
            let at = cell(c, r);
            if (c, r) == (col, row) {
                // Bars show the children's arrangement.
                let (bw, bh) = if h { (3.0, 10.0) } else { (10.0, 3.0) };
                for k in [-1.0, 0.0, 1.0] {
                    let off = if h { vec2(k * 4.5, 0.0) } else { vec2(0.0, k * 4.5) };
                    let len = if k == 0.0 { 1.0 } else { 0.7 };
                    p.rect_filled(Rect::from_center_size(at + off, vec2(bw * if h { 1.0 } else { len }, bh * if h { len } else { 1.0 })), 1.0, BLUE);
                }
            } else {
                p.circle_filled(at, 1.5, Color32::from_gray(0xB3));
            }
        }
    }
    if let Some(pos) = resp.interact_pointer_pos().filter(|_| resp.clicked()) {
        let c = (((pos.x - rect.min.x) / 21.4) as usize).min(2);
        let r = (((pos.y - rect.min.y) / 21.4) as usize).min(2);
        let to = |i: usize| [AxisAlign::Min, AxisAlign::Center, AxisAlign::Max][i];
        if h {
            (l.main, l.cross) = (to(c), to(r));
        } else {
            (l.main, l.cross) = (to(r), to(c));
        }
    }
}

fn section_title(ui: &mut Ui, title: &str) -> bool {
    let mut add = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            add = icon_button(ui, Icon::Plus, false, 22.0, "").clicked();
        });
    });
    add
}

fn heading(ui: &mut Ui, title: &str) {
    ui.add_space(4.0);
    ui.label(RichText::new(title).strong());
    ui.add_space(2.0);
}

impl App {
    // ---------- left: menu, pages, layers ----------

    pub fn left_panel(&mut self, ui: &mut Ui) {
        if self.sel != self.prev_sel {
            self.prev_sel = self.sel.clone();
            for id in self.sel.clone() {
                for a in self.doc.ancestors(id) {
                    self.doc.node_mut(a).expanded = true;
                }
            }
        }
        egui::Panel::left("left").resizable(true).default_size(240.0).min_size(180.0).show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let menu = icon_button(ui, Icon::Menu, false, 28.0, "Main menu");
                egui::Popup::menu(&menu).show(|ui| self.main_menu(ui));
                let name = self.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned());
                ui.label(RichText::new(name.unwrap_or_else(|| "Untitled".into())).strong());
            });
            ui.horizontal(|ui| {
                for (assets, label) in [(false, "File"), (true, "Assets")] {
                    let text = RichText::new(label);
                    let text = if self.left_tab_assets == assets { text.strong() } else { text.color(Color32::from_gray(0x80)) };
                    if ui.add(egui::Button::new(text).frame(false)).clicked() {
                        self.left_tab_assets = assets;
                    }
                }
            });
            ui.separator();
            if self.left_tab_assets {
                self.assets(ui);
                return;
            }
            self.pages_list(ui);
            ui.separator();
            ui.label(RichText::new("Layers").strong());
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| self.layers(ui));
        });
    }

    /// Assets tab: every main component in the file. Drag one onto the canvas to place an instance.
    fn assets(&mut self, ui: &mut Ui) {
        let search_id = ui.id().with("asset-search");
        let mut q: String = ui.data(|d| d.get_temp(search_id)).unwrap_or_default();
        ui.add(egui::TextEdit::singleline(&mut q).hint_text("Search assets").desired_width(f32::INFINITY));
        ui.data_mut(|d| d.insert_temp(search_id, q.clone()));
        ui.add_space(4.0);
        ui.label(RichText::new("Local components").strong());
        let mut comps: Vec<(usize, NodeId, String)> = self
            .doc
            .nodes
            .values()
            .filter(|n| n.kind == Kind::Component && n.name.to_lowercase().contains(&q.to_lowercase()))
            .map(|n| (self.doc.page_of(n.id), n.id, n.name.clone()))
            .collect();
        comps.sort_by(|a, b| (a.0, &a.2).cmp(&(b.0, &b.2)));
        if comps.is_empty() {
            ui.label(RichText::new("Create components with ⌥⌘K to reuse them here.").color(Color32::from_gray(0x80)));
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (page, id, name) in comps {
                let r = ui.dnd_drag_source(ui.id().with(("asset", id)), AssetDrag(id), |ui| {
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                        paint_icon(ui.painter(), rect, Icon::Component, crate::canvas::PURPLE);
                        ui.label(&name);
                        ui.label(RichText::new(&self.doc.pages[page].name).color(Color32::from_gray(0x99)).small());
                    });
                });
                if r.response.double_clicked() {
                    let at = self.view_center_world();
                    self.checkpoint();
                    let size = self.doc.node(id).rect.size();
                    let i = self.doc.instantiate(id, self.page, None, None, (at - size / 2.0).round());
                    self.sel = vec![i];
                }
            }
        });
    }

    fn main_menu(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let item = |ui: &mut Ui, label: &str, short: &str| ui.add(egui::Button::new(label).shortcut_text(short)).clicked();
        if item(ui, "New file", "⌘N") {
            self.request_new();
            ui.close();
        }
        if item(ui, "Open…", "⌘O") {
            self.request_open();
            ui.close();
        }
        if item(ui, "Save", "⌘S") {
            self.save(false);
            ui.close();
        }
        if item(ui, "Save as…", "⇧⌘S") {
            self.save(true);
            ui.close();
        }
        if item(ui, "Export…", "⇧⌘E") {
            self.export();
            ui.close();
        }
        ui.separator();
        if item(ui, "Undo", "⌘Z") {
            self.undo();
            ui.close();
        }
        if item(ui, "Redo", "⇧⌘Z") {
            self.redo();
            ui.close();
        }
        ui.separator();
        if item(ui, "Zoom to fit", "⇧1") {
            self.zoom_to(&ctx, None, false);
            ui.close();
        }
        if item(ui, "Zoom to 100%", "⇧0") {
            self.zoom_to(&ctx, None, true);
            ui.close();
        }
    }

    fn pages_list(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Pages").strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_button(ui, Icon::Plus, false, 22.0, "Add page").clicked() {
                    self.checkpoint();
                    let n = self.doc.pages.len() + 1;
                    self.doc.pages.push(Page { name: format!("Page {n}"), children: vec![], background: crate::doc::PAGE_BG });
                    self.select_page(n - 1);
                }
            });
        });
        let mut delete = None;
        for i in 0..self.doc.pages.len() {
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
            if i == self.page {
                ui.painter().rect_filled(rect, 4.0, SUBTLE);
            } else if resp.hovered() {
                ui.painter().rect_filled(rect, 4.0, SUBTLE.gamma_multiply(0.6));
            }
            if let Some((pi, buf)) = &mut self.renaming_page
                && *pi == i {
                    let r = ui.put(rect.shrink2(vec2(6.0, 3.0)), egui::TextEdit::singleline(buf));
                    r.request_focus();
                    if r.lost_focus() {
                        let name = buf.trim().to_string();
                        self.renaming_page = None;
                        if !name.is_empty() && name != self.doc.pages[i].name {
                            self.checkpoint();
                            self.doc.pages[i].name = name;
                        }
                    }
                    continue;
                }
            ui.painter().text(
                rect.left_center() + vec2(10.0, 0.0),
                egui::Align2::LEFT_CENTER,
                &self.doc.pages[i].name,
                FontId::proportional(12.0),
                ui.visuals().text_color(),
            );
            if resp.clicked() {
                self.select_page(i);
            }
            if resp.double_clicked() {
                self.renaming_page = Some((i, self.doc.pages[i].name.clone()));
            }
            resp.context_menu(|ui| {
                if ui.button("Rename").clicked() {
                    self.renaming_page = Some((i, self.doc.pages[i].name.clone()));
                    ui.close();
                }
                if ui.add_enabled(self.doc.pages.len() > 1, egui::Button::new("Delete page")).clicked() {
                    delete = Some(i);
                    ui.close();
                }
            });
        }
        if let Some(i) = delete {
            self.checkpoint();
            for id in self.doc.pages[i].children.clone() {
                self.doc.remove(id);
            }
            self.doc.pages.remove(i);
            self.page = self.page.min(self.doc.pages.len() - 1);
            self.sel.clear();
            self.view_ready = false;
        }
    }

    fn layer_rows(&self) -> Vec<(NodeId, usize)> {
        fn walk(app: &App, list: &[NodeId], depth: usize, out: &mut Vec<(NodeId, usize)>) {
            for &id in list.iter().rev() {
                out.push((id, depth));
                let n = app.doc.node(id);
                if n.expanded && !n.children.is_empty() {
                    walk(app, &n.children, depth + 1, out);
                }
            }
        }
        let mut out = vec![];
        walk(self, &self.doc.pages[self.page].children, 0, &mut out);
        out
    }

    fn layers(&mut self, ui: &mut Ui) {
        let rows = self.layer_rows();
        let mut rects: Vec<(NodeId, Rect)> = vec![];
        let shift = ui.input(|i| i.modifiers.shift || i.modifiers.command);
        for (id, depth) in rows {
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click_and_drag());
            rects.push((id, rect));
            let n = self.doc.node(id).clone();
            let selected = self.sel.contains(&id);
            let in_selected = !selected && self.sel.iter().any(|&s| self.doc.is_ancestor(s, id));
            let p = &ui.painter().clone();
            if selected {
                p.rect_filled(rect, 0.0, SEL_BG);
            } else if in_selected {
                p.rect_filled(rect, 0.0, CHILD_BG);
            } else if resp.hovered() || self.hover == Some(id) {
                p.rect_stroke(rect.shrink(0.5), 0.0, Stroke::new(1.0, BLUE), StrokeKind::Inside);
            }
            let comp = self.accent(id) == crate::canvas::PURPLE;
            let fg = match (comp, n.visible) {
                (true, true) => crate::canvas::PURPLE,
                (true, false) => crate::canvas::PURPLE.gamma_multiply(0.45),
                (false, true) => Color32::from_gray(0x1E),
                (false, false) => Color32::from_gray(0xB3),
            };
            let x = rect.min.x + 6.0 + depth as f32 * 16.0;
            let chev = Rect::from_center_size(pos2(x + 6.0, rect.center().y), vec2(12.0, 12.0));
            if !n.children.is_empty() {
                paint_icon(p, chev, Icon::Chevron(n.expanded), Color32::from_gray(0x80));
            }
            paint_icon(p, Rect::from_center_size(pos2(x + 22.0, rect.center().y), vec2(16.0, 16.0)), kind_icon(n.kind), fg);
            let name_pos = pos2(x + 36.0, rect.center().y);
            let renaming = matches!(&self.renaming, Some((r, _)) if *r == id);
            if renaming {
                let mut buf = self.renaming.as_ref().unwrap().1.clone();
                let edit_rect = Rect::from_min_max(pos2(name_pos.x - 2.0, rect.min.y + 3.0), rect.max - vec2(48.0, 3.0));
                let r = ui.put(edit_rect, egui::TextEdit::singleline(&mut buf));
                r.request_focus();
                self.renaming = Some((id, buf.clone()));
                if r.lost_focus() {
                    self.renaming = None;
                    let name = buf.trim().to_string();
                    if !name.is_empty() && name != self.doc.node(id).name {
                        self.checkpoint();
                        self.doc.node_mut(id).name = name;
                    }
                }
            } else {
                let strong = (n.parent.is_none() && n.kind.is_frame()) || n.kind == Kind::Component;
                let font = if strong { FontId::new(12.0, egui::FontFamily::Name("Inter SemiBold".into())) } else { FontId::proportional(12.0) };
                p.text(name_pos, egui::Align2::LEFT_CENTER, &n.name, font, fg);
            }
            // Eye / lock: always shown when active, otherwise on hover.
            let hovered = resp.hovered();
            let eye_r = Rect::from_center_size(pos2(rect.max.x - 16.0, rect.center().y), vec2(20.0, 20.0));
            let lock_r = Rect::from_center_size(pos2(rect.max.x - 38.0, rect.center().y), vec2(20.0, 20.0));
            let (visible, locked) = (n.visible, n.locked);
            if hovered || !visible {
                paint_icon(p, eye_r, Icon::Eye(visible), Color32::from_gray(0x60));
            }
            if hovered || locked {
                paint_icon(p, lock_r, Icon::Lock(locked), Color32::from_gray(0x60));
            }
            let click_pos = resp.interact_pointer_pos();
            if resp.clicked() {
                let at = |r: Rect| click_pos.is_some_and(|c| r.contains(c));
                if at(eye_r) {
                    self.checkpoint();
                    self.doc.node_mut(id).visible = !visible;
                } else if at(lock_r) {
                    self.checkpoint();
                    self.doc.node_mut(id).locked = !locked;
                    if !locked {
                        self.sel.retain(|s| *s != id);
                    }
                } else if at(chev.expand(4.0)) && !self.doc.node(id).children.is_empty() {
                    let n = self.doc.node_mut(id);
                    n.expanded = !n.expanded;
                } else if shift {
                    if selected { self.sel.retain(|s| *s != id) } else { self.sel.push(id) }
                } else {
                    self.end_text_edit();
                    self.sel = vec![id];
                }
            }
            if resp.double_clicked() && !click_pos.is_some_and(|c| eye_r.contains(c) || lock_r.contains(c)) {
                self.renaming = Some((id, self.doc.node(id).name.clone()));
            }
            if resp.drag_started() {
                self.layer_drag = Some(id);
            }
        }
        self.layer_drop(ui, &rects);
    }

    /// Drag-and-drop reordering in the layers list.
    fn layer_drop(&mut self, ui: &mut Ui, rects: &[(NodeId, Rect)]) {
        let Some(dragged) = self.layer_drag else { return };
        let (ptr, released) = ui.input(|i| (i.pointer.hover_pos(), i.pointer.any_released()));
        let target = ptr.and_then(|p| {
            rects.iter().find(|(_, r)| p.y >= r.min.y && p.y < r.max.y).map(|&(id, r)| {
                let t = (p.y - r.min.y) / r.height();
                let container = self.doc.node(id).kind.is_container();
                let zone = if container && (0.3..0.7).contains(&t) { 1 } else if t < 0.5 { 0 } else { 2 };
                (id, r, zone)
            })
        });
        let valid = |id: NodeId| id != dragged && !self.doc.is_ancestor(dragged, id);
        if let Some((id, r, zone)) = target.filter(|t| valid(t.0)) {
            let p = ui.painter();
            match zone {
                1 => {
                    p.rect_stroke(r.shrink(1.0), 2.0, Stroke::new(2.0, BLUE), StrokeKind::Inside);
                }
                z => {
                    let y = if z == 0 { r.min.y } else { r.max.y };
                    p.line_segment([pos2(r.min.x + 8.0, y), pos2(r.max.x, y)], Stroke::new(2.0, BLUE));
                }
            }
            if released {
                self.checkpoint();
                let n = self.doc.node(id);
                let into_top = zone == 1 || (zone == 2 && n.expanded && !n.children.is_empty());
                if into_top {
                    self.doc.reparent(dragged, self.page, Some(id), None);
                } else {
                    let parent = n.parent;
                    let list: Vec<NodeId> = self.doc.children_of(self.page, parent).iter().copied().filter(|c| *c != dragged).collect();
                    let i = list.iter().position(|c| *c == id).unwrap_or(0);
                    // The list is drawn top-first, so "above" means a higher z index.
                    self.doc.reparent(dragged, self.page, parent, Some(if zone == 0 { i + 1 } else { i }));
                }
                self.doc.reflow();
                self.sel = vec![dragged];
            }
        }
        if released {
            self.layer_drag = None;
        }
    }

    // ---------- right: design properties ----------

    pub fn right_panel(&mut self, ui: &mut Ui) {
        egui::Panel::right("right").resizable(false).exact_size(248.0).show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Design").strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let ctx = ui.ctx().clone();
                    ui.menu_button(format!("{}%", (self.view.zoom * 100.0).round()), |ui| {
                        if ui.add(egui::Button::new("Zoom in").shortcut_text("⌘+")).clicked() {
                            self.zoom_step(&ctx, 2.0);
                        }
                        if ui.add(egui::Button::new("Zoom out").shortcut_text("⌘-")).clicked() {
                            self.zoom_step(&ctx, 0.5);
                        }
                        if ui.add(egui::Button::new("Zoom to fit").shortcut_text("⇧1")).clicked() {
                            self.zoom_to(&ctx, None, false);
                        }
                        if ui.add(egui::Button::new("Zoom to 100%").shortcut_text("⇧0")).clicked() {
                            self.zoom_to(&ctx, None, true);
                        }
                    });
                });
            });
            ui.separator();
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.set_max_width(ui.available_width());
                // Never let a long label widen the panel.
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                ui.spacing_mut().item_spacing.y = 6.0;
                if self.sel.is_empty() {
                    self.page_props(ui);
                } else {
                    self.selection_props(ui);
                }
                ui.separator();
                self.export_props(ui);
            });
        });
    }

    fn page_props(&mut self, ui: &mut Ui) {
        heading(ui, "Page");
        let mut bg = self.doc.pages[self.page].background;
        ui.horizontal(|ui| {
            let mut c = [bg.0[0], bg.0[1], bg.0[2]];
            let mut changed = egui::color_picker::color_edit_button_srgb(ui, &mut c).changed();
            bg = Rgba::rgb(c[0], c[1], c[2]);
            changed |= hex_field(ui, ui.id().with("page-bg"), &mut bg, 56.0);
            if changed {
                self.begin_edit();
                self.doc.pages[self.page].background = bg;
            }
        });
    }

    fn selection_props(&mut self, ui: &mut Ui) {
        let roots = self.sel_roots();
        let first = self.sel[0];
        let n = self.doc.node(first).clone();
        self.component_section(ui, first);
        self.position_section(ui, &roots);
        ui.separator();
        self.layout_section(ui, &roots);
        if let [one] = roots[..] {
            self.constraints_section(ui, one);
        }
        ui.separator();
        self.appearance_section(ui, &n);
        if self.sel.iter().all(|&id| self.doc.node(id).kind == Kind::Text) {
            ui.separator();
            self.typography_section(ui, &n);
        }
        if n.kind != Kind::Group {
            ui.separator();
            self.paints_section(ui, false);
            ui.separator();
            self.paints_section(ui, true);
        }
        ui.separator();
        self.effects_section(ui);
    }

    fn component_section(&mut self, ui: &mut Ui, id: NodeId) {
        let n = self.doc.node(id).clone();
        let inst = self.owning_instance(id);
        if n.kind != Kind::Component && inst.is_none() {
            return;
        }
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
            let icon = if n.kind == Kind::Component { Icon::Component } else { Icon::Instance };
            paint_icon(ui.painter(), rect, icon, crate::canvas::PURPLE);
            let title = match (n.kind, inst) {
                (Kind::Component, _) => n.name.clone(),
                (_, Some(i)) => {
                    let main = self.doc.node(i).component.and_then(|c| self.doc.nodes.get(&c)).map_or("Missing component".into(), |c| c.name.clone());
                    format!("Instance of {main}")
                }
                _ => String::new(),
            };
            ui.label(RichText::new(title).strong().color(crate::canvas::PURPLE));
        });
        if let Some(i) = inst {
            ui.horizontal(|ui| {
                if ui.button("Go to main").clicked() {
                    self.go_to_main(i);
                }
                if ui.button("Reset").clicked() {
                    self.checkpoint();
                    self.doc.reset_instance(i);
                }
                if ui.button("Detach").clicked() {
                    self.checkpoint();
                    self.doc.detach_instance(i);
                }
            });
        } else if ui.button("Create instance").clicked() {
            self.checkpoint();
            self.sel = self.duplicate_or_instance(&[id]);
            if let Some(&i) = self.sel.first() {
                self.doc.translate(i, vec2(n.rect.width() + 40.0, 0.0));
            }
        }
        ui.separator();
    }

    /// Origin that X/Y are reported against: the nearest frame, like Figma.
    fn frame_origin(&self, id: NodeId) -> egui::Vec2 {
        self.doc
            .ancestors(id)
            .into_iter()
            .find(|a| self.doc.node(*a).kind.is_frame())
            .map(|f| self.doc.node(f).rect.min.to_vec2())
            .unwrap_or_default()
    }

    fn position_section(&mut self, ui: &mut Ui, roots: &[NodeId]) {
        heading(ui, "Position");
        ui.horizontal(|ui| {
            for a in [Align::Left, Align::HCenter, Align::Right, Align::Top, Align::VCenter, Align::Bottom] {
                if icon_button(ui, Icon::Align(a), false, 26.0, &format!("Align {a:?}")).clicked() {
                    self.align(a);
                }
            }
        });
        let first = roots[0];
        let b = self.doc.bounds(roots);
        let origin = self.frame_origin(first);
        // A lone rotated layer reports its rotated top-left corner, like Figma.
        let single = (roots.len() == 1).then(|| self.doc.node(first).clone()).filter(|n| n.rotation != 0.0);
        let anchor = single.as_ref().map_or(b.min, |n| n.corners()[0]);
        let (mut x, mut y) = (anchor.x - origin.x, anchor.y - origin.y);
        let mut moved = false;
        ui.horizontal(|ui| {
            moved |= num_field(ui, &mut x, "X", -1e6, half(ui));
            moved |= num_field(ui, &mut y, "Y", -1e6, half(ui));
        });
        if moved {
            self.begin_edit();
            let d = vec2(x + origin.x, y + origin.y) - anchor.to_vec2();
            for &id in roots {
                self.doc.translate(id, d);
            }
            self.doc.reflow();
        }
        // Rotation: a lone layer turns around its center, several turn around their shared center.
        let mut rot = if roots.len() == 1 { self.doc.node(first).rotation } else { 0.0 };
        let old = rot;
        ui.horizontal(|ui| {
            if num_field(ui, &mut rot, "@rotate", -1e6, half(ui)) {
                self.begin_edit();
                let c = self.doc.bounds(roots).center();
                for &id in roots {
                    let pivot = if roots.len() == 1 { self.doc.node(id).rect.center() } else { c };
                    self.doc.rotate(id, rot - old, pivot);
                }
            }
        });
    }

    fn layout_section(&mut self, ui: &mut Ui, roots: &[NodeId]) {
        let first = roots[0];
        let n = self.doc.node(first).clone();
        let is_frame = roots.len() == 1 && n.kind.is_frame();
        let al = is_frame && n.layout.mode != LayoutMode::None;
        ui.horizontal(|ui| {
            ui.label(RichText::new(if is_frame { "Auto layout" } else { "Layout" }).strong());
            if is_frame {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (icon, tip) = if al { (Icon::Minus, "Remove auto layout (⌥⇧A)") } else { (Icon::Plus, "Add auto layout (⇧A)") };
                    if icon_button(ui, icon, false, 22.0, tip).clicked() {
                        if al { self.remove_auto_layout() } else { self.add_auto_layout() }
                    }
                });
            }
        });
        if al {
            self.auto_layout_controls(ui, first);
        }

        // Size, with Figma's Fixed / Hug / Fill modes where they apply.
        let b = self.doc.bounds(roots);
        let single = (roots.len() == 1).then(|| self.doc.node(first).clone()).filter(|n| n.rotation != 0.0);
        let size = single.as_ref().map_or(b.size(), |n| n.rect.size());
        let (mut w, mut h) = (size.x, size.y);
        let parent_al = n.parent.is_some_and(|p| self.doc.node(p).layout.mode != LayoutMode::None) && !n.absolute;
        let can_hug = n.layout.mode != LayoutMode::None;
        let mut sizing = n.sizing;
        let mut sized = false;
        let mut sizing_changed = false;
        ui.horizontal(|ui| {
            let menus = roots.len() == 1 && (can_hug || parent_al);
            let wide = if menus { half(ui) - 22.0 } else { half(ui) };
            for (axis, v, label) in [(0usize, &mut w, "W"), (1, &mut h, "H")] {
                let shown = match (menus, sizing[axis]) {
                    (true, Sizing::Hug) => "W Hug",
                    (true, Sizing::Fill) => "W Fill",
                    _ => label,
                };
                let shown = if axis == 1 { shown.replacen('W', "H", 1) } else { shown.to_string() };
                sized |= num_field(ui, v, &shown, 0.01, wide);
                if menus {
                    sizing_changed |= sizing_menu(ui, ui.id().with(("sizing", axis)), &mut sizing[axis], can_hug, parent_al);
                }
            }
        });
        if sizing_changed {
            self.checkpoint();
            self.doc.node_mut(first).sizing = sizing;
            self.doc.reflow();
        }
        if sized {
            self.begin_edit();
            for &id in roots {
                let s = &mut self.doc.node_mut(id).sizing;
                if (w - size.x).abs() > 1e-4 {
                    s[0] = Sizing::Fixed;
                }
                if (h - size.y).abs() > 1e-4 {
                    s[1] = Sizing::Fixed;
                }
            }
            if let Some(n) = &single {
                // Grow from the rotated top-left corner.
                let local = Rect::from_min_size(n.rect.min, vec2(w, h));
                let c = crate::doc::rotate(local.center(), n.rect.center(), n.rotation);
                self.doc.set_rect(n.id, Rect::from_center_size(c, local.size()));
            } else if b.width() > 0.0 || b.height() > 0.0 {
                let nb = Rect::from_min_size(b.min, vec2(w, h));
                let sx = if b.width() > 0.0 { w / b.width() } else { 1.0 };
                let sy = if b.height() > 0.0 { h / b.height() } else { 1.0 };
                let map = |p: Pos2| pos2(nb.min.x + (p.x - b.min.x) * sx, nb.min.y + (p.y - b.min.y) * sy);
                for &id in roots {
                    let r = self.doc.node(id).rect;
                    self.doc.set_rect(id, Rect::from_two_pos(map(r.min), map(r.max)));
                }
            }
            self.doc.reflow();
        }
        if parent_al || n.absolute {
            let mut abs = n.absolute;
            if ui.checkbox(&mut abs, "Ignore auto layout").changed() {
                self.checkpoint();
                self.doc.node_mut(first).absolute = abs;
                self.doc.reflow();
            }
        }
        if n.kind.is_frame() {
            let mut clip = n.clip;
            if ui.checkbox(&mut clip, "Clip content").changed() {
                self.checkpoint();
                for &id in &self.sel.clone() {
                    if self.doc.node(id).kind.is_frame() {
                        self.doc.node_mut(id).clip = clip;
                    }
                }
            }
        }
    }

    fn auto_layout_controls(&mut self, ui: &mut Ui, f: NodeId) {
        let mut l = self.doc.node(f).layout;
        let before = l;
        ui.horizontal(|ui| {
            for (m, icon, tip) in [(LayoutMode::Vertical, Icon::ArrowDown, "Vertical layout"), (LayoutMode::Horizontal, Icon::ArrowRight, "Horizontal layout")] {
                if icon_button(ui, icon, l.mode == m, 26.0, tip).clicked() {
                    l.mode = m;
                }
            }
        });
        ui.horizontal(|ui| {
            align_grid(ui, &mut l);
            ui.vertical(|ui| {
                ui.add_enabled_ui(!l.space_between, |ui| {
                    num_field(ui, &mut l.gap, if l.mode == LayoutMode::Horizontal { "@gaph" } else { "@gapv" }, 0.0, (ui.available_width()).min(110.0));
                });
                ui.checkbox(&mut l.space_between, "Auto gap");
            });
        });
        let [t, r, b, lft] = &mut l.padding;
        let mut ph = if (*lft - *r).abs() < 1e-4 { *lft } else { f32::NAN };
        let mut pv = if (*t - *b).abs() < 1e-4 { *t } else { f32::NAN };
        let split_id = ui.id().with(("pad-split", f));
        let mut split = ui.data(|d| d.get_temp::<bool>(split_id)).unwrap_or(ph.is_nan() || pv.is_nan());
        ui.horizontal(|ui| {
            if split {
                for (v, label) in [(&mut *lft, "L"), (&mut *t, "T"), (&mut *r, "R"), (&mut *b, "B")] {
                    num_field(ui, v, label, 0.0, (ui.available_width() - 30.0) / 4.0 - 4.0);
                }
            } else {
                if num_field(ui, &mut ph, "@padh", 0.0, half(ui) - 16.0) {
                    (*lft, *r) = (ph, ph);
                }
                if num_field(ui, &mut pv, "@padv", 0.0, half(ui) - 16.0) {
                    (*t, *b) = (pv, pv);
                }
            }
            if icon_button(ui, Icon::Frame, split, 22.0, "Individual padding").clicked() {
                split = !split;
            }
        });
        ui.data_mut(|d| d.insert_temp(split_id, split));
        if l != before {
            self.begin_edit();
            self.doc.node_mut(f).layout = l;
            self.doc.reflow();
        }
    }

    fn constraints_section(&mut self, ui: &mut Ui, id: NodeId) {
        let n = self.doc.node(id).clone();
        let Some(p) = n.parent else { return };
        let pn = self.doc.node(p);
        if !pn.kind.is_frame() || (pn.layout.mode != LayoutMode::None && !n.absolute) {
            return;
        }
        ui.separator();
        heading(ui, "Constraints");
        let mut c = n.constraints;
        let names = |axis: usize, k: Constraint| match (axis, k) {
            (0, Constraint::Min) => "Left",
            (0, Constraint::Max) => "Right",
            (0, Constraint::Stretch) => "Left & right",
            (1, Constraint::Min) => "Top",
            (1, Constraint::Max) => "Bottom",
            (1, Constraint::Stretch) => "Top & bottom",
            (_, Constraint::Center) => "Center",
            (_, Constraint::Scale) => "Scale",
            _ => "",
        };
        let mut changed = false;
        ui.horizontal(|ui| {
            for axis in 0..2 {
                egui::ComboBox::from_id_salt(("constraint", axis)).width(half(ui) - 12.0).selected_text(names(axis, c[axis])).show_ui(ui, |ui| {
                    for k in [Constraint::Min, Constraint::Max, Constraint::Stretch, Constraint::Center, Constraint::Scale] {
                        changed |= ui.selectable_value(&mut c[axis], k, names(axis, k)).changed();
                    }
                });
            }
        });
        if changed {
            self.checkpoint();
            self.doc.node_mut(id).constraints = c;
        }
    }

    fn appearance_section(&mut self, ui: &mut Ui, n: &crate::doc::Node) {
        heading(ui, "Appearance");
        let mut op = (n.opacity * 100.0).round();
        let mut r = n.radius;
        let has_radius = n.kind == Kind::Rectangle || n.kind.is_frame();
        ui.horizontal(|ui| {
            if num_field(ui, &mut op, "@opacity", 0.0, half(ui)) {
                self.begin_edit();
                for &id in &self.sel {
                    self.doc.node_mut(id).opacity = op.min(100.0) / 100.0;
                }
            }
            if has_radius && num_field(ui, &mut r, "@radius", 0.0, half(ui)) {
                self.begin_edit();
                for &id in &self.sel {
                    if self.doc.node(id).kind == Kind::Rectangle || self.doc.node(id).kind.is_frame() {
                        self.doc.node_mut(id).radius = r;
                    }
                }
            }
        });
    }

    fn typography_section(&mut self, ui: &mut Ui, n: &crate::doc::Node) {
        use crate::doc::{Decoration, LineHeight, TextAlign, TextCase, TextSizing, VAlign};
        use crate::text::{families, weight_name, weights_for};
        heading(ui, "Typography");
        let mut t = n.clone();
        // Font family picker with search.
        let fam_id = ui.id().with("font-search");
        let resp = ui.add_sized([ui.available_width(), 24.0], egui::Button::new(&t.font_family));
        egui::Popup::menu(&resp).id(fam_id.with("popup")).show(|ui| {
            ui.set_width(220.0);
            let mut q: String = ui.data(|d| d.get_temp(fam_id)).unwrap_or_default();
            ui.add(egui::TextEdit::singleline(&mut q).hint_text("Search fonts").desired_width(f32::INFINITY));
            ui.data_mut(|d| d.insert_temp(fam_id, q.clone()));
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                let ql = q.to_lowercase();
                for f in families().iter().filter(|f| f.to_lowercase().contains(&ql)).take(400) {
                    if ui.selectable_label(*f == t.font_family, f).clicked() {
                        t.font_family = f.clone();
                        let ws = weights_for(f);
                        if !ws.contains(&t.font_weight) {
                            t.font_weight = *ws.iter().min_by_key(|w| (**w as i32 - 400).abs()).unwrap_or(&400);
                        }
                        ui.close();
                    }
                }
            });
        });
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("weight").width(half(ui) - 12.0).selected_text(weight_name(t.font_weight)).show_ui(ui, |ui| {
                for w in weights_for(&t.font_family) {
                    ui.selectable_value(&mut t.font_weight, w, weight_name(w));
                }
            });
            num_field(ui, &mut t.font_size, "", 1.0, half(ui));
        });
        // Line height: "Auto", px, or "%" typed into the field.
        ui.horizontal(|ui| {
            let lh_id = ui.id().with(("lh", n.id));
            let shown = match t.line_height {
                LineHeight::Auto => "Auto".to_string(),
                LineHeight::Px(v) => num(v),
                LineHeight::Percent(p) => format!("{}%", num(p)),
            };
            let mut buf: String = ui.data(|d| d.get_temp(lh_id)).unwrap_or(shown);
            let r = ui.add_sized([half(ui), 22.0], egui::TextEdit::singleline(&mut buf).id(lh_id).hint_text("Line height"));
            if r.has_focus() {
                ui.data_mut(|d| d.insert_temp(lh_id, buf.clone()));
            } else {
                ui.data_mut(|d| d.remove::<String>(lh_id));
            }
            if r.lost_focus() {
                let v = buf.trim();
                t.line_height = if v.eq_ignore_ascii_case("auto") || v.is_empty() {
                    LineHeight::Auto
                } else if let Some(p) = v.strip_suffix('%').and_then(|p| p.trim().parse().ok()) {
                    LineHeight::Percent(p)
                } else if let Ok(px) = v.parse() {
                    LineHeight::Px(px)
                } else {
                    t.line_height
                };
            }
            r.on_hover_text("Line height: Auto, pixels, or percent");
            let mut ls = t.letter_spacing;
            if ui.add_sized([half(ui), 22.0], DragValue::new(&mut ls).speed(0.1).suffix("%").prefix("|A|  ").custom_formatter(|v, _| num(v as f32))).changed() {
                t.letter_spacing = ls;
            }
        });
        // Alignment and resizing.
        ui.horizontal(|ui| {
            for (a, icon) in [(TextAlign::Left, Icon::Align(Align::Left)), (TextAlign::Center, Icon::Align(Align::HCenter)), (TextAlign::Right, Icon::Align(Align::Right))] {
                if icon_button(ui, icon, t.text_align == a, 24.0, &format!("Align {a:?}")).clicked() {
                    t.text_align = a;
                }
            }
            ui.add_space(6.0);
            for (v, icon) in [(VAlign::Top, Icon::Align(Align::Top)), (VAlign::Middle, Icon::Align(Align::VCenter)), (VAlign::Bottom, Icon::Align(Align::Bottom))] {
                if icon_button(ui, icon, t.text_valign == v, 24.0, &format!("Align {v:?}")).clicked() {
                    t.text_valign = v;
                    if t.text_sizing != TextSizing::Fixed {
                        t.text_sizing = TextSizing::Fixed;
                    }
                }
            }
        });
        ui.horizontal(|ui| {
            let sizing = match t.text_sizing {
                TextSizing::AutoWidth => "Auto width",
                TextSizing::AutoHeight => "Auto height",
                TextSizing::Fixed => "Fixed size",
            };
            egui::ComboBox::from_id_salt("text-sizing").width(half(ui) - 12.0).selected_text(sizing).show_ui(ui, |ui| {
                ui.selectable_value(&mut t.text_sizing, TextSizing::AutoWidth, "Auto width");
                ui.selectable_value(&mut t.text_sizing, TextSizing::AutoHeight, "Auto height");
                ui.selectable_value(&mut t.text_sizing, TextSizing::Fixed, "Fixed size");
            });
            let case = match t.text_case {
                TextCase::Original => "Aa  As typed",
                TextCase::Upper => "AA  Upper",
                TextCase::Lower => "aa  Lower",
                TextCase::Title => "Aa  Title",
            };
            egui::ComboBox::from_id_salt("text-case").width(half(ui) - 12.0).selected_text(case).show_ui(ui, |ui| {
                ui.selectable_value(&mut t.text_case, TextCase::Original, "As typed");
                ui.selectable_value(&mut t.text_case, TextCase::Upper, "UPPER CASE");
                ui.selectable_value(&mut t.text_case, TextCase::Lower, "lower case");
                ui.selectable_value(&mut t.text_case, TextCase::Title, "Title Case");
            });
        });
        ui.horizontal(|ui| {
            for (d, label) in [(Decoration::Underline, RichText::new(" U ").underline()), (Decoration::Strikethrough, RichText::new(" S ").strikethrough())] {
                let on = t.decoration == d;
                if ui.selectable_label(on, label).on_hover_text(format!("{d:?}")).clicked() {
                    t.decoration = if on { Decoration::None } else { d };
                }
            }
        });
        let changed = t.font_family != n.font_family
            || t.font_weight != n.font_weight
            || t.font_size != n.font_size
            || t.line_height != n.line_height
            || t.letter_spacing != n.letter_spacing
            || t.text_align != n.text_align
            || t.text_valign != n.text_valign
            || t.text_sizing != n.text_sizing
            || t.decoration != n.decoration
            || t.text_case != n.text_case;
        if changed {
            self.begin_edit();
            for &id in &self.sel.clone() {
                let d = self.doc.node_mut(id);
                d.font_family = t.font_family.clone();
                d.font_weight = t.font_weight;
                d.font_size = t.font_size;
                d.line_height = t.line_height;
                d.letter_spacing = t.letter_spacing;
                d.text_align = t.text_align;
                d.text_valign = t.text_valign;
                d.text_sizing = t.text_sizing;
                d.decoration = t.decoration;
                d.text_case = t.text_case;
            }
        }
    }

    fn paints_section(&mut self, ui: &mut Ui, stroke: bool) {
        let first = self.sel[0];
        let n = self.doc.node(first).clone();
        let mut paints = if stroke { n.strokes.clone() } else { n.fills.clone() };
        let mut changed = false;
        if section_title(ui, if stroke { "Stroke" } else { "Fill" }) {
            let c = match (stroke, n.kind) {
                (true, _) | (false, Kind::Text) => Rgba::BLACK,
                (false, Kind::Frame) => Rgba::WHITE,
                _ => SHAPE_GRAY,
            };
            paints.push(Paint::solid(c));
            changed = true;
        }
        let mut remove = None;
        // Topmost paint first, as in Figma.
        for i in (0..paints.len()).rev() {
            let id = ui.id().with((stroke, i, first));
            match paint_row(ui, id, &mut paints[i]) {
                PaintEvent::None => {}
                PaintEvent::Changed => changed = true,
                PaintEvent::Remove => remove = Some(i),
                PaintEvent::Image(bytes) => {
                    paints[i].image = self.add_image(bytes);
                    changed = true;
                }
            }
        }
        if let Some(i) = remove {
            paints.remove(i);
            changed = true;
        }
        let (mut width, mut align) = (n.stroke_width, n.stroke_align);
        let mut stroke_changed = false;
        if stroke && !paints.is_empty() {
            ui.horizontal(|ui| {
                stroke_changed |= ui
                    .add(DragValue::new(&mut width).range(0.0..=1000.0).speed(0.1).prefix("Weight  ").custom_formatter(|v, _| num(v as f32)))
                    .changed();
                if n.kind != Kind::Vector {
                    egui::ComboBox::from_id_salt("stroke-align").width(80.0).selected_text(format!("{align:?}")).show_ui(ui, |ui| {
                        for a in [StrokeAlign::Inside, StrokeAlign::Center, StrokeAlign::Outside] {
                            stroke_changed |= ui.selectable_value(&mut align, a, format!("{a:?}")).changed();
                        }
                    });
                }
            });
        }
        if changed || stroke_changed {
            self.begin_edit();
            for &id in &self.sel.clone() {
                let t = self.doc.node_mut(id);
                if t.kind == Kind::Group {
                    continue;
                }
                if stroke {
                    t.strokes = paints.clone();
                    if stroke_changed {
                        t.stroke_width = width;
                        if t.kind != Kind::Vector {
                            t.stroke_align = align;
                        }
                    }
                } else {
                    t.fills = paints.clone();
                }
            }
        }
    }

    fn effects_section(&mut self, ui: &mut Ui) {
        let first = self.sel[0];
        let mut shadows = self.doc.node(first).shadows.clone();
        let mut changed = false;
        if section_title(ui, "Effects") {
            shadows.push(Shadow::default());
            changed = true;
        }
        let mut remove = None;
        for i in (0..shadows.len()).rev() {
            let (c, r) = shadow_row(ui, ui.id().with(("shadow", i, first)), &mut shadows[i]);
            changed |= c;
            if r {
                remove = Some(i);
            }
        }
        if let Some(i) = remove {
            shadows.remove(i);
            changed = true;
        }
        if changed {
            self.begin_edit();
            for &id in &self.sel.clone() {
                self.doc.node_mut(id).shadows = shadows.clone();
            }
        }
    }

    fn export_props(&mut self, ui: &mut Ui) {
        heading(ui, "Export");
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("export-scale")
                .width(60.0)
                .selected_text(format!("{}x", num(self.export_scale)))
                .show_ui(ui, |ui| {
                    for s in [0.5, 1.0, 2.0, 3.0, 4.0] {
                        ui.selectable_value(&mut self.export_scale, s, format!("{}x", num(s)));
                    }
                });
            egui::ComboBox::from_id_salt("export-format")
                .width(60.0)
                .selected_text(if self.export_svg { "SVG" } else { "PNG" })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.export_svg, false, "PNG");
                    ui.selectable_value(&mut self.export_svg, true, "SVG");
                });
        });
        let label = match self.sel_roots().as_slice() {
            [] => format!("Export {}", self.doc.pages[self.page].name),
            [one] => format!("Export {}", self.doc.node(*one).name),
            many => format!("Export {} layers", many.len()),
        };
        if ui.add_sized([ui.available_width(), 26.0], egui::Button::new(label)).clicked() {
            self.export();
        }
    }

    // ---------- bottom toolbar ----------

    pub fn toolbar(&mut self, ctx: &egui::Context, canvas: Rect) {
        egui::Area::new(egui::Id::new("toolbar"))
            .fixed_pos(pos2(canvas.center().x, canvas.max.y - 16.0))
            .pivot(egui::Align2::CENTER_BOTTOM)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::WHITE)
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(6)
                    .stroke(Stroke::new(1.0, Color32::from_gray(0xE6)))
                    .shadow(egui::Shadow { offset: [0, 2], blur: 10, spread: 0, color: Color32::from_black_alpha(30) })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            let tools = [
                                (Tool::Move, Icon::Move, "Move (V)"),
                                (Tool::Frame, Icon::Frame, "Frame (F)"),
                                (Tool::Rectangle, Icon::Rect, "Rectangle (R)"),
                                (Tool::Ellipse, Icon::Ellipse, "Ellipse (O)"),
                                (Tool::Line, Icon::Line, "Line (L)"),
                                (Tool::Pen, Icon::Pen, "Pen (P)"),
                                (Tool::Text, Icon::Text, "Text (T)"),
                                (Tool::Hand, Icon::Hand, "Hand tool (H)"),
                            ];
                            for (t, icon, tip) in tools {
                                if icon_button(ui, icon, self.tool == t, 32.0, tip).clicked() {
                                    self.set_tool(t);
                                }
                            }
                        });
                    });
            });
    }
}

