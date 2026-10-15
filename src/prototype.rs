//! Prototype playback: present mode, interactions, transitions, smart animate, scrolling, devices.

use crate::app::App;
use crate::doc::{Action, Device, Direction, Document, Easing, Kind, NodeId, Overflow, OverlayPos, OverlaySettings, PaintKind, Reaction, ScrollPos, Transition, TransitionKind, Trigger};
use crate::render::{self, Scene, View};
use egui::{Color32, Key, Painter, Pos2, Rect, Stroke, Ui, Vec2, pos2, vec2};
use std::collections::HashMap;

pub struct Anim {
    pub from: NodeId,
    pub to: NodeId,
    pub transition: Transition,
    pub start: f64,
    /// Reverse direction (Back plays the transition backwards).
    pub back: bool,
}

/// An open overlay.
#[derive(Clone, Copy, Debug)]
pub struct Shown {
    pub id: NodeId,
    /// Top-left, from the frame's top-left (in frame units).
    pub at: Vec2,
    pub settings: OverlaySettings,
}

/// What a While hovering / While pressing interaction undoes when it lets go.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Undo {
    Back { shown: NodeId, to: NodeId },
    Variant { inst: NodeId, was: NodeId },
    Overlay,
}

/// A press that may turn into a drag.
#[derive(Clone, Debug)]
pub enum Dragging {
    /// A layer with an On drag interaction, and where the press started.
    Trigger(NodeId, Reaction, Pos2),
    /// A scrolling frame, dragged like a touch screen.
    Scroll(NodeId),
}

#[derive(Default)]
pub struct Presentation {
    pub frame: NodeId,
    pub history: Vec<NodeId>,
    pub overlays: Vec<Shown>,
    pub anim: Option<Anim>,
    pub shown_at: f64,
    pub delays_fired: bool,
    /// A held While hovering / pressing interaction: hotspot on screen, pressing, what to undo.
    pub held: Option<(Rect, bool, Undo)>,
    /// Show hotspots after a click that hit nothing.
    pub hint_until: f64,
    /// Variant changes and variable values apply to this copy; the file stays as it is.
    pub doc: Option<Document>,
    /// Scroll offsets of scrolling frames.
    pub scroll: HashMap<NodeId, Vec2>,
    /// Animated Scroll to: frame, from, to, start, transition.
    pub scrolling: Option<(NodeId, Vec2, Vec2, f64, Transition)>,
    /// Layers under the pointer last frame, topmost first (Mouse enter / leave).
    pub under: Vec<NodeId>,
    pub drag: Option<Dragging>,
}

impl Easing {
    pub const ALL: [Easing; 12] = [
        Easing::Linear,
        Easing::EaseIn,
        Easing::EaseOut,
        Easing::EaseInOut,
        Easing::EaseInBack,
        Easing::EaseOutBack,
        Easing::EaseInOutBack,
        Easing::Custom([0.25, 0.1, 0.25, 1.0]),
        Easing::Gentle,
        Easing::Quick,
        Easing::Bouncy,
        Easing::Slow,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Easing::Linear => "Linear",
            Easing::EaseIn => "Ease in",
            Easing::EaseOut => "Ease out",
            Easing::EaseInOut => "Ease in and out",
            Easing::EaseInBack => "Ease in back",
            Easing::EaseOutBack => "Ease out back",
            Easing::EaseInOutBack => "Ease in and out back",
            Easing::Custom(_) => "Custom bezier",
            Easing::Gentle => "Gentle",
            Easing::Quick => "Quick",
            Easing::Bouncy => "Bouncy",
            Easing::Slow => "Slow",
        }
    }

    /// Figma's spring presets: mass, stiffness, damping.
    fn spring(self) -> Option<(f32, f32, f32)> {
        match self {
            Easing::Gentle => Some((1.0, 100.0, 15.0)),
            Easing::Quick => Some((1.0, 300.0, 20.0)),
            Easing::Bouncy => Some((1.0, 600.0, 15.0)),
            Easing::Slow => Some((1.0, 80.0, 20.0)),
            _ => None,
        }
    }

    pub fn is_spring(self) -> bool {
        self.spring().is_some()
    }

    /// Figma's control points for each curve.
    fn bezier(self) -> [f32; 4] {
        match self {
            Easing::EaseIn => [0.42, 0.0, 1.0, 1.0],
            Easing::EaseOut => [0.0, 0.0, 0.58, 1.0],
            Easing::EaseInOut => [0.42, 0.0, 0.58, 1.0],
            Easing::EaseInBack => [0.3, -0.05, 0.7, -0.5],
            Easing::EaseOutBack => [0.45, 1.45, 0.8, 1.0],
            Easing::EaseInOutBack => [0.7, -0.4, 0.4, 1.4],
            Easing::Custom(c) => c,
            _ => [0.0, 0.0, 1.0, 1.0],
        }
    }

    /// How long the animation runs: springs take as long as they need to settle, as in Figma.
    pub fn seconds(self, duration: f32) -> f32 {
        match self.spring() {
            Some((m, k, c)) => {
                let (w, z) = ((k / m).sqrt(), c / (2.0 * (k * m).sqrt()));
                // Until the slowest decaying term is under 0.1%.
                let decay = if z < 1.0 { z * w } else { w * (z - (z * z - 1.0).sqrt()) };
                1000f32.ln() / decay
            }
            None => duration,
        }
    }

    /// Progress at `t` (0..1 of `seconds`); back curves and springs overshoot past 1.
    pub fn at(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        let Some((m, k, c)) = self.spring() else { return cubic_bezier(self.bezier(), t) };
        if t >= 1.0 {
            return 1.0;
        }
        // A damped spring released from 0 at rest, settling at 1.
        let s = t * self.seconds(0.0);
        let (w, z) = ((k / m).sqrt(), c / (2.0 * (k * m).sqrt()));
        if z < 1.0 {
            let wd = w * (1.0 - z * z).sqrt();
            1.0 - (-z * w * s).exp() * ((wd * s).cos() + z * w / wd * (wd * s).sin())
        } else {
            let q = (z * z - 1.0).sqrt().max(1e-3);
            let (r1, r2) = (-w * (z - q), -w * (z + q));
            1.0 - (r2 * (r1 * s).exp() - r1 * (r2 * s).exp()) / (r2 - r1)
        }
    }
}

