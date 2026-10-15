//! Vector edit mode: move anchors and handles, add and delete points, toggle smooth corners,
//! marquee points, bend, continue paths with the pen, cut segments, and join endpoints.

use crate::app::{App, Drag};
use crate::canvas::BLUE;
use crate::doc::{Document, Kind, NodeId, PathPoint, SubPath, rotate};
use egui::{Color32, Painter, Pos2, Rect, Stroke, StrokeKind, Vec2, vec2};

/// Anchor address: (subpath, index).
pub type PointRef = (usize, usize);

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Grab {
    Anchor(PointRef),
    /// `true` = outgoing handle.
    Handle(PointRef, bool),
    /// Pull mirrored handles out of an anchor: dragging after a pen click, or Cmd-dragging an
    /// anchor (Figma's Bend).
    Pull(PointRef),
    /// Cmd-drag on a segment bends it through the pointer: (subpath, segment start, t).
    Segment(usize, usize, f32),
}

/// Vector edit mode's own tools, as in Figma's toolbar while editing a path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum VTool {
    #[default]
    Move,
    /// P: add points, continue open paths from their ends.
    Pen,
    /// Y: click a segment to cut the path there.
    Cut,
}

#[derive(Default)]
pub struct VectorEdit {
    pub id: NodeId,
    pub selected: Vec<PointRef>,
    pub tool: VTool,
    /// Pen: the open subpath being extended at its end.
    pub drawing: Option<usize>,
}

/// Reverse a contour's direction: handles swap sides.
fn reverse(pts: &mut [PathPoint]) {
    pts.reverse();
    for p in pts {
        std::mem::swap(&mut p.h_in, &mut p.h_out);
    }
}

impl Document {
    /// Re-fit a vector's box after editing anchors without moving anything on screen,
    /// even when rotated (the pivot moves with the box, so compensate).
    pub fn refit_vector_keep(&mut self, id: NodeId) {
        let n = self.node_mut(id);
        let (c0, rot) = (n.rect.center(), n.rotation);
        n.refit_vector();
        let c1 = n.rect.center();
        let d = c1 - c0;
        let fix = rotate(Pos2::ZERO + d, Pos2::ZERO, rot).to_vec2() - d;
        n.rect = n.rect.translate(fix);
    }
}

fn world(app: &App, id: NodeId, local: Pos2) -> Pos2 {
    let n = app.doc.node(id);
    rotate(n.rect.min + local.to_vec2(), n.rect.center(), n.rotation)
}

fn local(app: &App, id: NodeId, w: Pos2) -> Pos2 {
    let n = app.doc.node(id);
    (n.to_local(w) - n.rect.min).to_pos2()
}

/// Split a cubic at `t` (de Casteljau), returning the new middle anchor and updated neighbors.
fn split(a: PathPoint, b: PathPoint, t: f32) -> (PathPoint, PathPoint, PathPoint) {
    let (p0, p1, p2, p3) = (a.p, a.p + a.h_out, b.p + b.h_in, b.p);
    let l = |x: Pos2, y: Pos2| x + (y - x) * t;
    let (q0, q1, q2) = (l(p0, p1), l(p1, p2), l(p2, p3));
    let (r0, r1) = (l(q0, q1), l(q1, q2));
    let m = l(r0, r1);
    let a2 = PathPoint { h_out: q0 - p0, ..a };
    let mid = PathPoint { p: m, h_in: r0 - m, h_out: r1 - m };
    let b2 = PathPoint { h_in: q2 - p3, ..b };
    (a2, mid, b2)
}

impl App {
    /// Enter vector edit mode, converting simple shapes to paths first.
    pub fn enter_vector_edit(&mut self, id: NodeId) {
        let kind = self.doc.node(id).kind;
        // Shapes become paths on their first edit, as in Figma.
        if matches!(kind, Kind::Rectangle | Kind::Ellipse | Kind::Polygon | Kind::Star) {
            self.sel = vec![id];
            self.flatten_selection();
        } else if kind != Kind::Vector {
            return;
        }
        let id = self.sel.first().copied().unwrap_or(id);
        if self.doc.node(id).kind == Kind::Vector {
            self.vector_edit = Some(VectorEdit { id, ..Default::default() });
            self.sel = vec![id];
        }
    }

