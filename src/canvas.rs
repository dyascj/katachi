//! Infinite canvas: navigation, tools, selection, snapping, and on-canvas overlays.

use crate::app::{App, Drag, GradGrab, Grip, Press, Tool};
use crate::doc::{Axis, Guide, Kind, LayoutMode, Node, NodeId, Order, PathPoint, Sizing, rotate};
use crate::render::{self, View};
use egui::{
    Color32, CornerRadius, CursorIcon, FontId, Key, Painter, PointerButton, Pos2, Rect, Sense, Stroke, StrokeKind,
    Ui, Vec2, pos2, vec2,
};

const TEXT_EDIT_ID: &str = "katachi-text-edit";

pub const BLUE: Color32 = Color32::from_rgb(0x0D, 0x99, 0xFF);
pub const PURPLE: Color32 = Color32::from_rgb(0x97, 0x47, 0xFF);
const SNAP: Color32 = Color32::from_rgb(0xF2, 0x48, 0x22);
/// Ruler thickness, in screen points.
const RULER: f32 = 20.0;
/// Figma's pink for spacing: equal-gap markers and smart selection handles.
const GAP: Color32 = Color32::from_rgb(0xFF, 0x24, 0xBD);

pub fn num(v: f32) -> String {
    let r = (v * 100.0).round() / 100.0;
    if r.fract() == 0.0 { format!("{}", r as i64) } else { format!("{r}") }
}