/// CSS-style cubic bezier easing: y where x = t.
fn cubic_bezier([x1, y1, x2, y2]: [f32; 4], t: f32) -> f32 {
    let b = |a: f32, b: f32, s: f32| 3.0 * a * s * (1.0 - s) * (1.0 - s) + 3.0 * b * s * s * (1.0 - s) + s * s * s;
    let (x1, x2) = (x1.clamp(0.0, 1.0), x2.clamp(0.0, 1.0));
    // x only rises with its control points inside 0..1, so bisect for the curve parameter.
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..32 {
        let mid = (lo + hi) / 2.0;
        if b(x1, x2, mid) < t { lo = mid } else { hi = mid }
    }
    b(y1, y2, (lo + hi) / 2.0)
}

impl Device {
    pub const ALL: [Device; 7] = [Device::None, Device::IPhone16, Device::IPhone16ProMax, Device::Pixel9, Device::IPad, Device::MacBook, Device::Desktop];

    pub fn label(self) -> &'static str {
        match self {
            Device::None => "None",
            Device::IPhone16 => "iPhone 16",
            Device::IPhone16ProMax => "iPhone 16 Pro Max",
            Device::Pixel9 => "Pixel 9",
            Device::IPad => "iPad",
            Device::MacBook => "MacBook",
            Device::Desktop => "Desktop",
        }
    }

    /// Screen size in points.
    pub fn screen(self) -> Option<Vec2> {
        Some(match self {
            Device::None => return None,
            Device::IPhone16 => vec2(393.0, 852.0),
            Device::IPhone16ProMax => vec2(440.0, 956.0),
            Device::Pixel9 => vec2(412.0, 923.0),
            Device::IPad => vec2(820.0, 1180.0),
            Device::MacBook => vec2(1280.0, 832.0),
            Device::Desktop => vec2(1440.0, 1024.0),
        })
    }

    /// Bezel around the screen (top, sides, bottom), outer corner radius, and what hangs below
    /// it (a laptop's base, a monitor's stand), in points. The corner radius stays under
    /// 3.4 times the side bezel so the corner masks over the square content stay inside the body.
    fn bezel(self) -> ([f32; 3], f32, f32) {
        match self {
            Device::IPhone16 | Device::IPhone16ProMax => ([14.0; 3], 47.0, 0.0),
            Device::Pixel9 => ([12.0; 3], 40.0, 0.0),
            Device::IPad => ([24.0; 3], 38.0, 0.0),
            Device::MacBook => ([22.0, 16.0, 16.0], 16.0, 18.0),
            Device::Desktop => ([20.0, 20.0, 48.0], 12.0, 130.0),
            Device::None => ([0.0; 3], 0.0, 0.0),
        }
    }
}

/// Where a device's screen sits in the window (scaled to fit, never up) and its scale.
fn device_screen(device: Device, window: Rect) -> Option<(Rect, f32)> {
    let size = device.screen()?;
    let ([top, side, bottom], _, below) = device.bezel();
    let outer = vec2(size.x + 2.0 * side, size.y + top + bottom + below);
    let area = window.shrink(48.0);
    let scale = (area.width() / outer.x).min(area.height() / outer.y).clamp(0.05, 1.0);
    let o = Rect::from_center_size(area.center(), outer * scale);
    Some((Rect::from_min_size(o.min + vec2(side, top) * scale, size * scale), scale))
}

const BEZEL: Color32 = Color32::from_gray(0x14);

/// The device body under the screen, vector drawn.
fn draw_device(painter: &Painter, device: Device, screen: Rect, s: f32) {
    let ([top, side, bottom], r, below) = device.bezel();
    let body = Rect::from_min_max(screen.min - vec2(side, top) * s, screen.max + vec2(side, bottom) * s);
    let rim = Stroke::new(2.0, Color32::from_gray(0x50));
    match device {
        Device::MacBook => {
            let w = body.width() * 0.06;
            let base = Rect::from_min_max(pos2(body.min.x - w, body.max.y), pos2(body.max.x + w, body.max.y + below * s));
            painter.rect_filled(base, egui::CornerRadius { nw: 0, ne: 0, sw: (8.0 * s) as u8, se: (8.0 * s) as u8 }, Color32::from_gray(0xA8));
            let notch = Rect::from_center_size(pos2(base.center().x, base.min.y + 3.0 * s), vec2(base.width() * 0.12, 6.0 * s));
            painter.rect_filled(notch, 3.0 * s, Color32::from_gray(0x80));
        }
        Device::Desktop => {
            let neck = Rect::from_min_max(pos2(body.center().x - 60.0 * s, body.max.y), pos2(body.center().x + 60.0 * s, body.max.y + (below - 16.0) * s));
            painter.rect_filled(neck, 0.0, Color32::from_gray(0x9A));
            let foot = Rect::from_center_size(pos2(body.center().x, neck.max.y + 8.0 * s), vec2(300.0 * s, 16.0 * s));
            painter.rect_filled(foot, 6.0 * s, Color32::from_gray(0xA8));
        }
        _ => {}
    }
    painter.rect(body, r * s, BEZEL, rim, egui::StrokeKind::Outside);
    painter.rect_filled(screen, 0.0, Color32::BLACK);
}