    pub fn exit_vector_edit(&mut self) {
        // A pen click that never got a second point leaves nothing behind.
        if let Some(id) = self.vector_edit.take().map(|v| v.id).filter(|id| self.doc.nodes.get(id).is_some_and(|n| n.paths.iter().any(|sp| sp.points.len() < 2))) {
            self.doc.node_mut(id).paths.retain(|sp| sp.points.len() >= 2);
            self.doc.refit_vector_keep(id);
        }
    }

    /// What the pointer at screen `s` grabs in edit mode.
    pub fn vector_grab(&self, s: Pos2) -> Option<Grab> {
        let ve = self.vector_edit.as_ref()?;
        let n = self.doc.node(ve.id);
        let near = |p: Pos2| self.view.to_screen(world(self, ve.id, p)).distance(s) < 6.0;
        // Handles of selected anchors win over anchors underneath them.
        for &(si, pi) in &ve.selected {
            let Some(pt) = n.paths.get(si).and_then(|sp| sp.points.get(pi)) else { continue };
            for (out, h) in [(true, pt.h_out), (false, pt.h_in)] {
                if h != Vec2::ZERO && near(pt.p + h) {
                    return Some(Grab::Handle((si, pi), out));
                }
            }
        }
        for (si, sp) in n.paths.iter().enumerate() {
            for (pi, pt) in sp.points.iter().enumerate() {
                if near(pt.p) {
                    return Some(Grab::Anchor((si, pi)));
                }
            }
        }
        None
    }

    /// Nearest segment under screen `s`: (subpath, segment start, t).
    fn segment_at(&self, s: Pos2) -> Option<(usize, usize, f32)> {
        let ve = self.vector_edit.as_ref()?;
        let n = self.doc.node(ve.id);
        let mut best: Option<(f32, usize, usize, f32)> = None;
        for (si, sp) in n.paths.iter().enumerate() {
            let k = sp.points.len();
            let segs = if sp.closed { k } else { k.saturating_sub(1) };
            for i in 0..segs {
                let (a, b) = (sp.points[i], sp.points[(i + 1) % k]);
                for step in 0..=32 {
                    let t = step as f32 / 32.0;
                    let (_, m, _) = split(a, b, t.clamp(0.001, 0.999));
                    let d = self.view.to_screen(world(self, ve.id, m.p)).distance(s);
                    if d < 5.0 && best.is_none_or(|b| d < b.0) {
                        best = Some((d, si, i, t.clamp(0.001, 0.999)));
                    }
                }
            }
        }
        best.map(|(_, si, i, t)| (si, i, t))
    }

    /// Whether `r` is the first or last anchor of an open contour.
    fn is_open_end(&self, (si, pi): PointRef) -> bool {
        let Some(ve) = &self.vector_edit else { return false };
        self.doc.node(ve.id).paths.get(si).is_some_and(|sp| !sp.closed && (pi == 0 || pi + 1 == sp.points.len()))
    }

    /// Mouse down in edit mode. Returns false if the press should leave edit mode.
    pub fn vector_press(&mut self, s: Pos2, mods: egui::Modifiers) -> bool {
        let Some((id, tool)) = self.vector_edit.as_ref().map(|v| (v.id, v.tool)) else { return false };
        let shift = mods.shift;
        self.vector_outside = false;
        let grab = self.vector_grab(s);
        match tool {
            VTool::Pen => return self.pen_press(s, grab),
            VTool::Cut => {
                if let Some((si, i, t)) = self.segment_at(s) {
                    self.checkpoint();
                    self.cut_at(si, i, t);
                }
                return true;
            }
            VTool::Move => {}
        }
        // Cmd: bend. On an anchor, pull out smooth handles; on a segment, curve it.
        if mods.command {
            if let Some(Grab::Anchor(r)) = grab {
                self.vector_edit.as_mut().unwrap().selected = vec![r];
                self.vector_grabbed = Some(Grab::Pull(r));
                return true;
            }
            if let Some((si, i, t)) = self.segment_at(s) {
                self.vector_grabbed = Some(Grab::Segment(si, i, t));
                return true;
            }
        }
        match grab {
            Some(Grab::Anchor(r)) => {
                let ve = self.vector_edit.as_mut().unwrap();
                if shift {
                    if let Some(i) = ve.selected.iter().position(|x| *x == r) {
                        ve.selected.remove(i);
                    } else {
                        ve.selected.push(r);
                    }
                } else if !ve.selected.contains(&r) {
                    ve.selected = vec![r];
                }
                self.vector_grabbed = Some(Grab::Anchor(r));
                true
            }
            Some(g @ Grab::Handle(..)) => {
                self.vector_grabbed = Some(g);
                true
            }
            None => {
                if let Some((si, i, t)) = self.segment_at(s) {
                    // Clicking a segment adds a point there.
                    self.checkpoint();
                    let n = self.doc.node_mut(id);
                    let sp = &mut n.paths[si];
                    let k = sp.points.len();
                    let (a2, mid, b2) = split(sp.points[i], sp.points[(i + 1) % k], t);
                    sp.points[i] = a2;
                    sp.points[(i + 1) % k] = b2;
                    sp.points.insert(i + 1, mid);
                    let r = (si, i + 1);
                    self.vector_edit.as_mut().unwrap().selected = vec![r];
                    self.vector_grabbed = Some(Grab::Anchor(r));
                    return true;
                }
                // Empty canvas: a drag marquees points (Shift adds to them); a click outside the
                // path leaves edit mode.
                let inside = self.doc.node(id).aabb().expand(8.0 / self.view.zoom).contains(self.view.to_world(s));
                if !shift {
                    self.vector_edit.as_mut().unwrap().selected.clear();
                }
                self.vector_grabbed = None;
                self.vector_outside = !inside;
                true
            }
            Some(_) => true,
        }
    }