impl App {
    pub fn canvas_panel(&mut self, ui: &mut Ui) {
        let bg = crate::appearance::page_bg(ui.ctx(), self.doc.pages[self.page].background);
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(bg)).show(ui, |ui| self.canvas(ui));
    }

    fn canvas(&mut self, ui: &mut Ui) {
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = resp.rect;
        self.canvas_rect = Some(rect);
        let painter = painter.with_clip_rect(rect);
        if !self.view_ready {
            self.view_ready = true;
            let kids = self.doc.pages[self.page].children.clone();
            self.view = match self.doc.pages[self.page].view {
                // Back where this page was last viewed.
                Some([x, y, zoom]) => View { zoom, pan: rect.center().to_vec2() - vec2(x, y) * zoom },
                None if kids.is_empty() => View { zoom: 1.0, pan: rect.center().to_vec2() },
                None => View::fit(self.doc.bounds(&kids), rect, 1.0),
            };
        }
        if let Some(t) = self.zoom_target.take().filter(|t| self.doc.nodes.contains_key(t)) {
            self.view = View::fit(self.doc.node(t).aabb(), rect, 1.0);
        }
        if let Some(ve) = &self.vector_edit
            && (self.sel != [ve.id] || !self.doc.nodes.contains_key(&ve.id) || self.tool != Tool::Move) {
                self.vector_edit = None;
            }
        let prof = std::env::var_os("KATACHI_PROFILE").is_some();
        let t0 = std::time::Instant::now();
        self.fit_texts(&painter);
        let t_text = t0.elapsed();

        let typing = ui.ctx().egui_wants_keyboard_input();
        let (scroll, zoom, hover_pos, mods, space, mid, primary_pressed, secondary_pressed) = ui.input(|i| {
            (
                i.smooth_scroll_delta,
                i.zoom_delta(),
                i.pointer.hover_pos(),
                i.modifiers,
                i.key_down(Key::Space),
                i.pointer.middle_down(),
                i.pointer.primary_pressed(),
                i.pointer.secondary_pressed(),
            )
        });
        let over = resp.hovered() || resp.dragged();
        if let Some(hp) = hover_pos.filter(|_| over) {
            if zoom != 1.0 {
                self.view.zoom_at(hp, zoom);
            }
            self.view.pan += scroll;
        }

        let mut scene = render::Scene { doc: &self.doc, view: self.view, hide: self.editing_text, outline: self.outline_mode, tex: &mut self.textures };
        let t1 = std::time::Instant::now();
        render::draw_page(&painter, &mut scene, self.page);
        self.textures.end_frame();
        if prof {
            eprintln!("  fit_texts {t_text:?} draw {:?}", t1.elapsed());
        }
        self.draw_pixel_grid(&painter, rect);
        self.draw_layout_grids(&painter);
        self.layout_frame_labels(&painter);

        self.accept_drops(ui, hover_pos.filter(|p| rect.contains(*p)).unwrap_or(rect.center()));
        let ptr = resp.interact_pointer_pos().or(hover_pos);
        let world = ptr.map(|p| self.view.to_world(p));
        self.pointer_world = hover_pos.filter(|p| rect.contains(*p)).map(|p| self.view.to_world(p));
        let panning = self.tool == Tool::Hand || (space && !typing) || mid || matches!(self.drag, Some(Drag::Pan));

        // Hover target for the move tool.
        self.hover = None;
        if self.drag.is_none() && self.tool == Tool::Move && resp.hovered() && !panning
            && let (Some(s), Some(w)) = (ptr, world)
                && self.handle_at(s).is_none() && self.guide_at(s).is_none() {
                    self.hover = self.target_at(w, s, mods.command);
                }

        if resp.hovered() && (primary_pressed || secondary_pressed) {
            self.end_text_edit();
        }
        if primary_pressed && resp.hovered() && !panning
            && let (Some(s), Some(w)) = (ptr, world) {
                self.on_press(s, w, mods);
            }
        if secondary_pressed && resp.hovered()
            && let (Some(s), Some(w)) = (ptr, world) {
                self.menu_at = Some(w);
                match self.target_at(w, s, mods.command) {
                    Some(t) if !self.sel.contains(&t) => self.sel = vec![t],
                    None => self.sel.clear(),
                    _ => {}
                }
            }

        // Drags.
        if panning && (resp.dragged_by(PointerButton::Primary) || resp.dragged_by(PointerButton::Middle)) {
            self.view.pan += resp.drag_delta();
            self.drag.get_or_insert(Drag::Pan);
        } else if resp.drag_started_by(PointerButton::Primary) {
            self.on_drag_start(mods);
        }
        if let (Some(w), true) = (world, resp.dragged_by(PointerButton::Primary)) {
            self.on_drag(w, mods);
        }
        if resp.drag_stopped()
            && let Some(w) = world {
                self.on_drag_end(w);
            }
        // The click that closed a pen path ends there (it could otherwise count as a
        // double-click and select the frame under it).
        let swallow = self.pen_closed && (resp.clicked() || resp.drag_stopped());
        if swallow {
            self.pen_closed = false;
        }
        if resp.clicked() && !swallow
            && let Some(w) = world {
                self.on_click(w, mods);
            }
        if resp.double_clicked() && !swallow && self.tool == Tool::Move
            && let Some(w) = world {
                self.on_double_click(w);
            }
        if let (Some(drop), Some(p)) = (resp.dnd_release_payload::<crate::panels::AssetDrag>(), hover_pos) {
            // Instance dropped from the Assets tab, centered under the pointer.
            if self.doc.nodes.contains_key(&drop.0) {
                self.checkpoint();
                let w = self.view.to_world(p);
                let size = self.doc.node(drop.0).rect.size();
                let mut parent = self.frame_at(w, &[]);
                // Never into an instance (its layers come from its main) or the component itself.
                while let Some(p) = parent.filter(|&p| self.owning_instance(p).is_some() || p == drop.0 || self.doc.is_ancestor(drop.0, p)) {
                    parent = self.doc.node(p).parent;
                }
                let i = self.doc.instantiate(drop.0, self.page, parent, None, self.px(w - size / 2.0));
                self.sel = vec![i];
            }
        }
        crate::chrome::context_menu(&resp, |ui| self.context_menu(ui));

        self.text_editor(ui, &painter);
        self.draw_overlays(&painter, ptr);
        // Gradient handles edit what the open paint popover shows, so pressing one keeps it open.
        let on_gradient = matches!(self.drag, Some(Drag::Gradient { .. })) || hover_pos.filter(|_| resp.hovered()).and_then(|p| self.gradient_hit(p)).is_some();
        crate::chrome::pin_popovers(ui.ctx(), on_gradient || self.eyedrop.is_some());
        if let Some(g) = self.gradient_edit {
            ui.data_mut(|d| d.insert_temp(g.ui.with("stop"), g.stop));
        }
        // Dev Mode shows distances on hover; design mode needs Alt.
        if (mods.alt || self.dev_mode) && self.drag.is_none() {
            self.draw_measurements(&painter);
        }
        if self.dev_mode {
            if let Some(h) = self.hover.filter(|h| self.doc.nodes.contains_key(h)) {
                self.draw_dev_spacing(&painter, h);
            }
            self.draw_annotations(&painter);
        }
        self.draw_guides(&painter, rect);
        if self.rulers {
            self.draw_rulers(&painter, rect);
        }
        self.draw_peers(&painter);
        self.comment_hits = self.draw_comment_pins(&painter);
        self.comment_popups(ui.ctx());
        self.set_cursor(ui, ptr, panning);
        self.toolbar(ui.ctx(), rect);
        self.draw_toast(&painter, rect);
    }

    /// Files dropped on the canvas: images become image-filled rectangles, SVGs become vectors.
    /// An image dropped on a layer with an image fill (or on any layer with Alt) replaces its image.
    fn accept_drops(&mut self, ui: &Ui, at: Pos2) {
        let (files, alt) = ui.input(|i| (i.raw.dropped_files.clone(), i.modifiers.alt));
        let mut placed = vec![];
        let mut offset = 0.0;
        for f in files {
            let bytes = match (&f.bytes, &f.path) {
                (Some(b), _) => b.to_vec(),
                (None, Some(p)) => match std::fs::read(p) {
                    Ok(b) => b,
                    Err(_) => continue,
                },
                _ => continue,
            };
            let stem = f.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned());
            let name = stem.unwrap_or_else(|| f.name.clone());
            let w = self.view.to_world(at) + vec2(offset, offset);
            // Design files open in their own tab.
            let is_fig = [".fig", ".katachi"].iter().any(|x| f.name.to_lowercase().ends_with(x))
                || f.path.as_ref().is_some_and(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("fig") || e.eq_ignore_ascii_case("katachi")));
            if is_fig {
                if let Some(p) = &f.path {
                    self.want = Some(crate::app::Want::Open(p.clone()));
                }
                return;
            }
            let is_svg = f.name.to_lowercase().ends_with(".svg") || f.path.as_ref().is_some_and(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("svg")));
            let onto = self.doc.hit(self.page, w, 0.0).filter(|&t| alt || self.doc.node(t).fills.iter().any(|p| p.kind == crate::doc::PaintKind::Image));
            let id = match onto {
                _ if is_svg => self.import_svg(&bytes, &name, w, true),
                Some(t) => self.replace_fill_image(t, bytes),
                None => self.place_image(bytes, &name, w),
            };
            placed.extend(id);
            offset += 20.0;
        }
        if !placed.is_empty() {
            self.sel = placed;
        }
    }

    /// Text boxes hug their content per their sizing mode; fonts load on demand.
    fn fit_texts(&mut self, painter: &Painter) {
        self.fit_texts_on(painter, self.page);
    }

    /// Measure text on every page (and settle the layouts that depend on it), e.g. before export
    /// or after an agent edits: the canvas only measures the page on screen.
    pub fn fit_all_texts(&mut self, ctx: &egui::Context) {
        let painter = Painter::new(ctx.clone(), egui::LayerId::background(), Rect::EVERYTHING);
        for page in 0..self.doc.pages.len() {
            // Auto-height text inside filling layouts settles over a couple of rounds.
            for _ in 0..3 {
                if !self.fit_texts_on(&painter, page) {
                    break;
                }
            }
        }
    }

    /// Measure `page`'s text layers whose inputs changed since last time. Returns whether any resized.
    fn fit_texts_on(&mut self, painter: &Painter, page: usize) -> bool {
        let mut ids = vec![];
        let mut stack: Vec<NodeId> = self.doc.pages[page].children.clone();
        while let Some(id) = stack.pop() {
            let n = self.doc.node(id);
            if n.kind == Kind::Text {
                ids.push(id);
            }
            stack.extend(n.children.iter().copied());
        }
        let ctx = painter.ctx().clone();
        let mut changed = false;
        for id in ids {
            let n = self.doc.node(id);
            crate::text::ensure_font(&ctx, &n.font_family, n.font_weight, n.italic);
            for sp in &n.spans {
                let fam = sp.style.font_family.as_deref().unwrap_or(&n.font_family);
                crate::text::ensure_font(&ctx, fam, sp.style.font_weight.unwrap_or(n.font_weight), sp.style.italic.unwrap_or(n.italic));
            }
            let key = text_key(&ctx, n);
            if self.text_keys.get(&id) == Some(&key) {
                continue;
            }
            let (_, size, lines) = crate::text::layout(painter, n, 1.0, Color32::BLACK);
            let n = self.doc.node_mut(id);
            n.lines = lines;
            let want = match n.text_sizing {
                crate::doc::TextSizing::AutoWidth => size,
                crate::doc::TextSizing::AutoHeight => vec2(n.rect.width(), size.y),
                crate::doc::TextSizing::Fixed => n.rect.size(),
            };
            // In a session, ignore sub-pixel differences between machines so peers don't ping-pong.
            let tol = if self.collab.is_some() { 1.0 } else { 0.01 };
            if (n.rect.size() - want).length() > tol {
                n.rect = Rect::from_min_size(n.rect.min, want);
                changed = true;
            }
            let key = text_key(&ctx, self.doc.node(id));
            self.text_keys.insert(id, key);
        }
        if changed {
            self.doc.reflow();
        }
        changed
    }

    fn layout_frame_labels(&mut self, painter: &Painter) {
        self.frame_labels.clear();
        for &id in &self.doc.pages[self.page].children {
            let n = self.doc.node(id);
            if !n.kind.is_frame() || !n.visible {
                continue;
            }
            let active = self.sel.contains(&id) || self.hover == Some(id);
            let comp = matches!(n.kind, Kind::Component | Kind::Instance | Kind::ComponentSet);
            let color = match (comp, active) {
                (true, _) => PURPLE,
                (false, true) => BLUE,
                _ => crate::appearance::muted(painter.ctx()),
            };
            let r = self.view.rect_to_screen(n.aabb());
            if n.kind == Kind::Section {
                // Sections carry their name in a pill inside the top-left corner.
                let g = painter.layout_no_wrap(n.name.clone(), FontId::proportional(11.0), if active { Color32::WHITE } else { crate::appearance::ink(painter.ctx()) });
                let pill = Rect::from_min_size(r.min + vec2(8.0, 8.0), g.size() + vec2(12.0, 6.0));
                painter.rect_filled(pill, 4.0, if active { BLUE } else { crate::appearance::subtle(painter.ctx()).to_opaque() });
                painter.galley(pill.min + vec2(6.0, 3.0), g, Color32::WHITE);
                self.frame_labels.push((id, pill));
                continue;
            }
            let galley = painter.layout_no_wrap(n.name.clone(), FontId::proportional(11.0), color);
            let indent = if comp { 14.0 } else { 0.0 };
            let pos = pos2(r.min.x + indent, r.min.y - galley.size().y - 4.0);
            if comp {
                let icon = if n.kind == Kind::Component { crate::panels::Icon::Component } else { crate::panels::Icon::Instance };
                let ir = Rect::from_center_size(pos2(r.min.x + 5.0, pos.y + galley.size().y / 2.0), vec2(12.0, 12.0));
                crate::panels::paint_icon(painter, ir, icon, PURPLE);
            }
            self.frame_labels.push((id, Rect::from_min_max(pos2(r.min.x, pos.y), pos + galley.size())));
            painter.galley(pos, galley, color);
        }
    }

    /// Node that a click at world `w` / screen `s` would select.
    fn target_at(&self, w: Pos2, s: Pos2, deep: bool) -> Option<NodeId> {
        if let Some((id, _)) = self.frame_labels.iter().find(|(id, r)| r.contains(s) && !self.doc.node(*id).locked) {
            return Some(*id);
        }
        self.doc.hit(self.page, w, 3.0 / self.view.zoom).map(|h| self.selectable(h, deep))
    }

    /// Unrotated rect and rotation of the selection: a lone layer keeps its own frame.
    pub fn sel_frame(&self) -> Option<(Rect, f32)> {
        match self.sel_roots().as_slice() {
            [] => None,
            [one] => {
                let n = self.doc.node(*one);
                Some((n.rect, n.rotation))
            }
            many => Some((self.doc.bounds(many), 0.0)),
        }
    }

    fn handle_at(&self, s: Pos2) -> Option<Grip> {
        if self.editing_text.is_some() || !self.tool.selects() || self.vector_edit.is_some() {
            return None;
        }
        let (r, rot) = self.sel_frame()?;
        // Work in the selection's own (unrotated) screen frame.
        let s = self.view.to_screen(rotate(self.view.to_world(s), r.center(), -rot));
        let b = self.view.rect_to_screen(r);
        let near = |a: f32, b: f32| (a - b).abs() <= 5.0;
        let side = |v: f32, lo: f32, hi: f32| -> i8 {
            if near(v, lo) {
                -1
            } else if near(v, hi) {
                1
            } else {
                0
            }
        };
        let (hx, hy) = (side(s.x, b.min.x, b.max.x), side(s.y, b.min.y, b.max.y));
        let in_x = s.x > b.min.x - 5.0 && s.x < b.max.x + 5.0;
        let in_y = s.y > b.min.y - 5.0 && s.y < b.max.y + 5.0;
        match (hx, hy) {
            (x, 0) if x != 0 && in_y => return Some(Grip::Resize(x, 0)),
            (0, y) if y != 0 && in_x => return Some(Grip::Resize(0, y)),
            (x, y) if x != 0 && y != 0 => return Some(Grip::Resize(x, y)),
            _ => {}
        }
        if let Some(g) = self.smart_grip_at(s).or_else(|| self.layout_grip_at(s)) {
            return Some(g);
        }
        let corner = [b.left_top(), b.right_top(), b.left_bottom(), b.right_bottom()]
            .iter()
            .any(|c| c.distance(s) < 20.0);
        (corner && !b.expand(5.0).contains(s)).then_some(Grip::Rotate)
    }

    /// The gradient whose handles are on the canvas: its layer and paint.
    fn edited_gradient(&self) -> Option<(NodeId, crate::doc::Paint)> {
        let g = self.gradient_edit?;
        let [id] = self.sel[..] else { return None };
        let n = self.doc.node(id);
        let p = if g.stroke { n.strokes.get(g.index) } else { n.fills.get(g.index) }?;
        p.kind.is_gradient().then(|| (id, p.clone()))
    }

    fn set_edited_gradient(&mut self, id: NodeId, p: crate::doc::Paint) {
        let Some(g) = self.gradient_edit else { return };
        let n = self.doc.node_mut(id);
        let paints = if g.stroke { &mut n.strokes } else { &mut n.fills };
        if let Some(slot) = paints.get_mut(g.index) {
            *slot = p;
        }
    }

    /// Screen positions of a gradient's start, end, and width handles (paints live in the
    /// layer's unrotated unit square).
    fn gradient_handles(&self, id: NodeId, p: &crate::doc::Paint) -> [Pos2; 3] {
        let n = self.doc.node(id);
        [p.from, p.to, p.width_point()].map(|q| self.view.to_screen(rotate(n.rect.min + q.to_vec2() * n.rect.size(), n.rect.center(), n.rotation)))
    }

    fn gradient_hit(&self, s: Pos2) -> Option<GradGrab> {
        if self.vector_edit.is_some() || self.editing_text.is_some() {
            return None;
        }
        let (id, p) = self.edited_gradient()?;
        let [a, b, w] = self.gradient_handles(id, &p);
        let near = |q: Pos2| q.distance(s) <= 7.0;
        if p.kind != crate::doc::PaintKind::Linear && near(w) {
            return Some(GradGrab::Width);
        }
        if near(a) {
            return Some(GradGrab::From);
        }
        if near(b) {
            return Some(GradGrab::To);
        }
        if let Some(i) = p.stops.iter().position(|(t, _)| near(a.lerp(b, *t))) {
            return Some(GradGrab::Stop(i));
        }
        let t = line_t(a, b, s);
        (a.lerp(b, t).distance(s) <= 4.0).then_some(GradGrab::Line)
    }

    /// Add a stop where world point `w` falls on the gradient line, colored as the gradient is there.
    fn add_gradient_stop(&mut self, w: Pos2) -> Option<usize> {
        let (id, mut p) = self.edited_gradient()?;
        let [a, b, _] = self.gradient_handles(id, &p);
        let t = line_t(a, b, self.view.to_screen(w));
        let i = p.stops.partition_point(|s| s.0 <= t);
        p.stops.insert(i, (t, p.sample(t)));
        self.set_edited_gradient(id, p);
        self.gradient_edit.as_mut()?.stop = i;
        Some(i)
    }

    /// A click on the gradient editor: the line adds a stop, a stop or end selects it.
    fn gradient_click(&mut self, g: GradGrab, w: Pos2) {
        let Some((_, p)) = self.edited_gradient() else { return };
        let stop = match g {
            GradGrab::Line => {
                self.checkpoint();
                self.add_gradient_stop(w)
            }
            GradGrab::Stop(i) => Some(i),
            GradGrab::From => Some(0),
            GradGrab::To => Some(p.stops.len().saturating_sub(1)),
            GradGrab::Width => None,
        };
        if let (Some(i), Some(e)) = (stop, self.gradient_edit.as_mut()) {
            e.stop = i;
        }
    }

    /// Move a gradient handle or stop to world point `w`.
    fn drag_gradient(&mut self, grab: &mut GradGrab, w: Pos2) {
        let Some((id, mut p)) = self.edited_gradient() else { return };
        let n = self.doc.node(id);
        let l = rotate(w, n.rect.center(), -n.rotation);
        let q = pos2((l.x - n.rect.min.x) / n.rect.width().max(1e-6), (l.y - n.rect.min.y) / n.rect.height().max(1e-6));
        let linear = p.kind == crate::doc::PaintKind::Linear;
        match *grab {
            // Radial, angular, and diamond gradients move as a whole by their center.
            GradGrab::From if !linear => {
                let d = q - p.from;
                p.to += d;
                if let Some(wt) = &mut p.width_to {
                    *wt += d;
                }
                p.from = q;
            }
            GradGrab::From => p.from = q,
            GradGrab::To => {
                // The width axis turns and scales with the main one (complex multiplication by new / old).
                if let Some(wt) = p.width_to {
                    let (od, nd, ow) = (p.to - p.from, q - p.from, wt - p.from);
                    let r = vec2(nd.x * od.x + nd.y * od.y, nd.y * od.x - nd.x * od.y) / od.length_sq().max(1e-12);
                    p.width_to = Some(p.from + vec2(ow.x * r.x - ow.y * r.y, ow.x * r.y + ow.y * r.x));
                }
                p.to = q;
            }
            GradGrab::Width => p.width_to = Some(q),
            GradGrab::Stop(i) if i < p.stops.len() => {
                let [a, b, _] = self.gradient_handles(id, &p);
                let mut s = p.stops.remove(i);
                s.0 = line_t(a, b, self.view.to_screen(w));
                let j = p.stops.partition_point(|x| x.0 <= s.0);
                p.stops.insert(j, s);
                *grab = GradGrab::Stop(j);
                if let Some(e) = self.gradient_edit.as_mut() {
                    e.stop = j;
                }
            }
            _ => return,
        }
        self.set_edited_gradient(id, p);
    }

    /// Figma's gradient editor on the canvas: the line from start to end with its stops, and the handles.
    fn draw_gradient_handles(&self, painter: &Painter) {
        let Some((id, p)) = self.edited_gradient() else { return };
        let [a, b, w] = self.gradient_handles(id, &p);
        let shadow = Stroke::new(3.0, Color32::from_black_alpha(70));
        let line = Stroke::new(1.5, Color32::WHITE);
        let knob = |c: Pos2, r: f32| {
            painter.circle(c, r, Color32::WHITE, Stroke::new(1.0, Color32::from_black_alpha(90)));
        };
        if p.kind != crate::doc::PaintKind::Linear {
            painter.line_segment([a, w], shadow);
            painter.extend(egui::Shape::dashed_line(&[a, w], line, 4.0, 3.0));
            knob(w, 4.0);
        }
        painter.line_segment([a, b], shadow);
        painter.line_segment([a, b], line);
        knob(a, 5.0);
        knob(b, 5.0);
        let sel = self.gradient_edit.map_or(0, |g| g.stop);
        for (i, (t, c)) in p.stops.iter().enumerate() {
            let r = Rect::from_center_size(a.lerp(b, *t), vec2(12.0, 12.0));
            painter.rect_filled(r.expand(1.0), 3.0, Color32::from_black_alpha(70));
            painter.rect_filled(r, 3.0, Color32::WHITE);
            painter.rect_filled(r.shrink(2.0), 2.0, c.to32());
            if i == sel {
                painter.rect_stroke(r.expand(1.5), 4.0, Stroke::new(1.5, BLUE), StrokeKind::Outside);
            }
        }
    }

    fn on_press(&mut self, s: Pos2, w: Pos2, mods: egui::Modifiers) {
        let tool = self.tool;
        // Clicking a comment pin opens its thread (whenever pins are showing).
        if self.comments_visible() {
            if let Some((cid, _)) = self.comment_hits.iter().find(|(_, r)| r.contains(s)) {
                self.open_comment = Some(*cid);
                self.draft_comment = None;
                return;
            }
            if tool == Tool::Comment {
                self.start_comment(w);
                return;
            }
        }
        // Rulers pull out new guides; the move tool grabs existing ones.
        let ruler = self.ruler_at(s);
        let guide = if ruler.is_none() { self.guide_at(s) } else { None };
        self.guide_sel = guide;
        if let Some(axis) = ruler.or(guide.map(|i| self.doc.pages[self.page].guides[i].axis)) {
            if guide.is_some() {
                self.sel.clear();
            }
            self.guide_grab = Some((axis, guide));
            self.press = Some(Press { world: w, target: None, handle: None, toggle_off: false });
            return;
        }
        if tool == Tool::Move && self.vector_edit.is_some() && self.vector_press(s, mods) {
            self.press = Some(Press { world: w, target: None, handle: None, toggle_off: false });
            return;
        }
        if tool.selects() && self.prototype_tab
            && let Some(from) = self.connect_handle_at(s) {
                self.connecting = Some(from);
                self.press = Some(Press { world: w, target: None, handle: None, toggle_off: false });
                return;
            }
        match tool {
            Tool::Move | Tool::Scale => {
                self.vector_edit = None;
                if let Some(g) = self.gradient_hit(s) {
                    self.press = Some(Press { world: w, target: None, handle: Some(Grip::Gradient(g)), toggle_off: false });
                    return;
                }
                let handle = self.handle_at(s);
                let target = if handle.is_some() { None } else { self.target_at(w, s, mods.command) };
                let mut toggle_off = false;
                let mut defer = false;
                if let Some(t) = target {
                    let n = self.doc.node(t);
                    // An unselected top-level frame with content: click selects it, drag marquees.
                    defer = n.kind.is_frame()
                        && n.parent.is_none()
                        && !n.children.is_empty()
                        && !self.sel.contains(&t)
                        && !self.frame_labels.iter().any(|(id, r)| *id == t && r.contains(s));
                    if mods.shift {
                        if self.sel.contains(&t) {
                            toggle_off = true;
                        } else if !defer {
                            self.sel.push(t);
                        }
                    } else if !self.sel.contains(&t) && !defer {
                        // Pressing inside a selected parent drags the parent; a plain click selects the child.
                        if !self.sel.iter().any(|&p| self.doc.is_ancestor(p, t)) {
                            self.sel = vec![t];
                        }
                    }
                } else if handle.is_none() && !mods.shift {
                    self.sel.clear();
                }
                self.press = Some(Press { world: w, target: target.filter(|_| !defer), handle, toggle_off });
                if defer {
                    self.pending_frame = target;
                }
            }
            Tool::Pen if self.pen.is_empty() && self.pen_continue(s) => {
                self.press = Some(Press { world: w, target: None, handle: None, toggle_off: false });
            }
            Tool::Pen => {
                let close = self.pen.len() >= 2 && self.view.to_screen(self.pen[0].p).distance(s) < 8.0;
                if close {
                    self.finish_pen(true);
                    self.pen_closed = true;
                } else {
                    let mut p = w;
                    if let (true, Some(last)) = (mods.shift, self.pen.last()) {
                        p = snap_angle(last.p, p);
                    }
                    self.pen.push(PathPoint { p: self.px(p), ..Default::default() });
                }
            }
            _ => self.press = Some(Press { world: w, target: None, handle: None, toggle_off: false }),
        }
    }

    fn on_drag_start(&mut self, mods: egui::Modifiers) {
        let Some(press) = self.press.take() else {
            if self.tool == Tool::Pen && !self.pen.is_empty() {
                self.drag = Some(Drag::PenHandle);
            }
            return;
        };
        self.pending_frame = None;
        if let Some((axis, index)) = self.guide_grab.take() {
            self.checkpoint();
            let guides = &mut self.doc.pages[self.page].guides;
            let index = index.unwrap_or_else(|| {
                guides.push(Guide { axis, offset: 0.0 });
                guides.len() - 1
            });
            self.guide_sel = Some(index);
            self.drag = Some(Drag::Guide { index });
            return;
        }
        let start = press.world;
        if let Some(ve) = &self.vector_edit {
            self.drag = match self.vector_grabbed {
                None if ve.tool == crate::vector_edit::VTool::Move => {
                    self.vector_outside = false;
                    Some(Drag::PointMarquee { start, base_sel: ve.selected.clone() })
                }
                _ => self.vector_drag_start(),
            };
            self.vector_start = start;
            return;
        }
        if let Some(from) = self.connecting.take() {
            self.drag = Some(Drag::Connect { from });
            return;
        }
        self.drag = match self.tool {
            Tool::Move | Tool::Scale => {
                if let Some(Grip::Gradient(g)) = press.handle {
                    // One undo step per drag; dragging off the line pulls out a new stop.
                    self.checkpoint();
                    let grab = if g == GradGrab::Line { self.add_gradient_stop(start).map_or(g, GradGrab::Stop) } else { g };
                    Some(Drag::Gradient { grab })
                } else if let Some(grip) = press.handle {
                    self.checkpoint();
                    let (bounds, rot) = self.sel_frame().unwrap_or((Rect::NOTHING, 0.0));
                    let base = self.doc.clone();
                    match grip {
                        Grip::Resize(x, y) => Some(Drag::Resize { base, handle: (x, y), bounds, rot }),
                        Grip::Rotate => {
                            let c = bounds.center();
                            let a = (start - c).angle();
                            Some(Drag::Rotate { base, center: c, start_angle: a, start_rot: rot, deg: 0.0 })
                        }
                        Grip::Gap | Grip::Padding(_) => Some(Drag::LayoutSpacing { base, id: self.sel[0], grip, start }),
                        Grip::Spacing(_) | Grip::Swap(_) => self.smart_selection().map(|(axis, items)| match grip {
                            Grip::Swap(id) => Drag::Swap { base, id, axis, items, start },
                            _ => Drag::Spacing { base, axis, gap: gap_of(&items, axis), items, start },
                        }),
                        Grip::Gradient(_) => None,
                    }
                } else if press.target.is_some() && !self.sel.is_empty() {
                    self.checkpoint();
                    if mods.alt {
                        let roots = self.sel_roots();
                        self.sel = self.duplicate_or_instance(&roots);
                    }
                    Some(Drag::Move { base: self.doc.clone(), start })
                } else {
                    let base_sel = if mods.shift { self.sel.clone() } else { vec![] };
                    Some(Drag::Marquee { start, base_sel })
                }
            }
            Tool::Pencil => Some(Drag::Pencil { points: vec![start] }),
            Tool::Zoom => Some(Drag::Zoom { start }),
            t if t.draws_shape() || t == Tool::Text => {
                self.checkpoint();
                let start = self.snap_point(self.px(start), &[]);
                let parent = if t == Tool::Section { None } else { self.frame_at(start, &[]) };
                let mut node = self.shape_for(t, Rect::from_min_size(start, Vec2::ZERO));
                if t == Tool::Text {
                    node.text_sizing = crate::doc::TextSizing::Fixed;
                }
                if node.kind.is_frame() && parent.is_some() {
                    node.clip = true;
                }
                let id = self.doc.insert(self.page, parent, None, node);
                self.sel = vec![id];
                Some(Drag::Create { base: self.doc.clone(), start, id })
            }
            _ => None,
        };
    }

    fn on_drag(&mut self, w: Pos2, mods: egui::Modifiers) {
        let Some(mut drag) = self.drag.take() else { return };
        match &mut drag {
            Drag::Move { base, start, .. } => {
                self.doc = base.clone();
                let roots = self.sel_roots();
                if let Some(p) = self.reorder_parent(&roots, w) {
                    // Live reorder inside auto layout: siblings slide, the dragged layer follows the cursor.
                    let id = roots[0];
                    let from = self.doc.node(id).aabb().min;
                    let i = self.al_index(p, id, w);
                    self.doc.reparent(id, self.page, Some(p), Some(i));
                    self.doc.reflow();
                    let now = self.doc.node(id).aabb().min;
                    self.doc.translate(id, from + (w - *start) - now);
                    self.guides.clear();
                    self.gap_marks.clear();
                    self.drag = Some(drag);
                    return;
                }
                let b = self.doc.bounds(&roots);
                let mut d = w - *start;
                if mods.shift {
                    if d.x.abs() > d.y.abs() { d.y = 0.0 } else { d.x = 0.0 }
                }
                let corr = self.snap(b.translate(d), &roots, [true; 3], [true; 3]);
                d += corr;
                let d = self.px(b.min + d) - b.min;
                for id in roots {
                    self.doc.translate(id, d);
                }
                self.doc.reflow();
            }
            Drag::Rotate { base, center, start_angle, start_rot, deg } => {
                self.doc = base.clone();
                let mut d = -((w - *center).angle() - *start_angle).to_degrees();
                if mods.shift {
                    d = ((*start_rot + d) / 15.0).round() * 15.0 - *start_rot;
                }
                *deg = crate::doc::normalize_deg(*start_rot + d);
                for id in self.sel_roots() {
                    self.doc.rotate(id, d, *center);
                }
            }
            Drag::Resize { base, handle, bounds, rot } => {
                self.doc = base.clone();
                let roots = self.sel_roots();
                let (hx, hy) = *handle;
                let b = *bounds;
                let rot = *rot;
                // Rotated layers resize along their own axes.
                let w = rotate(w, b.center(), -rot);
                let mut nb = b;
                match hx {
                    -1 => nb.min.x = w.x,
                    1 => nb.max.x = w.x,
                    _ => {}
                }
                match hy {
                    -1 => nb.min.y = w.y,
                    1 => nb.max.y = w.y,
                    _ => {}
                }
                let corr = if rot == 0.0 {
                    self.snap(nb, &roots, [hx == -1, false, hx == 1], [hy == -1, false, hy == 1])
                } else {
                    Vec2::ZERO
                };
                if hx == -1 { nb.min.x += corr.x } else if hx == 1 { nb.max.x += corr.x }
                if hy == -1 { nb.min.y += corr.y } else if hy == 1 { nb.max.y += corr.y }
                if mods.shift && b.width() > 0.0 && b.height() > 0.0 {
                    let (sx, sy) = (nb.width() / b.width(), nb.height() / b.height());
                    let s = match (hx, hy) {
                        (0, _) => sy,
                        (_, 0) => sx,
                        _ => if sx.abs() > sy.abs() { sx } else { sy },
                    };
                    let size = b.size() * s.abs();
                    // Grow away from the fixed edge; edge handles stay centered on the other axis.
                    let fix_x = match hx { -1 => nb.max.x - size.x, 1 => nb.min.x, _ => b.center().x - size.x / 2.0 };
                    let fix_y = match hy { -1 => nb.max.y - size.y, 1 => nb.min.y, _ => b.center().y - size.y / 2.0 };
                    nb = Rect::from_min_size(pos2(fix_x, fix_y), size);
                }
                if mods.alt {
                    let c = b.center();
                    if hx == -1 { nb.max.x = 2.0 * c.x - nb.min.x }
                    if hx == 1 { nb.min.x = 2.0 * c.x - nb.max.x }
                    if hy == -1 { nb.max.y = 2.0 * c.y - nb.min.y }
                    if hy == 1 { nb.min.y = 2.0 * c.y - nb.max.y }
                }
                let nb = Rect::from_two_pos(self.px(nb.min), self.px(nb.max));
                if rot != 0.0 {
                    // Keep the fixed side planted: move the new center back into world space.
                    let c = rotate(nb.center(), b.center(), rot);
                    self.doc.set_rect_with(roots[0], Rect::from_center_size(c, nb.size()), !mods.command);
                    self.drag = Some(drag);
                    return;
                }
                let sx = if b.width() > 0.0 { nb.width() / b.width() } else { 1.0 };
                let sy = if b.height() > 0.0 { nb.height() / b.height() } else { 1.0 };
                let map = |p: Pos2| pos2(nb.min.x + (p.x - b.min.x) * sx, nb.min.y + (p.y - b.min.y) * sy);
                if self.tool == Tool::Scale {
                    // K: strokes, radii, effects, and type scale along with the box.
                    // A flat box (a line) scales by its one real axis.
                    let k = match (b.width() > 0.0, b.height() > 0.0) {
                        (true, true) => (sx.abs() * sy.abs()).sqrt(),
                        (true, false) => sx.abs(),
                        _ => sy.abs(),
                    }
                    .max(0.001);
                    for &id in &roots {
                        for d in std::iter::once(id).chain(self.doc.descendants(id)) {
                            scale_props(self.doc.node_mut(d), k);
                        }
                    }
                }
                for id in roots {
                    let r = self.doc.node(id).rect;
                    // Dragging a size by hand pins it, as in Figma.
                    let n = self.doc.node_mut(id);
                    if hx != 0 {
                        n.sizing[0] = Sizing::Fixed;
                    }
                    if hy != 0 {
                        n.sizing[1] = Sizing::Fixed;
                    }
                    // Text: dragging width wraps (auto height); dragging height fixes the box.
                    if n.kind == Kind::Text {
                        use crate::doc::TextSizing;
                        if hy != 0 {
                            n.text_sizing = TextSizing::Fixed;
                        } else if hx != 0 && n.text_sizing == TextSizing::AutoWidth {
                            n.text_sizing = TextSizing::AutoHeight;
                        }
                    }
                    self.doc.set_rect_with(id, Rect::from_two_pos(map(r.min), map(r.max)), !mods.command);
                }
                self.doc.reflow();
            }
            Drag::Create { base, start, id } => {
                self.doc = base.clone();
                let a = *start;
                let p = self.snap_point(w, &[*id]);
                let mut b = self.px(p);
                if self.doc.node(*id).kind == Kind::Vector {
                    if mods.shift {
                        b = snap_angle(a, b);
                    }
                    let n = self.doc.node_mut(*id);
                    n.rect = Rect::from_min_size(Pos2::ZERO, Vec2::ZERO);
                    n.paths = vec![crate::doc::SubPath {
                        points: vec![PathPoint { p: a, ..Default::default() }, PathPoint { p: b, ..Default::default() }],
                        closed: false,
                    }];
                    n.refit_vector();
                } else {
                    if mods.shift {
                        let d = b - a;
                        let s = d.x.abs().max(d.y.abs());
                        b = a + vec2(s.copysign(d.x), s.copysign(d.y));
                    }
                    let a = if mods.alt { a - (b - a) } else { a };
                    self.doc.set_rect(*id, Rect::from_two_pos(a, b));
                }
            }
            Drag::Marquee { start, base_sel } => {
                let r = Rect::from_two_pos(*start, w);
                let mut sel = base_sel.clone();
                for id in self.marquee_hits(r) {
                    if !sel.contains(&id) {
                        sel.push(id);
                    }
                }
                self.sel = sel;
            }
            Drag::PointMarquee { start, base_sel } => {
                let (r, base) = (Rect::from_two_pos(*start, w), base_sel.clone());
                self.vector_marquee(r, &base);
            }
            Drag::Vector { base, id, grab } => {
                let (base, id, grab) = (base.clone(), *id, *grab);
                self.vector_drag(&base, id, grab, self.vector_start, w, mods.alt);
            }
            Drag::Pencil { points } => {
                if points.last().is_none_or(|l| l.distance(w) * self.view.zoom > 1.5) {
                    points.push(w);
                }
            }
            Drag::PenHandle => {
                let z = self.view.zoom;
                if let Some(last) = self.pen.last_mut() {
                    let h = w - last.p;
                    if h.length() * z > 2.0 {
                        last.h_out = h;
                        last.h_in = -h;
                    }
                }
            }
            Drag::Guide { index } => {
                if let Some(axis) = self.doc.pages[self.page].guides.get(*index).map(|g| g.axis) {
                    self.doc.pages[self.page].guides[*index].offset = self.px1(if axis == Axis::X { w.x } else { w.y });
                }
            }
            Drag::Spacing { base, axis, items, gap, start } => {
                self.doc = base.clone();
                let g = self.px1((*gap + (w - *start)[*axis]).max(0.0));
                lay_out(&mut self.doc, *axis, items, items[0].1.min[*axis], g);
            }
            Drag::Swap { base, id, start, .. } => {
                self.doc = base.clone();
                let d = w - *start;
                self.doc.translate(*id, vec2(self.px1(d.x), self.px1(d.y)));
            }
            Drag::Gradient { grab } => self.drag_gradient(grab, w),
            Drag::LayoutSpacing { base, id, grip, start } => {
                self.doc = base.clone();
                let d = w - *start;
                let mut l = self.doc.node(*id).layout;
                let a = if l.mode == LayoutMode::Vertical { 1 } else { 0 };
                match *grip {
                    Grip::Gap => {
                        l.gap = self.px1((l.gap + d[a]).max(0.0));
                        l.space_between = false;
                    }
                    Grip::Padding(side) => {
                        // Dragging inward adds padding. Alt mirrors to the opposite side, Shift sets all four.
                        let inward = [d.y, -d.x, -d.y, d.x][side];
                        let v = self.px1((l.padding[side] + inward).max(0.0));
                        for s in 0..4 {
                            if s == side || mods.shift || (mods.alt && s == (side + 2) % 4) {
                                l.padding[s] = v;
                            }
                        }
                    }
                    _ => {}
                }
                self.doc.node_mut(*id).layout = l;
                self.doc.reflow();
            }
            Drag::Pan | Drag::Connect { .. } | Drag::Zoom { .. } => {}
        }
        self.drag = Some(drag);
    }

    fn on_drag_end(&mut self, w: Pos2) {
        self.guides.clear();
        self.gap_marks.clear();
        self.vector_grabbed = None;
        match self.drag.take() {
            Some(Drag::Connect { from }) => {
                // Dropping on a frame wires an On click → Navigate interaction to it.
                let top = crate::prototype::top_frame(&self.doc, from);
                let target = crate::prototype::frames_on_page(&self.doc, self.page)
                    .into_iter()
                    .rev()
                    .find(|f| *f != top && self.doc.node(*f).aabb().contains(w));
                if let Some(t) = target {
                    self.checkpoint();
                    self.doc.node_mut(from).reactions.push(crate::doc::Reaction { action: crate::doc::Action::Navigate(t), ..Default::default() });
                }
            }
            Some(Drag::Zoom { start }) => {
                let r = Rect::from_two_pos(start, w);
                if let Some(screen) = self.canvas_rect {
                    if r.width().max(r.height()) * self.view.zoom > 4.0 {
                        self.view = View::fit(r, screen, 256.0);
                    } else {
                        self.view.zoom_at(self.view.to_screen(w), 2.0);
                    }
                }
            }
            Some(Drag::Guide { index }) => {
                // Dropped back on its ruler (or off the canvas that way): gone.
                let (s, r) = (self.view.to_screen(w), self.canvas_rect.unwrap_or(Rect::EVERYTHING));
                let guides = &mut self.doc.pages[self.page].guides;
                let gone = guides.get(index).is_some_and(|g| match g.axis {
                    Axis::X => s.x < r.min.x + RULER,
                    Axis::Y => s.y < r.min.y + RULER,
                });
                if gone {
                    guides.remove(index);
                    self.guide_sel = None;
                }
            }
            Some(Drag::Swap { base, id, axis, mut items, .. }) => {
                // Dropped on another layer's slot: trade places. Anywhere else: snap back.
                self.doc = base;
                let i = items.iter().position(|(n, _)| *n == id);
                let j = items.iter().position(|(n, r)| *n != id && r.contains(w));
                if let (Some(i), Some(j)) = (i, j) {
                    let g = gap_of(&items, axis);
                    let first = items[0].1.min[axis];
                    items.swap(i, j);
                    lay_out(&mut self.doc, axis, &items, first, g);
                }
            }
            Some(Drag::Pencil { points }) => {
                let pts = smooth_stroke(&points, self.prefs.pencil_smoothing / self.view.zoom);
                if pts.len() >= 2 {
                    self.checkpoint();
                    let parent = self.frame_at(points[0], &[]);
                    let mut n = Node::vector(&pts, false);
                    n.stroke_width = 2.0;
                    n.cap = crate::doc::Cap::Round;
                    n.join = crate::doc::Join::Round;
                    let id = self.doc.insert(self.page, parent, None, n);
                    self.sel = vec![id];
                }
            }
            Some(Drag::Move { .. }) => {
                // Dropping onto a frame nests the layer in it; dragging out un-nests it.
                let roots = self.sel_roots();
                if self.reorder_parent(&roots, w).is_none() {
                    for id in roots {
                        let parent = self.doc.node(id).parent;
                        if parent.is_some_and(|p| self.doc.node(p).kind == Kind::Group) {
                            continue;
                        }
                        let target = self.frame_at(w, &[id]);
                        if target != parent {
                            let index = target
                                .filter(|t| self.doc.node(*t).layout.mode != LayoutMode::None)
                                .map(|t| self.al_index(t, id, w));
                            self.doc.reparent(id, self.page, target, index);
                        }
                    }
                }
                self.doc.reflow();
            }
            Some(Drag::Create { id, .. }) => {
                let n = self.doc.node(id).clone();
                if matches!(n.kind, Kind::Frame | Kind::Section) {
                    // A new frame drawn around sibling layers adopts them.
                    let (r, parent) = (n.rect, n.parent);
                    let kids: Vec<NodeId> = self
                        .doc
                        .children_of(self.page, parent)
                        .iter()
                        .copied()
                        .filter(|&c| c != id && r.contains_rect(self.doc.node(c).aabb()))
                        .collect();
                    for c in kids {
                        self.doc.reparent(c, self.page, Some(id), None);
                    }
                }
                if n.kind == Kind::Text {
                    self.editing_text = Some(id);
                    self.focus_text = true;
                }
                if n.kind != Kind::Vector && (n.rect.width() < 1.0 || n.rect.height() < 1.0) {
                    let r = Rect::from_min_size(n.rect.min, n.rect.size().max(Vec2::splat(1.0)));
                    self.doc.set_rect(id, r);
                }
                // Text goes straight to typing; shape tools stay on if "Keep tool selected" is set.
                if n.kind == Kind::Text || !self.prefs.keep_tool_selected {
                    self.tool = Tool::Move;
                }
            }
            _ => {}
        }
    }

    fn on_click(&mut self, w: Pos2, mods: egui::Modifiers) {
        if self.guide_grab.take().is_some() {
            self.press = None;
            return;
        }
        if self.vector_edit.is_some() {
            self.press = None;
            if std::mem::take(&mut self.vector_outside) {
                // A click on empty canvas outside the path leaves edit mode and selects as usual.
                self.vector_edit = None;
                self.on_press(self.view.to_screen(w), w, mods);
                self.on_click(w, mods);
            }
            return;
        }
        match self.tool {
            Tool::Move | Tool::Scale => {
                if let Some(t) = self.pending_frame.take() {
                    if mods.shift { self.sel.push(t) } else { self.sel = vec![t] }
                }
                if let Some(Press { handle: Some(Grip::Gradient(g)), .. }) = self.press {
                    self.press = None;
                    return self.gradient_click(g, w);
                }
                if let Some(p) = self.press.take() {
                    match p.target {
                        Some(t) if p.toggle_off => self.sel.retain(|s| *s != t),
                        Some(t) if !mods.shift => self.sel = vec![t],
                        _ => {}
                    }
                }
            }
            Tool::Zoom => {
                let at = self.view.to_screen(w);
                self.view.zoom_at(at, if mods.alt { 0.5 } else { 2.0 });
            }
            Tool::Text => {
                // Created on release so the new field keeps keyboard focus.
                self.checkpoint();
                let parent = self.frame_at(w, &[]);
                let n = Node::new(Kind::Text, Rect::from_min_size(self.px(w), Vec2::ZERO));
                let id = self.doc.insert(self.page, parent, None, n);
                self.sel = vec![id];
                self.editing_text = Some(id);
                self.focus_text = true;
                self.tool = Tool::Move;
            }
            t if t.draws_shape() => {
                // A click without a drag drops a default 100×100 shape.
                self.checkpoint();
                let p = self.px(w);
                let parent = if t == Tool::Section { None } else { self.frame_at(p, &[]) };
                let node = self.shape_for(t, Rect::from_min_size(p, vec2(100.0, 100.0)));
                let id = self.doc.insert(self.page, parent, None, node);
                self.sel = vec![id];
                if !self.prefs.keep_tool_selected {
                    self.tool = Tool::Move;
                }
                self.press = None;
            }
            _ => {}
        }
    }

    /// The layer a drawing tool makes, spanning `r` (lines run from its top-left to bottom-right).
    fn shape_for(&self, t: Tool, r: Rect) -> Node {
        match t {
            Tool::Frame => Node::new(Kind::Frame, r),
            Tool::Section => Node::new(Kind::Section, r),
            Tool::Slice => Node::new(Kind::Slice, r),
            Tool::Rectangle => Node::new(Kind::Rectangle, r),
            Tool::Ellipse => Node::new(Kind::Ellipse, r),
            Tool::Polygon => Node::new(Kind::Polygon, r),
            Tool::Star => Node::new(Kind::Star, r),
            Tool::Text => Node::new(Kind::Text, r),
            _ => {
                let end = if r.width() == 0.0 && r.height() == 0.0 { r.min } else { pos2(r.max.x, r.min.y) };
                let mut n = Node::vector(&[PathPoint { p: r.min, ..Default::default() }, PathPoint { p: end, ..Default::default() }], false);
                if t == Tool::Arrow {
                    n.end_marker = crate::doc::Marker::Arrow;
                    n.name = self.doc.auto_name("Arrow");
                } else {
                    n.name = self.doc.auto_name("Line");
                }
                n
            }
        }
    }

    fn on_double_click(&mut self, w: Pos2) {
        if self.vector_edit.is_some() {
            let s = self.view.to_screen(w);
            self.vector_toggle_smooth(s);
            return;
        }
        if let [one] = self.sel[..]
            && matches!(self.doc.node(one).kind, Kind::Vector | Kind::Rectangle | Kind::Ellipse | Kind::Polygon | Kind::Star)
                && self.doc.node(one).aabb().expand(4.0 / self.view.zoom).contains(w)
            {
                self.enter_vector_edit(one);
                return;
            }
        let Some(hit) = self.doc.hit(self.page, w, 3.0 / self.view.zoom) else { return };
        let Some(&cur) = self.sel.first() else { return };
        if self.doc.node(cur).kind == Kind::Text {
            self.checkpoint();
            self.editing_text = Some(cur);
            self.focus_text = true;
            return;
        }
        // Dive one level into the selected container toward the clicked layer.
        let mut chain = self.doc.ancestors(hit);
        chain.reverse();
        chain.push(hit);
        if let Some(i) = chain.iter().position(|&c| c == cur)
            && let Some(&next) = chain.get(i + 1) {
                self.sel = vec![next];
                if self.doc.node(next).kind == Kind::Text && i + 2 == chain.len() {
                    self.checkpoint();
                    self.editing_text = Some(next);
                    self.focus_text = true;
                }
            }
    }

    /// The auto-layout frame a single dragged layer is being reordered within, if any.
    fn reorder_parent(&self, roots: &[NodeId], w: Pos2) -> Option<NodeId> {
        let [id] = roots else { return None };
        let n = self.doc.node(*id);
        let p = self.doc.node(n.parent?);
        let inside = p.rect.contains(p.to_local(w));
        (p.layout.mode != LayoutMode::None && !n.absolute && inside).then_some(p.id)
    }

    /// Child-list index for dropping `id` into auto-layout frame `p` at pointer `w`.
    fn al_index(&self, p: NodeId, id: NodeId, w: Pos2) -> usize {
        let pn = self.doc.node(p);
        let h = pn.layout.mode == LayoutMode::Horizontal;
        let mut index = 0;
        let others = pn.children.iter().filter(|c| **c != id);
        for (i, c) in others.enumerate() {
            let k = self.doc.node(*c);
            if !k.visible || k.absolute {
                continue;
            }
            let (b, center) = (k.aabb(), k.aabb().center());
            let before = match pn.layout.mode {
                // Grids read row by row: rows above, then cells to the left on the pointer's row.
                LayoutMode::Grid => b.max.y < w.y || (b.min.y <= w.y && center.x < w.x),
                _ if h => center.x < w.x,
                _ => center.y < w.y,
            };
            if before {
                index = i + 1;
            }
        }
        index
    }

    fn marquee_hits(&self, r: Rect) -> Vec<NodeId> {
        let usable = |n: &Node| n.visible && !n.locked;
        let mut out = vec![];
        for &id in &self.doc.pages[self.page].children {
            let n = self.doc.node(id);
            if !usable(n) {
                continue;
            }
            if n.kind.is_frame() && !n.children.is_empty() && !r.contains_rect(n.aabb()) {
                out.extend(n.children.iter().copied().filter(|c| {
                    let c = self.doc.node(*c);
                    usable(c) && r.intersects(c.aabb())
                }));
            } else if r.intersects(n.aabb()) {
                out.push(id);
            }
        }
        out
    }

    // ---------- snapping ----------

    /// Visible siblings of the first excluded layer: what it aligns and spaces against.
    fn snap_siblings(&self, exclude: &[NodeId]) -> Vec<Rect> {
        let parent = exclude.first().and_then(|&e| self.doc.nodes.get(&e)).and_then(|n| n.parent);
        self.doc
            .children_of(self.page, parent)
            .iter()
            .filter(|c| !exclude.contains(c))
            .map(|c| self.doc.node(*c))
            .filter(|n| n.visible)
            .map(|n| n.aabb())
            .collect()
    }

    fn snap_targets(&self, exclude: &[NodeId]) -> Vec<Rect> {
        let parent = exclude.first().and_then(|&e| self.doc.nodes.get(&e)).and_then(|n| n.parent);
        let mut out = self.snap_siblings(exclude);
        if let Some(p) = parent {
            let pn = self.doc.node(p);
            out.push(pn.aabb());
            // Column and row edges of the parent's layout guides snap too.
            if self.show_grids {
                let r = pn.rect;
                for g in pn.grids.iter().filter(|g| g.visible) {
                    match g.kind {
                        crate::doc::GridKind::Columns => {
                            for (a, b) in g.bands(r.min.x, r.max.x) {
                                out.push(Rect::from_min_max(pos2(a, r.min.y), pos2(a, r.max.y)));
                                out.push(Rect::from_min_max(pos2(b, r.min.y), pos2(b, r.max.y)));
                            }
                        }
                        crate::doc::GridKind::Rows => {
                            for (a, b) in g.bands(r.min.y, r.max.y) {
                                out.push(Rect::from_min_max(pos2(r.min.x, a), pos2(r.max.x, a)));
                                out.push(Rect::from_min_max(pos2(r.min.x, b), pos2(r.max.x, b)));
                            }
                        }
                        // Uniform grids snap on their lines, unless they're too dense to be useful.
                        crate::doc::GridKind::Grid => {
                            let step = g.size.max(1.0);
                            if (r.width() + r.height()) / step <= 400.0 {
                                let mut x = r.min.x;
                                while x <= r.max.x {
                                    out.push(Rect::from_min_max(pos2(x, r.min.y), pos2(x, r.max.y)));
                                    x += step;
                                }
                                let mut y = r.min.y;
                                while y <= r.max.y {
                                    out.push(Rect::from_min_max(pos2(r.min.x, y), pos2(r.max.x, y)));
                                    y += step;
                                }
                            }
                        }
                    }
                }
            }
        }
        out
    }

    /// Layout guides over frames (Ctrl+G toggles), drawn above content like Figma.
    fn draw_layout_grids(&self, painter: &Painter) {
        if !self.show_grids {
            return;
        }
        for n in self.doc.nodes.values().filter(|n| !n.grids.is_empty() && n.visible && self.doc.page_of(n.id) == self.page) {
            let r = n.rect;
            let clip = painter.with_clip_rect(painter.clip_rect().intersect(self.view.rect_to_screen(r)));
            for g in n.grids.iter().filter(|g| g.visible) {
                let c = g.color.to32();
                match g.kind {
                    crate::doc::GridKind::Columns => {
                        for (a, b) in g.bands(r.min.x, r.max.x) {
                            clip.rect_filled(self.view.rect_to_screen(Rect::from_min_max(pos2(a, r.min.y), pos2(b, r.max.y))), 0.0, c);
                        }
                    }
                    crate::doc::GridKind::Rows => {
                        for (a, b) in g.bands(r.min.y, r.max.y) {
                            clip.rect_filled(self.view.rect_to_screen(Rect::from_min_max(pos2(r.min.x, a), pos2(r.max.x, b))), 0.0, c);
                        }
                    }
                    crate::doc::GridKind::Grid => {
                        let step = g.size.max(1.0);
                        if step * self.view.zoom < 3.0 {
                            continue;
                        }
                        let stroke = Stroke::new(1.0, c);
                        let mut x = r.min.x + step;
                        while x < r.max.x {
                            let sx = self.view.to_screen(pos2(x, 0.0)).x;
                            clip.line_segment([pos2(sx, self.view.to_screen(r.min).y), pos2(sx, self.view.to_screen(r.max).y)], stroke);
                            x += step;
                        }
                        let mut y = r.min.y + step;
                        while y < r.max.y {
                            let sy = self.view.to_screen(pos2(0.0, y)).y;
                            clip.line_segment([pos2(self.view.to_screen(r.min).x, sy), pos2(self.view.to_screen(r.max).x, sy)], stroke);
                            y += step;
                        }
                    }
                }
            }
        }
    }

    /// Correction that snaps the masked edges of `moving` to nearby sibling edges/centers, or
    /// to a gap that repeats the spacing between its neighbors (Figma's smart spacing guides).
    /// Records guide lines and gap markers for display.
    fn snap(&mut self, moving: Rect, exclude: &[NodeId], mx: [bool; 3], my: [bool; 3]) -> Vec2 {
        self.guides.clear();
        self.gap_marks.clear();
        let tol = 5.0 / self.view.zoom;
        let objects = self.prefs.snap_to_objects;
        let targets = if objects { self.snap_targets(exclude) } else { vec![] };
        let sibs = if objects { self.snap_siblings(exclude) } else { vec![] };
        // Ruler guides always snap.
        let lines: Vec<Guide> = self.doc.pages[self.page].guides.clone();
        let vals = |r: Rect, a: usize| [r.min[a], r.center()[a], r.max[a]];
        let mut corr = Vec2::ZERO;
        let mut gap_used = [None, None];
        for (a, mask) in [mx, my].into_iter().enumerate() {
            let mut best: Option<f32> = None;
            for (_, mv) in vals(moving, a).iter().enumerate().filter(|(i, _)| mask[*i]) {
                let on_axis = lines.iter().filter(|g| g.axis as usize == a).map(|g| g.offset);
                for tv in targets.iter().flat_map(|r| vals(*r, a)).chain(on_axis) {
                    let d = tv - mv;
                    if d.abs() < tol && best.is_none_or(|b| d.abs() < b.abs()) {
                        best = Some(d);
                    }
                }
            }
            let gap = gap_snaps(moving, &sibs, a, mask).into_iter().filter(|(d, _)| d.abs() < tol).min_by(|x, y| x.0.abs().total_cmp(&y.0.abs()));
            match (best, gap) {
                (b, Some((d, g))) if b.is_none_or(|b| d.abs() <= b.abs()) => {
                    corr[a] = d;
                    gap_used[a] = Some(g);
                }
                (Some(b), _) => corr[a] = b,
                _ => {}
            }
        }
        let m = moving.translate(corr);
        for r in &targets {
            for (i, mv) in vals(m, 0).iter().enumerate() {
                if mx[i] && vals(*r, 0).iter().any(|t| (t - mv).abs() < 0.01) {
                    self.guides.push([pos2(*mv, m.min.y.min(r.min.y)), pos2(*mv, m.max.y.max(r.max.y))]);
                }
            }
            for (i, mv) in vals(m, 1).iter().enumerate() {
                if my[i] && vals(*r, 1).iter().any(|t| (t - mv).abs() < 0.01) {
                    self.guides.push([pos2(m.min.x.min(r.min.x), *mv), pos2(m.max.x.max(r.max.x), *mv)]);
                }
            }
        }
        for (a, g) in gap_used.into_iter().enumerate() {
            if let Some(g) = g {
                self.gap_marks.extend(equal_gaps(m, &sibs, a, g));
            }
        }
        corr
    }

    fn snap_point(&mut self, p: Pos2, exclude: &[NodeId]) -> Pos2 {
        let r = Rect::from_min_size(p, Vec2::ZERO);
        p + self.snap(r, exclude, [true, false, true], [true, false, true])
    }

    // ---------- smart selection ----------

    /// Figma's smart selection: 2+ selected layers in a row or column with equal gaps (within
    /// 1px). Returns the axis (0 = row) and the layers in order along it.
    pub fn smart_selection(&self) -> Option<(usize, Vec<(NodeId, Rect)>)> {
        let roots = self.sel_roots();
        if roots.len() < 2 || self.vector_edit.is_some() {
            return None;
        }
        (0..2).find_map(|a| {
            let mut items: Vec<(NodeId, Rect)> = roots.iter().map(|&i| (i, self.doc.node(i).aabb())).collect();
            items.sort_by(|x, y| x.1.min[a].total_cmp(&y.1.min[a]));
            let gaps: Vec<f32> = items.windows(2).map(|w| w[1].1.min[a] - w[0].1.max[a]).collect();
            gaps.iter().all(|g| *g >= 0.0 && (g - gaps[0]).abs() <= 1.0).then_some((a, items))
        })
    }

    /// Screen rects of the gaps between smart-selected layers, spanning the selection across.
    fn smart_gaps(&self, a: usize, items: &[(NodeId, Rect)]) -> Vec<Rect> {
        let b = self.doc.bounds(&items.iter().map(|(i, _)| *i).collect::<Vec<_>>());
        items
            .windows(2)
            .map(|w| {
                let (mut lo, mut hi) = (b.min, b.max);
                lo[a] = w[0].1.max[a];
                hi[a] = w[1].1.min[a];
                self.view.rect_to_screen(Rect::from_min_max(lo, hi))
            })
            .collect()
    }

    /// The spacing handle mid-gap, or a layer's center knob, under screen point `s`.
    fn smart_grip_at(&self, s: Pos2) -> Option<Grip> {
        let (a, items) = self.smart_selection()?;
        for g in self.smart_gaps(a, &items) {
            let d = s - g.center();
            if d[a].abs() <= 4.0 && d[1 - a].abs() <= 10.0 {
                return Some(Grip::Spacing(a));
            }
        }
        items.iter().find(|(_, r)| self.view.to_screen(r.center()).distance(s) <= 6.0).map(|(id, _)| Grip::Swap(*id))
    }

    /// With the pointer over a smart selection: knobs on each layer and a pink handle in each
    /// gap; the hovered or dragged gaps light up with their size.
    fn draw_smart_selection(&self, painter: &Painter, ptr: Option<Pos2>) {
        let Some((a, items)) = self.smart_selection() else { return };
        let gaps = self.smart_gaps(a, &items);
        let b = self.view.rect_to_screen(self.doc.bounds(&self.sel_roots()));
        let dragging = matches!(self.drag, Some(Drag::Spacing { .. }));
        if !dragging && !ptr.is_some_and(|p| b.expand(4.0).contains(p)) {
            return;
        }
        let hot = dragging || ptr.is_some_and(|p| gaps.iter().any(|g| g.expand(2.0).contains(p)));
        for g in &gaps {
            if hot {
                painter.rect_filled(*g, 0.0, GAP.gamma_multiply(0.2));
                let mut at = g.center();
                at[1 - a] = g.max[1 - a] + 12.0;
                pill(painter, at, num(g.size()[a] / self.view.zoom), GAP);
            }
            let mut half = Vec2::ZERO;
            half[1 - a] = 6.0;
            painter.line_segment([g.center() - half, g.center() + half], Stroke::new(2.0, GAP));
        }
        for (_, r) in &items {
            painter.circle(self.view.to_screen(r.center()), 3.5, Color32::WHITE, Stroke::new(1.5, GAP));
        }
    }

    // ---------- auto layout spacing handles ----------

    /// A lone selected auto layout frame's spacing bands in world space, each with its grip and
    /// thin axis: the gaps between in-flow children and the four padding bands.
    fn layout_bands(&self) -> Vec<(Grip, Rect, usize)> {
        let [id] = self.sel[..] else { return vec![] };
        let n = self.doc.node(id);
        if !n.kind.is_frame() || n.rotation != 0.0 || !matches!(n.layout.mode, LayoutMode::Horizontal | LayoutMode::Vertical) || self.vector_edit.is_some() {
            return vec![];
        }
        let l = n.layout;
        let a = if l.mode == LayoutMode::Horizontal { 0 } else { 1 };
        let [pt, pr, pb, pl] = l.padding;
        let inner = Rect::from_min_max(n.rect.min + vec2(pl, pt), n.rect.max - vec2(pr, pb));
        let kids: Vec<Rect> = n.children.iter().map(|c| self.doc.node(*c)).filter(|k| k.visible && !k.absolute).map(|k| k.aabb()).collect();
        let mut out = vec![];
        for w in kids.windows(2) {
            // Wrapped rows: only neighbors on the same row have a gap between them.
            let same_row = w[0].min[1 - a] < w[1].max[1 - a] && w[1].min[1 - a] < w[0].max[1 - a];
            if w[1].min[a] > w[0].max[a] && same_row {
                let mut r = if l.wrap { w[0].union(w[1]) } else { inner };
                (r.min[a], r.max[a]) = (w[0].max[a], w[1].min[a]);
                out.push((Grip::Gap, r, a));
            }
        }
        // Zero padding still gets a grabbable sliver.
        let r = n.rect;
        let t = |v: f32| v.max(14.0 / self.view.zoom);
        out.push((Grip::Padding(0), Rect::from_min_max(r.min, pos2(r.max.x, r.min.y + t(pt))), 1));
        out.push((Grip::Padding(1), Rect::from_min_max(pos2(r.max.x - t(pr), r.min.y), r.max), 0));
        out.push((Grip::Padding(2), Rect::from_min_max(pos2(r.min.x, r.max.y - t(pb)), r.max), 1));
        out.push((Grip::Padding(3), Rect::from_min_max(r.min, pos2(r.min.x + t(pl), r.max.y)), 0));
        out
    }

    /// The gap or padding handle (mid-band, as in Figma) under screen point `s`.
    fn layout_grip_at(&self, s: Pos2) -> Option<Grip> {
        self.layout_bands().into_iter().find_map(|(g, r, t)| {
            let r = self.view.rect_to_screen(r);
            let d = s - r.center();
            (d[t].abs() <= (r.size()[t] / 2.0).max(4.0) && d[1 - t].abs() <= 10.0).then_some(g)
        })
    }

    /// Hovering a selected auto layout frame's gap lights up every gap in pink with its size;
    /// hovering a padding band lights up that band. Each shows a handle to drag.
    fn draw_layout_spacing(&self, painter: &Painter, ptr: Option<Pos2>) {
        let bands = self.layout_bands();
        let hot = match &self.drag {
            Some(Drag::LayoutSpacing { grip, .. }) => Some(*grip),
            _ => ptr.and_then(|p| bands.iter().find(|(_, r, _)| self.view.rect_to_screen(*r).contains(p)).map(|(g, _, _)| *g)),
        };
        let Some(hot) = hot else { return };
        let l = self.doc.node(self.sel[0]).layout;
        for (g, r, t) in bands.iter().filter(|(g, _, _)| *g == hot) {
            let r = self.view.rect_to_screen(*r);
            painter.rect_filled(r, 0.0, GAP.gamma_multiply(0.2));
            let mut half = Vec2::ZERO;
            half[1 - t] = 6.0;
            painter.line_segment([r.center() - half, r.center() + half], Stroke::new(2.0, GAP));
            let v = match g {
                Grip::Padding(side) => l.padding[*side],
                _ => l.gap,
            };
            let mut at = r.center();
            at[1 - t] += 16.0;
            pill(painter, at, num(v), GAP);
        }
    }

    /// `p` on whole pixels when snapping to the pixel grid (Shift Cmd '), else as is.
    fn px(&self, p: Pos2) -> Pos2 {
        if self.prefs.snap_to_pixels { p.round() } else { p }
    }

    fn px1(&self, v: f32) -> f32 {
        if self.prefs.snap_to_pixels { v.round() } else { v }
    }

    // ---------- overlays ----------

    /// Purple for components, instances, and anything inside them; blue otherwise.
    pub fn accent(&self, id: NodeId) -> Color32 {
        let comp = std::iter::once(id)
            .chain(self.doc.ancestors(id))
            .any(|a| matches!(self.doc.node(a).kind, Kind::Component | Kind::Instance | Kind::ComponentSet));
        if comp { PURPLE } else { BLUE }
    }

    fn outline(&self, painter: &Painter, id: NodeId, width: f32) {
        // The hovered layer can vanish mid-frame (flattened, deleted).
        let Some(n) = self.doc.nodes.get(&id) else { return };
        let accent = self.accent(id);
        match n.kind {
            Kind::Vector | Kind::Ellipse => {
                painter.add(render::stroke_mesh(&render::node_path(n, &self.view, 0.0), width, accent));
            }
            _ => {
                let pts = n.corners().map(|p| self.view.to_screen(p)).to_vec();
                painter.add(egui::Shape::closed_line(pts, Stroke::new(width, accent)));
            }
        }
    }

    /// Collaborators' selections and cursors, in their colors.
    fn draw_peers(&self, painter: &Painter) {
        let Some(c) = &self.collab else { return };
        for p in c.peers.values().filter(|p| p.page == self.page) {
            let col = crate::collab::PEER_COLORS[p.color % crate::collab::PEER_COLORS.len()];
            for id in &p.selection {
                if let Some(n) = self.doc.nodes.get(id) {
                    let pts = n.corners().map(|q| self.view.to_screen(q)).to_vec();
                    painter.add(egui::Shape::closed_line(pts, Stroke::new(2.0, col)));
                }
            }
            if let Some(w) = p.cursor {
                let s = self.view.to_screen(w);
                let arrow = vec![s, s + vec2(0.0, 16.0), s + vec2(4.5, 12.0), s + vec2(8.0, 18.5), s + vec2(10.5, 17.3), s + vec2(7.2, 11.0), s + vec2(12.0, 11.0)];
                painter.add(egui::Shape::convex_polygon(arrow[..3].to_vec(), col, Stroke::NONE));
                painter.add(egui::Shape::closed_line(arrow, Stroke::new(1.5, col)));
                let g = painter.layout_no_wrap(p.name.clone(), FontId::proportional(11.0), Color32::WHITE);
                let pill = Rect::from_min_size(s + vec2(12.0, 18.0), g.size() + vec2(10.0, 4.0));
                painter.rect_filled(pill, 4.0, col);
                painter.galley(pill.min + vec2(5.0, 2.0), g, Color32::WHITE);
            }
        }
    }

    /// Prototype tab: the "+" circle on the right edge of a single selection.
    fn connect_handle_at(&self, s: Pos2) -> Option<NodeId> {
        let [one] = self.sel[..] else { return None };
        let b = self.view.rect_to_screen(self.doc.node(one).aabb());
        (pos2(b.max.x, b.center().y).distance(s) < 9.0).then_some(one)
    }

    /// Blue noodles from layers to their destinations, as in Figma's Prototype tab.
    fn draw_connections(&self, painter: &Painter, ptr: Option<Pos2>) {
        let stroke = Stroke::new(1.5, BLUE);
        let noodle = |a: Pos2, b: Pos2| {
            let dx = ((b.x - a.x).abs() / 2.0).max(40.0);
            let pts = [a, a + vec2(dx, 0.0), b - vec2(dx, 0.0), b];
            painter.add(egui::epaint::CubicBezierShape::from_points_stroke(pts, false, Color32::TRANSPARENT, stroke));
            painter.circle_filled(a, 3.5, BLUE);
            painter.add(egui::Shape::convex_polygon(vec![b, b + vec2(-8.0, -4.5), b + vec2(-8.0, 4.5)], BLUE, Stroke::NONE));
        };
        for n in self.doc.nodes.values().filter(|n| !n.reactions.is_empty() && self.doc.page_of(n.id) == self.page) {
            let a = self.view.rect_to_screen(n.aabb());
            for r in &n.reactions {
                if let crate::doc::Action::Navigate(t) | crate::doc::Action::OpenOverlay(t) | crate::doc::Action::SwapOverlay(t) | crate::doc::Action::ChangeTo(t) | crate::doc::Action::ScrollTo(t) = r.action
                    && let Some(tn) = self.doc.nodes.get(&t) {
                        let b = self.view.rect_to_screen(tn.aabb());
                        noodle(pos2(a.max.x, a.center().y), pos2(b.min.x, b.center().y));
                    }
            }
        }
        for &f in &self.doc.pages[self.page].flows {
            if let Some(n) = self.doc.nodes.get(&f) {
                let r = self.view.rect_to_screen(n.aabb());
                let g = painter.layout_no_wrap("▶ Flow".into(), FontId::proportional(10.0), Color32::WHITE);
                let pill = Rect::from_min_size(pos2(r.max.x - g.size().x - 10.0, r.min.y - g.size().y - 8.0), g.size() + vec2(10.0, 4.0));
                painter.rect_filled(pill, 3.0, BLUE);
                painter.galley(pill.min + vec2(5.0, 2.0), g, Color32::WHITE);
            }
        }
        if let [one] = self.sel[..] {
            let b = self.view.rect_to_screen(self.doc.node(one).aabb());
            let h = pos2(b.max.x, b.center().y);
            if let (Some(Drag::Connect { .. }), Some(p)) = (&self.drag, ptr) {
                noodle(h, p);
            }
            painter.circle(h, 6.0, Color32::WHITE, Stroke::new(1.5, BLUE));
            painter.line_segment([h - vec2(3.0, 0.0), h + vec2(3.0, 0.0)], Stroke::new(1.5, BLUE));
            painter.line_segment([h - vec2(0.0, 3.0), h + vec2(0.0, 3.0)], Stroke::new(1.5, BLUE));
        }
    }

    fn draw_overlays(&self, painter: &Painter, ptr: Option<Pos2>) {
        // Component sets wear a dashed purple border, as in Figma.
        for n in self.doc.nodes.values().filter(|n| n.kind == Kind::ComponentSet && self.doc.page_of(n.id) == self.page) {
            let pts = n.corners().map(|p| self.view.to_screen(p));
            let mut ring = pts.to_vec();
            ring.push(pts[0]);
            painter.extend(egui::Shape::dashed_line(&ring, Stroke::new(1.0, PURPLE), 5.0, 3.0));
        }
        if self.prototype_tab {
            self.draw_connections(painter, ptr);
        }
        // Slices are invisible in output but outlined while editing.
        for n in self.doc.nodes.values().filter(|n| n.kind == Kind::Slice && n.visible && self.doc.page_of(n.id) == self.page) {
            let pts = n.corners().map(|p| self.view.to_screen(p));
            let mut ring = pts.to_vec();
            ring.push(pts[0]);
            painter.extend(egui::Shape::dashed_line(&ring, Stroke::new(1.0, Color32::from_gray(0x99)), 4.0, 3.0));
        }
        if let Some(h) = self.hover.filter(|h| !self.sel.contains(h) && self.prefs.highlight_on_hover) {
            self.outline(painter, h, 2.0);
        }
        if self.vector_edit.is_some() {
            self.draw_vector_edit(painter, ptr);
            if let (Some(Drag::PointMarquee { start, .. }), Some(p)) = (&self.drag, ptr) {
                let r = Rect::from_two_pos(self.view.to_screen(*start), p);
                painter.rect_filled(r, 0.0, BLUE.gamma_multiply(0.1));
                painter.rect_stroke(r, 0.0, Stroke::new(1.0, BLUE), StrokeKind::Inside);
            }
            return;
        }
        let roots = self.sel_roots();
        if !roots.is_empty() {
            if roots.len() > 1 {
                for &id in &roots {
                    self.outline(painter, id, 1.0);
                }
            }
            let (fr, rot) = self.sel_frame().unwrap_or((Rect::NOTHING, 0.0));
            let corners = [fr.left_top(), fr.right_top(), fr.right_bottom(), fr.left_bottom()]
                .map(|p| self.view.to_screen(rotate(p, fr.center(), rot)));
            let accent = self.accent(roots[0]);
            painter.add(egui::Shape::closed_line(corners.to_vec(), Stroke::new(1.0, accent)));
            if self.editing_text.is_none() && self.tool.selects() {
                for c in corners {
                    let h = Rect::from_center_size(c, vec2(7.0, 7.0));
                    painter.rect_filled(h, 1.0, Color32::WHITE);
                    painter.rect_stroke(h, 1.0, Stroke::new(1.0, accent), StrokeKind::Middle);
                }
            }
            let b = self.view.rect_to_screen(self.doc.bounds(&roots));
            let label = match &self.drag {
                Some(Drag::Rotate { deg, .. }) => format!("{}°", num(*deg)),
                _ => format!("{} × {}", num(fr.width()), num(fr.height())),
            };
            // The size label (rotation angle while rotating), unless turned off in Settings.
            if self.prefs.show_dimensions || matches!(self.drag, Some(Drag::Rotate { .. })) {
                let g = painter.layout_no_wrap(label, FontId::proportional(11.0), Color32::WHITE);
                let pill = Rect::from_center_size(b.center_bottom() + vec2(0.0, 8.0 + g.size().y / 2.0 + 2.0), g.size() + vec2(8.0, 4.0));
                painter.rect_filled(pill, 2.0, accent);
                painter.galley(pill.min + vec2(4.0, 2.0), g, Color32::WHITE);
            }
        }
        if self.editing_text.is_none() && self.tool.selects() && matches!(self.drag, None | Some(Drag::Spacing { .. } | Drag::Swap { .. })) {
            self.draw_smart_selection(painter, ptr);
        }
        self.draw_gradient_handles(painter);
        if self.editing_text.is_none() && self.tool.selects() && matches!(self.drag, None | Some(Drag::LayoutSpacing { .. })) {
            self.draw_layout_spacing(painter, ptr);
        }
        for g in &self.guides {
            painter.line_segment([self.view.to_screen(g[0]), self.view.to_screen(g[1])], Stroke::new(1.0, SNAP));
        }
        // Equal gaps: a pink span with end ticks and the gap size, as in Figma.
        for [a, b] in &self.gap_marks {
            let (sa, sb) = (self.view.to_screen(*a), self.view.to_screen(*b));
            let t = (sb - sa).normalized().rot90() * 4.0;
            for seg in [[sa, sb], [sa - t, sa + t], [sb - t, sb + t]] {
                painter.line_segment(seg, Stroke::new(1.0, GAP));
            }
            pill(painter, sa.lerp(sb, 0.5) + t * 3.0, num(a.distance(*b)), GAP);
        }
        if let (Some(Drag::Marquee { start, .. } | Drag::Zoom { start }), Some(p)) = (&self.drag, ptr) {
            let r = Rect::from_two_pos(self.view.to_screen(*start), p);
            painter.rect_filled(r, 0.0, BLUE.gamma_multiply(0.1));
            painter.rect_stroke(r, 0.0, Stroke::new(1.0, BLUE), StrokeKind::Inside);
        }
        self.draw_pen(painter, ptr);
    }

    fn draw_pen(&self, painter: &Painter, ptr: Option<Pos2>) {
        if self.pen.is_empty() {
            return;
        }
        let mut pts = self.pen.clone();
        let dragging = matches!(self.drag, Some(Drag::PenHandle));
        if let (Some(p), false) = (ptr, dragging) {
            pts.push(PathPoint { p: self.view.to_world(p), ..Default::default() });
        }
        if pts.len() >= 2 {
            let n = Node::vector(&pts, false);
            painter.add(render::stroke_mesh(&render::node_path(&n, &self.view, 0.0), 1.0, BLUE));
        }
        for pt in &self.pen {
            let c = self.view.to_screen(pt.p);
            if pt.h_out != Vec2::ZERO {
                for h in [pt.h_in, pt.h_out] {
                    let e = self.view.to_screen(pt.p + h);
                    painter.line_segment([c, e], Stroke::new(1.0, BLUE));
                    painter.circle_filled(e, 3.0, BLUE);
                }
            }
            let r = Rect::from_center_size(c, vec2(6.0, 6.0));
            painter.rect_filled(r, 0.0, Color32::WHITE);
            painter.rect_stroke(r, 0.0, Stroke::new(1.0, BLUE), StrokeKind::Middle);
        }
    }

    /// Figma shows a pixel grid from 800% zoom (Cmd ' turns it off).
    fn draw_pixel_grid(&self, painter: &Painter, rect: Rect) {
        if self.view.zoom < 8.0 || !self.prefs.pixel_grid {
            return;
        }
        let (a, b) = (self.view.to_world(rect.min), self.view.to_world(rect.max));
        let stroke = Stroke::new(1.0, if painter.ctx().global_style().visuals.dark_mode { Color32::from_white_alpha(18) } else { Color32::from_black_alpha(18) });
        for x in (a.x.floor() as i64)..=(b.x.ceil() as i64) {
            let sx = self.view.to_screen(pos2(x as f32, 0.0)).x.round() + 0.5;
            painter.line_segment([pos2(sx, rect.min.y), pos2(sx, rect.max.y)], stroke);
        }
        for y in (a.y.floor() as i64)..=(b.y.ceil() as i64) {
            let sy = self.view.to_screen(pos2(0.0, y as f32)).y.round() + 0.5;
            painter.line_segment([pos2(rect.min.x, sy), pos2(rect.max.x, sy)], stroke);
        }
    }

    /// Hold Alt over another layer to see distances from the selection (red lines).
    fn draw_measurements(&self, painter: &Painter) {
        let roots = self.sel_roots();
        let Some(&first) = roots.first() else { return };
        let s = self.doc.bounds(&roots);
        let target = self
            .hover
            .filter(|h| !roots.contains(h) && self.doc.nodes.contains_key(h))
            .or_else(|| self.doc.node(first).parent)
            .filter(|t| !roots.contains(t));
        let Some(t) = target.map(|t| self.doc.node(t).aabb()) else { return };
        let red = SNAP;
        let v = self.view;
        painter.rect_stroke(v.rect_to_screen(t), 0.0, Stroke::new(1.0, red), StrokeKind::Middle);
        let mut lines: Vec<(Pos2, Pos2)> = vec![];
        let (cx, cy) = (s.center().x, s.center().y);
        if t.contains_rect(s) {
            lines.push((pos2(t.min.x, cy), pos2(s.min.x, cy)));
            lines.push((pos2(s.max.x, cy), pos2(t.max.x, cy)));
            lines.push((pos2(cx, t.min.y), pos2(cx, s.min.y)));
            lines.push((pos2(cx, s.max.y), pos2(cx, t.max.y)));
        } else {
            let y = if s.max.y > t.min.y && s.min.y < t.max.y { (s.min.y.max(t.min.y) + s.max.y.min(t.max.y)) / 2.0 } else { cy };
            let x = if s.max.x > t.min.x && s.min.x < t.max.x { (s.min.x.max(t.min.x) + s.max.x.min(t.max.x)) / 2.0 } else { cx };
            if s.max.x <= t.min.x {
                lines.push((pos2(s.max.x, y), pos2(t.min.x, y)));
            } else if t.max.x <= s.min.x {
                lines.push((pos2(t.max.x, y), pos2(s.min.x, y)));
            }
            if s.max.y <= t.min.y {
                lines.push((pos2(x, s.max.y), pos2(x, t.min.y)));
            } else if t.max.y <= s.min.y {
                lines.push((pos2(x, t.max.y), pos2(x, s.min.y)));
            }
        }
        for (a, b) in lines {
            let d = a.distance(b);
            if d < 0.01 {
                continue;
            }
            let (sa, sb) = (v.to_screen(a), v.to_screen(b));
            painter.line_segment([sa, sb], Stroke::new(1.0, red));
            pill(painter, sa.lerp(sb, 0.5), num(d), red);
        }
    }

    /// Dev Mode, hovering an auto layout frame: its padding and the gaps between children, shaded
    /// pink with their sizes, as Figma shows them.
    fn draw_dev_spacing(&self, painter: &Painter, id: NodeId) {
        for (band, size) in spacing_bands(&self.doc, id) {
            let sr = self.view.rect_to_screen(band);
            painter.rect_filled(sr, 0.0, GAP.gamma_multiply(0.2));
            painter.rect_stroke(sr, 0.0, Stroke::new(1.0, GAP.gamma_multiply(0.6)), StrokeKind::Inside);
            pill(painter, sr.center(), num(size), GAP);
        }
    }

    /// Dev Mode annotations: each note as a label to the right of its layer, joined by a leader line.
    fn draw_annotations(&self, painter: &Painter) {
        let stroke = Stroke::new(1.0, PURPLE);
        for n in self.doc.nodes.values().filter(|n| !n.annotations.is_empty() && self.doc.page_of(n.id) == self.page) {
            let r = self.view.rect_to_screen(n.aabb());
            let mut y = r.min.y;
            for note in &n.annotations {
                let g = painter.layout(note.clone(), FontId::proportional(11.0), Color32::WHITE, 200.0);
                let label = Rect::from_min_size(pos2(r.max.x + 32.0, y), g.size() + vec2(12.0, 8.0));
                let from = pos2(r.max.x, r.center().y.clamp(r.min.y, r.max.y));
                painter.line_segment([from, pos2(label.min.x, label.center().y)], stroke);
                painter.circle_filled(from, 3.0, PURPLE);
                painter.rect_filled(label, 4.0, PURPLE);
                painter.galley(label.min + vec2(6.0, 4.0), g, Color32::WHITE);
                y = label.max.y + 6.0;
            }
        }
    }

    /// The ruler under screen point `s`, as the axis of the guide it pulls out (top ruler: Y).
    fn ruler_at(&self, s: Pos2) -> Option<Axis> {
        let r = self.canvas_rect.filter(|_| self.rulers)?;
        let (top, left) = (s.y < r.min.y + RULER, s.x < r.min.x + RULER);
        match (top, left) {
            (true, false) => Some(Axis::Y),
            (false, true) => Some(Axis::X),
            _ => None,
        }
    }

    /// The ruler guide within grabbing distance of screen point `s` (move tool only).
    fn guide_at(&self, s: Pos2) -> Option<usize> {
        if !self.tool.selects() || self.vector_edit.is_some() {
            return None;
        }
        self.doc.pages[self.page].guides.iter().position(|g| {
            let at = self.view.to_screen(pos2(g.offset, g.offset));
            match g.axis {
                Axis::X => (at.x - s.x).abs() <= 3.0,
                Axis::Y => (at.y - s.y).abs() <= 3.0,
            }
        })
    }

    pub fn delete_guide(&mut self) {
        if let Some(i) = self.guide_sel.take().filter(|i| *i < self.doc.pages[self.page].guides.len()) {
            self.checkpoint();
            self.doc.pages[self.page].guides.remove(i);
        }
    }

    pub fn remove_all_guides(&mut self) {
        self.guide_sel = None;
        if !self.doc.pages[self.page].guides.is_empty() {
            self.checkpoint();
            self.doc.pages[self.page].guides.clear();
        }
    }

    /// Ruler guides: thin red lines across the canvas. The selected one is bolder and shows its
    /// position next to the ruler.
    fn draw_guides(&self, painter: &Painter, rect: Rect) {
        for (i, g) in self.doc.pages[self.page].guides.iter().enumerate() {
            let on = self.guide_sel == Some(i);
            let at = self.view.to_screen(pos2(g.offset, g.offset));
            let (line, label) = match g.axis {
                Axis::X => ([pos2(at.x, rect.min.y), pos2(at.x, rect.max.y)], pos2(at.x, rect.min.y + RULER + 12.0)),
                Axis::Y => ([pos2(rect.min.x, at.y), pos2(rect.max.x, at.y)], pos2(rect.min.x + RULER + 20.0, at.y)),
            };
            painter.line_segment(line, Stroke::new(if on { 1.5 } else { 1.0 }, SNAP));
            if on {
                pill(painter, label, num(g.offset), SNAP);
            }
        }
    }

    /// Shift+R: top and left rulers with the selection's span highlighted.
    fn draw_rulers(&self, painter: &Painter, rect: Rect) {
        const T: f32 = RULER;
        let bg = painter.ctx().global_style().visuals.panel_fill.to_opaque();
        let line = crate::appearance::border(painter.ctx());
        let text = crate::appearance::muted(painter.ctx());
        let top = Rect::from_min_max(rect.min, pos2(rect.max.x, rect.min.y + T));
        let left = Rect::from_min_max(rect.min, pos2(rect.min.x + T, rect.max.y));
        painter.rect_filled(top, 0.0, bg);
        painter.rect_filled(left, 0.0, bg);
        // Pick a tick spacing that leaves ~60px between labels.
        let step = [1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0]
            .into_iter()
            .find(|s| s * self.view.zoom >= 60.0)
            .unwrap_or(10000.0);
        if let Some(b) = self.sel.first().map(|_| self.doc.bounds(&self.sel_roots())).filter(|b| b.is_finite()) {
            let sb = self.view.rect_to_screen(b);
            painter.rect_filled(Rect::from_min_max(pos2(sb.min.x, top.min.y), pos2(sb.max.x, top.max.y)), 0.0, BLUE.gamma_multiply(0.15));
            painter.rect_filled(Rect::from_min_max(pos2(left.min.x, sb.min.y), pos2(left.max.x, sb.max.y)), 0.0, BLUE.gamma_multiply(0.15));
        }
        let (a, b) = (self.view.to_world(rect.min), self.view.to_world(rect.max));
        let mut x = (a.x / step).floor() * step;
        while x <= b.x {
            let sx = self.view.to_screen(pos2(x, 0.0)).x;
            if sx > left.max.x {
                painter.line_segment([pos2(sx, top.max.y - 6.0), pos2(sx, top.max.y)], Stroke::new(1.0, line));
                painter.text(pos2(sx + 3.0, top.min.y + 3.0), egui::Align2::LEFT_TOP, num(x), FontId::proportional(9.0), text);
            }
            x += step;
        }
        let mut y = (a.y / step).floor() * step;
        while y <= b.y {
            let sy = self.view.to_screen(pos2(0.0, y)).y;
            if sy > top.max.y {
                painter.line_segment([pos2(left.max.x - 6.0, sy), pos2(left.max.x, sy)], Stroke::new(1.0, line));
                painter.text(pos2(left.min.x + 3.0, sy + 2.0), egui::Align2::LEFT_TOP, num(y), FontId::proportional(9.0), text);
            }
            y += step;
        }
        painter.line_segment([top.left_bottom(), top.right_bottom()], Stroke::new(1.0, line));
        painter.line_segment([left.right_top(), left.right_bottom()], Stroke::new(1.0, line));
    }

    fn set_cursor(&self, ui: &Ui, ptr: Option<Pos2>, panning: bool) {
        let Some(p) = ptr.filter(|_| ui.ui_contains_pointer()) else { return };
        let guide = match &self.drag {
            Some(Drag::Guide { index }) => Some(*index),
            None => self.guide_at(p),
            _ => None,
        };
        let guide = guide.and_then(|i| self.doc.pages[self.page].guides.get(i)).map(|g| g.axis);
        let icon = if panning {
            if ui.input(|i| i.pointer.any_down()) { CursorIcon::Grabbing } else { CursorIcon::Grab }
        } else if let Some(axis) = guide {
            if axis == Axis::X { CursorIcon::ResizeHorizontal } else { CursorIcon::ResizeVertical }
        } else {
            match self.tool {
                Tool::Move | Tool::Scale if self.gradient_hit(p).is_some() => CursorIcon::Grab,
                Tool::Move | Tool::Scale => match self.handle_at(p) {
                    Some(Grip::Resize(-1, -1) | Grip::Resize(1, 1)) => CursorIcon::ResizeNwSe,
                    Some(Grip::Resize(1, -1) | Grip::Resize(-1, 1)) => CursorIcon::ResizeNeSw,
                    Some(Grip::Resize(_, 0)) => CursorIcon::ResizeHorizontal,
                    Some(Grip::Resize(0, _)) => CursorIcon::ResizeVertical,
                    Some(Grip::Rotate) => CursorIcon::Alias,
                    Some(Grip::Spacing(0)) => CursorIcon::ResizeHorizontal,
                    Some(Grip::Spacing(_)) => CursorIcon::ResizeVertical,
                    Some(Grip::Swap(_)) => CursorIcon::Grab,
                    Some(Grip::Gap) if self.sel.first().is_some_and(|&f| self.doc.node(f).layout.mode == LayoutMode::Vertical) => CursorIcon::ResizeVertical,
                    Some(Grip::Gap | Grip::Padding(1 | 3)) => CursorIcon::ResizeHorizontal,
                    Some(Grip::Padding(_)) => CursorIcon::ResizeVertical,
                    _ => CursorIcon::Default,
                },
                Tool::Text => CursorIcon::Text,
                Tool::Comment => CursorIcon::Cell,
                Tool::Hand => CursorIcon::Grab,
                Tool::Zoom if ui.input(|i| i.modifiers.alt) => CursorIcon::ZoomOut,
                Tool::Zoom => CursorIcon::ZoomIn,
                _ => CursorIcon::Crosshair,
            }
        };
        ui.ctx().set_cursor_icon(icon);
    }

    fn draw_toast(&mut self, painter: &Painter, rect: Rect) {
        let now = painter.ctx().input(|i| i.time);
        let Some((msg, t0)) = &mut self.toast else { return };
        if *t0 == 0.0 {
            *t0 = now;
        }
        if now - *t0 > 3.0 {
            self.toast = None;
            return;
        }
        // Figma's toast: a 40-point near-black card above the toolbar, centered on the canvas.
        let g = painter.layout_no_wrap(msg.clone(), FontId::proportional(12.0), Color32::WHITE);
        let r = Rect::from_center_size(rect.center_bottom() - vec2(0.0, 96.0), vec2(g.size().x + 32.0, 40.0));
        painter.add(egui::Shape::Rect(egui::epaint::RectShape::filled(r.translate(vec2(0.0, 4.0)), CornerRadius::same(10), Color32::from_black_alpha(60)).with_blur_width(16.0)));
        painter.rect(r, CornerRadius::same(10), crate::chrome::MENU_BG, Stroke::new(1.0, Color32::from_gray(0x3A)), StrokeKind::Inside);
        painter.galley(r.center() - g.size() / 2.0, g, Color32::WHITE);
        painter.ctx().request_repaint();
    }

    // ---------- text editing ----------

    fn text_editor(&mut self, ui: &mut Ui, painter: &Painter) {
        let Some(id) = self.editing_text else { return };
        let Some(n) = self.doc.nodes.get(&id).cloned() else {
            self.editing_text = None;
            return;
        };
        let color = n.first_fill().map_or(Color32::BLACK, |c| c.to32());
        let zoom = self.view.zoom;
        let mut text = n.text.clone();
        let (galley, size, _) = crate::text::layout(painter, &n, zoom, color);
        let width = match n.text_sizing {
            crate::doc::TextSizing::AutoWidth => size.x * zoom + n.font_size * zoom,
            _ => n.rect.width() * zoom,
        };
        let local = n.rect.min + crate::text::draw_offset(&n, galley.rows.len());
        let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, _wrap: f32| {
            let g = crate::text::lay(ui.painter(), &n, buf.as_str(), zoom, color);
            crate::text::align_rows(g, &n, zoom)
        };
        for (off, m) in crate::text::markers(&n, &galley, zoom) {
            let g = crate::text::marker_galley(painter, &n, &m, zoom, color);
            painter.galley(self.view.to_screen(local) + off, g, color);
        }
        let out = egui::Area::new(egui::Id::new("canvas-text-edit"))
            .fixed_pos(self.view.to_screen(local))
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::TextEdit::multiline(&mut text)
                    .id(egui::Id::new(TEXT_EDIT_ID))
                    .layouter(&mut layouter)
                    .frame(egui::Frame::NONE)
                    .margin(egui::Margin::ZERO)
                    .desired_width(width)
                    .desired_rows(1)
                    .show(ui)
            })
            .inner;
        if self.focus_text {
            out.response.request_focus();
            self.focus_text = false;
        }
        self.link_popover(ui.ctx(), id, &out.galley, out.galley_pos);
        if text != n.text {
            let node = self.doc.node_mut(id);
            node.spans = crate::doc::shift_spans(&node.spans, &n.text, &text);
            node.lists = crate::doc::shift_lists(&node.lists, &n.text, &text);
            // Figma names a text layer after its text until it's renamed by hand.
            if crate::doc::auto_text_name(&node.name, &n.text) {
                node.name = crate::doc::text_layer_name(&text);
            }
            node.text = text;
            self.dirty = true;
        }
    }

    /// Cmd K while editing: a link field for the selected characters (showing their link, if any).
    pub fn create_link(&mut self, ctx: &egui::Context) {
        let (Some(id), Some((a, b))) = (self.editing_text, self.text_selection(ctx)) else { return };
        let url = crate::doc::link_at(&self.doc.node(id).spans, a).map(|l| l.2).unwrap_or_default();
        self.link_edit = Some((a, b, url));
        self.focus_link = true;
    }

    /// Figma's link popover under the caret: the link there (open, edit, remove), or the field
    /// for the link Cmd K is adding.
    fn link_popover(&mut self, ctx: &egui::Context, id: NodeId, galley: &egui::Galley, origin: Pos2) {
        let Some(caret) = egui::TextEdit::load_state(ctx, egui::Id::new(TEXT_EDIT_ID)).and_then(|s| s.cursor.char_range()).map(|r| r.primary.index) else { return };
        let found = crate::doc::link_at(&self.doc.node(id).spans, caret.0);
        let anchor = self.link_edit.as_ref().map(|e| e.0).or(found.as_ref().map(|l| l.0));
        let Some(anchor) = anchor else { return };
        let at = origin + galley.pos_from_cursor(egui::text::CCursor::new(anchor)).left_bottom().to_vec2() + vec2(0.0, 6.0);
        let (mut apply, mut remove) = (None, None);
        egui::Area::new(egui::Id::new("text-link")).fixed_pos(at).order(egui::Order::Foreground).show(ctx, |ui| {
            crate::chrome::popover_frame(ui).inner_margin(8).show(ui, |ui| {
                if let Some((a, b, url)) = &mut self.link_edit {
                    let r = ui.add(egui::TextEdit::singleline(url).hint_text("Paste or type a link").desired_width(240.0));
                    if std::mem::take(&mut self.focus_link) {
                        r.request_focus();
                    }
                    if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        apply = Some((*a, *b, url.trim().to_string()));
                    }
                } else if let Some((a, b, url)) = &found {
                    ui.horizontal(|ui| {
                        ui.add(egui::Hyperlink::from_label_and_url(url.as_str(), url.as_str()).open_in_new_tab(true));
                        if ui.small_button("Edit").clicked() {
                            self.link_edit = Some((*a, *b, url.clone()));
                            self.focus_link = true;
                        }
                        if ui.small_button("Remove link").clicked() {
                            remove = Some((*a, *b));
                        }
                    });
                }
            });
        });
        // An empty field removes the link; a bare domain gets https, as in Figma.
        let change = apply.map(|(a, b, url)| (a, b, if url.is_empty() || url.contains(':') { url } else { format!("https://{url}") })).or(remove.map(|(a, b)| (a, b, String::new())));
        if let Some((a, b, url)) = change {
            self.checkpoint();
            let n = self.doc.node_mut(id);
            n.spans = crate::doc::style_range(&n.spans, n.text.chars().count(), a, b, &crate::doc::SpanStyle { link: Some(url), ..Default::default() });
            self.link_edit = None;
            self.focus_text = true;
        }
    }

    /// Characters selected in the in-place text editor, if any.
    pub fn text_selection(&self, ctx: &egui::Context) -> Option<(usize, usize)> {
        self.editing_text?;
        let state = egui::TextEdit::load_state(ctx, egui::Id::new(TEXT_EDIT_ID))?;
        let r = state.cursor.char_range()?;
        let (a, b) = (r.primary.index.0, r.secondary.index.0);
        (a != b).then(|| (a.min(b), a.max(b)))
    }

    /// Toggle a list on the paragraphs under the editor's cursor, or every paragraph of the selected text layers.
    pub fn toggle_list(&mut self, ctx: &egui::Context, kind: crate::doc::ListKind) {
        use crate::doc::{ListKind, ListLine};
        let ids: Vec<NodeId> = match self.editing_text {
            Some(id) => vec![id],
            None => self.sel.iter().copied().filter(|&i| self.doc.node(i).kind == Kind::Text).collect(),
        };
        if ids.is_empty() {
            return;
        }
        let range = self.editing_text.and_then(|id| {
            let r = egui::TextEdit::load_state(ctx, egui::Id::new(TEXT_EDIT_ID))?.cursor.char_range()?;
            let para = |i: usize| self.doc.node(id).text.chars().take(i).filter(|c| *c == '\n').count();
            Some((para(r.primary.index.0.min(r.secondary.index.0)), para(r.primary.index.0.max(r.secondary.index.0))))
        });
        self.checkpoint();
        for id in ids {
            let n = self.doc.node_mut(id);
            let count = n.text.matches('\n').count() + 1;
            let (a, b) = range.unwrap_or((0, count - 1));
            n.lists.resize(count.max(n.lists.len()), ListLine::default());
            let on = (a..=b).all(|p| n.lists[p].kind == kind);
            for l in &mut n.lists[a..=b] {
                l.kind = if on { ListKind::Plain } else { kind };
            }
            while n.lists.last().is_some_and(|l| l.kind == ListKind::Plain) {
                n.lists.pop();
            }
        }
    }

    /// Restyle the selected characters of the text being edited. Returns whether it applied.
    pub fn style_selection(&mut self, ctx: &egui::Context, patch: crate::doc::SpanStyle) -> bool {
        let (Some(id), Some((a, b))) = (self.editing_text, self.text_selection(ctx)) else { return false };
        self.begin_edit();
        let n = self.doc.node_mut(id);
        let len = n.text.chars().count();
        n.spans = crate::doc::style_range(&n.spans, len, a, b, &patch);
        true
    }

    /// Figma's text formatting shortcuts, while editing text or with text layers selected.
    /// Returns whether one applied (the caller then drops the key's text and clipboard events).
    pub fn text_shortcut(&mut self, ctx: &egui::Context, key: Key, m: egui::Modifiers, typing: bool) -> bool {
        let Some(f) = text_fmt_key(key, m) else { return false };
        // A panel field with focus keeps its keys; the canvas editor doesn't.
        let editing = self.editing_text.is_some() && (!typing || ctx.memory(|mem| mem.has_focus(egui::Id::new(TEXT_EDIT_ID))));
        let layers = !typing && self.sel.iter().any(|&i| self.doc.node(i).kind == Kind::Text);
        (editing || layers) && self.format_text(ctx, f)
    }

    /// Apply a formatting command to the characters selected in the editor, else to the whole
    /// edited or selected text layers. Toggles follow the first character or layer, as in Figma.
    pub fn format_text(&mut self, ctx: &egui::Context, f: TextFmt) -> bool {
        use crate::doc::{Decoration, LineHeight, SpanStyle};
        let ids: Vec<NodeId> = match self.editing_text {
            Some(id) => vec![id],
            None => self.sel.iter().copied().filter(|&i| self.doc.node(i).kind == Kind::Text).collect(),
        };
        let Some(&first) = ids.first() else { return false };
        let deco = |cur: Decoration, d: Decoration| if cur == d { Decoration::None } else { d };
        if let (Some((a, b)), Some(id)) = (self.text_selection(ctx), self.editing_text) {
            let n = self.doc.node(id);
            let st = n.spans.iter().find(|sp| sp.start <= a && sp.end > a).map(|sp| sp.style.clone()).unwrap_or_default();
            let family = st.font_family.clone().unwrap_or_else(|| n.font_family.clone());
            let weight = st.font_weight.unwrap_or(n.font_weight);
            let patch = match f {
                TextFmt::Bold => Some(SpanStyle { font_weight: Some(bold_toggle(&family, weight)), ..Default::default() }),
                TextFmt::Weight(d) => Some(SpanStyle { font_weight: Some(weight_step(&family, weight, d)), ..Default::default() }),
                TextFmt::Italic => Some(SpanStyle { italic: Some(!st.italic.unwrap_or(n.italic)), ..Default::default() }),
                TextFmt::Underline => Some(SpanStyle { decoration: Some(deco(st.decoration.unwrap_or(n.decoration), Decoration::Underline)), ..Default::default() }),
                TextFmt::Strikethrough => Some(SpanStyle { decoration: Some(deco(st.decoration.unwrap_or(n.decoration), Decoration::Strikethrough)), ..Default::default() }),
                TextFmt::Size(d) => Some(SpanStyle { font_size: Some((st.font_size.unwrap_or(n.font_size) + d).max(1.0)), ..Default::default() }),
                // Alignment, letter spacing and line height belong to the whole layer here.
                _ => None,
            };
            if let Some(p) = patch {
                self.checkpoint();
                let n = self.doc.node_mut(id);
                n.spans = crate::doc::style_range(&n.spans, n.text.chars().count(), a, b, &p);
                return true;
            }
        }
        let n0 = self.doc.node(first).clone();
        self.checkpoint();
        for id in ids {
            let n = self.doc.node_mut(id);
            // Restyling a whole layer replaces that property's per-character overrides.
            let clear = |n: &mut Node, f: fn(&mut SpanStyle)| {
                n.spans.iter_mut().for_each(|sp| f(&mut sp.style));
                n.spans.retain(|sp| !sp.style.is_empty());
            };
            match f {
                TextFmt::Bold => {
                    n.font_weight = bold_toggle(&n.font_family, if n0.font_weight >= 600 { 700 } else { 400 });
                    clear(n, |s| s.font_weight = None);
                }
                TextFmt::Weight(d) => {
                    n.font_weight = weight_step(&n.font_family, n.font_weight, d);
                    clear(n, |s| s.font_weight = None);
                }
                TextFmt::Italic => {
                    n.italic = !n0.italic;
                    clear(n, |s| s.italic = None);
                }
                TextFmt::Underline | TextFmt::Strikethrough => {
                    n.decoration = deco(n0.decoration, if f == TextFmt::Underline { Decoration::Underline } else { Decoration::Strikethrough });
                    clear(n, |s| s.decoration = None);
                }
                TextFmt::Size(d) => {
                    n.font_size = (n.font_size + d).max(1.0);
                    for sp in &mut n.spans {
                        sp.style.font_size = sp.style.font_size.map(|s| (s + d).max(1.0));
                    }
                }
                TextFmt::Align(a) => n.text_align = a,
                TextFmt::Letter(d) => n.letter_spacing += d,
                TextFmt::Line(d) => {
                    n.line_height = match n.line_height {
                        LineHeight::Auto => LineHeight::Px((crate::text::line_pitch(n).round() + d).max(1.0)),
                        LineHeight::Px(v) => LineHeight::Px((v + d).max(1.0)),
                        LineHeight::Percent(p) => LineHeight::Percent((p + d).max(1.0)),
                    }
                }
            }
            // Changing a styled value by hand detaches the text style, like the panel.
            if !matches!(f, TextFmt::Align(_)) {
                n.text_style = None;
            }
        }
        true
    }

    pub fn context_menu(&mut self, ui: &mut Ui) {
        let has = !self.sel.is_empty();
        let ctx = ui.ctx().clone();
        let item = |ui: &mut Ui, label: &str, short: &str, enabled: bool| {
            ui.add_enabled(enabled, egui::Button::new(label).shortcut_text(crate::chrome::keys(short))).clicked()
        };
        if item(ui, "Copy", "⌘C", has) {
            self.copy(&ctx);
            ui.close();
        }
        if item(ui, "Paste", "⌘V", true) {
            self.paste_from_menu(&ctx, false);
            ui.close();
        }
        ui.add_enabled_ui(has, |ui| {
            ui.menu_button("Copy/Paste as", |ui| {
                for (label, short, what) in [("Copy as PNG", "⇧⌘C", "png"), ("Copy as SVG", "", "svg"), ("Copy as CSS", "", "css"), ("Copy as code", "", "code")] {
                    if ui.add(egui::Button::new(label).shortcut_text(crate::chrome::keys(short))).clicked() {
                        self.copy_as(&ctx, what);
                        ui.close();
                    }
                }
                ui.separator();
                if ui.add(egui::Button::new("Copy properties").shortcut_text(crate::chrome::keys("⌥⌘C"))).clicked() {
                    self.style_clip = self.sel.first().map(|&id| self.doc.node(id).clone());
                    ui.close();
                }
                if ui.add_enabled(self.style_clip.is_some(), egui::Button::new("Paste properties").shortcut_text(crate::chrome::keys("⌥⌘V"))).clicked() {
                    self.paste_style();
                    ui.close();
                }
                if ui.add(egui::Button::new("Paste over selection").shortcut_text(crate::chrome::keys("⇧⌘V"))).clicked() {
                    self.paste_from_menu(&ctx, true);
                    ui.close();
                }
                if ui.add(egui::Button::new("Paste to replace").shortcut_text(crate::chrome::keys("⇧⌘R"))).clicked() {
                    self.paste_to_replace();
                    ui.close();
                }
            });
        });
        ui.separator();
        let under = self.menu_at.map(|p| self.layers_at(p)).unwrap_or_default();
        if under.len() > 1 {
            ui.menu_button("Select layer", |ui| {
                for id in under.into_iter().take(16) {
                    let n = self.doc.node(id);
                    let depth = self.doc.ancestors(id).len();
                    if ui.selectable_label(self.sel == [id], format!("{}{}", "   ".repeat(depth.min(6)), n.name)).clicked() {
                        self.sel = vec![id];
                        ui.close();
                    }
                }
            });
        }
        if item(ui, "Select matching layers", "⌥⌘A", has) {
            self.select_matching();
            ui.close();
        }
        if item(ui, "Duplicate", "⌘D", has) {
            self.duplicate_selection();
            ui.close();
        }
        if item(ui, "Delete", "⌫", has) {
            self.delete_selection();
            ui.close();
        }
        ui.separator();
        if item(ui, "Bring to front", "⌥⌘]", has) {
            self.reorder_selection(Order::Front);
            ui.close();
        }
        if item(ui, "Bring forward", "⌘]", has) {
            self.reorder_selection(Order::Forward);
            ui.close();
        }
        if item(ui, "Send backward", "⌘[", has) {
            self.reorder_selection(Order::Backward);
            ui.close();
        }
        if item(ui, "Send to back", "⌥⌘[", has) {
            self.reorder_selection(Order::Back);
            ui.close();
        }
        ui.separator();
        if item(ui, "Group selection", "⌘G", has) {
            self.group_selection(false);
            ui.close();
        }
        let multi = self.sel_roots().len() > 1 || self.sel.iter().any(|&s| self.doc.node(s).kind == Kind::Boolean);
        ui.add_enabled_ui(multi, |ui| {
            ui.menu_button("Boolean", |ui| {
                for (op, short) in [
                    (crate::doc::BoolOp::Union, "⌥⇧U"),
                    (crate::doc::BoolOp::Subtract, "⌥⇧S"),
                    (crate::doc::BoolOp::Intersect, "⌥⇧I"),
                    (crate::doc::BoolOp::Exclude, "⌥⇧E"),
                ] {
                    if ui.add(egui::Button::new(format!("{op:?} selection")).shortcut_text(crate::chrome::keys(short))).clicked() {
                        self.boolean_selection(op);
                        ui.close();
                    }
                }
            });
        });
        let frames = has && self.sel.iter().all(|&s| self.doc.node(s).kind.is_frame() && self.doc.node(s).kind != Kind::Instance);
        if frames {
            let ready = self.doc.node(self.sel[0]).ready;
            if item(ui, if ready { "Mark as changed" } else { "Mark as ready for dev" }, "", true) {
                self.toggle_ready_for_dev();
                ui.close();
            }
        }
        let comps = self.sel.len() > 1 && self.sel.iter().all(|&s| self.doc.node(s).kind == Kind::Component);
        if comps && item(ui, "Combine as variants", "", true) {
            self.combine_as_variants();
            ui.close();
        }
        let masked = self.sel.first().is_some_and(|&s| self.doc.node(s).is_mask);
        if item(ui, if masked { "Remove mask" } else { "Use as mask" }, "⌃⌘M", has) {
            self.toggle_mask();
            ui.close();
        }
        if item(ui, "Outline stroke", "⌥⌘O", has) {
            self.outline_stroke_selection();
            ui.close();
        }
        if self.vector_edit.is_some() && item(ui, "Join selection", "⌘J", self.vector_edit.as_ref().is_some_and(|ve| ve.selected.len() == 2)) {
            self.vector_join();
            ui.close();
        }
        if item(ui, "Flatten", "⌘E", has) {
            self.flatten_selection();
            ui.close();
        }
        if item(ui, "Frame selection", "⌥⌘G", has) {
            self.group_selection(true);
            ui.close();
        }
        if item(ui, "Add auto layout", "⇧A", has) {
            self.add_auto_layout();
            ui.close();
        }
        let container = self.sel.iter().any(|&s| self.doc.node(s).kind.is_container());
        if item(ui, "Ungroup", "⇧⌘G", container) {
            self.ungroup_selection();
            ui.close();
        }
        ui.separator();
        let one = (self.sel.len() == 1).then(|| self.sel[0]);
        let kind = one.map(|id| self.doc.node(id).kind);
        if kind != Some(Kind::Component) && item(ui, "Create component", "⌥⌘K", has) {
            self.create_component();
            ui.close();
        }
        if kind == Some(Kind::Component) {
            if item(ui, "Add variant", "", true) {
                self.add_variant_to(one.unwrap());
                ui.close();
            }
            if item(ui, "Create instance", "", true) {
                self.duplicate_selection();
                ui.close();
            }
        }
        if let Some(inst) = one.and_then(|id| self.owning_instance(id)) {
            if item(ui, "Go to main component", "", true) {
                self.go_to_main(inst);
                ui.close();
            }
            ui.menu_button("Swap instance", |ui| {
                if self.swap_menu(ui, inst) {
                    ui.close();
                }
            });
            self.instance_menu(ui, inst);
            if item(ui, "Detach instance", "⌥⌘B", true) {
                self.checkpoint();
                self.doc.detach_instance(inst);
                ui.close();
            }
        }
        ui.separator();
        if item(ui, "Show/Hide", "⇧⌘H", has) {
            self.toggle_visible();
            ui.close();
        }
        if item(ui, "Lock/Unlock", "⇧⌘L", has) {
            self.toggle_locked();
            ui.close();
        }
        if has {
            ui.separator();
            if item(ui, "Flip horizontal", "⇧H", true) {
                self.flip_selection(true);
                ui.close();
            }
            if item(ui, "Flip vertical", "⇧V", true) {
                self.flip_selection(false);
                ui.close();
            }
            let pages: Vec<(usize, String)> = self.doc.pages.iter().enumerate().filter(|(i, p)| *i != self.page && !crate::panels::is_divider(&p.name)).map(|(i, p)| (i, p.name.clone())).collect();
            if !pages.is_empty() {
                ui.menu_button("Move to page", |ui| {
                    for (i, name) in pages {
                        if ui.button(name).clicked() {
                            self.move_to_page(i);
                            ui.close();
                        }
                    }
                });
            }
        } else {
            // Right-clicking empty canvas: page-wide commands, as in Figma.
            if item(ui, "Lock all", "", true) {
                self.lock_all(true);
                ui.close();
            }
            if item(ui, "Unlock all", "", true) {
                self.lock_all(false);
                ui.close();
            }
            if item(ui, if self.ui_hidden { "Show UI" } else { "Hide UI" }, "⌘\\", true) {
                self.ui_hidden = !self.ui_hidden;
                ui.close();
            }
            if item(ui, if self.show_comments { "Hide comments" } else { "Show comments" }, "⇧C", true) {
                self.show_comments = !self.show_comments;
                ui.close();
            }
        }
        if !self.doc.pages[self.page].guides.is_empty() {
            ui.separator();
            if item(ui, "Remove all guides", "", true) {
                self.remove_all_guides();
                ui.close();
            }
        }
    }
}

