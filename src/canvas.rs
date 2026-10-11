//! Infinite canvas: navigation, tools, selection, snapping, and on-canvas overlays.

use crate::app::{App, Drag, Grip, Press, Tool};
use crate::doc::{Kind, LayoutMode, Node, NodeId, Order, PathPoint, Sizing, rotate};
use crate::render::{self, View};
use egui::{
    Color32, CornerRadius, CursorIcon, FontId, Key, Painter, PointerButton, Pos2, Rect, Sense, Stroke, StrokeKind,
    Ui, Vec2, pos2, vec2,
};

const TEXT_EDIT_ID: &str = "katachi-text-edit";

pub const BLUE: Color32 = Color32::from_rgb(0x0D, 0x99, 0xFF);
pub const PURPLE: Color32 = Color32::from_rgb(0x97, 0x47, 0xFF);
const SNAP: Color32 = Color32::from_rgb(0xF2, 0x48, 0x22);

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
            self.view = if kids.is_empty() {
                View { zoom: 1.0, pan: rect.center().to_vec2() }
            } else {
                View::fit(self.doc.bounds(&kids), rect, 1.0)
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
                && self.handle_at(s).is_none() {
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
        if resp.clicked()
            && let Some(w) = world {
                self.on_click(w, mods);
            }
        if resp.double_clicked() && self.tool == Tool::Move
            && let Some(w) = world {
                self.on_double_click(w);
            }
        if let (Some(drop), Some(p)) = (resp.dnd_release_payload::<crate::panels::AssetDrag>(), hover_pos) {
            // Instance dropped from the Assets tab, centered under the pointer.
            if self.doc.nodes.contains_key(&drop.0) {
                self.checkpoint();
                let w = self.view.to_world(p);
                let size = self.doc.node(drop.0).rect.size();
                let parent = self.frame_at(w, &[]);
                let i = self.doc.instantiate(drop.0, self.page, parent, None, (w - size / 2.0).round());
                self.sel = vec![i];
            }
        }
        resp.context_menu(|ui| self.context_menu(ui));

        self.text_editor(ui, &painter);
        self.draw_overlays(&painter, ptr);
        // Dev Mode shows distances on hover; design mode needs Alt.
        if (mods.alt || self.dev_mode) && self.drag.is_none() {
            self.draw_measurements(&painter);
        }
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
    fn accept_drops(&mut self, ui: &Ui, at: Pos2) {
        let files = ui.input(|i| i.raw.dropped_files.clone());
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
            let id = if is_svg { self.import_svg(&bytes, &name, w, true) } else { self.place_image(bytes, &name, w) };
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
            crate::text::ensure_font(&ctx, &n.font_family, n.font_weight);
            for sp in &n.spans {
                let fam = sp.style.font_family.as_deref().unwrap_or(&n.font_family);
                crate::text::ensure_font(&ctx, fam, sp.style.font_weight.unwrap_or(n.font_weight));
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
        let corner = [b.left_top(), b.right_top(), b.left_bottom(), b.right_bottom()]
            .iter()
            .any(|c| c.distance(s) < 20.0);
        (corner && !b.expand(5.0).contains(s)).then_some(Grip::Rotate)
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
        if tool == Tool::Move && self.vector_edit.is_some() && self.vector_press(s, mods.shift) {
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
            Tool::Pen => {
                let close = self.pen.len() >= 2 && self.view.to_screen(self.pen[0].p).distance(s) < 8.0;
                if close {
                    self.finish_pen(true);
                } else {
                    let mut p = w;
                    if let (true, Some(last)) = (mods.shift, self.pen.last()) {
                        p = snap_angle(last.p, p);
                    }
                    self.pen.push(PathPoint { p: p.round(), ..Default::default() });
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
        let start = press.world;
        if self.vector_edit.is_some() {
            self.drag = self.vector_drag_start();
            self.vector_start = start;
            return;
        }
        if let Some(from) = self.connecting.take() {
            self.drag = Some(Drag::Connect { from });
            return;
        }
        self.drag = match self.tool {
            Tool::Move | Tool::Scale => {
                if let Some(grip) = press.handle {
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
            t if t.draws_shape() || t == Tool::Text => {
                self.checkpoint();
                let start = self.snap_point(start.round(), &[]);
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
                let d = (b.min + d).round() - b.min;
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
                let nb = Rect::from_two_pos(nb.min.round(), nb.max.round());
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
                let mut b = self.snap_point(w, &[*id]).round();
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
            Drag::Pan | Drag::Connect { .. } => {}
        }
        self.drag = Some(drag);
    }

    fn on_drag_end(&mut self, w: Pos2) {
        self.guides.clear();
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
            Some(Drag::Pencil { points }) => {
                let pts = smooth_stroke(&points, 1.5 / self.view.zoom);
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
                self.tool = Tool::Move;
            }
            _ => {}
        }
    }

    fn on_click(&mut self, w: Pos2, mods: egui::Modifiers) {
        if self.vector_edit.is_some() {
            self.press = None;
            return;
        }
        match self.tool {
            Tool::Move | Tool::Scale => {
                if let Some(t) = self.pending_frame.take() {
                    if mods.shift { self.sel.push(t) } else { self.sel = vec![t] }
                }
                if let Some(p) = self.press.take() {
                    match p.target {
                        Some(t) if p.toggle_off => self.sel.retain(|s| *s != t),
                        Some(t) if !mods.shift => self.sel = vec![t],
                        _ => {}
                    }
                }
            }
            Tool::Text => {
                // Created on release so the new field keeps keyboard focus.
                self.checkpoint();
                let parent = self.frame_at(w, &[]);
                let n = Node::new(Kind::Text, Rect::from_min_size(w.round(), Vec2::ZERO));
                let id = self.doc.insert(self.page, parent, None, n);
                self.sel = vec![id];
                self.editing_text = Some(id);
                self.focus_text = true;
                self.tool = Tool::Move;
            }
            t if t.draws_shape() => {
                // A click without a drag drops a default 100×100 shape.
                self.checkpoint();
                let p = w.round();
                let parent = if t == Tool::Section { None } else { self.frame_at(p, &[]) };
                let node = self.shape_for(t, Rect::from_min_size(p, vec2(100.0, 100.0)));
                let id = self.doc.insert(self.page, parent, None, node);
                self.sel = vec![id];
                self.tool = Tool::Move;
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
            && matches!(self.doc.node(one).kind, Kind::Vector | Kind::Rectangle | Kind::Ellipse)
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
            let center = k.aabb().center();
            let before = if h { center.x < w.x } else { center.y < w.y };
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

    fn snap_targets(&self, exclude: &[NodeId]) -> Vec<Rect> {
        let parent = exclude.first().and_then(|&e| self.doc.nodes.get(&e)).and_then(|n| n.parent);
        let mut out: Vec<Rect> = self
            .doc
            .children_of(self.page, parent)
            .iter()
            .filter(|c| !exclude.contains(c))
            .map(|c| self.doc.node(*c))
            .filter(|n| n.visible)
            .map(|n| n.aabb())
            .collect();
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
                        crate::doc::GridKind::Grid => {}
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

    /// Correction that snaps the masked edges of `moving` to nearby sibling edges/centers.
    /// Records guide lines for display.
    fn snap(&mut self, moving: Rect, exclude: &[NodeId], mx: [bool; 3], my: [bool; 3]) -> Vec2 {
        let tol = 5.0 / self.view.zoom;
        let targets = self.snap_targets(exclude);
        let xs = |r: Rect| [r.min.x, r.center().x, r.max.x];
        let ys = |r: Rect| [r.min.y, r.center().y, r.max.y];
        let best = |m: [f32; 3], mask: [bool; 3], t: &dyn Fn(Rect) -> [f32; 3]| {
            let mut best: Option<f32> = None;
            for r in &targets {
                for (_, mv) in m.iter().enumerate().filter(|(i, _)| mask[*i]) {
                    for tv in t(*r) {
                        let d = tv - mv;
                        if d.abs() < tol && best.is_none_or(|b| d.abs() < b.abs()) {
                            best = Some(d);
                        }
                    }
                }
            }
            best
        };
        let corr = vec2(best(xs(moving), mx, &xs).unwrap_or(0.0), best(ys(moving), my, &ys).unwrap_or(0.0));
        let m = moving.translate(corr);
        self.guides.clear();
        for r in &targets {
            for (i, mv) in xs(m).iter().enumerate() {
                if mx[i] && xs(*r).iter().any(|t| (t - mv).abs() < 0.01) {
                    self.guides.push([pos2(*mv, m.min.y.min(r.min.y)), pos2(*mv, m.max.y.max(r.max.y))]);
                }
            }
            for (i, mv) in ys(m).iter().enumerate() {
                if my[i] && ys(*r).iter().any(|t| (t - mv).abs() < 0.01) {
                    self.guides.push([pos2(m.min.x.min(r.min.x), *mv), pos2(m.max.x.max(r.max.x), *mv)]);
                }
            }
        }
        corr
    }

    fn snap_point(&mut self, p: Pos2, exclude: &[NodeId]) -> Pos2 {
        let r = Rect::from_min_size(p, Vec2::ZERO);
        p + self.snap(r, exclude, [true, false, false], [true, false, false])
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
                if let crate::doc::Action::Navigate(t) | crate::doc::Action::OpenOverlay(t) = r.action
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
        if let Some(h) = self.hover.filter(|h| !self.sel.contains(h)) {
            self.outline(painter, h, 2.0);
        }
        if self.vector_edit.is_some() {
            self.draw_vector_edit(painter);
            self.draw_pen(painter, ptr);
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
            let g = painter.layout_no_wrap(label, FontId::proportional(11.0), Color32::WHITE);
            let pill = Rect::from_center_size(b.center_bottom() + vec2(0.0, 8.0 + g.size().y / 2.0 + 2.0), g.size() + vec2(8.0, 4.0));
            painter.rect_filled(pill, 2.0, accent);
            painter.galley(pill.min + vec2(4.0, 2.0), g, Color32::WHITE);
        }
        for g in &self.guides {
            painter.line_segment([self.view.to_screen(g[0]), self.view.to_screen(g[1])], Stroke::new(1.0, SNAP));
        }
        if let (Some(Drag::Marquee { start, .. }), Some(p)) = (&self.drag, ptr) {
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

    /// Figma shows a pixel grid from 800% zoom.
    fn draw_pixel_grid(&self, painter: &Painter, rect: Rect) {
        if self.view.zoom < 8.0 {
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
            let g = painter.layout_no_wrap(num(d), FontId::proportional(11.0), Color32::WHITE);
            let pill = Rect::from_center_size(sa.lerp(sb, 0.5), g.size() + vec2(8.0, 4.0));
            painter.rect_filled(pill, 2.0, red);
            painter.galley(pill.min + vec2(4.0, 2.0), g, Color32::WHITE);
        }
    }

    /// Shift+R: top and left rulers with the selection's span highlighted.
    fn draw_rulers(&self, painter: &Painter, rect: Rect) {
        const T: f32 = 20.0;
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
        let icon = if panning {
            if ui.input(|i| i.pointer.any_down()) { CursorIcon::Grabbing } else { CursorIcon::Grab }
        } else {
            match self.tool {
                Tool::Move | Tool::Scale => match self.handle_at(p) {
                    Some(Grip::Resize(-1, -1) | Grip::Resize(1, 1)) => CursorIcon::ResizeNwSe,
                    Some(Grip::Resize(1, -1) | Grip::Resize(-1, 1)) => CursorIcon::ResizeNeSw,
                    Some(Grip::Resize(_, 0)) => CursorIcon::ResizeHorizontal,
                    Some(Grip::Resize(0, _)) => CursorIcon::ResizeVertical,
                    Some(Grip::Rotate) => CursorIcon::Alias,
                    _ => CursorIcon::Default,
                },
                Tool::Text => CursorIcon::Text,
                Tool::Comment => CursorIcon::Cell,
                Tool::Hand => CursorIcon::Grab,
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
        let g = painter.layout_no_wrap(msg.clone(), FontId::proportional(12.0), Color32::WHITE);
        let r = Rect::from_center_size(rect.center_bottom() - vec2(0.0, 80.0), g.size() + vec2(24.0, 14.0));
        painter.rect_filled(r, CornerRadius::same(6), Color32::from_gray(0x1E));
        painter.galley(r.min + vec2(12.0, 7.0), g, Color32::WHITE);
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
            let g = ui.painter().layout_job(crate::text::job(ui.ctx(), &n, buf.as_str(), zoom, color));
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
                ui.add(
                    egui::TextEdit::multiline(&mut text)
                        .id(egui::Id::new(TEXT_EDIT_ID))
                        .layouter(&mut layouter)
                        .frame(egui::Frame::NONE)
                        .margin(egui::Margin::ZERO)
                        .desired_width(width)
                        .desired_rows(1),
                )
            })
            .inner;
        if self.focus_text {
            out.request_focus();
            self.focus_text = false;
        }
        if text != n.text {
            let node = self.doc.node_mut(id);
            node.spans = crate::doc::shift_spans(&node.spans, &n.text, &text);
            node.lists = crate::doc::shift_lists(&node.lists, &n.text, &text);
            node.text = text;
            self.dirty = true;
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

    fn context_menu(&mut self, ui: &mut Ui) {
        let has = !self.sel.is_empty();
        let ctx = ui.ctx().clone();
        let item = |ui: &mut Ui, label: &str, short: &str, enabled: bool| {
            ui.add_enabled(enabled, egui::Button::new(label).shortcut_text(short)).clicked()
        };
        if item(ui, "Copy", "⌘C", has) {
            self.copy(&ctx);
            ui.close();
        }
        if item(ui, "Paste", "⌘V", true) {
            // Reading the system clipboard goes through egui's paste event.
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
            ui.close();
        }
        ui.add_enabled_ui(has, |ui| {
            ui.menu_button("Copy/Paste as", |ui| {
                for (label, short, what) in [("Copy as PNG", "⇧⌘C", "png"), ("Copy as SVG", "", "svg"), ("Copy as CSS", "", "css")] {
                    if ui.add(egui::Button::new(label).shortcut_text(short)).clicked() {
                        self.copy_as(&ctx, what);
                        ui.close();
                    }
                }
                ui.separator();
                if ui.add(egui::Button::new("Copy properties").shortcut_text("⌥⌘C")).clicked() {
                    self.style_clip = self.sel.first().map(|&id| self.doc.node(id).clone());
                    ui.close();
                }
                if ui.add_enabled(self.style_clip.is_some(), egui::Button::new("Paste properties").shortcut_text("⌥⌘V")).clicked() {
                    self.paste_style();
                    ui.close();
                }
                if ui.add_enabled(self.clip_frag.is_some(), egui::Button::new("Paste to replace").shortcut_text("⇧⌘R")).clicked() {
                    self.paste_to_replace();
                    ui.close();
                }
            });
        });
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
                    if ui.add(egui::Button::new(format!("{op:?} selection")).shortcut_text(short)).clicked() {
                        self.boolean_selection(op);
                        ui.close();
                    }
                }
            });
        });
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
        if item(ui, "Flatten", "⌘E", has) {
            self.flatten_selection();
            ui.close();
        }
        if item(ui, "Frame selection", "⌥⌘G", has) {
            self.group_selection(true);
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
        if let Some(inst) = one.and_then(|id| self.owning_instance(id)) {
            if item(ui, "Go to main component", "", true) {
                self.go_to_main(inst);
                ui.close();
            }
            if item(ui, "Reset instance", "", true) {
                self.checkpoint();
                self.doc.reset_instance(inst);
                ui.close();
            }
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
    }
}

/// Everything that affects a text layer's measured size, hashed.
fn text_key(ctx: &egui::Context, n: &Node) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::hash::DefaultHasher::new();
    n.text.hash(&mut h);
    format!("{:?}", crate::text::font_family(ctx, &n.font_family, n.font_weight)).hash(&mut h);
    (n.font_size.to_bits(), n.letter_spacing.to_bits(), n.text_sizing as u8, n.text_case as u8).hash(&mut h);
    format!("{:?}", n.line_height).hash(&mut h);
    (n.truncate, n.max_lines, n.paragraph_spacing.to_bits()).hash(&mut h);
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

fn snap_angle(from: Pos2, to: Pos2) -> Pos2 {
    let d = to - from;
    let a = (d.y.atan2(d.x) / std::f32::consts::FRAC_PI_4).round() * std::f32::consts::FRAC_PI_4;
    from + Vec2::angled(a) * d.length()
}
