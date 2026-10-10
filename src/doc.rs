//! Document model: pages hold a tree of nodes. Coordinates are absolute world units.

use egui::{Color32, Pos2, Rect, Vec2, pos2, vec2};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub type NodeId = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Kind {
    Frame,
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
            Kind::Group => "Group",
            Kind::Rectangle => "Rectangle",
            Kind::Ellipse => "Ellipse",
            Kind::Vector => "Vector",
            Kind::Text => "Text",
        }
    }
    pub fn is_container(self) -> bool {
        matches!(self, Kind::Frame | Kind::Group)
    }
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

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Paint {
    pub color: Color32,
    pub visible: bool,
}

impl Paint {
    pub fn solid(color: Color32) -> Self {
        Self { color, visible: true }
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
            points: vec![],
            closed: false,
            text: String::new(),
            font_size: 12.0,
            font_weight: 400,
            expanded: true,
        }
    }
}

pub const SHAPE_GRAY: Color32 = Color32::from_rgb(0xD9, 0xD9, 0xD9);

impl Node {
    /// A node with Figma's defaults for its kind.
    pub fn new(kind: Kind, rect: Rect) -> Self {
        let mut n = Node { kind, rect, ..Default::default() };
        match kind {
            Kind::Frame => {
                n.fills = vec![Paint::solid(Color32::WHITE)];
                n.clip = true;
            }
            Kind::Rectangle | Kind::Ellipse => n.fills = vec![Paint::solid(SHAPE_GRAY)],
            Kind::Vector => {
                n.strokes = vec![Paint::solid(Color32::BLACK)];
                n.stroke_align = StrokeAlign::Center;
            }
            Kind::Text => n.fills = vec![Paint::solid(Color32::BLACK)],
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

    pub fn first_fill(&self) -> Option<Color32> {
        self.fills.iter().rev().find(|p| p.visible).map(|p| p.color)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Page {
    pub name: String,
    pub children: Vec<NodeId>,
    pub background: Color32,
}

pub const PAGE_BG: Color32 = Color32::from_rgb(0xF5, 0xF5, 0xF5);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Document {
    pub pages: Vec<Page>,
    pub nodes: HashMap<NodeId, Node>,
    pub next_id: NodeId,
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
        self.fit_groups();
    }

    /// Resize a node. Groups and vectors scale their contents; frames keep children in place.
    pub fn set_rect(&mut self, id: NodeId, r: Rect) {
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
        if node.kind == Kind::Group {
            for c in node.children.clone() {
                let cr = self.nodes[&c].rect;
                self.set_rect(c, Rect::from_two_pos(map(cr.min), map(cr.max)));
            }
        }
    }

    /// Groups always hug their children; empty groups disappear.
    pub fn fit_groups(&mut self) {
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
        self.fit_groups();
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
        self.fit_groups();
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
            if n.kind == Kind::Frame && n.clip && !n.rect.expand(tol).contains(n.to_local(p)) {
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
            Kind::Frame | Kind::Rectangle | Kind::Text => n.rect.expand(tol).contains(p),
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
        d.fit_groups();
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
    fn reparent_rejects_cycles() {
        let mut d = Document::default();
        let f = d.insert(0, None, None, Node::new(Kind::Frame, r(0., 0., 100., 100.)));
        let g = d.insert(0, Some(f), None, Node::new(Kind::Frame, r(0., 0., 50., 50.)));
        d.reparent(f, 0, Some(g), None);
        assert_eq!(d.node(f).parent, None);
    }
}