/// An auto layout frame's padding bands and the gaps between its flowing children, each with its
/// size (world space; empty for other layers and rotated frames).
pub fn spacing_bands(doc: &crate::doc::Document, id: NodeId) -> Vec<(Rect, f32)> {
    let n = doc.node(id);
    if n.layout.mode == crate::doc::LayoutMode::None || n.rotation != 0.0 {
        return vec![];
    }
    let r = n.rect;
    let [t, rt, b, l] = n.layout.padding;
    let mut bands: Vec<(Rect, f32)> = vec![
        (Rect::from_min_max(r.min, pos2(r.max.x, r.min.y + t)), t),
        (Rect::from_min_max(pos2(r.max.x - rt, r.min.y), r.max), rt),
        (Rect::from_min_max(pos2(r.min.x, r.max.y - b), r.max), b),
        (Rect::from_min_max(r.min, pos2(r.min.x + l, r.max.y)), l),
    ];
    let horizontal = n.layout.mode == crate::doc::LayoutMode::Horizontal;
    let kids: Vec<Rect> = n.children.iter().map(|c| doc.node(*c)).filter(|c| c.visible && !c.absolute).map(|c| c.aabb()).collect();
    let inner = Rect::from_min_max(r.min + vec2(l, t), r.max - vec2(rt, b));
    for w in kids.windows(2) {
        let gap = if horizontal {
            Rect::from_min_max(pos2(w[0].max.x, inner.min.y), pos2(w[1].min.x, inner.max.y))
        } else {
            Rect::from_min_max(pos2(inner.min.x, w[0].max.y), pos2(inner.max.x, w[1].min.y))
        };
        bands.push((gap, if horizontal { gap.width() } else { gap.height() }));
    }
    bands.retain(|(b, s)| *s > 0.0 && b.is_positive());
    bands
}

