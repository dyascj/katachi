//! Infinite canvas: navigation, tools, selection, snapping, and on-canvas overlays.

use crate::app::{App, Drag, Grip, Press, Tool};
use crate::doc::{Kind, LayoutMode, Node, NodeId, Order, PathPoint, Sizing, rotate};
use crate::render::{self, View};
use egui::{
    Color32, CornerRadius, CursorIcon, FontId, Key, Painter, PointerButton, Pos2, Rect, Sense, Stroke, StrokeKind,
    Ui, Vec2, pos2, vec2,
};

pub const BLUE: Color32 = Color32::from_rgb(0x0D, 0x99, 0xFF);
const SNAP: Color32 = Color32::from_rgb(0xF2, 0x48, 0x22);

pub fn num(v: f32) -> String {
    let r = (v * 100.0).round() / 100.0;
    if r.fract() == 0.0 { format!("{}", r as i64) } else { format!("{r}") }
}

impl App {
    pub fn canvas_panel(&mut self, ui: &mut Ui) {
        let bg = self.doc.pages[self.page].background;
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(bg.to32())).show(ui, |ui| self.canvas(ui));
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
        self.fit_texts(&painter);

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

        let mut scene = render::Scene { doc: &self.doc, view: self.view, hide: self.editing_text, tex: &mut self.textures };
        render::draw_page(&painter, &mut scene, self.page);
        self.textures.end_frame();
        self.layout_frame_labels(&painter);

        self.accept_drops(ui, hover_pos.filter(|p| rect.contains(*p)).unwrap_or(rect.center()));
        let ptr = resp.interact_pointer_pos().or(hover_pos);
        let world = ptr.map(|p| self.view.to_world(p));
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
        resp.context_menu(|ui| self.context_menu(ui));