/// What sits over the screen: rounded screen corners, the island or camera.
fn draw_device_front(painter: &Painter, device: Device, screen: Rect, s: f32) {
    let ([_, side, _], r, _) = device.bezel();
    let radius = ((r - side) * s).max(0.0);
    // Mask the square content corners down to the screen's rounded ones.
    if radius > 0.5 {
        let mut mesh = egui::Mesh::default();
        let corners = [(screen.left_top(), vec2(1.0, 1.0)), (screen.right_top(), vec2(-1.0, 1.0)), (screen.right_bottom(), vec2(-1.0, -1.0)), (screen.left_bottom(), vec2(1.0, -1.0))];
        for (c, d) in corners {
            let center = c + d * radius;
            let i0 = mesh.vertices.len() as u32;
            mesh.colored_vertex(c, BEZEL);
            for k in 0..=8 {
                let a = std::f32::consts::FRAC_PI_2 * k as f32 / 8.0;
                mesh.colored_vertex(center - vec2(d.x * a.cos(), d.y * a.sin()) * radius, BEZEL);
            }
            for k in 0..8 {
                mesh.add_triangle(i0, i0 + 1 + k, i0 + 2 + k);
            }
        }
        painter.add(mesh);
    }
    match device {
        Device::IPhone16 | Device::IPhone16ProMax => {
            let island = Rect::from_center_size(pos2(screen.center().x, screen.min.y + 29.0 * s), vec2(124.0, 36.0) * s);
            painter.rect_filled(island, 18.0 * s, Color32::BLACK);
        }
        Device::Pixel9 => {
            painter.circle_filled(pos2(screen.center().x, screen.min.y + 20.0 * s), 7.0 * s, Color32::BLACK);
        }
        Device::IPad | Device::MacBook => {
            painter.circle_filled(pos2(screen.center().x, screen.min.y - 11.0 * s), 3.0 * s, Color32::from_gray(0x30));
        }
        _ => {}
    }
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
                n.opacity = lerp(a.opacity, n.opacity, t).clamp(0.0, 1.0);
                n.radius = lerp(a.radius, n.radius, t).max(0.0);
                n.rotation = lerp(a.rotation, n.rotation, t);
                if let (Some(fa), Some(fb)) = (a.fills.first(), n.fills.first_mut())
                    && fa.kind == PaintKind::Solid && fb.kind == PaintKind::Solid {
                        fb.color = fa.color.lerp(fb.color, t.clamp(0.0, 1.0));
                    }
            }
            None => n.opacity *= t.clamp(0.0, 1.0),
        }
    }
    d
}

/// How far a frame scrolls: how much its content runs past its right and bottom edges.
pub fn scroll_range(doc: &Document, f: NodeId) -> Vec2 {
    let n = doc.node(f);
    let content = n.children.iter().map(|c| doc.node(*c)).filter(|c| c.visible && c.scroll_pos != ScrollPos::Fixed).fold(n.rect, |r, c| r.union(c.aabb()));
    let over = (content.max - n.rect.max).max(Vec2::ZERO);
    match n.overflow {
        Overflow::None => Vec2::ZERO,
        Overflow::Vertical => vec2(0.0, over.y),
        Overflow::Horizontal => vec2(over.x, 0.0),
        Overflow::Both => over,
    }
}

/// Move a scrolled frame's content: scrolling layers by the offset, sticky ones until their
/// top-left reaches the frame's, fixed ones not at all. Fixed and sticky layers draw on top.
fn scroll_frame(d: &mut Document, f: NodeId, off: Vec2) {
    if !d.nodes.contains_key(&f) || off == Vec2::ZERO {
        return;
    }
    let top = d.node(f).rect.min;
    let mut kids = d.node(f).children.clone();
    for &c in &kids {
        let n = d.node(c);
        let shift = match n.scroll_pos {
            ScrollPos::Scroll => -off,
            ScrollPos::Fixed => Vec2::ZERO,
            ScrollPos::Sticky => (-off).max((top - n.rect.min).min(Vec2::ZERO)),
        };
        d.translate(c, shift);
    }
    kids.sort_by_key(|c| d.node(*c).scroll_pos != ScrollPos::Scroll);
    let n = d.node_mut(f);
    n.children = kids;
    n.clip = true;
}

/// Layers under `p` in `root`, topmost first. Clipping frames hide what lies outside them.
fn hits(doc: &Document, root: NodeId, p: Pos2) -> Vec<NodeId> {
    fn walk(doc: &Document, id: NodeId, p: Pos2, out: &mut Vec<NodeId>) {
        let n = doc.node(id);
        let inside = n.aabb().contains(p);
        if !n.visible || (n.clip && !inside) {
            return;
        }
        for &c in n.children.iter().rev() {
            walk(doc, c, p, out);
        }
        if inside {
            out.push(id);
        }
    }
    let mut out = vec![];
    if doc.nodes.contains_key(&root) {
        walk(doc, root, p, &mut out);
    }
    out
}