/// A small measurement label centered at `at`: white text on `fill`.
fn pill(painter: &Painter, at: Pos2, text: String, fill: Color32) {
    let g = painter.layout_no_wrap(text, FontId::proportional(11.0), Color32::WHITE);
    let r = Rect::from_center_size(at, g.size() + vec2(8.0, 4.0));
    painter.rect_filled(r, 2.0, fill);
    painter.galley(r.min + vec2(4.0, 2.0), g, Color32::WHITE);
}

/// A text formatting command (Figma's Text menu and its shortcuts).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TextFmt {
    Bold,
    Italic,
    Underline,
    Strikethrough,
    Align(crate::doc::TextAlign),
    /// Font size, in px.
    Size(f32),
    /// Steps through the family's installed weights.
    Weight(i32),
    /// Letter spacing, in percent.
    Letter(f32),
    /// Line height, in px (or percent when set as one).
    Line(f32),
}

/// Figma's Text menu, in groups: command, label, shortcut. Also searchable in quick actions.
pub const TEXT_MENU: &[&[(TextFmt, &str, &str)]] = {
    use crate::doc::TextAlign;
    &[
        &[(TextFmt::Bold, "Bold", "⌘B"), (TextFmt::Italic, "Italic", "⌘I"), (TextFmt::Underline, "Underline", "⌘U"), (TextFmt::Strikethrough, "Strikethrough", "⇧⌘X")],
        &[
            (TextFmt::Align(TextAlign::Left), "Text align left", "⌥⌘L"),
            (TextFmt::Align(TextAlign::Center), "Text align center", "⌥⌘T"),
            (TextFmt::Align(TextAlign::Right), "Text align right", "⌥⌘R"),
            (TextFmt::Align(TextAlign::Justify), "Text align justified", "⌥⌘J"),
        ],
        &[
            (TextFmt::Size(1.0), "Increase font size", "⇧⌘>"),
            (TextFmt::Size(-1.0), "Decrease font size", "⇧⌘<"),
            (TextFmt::Weight(1), "Increase font weight", "⌥⌘>"),
            (TextFmt::Weight(-1), "Decrease font weight", "⌥⌘<"),
            (TextFmt::Letter(1.0), "Increase letter spacing", "⌥>"),
            (TextFmt::Letter(-1.0), "Decrease letter spacing", "⌥<"),
            (TextFmt::Line(1.0), "Increase line height", "⌥⇧>"),
            (TextFmt::Line(-1.0), "Decrease line height", "⌥⇧<"),
        ],
    ]
};