    /// Marquee in edit mode: anchors inside the world rect, added to `base`.
    pub fn vector_marquee(&mut self, r: Rect, base: &[PointRef]) {
        let Some(id) = self.vector_edit.as_ref().map(|v| v.id) else { return };
        let mut sel = base.to_vec();
        for (si, sp) in self.doc.node(id).paths.iter().enumerate() {
            for (pi, pt) in sp.points.iter().enumerate() {
                if r.contains(world(self, id, pt.p)) && !sel.contains(&(si, pi)) {
                    sel.push((si, pi));
                }
            }
        }
        self.vector_edit.as_mut().unwrap().selected = sel;
    }

    /// Pen in edit mode: click an open end to continue from it, click empty canvas to add a point
    /// (drag for handles), click the path's other end to close it or another open end to join.
    fn pen_press(&mut self, s: Pos2, grab: Option<Grab>) -> bool {
        let ve = self.vector_edit.as_ref().unwrap();
        let (id, drawing) = (ve.id, ve.drawing);
        if let Some(Grab::Anchor(r)) = grab.filter(|g| matches!(g, Grab::Anchor(r) if self.is_open_end(*r))) {
            let len = self.doc.node(id).paths[r.0].points.len();
            match drawing {
                None => {
                    // Continue from here: flip the contour so new points go on its end.
                    self.checkpoint();
                    if r.1 == 0 && len > 1 {
                        reverse(&mut self.doc.node_mut(id).paths[r.0].points);
                    }
                    let ve = self.vector_edit.as_mut().unwrap();
                    ve.drawing = Some(r.0);
                    ve.selected = vec![(r.0, len - 1)];
                }
                Some(si) if (si, self.doc.node(id).paths[si].points.len() - 1) != r => {
                    let end = (si, self.doc.node(id).paths[si].points.len() - 1);
                    self.vector_edit.as_mut().unwrap().selected = vec![end, r];
                    self.vector_join();
                }
                _ => {}
            }
            return true;
        }
        self.checkpoint();
        let p = local(self, id, self.view.to_world(s));
        let n = self.doc.node_mut(id);
        let si = drawing.unwrap_or_else(|| {
            n.paths.push(SubPath::default());
            n.paths.len() - 1
        });
        n.paths[si].points.push(PathPoint { p, ..Default::default() });
        let r = (si, n.paths[si].points.len() - 1);
        self.doc.refit_vector_keep(id);
        let ve = self.vector_edit.as_mut().unwrap();
        ve.drawing = Some(si);
        ve.selected = vec![r];
        self.vector_grabbed = Some(Grab::Pull(r));
        true
    }

    /// The Pen tool over a lone selected path's open end continues that path (Figma).
    pub fn pen_continue(&mut self, s: Pos2) -> bool {
        let [id] = self.sel[..] else { return false };
        if self.doc.node(id).kind != Kind::Vector {
            return false;
        }
        self.vector_edit = Some(VectorEdit { id, tool: VTool::Pen, ..Default::default() });
        match self.vector_grab(s) {
            Some(Grab::Anchor(r)) if self.is_open_end(r) => {
                self.tool = crate::app::Tool::Move;
                self.pen_press(s, Some(Grab::Anchor(r)))
            }
            _ => {
                self.vector_edit = None;
                false
            }
        }
    }