/// The topmost layer under the pointer that (itself or a parent) reacts to the trigger.
/// Layers without interactions don't block the ones below, so a label over a linked button
/// still clicks the button.
fn reaction_at(doc: &Document, under: &[NodeId], want: impl Fn(&Trigger) -> bool) -> Option<(NodeId, Reaction)> {
    under.iter().find_map(|&hit| std::iter::once(hit).chain(doc.ancestors(hit)).find_map(|id| doc.node(id).reactions.iter().find(|r| want(&r.trigger)).map(|r| (id, r.clone()))))
}

/// The instance a Change to on `trigger` switches: the nearest one whose variant shares a set with `to`.
fn variant_instance(doc: &Document, trigger: NodeId, to: NodeId) -> Option<NodeId> {
    let set = doc.nodes.get(&to)?.parent?;
    std::iter::once(trigger).chain(doc.ancestors(trigger)).find(|&a| {
        let n = doc.node(a);
        n.kind == Kind::Instance && n.component.and_then(|c| doc.nodes.get(&c)).is_some_and(|c| c.parent == Some(set))
    })
}

/// Where a frame sits on screen while presenting: filling a device's screen width, or scaled
/// down to fit the window.
fn present_view(doc: &Document, frame: NodeId, area: Rect, device: bool) -> View {
    let r = doc.node(frame).rect;
    if device && r.width() > 0.0 {
        let zoom = area.width() / r.width();
        return View { zoom, pan: area.min.to_vec2() - r.min.to_vec2() * zoom };
    }
    View::fit(r, area, 1.0)
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
        self.presenting = Some(Presentation { frame, shown_at: now, ..Default::default() });
    }

    /// The document as presented: interactive state over the file, scrolled frames moved.
    pub fn presented(&self) -> Document {
        let Some(p) = &self.presenting else { return self.doc.clone() };
        // ponytail: a fresh copy every presented frame (layers are shared, only the map is copied); cache it if huge files stutter
        let mut d = p.doc.clone().unwrap_or_else(|| self.doc.clone());
        for (&f, &off) in &p.scroll {
            scroll_frame(&mut d, f, off);
        }
        d
    }

    /// Where a frame sits on screen while presenting in `window`.
    pub fn presented_view(&self, doc: &Document, frame: NodeId, window: Rect) -> View {
        let device = device_screen(self.doc.pages[self.page].prototype.device, window);
        present_view(doc, frame, device.map_or(window, |(r, _)| r), device.is_some())
    }

    fn go(&mut self, to: NodeId, transition: Transition, back: bool, now: f64) {
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
        p.scroll.clear();
        p.scrolling = None;
        p.shown_at = now;
        p.delays_fired = false;
    }

    /// Run an interaction. `doc` is the presented document, `trigger` the layer it's on.
    fn perform(&mut self, doc: &Document, trigger: NodeId, r: &Reaction, ctx: &egui::Context, now: f64) {
        match &r.action {
            Action::None => {}
            Action::Navigate(to) => self.go(*to, r.transition, false, now),
            Action::Back => {
                let prev = self.presenting.as_mut().and_then(|p| p.history.pop());
                if let Some(prev) = prev {
                    self.go(prev, r.transition, true, now);
                }
            }
            Action::OpenOverlay(id) | Action::SwapOverlay(id) => {
                let Some(p) = self.presenting.as_mut() else { return };
                if !doc.nodes.contains_key(id) {
                    return;
                }
                let at = overlay_at(doc, p, trigger, *id, &r.overlay);
                if matches!(r.action, Action::SwapOverlay(_)) {
                    p.overlays.pop();
                }
                p.overlays.push(Shown { id: *id, at, settings: r.overlay });
            }
            Action::CloseOverlay => {
                if let Some(p) = self.presenting.as_mut() {
                    p.overlays.pop();
                }
            }
            Action::OpenUrl(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
            Action::ChangeTo(v) => {
                if let Some(inst) = variant_instance(doc, trigger, *v) {
                    self.change_variant(inst, *v);
                }
            }
            Action::ScrollTo(id) => {
                let Some(p) = self.presenting.as_mut() else { return };
                let base = p.doc.as_ref().unwrap_or(&self.doc);
                let Some(f) = base.nodes.get(id).and_then(|_| base.ancestors(*id).into_iter().find(|a| base.node(*a).overflow != Overflow::None)) else { return };
                let range = scroll_range(base, f);
                let to = (base.node(*id).rect.min - base.node(f).rect.min).clamp(Vec2::ZERO, range);
                let from = p.scroll.get(&f).copied().unwrap_or_default();
                if r.transition.kind == TransitionKind::Instant {
                    p.scroll.insert(f, to);
                } else {
                    p.scrolling = Some((f, from, to, now, r.transition));
                }
            }
            Action::SetVariable(var, val) => {
                let Some(p) = self.presenting.as_mut() else { return };
                let d = p.doc.get_or_insert_with(|| self.doc.clone());
                if let Some(v) = d.variables.iter_mut().find(|v| v.id == *var) {
                    v.values.iter_mut().for_each(|x| *x = val.clone());
                    d.reflow();
                }
            }
        }
    }

    /// Swap an instance to another variant in the presented copy.
    fn change_variant(&mut self, inst: NodeId, to: NodeId) {
        let Some(p) = self.presenting.as_mut() else { return };
        let d = p.doc.get_or_insert_with(|| self.doc.clone());
        if d.nodes.contains_key(&inst) {
            d.swap_component(inst, to);
            d.reflow();
        }
    }

    /// While hovering / pressing: act now, undo when the hover or press ends.
    fn hold(&mut self, doc: &Document, id: NodeId, r: &Reaction, spot: Rect, press: bool, ctx: &egui::Context, now: f64) {
        let Some(frame) = self.presenting.as_ref().map(|p| p.frame) else { return };
        let undo = match r.action {
            Action::Navigate(to) => Some(Undo::Back { shown: to, to: frame }),
            Action::ChangeTo(v) => variant_instance(doc, id, v).and_then(|inst| doc.node(inst).component.map(|was| Undo::Variant { inst, was })),
            Action::OpenOverlay(_) => Some(Undo::Overlay),
            _ => None,
        };
        self.perform(doc, id, r, ctx, now);
        if let (Some(u), Some(p)) = (undo, self.presenting.as_mut()) {
            p.held = Some((spot, press, u));
        }
    }

    fn release(&mut self, undo: Undo, now: f64) {
        let Some(p) = self.presenting.as_mut() else { return };
        p.held = None;
        match undo {
            Undo::Back { shown, to } => {
                if p.frame == shown {
                    p.history.pop();
                    self.go(to, Default::default(), true, now);
                }
            }
            Undo::Variant { inst, was } => self.change_variant(inst, was),
            Undo::Overlay => {
                p.overlays.pop();
            }
        }
    }

    fn scroll_by(&mut self, f: NodeId, d: Vec2) {
        let Some(p) = self.presenting.as_mut() else { return };
        let range = scroll_range(p.doc.as_ref().unwrap_or(&self.doc), f);
        let o = p.scroll.entry(f).or_default();
        *o = (*o + d).clamp(Vec2::ZERO, range);
        p.scrolling = None;
    }

    /// The scrolling frame under the pointer: the nearest one that has room to scroll.
    fn scroller_at(&self, doc: &Document, under: &[NodeId]) -> Option<NodeId> {
        let base = self.presenting.as_ref().and_then(|p| p.doc.as_ref()).unwrap_or(&self.doc);
        let &top = under.first()?;
        std::iter::once(top).chain(doc.ancestors(top)).find(|&a| doc.node(a).overflow != Overflow::None && base.nodes.contains_key(&a) && scroll_range(base, a) != Vec2::ZERO)
    }

    pub fn presentation_ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        let window = ui.max_rect();
        let settings = self.doc.pages[self.page].prototype;
        let painter = ui.painter_at(window);
        painter.rect_filled(window, 0.0, settings.background.to32());
        let Some(p) = &self.presenting else { return };
        let frame = p.frame;
        if !self.doc.nodes.contains_key(&frame) {
            self.presenting = None;
            return;
        }

        // Keys: Key/Gamepad interactions first; Esc exits, R restarts, arrows step through frames.
        let keys: Vec<(Key, egui::Modifiers)> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. } => Some((*key, *modifiers)),
                    _ => None,
                })
                .collect()
        });
        for (k, m) in keys {
            if k != Key::Escape {
                let doc = self.presented();
                let root = self.presenting.as_ref().map_or(frame, |p| p.overlays.last().map_or(p.frame, |s| s.id));
                let found = std::iter::once(root).chain(doc.descendants(root)).find_map(|id| {
                    doc.node(id).reactions.iter().find(|r| matches!(r.trigger, Trigger::Key(rk, rm) if rk == k && rm.command == m.command && rm.shift == m.shift && rm.alt == m.alt)).map(|r| (id, r.clone()))
                });
                if let Some((id, r)) = found {
                    self.perform(&doc, id, &r, &ctx, now);
                    continue;
                }
            }
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
        let Some(p) = self.presenting.as_mut() else { return };
        let (frame, overlays) = (p.frame, p.overlays.clone());
        // Animated Scroll to.
        if let Some((f, from, to, start, tr)) = p.scrolling {
            let t = ((now - start) as f32 / tr.easing.seconds(tr.duration).max(0.01)).min(1.0);
            p.scroll.insert(f, from + (to - from) * tr.easing.at(t));
            if t >= 1.0 {
                p.scrolling = None;
            }
            ctx.request_repaint();
        }
        let doc = self.presented();

        // A device's screen, or the whole window.
        let device = device_screen(settings.device, window);
        let area = device.map_or(window, |(r, _)| r);
        if let Some((screen, s)) = device {
            draw_device(&painter, settings.device, screen, s);
        }
        let content = painter.with_clip_rect(area);
        let front = |painter: &Painter| {
            if let Some((screen, s)) = device {
                draw_device_front(painter, settings.device, screen, s);
            }
        };
        let view = self.presented_view(&doc, frame, window);
        let anim = self.presenting.as_ref().and_then(|p| p.anim.as_ref().map(|a| (a.from, a.to, a.transition, a.start, a.back)));
        if let Some((from, to, tr, start, back)) = anim.filter(|a| doc.nodes.contains_key(&a.0) && doc.nodes.contains_key(&a.1)) {
            let t = ((now - start) as f32 / tr.easing.seconds(tr.duration).max(0.01)).min(1.0);
            let e = tr.easing.at(t);
            let (w, h) = (area.width(), area.height());
            let dir = match tr.direction {
                Direction::Left => vec2(w, 0.0),
                Direction::Right => vec2(-w, 0.0),
                Direction::Up => vec2(0.0, h),
                Direction::Down => vec2(0.0, -h),
            };
            let dir = if back { -dir } else { dir };
            let fv = self.presented_view(&doc, from, window);
            let shift = |v: View, d: Vec2| View { pan: v.pan + d, zoom: v.zoom };
            let tex = &mut self.textures;
            let mut draw = |d: &Document, view: View, id: NodeId, a: f32| render::draw_subtree(&content, &mut Scene { doc: d, view, hide: None, outline: false, tex: &mut *tex }, id, a);
            match tr.kind {
                TransitionKind::SmartAnimate => draw(&smart_frame(&doc, from, to, e), view, to, 1.0),
                TransitionKind::Dissolve => {
                    draw(&doc, fv, from, 1.0);
                    draw(&doc, view, to, e.clamp(0.0, 1.0));
                }
                TransitionKind::MoveIn | TransitionKind::SlideIn => {
                    draw(&doc, fv, from, 1.0);
                    let slide = tr.kind == TransitionKind::SlideIn;
                    let k = if slide { 0.3 } else { 1.0 };
                    let a = if slide { e.clamp(0.0, 1.0) } else { 1.0 };
                    draw(&doc, shift(view, dir * k * (1.0 - e)), to, a);
                }
                TransitionKind::Push => {
                    draw(&doc, shift(fv, -dir * e), from, 1.0);
                    draw(&doc, shift(view, dir * (1.0 - e)), to, 1.0);
                }
                TransitionKind::Instant => {}
            }
            if t >= 1.0
                && let Some(p) = self.presenting.as_mut() {
                    p.anim = None;
                }
            front(&painter);
            ctx.request_repaint();
            self.textures.end_frame();
            return;
        }
        render::draw_subtree(&content, &mut Scene { doc: &doc, view, hide: None, outline: false, tex: &mut self.textures }, frame, 1.0);
        // Overlays sit where they opened, over an optional background.
        let mut overlay_views = vec![];
        for s in &overlays {
            let Some(o) = doc.nodes.get(&s.id) else { continue };
            if let Some(bg) = s.settings.background {
                content.rect_filled(area, 0.0, bg.to32());
            }
            let at = view.to_screen(doc.node(frame).rect.min) + s.at * view.zoom;
            let ov = View { zoom: view.zoom, pan: at.to_vec2() - o.rect.min.to_vec2() * view.zoom };
            render::draw_subtree(&content, &mut Scene { doc: &doc, view: ov, hide: None, outline: false, tex: &mut self.textures }, s.id, 1.0);
            overlay_views.push((s.id, ov, s.settings.close_outside));
        }
        self.textures.end_frame();
        front(&painter);

        // Interactions: the topmost overlay under the pointer gets input, else the frame, so a
        // Mouse enter tooltip doesn't cut off the layer that opened it.
        let (top, top_view, close_outside) = overlay_views.last().copied().unwrap_or((frame, view, false));
        let (ptr, clicked, pressed, down, wheel, moved) =
            ctx.input(|i| (i.pointer.hover_pos(), i.pointer.primary_clicked(), i.pointer.primary_pressed(), i.pointer.primary_down(), i.smooth_scroll_delta, i.pointer.delta()));
        if let Some(sp) = ptr {
            let outside = top != frame && !doc.node(top).rect.contains(top_view.to_world(sp));
            let (target, tview) = overlay_views.iter().rev().map(|&(o, v, _)| (o, v)).find(|&(o, v)| doc.node(o).rect.contains(v.to_world(sp))).unwrap_or((frame, view));
            let w = tview.to_world(sp);
            let under = hits(&doc, target, w);
            // A held While hovering / pressing ends when the pointer leaves its hotspot or lets go.
            if let Some((spot, press, undo)) = self.presenting.as_ref().and_then(|p| p.held)
                && (if press { !down } else { !spot.contains(sp) }) {
                    self.release(undo, now);
                }
            if clicked {
                if outside && close_outside {
                    if let Some(p) = self.presenting.as_mut() {
                        p.overlays.pop();
                    }
                } else if let Some(url) = under.iter().find_map(|&id| crate::text::link_at(&content, doc.node(id), w)) {
                    // Text links open in the browser, as in Figma's prototypes.
                    ctx.open_url(egui::OpenUrl::new_tab(url));
                } else if let Some((id, r)) = reaction_at(&doc, &under, |t| *t == Trigger::OnClick) {
                    self.perform(&doc, id, &r, &ctx, now);
                } else if let Some(p) = self.presenting.as_mut() {
                    p.hint_until = now + 0.6;
                }
            }
            if pressed {
                if let Some((id, r)) = reaction_at(&doc, &under, |t| *t == Trigger::WhilePressed) {
                    let spot = tview.rect_to_screen(doc.node(id).aabb());
                    self.hold(&doc, id, &r, spot, true, &ctx, now);
                }
                // A press may become a drag: an On drag interaction, else the frame scrolls.
                let drag = reaction_at(&doc, &under, |t| *t == Trigger::OnDrag).map(|(id, r)| Dragging::Trigger(id, r, sp)).or_else(|| self.scroller_at(&doc, &under).map(Dragging::Scroll));
                if let Some(p) = self.presenting.as_mut() {
                    p.drag = drag;
                }
            }
            match self.presenting.as_ref().and_then(|p| p.drag.clone()).filter(|_| down) {
                Some(Dragging::Trigger(id, r, start)) if (sp - start).length() > 6.0 => {
                    if let Some(p) = self.presenting.as_mut() {
                        p.drag = None;
                    }
                    self.perform(&doc, id, &r, &ctx, now);
                }
                Some(Dragging::Scroll(f)) => self.scroll_by(f, -moved / tview.zoom),
                Some(_) => {}
                None => {
                    if let Some(p) = self.presenting.as_mut() {
                        p.drag = None;
                    }
                }
            }
            if wheel != Vec2::ZERO
                && let Some(f) = self.scroller_at(&doc, &under) {
                    self.scroll_by(f, -wheel / tview.zoom);
                }
            // Enter and leave: the topmost layer that reacts.
            let before = self.presenting.as_mut().map(|p| std::mem::replace(&mut p.under, under.clone())).unwrap_or_default();
            let entered: Vec<NodeId> = under.iter().copied().filter(|id| !before.contains(id)).collect();
            let left: Vec<NodeId> = before.iter().copied().filter(|id| !under.contains(id) && doc.nodes.contains_key(id)).collect();
            let first = |ids: &[NodeId], want: Trigger| ids.iter().find_map(|&id| doc.node(id).reactions.iter().find(|r| r.trigger == want).map(|r| (id, r.clone())));
            if self.presenting.as_ref().is_some_and(|p| p.held.is_none())
                && let Some((id, r)) = first(&entered, Trigger::OnHover) {
                    let spot = tview.rect_to_screen(doc.node(id).aabb());
                    self.hold(&doc, id, &r, spot, false, &ctx, now);
                }
            if let Some((id, r)) = first(&entered, Trigger::MouseEnter) {
                self.perform(&doc, id, &r, &ctx, now);
            }
            if let Some((id, r)) = first(&left, Trigger::MouseLeave) {
                self.perform(&doc, id, &r, &ctx, now);
            }
        }
        // After-delay triggers fire once per visit.
        let (shown_at, fired) = self.presenting.as_ref().map_or((now, true), |p| (p.shown_at, p.delays_fired));
        if !fired && doc.nodes.contains_key(&frame) {
            let delayed = std::iter::once(frame)
                .chain(doc.descendants(frame))
                .find_map(|id| doc.node(id).reactions.iter().find(|r| matches!(r.trigger, Trigger::AfterDelay(ms) if (now - shown_at) * 1000.0 >= ms as f64)).map(|r| (id, r.clone())));
            if let Some((id, r)) = delayed {
                if let Some(p) = self.presenting.as_mut() {
                    p.delays_fired = true;
                }
                self.perform(&doc, id, &r, &ctx, now);
            }
            ctx.request_repaint();
        }
        // Flash hotspots after a missed click, like Figma.
        if self.presenting.as_ref().is_some_and(|p| p.hint_until > now) && doc.nodes.contains_key(&top) {
            for id in std::iter::once(top).chain(doc.descendants(top)) {
                if !doc.node(id).reactions.is_empty() {
                    let r = top_view.rect_to_screen(doc.node(id).aabb());
                    content.rect_filled(r, 2.0, crate::canvas::BLUE.gamma_multiply(0.25));
                    content.rect_stroke(r, 2.0, Stroke::new(2.0, crate::canvas::BLUE), egui::StrokeKind::Inside);
                }
            }
            ctx.request_repaint();
        }
        let hint = painter.layout_no_wrap("Esc to exit  ·  R to restart".into(), egui::FontId::proportional(11.0), Color32::from_gray(0x99));
        painter.galley(pos2(window.min.x + 12.0, window.max.y - 24.0), hint, Color32::WHITE);
    }
}

