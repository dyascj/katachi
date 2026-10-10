//! Document model: pages hold a tree of nodes. Coordinates are absolute world units.

use egui::{Color32, Pos2, Rect, Vec2, pos2, vec2};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub type NodeId = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Kind {
    Frame,
    /// A main component: a frame other layers can instantiate.
    Component,
    /// A live copy of a component.
    Instance,
    Group,
    Rectangle,
    Ellipse,
    Vector,
    Text,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Frame => "Frame",
            Kind::Component => "Component",
            Kind::Instance => "Instance",
            Kind::Group => "Group",
            Kind::Rectangle => "Rectangle",
            Kind::Ellipse => "Ellipse",
            Kind::Vector => "Vector",
            Kind::Text => "Text",
        }
    }
    pub fn is_container(self) -> bool {
        self.is_frame() || self == Kind::Group
    }
    /// Frames, components, and instances all behave as frames.
    pub fn is_frame(self) -> bool {
        matches!(self, Kind::Frame | Kind::Component | Kind::Instance)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LayoutMode {
    #[default]
    None,
    Horizontal,
    Vertical,
}

/// How a layer sizes itself along one axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Sizing {
    #[default]
    Fixed,
    /// Shrink-wrap content (auto-layout frames).
    Hug,
    /// Stretch to the auto-layout parent.
    Fill,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum AxisAlign {
    #[default]
    Min,
    Center,
    Max,
}

/// Figma constraint for one axis: Left/Top, Right/Bottom, Left & right, Center, Scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Constraint {
    #[default]
    Min,
    Max,
    Stretch,
    Center,
    Scale,
}

#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoLayout {
    pub mode: LayoutMode,
    pub gap: f32,
    /// "Auto" spacing: children spread to fill the main axis.
    pub space_between: bool,
    /// Top, right, bottom, left.
    pub padding: [f32; 4],
    pub main: AxisAlign,
    pub cross: AxisAlign,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StrokeAlign {
    Inside,
    Center,
    Outside,
}

/// A vector anchor. `p` is relative to the node's `rect.min`; handles are offsets from `p`.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct PathPoint {
    pub p: Pos2,
    pub h_in: Vec2,
    pub h_out: Vec2,
}

/// Unmultiplied sRGBA. Unlike egui's premultiplied `Color32`, a 0% opacity color keeps its hue.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Rgba(pub [u8; 4]);

impl Rgba {
    pub const BLACK: Rgba = Rgba([0, 0, 0, 255]);
    pub const WHITE: Rgba = Rgba([255, 255, 255, 255]);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Rgba([r, g, b, 255])
    }
    pub fn to32(self) -> Color32 {
        let [r, g, b, a] = self.0;
        Color32::from_rgba_unmultiplied(r, g, b, a)
    }
    pub fn a(self) -> u8 {
        self.0[3]
    }
    pub fn with_alpha(self, a: u8) -> Self {
        Rgba([self.0[0], self.0[1], self.0[2], a])
    }
    pub fn hex(self) -> String {
        format!("{:02X}{:02X}{:02X}", self.0[0], self.0[1], self.0[2])
    }
    pub fn lerp(self, o: Rgba, t: f32) -> Rgba {
        Rgba(std::array::from_fn(|i| (self.0[i] as f32 + (o.0[i] as f32 - self.0[i] as f32) * t).round() as u8))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PaintKind {
    #[default]
    Solid,
    Linear,
    Radial,
    Image,
}

/// A fill or stroke paint. For gradients and images, `color`'s alpha is the paint opacity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Paint {
    pub color: Rgba,
    pub visible: bool,
    #[serde(default)]
    pub kind: PaintKind,
    /// Gradient stops: position in 0..=1 and color.
    #[serde(default)]
    pub stops: Vec<(f32, Rgba)>,
    /// Gradient handles in the layer's unit square: linear start/end, or radial center/edge.
    #[serde(default)]
    pub from: Pos2,
    #[serde(default)]
    pub to: Pos2,
    /// Key into `Document::images`.
    #[serde(default)]
    pub image: String,
}

impl Paint {
    pub fn solid(color: Rgba) -> Self {
        Self { color, visible: true, kind: PaintKind::Solid, stops: vec![], from: Pos2::ZERO, to: Pos2::ZERO, image: String::new() }
    }

    /// Switch kind, seeding Figma's defaults from the current color.
    pub fn convert(&mut self, kind: PaintKind) {
        if kind == self.kind {
            return;
        }
        let a = self.color.a();
        let base = if self.kind == PaintKind::Solid { self.color.with_alpha(255) } else { self.stops.first().map_or(Rgba::BLACK, |s| s.1) };
        match kind {
            PaintKind::Solid => self.color = base.with_alpha(a),
            PaintKind::Linear | PaintKind::Radial => {
                if self.stops.len() < 2 {
                    self.stops = vec![(0.0, base), (1.0, base.with_alpha(0))];
                }
                (self.from, self.to) = if kind == PaintKind::Linear {
                    (pos2(0.5, 0.0), pos2(0.5, 1.0))
                } else {
                    (pos2(0.5, 0.5), pos2(1.0, 0.5))
                };
                self.color = Rgba::WHITE.with_alpha(a);
            }
            PaintKind::Image => self.color = Rgba::WHITE.with_alpha(a),
        }
        self.kind = kind;
    }

    /// Paint opacity in 0..=1.
    pub fn alpha(&self) -> f32 {
        self.color.a() as f32 / 255.0
    }

    /// Gradient color at `t`, interpolated in unmultiplied sRGB like Figma.
    pub fn sample(&self, t: f32) -> Rgba {
        let stops = &self.stops;
        let Some(first) = stops.first() else { return Rgba([0, 0, 0, 0]) };
        if t <= first.0 {
            return first.1;
        }
        for w in stops.windows(2) {
            if t <= w[1].0 {
                let span = (w[1].0 - w[0].0).max(1e-6);
                return w[0].1.lerp(w[1].1, (t - w[0].0) / span);
            }
        }
        stops.last().unwrap().1
    }