    /// Cut tool: split segment `i` of contour `si` at `t`, opening the path there.
    fn cut_at(&mut self, si: usize, i: usize, t: f32) {
        let id = self.vector_edit.as_ref().unwrap().id;
        let n = self.doc.node_mut(id);
        let sp = n.paths[si].clone();
        let k = sp.points.len();
        let mut pts = sp.points;
        let (a2, mid, b2) = split(pts[i], pts[(i + 1) % k], t);
        pts[i] = a2;
        pts[(i + 1) % k] = b2;
        let head = PathPoint { h_out: Vec2::ZERO, ..mid };
        let tail = PathPoint { h_in: Vec2::ZERO, ..mid };
        if sp.closed {
            // From the cut all the way around back to it.
            let mut open = vec![tail];
            open.extend_from_slice(&pts[i + 1..]);
            open.extend_from_slice(&pts[..=i]);
            open.push(head);
            n.paths[si] = SubPath { points: open, closed: false };
        } else {
            let mut first = pts[..=i].to_vec();
            first.push(head);
            let mut second = vec![tail];
            second.extend_from_slice(&pts[i + 1..]);
            n.paths[si] = SubPath { points: first, closed: false };
            n.paths.insert(si + 1, SubPath { points: second, closed: false });
        }
        let ve = self.vector_edit.as_mut().unwrap();
        ve.selected.clear();
        ve.drawing = None;
        self.doc.refit_vector_keep(id);
    }

    /// Cmd J: join the two selected open ends. Both ends of one path close it; ends of two paths
    /// make one path, merging the meeting points when they sit on top of each other.
    pub fn vector_join(&mut self) {
        let Some(ve) = &self.vector_edit else { return };
        let (id, sel) = (ve.id, ve.selected.clone());
        let [a, b] = sel[..] else { return self.toast("Select two open endpoints to join".into()) };
        if a == b || !self.is_open_end(a) || !self.is_open_end(b) {
            return self.toast("Select two open endpoints to join".into());
        }
        self.checkpoint();
        let n = self.doc.node_mut(id);
        if a.0 == b.0 {
            let pts = &mut n.paths[a.0].points;
            if pts[0].p.distance(pts[pts.len() - 1].p) < 0.5 && pts.len() > 2 {
                let last = pts.pop().unwrap();
                pts[0].h_in = last.h_in;
            }
            n.paths[a.0].closed = true;
        } else {
            let mut p1 = n.paths[a.0].points.clone();
            let mut p2 = n.paths[b.0].points.clone();
            if a.1 == 0 {
                reverse(&mut p1);
            }
            if b.1 != 0 {
                reverse(&mut p2);
            }
            if p1[p1.len() - 1].p.distance(p2[0].p) < 0.5 {
                let first = p2.remove(0);
                p1.last_mut().unwrap().h_out = first.h_out;
            }
            p1.extend(p2);
            n.paths[a.0].points = p1;
            n.paths.remove(b.0);
        }
        let ve = self.vector_edit.as_mut().unwrap();
        ve.selected.clear();
        ve.drawing = None;
        self.doc.refit_vector_keep(id);
    }

    pub fn vector_drag_start(&mut self) -> Option<Drag> {
        let g = self.vector_grabbed?;
        let id = self.vector_edit.as_ref()?.id;
        self.checkpoint();
        Some(Drag::Vector { base: self.doc.clone(), id, grab: g })
    }

