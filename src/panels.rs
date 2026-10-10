//! Side panels (pages, layers, design properties), floating toolbar, and painted icons.

use crate::app::{Align, App, Tool};
use crate::canvas::{BLUE, num};
use crate::doc::{Kind, NodeId, Page, Paint, Rgba, SHAPE_GRAY, Shadow, StrokeAlign};
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
}

const SUBTLE: Color32 = Color32::from_rgb(0xF5, 0xF5, 0xF5);
const SEL_BG: Color32 = Color32::from_rgb(0xE5, 0xF4, 0xFF);
const CHILD_BG: Color32 = Color32::from_rgb(0xF2, 0xF9, 0xFF);

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
        Kind::Group => Icon::Group,
        Kind::Rectangle => Icon::Rect,
        Kind::Ellipse => Icon::Ellipse,
        Kind::Vector => Icon::Pen,
        Kind::Text => Icon::Text,
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
            ui.separator();
            self.pages_list(ui);
            ui.separator();
            ui.label(RichText::new("Layers").strong());
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| self.layers(ui));
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
            let dim = if n.visible { 0x1E } else { 0xB3 };
            let fg = Color32::from_gray(dim);
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
                let strong = n.parent.is_none() && n.kind == Kind::Frame;
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
                self.doc.fit_groups();
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
        // Alignment
        ui.horizontal(|ui| {
            for a in [Align::Left, Align::HCenter, Align::Right, Align::Top, Align::VCenter, Align::Bottom] {
                if icon_button(ui, Icon::Align(a), false, 26.0, &format!("Align {a:?}")).clicked() {
                    self.align(a);
                }
            }
        });
        ui.separator();

        // Position & size, relative to the nearest frame like Figma.
        let b = self.doc.bounds(&roots);
        let origin = self
            .doc
            .ancestors(first)
            .into_iter()
            .find(|a| self.doc.node(*a).kind == Kind::Frame)
            .map(|f| self.doc.node(f).rect.min.to_vec2())
            .unwrap_or_default();
        // A lone rotated layer reports its own size and its rotated top-left corner, like Figma.
        let single = (roots.len() == 1).then(|| self.doc.node(first).clone()).filter(|n| n.rotation != 0.0);
        let (anchor, size) = match &single {
            Some(n) => (n.corners()[0], n.rect.size()),
            None => (b.min, b.size()),
        };
        let (mut x, mut y, mut w, mut h) = (anchor.x - origin.x, anchor.y - origin.y, size.x, size.y);
        let half = (ui.available_width() - 8.0) / 2.0;
        let field = |ui: &mut Ui, v: &mut f32, prefix: &str, min: f32| {
            ui.add_sized([half, 22.0], DragValue::new(v).prefix(prefix).speed(1.0).range(min..=1e6).custom_formatter(|v, _| num(v as f32)))
                .changed()
        };
        let (mut moved, mut sized) = (false, false);
        ui.horizontal(|ui| {
            moved |= field(ui, &mut x, "X  ", -1e6);
            moved |= field(ui, &mut y, "Y  ", -1e6);
        });
        ui.horizontal(|ui| {
            sized |= field(ui, &mut w, "W  ", 0.01);
            sized |= field(ui, &mut h, "H  ", 0.01);
        });
        if moved {
            self.begin_edit();
            let d = vec2(x + origin.x, y + origin.y) - anchor.to_vec2();
            for &id in &roots {
                self.doc.translate(id, d);
            }
            self.doc.fit_groups();
        }
        if let (true, Some(n)) = (sized, &single) {
            // Grow from the rotated top-left corner.
            self.begin_edit();
            let local = Rect::from_min_size(n.rect.min, vec2(w, h));
            let c = crate::doc::rotate(local.center(), n.rect.center(), n.rotation);
            self.doc.set_rect(n.id, Rect::from_center_size(c, local.size()));
            self.doc.fit_groups();
        } else if sized && (b.width() > 0.0 || b.height() > 0.0) {
            self.begin_edit();
            let nb = Rect::from_min_size(b.min, vec2(w, h));
            let sx = if b.width() > 0.0 { w / b.width() } else { 1.0 };
            let sy = if b.height() > 0.0 { h / b.height() } else { 1.0 };
            let map = |p: Pos2| pos2(nb.min.x + (p.x - b.min.x) * sx, nb.min.y + (p.y - b.min.y) * sy);
            for &id in &roots {
                let r = self.doc.node(id).rect;
                self.doc.set_rect(id, Rect::from_two_pos(map(r.min), map(r.max)));
            }
            self.doc.fit_groups();
        }
        // Rotation: a lone layer turns around its center, several turn around their shared center.
        let mut rot = if roots.len() == 1 { self.doc.node(first).rotation } else { 0.0 };
        let old = rot;
        ui.horizontal(|ui| {
            ui.label("Rotation");
            if ui.add(DragValue::new(&mut rot).speed(0.5).suffix("°").custom_formatter(|v, _| num(v as f32))).changed() {
                self.begin_edit();
                let c = self.doc.bounds(&roots).center();
                for &id in &roots {
                    let pivot = if roots.len() == 1 { self.doc.node(id).rect.center() } else { c };
                    self.doc.rotate(id, rot - old, pivot);
                }
            }
        });
        let n = self.doc.node(first).clone();
        if matches!(n.kind, Kind::Rectangle | Kind::Frame) {
            let mut r = n.radius;
            ui.horizontal(|ui| {
                ui.label("Radius");
                if ui.add(DragValue::new(&mut r).range(0.0..=1e5).speed(0.5).custom_formatter(|v, _| num(v as f32))).changed() {
                    self.begin_edit();
                    for &id in &self.sel {
                        if matches!(self.doc.node(id).kind, Kind::Rectangle | Kind::Frame) {
                            self.doc.node_mut(id).radius = r;
                        }
                    }
                }
            });
        }
        if n.kind == Kind::Frame {
            let mut clip = n.clip;
            if ui.checkbox(&mut clip, "Clip content").changed() {
                self.checkpoint();
                for &id in &self.sel {
                    if self.doc.node(id).kind == Kind::Frame {
                        self.doc.node_mut(id).clip = clip;
                    }
                }
            }
        }
        ui.separator();

        heading(ui, "Layer");
        let mut op = (n.opacity * 100.0).round();
        ui.horizontal(|ui| {
            ui.label("Opacity");
            if ui.add(DragValue::new(&mut op).range(0.0..=100.0).suffix("%").speed(0.5)).changed() {
                self.begin_edit();
                for &id in &self.sel {
                    self.doc.node_mut(id).opacity = op / 100.0;
                }
            }
        });

        if self.sel.iter().all(|&id| self.doc.node(id).kind == Kind::Text) {
            ui.separator();
            heading(ui, "Text");
            let (mut size, mut weight) = (n.font_size, n.font_weight);
            let mut changed = false;
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("weight")
                    .width(110.0)
                    .selected_text(weight_name(weight))
                    .show_ui(ui, |ui| {
                        for w in [400, 500, 600, 700] {
                            changed |= ui.selectable_value(&mut weight, w, weight_name(w)).changed();
                        }
                    });
                changed |= ui.add(DragValue::new(&mut size).range(1.0..=1000.0).speed(0.25).custom_formatter(|v, _| num(v as f32))).changed();
            });
            if changed {
                self.begin_edit();
                for &id in &self.sel {
                    let t = self.doc.node_mut(id);
                    t.font_size = size;
                    t.font_weight = weight;
                }
            }
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

fn weight_name(w: u16) -> &'static str {
    match w {
        500 => "Medium",
        600 => "Semi Bold",
        700 => "Bold",
        _ => "Regular",
    }
}