/// Figma's text shortcuts. "<" and ">" are the comma and period keys.
pub fn text_fmt_key(key: Key, m: egui::Modifiers) -> Option<TextFmt> {
    use crate::doc::TextAlign;
    if m.ctrl && !m.command {
        return None;
    }
    Some(match (m.command, m.shift, m.alt, key) {
        (true, false, false, Key::B) => TextFmt::Bold,
        (true, false, false, Key::I) => TextFmt::Italic,
        (true, false, false, Key::U) => TextFmt::Underline,
        (true, true, false, Key::X) => TextFmt::Strikethrough,
        (true, false, true, Key::L) => TextFmt::Align(TextAlign::Left),
        (true, false, true, Key::T) => TextFmt::Align(TextAlign::Center),
        (true, false, true, Key::R) => TextFmt::Align(TextAlign::Right),
        (true, false, true, Key::J) => TextFmt::Align(TextAlign::Justify),
        (true, true, false, Key::Period) => TextFmt::Size(1.0),
        (true, true, false, Key::Comma) => TextFmt::Size(-1.0),
        (true, false, true, Key::Period) => TextFmt::Weight(1),
        (true, false, true, Key::Comma) => TextFmt::Weight(-1),
        (false, false, true, Key::Period) => TextFmt::Letter(1.0),
        (false, false, true, Key::Comma) => TextFmt::Letter(-1.0),
        (false, true, true, Key::Period) => TextFmt::Line(1.0),
        (false, true, true, Key::Comma) => TextFmt::Line(-1.0),
        _ => return None,
    })
}