    pub fn vector_drag(&mut self, base: &Document, id: NodeId, grab: Grab, start: Pos2, w: Pos2, alt: bool) {
        self.doc = base.clone();
        let d = local(self, id, w) - local(self, id, start);
        let selected = self.vector_edit.as_ref().map(|v| v.selected.clone()).unwrap_or_default();
        let n = self.doc.node_mut(id);
        match grab {
            Grab::Anchor(_) => {
                for (si, pi) in selected {
                    if let Some(pt) = n.paths.get_mut(si).and_then(|sp| sp.points.get_mut(pi)) {
                        pt.p += d;
                    }
                }
            }
            Grab::Pull((si, pi)) => {
                if let Some(pt) = n.paths.get_mut(si).and_then(|sp| sp.points.get_mut(pi)) {
                    (pt.h_out, pt.h_in) = (d, -d);
                }
            }
            Grab::Segment(si, i, t) => {
                // Moving both control points by D moves the curve at t by 3t(1-t)D.
                let sp = &mut n.paths[si];
                let k = sp.points.len();
                let m = d / (3.0 * t * (1.0 - t));
                sp.points[i].h_out += m;
                sp.points[(i + 1) % k].h_in += m;
            }
            Grab::Handle((si, pi), out) => {
                let pt = &mut n.paths[si].points[pi];
                let (h, other) = if out { (&mut pt.h_out, pt.h_in) } else { (&mut pt.h_in, pt.h_out) };
                let before = *h;
                *h += d;
                // Smooth points keep their handles in line unless Alt breaks them.
                let smooth = other != Vec2::ZERO && before != Vec2::ZERO && before.normalized().dot(-other.normalized()) > 0.99;
                let new = *h;
                if smooth && !alt {
                    let opp = -new.normalized() * other.length();
                    if out { pt.h_in = opp } else { pt.h_out = opp }
                }
            }
        }
        self.doc.refit_vector_keep(id);
    }

    pub fn vector_delete_points(&mut self) {
        let Some(ve) = self.vector_edit.as_mut() else { return };
        if ve.selected.is_empty() {
            return;
        }
        ve.drawing = None;
        let (id, mut sel) = (ve.id, std::mem::take(&mut ve.selected));
        self.checkpoint();
        sel.sort();
        let n = self.doc.node_mut(id);
        for &(si, pi) in sel.iter().rev() {
            if let Some(sp) = n.paths.get_mut(si)
                && pi < sp.points.len() {
                    sp.points.remove(pi);
                }
        }
        n.paths.retain(|sp| sp.points.len() >= 2);
        if n.paths.is_empty() {
            self.doc.remove(id);
            self.sel.clear();
            self.vector_edit = None;
        } else {
            self.doc.refit_vector_keep(id);
        }
    }

    /// Double-click an anchor: corner <-> smooth.
    pub fn vector_toggle_smooth(&mut self, s: Pos2) -> bool {
        let Some(Grab::Anchor(at)) = self.vector_grab(s) else { return false };
        self.checkpoint();
        self.toggle_smooth_at(at);
        true
    }

    /// Bend a corner into a smooth point, or straighten a smooth one.
    fn toggle_smooth_at(&mut self, (si, pi): PointRef) {
        let id = self.vector_edit.as_ref().unwrap().id;
        let n = self.doc.node_mut(id);
        let sp = &mut n.paths[si];
        let k = sp.points.len();
        let pt = sp.points[pi];
        if pt.h_in != Vec2::ZERO || pt.h_out != Vec2::ZERO {
            sp.points[pi].h_in = Vec2::ZERO;
            sp.points[pi].h_out = Vec2::ZERO;
        } else {
            let prev = if pi > 0 || sp.closed { Some(sp.points[(pi + k - 1) % k].p) } else { None };
            let next = if pi + 1 < k || sp.closed { Some(sp.points[(pi + 1) % k].p) } else { None };
            let dir = match (prev, next) {
                (Some(a), Some(b)) => (b - a).normalized(),
                (None, Some(b)) => (b - pt.p).normalized(),
                (Some(a), None) => (pt.p - a).normalized(),
                _ => Vec2::X,
            };
            let len = [prev, next].iter().flatten().map(|q| q.distance(pt.p)).fold(0.0, f32::max) / 3.0;
            sp.points[pi].h_out = dir * len;
            sp.points[pi].h_in = -dir * len;
        }
        self.doc.refit_vector_keep(id);
    }