        self.text_editor(ui, &painter);
        self.draw_overlays(&painter, ptr);
        self.set_cursor(ui, ptr, panning);
        self.toolbar(ui.ctx(), rect);
        self.draw_toast(&painter, rect);
    }

    /// Image files dropped on the canvas become image-filled rectangles at natural size.
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
            let Ok(img) = image::load_from_memory(&bytes) else {
                self.toast(format!("Can't import {}", f.name));
                continue;
            };
            if placed.is_empty() {
                self.checkpoint();
            }
            let size = vec2(img.width() as f32, img.height() as f32);
            let key = self.add_image(bytes);
            let w = self.view.to_world(at) + vec2(offset, offset);
            let mut n = Node::new(Kind::Rectangle, Rect::from_center_size(w.round(), size));
            let mut p = crate::doc::Paint::solid(crate::doc::Rgba::WHITE);
            p.convert(crate::doc::PaintKind::Image);
            p.image = key;
            n.fills = vec![p];
            let stem = f.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned());
            n.name = stem.unwrap_or_else(|| f.name.clone());
            let parent = self.frame_at(w, &[]);
            placed.push(self.doc.insert(self.page, parent, None, n));
            offset += 20.0;
        }
        if !placed.is_empty() {
            self.sel = placed;
            self.tool = Tool::Move;
        }
    }

    /// Auto-width text: the box always hugs its content.
    fn fit_texts(&mut self, painter: &Painter) {
        let ids: Vec<NodeId> = self.doc.nodes.values().filter(|n| n.kind == Kind::Text).map(|n| n.id).collect();
        let mut changed = false;
        for id in ids {
            let size = render::measure_text(painter, self.doc.node(id));
            let n = self.doc.node_mut(id);
            if (n.rect.size() - size).length() > 0.01 {
                n.rect = Rect::from_min_size(n.rect.min, size);
                changed = true;
            }
        }
        if changed {
            self.doc.reflow();
        }
    }

    fn layout_frame_labels(&mut self, painter: &Painter) {
        self.frame_labels.clear();
        for &id in &self.doc.pages[self.page].children {
            let n = self.doc.node(id);
            if n.kind != Kind::Frame || !n.visible {
                continue;
            }
            let active = self.sel.contains(&id) || self.hover == Some(id);
            let color = if active { BLUE } else { Color32::from_gray(0x80) };
            let galley = painter.layout_no_wrap(n.name.clone(), FontId::proportional(11.0), color);
            let r = self.view.rect_to_screen(n.aabb());
            let pos = pos2(r.min.x, r.min.y - galley.size().y - 4.0);
            self.frame_labels.push((id, Rect::from_min_size(pos, galley.size())));
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
        if self.editing_text.is_some() || self.tool != Tool::Move {
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
        match self.tool {
            Tool::Move => {
                let handle = self.handle_at(s);
                let target = if handle.is_some() { None } else { self.target_at(w, s, mods.command) };
                let mut toggle_off = false;
                let mut defer = false;
                if let Some(t) = target {
                    let n = self.doc.node(t);
                    // An unselected top-level frame with content: click selects it, drag marquees.
                    defer = n.kind == Kind::Frame
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
                        self.sel = vec![t];
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
        self.drag = match self.tool {
            Tool::Move => {
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
                        self.sel = self.doc.duplicate(&self.sel_roots());
                    }
                    Some(Drag::Move { base: self.doc.clone(), start })
                } else {
                    let base_sel = if mods.shift { self.sel.clone() } else { vec![] };
                    Some(Drag::Marquee { start, base_sel })
                }
            }
            Tool::Frame | Tool::Rectangle | Tool::Ellipse | Tool::Line => {
                self.checkpoint();
                let start = self.snap_point(start.round(), &[]);
                let parent = self.frame_at(start, &[]);
                let rect = Rect::from_min_size(start, Vec2::ZERO);
                let mut node = match self.tool {
                    Tool::Frame => Node::new(Kind::Frame, rect),
                    Tool::Rectangle => Node::new(Kind::Rectangle, rect),
                    Tool::Ellipse => Node::new(Kind::Ellipse, rect),
                    _ => {
                        let mut n = Node::vector(&[PathPoint { p: start, ..Default::default() }; 2], false);
                        n.name = self.doc.auto_name("Line");
                        n
                    }
                };
                if node.kind == Kind::Frame && parent.is_some() {
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
                    n.points = vec![PathPoint { p: a, ..Default::default() }, PathPoint { p: b, ..Default::default() }];
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
            Drag::Pan => {}
        }
        self.drag = Some(drag);
    }

    fn on_drag_end(&mut self, w: Pos2) {
        self.guides.clear();
        match self.drag.take() {
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
                if n.kind == Kind::Frame {
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
        match self.tool {
            Tool::Move => {
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
            Tool::Frame | Tool::Rectangle | Tool::Ellipse | Tool::Line => {
                // A click without a drag drops a default 100×100 shape.
                self.checkpoint();
                let p = w.round();
                let parent = self.frame_at(p, &[]);
                let node = match self.tool {
                    Tool::Frame => Node::new(Kind::Frame, Rect::from_min_size(p, vec2(100.0, 100.0))),
                    Tool::Rectangle => Node::new(Kind::Rectangle, Rect::from_min_size(p, vec2(100.0, 100.0))),
                    Tool::Ellipse => Node::new(Kind::Ellipse, Rect::from_min_size(p, vec2(100.0, 100.0))),
                    _ => {
                        let mut n = Node::vector(
                            &[PathPoint { p, ..Default::default() }, PathPoint { p: p + vec2(100.0, 0.0), ..Default::default() }],
                            false,
                        );
                        n.name = self.doc.auto_name("Line");
                        n
                    }
                };
                let id = self.doc.insert(self.page, parent, None, node);
                self.sel = vec![id];
                self.tool = Tool::Move;
                self.press = None;
            }
            _ => {}
        }
    }

    fn on_double_click(&mut self, w: Pos2) {
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
            if n.kind == Kind::Frame && !n.children.is_empty() && !r.contains_rect(n.aabb()) {
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
            out.push(self.doc.node(p).aabb());
        }
        out
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

    fn outline(&self, painter: &Painter, id: NodeId, width: f32) {
        let n = self.doc.node(id);
        match n.kind {
            Kind::Vector | Kind::Ellipse => {
                painter.add(render::stroke_mesh(&render::node_path(n, &self.view, 0.0), width, BLUE));
            }
            _ => {
                let pts = n.corners().map(|p| self.view.to_screen(p)).to_vec();
                painter.add(egui::Shape::closed_line(pts, Stroke::new(width, BLUE)));
            }
        }
    }

    fn draw_overlays(&self, painter: &Painter, ptr: Option<Pos2>) {
        if let Some(h) = self.hover.filter(|h| !self.sel.contains(h)) {
            self.outline(painter, h, 2.0);
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
            painter.add(egui::Shape::closed_line(corners.to_vec(), Stroke::new(1.0, BLUE)));
            if self.editing_text.is_none() && self.tool == Tool::Move {
                for c in corners {
                    let h = Rect::from_center_size(c, vec2(7.0, 7.0));
                    painter.rect_filled(h, 1.0, Color32::WHITE);
                    painter.rect_stroke(h, 1.0, Stroke::new(1.0, BLUE), StrokeKind::Middle);
                }
            }
            let b = self.view.rect_to_screen(self.doc.bounds(&roots));
            let label = match &self.drag {
                Some(Drag::Rotate { deg, .. }) => format!("{}°", num(*deg)),
                _ => format!("{} × {}", num(fr.width()), num(fr.height())),
            };
            let g = painter.layout_no_wrap(label, FontId::proportional(11.0), Color32::WHITE);
            let pill = Rect::from_center_size(b.center_bottom() + vec2(0.0, 8.0 + g.size().y / 2.0 + 2.0), g.size() + vec2(8.0, 4.0));
            painter.rect_filled(pill, 2.0, BLUE);
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

    fn set_cursor(&self, ui: &Ui, ptr: Option<Pos2>, panning: bool) {
        let Some(p) = ptr.filter(|_| ui.ui_contains_pointer()) else { return };
        let icon = if panning {
            if ui.input(|i| i.pointer.any_down()) { CursorIcon::Grabbing } else { CursorIcon::Grab }
        } else {
            match self.tool {
                Tool::Move => match self.handle_at(p) {
                    Some(Grip::Resize(-1, -1) | Grip::Resize(1, 1)) => CursorIcon::ResizeNwSe,
                    Some(Grip::Resize(1, -1) | Grip::Resize(-1, 1)) => CursorIcon::ResizeNeSw,
                    Some(Grip::Resize(_, 0)) => CursorIcon::ResizeHorizontal,
                    Some(Grip::Resize(0, _)) => CursorIcon::ResizeVertical,
                    Some(Grip::Rotate) => CursorIcon::Alias,
                    _ => CursorIcon::Default,
                },
                Tool::Text => CursorIcon::Text,
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
        let font = FontId::new(n.font_size * self.view.zoom, render::font_family(n.font_weight));
        let color = n.first_fill().map_or(Color32::BLACK, |c| c.to32());
        let mut text = n.text.clone();
        let width = render::measure_text(painter, &n).x * self.view.zoom + n.font_size * self.view.zoom;
        let out = egui::Area::new(egui::Id::new("canvas-text-edit"))
            .fixed_pos(self.view.to_screen(n.rect.min))
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut text)
                        .font(font)
                        .text_color(color)
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
            self.doc.node_mut(id).text = text;
            self.dirty = true;
        }
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

fn snap_angle(from: Pos2, to: Pos2) -> Pos2 {
    let d = to - from;
    let a = (d.y.atan2(d.x) / std::f32::consts::FRAC_PI_4).round() * std::f32::consts::FRAC_PI_4;
    from + Vec2::angled(a) * d.length()
}