/// The installed weight nearest `w`.
fn nearest_weight(family: &str, w: u16) -> u16 {
    crate::text::weights_for(family).into_iter().min_by_key(|x| (*x as i32 - w as i32).abs()).unwrap_or(w)
}

/// Cmd B: bold text goes back to regular, anything lighter to bold.
fn bold_toggle(family: &str, w: u16) -> u16 {
    nearest_weight(family, if w >= 600 { 400 } else { 700 })
}

/// The next installed weight up or down.
fn weight_step(family: &str, w: u16, d: i32) -> u16 {
    let ws = crate::text::weights_for(family);
    let cur = nearest_weight(family, w);
    let i = ws.iter().position(|x| *x == cur).unwrap_or(0) as i32;
    ws.get((i + d).clamp(0, ws.len() as i32 - 1) as usize).copied().unwrap_or(w)
}

/// Everything that affects a text layer's measured size, hashed.
fn text_key(ctx: &egui::Context, n: &Node) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::hash::DefaultHasher::new();
    n.text.hash(&mut h);
    format!("{:?}", crate::text::font_family(ctx, &n.font_family, n.font_weight, n.italic)).hash(&mut h);
    (n.font_size.to_bits(), n.letter_spacing.to_bits(), n.text_sizing as u8, n.text_case as u8, n.italic).hash(&mut h);
    format!("{:?}", n.line_height).hash(&mut h);
    (n.truncate, n.max_lines, n.paragraph_spacing.to_bits(), n.paragraph_indent.to_bits(), n.vertical_trim).hash(&mut h);
    format!("{:?}", n.lists).hash(&mut h);
    if n.truncate && n.text_sizing == crate::doc::TextSizing::Fixed {
        n.rect.height().to_bits().hash(&mut h);
    }
    if n.text_sizing != crate::doc::TextSizing::AutoWidth {
        n.rect.width().to_bits().hash(&mut h);
    }
    if !n.spans.is_empty() {
        format!("{:?}", n.spans).hash(&mut h);
    }
    h.finish()
}

