//! Prototype playback: present mode, interactions, transitions, smart animate.

use crate::app::App;
use crate::doc::{Action, Direction, Document, Kind, NodeId, PaintKind, Reaction, TransitionKind, Trigger};
use crate::render::{self, Scene, View};
use egui::{Color32, Key, Pos2, Rect, Ui, Vec2, pos2, vec2};
use std::collections::HashMap;

pub struct Anim {
    pub from: NodeId,
    pub to: NodeId,
    pub transition: crate::doc::Transition,
    pub start: f64,
    /// Reverse direction (Back plays the transition backwards).
    pub back: bool,
}

pub struct Presentation {
    pub frame: NodeId,
    pub history: Vec<NodeId>,
    pub overlays: Vec<NodeId>,
    pub anim: Option<Anim>,
    pub shown_at: f64,
    pub delays_fired: bool,
    /// Frame to return to when a hover/press navigation ends.
    pub hover_return: Option<(NodeId, NodeId)>,
    /// Show hotspots after a click that hit nothing.
    pub hint_until: f64,
}

fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Top-level frame containing `id` (or `id` itself).
pub fn top_frame(doc: &Document, id: NodeId) -> NodeId {
    *doc.ancestors(id).last().unwrap_or(&id)
}

/// Name path of a layer relative to its top frame, for smart-animate matching.
fn name_path(doc: &Document, id: NodeId, root: NodeId) -> String {
    let mut parts = vec![doc.node(id).name.clone()];
    for a in doc.ancestors(id) {
        if a == root {
            break;
        }
        parts.push(doc.node(a).name.clone());
    }
    parts.reverse();
    parts.join("/")
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// A copy of the document where `to` looks part-way between `from` and itself.
fn smart_frame(doc: &Document, from: NodeId, to: NodeId, t: f32) -> Document {
    let mut d = doc.clone();
    let (fr, tr) = (doc.node(from).rect.min, doc.node(to).rect.min);
    let src: HashMap<String, NodeId> = doc.descendants(from).into_iter().map(|id| (name_path(doc, id, from), id)).collect();
    for id in doc.descendants(to) {
        let n = d.node_mut(id);
        match src.get(&name_path(doc, id, to)) {
            Some(&s) => {
                let a = doc.node(s);
                // Compare positions relative to each frame.
                let ar = a.rect.translate(tr - fr);
                let br = n.rect;
                n.rect = Rect::from_min_max(
                    pos2(lerp(ar.min.x, br.min.x, t), lerp(ar.min.y, br.min.y, t)),
                    pos2(lerp(ar.max.x, br.max.x, t), lerp(ar.max.y, br.max.y, t)),
                );
                n.opacity = lerp(a.opacity, n.opacity, t);
                n.radius = lerp(a.radius, n.radius, t);
                n.rotation = lerp(a.rotation, n.rotation, t);
                if let (Some(fa), Some(fb)) = (a.fills.first(), n.fills.first_mut())
                    && fa.kind == PaintKind::Solid && fb.kind == PaintKind::Solid {
                        fb.color = fa.color.lerp(fb.color, t);
                    }
            }
            None => n.opacity *= t,
        }
    }
    d
}

impl App {
    /// Cmd+Alt+Enter / ▷: start at the selected frame, the first flow, or the first frame.
    pub fn present(&mut self, now: f64) {
        let page = &self.doc.pages[self.page];
        let start = self
            .sel
            .first()
            .map(|&s| top_frame(&self.doc, s))
            .filter(|f| self.doc.node(*f).kind.is_frame())
            .or_else(|| page.flows.first().copied().filter(|f| self.doc.nodes.contains_key(f)))
            .or_else(|| page.children.iter().copied().find(|c| self.doc.node(*c).kind.is_frame()));
        let Some(frame) = start else {
            self.toast("Add a frame to present".into());
            return;
        };
        self.end_text_edit();
        self.presenting = Some(Presentation {
            frame,
            history: vec![],
            overlays: vec![],
            anim: None,
            shown_at: now,
            delays_fired: false,
            hover_return: None,
            hint_until: 0.0,
        });
    }

    fn go(&mut self, to: NodeId, transition: crate::doc::Transition, back: bool, now: f64) {
        let Some(p) = self.presenting.as_mut() else { return };
        if !self.doc.nodes.contains_key(&to) || to == p.frame {
            return;
        }
        if !back {
            p.history.push(p.frame);
        }
        p.anim = (transition.kind != TransitionKind::Instant).then_some(Anim { from: p.frame, to, transition, start: now, back });
        p.frame = to;
        p.overlays.clear();
        p.shown_at = now;
        p.delays_fired = false;
    }

    fn perform(&mut self, r: &Reaction, ctx: &egui::Context, now: f64) {
        match &r.action {
            Action::None => {}
            Action::Navigate(to) => self.go(*to, r.transition, false, now),
            Action::Back => {
                let prev = self.presenting.as_mut().and_then(|p| p.history.pop());
                if let Some(prev) = prev {
                    self.go(prev, r.transition, true, now);
                }
            }
            Action::OpenOverlay(id) => {
                if let Some(p) = self.presenting.as_mut() {
                    p.overlays.push(*id);
                }
            }
            Action::CloseOverlay => {
                if let Some(p) = self.presenting.as_mut() {
                    p.overlays.pop();
                }
            }
            Action::OpenUrl(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
        }
    }

    /// Deepest visible layer under `p` inside `root`, then bubble up to the first with a matching reaction.
    fn reaction_at(&self, root: NodeId, p: Pos2, want: impl Fn(&Trigger) -> bool) -> Option<(NodeId, Reaction)> {
        fn deepest(doc: &Document, id: NodeId, p: Pos2) -> Option<NodeId> {
            let n = doc.node(id);
            if !n.visible || !n.aabb().contains(p) {
                return None;
            }
            for &c in n.children.iter().rev() {
                if let Some(h) = deepest(doc, c, p) {
                    return Some(h);
                }
            }
            Some(id)
        }
        let hit = deepest(&self.doc, root, p)?;
        std::iter::once(hit).chain(self.doc.ancestors(hit)).find_map(|id| {
            self.doc.node(id).reactions.iter().find(|r| want(&r.trigger)).map(|r| (id, r.clone()))
        })
    }

    /// Where a frame sits on screen while presenting (scaled down to fit, never up).
    fn present_view(&self, frame: NodeId, screen: Rect) -> View {
        View::fit(self.doc.node(frame).rect, screen, 1.0)
    }

    pub fn presentation_ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        let screen = ui.max_rect();
        let painter = ui.painter_at(screen);
        painter.rect_filled(screen, 0.0, Color32::from_gray(0x1E));
        let Some(p) = &self.presenting else { return };
        let (frame, overlays) = (p.frame, p.overlays.clone());
        if !self.doc.nodes.contains_key(&frame) {
            self.presenting = None;
            return;
        }

        // Keys: Esc exits, R restarts, arrows step through top-level frames.
        let keys: Vec<Key> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key { key, pressed: true, .. } => Some(*key),
                    _ => None,
                })
                .collect()
        });
        for k in keys {
            match k {
                Key::Escape => {
                    self.presenting = None;
                    return;
                }
                Key::R => {
                    let first = self.presenting.as_ref().and_then(|p| p.history.first().copied()).unwrap_or(frame);
                    self.presenting = None;
                    self.sel = vec![first];
                    self.present(now);
                    return;
                }
                Key::ArrowRight | Key::ArrowLeft => {
                    let frames: Vec<NodeId> = self.doc.pages[self.page].children.iter().copied().filter(|c| self.doc.node(*c).kind.is_frame()).collect();
                    if let Some(i) = frames.iter().position(|f| *f == frame) {
                        let j = if k == Key::ArrowRight { (i + 1).min(frames.len() - 1) } else { i.saturating_sub(1) };
                        self.go(frames[j], Default::default(), false, now);
                    }
                }
                _ => {}
            }
        }

        let view = self.present_view(frame, screen);
        let anim = self.presenting.as_ref().and_then(|p| p.anim.as_ref().map(|a| (a.from, a.to, a.transition, a.start, a.back)));
        match anim {
            Some((from, to, tr, start, back)) => {
                let t = ((now - start) as f32 / tr.duration.max(0.01)).min(1.0);
                let e = ease(t);
                let w = screen.width();
                let h = screen.height();
                let dir = match tr.direction {
                    Direction::Left => vec2(w, 0.0),
                    Direction::Right => vec2(-w, 0.0),
                    Direction::Up => vec2(0.0, h),
                    Direction::Down => vec2(0.0, -h),
                };
                let dir = if back { -dir } else { dir };
                let fv = self.present_view(from, screen);
                let shift = |v: View, d: Vec2| View { pan: v.pan + d, zoom: v.zoom };
                match tr.kind {
                    TransitionKind::SmartAnimate => {
                        let d = smart_frame(&self.doc, from, to, e);
                        render::draw_subtree(&painter, &mut Scene { doc: &d, view, hide: None, tex: &mut self.textures }, to, 1.0);
                    }
                    TransitionKind::Dissolve => {
                        render::draw_subtree(&painter, &mut Scene { doc: &self.doc, view: fv, hide: None, tex: &mut self.textures }, from, 1.0);
                        render::draw_subtree(&painter, &mut Scene { doc: &self.doc, view, hide: None, tex: &mut self.textures }, to, e);
                    }
                    TransitionKind::MoveIn | TransitionKind::SlideIn => {
                        render::draw_subtree(&painter, &mut Scene { doc: &self.doc, view: fv, hide: None, tex: &mut self.textures }, from, 1.0);
                        let k = if tr.kind == TransitionKind::SlideIn { 0.3 } else { 1.0 };
                        let a = if tr.kind == TransitionKind::SlideIn { e } else { 1.0 };
                        let v = shift(view, dir * k * (1.0 - e));
                        render::draw_subtree(&painter, &mut Scene { doc: &self.doc, view: v, hide: None, tex: &mut self.textures }, to, a);
                    }
                    TransitionKind::Push => {
                        let fv2 = shift(fv, -dir * e);
                        render::draw_subtree(&painter, &mut Scene { doc: &self.doc, view: fv2, hide: None, tex: &mut self.textures }, from, 1.0);
                        let v = shift(view, dir * (1.0 - e));
                        render::draw_subtree(&painter, &mut Scene { doc: &self.doc, view: v, hide: None, tex: &mut self.textures }, to, 1.0);
                    }
                    TransitionKind::Instant => {}
                }
                if t >= 1.0
                    && let Some(p) = self.presenting.as_mut() {
                        p.anim = None;
                    }
                ctx.request_repaint();
                self.textures.end_frame();
                return;
            }
            None => {
                render::draw_subtree(&painter, &mut Scene { doc: &self.doc, view, hide: None, tex: &mut self.textures }, frame, 1.0);
            }
        }
        // Overlays sit centered over the frame with a dim backdrop.
        let mut overlay_views = vec![];
        for &o in &overlays {
            if !self.doc.nodes.contains_key(&o) {
                continue;
            }
            painter.rect_filled(screen, 0.0, Color32::from_black_alpha(100));
            let fr = view.rect_to_screen(self.doc.node(frame).rect);
            let size = self.doc.node(o).rect.size() * view.zoom;
            let at = fr.center() - size / 2.0;
            let ov = View { zoom: view.zoom, pan: at.to_vec2() - self.doc.node(o).rect.min.to_vec2() * view.zoom };
            render::draw_subtree(&painter, &mut Scene { doc: &self.doc, view: ov, hide: None, tex: &mut self.textures }, o, 1.0);
            overlay_views.push((o, ov));
        }
        self.textures.end_frame();

        // Interactions: the top overlay gets input first.
        let (target, tview) = overlay_views.last().copied().unwrap_or((frame, view));
        let (ptr, clicked, pressed, released) = ctx.input(|i| (i.pointer.hover_pos(), i.pointer.primary_clicked(), i.pointer.primary_pressed(), i.pointer.primary_released()));
        if let Some(sp) = ptr {
            let w = tview.to_world(sp);
            if clicked {
                if let Some((_, r)) = self.reaction_at(target, w, |t| *t == Trigger::OnClick) {
                    self.perform(&r, &ctx, now);
                } else if !overlays.is_empty() && !self.doc.node(target).rect.contains(w) {
                    // Clicking outside an overlay closes it.
                    if let Some(p) = self.presenting.as_mut() {
                        p.overlays.pop();
                    }
                } else if let Some(p) = self.presenting.as_mut() {
                    p.hint_until = now + 0.6;
                }
            }
            if pressed
                && let Some((_, r)) = self.reaction_at(target, w, |t| *t == Trigger::WhilePressed) {
                    if let Action::Navigate(to) = r.action
                        && let Some(p) = self.presenting.as_mut() {
                            p.hover_return = Some((to, frame));
                        }
                    self.perform(&r, &ctx, now);
                }
            // Hover navigations return when the pointer leaves the hotspot.
            let hovering = self.reaction_at(target, w, |t| *t == Trigger::OnHover);
            let ret = self.presenting.as_ref().and_then(|p| p.hover_return);
            match (hovering, ret) {
                (Some((_, r)), None) => {
                    if let Action::Navigate(to) = r.action
                        && let Some(p) = self.presenting.as_mut() {
                            p.hover_return = Some((to, frame));
                        }
                    self.perform(&r, &ctx, now);
                }
                (None, Some((shown, back))) if shown == frame && !ctx.input(|i| i.pointer.primary_down()) => {
                    if let Some(p) = self.presenting.as_mut() {
                        p.hover_return = None;
                        p.history.pop();
                    }
                    self.go(back, Default::default(), true, now);
                }
                _ => {}
            }
            let _ = released;
        }
        // After-delay triggers fire once per visit.
        let (shown_at, fired) = self.presenting.as_ref().map_or((now, true), |p| (p.shown_at, p.delays_fired));
        if !fired {
            let delayed: Vec<Reaction> = std::iter::once(frame)
                .chain(self.doc.descendants(frame))
                .flat_map(|id| self.doc.node(id).reactions.clone())
                .filter(|r| matches!(r.trigger, Trigger::AfterDelay(ms) if (now - shown_at) * 1000.0 >= ms as f64))
                .collect();
            if let Some(r) = delayed.first() {
                if let Some(p) = self.presenting.as_mut() {
                    p.delays_fired = true;
                }
                self.perform(r, &ctx, now);
            }
            ctx.request_repaint();
        }
        // Flash hotspots after a missed click, like Figma.
        if self.presenting.as_ref().is_some_and(|p| p.hint_until > now) {
            for id in std::iter::once(target).chain(self.doc.descendants(target)) {
                if !self.doc.node(id).reactions.is_empty() {
                    let r = tview.rect_to_screen(self.doc.node(id).aabb());
                    painter.rect_filled(r, 2.0, crate::canvas::BLUE.gamma_multiply(0.25));
                    painter.rect_stroke(r, 2.0, egui::Stroke::new(2.0, crate::canvas::BLUE), egui::StrokeKind::Inside);
                }
            }
            ctx.request_repaint();
        }
        let hint = painter.layout_no_wrap("Esc to exit  ·  R to restart".into(), egui::FontId::proportional(11.0), Color32::from_gray(0x99));
        painter.galley(pos2(screen.min.x + 12.0, screen.max.y - 24.0), hint, Color32::WHITE);
    }
}

/// Destinations a reaction can point at: top-level frames on the page.
pub fn frames_on_page(doc: &Document, page: usize) -> Vec<NodeId> {
    doc.pages[page].children.iter().copied().filter(|c| doc.node(*c).kind.is_frame() && doc.node(*c).kind != Kind::Section).collect()
}

pub fn describe(doc: &Document, r: &Reaction) -> String {
    let trig = match r.trigger {
        Trigger::OnClick => "On click".to_string(),
        Trigger::OnHover => "On hover".to_string(),
        Trigger::WhilePressed => "While pressing".to_string(),
        Trigger::AfterDelay(ms) => format!("After {ms}ms"),
    };
    let act = match &r.action {
        Action::None => "None".to_string(),
        Action::Navigate(id) => format!("→ {}", doc.nodes.get(id).map_or("missing", |n| n.name.as_str())),
        Action::Back => "Back".to_string(),
        Action::OpenOverlay(id) => format!("Open {}", doc.nodes.get(id).map_or("missing", |n| n.name.as_str())),
        Action::CloseOverlay => "Close overlay".to_string(),
        Action::OpenUrl(u) => format!("Open {u}"),
    };
    format!("{trig}  {act}")
}