    /// Gradient parameter for a point in the layer's unit square.
    pub fn gradient_t(&self, p: Pos2) -> f32 {
        let d = self.to - self.from;
        match self.kind {
            PaintKind::Radial => (p - self.from).length() / d.length().max(1e-6),
            _ => (p - self.from).dot(d) / d.length_sq().max(1e-6),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Shadow {
    pub color: Rgba,
    pub offset: Vec2,
    pub blur: f32,
    pub spread: f32,
    pub visible: bool,
}

impl Default for Shadow {
    /// Figma's default drop shadow.
    fn default() -> Self {
        Self { color: Rgba([0, 0, 0, 64]), offset: vec2(0.0, 4.0), blur: 4.0, spread: 0.0, visible: true }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Node {
    pub id: NodeId,
    pub name: String,
    pub kind: Kind,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub rect: Rect,
    /// Degrees counter-clockwise around the rect center, as shown in Figma.
    pub rotation: f32,
    pub visible: bool,
    pub locked: bool,
    pub opacity: f32,
    pub fills: Vec<Paint>,
    pub strokes: Vec<Paint>,
    pub stroke_width: f32,
    pub stroke_align: StrokeAlign,
    pub radius: f32,
    pub clip: bool,
    pub shadows: Vec<Shadow>,
    pub layout: AutoLayout,
    /// Horizontal, vertical.
    pub sizing: [Sizing; 2],
    pub constraints: [Constraint; 2],
    /// Ignores the parent's auto layout.
    pub absolute: bool,
    /// Instances: the main component they copy.
    pub component: Option<NodeId>,
    /// Inside an instance (and the instance itself): the component layer this mirrors.
    pub source: Option<NodeId>,
    /// The source's values as last copied; a property that differs from this is an override.
    pub synced: Option<Box<Node>>,
    pub points: Vec<PathPoint>,
    pub closed: bool,
    pub text: String,
    pub font_size: f32,
    pub font_weight: u16,
    pub expanded: bool,
}

impl Default for Node {
    fn default() -> Self {
        Self {
            id: 0,
            name: String::new(),
            kind: Kind::Rectangle,
            parent: None,
            children: vec![],
            rect: Rect::from_min_size(Pos2::ZERO, vec2(100.0, 100.0)),
            rotation: 0.0,
            visible: true,
            locked: false,
            opacity: 1.0,
            fills: vec![],
            strokes: vec![],
            stroke_width: 1.0,
            stroke_align: StrokeAlign::Inside,
            radius: 0.0,
            clip: false,
            shadows: vec![],
            layout: AutoLayout::default(),
            sizing: [Sizing::Fixed; 2],
            constraints: [Constraint::Min; 2],
            absolute: false,
            component: None,
            source: None,
            synced: None,
            points: vec![],
            closed: false,
            text: String::new(),
            font_size: 12.0,
            font_weight: 400,
            expanded: true,
        }
    }
}

pub const SHAPE_GRAY: Rgba = Rgba::rgb(0xD9, 0xD9, 0xD9);

impl Node {
    /// A node with Figma's defaults for its kind.
    pub fn new(kind: Kind, rect: Rect) -> Self {
        let mut n = Node { kind, rect, ..Default::default() };
        match kind {
            Kind::Frame | Kind::Component | Kind::Instance => {
                n.fills = vec![Paint::solid(Rgba::WHITE)];
                n.clip = true;
            }
            Kind::Rectangle | Kind::Ellipse => n.fills = vec![Paint::solid(SHAPE_GRAY)],
            Kind::Vector => {
                n.strokes = vec![Paint::solid(Rgba::BLACK)];
                n.stroke_align = StrokeAlign::Center;
            }
            Kind::Text => n.fills = vec![Paint::solid(Rgba::BLACK)],
            Kind::Group => {}
        }
        n
    }

    /// Build a vector node from absolute anchors; bounds come from the anchors and handles.
    pub fn vector(abs: &[PathPoint], closed: bool) -> Self {
        let mut n = Node::new(Kind::Vector, Rect::NOTHING);
        n.points = abs.to_vec();
        n.closed = closed;
        n.refit_vector();
        n
    }

    /// Re-derive `rect` from absolute-ized points so `rect` tightly wraps the path.
    pub fn refit_vector(&mut self) {
        let origin = if self.rect.is_finite() { self.rect.min.to_vec2() } else { Vec2::ZERO };
        let abs: Vec<PathPoint> = self
            .points
            .iter()
            .map(|pt| PathPoint { p: pt.p + origin, ..*pt })
            .collect();
        let mut r = Rect::NOTHING;
        for (i, pt) in abs.iter().enumerate() {
            r.extend_with(pt.p);
            // ponytail: bounds include handle tips, tight curve bounds if users notice
            if i > 0 || self.closed {
                r.extend_with(pt.p + pt.h_in);
            }
            if i + 1 < abs.len() || self.closed {
                r.extend_with(pt.p + pt.h_out);
            }
        }
        if !r.is_finite() {
            r = Rect::from_min_size(Pos2::ZERO, Vec2::ZERO);
        }
        self.points = abs.iter().map(|pt| PathPoint { p: pt.p - r.min.to_vec2(), ..*pt }).collect();
        self.rect = r;
    }

    /// World corners (top-left, top-right, bottom-right, bottom-left), rotation applied.
    pub fn corners(&self) -> [Pos2; 4] {
        let r = self.rect;
        [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()].map(|p| rotate(p, r.center(), self.rotation))
    }

    /// Axis-aligned bounds in world space.
    pub fn aabb(&self) -> Rect {
        if self.rotation == 0.0 {
            return self.rect;
        }
        Rect::from_points(&self.corners())
    }

    /// World point mapped into the node's unrotated frame.
    pub fn to_local(&self, p: Pos2) -> Pos2 {
        rotate(p, self.rect.center(), -self.rotation)
    }

    pub fn first_fill(&self) -> Option<Rgba> {
        self.fills.iter().rev().find(|p| p.visible && p.kind == PaintKind::Solid).map(|p| p.color)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Page {
    pub name: String,
    pub children: Vec<NodeId>,
    pub background: Rgba,
}

pub const PAGE_BG: Rgba = Rgba::rgb(0xF5, 0xF5, 0xF5);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Document {
    pub pages: Vec<Page>,
    pub nodes: HashMap<NodeId, Node>,
    pub next_id: NodeId,
    /// Embedded images: content hash -> encoded PNG/JPEG bytes (base64 in files).
    #[serde(default, with = "images_b64")]
    pub images: HashMap<String, std::sync::Arc<Vec<u8>>>,
}

mod images_b64 {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::HashMap;
    use std::sync::Arc;

    pub fn serialize<S: Serializer>(m: &HashMap<String, Arc<Vec<u8>>>, s: S) -> Result<S::Ok, S::Error> {
        let enc: HashMap<&String, String> = m.iter().map(|(k, v)| (k, STANDARD.encode(v.as_slice()))).collect();
        enc.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<HashMap<String, Arc<Vec<u8>>>, D::Error> {
        let enc = HashMap::<String, String>::deserialize(d)?;
        enc.into_iter()
            .map(|(k, v)| STANDARD.decode(v).map(|b| (k, Arc::new(b))).map_err(serde::de::Error::custom))
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    Forward,
    Backward,
    Front,
    Back,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            pages: vec![Page { name: "Page 1".into(), children: vec![], background: PAGE_BG }],
            nodes: HashMap::new(),
            next_id: 1,
            images: HashMap::new(),
        }
    }
}

impl Document {
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[&id]
    }
    pub fn node_mut(&mut self, id: NodeId) -> &mut Node {
        self.nodes.get_mut(&id).expect("node exists")
    }

    pub fn page_of(&self, id: NodeId) -> usize {
        let root = *self.ancestors(id).last().unwrap_or(&id);
        self.pages.iter().position(|p| p.children.contains(&root)).unwrap_or(0)
    }

    /// Parent chain, nearest first (excludes `id`).
    pub fn ancestors(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = vec![];
        let mut cur = self.nodes.get(&id).and_then(|n| n.parent);
        while let Some(p) = cur {
            out.push(p);
            cur = self.nodes[&p].parent;
        }
        out
    }

    pub fn is_ancestor(&self, anc: NodeId, id: NodeId) -> bool {
        self.ancestors(id).contains(&anc)
    }

    /// All descendants, pre-order.
    pub fn descendants(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = vec![];
        let mut stack: Vec<NodeId> = self.nodes[&id].children.iter().rev().copied().collect();
        while let Some(c) = stack.pop() {
            out.push(c);
            stack.extend(self.nodes[&c].children.iter().rev());
        }
        out
    }

    pub fn children_of(&self, page: usize, parent: Option<NodeId>) -> &Vec<NodeId> {
        match parent {
            Some(p) => &self.nodes[&p].children,
            None => &self.pages[page].children,
        }
    }

    fn children_of_mut(&mut self, page: usize, parent: Option<NodeId>) -> &mut Vec<NodeId> {
        match parent {
            Some(p) => &mut self.node_mut(p).children,
            None => &mut self.pages[page].children,
        }
    }

    /// Next default name, e.g. "Rectangle 3".
    pub fn auto_name(&self, label: &str) -> String {
        let n = self
            .nodes
            .values()
            .filter(|n| n.name.strip_prefix(label).is_some_and(|r| r.starts_with(' ')))
            .count();
        format!("{label} {}", n + 1)
    }

    /// Insert `node` under `parent` (None = page root). `index` None = on top.
    pub fn insert(&mut self, page: usize, parent: Option<NodeId>, index: Option<usize>, mut node: Node) -> NodeId {
        let id = self.next_id;
        self.next_id += 1;
        node.id = id;
        node.parent = parent;
        if node.name.is_empty() {
            node.name = self.auto_name(node.kind.label());
        }
        self.nodes.insert(id, node);
        let list = self.children_of_mut(page, parent);
        let i = index.unwrap_or(list.len()).min(list.len());
        list.insert(i, id);
        id
    }

    /// Unlink `id` from its parent list. Returns (page, parent, index).
    fn detach(&mut self, id: NodeId) -> (usize, Option<NodeId>, usize) {
        let page = self.page_of(id);
        let parent = self.nodes[&id].parent;
        let list = self.children_of_mut(page, parent);
        let i = list.iter().position(|c| *c == id).unwrap_or(0);
        list.retain(|c| *c != id);
        (page, parent, i)
    }

    pub fn index_in_parent(&self, id: NodeId) -> usize {
        let page = self.page_of(id);
        let parent = self.nodes[&id].parent;
        self.children_of(page, parent).iter().position(|c| *c == id).unwrap_or(0)
    }

    pub fn remove(&mut self, id: NodeId) {
        if !self.nodes.contains_key(&id) {
            return;
        }
        self.detach(id);
        for d in self.descendants(id) {
            self.nodes.remove(&d);
        }
        self.nodes.remove(&id);
    }

    /// Move `id` under `parent` at `index` (index is computed after detaching).
    pub fn reparent(&mut self, id: NodeId, page: usize, parent: Option<NodeId>, index: Option<usize>) {
        if parent == Some(id) || parent.is_some_and(|p| self.is_ancestor(id, p)) {
            return;
        }
        self.detach(id);
        self.node_mut(id).parent = parent;
        let list = self.children_of_mut(page, parent);
        let i = index.unwrap_or(list.len()).min(list.len());
        list.insert(i, id);
    }

    pub fn translate(&mut self, id: NodeId, d: Vec2) {
        for n in std::iter::once(id).chain(self.descendants(id)) {
            let node = self.node_mut(n);
            node.rect = node.rect.translate(d);
        }
    }

    /// Rotate a subtree by `deg` (counter-clockwise) around `pivot`.
    /// Groups stay axis-aligned and re-hug their rotated children.
    pub fn rotate(&mut self, id: NodeId, deg: f32, pivot: Pos2) {
        for n in std::iter::once(id).chain(self.descendants(id)) {
            let node = self.node_mut(n);
            let c = rotate(node.rect.center(), pivot, deg);
            node.rect = Rect::from_center_size(c, node.rect.size());
            if node.kind != Kind::Group {
                node.rotation = normalize_deg(node.rotation + deg);
            }
        }
        self.reflow();
    }

    /// Resize a node. Groups and vectors scale their contents; frame children follow their constraints.
    pub fn set_rect(&mut self, id: NodeId, r: Rect) {
        self.set_rect_with(id, r, true);
    }

    /// `constraints: false` resizes a frame without moving its children (Cmd-drag in Figma).
    pub fn set_rect_with(&mut self, id: NodeId, r: Rect, constraints: bool) {
        let old = self.nodes[&id].rect;
        let sx = if old.width() > 0.0 { r.width() / old.width() } else { 1.0 };
        let sy = if old.height() > 0.0 { r.height() / old.height() } else { 1.0 };
        let map = |p: Pos2| pos2(r.min.x + (p.x - old.min.x) * sx, r.min.y + (p.y - old.min.y) * sy);
        let node = self.node_mut(id);
        node.rect = r;
        for pt in &mut node.points {
            pt.p = pos2(pt.p.x * sx, pt.p.y * sy);
            pt.h_in = vec2(pt.h_in.x * sx, pt.h_in.y * sy);
            pt.h_out = vec2(pt.h_out.x * sx, pt.h_out.y * sy);
        }
        let (kind, mode) = (node.kind, node.layout.mode);
        let kids = node.children.clone();
        if kind == Kind::Group {
            for c in kids {
                let cr = self.nodes[&c].rect;
                self.set_rect(c, Rect::from_two_pos(map(cr.min), map(cr.max)));
            }
        } else if kind.is_frame() && mode == LayoutMode::None && constraints {
            for c in kids {
                let (cr, [ch, cv]) = (self.nodes[&c].rect, self.nodes[&c].constraints);
                let axis = |k: Constraint, lo: f32, hi: f32, olo: f32, ohi: f32, nlo: f32, nhi: f32| -> (f32, f32) {
                    match k {
                        Constraint::Min => (lo + nlo - olo, hi + nlo - olo),
                        Constraint::Max => (lo + nhi - ohi, hi + nhi - ohi),
                        Constraint::Stretch => (lo + nlo - olo, hi + nhi - ohi),
                        Constraint::Center => {
                            let d = (nlo + nhi - olo - ohi) / 2.0;
                            (lo + d, hi + d)
                        }
                        Constraint::Scale => {
                            let s = if ohi > olo { (nhi - nlo) / (ohi - olo) } else { 1.0 };
                            (nlo + (lo - olo) * s, nlo + (hi - olo) * s)
                        }
                    }
                };
                let (x0, x1) = axis(ch, cr.min.x, cr.max.x, old.min.x, old.max.x, r.min.x, r.max.x);
                let (y0, y1) = axis(cv, cr.min.y, cr.max.y, old.min.y, old.max.y, r.min.y, r.max.y);
                let nr = Rect::from_min_max(pos2(x0, y0), pos2(x1.max(x0), y1.max(y0)));
                if (nr.size() - cr.size()).length() < 1e-4 {
                    self.translate(c, nr.min - cr.min);
                } else {
                    self.set_rect(c, nr);
                }
            }
        }
    }

    /// Create an instance of `comp` with its top-left at `at`.
    pub fn instantiate(&mut self, comp: NodeId, page: usize, parent: Option<NodeId>, index: Option<usize>, at: Pos2) -> NodeId {
        let c = self.nodes[&comp].clone();
        let mut n = Node { children: vec![], name: c.name.clone(), ..c.clone() };
        n.kind = Kind::Instance;
        n.component = Some(comp);
        n.source = Some(comp);
        n.synced = Some(Box::new(Node { children: vec![], ..c }));
        n.rect = Rect::from_min_size(at, n.rect.size());
        let id = self.insert(page, parent, index, n);
        self.sync_instance(id);
        self.reflow();
        id
    }

    /// Turn an instance back into a plain frame; nested instances stay instances.
    pub fn detach_instance(&mut self, inst: NodeId) {
        let n = self.node_mut(inst);
        n.kind = Kind::Frame;
        n.component = None;
        n.source = None;
        n.synced = None;
        for d in self.descendants(inst) {
            if self.ancestors(d).iter().take_while(|a| **a != inst).any(|a| self.nodes[a].kind == Kind::Instance) {
                continue;
            }
            let n = self.node_mut(d);
            n.source = None;
            n.synced = None;
        }
    }

    /// Drop all overrides on an instance.
    pub fn reset_instance(&mut self, inst: NodeId) {
        for id in std::iter::once(inst).chain(self.descendants(inst)) {
            if let Some(src) = self.nodes[&id].source.and_then(|s| self.nodes.get(&s)).cloned() {
                let n = self.node_mut(id);
                let keep = n.rect;
                *n = Node { id: n.id, parent: n.parent, children: n.children.clone(), kind: n.kind, component: n.component, source: n.source, synced: None, ..src };
                n.rect = keep;
                n.synced = Some(Box::new(Node { children: vec![], ..n.clone() }));
            }
        }
        if let Some(c) = self.nodes[&inst].component.and_then(|c| self.nodes.get(&c)) {
            let size = c.rect.size();
            let n = self.node_mut(inst);
            n.rect = Rect::from_min_size(n.rect.min, size);
        }
        self.sync_instance(inst);
        self.reflow();
    }

    /// Bring an instance in line with its main component, keeping overrides.
    pub fn sync_instance(&mut self, inst: NodeId) {
        let Some(comp) = self.nodes[&inst].component.filter(|c| self.nodes.contains_key(c)) else { return };
        if comp == inst || self.is_ancestor(inst, comp) {
            return;
        }
        let page = self.page_of(inst);
        let c = self.nodes[&comp].clone();
        // Instance layers by the component layer they mirror.
        let mut by_source: HashMap<NodeId, NodeId> = HashMap::new();
        for d in self.descendants(inst) {
            if let Some(s) = self.nodes[&d].source {
                by_source.entry(s).or_insert(d);
            }
        }
        // Root: merge appearance, keep placement; size follows the main unless resized.
        let target_size = {
            let n = &self.nodes[&inst];
            let snap = n.synced.as_deref();
            if snap.is_none_or(|s| s.rect.size() == n.rect.size()) { c.rect.size() } else { n.rect.size() }
        };
        let actual = Rect::from_min_size(self.nodes[&inst].rect.min, target_size);
        merge_from(self.node_mut(inst), &c);
        self.node_mut(inst).rect = Rect::from_min_size(actual.min, c.rect.size());
        let offset = actual.min - c.rect.min;
        // Children, in component order.
        let mut keep = std::collections::HashSet::new();
        let mut order: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        for cid in self.descendants(comp) {
            let src = self.nodes[&cid].clone();
            let parent = if src.parent == Some(comp) { inst } else { by_source[&src.parent.unwrap()] };
            let id = match by_source.get(&cid) {
                Some(&e) => {
                    merge_from(self.node_mut(e), &src);
                    if self.nodes[&e].parent != Some(parent) {
                        self.reparent(e, page, Some(parent), None);
                    }
                    e
                }
                None => {
                    let mut n = Node { children: vec![], ..src.clone() };
                    n.source = Some(cid);
                    n.synced = Some(Box::new(Node { children: vec![], ..src.clone() }));
                    // Nested instances keep pointing at their own component.
                    let id = self.insert(page, Some(parent), None, n);
                    by_source.insert(cid, id);
                    id
                }
            };
            let text = self.nodes[&id].kind == Kind::Text;
            let n = self.node_mut(id);
            n.rect = if text {
                Rect::from_min_size(src.rect.min + offset, n.rect.size())
            } else {
                Rect::from_min_size(src.rect.min + offset, src.rect.size())
            };
            keep.insert(id);
            order.entry(parent).or_default().push(id);
        }
        for d in self.descendants(inst) {
            if !keep.contains(&d) && self.nodes.get(&d).is_some_and(|n| n.source.is_some()) {
                self.remove(d);
            }
        }
        for (parent, kids) in order {
            let extra: Vec<NodeId> = self.nodes[&parent].children.iter().copied().filter(|k| !kids.contains(k)).collect();
            self.node_mut(parent).children = kids.into_iter().chain(extra).collect();
        }
        // Resized instances re-apply constraints from the component's size.
        if actual.size() != c.rect.size() {
            self.set_rect(inst, actual);
        }
    }

    /// Re-derive everything computed from children: group bounds and auto layout.
    pub fn reflow(&mut self) {
        let instances: Vec<NodeId> = self.nodes.values().filter(|n| n.kind == Kind::Instance).map(|n| n.id).collect();
        // ponytail: syncs every instance on every edit; track dirty components when files get large
        for _ in 0..2 {
            for &i in &instances {
                if self.nodes.contains_key(&i) {
                    self.sync_instance(i);
                }
            }
        }
        self.reflow_layout();
    }

    fn reflow_layout(&mut self) {
        self.fit_groups_only();
        let mut frames: Vec<(usize, NodeId)> = self
            .nodes
            .values()
            .filter(|n| n.kind.is_frame() && n.layout.mode != LayoutMode::None)
            .map(|n| (self.ancestors(n.id).len(), n.id))
            .collect();
        if frames.is_empty() {
            return;
        }
        // Deepest first so hugging frames know their content, then top-down so Fill sizes flow into children.
        frames.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        for &(_, f) in &frames {
            self.layout_frame(f);
        }
        self.fit_groups_only();
        for &(_, f) in frames.iter().rev() {
            self.layout_frame(f);
        }
        self.fit_groups_only();
    }

    fn layout_frame(&mut self, f: NodeId) {
        let n = self.nodes[&f].clone();
        let l = n.layout;
        let h = l.mode == LayoutMode::Horizontal;
        let (ma, ca) = if h { (0, 1) } else { (1, 0) };
        let main = |v: Vec2| if h { v.x } else { v.y };
        let cross = |v: Vec2| if h { v.y } else { v.x };
        let along = |m: f32, c: f32| if h { vec2(m, c) } else { vec2(c, m) };
        let [pt, pr, pb, pl] = l.padding;
        let (p0, p1, q0, q1) = if h { (pl, pr, pt, pb) } else { (pt, pb, pl, pr) };
        let kids: Vec<NodeId> = n
            .children
            .iter()
            .copied()
            .filter(|c| self.nodes[c].visible && !self.nodes[c].absolute)
            .collect();
        // Text is always auto-width, so it can only hug.
        let fills = |k: &Node, axis: usize| k.sizing[axis] == Sizing::Fill && k.kind != Kind::Text;
        let count = kids.len() as f32;
        let gaps = l.gap * (count - 1.0).max(0.0);
        let fixed_main: f32 = kids.iter().map(|c| &self.nodes[c]).filter(|k| !fills(k, ma)).map(|k| main(k.aabb().size())).sum();
        let n_fill = kids.iter().filter(|c| fills(&self.nodes[c], ma)).count() as f32;

        let mut size = n.rect.size();
        if n.sizing[ma] == Sizing::Hug {
            let fill_now: f32 = kids.iter().map(|c| &self.nodes[c]).filter(|k| fills(k, ma)).map(|k| main(k.aabb().size())).sum();
            let m = p0 + p1 + fixed_main + fill_now + if l.space_between { 0.0 } else { gaps };
            size = along(m, cross(size));
        }
        if n.sizing[ca] == Sizing::Hug {
            let c = kids.iter().map(|c| &self.nodes[c]).filter(|k| !fills(k, ca)).map(|k| cross(k.aabb().size())).fold(0.0, f32::max);
            size = along(main(size), q0 + q1 + c);
        }
        if size != n.rect.size() {
            self.node_mut(f).rect = Rect::from_min_size(n.rect.min, size);
        }
        let inner_main = (main(size) - p0 - p1).max(0.0);
        let inner_cross = (cross(size) - q0 - q1).max(0.0);
        let free = inner_main - fixed_main - gaps;
        let fill_main = if n_fill > 0.0 { (free / n_fill).max(0.0) } else { 0.0 };
        let gap = if l.space_between && n_fill == 0.0 && count > 1.0 { (inner_main - fixed_main) / (count - 1.0) } else { l.gap };
        let lead = if n_fill > 0.0 || l.space_between {
            0.0
        } else {
            match l.main {
                AxisAlign::Min => 0.0,
                AxisAlign::Center => free / 2.0,
                AxisAlign::Max => free,
            }
        };
        let origin = n.rect.min;
        let mut cursor = main(origin.to_vec2()) + p0 + lead;
        for c in kids {
            let k = self.nodes[&c].clone();
            let mut ks = k.aabb().size();
            if fills(&k, ma) {
                ks = along(fill_main, cross(ks));
            }
            if fills(&k, ca) {
                ks = along(main(ks), inner_cross);
            }
            if (ks - k.aabb().size()).length() > 1e-4 && k.rotation == 0.0 {
                self.set_rect(c, Rect::from_min_size(k.rect.min, ks));
            }
            let off = match l.cross {
                AxisAlign::Min => 0.0,
                AxisAlign::Center => (inner_cross - cross(ks)) / 2.0,
                AxisAlign::Max => inner_cross - cross(ks),
            };
            let target = along(cursor, cross(origin.to_vec2()) + q0 + off).to_pos2();
            let cur = self.nodes[&c].aabb().min;
            if cur != target {
                self.translate(c, target - cur);
            }
            cursor += main(ks) + gap;
        }
    }

    /// Groups always hug their children; empty groups disappear.
    fn fit_groups_only(&mut self) {
        let mut groups: Vec<(usize, NodeId)> = self
            .nodes
            .values()
            .filter(|n| n.kind == Kind::Group)
            .map(|n| (self.ancestors(n.id).len(), n.id))
            .collect();
        groups.sort_by(|a, b| b.0.cmp(&a.0)); // deepest first
        for (_, g) in groups {
            let kids = self.nodes[&g].children.clone();
            if kids.is_empty() {
                self.remove(g);
                continue;
            }
            let r = kids.iter().fold(Rect::NOTHING, |r, c| r.union(self.nodes[c].aabb()));
            self.node_mut(g).rect = r;
        }
    }

    /// Siblings sorted bottom-to-top, as they appear in their parent list.
    pub fn sort_by_z(&self, ids: &[NodeId]) -> Vec<NodeId> {
        let mut v: Vec<(Vec<usize>, NodeId)> = ids.iter().map(|&id| (self.z_path(id), id)).collect();
        v.sort();
        v.into_iter().map(|(_, id)| id).collect()
    }

    /// Path of indices from the page root to `id`; sorts in paint order.
    pub fn z_path(&self, id: NodeId) -> Vec<usize> {
        let mut path: Vec<usize> = std::iter::once(id)
            .chain(self.ancestors(id))
            .map(|n| self.index_in_parent(n))
            .collect();
        path.reverse();
        path
    }

    pub fn group(&mut self, ids: &[NodeId]) -> Option<NodeId> {
        let ids = self.sort_by_z(ids);
        let top = *ids.last()?;
        let page = self.page_of(top);
        let parent = self.nodes[&top].parent;
        let index = self.index_in_parent(top);
        let g = self.insert(page, parent, Some(index + 1), Node::new(Kind::Group, Rect::NOTHING));
        for id in ids {
            self.reparent(id, page, Some(g), None);
        }
        self.reflow();
        Some(g)
    }

    /// Dissolve a group or frame, keeping its children in place. Returns the children.
    pub fn ungroup(&mut self, id: NodeId) -> Vec<NodeId> {
        let page = self.page_of(id);
        let parent = self.nodes[&id].parent;
        let index = self.index_in_parent(id);
        let kids = self.nodes[&id].children.clone();
        for (i, c) in kids.iter().enumerate() {
            self.reparent(*c, page, parent, Some(index + 1 + i));
        }
        self.remove(id);
        self.reflow();
        kids
    }

    pub fn reorder(&mut self, id: NodeId, order: Order) {
        let page = self.page_of(id);
        let parent = self.nodes[&id].parent;
        let list = self.children_of_mut(page, parent);
        let Some(i) = list.iter().position(|c| *c == id) else { return };
        let last = list.len() - 1;
        let j = match order {
            Order::Forward => (i + 1).min(last),
            Order::Backward => i.saturating_sub(1),
            Order::Front => last,
            Order::Back => 0,
        };
        let n = list.remove(i);
        list.insert(j, n);
    }

    /// Copy subtrees into a standalone fragment (roots first, in z order).
    pub fn extract(&self, ids: &[NodeId]) -> Vec<Node> {
        let mut out = vec![];
        for id in self.sort_by_z(ids) {
            out.push(self.nodes[&id].clone());
            out.extend(self.descendants(id).into_iter().map(|d| self.nodes[&d].clone()));
        }
        out
    }

    /// Insert a fragment from `extract` under `parent` with fresh ids. Returns new root ids.
    pub fn paste(&mut self, frag: &[Node], page: usize, parent: Option<NodeId>, index: Option<usize>) -> Vec<NodeId> {
        let in_frag: std::collections::HashSet<NodeId> = frag.iter().map(|n| n.id).collect();
        let mut map: HashMap<NodeId, NodeId> = HashMap::new();
        let mut roots = vec![];
        let mut at = index;
        for n in frag {
            let mut copy = n.clone();
            copy.children.clear();
            let is_root = !n.parent.is_some_and(|p| in_frag.contains(&p));
            let (par, idx) = if is_root { (parent, at) } else { (Some(map[&n.parent.unwrap()]), None) };
            let new = self.insert(page, par, idx, copy);
            map.insert(n.id, new);
            if is_root {
                roots.push(new);
                at = at.map(|i| i + 1);
            }
        }
        roots
    }

    pub fn duplicate(&mut self, ids: &[NodeId]) -> Vec<NodeId> {
        let mut out = vec![];
        for id in self.sort_by_z(ids) {
            let page = self.page_of(id);
            let parent = self.nodes[&id].parent;
            let index = self.index_in_parent(id);
            let frag = self.extract(&[id]);
            out.extend(self.paste(&frag, page, parent, Some(index + 1)));
        }
        out
    }

    /// Deepest visible, unlocked node under `p` (topmost in paint order).
    pub fn hit(&self, page: usize, p: Pos2, tol: f32) -> Option<NodeId> {
        self.hit_in(&self.pages[page].children, p, tol)
    }

    fn hit_in(&self, list: &[NodeId], p: Pos2, tol: f32) -> Option<NodeId> {
        for &id in list.iter().rev() {
            let n = &self.nodes[&id];
            if !n.visible || n.locked {
                continue;
            }
            if n.kind.is_frame() && n.clip && !n.rect.expand(tol).contains(n.to_local(p)) {
                continue;
            }
            if let Some(h) = self.hit_in(&n.children, p, tol) {
                return Some(h);
            }
            if self.hit_self(n, p, tol) {
                return Some(id);
            }
        }
        None
    }

    fn hit_self(&self, n: &Node, p: Pos2, tol: f32) -> bool {
        let p = n.to_local(p);
        match n.kind {
            Kind::Group => false,
            Kind::Frame | Kind::Component | Kind::Instance | Kind::Rectangle | Kind::Text => n.rect.expand(tol).contains(p),
            Kind::Ellipse => {
                let c = n.rect.center();
                let r = n.rect.size() / 2.0 + Vec2::splat(tol);
                let d = p - c;
                (d.x / r.x).powi(2) + (d.y / r.y).powi(2) <= 1.0
            }
            Kind::Vector => {
                let filled = n.closed && n.fills.iter().any(|f| f.visible);
                let poly = flatten(n);
                if filled && point_in_polygon(&poly, p) {
                    return true;
                }
                let reach = tol + n.stroke_width / 2.0;
                let segs = poly.len().saturating_sub(if n.closed { 0 } else { 1 });
                (0..segs).any(|i| seg_dist(p, poly[i], poly[(i + 1) % poly.len()]) <= reach)
            }
        }
    }

    pub fn bounds(&self, ids: &[NodeId]) -> Rect {
        ids.iter().fold(Rect::NOTHING, |r, id| r.union(self.nodes[id].aabb()))
    }
}

/// Copy each property from `src` unless the instance layer overrode it
/// (its value no longer matches what was last copied).
fn merge_from(e: &mut Node, src: &Node) {
    let snap = e.synced.take().map(|b| *b).unwrap_or_else(|| src.clone());
    macro_rules! take {
        ($($f:ident),*) => {$(
            if e.$f == snap.$f {
                e.$f = src.$f.clone();
            }
        )*};
    }
    take!(name, visible, opacity, fills, strokes, stroke_width, stroke_align, radius, clip, shadows, layout, sizing, constraints, absolute, points, closed, text, font_size, font_weight, rotation, component);
    e.synced = Some(Box::new(Node { children: vec![], synced: None, ..src.clone() }));
}

/// Rotate `p` around `c` by `deg` counter-clockwise on screen (y points down).
pub fn rotate(p: Pos2, c: Pos2, deg: f32) -> Pos2 {
    if deg == 0.0 {
        return p;
    }
    // Exact quarter turns keep coordinates on whole pixels.
    let (s, co) = match normalize_deg(deg) {
        90.0 => (-1.0, 0.0),
        180.0 => (0.0, -1.0),
        -90.0 => (1.0, 0.0),
        _ => (-deg.to_radians()).sin_cos(),
    };
    let d = p - c;
    c + vec2(d.x * co - d.y * s, d.x * s + d.y * co)
}

/// Wrap to (-180, 180].
pub fn normalize_deg(d: f32) -> f32 {
    let r = d.rem_euclid(360.0);
    if r > 180.0 { r - 360.0 } else { r }
}

/// Absolute polyline approximating a vector node (16 steps per curve).
pub fn flatten(n: &Node) -> Vec<Pos2> {
    let o = n.rect.min.to_vec2();
    let pts = &n.points;
    let mut out = vec![];
    let segs = if n.closed { pts.len() } else { pts.len().saturating_sub(1) };
    if let Some(first) = pts.first() {
        out.push(first.p + o);
    }
    for i in 0..segs {
        let a = pts[i];
        let b = pts[(i + 1) % pts.len()];
        let (p0, p1, p2, p3) = (a.p + o, a.p + a.h_out + o, b.p + b.h_in + o, b.p + o);
        for s in 1..=16 {
            let t = s as f32 / 16.0;
            let u = 1.0 - t;
            let v = p0.to_vec2() * u * u * u
                + p1.to_vec2() * 3.0 * u * u * t
                + p2.to_vec2() * 3.0 * u * t * t
                + p3.to_vec2() * t * t * t;
            out.push(v.to_pos2());
        }
    }
    out
}

fn point_in_polygon(poly: &[Pos2], p: Pos2) -> bool {
    let mut inside = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + n - 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
    }
    inside
}

fn seg_dist(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = if ab.length_sq() > 0.0 { ((p - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
    (a + ab * t).distance(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_min_size(pos2(x, y), vec2(w, h))
    }

    #[test]
    fn insert_names_and_order() {
        let mut d = Document::default();
        let a = d.insert(0, None, None, Node::new(Kind::Rectangle, r(0., 0., 10., 10.)));
        let b = d.insert(0, None, None, Node::new(Kind::Rectangle, r(5., 5., 10., 10.)));
        assert_eq!(d.node(a).name, "Rectangle 1");
        assert_eq!(d.node(b).name, "Rectangle 2");
        assert_eq!(d.hit(0, pos2(7., 7.), 0.), Some(b), "topmost wins");
        d.reorder(b, Order::Back);
        assert_eq!(d.hit(0, pos2(7., 7.), 0.), Some(a));
    }

    #[test]
    fn group_ungroup_and_scale() {
        let mut d = Document::default();
        let a = d.insert(0, None, None, Node::new(Kind::Rectangle, r(0., 0., 10., 10.)));
        let b = d.insert(0, None, None, Node::new(Kind::Ellipse, r(20., 20., 10., 10.)));
        let g = d.group(&[b, a]).unwrap();
        assert_eq!(d.node(g).rect, r(0., 0., 30., 30.));
        assert_eq!(d.node(g).children, vec![a, b], "z order kept");
        assert_eq!(d.pages[0].children, vec![g]);
        d.set_rect(g, r(0., 0., 60., 60.));
        assert_eq!(d.node(b).rect, r(40., 40., 20., 20.));
        d.translate(g, vec2(1., 1.));
        assert_eq!(d.node(a).rect, r(1., 1., 20., 20.));
        assert_eq!(d.ungroup(g), vec![a, b]);
        assert_eq!(d.pages[0].children, vec![a, b]);
        assert!(!d.nodes.contains_key(&g));
    }

    #[test]
    fn empty_group_is_removed() {
        let mut d = Document::default();
        let a = d.insert(0, None, None, Node::new(Kind::Rectangle, r(0., 0., 10., 10.)));
        let g = d.group(&[a]).unwrap();
        d.remove(a);
        d.reflow();
        assert!(!d.nodes.contains_key(&g));
        assert!(d.pages[0].children.is_empty());
    }

    #[test]
    fn frame_clips_hits_and_keeps_children_on_resize() {
        let mut d = Document::default();
        let f = d.insert(0, None, None, Node::new(Kind::Frame, r(0., 0., 100., 100.)));
        let c = d.insert(0, Some(f), None, Node::new(Kind::Rectangle, r(90., 90., 50., 50.)));
        assert_eq!(d.hit(0, pos2(95., 95.), 0.), Some(c));
        assert_eq!(d.hit(0, pos2(120., 120.), 0.), None, "clipped");
        d.set_rect(f, r(0., 0., 200., 200.));
        assert_eq!(d.node(c).rect, r(90., 90., 50., 50.));
    }

    #[test]
    fn duplicate_and_paste_deep() {
        let mut d = Document::default();
        let f = d.insert(0, None, None, Node::new(Kind::Frame, r(0., 0., 100., 100.)));
        d.insert(0, Some(f), None, Node::new(Kind::Rectangle, r(1., 1., 5., 5.)));
        let dup = d.duplicate(&[f]);
        assert_eq!(d.pages[0].children, vec![f, dup[0]]);
        assert_eq!(d.node(dup[0]).children.len(), 1);
        let kid = d.node(dup[0]).children[0];
        assert_eq!(d.node(kid).parent, Some(dup[0]));
        assert_eq!(d.nodes.len(), 4);
    }

    #[test]
    fn vector_hit_and_bounds() {
        let pts = [
            PathPoint { p: pos2(10., 10.), ..Default::default() },
            PathPoint { p: pos2(110., 10.), ..Default::default() },
        ];
        let mut d = Document::default();
        let v = d.insert(0, None, None, Node::vector(&pts, false));
        assert_eq!(d.node(v).rect, r(10., 10., 100., 0.));
        assert_eq!(d.hit(0, pos2(50., 11.), 1.), Some(v));
        assert_eq!(d.hit(0, pos2(50., 20.), 1.), None);
        d.set_rect(v, r(10., 10., 50., 0.));
        assert_eq!(d.node(v).points[1].p, pos2(50., 0.));
    }

    #[test]
    fn rotation_hits_and_bounds() {
        let mut d = Document::default();
        let a = d.insert(0, None, None, Node::new(Kind::Rectangle, r(0., 0., 100., 10.)));
        d.rotate(a, 90.0, pos2(50., 5.));
        let n = d.node(a);
        assert_eq!(n.rotation, 90.0);
        let b = n.aabb();
        assert!((b.width() - 10.0).abs() < 0.01 && (b.height() - 100.0).abs() < 0.01, "{b:?}");
        assert_eq!(d.hit(0, pos2(50., 40.), 0.), Some(a), "inside rotated box");
        assert_eq!(d.hit(0, pos2(90., 5.), 0.), None, "outside rotated box");
        // Counter-clockwise: the right end swings up.
        let tr = n.corners()[1];
        assert!(tr.y < -40.0, "{tr:?}");
        // Groups re-hug rotated children and carry them around a pivot.
        let g = d.group(&[a]).unwrap();
        d.rotate(g, 90.0, pos2(50., 5.));
        assert_eq!(d.node(a).rotation, 180.0);
        assert_eq!(d.node(g).rotation, 0.0);
        assert!((d.node(g).rect.width() - 100.0).abs() < 0.01);
    }

    #[test]
    fn gradients_sample_and_convert() {
        let mut p = Paint::solid(Rgba::rgb(255, 0, 0));
        p.convert(PaintKind::Linear);
        assert_eq!(p.stops[0].1, Rgba::rgb(255, 0, 0));
        assert_eq!(p.sample(0.5), Rgba([255, 0, 0, 128]), "fades without darkening");
        assert_eq!(p.sample(1.0).a(), 0);
        assert_eq!(p.gradient_t(pos2(0.3, 0.5)), 0.5, "top-to-bottom by default");
        p.convert(PaintKind::Radial);
        assert_eq!(p.gradient_t(pos2(0.5, 0.0)), 1.0);
        p.convert(PaintKind::Solid);
        assert_eq!(p.color, Rgba::rgb(255, 0, 0));
    }

    #[test]
    fn images_roundtrip_through_json() {
        let mut d = Document::default();
        d.images.insert("abc".into(), std::sync::Arc::new(vec![1, 2, 3, 250]));
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains("AQID+g=="), "{json}");
        let back: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(back.images["abc"].as_slice(), &[1, 2, 3, 250]);
    }

    fn al_frame(d: &mut Document, mode: LayoutMode) -> NodeId {
        let mut f = Node::new(Kind::Frame, r(0., 0., 300., 100.));
        f.layout = AutoLayout { mode, gap: 10., padding: [5., 5., 5., 5.], ..Default::default() };
        d.insert(0, None, None, f)
    }

    #[test]
    fn auto_layout_stacks_hugs_and_fills() {
        let mut d = Document::default();
        let f = al_frame(&mut d, LayoutMode::Horizontal);
        let a = d.insert(0, Some(f), None, Node::new(Kind::Rectangle, r(100., 100., 20., 30.)));
        let b = d.insert(0, Some(f), None, Node::new(Kind::Rectangle, r(0., 0., 40., 10.)));
        d.reflow();
        assert_eq!(d.node(a).rect, r(5., 5., 20., 30.));
        assert_eq!(d.node(b).rect, r(35., 5., 40., 10.));
        // Hug both axes.
        d.node_mut(f).sizing = [Sizing::Hug, Sizing::Hug];
        d.reflow();
        assert_eq!(d.node(f).rect, r(0., 0., 80., 40.));
        // Fill: b takes the rest of a fixed 300 wide frame; cross fill stretches.
        d.node_mut(f).sizing = [Sizing::Fixed, Sizing::Fixed];
        d.node_mut(f).rect = r(0., 0., 300., 100.);
        d.node_mut(b).sizing = [Sizing::Fill, Sizing::Fill];
        d.reflow();
        assert_eq!(d.node(b).rect, r(35., 5., 260., 90.));
        // Centered, vertical, space-between.
        d.node_mut(b).sizing = [Sizing::Fixed, Sizing::Fixed];
        d.node_mut(b).rect = r(0., 0., 40., 10.);
        d.node_mut(f).layout.mode = LayoutMode::Vertical;
        d.node_mut(f).layout.space_between = true;
        d.node_mut(f).layout.cross = AxisAlign::Center;
        d.reflow();
        assert_eq!(d.node(a).rect.min, pos2(140., 5.));
        assert_eq!(d.node(b).rect.max.y, 95.);
        // Absolute children are left alone; hidden ones don't take space.
        d.node_mut(a).absolute = true;
        d.node_mut(a).rect = r(200., 50., 20., 30.);
        d.reflow();
        assert_eq!(d.node(a).rect, r(200., 50., 20., 30.));
    }

    #[test]
    fn nested_auto_layout_hug_propagates() {
        let mut d = Document::default();
        let outer = al_frame(&mut d, LayoutMode::Vertical);
        d.node_mut(outer).sizing = [Sizing::Hug, Sizing::Hug];
        let inner = d.insert(0, Some(outer), None, {
            let mut f = Node::new(Kind::Frame, r(0., 0., 10., 10.));
            f.layout = AutoLayout { mode: LayoutMode::Horizontal, gap: 4., ..Default::default() };
            f.sizing = [Sizing::Hug, Sizing::Hug];
            f
        });
        d.insert(0, Some(inner), None, Node::new(Kind::Rectangle, r(0., 0., 30., 30.)));
        d.insert(0, Some(inner), None, Node::new(Kind::Rectangle, r(0., 0., 30., 20.)));
        d.reflow();
        assert_eq!(d.node(inner).rect, r(5., 5., 64., 30.));
        assert_eq!(d.node(outer).rect, r(0., 0., 74., 40.));
    }

    #[test]
    fn constraints_follow_frame_resize() {
        let mut d = Document::default();
        let f = d.insert(0, None, None, Node::new(Kind::Frame, r(0., 0., 100., 100.)));
        let mk = |d: &mut Document, h: Constraint, v: Constraint| {
            let mut n = Node::new(Kind::Rectangle, r(10., 10., 20., 20.));
            n.constraints = [h, v];
            d.insert(0, Some(f), None, n)
        };
        let left = mk(&mut d, Constraint::Min, Constraint::Min);
        let right = mk(&mut d, Constraint::Max, Constraint::Max);
        let both = mk(&mut d, Constraint::Stretch, Constraint::Center);
        let scale = mk(&mut d, Constraint::Scale, Constraint::Scale);
        d.set_rect(f, r(-50., 0., 200., 200.));
        assert_eq!(d.node(left).rect, r(-40., 10., 20., 20.), "left edge moved, child follows it");
        assert_eq!(d.node(right).rect, r(60., 110., 20., 20.));
        assert_eq!(d.node(both).rect, r(-40., 60., 120., 20.));
        assert_eq!(d.node(scale).rect, r(-30., 20., 40., 40.));
        d.set_rect_with(f, r(0., 0., 100., 100.), false);
        assert_eq!(d.node(left).rect, r(-40., 10., 20., 20.), "Cmd-resize ignores constraints");
    }

    #[test]
    fn instances_follow_main_and_keep_overrides() {
        let mut d = Document::default();
        let mut comp = Node::new(Kind::Component, r(0., 0., 100., 50.));
        comp.name = "Button".into();
        let c = d.insert(0, None, None, comp);
        let mut label = Node::new(Kind::Text, r(10., 10., 30., 15.));
        label.text = "OK".into();
        let t = d.insert(0, Some(c), None, label);
        let bg = d.insert(0, Some(c), Some(0), Node::new(Kind::Rectangle, r(0., 0., 100., 50.)));
        let i = d.instantiate(c, 0, None, None, pos2(200., 0.));
        let inst = d.node(i).clone();
        assert_eq!((inst.kind, inst.name.as_str(), inst.rect), (Kind::Instance, "Button", r(200., 0., 100., 50.)));
        assert_eq!(inst.children.len(), 2);
        let (ibg, it) = (inst.children[0], inst.children[1]);
        assert_eq!(d.node(it).text, "OK");
        assert_eq!(d.node(it).rect.min, pos2(210., 10.));
        assert_eq!(d.node(ibg).source, Some(bg));

        // Override the label text, then change the main: fill propagates, text override sticks.
        d.node_mut(it).text = "Cancel".into();
        d.node_mut(bg).fills = vec![Paint::solid(Rgba::rgb(255, 0, 0))];
        d.node_mut(t).text = "Okay".into();
        d.node_mut(t).font_size = 20.0;
        d.reflow();
        assert_eq!(d.node(ibg).fills[0].color, Rgba::rgb(255, 0, 0));
        assert_eq!(d.node(it).text, "Cancel", "override kept");
        assert_eq!(d.node(it).font_size, 20.0, "non-overridden prop updates");

        // Structure changes flow through; reset drops overrides.
        let dot = d.insert(0, Some(c), None, Node::new(Kind::Ellipse, r(80., 5., 10., 10.)));
        d.reflow();
        assert_eq!(d.node(i).children.len(), 3);
        assert_eq!(d.node(*d.node(i).children.last().unwrap()).rect.min, pos2(280., 5.));
        d.remove(dot);
        d.reflow();
        assert_eq!(d.node(i).children.len(), 2);
        d.reset_instance(i);
        assert_eq!(d.node(it).text, "Okay");

        // Moving the main doesn't move instances; detaching cuts the link.
        d.translate(c, vec2(-50., 0.));
        d.reflow();
        assert_eq!(d.node(i).rect.min, pos2(200., 0.));
        d.detach_instance(i);
        d.node_mut(t).text = "Later".into();
        d.reflow();
        assert_eq!(d.node(i).kind, Kind::Frame);
        assert_eq!(d.node(it).text, "Okay");
    }

    #[test]
    fn resized_instance_applies_constraints() {
        let mut d = Document::default();
        let c = d.insert(0, None, None, Node::new(Kind::Component, r(0., 0., 100., 50.)));
        let mut pin = Node::new(Kind::Rectangle, r(80., 0., 20., 20.));
        pin.constraints = [Constraint::Max, Constraint::Min];
        d.insert(0, Some(c), None, pin);
        let i = d.instantiate(c, 0, None, None, pos2(0., 100.));
        d.set_rect(i, r(0., 100., 200., 50.));
        d.reflow();
        let kid = d.node(i).children[0];
        assert_eq!(d.node(kid).rect, r(180., 100., 20., 20.), "pinned right");
        assert_eq!(d.node(i).rect.width(), 200.0, "size override kept");
    }

    #[test]
    fn reparent_rejects_cycles() {
        let mut d = Document::default();
        let f = d.insert(0, None, None, Node::new(Kind::Frame, r(0., 0., 100., 100.)));
        let g = d.insert(0, Some(f), None, Node::new(Kind::Frame, r(0., 0., 50., 50.)));
        d.reparent(f, 0, Some(g), None);
        assert_eq!(d.node(f).parent, None);
    }
}