/// Where an overlay opens: its top-left from the frame's top-left.
fn overlay_at(doc: &Document, p: &Presentation, trigger: NodeId, o: NodeId, s: &OverlaySettings) -> Vec2 {
    let f = doc.node(p.frame).rect;
    let free = f.size() - doc.node(o).rect.size();
    let k = match s.position {
        OverlayPos::Center => vec2(0.5, 0.5),
        OverlayPos::TopLeft => vec2(0.0, 0.0),
        OverlayPos::TopCenter => vec2(0.5, 0.0),
        OverlayPos::TopRight => vec2(1.0, 0.0),
        OverlayPos::BottomLeft => vec2(0.0, 1.0),
        OverlayPos::BottomCenter => vec2(0.5, 1.0),
        OverlayPos::BottomRight => vec2(1.0, 1.0),
        OverlayPos::Manual => {
            // From the trigger, wherever it sits: in the frame or in an open overlay.
            let t = doc.node(trigger).aabb().min;
            let root = top_frame(doc, trigger);
            let base = p.overlays.iter().rev().find(|s| s.id == root).map_or(t - f.min, |s| s.at + (t - doc.node(root).rect.min));
            return base + s.offset;
        }
    };
    free * k
}

/// Destinations a reaction can point at: top-level frames on the page.
pub fn frames_on_page(doc: &Document, page: usize) -> Vec<NodeId> {
    doc.pages[page].children.iter().copied().filter(|c| doc.node(*c).kind.is_frame() && doc.node(*c).kind != Kind::Section).collect()
}