    /// The Design panel while editing a vector: the selected point's position, Bend, Delete, Done.
    pub fn vector_edit_panel(&mut self, ui: &mut egui::Ui) {
        let Some(ve) = self.vector_edit.as_ref() else { return };
        let (id, selected) = (ve.id, ve.selected.clone());
        let muted = crate::appearance::muted(ui.ctx());
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Edit vector").strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Done").on_hover_text("Esc or Enter").clicked() {
                    self.exit_vector_edit();
                }
            });
        });
        if self.vector_edit.is_none() {
            return;
        }
        match selected.as_slice() {
            [] => {
                ui.label(egui::RichText::new("Select a point to edit it.").color(muted));
            }
            [one] => {
                // Positions read like the layer's X and Y: relative to the parent frame.
                let origin = self.doc.node(id).parent.map_or(Pos2::ZERO, |p| self.doc.node(p).rect.min);
                let Some(pt) = self.doc.node(id).paths.get(one.0).and_then(|sp| sp.points.get(one.1)).copied() else { return };
                let w = world(self, id, pt.p) - origin.to_vec2();
                let (mut x, mut y) = (w.x, w.y);
                let mut moved = false;
                ui.horizontal(|ui| {
                    ui.label("X");
                    moved |= ui.add(egui::DragValue::new(&mut x).range(-1e6..=1e6).speed(0.5)).changed();
                    ui.label("Y");
                    moved |= ui.add(egui::DragValue::new(&mut y).range(-1e6..=1e6).speed(0.5)).changed();
                });
                if moved {
                    self.begin_edit();
                    let to = local(self, id, egui::pos2(x, y) + origin.to_vec2());
                    if let Some(p) = self.doc.node_mut(id).paths.get_mut(one.0).and_then(|sp| sp.points.get_mut(one.1)) {
                        p.p = to;
                    }
                    self.doc.refit_vector_keep(id);
                }
            }
            many => {
                ui.label(format!("{} points selected", many.len()));
            }
        }
        let keys = crate::chrome::keys;
        ui.horizontal(|ui| {
            let ve = self.vector_edit.as_mut().unwrap();
            for (t, label, key) in [(VTool::Move, "Move", "V"), (VTool::Pen, "Pen", "P"), (VTool::Cut, "Cut", "Y")] {
                if ui.selectable_label(ve.tool == t, label).on_hover_text(keys(key)).clicked() {
                    ve.tool = t;
                    ve.drawing = None;
                }
            }
        });
        ui.horizontal(|ui| {
            if ui.add_enabled(selected.len() == 2, egui::Button::new("Join")).on_hover_text(format!("Join two open endpoints ({})", keys("⌘J"))).clicked() {
                self.vector_join();
            }
            if ui.add_enabled(!selected.is_empty(), egui::Button::new("Bend")).on_hover_text("Smooth or straighten the selected points (double-click a point)").clicked() {
                self.checkpoint();
                for &at in &selected {
                    self.toggle_smooth_at(at);
                }
            }
            if ui.add_enabled(!selected.is_empty(), egui::Button::new("Delete")).on_hover_text("Delete the selected points (⌫)").clicked() {
                self.vector_delete_points();
            }
        });
        ui.add_space(4.0);
        ui.label(egui::RichText::new("Click a segment to add a point. Drag points or their handles, or drag a box around several; Shift adds. Cmd-drag bends.").small().color(muted));
    }

    pub fn draw_vector_edit(&self, painter: &Painter, ptr: Option<Pos2>) {
        let Some(ve) = &self.vector_edit else { return };
        let Some(n) = self.doc.nodes.get(&ve.id) else { return };
        painter.add(crate::render::stroke_mesh(&crate::render::node_path(n, &self.view, 0.0), 1.0, BLUE));
        let sc = |p: Pos2| self.view.to_screen(world(self, ve.id, p));
        // Pen: a rubber band from the end being drawn to the pointer.
        if let (Some(last), Some(p), None) = (ve.drawing.and_then(|si| n.paths.get(si)).and_then(|sp| sp.points.last()), ptr, &self.drag) {
            painter.line_segment([sc(last.p), p], Stroke::new(1.0, BLUE));
        }
        for (si, sp) in n.paths.iter().enumerate() {
            for (pi, pt) in sp.points.iter().enumerate() {
                let selected = ve.selected.contains(&(si, pi));
                if selected {
                    for h in [pt.h_in, pt.h_out] {
                        if h != Vec2::ZERO {
                            painter.line_segment([sc(pt.p), sc(pt.p + h)], Stroke::new(1.0, BLUE));
                            painter.circle(sc(pt.p + h), 3.5, Color32::WHITE, Stroke::new(1.0, BLUE));
                        }
                    }
                }
                let r = Rect::from_center_size(sc(pt.p), vec2(7.0, 7.0));
                painter.rect_filled(r, 1.0, if selected { BLUE } else { Color32::WHITE });
                painter.rect_stroke(r, 1.0, Stroke::new(1.0, BLUE), StrokeKind::Middle);
            }
        }
    }
}