/// Scale-tool property scaling: everything measured in pixels grows with the layer.
fn scale_props(n: &mut Node, k: f32) {
    n.stroke_width *= k;
    n.radius *= k;
    if let Some(c) = &mut n.corner_radii {
        c.iter_mut().for_each(|r| *r *= k);
    }
    n.font_size *= k;
    if let crate::doc::LineHeight::Px(px) = &mut n.line_height {
        *px *= k;
    }
    for s in &mut n.shadows {
        s.blur *= k;
        s.spread *= k;
        s.offset *= k;
    }
    n.layout.gap *= k;
    n.layout.padding.iter_mut().for_each(|p| *p *= k);
    n.dash.iter_mut().for_each(|d| *d *= k);
}

/// Freehand points -> fewer anchors (Ramer-Douglas-Peucker) with smooth Catmull-Rom handles.
pub fn smooth_stroke(points: &[Pos2], tol: f32) -> Vec<PathPoint> {
    fn rdp(p: &[Pos2], tol: f32, out: &mut Vec<Pos2>) {
        let (a, b) = (p[0], p[p.len() - 1]);
        let (mut idx, mut dmax) = (0, 0.0);
        for (i, q) in p.iter().enumerate().take(p.len() - 1).skip(1) {
            let ab = b - a;
            let d = if ab.length_sq() > 0.0 { (ab.x * (a.y - q.y) - ab.y * (a.x - q.x)).abs() / ab.length() } else { a.distance(*q) };
            if d > dmax {
                (idx, dmax) = (i, d);
            }
        }
        if dmax > tol {
            rdp(&p[..=idx], tol, out);
            out.pop();
            rdp(&p[idx..], tol, out);
        } else {
            out.push(a);
            out.push(b);
        }
    }
    if points.len() < 2 {
        return vec![];
    }
    let mut simple = vec![];
    rdp(points, tol, &mut simple);
    simple.dedup();
    let n = simple.len();
    (0..n)
        .map(|i| {
            let p = simple[i];
            if i == 0 || i + 1 == n {
                return PathPoint { p, ..Default::default() };
            }
            let t = (simple[i + 1] - simple[i - 1]) / 6.0;
            PathPoint { p, h_in: -t, h_out: t }
        })
        .collect()
}