/// The variants a Change to on `id` can pick: the other variants of the set it's in.
pub fn sibling_variants(doc: &Document, id: NodeId) -> Vec<NodeId> {
    std::iter::once(id)
        .chain(doc.ancestors(id))
        .find(|&a| doc.node(a).kind == Kind::Component && doc.node(a).parent.is_some_and(|s| doc.node(s).kind == Kind::ComponentSet))
        .and_then(|v| doc.node(v).parent)
        .map_or(vec![], |set| doc.node(set).children.clone())
}

/// Temp-data key naming the interaction whose Key/Gamepad trigger is waiting for a key press;
/// shortcuts stay off meanwhile so the key reaches the editor.
pub const RECORDING_KEY: &str = "prototype-recording-key";

/// A key trigger as Figma shows it ("⌘⇧K"), for this platform.
pub fn key_label(k: Key, m: egui::Modifiers) -> String {
    let mods = [(m.alt, "⌥"), (m.shift, "⇧"), (m.command, "⌘")].iter().filter(|x| x.0).map(|x| x.1).collect::<String>();
    crate::chrome::keys(&format!("{mods}{}", k.symbol_or_name()))
}

pub fn trigger_label(t: &Trigger) -> String {
    match *t {
        Trigger::OnClick => "On click".into(),
        Trigger::OnHover => "While hovering".into(),
        Trigger::WhilePressed => "While pressing".into(),
        Trigger::AfterDelay(ms) => format!("After {ms}ms"),
        Trigger::MouseEnter => "Mouse enter".into(),
        Trigger::MouseLeave => "Mouse leave".into(),
        Trigger::OnDrag => "On drag".into(),
        Trigger::Key(k, m) => format!("Key {}", key_label(k, m)),
    }
}

pub fn describe(doc: &Document, r: &Reaction) -> String {
    let name = |id: &NodeId| doc.nodes.get(id).map_or("missing".to_string(), |n| n.name.clone());
    let act = match &r.action {
        Action::None => "None".to_string(),
        Action::Navigate(id) => format!("→ {}", name(id)),
        Action::Back => "Back".to_string(),
        Action::OpenOverlay(id) => format!("Open {}", name(id)),
        Action::CloseOverlay => "Close overlay".to_string(),
        Action::OpenUrl(u) => format!("Open {u}"),
        Action::ChangeTo(id) => format!("Change to {}", name(id)),
        Action::ScrollTo(id) => format!("Scroll to {}", name(id)),
        Action::SwapOverlay(id) => format!("Swap to {}", name(id)),
        Action::SetVariable(v, _) => format!("Set {}", doc.variable(*v).map_or("missing", |v| v.name.as_str())),
    };
    format!("{}  {act}", trigger_label(&r.trigger))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easing_curves_and_springs() {
        for e in Easing::ALL {
            assert!(e.at(0.0).abs() < 1e-3, "{e:?} starts at 0");
            assert!((e.at(1.0) - 1.0).abs() < 1e-3, "{e:?} ends at 1");
        }
        assert!((Easing::Linear.at(0.3) - 0.3).abs() < 1e-3);
        assert!(Easing::EaseIn.at(0.5) < 0.5 && Easing::EaseOut.at(0.5) > 0.5);
        assert!((Easing::EaseInOut.at(0.5) - 0.5).abs() < 1e-3, "symmetric");
        assert!((0..100).any(|i| Easing::EaseOutBack.at(i as f32 / 100.0) > 1.0), "back overshoots");
        assert!((0..100).any(|i| Easing::EaseInBack.at(i as f32 / 100.0) < 0.0), "back undershoots");
        // Bouncy overshoots more than Gentle; Slow is overdamped and never overshoots.
        let peak = |e: Easing| (0..400).map(|i| e.at(i as f32 / 400.0)).fold(0.0f32, f32::max);
        assert!(peak(Easing::Bouncy) > peak(Easing::Gentle) && peak(Easing::Gentle) > 1.0);
        assert!(peak(Easing::Slow) <= 1.0 + 1e-3);
        assert!(Easing::Quick.seconds(5.0) < Easing::Slow.seconds(5.0) && Easing::Quick.seconds(5.0) < 2.0, "springs ignore duration");
        assert_eq!(Easing::Linear.seconds(0.4), 0.4);
    }

    #[test]
    fn old_interactions_still_load() {
        let r: Reaction = serde_json::from_str(r#"{"trigger":"OnHover","action":{"OpenOverlay":7},"transition":{"kind":"Dissolve","direction":"Left","duration":0.3}}"#).unwrap();
        assert_eq!(r.transition.easing, Easing::EaseOut);
        assert_eq!(r.overlay, OverlaySettings::default());
        let page: crate::doc::Page = serde_json::from_str(r#"{"name":"P","children":[],"background":[245,245,245,255]}"#).unwrap();
        assert_eq!(page.prototype.device, Device::None);
        // New fields round-trip.
        let k = Reaction { trigger: Trigger::Key(Key::K, egui::Modifiers::COMMAND), action: Action::ChangeTo(3), ..Default::default() };
        assert_eq!(serde_json::from_str::<Reaction>(&serde_json::to_string(&k).unwrap()).unwrap(), k);
    }

    #[test]
    fn scrolling_moves_scroll_and_sticky_layers_not_fixed() {
        use crate::doc::Node;
        let mut d = Document::default();
        let f = d.insert(0, None, None, Node::new(Kind::Frame, Rect::from_min_size(Pos2::ZERO, vec2(100.0, 100.0))));
        d.node_mut(f).overflow = Overflow::Vertical;
        let long = d.insert(0, Some(f), None, Node::new(Kind::Rectangle, Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 300.0))));
        let header = d.insert(0, Some(f), None, Node::new(Kind::Rectangle, Rect::from_min_size(pos2(0.0, 40.0), vec2(100.0, 10.0))));
        d.node_mut(header).scroll_pos = ScrollPos::Sticky;
        let fab = d.insert(0, Some(f), None, Node::new(Kind::Rectangle, Rect::from_min_size(pos2(80.0, 80.0), vec2(10.0, 10.0))));
        d.node_mut(fab).scroll_pos = ScrollPos::Fixed;
        assert_eq!(scroll_range(&d, f), vec2(0.0, 200.0));
        let mut s = d.clone();
        scroll_frame(&mut s, f, vec2(0.0, 30.0));
        assert_eq!(s.node(long).rect.min.y, -30.0);
        assert_eq!(s.node(header).rect.min.y, 10.0, "sticky scrolls until the top");
        assert_eq!(s.node(fab).rect.min.y, 80.0);
        let mut s = d.clone();
        scroll_frame(&mut s, f, vec2(0.0, 120.0));
        assert_eq!(s.node(header).rect.min.y, 0.0, "then sticks");
        assert_eq!(*s.node(f).children.last().unwrap(), fab, "fixed layers draw on top");
    }
}