/// The (first) gap between consecutive `items` along axis `a`.
fn gap_of(items: &[(NodeId, Rect)], a: usize) -> f32 {
    items.get(1).map_or(0.0, |n| n.1.min[a] - items[0].1.max[a])
}

/// A vector of length `v` along axis `a`.
fn axis_vec(a: usize, v: f32) -> Vec2 {
    let mut d = Vec2::ZERO;
    d[a] = v;
    d
}

/// Space `items` (in order, from their recorded rects) `g` apart along axis `a`, starting at `from`.
fn lay_out(doc: &mut crate::doc::Document, a: usize, items: &[(NodeId, Rect)], from: f32, g: f32) {
    let mut cursor = from;
    for (id, r) in items {
        doc.translate(*id, axis_vec(a, cursor - r.min[a]));
        cursor += r.max[a] - r.min[a] + g;
    }
    doc.reflow();
}

/// Whether `r` and `s` overlap across axis `a`: in the same row (a = 0) or column (a = 1).
fn across(r: Rect, s: Rect, a: usize) -> bool {
    let b = 1 - a;
    r.min[b] < s.max[b] && s.min[b] < r.max[b]
}

/// Adjacent pairs along axis `a` with empty space between them: each rect and its nearest
/// neighbor after it in the same row or column.
fn neighbors(rs: &[Rect], a: usize) -> Vec<(Rect, Rect)> {
    rs.iter()
        .filter_map(|p| {
            let q = rs.iter().filter(|q| q.min[a] > p.max[a] && across(*p, **q, a)).min_by(|x, y| x.min[a].total_cmp(&y.min[a]))?;
            Some((*p, *q))
        })
        .collect()
}

/// Equal-spacing snaps for `m` along axis `a`: corrections that put a masked edge one existing
/// gap (between neighbors in its row) away from the nearest neighbor, or center `m` between
/// its two neighbors. Each is (correction, gap).
fn gap_snaps(m: Rect, sibs: &[Rect], a: usize, mask: [bool; 3]) -> Vec<(f32, f32)> {
    let row: Vec<Rect> = sibs.iter().copied().filter(|s| across(m, *s, a)).collect();
    let gaps: Vec<f32> = neighbors(&row, a).iter().map(|(p, q)| q.min[a] - p.max[a]).collect();
    let c = m.center()[a];
    let before = row.iter().filter(|s| s.max[a] <= c).max_by(|x, y| x.max[a].total_cmp(&y.max[a]));
    let after = row.iter().filter(|s| s.min[a] >= c).min_by(|x, y| x.min[a].total_cmp(&y.min[a]));
    let mut out = vec![];
    if let (true, Some(l)) = (mask[0], before) {
        out.extend(gaps.iter().map(|g| (l.max[a] + g - m.min[a], *g)));
    }
    if let (true, Some(r)) = (mask[2], after) {
        out.extend(gaps.iter().map(|g| (r.min[a] - g - m.max[a], *g)));
    }
    if let (true, Some(l), Some(r)) = (mask[1], before, after) {
        let g = (r.min[a] - l.max[a] - (m.max[a] - m.min[a])) / 2.0;
        if g > 0.0 {
            out.push((l.max[a] + g - m.min[a], g));
        }
    }
    out
}

/// Every gap of size `g` along axis `a` in `m`'s row, as world segments across the gap.
fn equal_gaps(m: Rect, sibs: &[Rect], a: usize, g: f32) -> Vec<[Pos2; 2]> {
    let mut row: Vec<Rect> = sibs.iter().copied().filter(|s| across(m, *s, a)).collect();
    row.push(m);
    let b = 1 - a;
    neighbors(&row, a)
        .into_iter()
        .filter(|(p, q)| (q.min[a] - p.max[a] - g).abs() < 0.5)
        .map(|(p, q)| {
            let mut s = p.max;
            s[b] = (p.min[b].max(q.min[b]) + p.max[b].min(q.max[b])) / 2.0;
            let mut e = s;
            e[a] = q.min[a];
            [s, e]
        })
        .collect()
}

/// Where `s` projects onto segment a-b, 0 at a and 1 at b.
fn line_t(a: Pos2, b: Pos2, s: Pos2) -> f32 {
    ((s - a).dot(b - a) / (b - a).length_sq().max(1e-6)).clamp(0.0, 1.0)
}

fn snap_angle(from: Pos2, to: Pos2) -> Pos2 {
    let d = to - from;
    let a = (d.y.atan2(d.x) / std::f32::consts::FRAC_PI_4).round() * std::f32::consts::FRAC_PI_4;
    from + Vec2::angled(a) * d.length()
}
