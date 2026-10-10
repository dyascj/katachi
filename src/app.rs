//! App state, document actions, undo, clipboard, files, and keyboard shortcuts.

use crate::doc::{AutoLayout, BoolOp, Document, Kind, LayoutMode, Node, NodeId, Order, Sizing};
use crate::render::View;
use egui::{Event, Key, Modifiers, Pos2, Rect, Vec2, vec2};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Move,
    Hand,
    /// K: resizing also scales strokes, radii, effects, and text.
    Scale,
    Frame,
    Section,
    Slice,
    Rectangle,
    Ellipse,
    Line,
    Arrow,
    Polygon,
    Star,
    Pen,
    Pencil,
    Text,
}

impl Tool {
    /// Tools that draw a box by dragging (or drop a default one on click).
    pub fn draws_shape(self) -> bool {
        matches!(
            self,
            Tool::Frame | Tool::Section | Tool::Slice | Tool::Rectangle | Tool::Ellipse | Tool::Line | Tool::Arrow | Tool::Polygon | Tool::Star
        )
    }
    /// Tools that select and transform.
    pub fn selects(self) -> bool {
        matches!(self, Tool::Move | Tool::Scale)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    HCenter,
    Right,
    Top,
    VCenter,
    Bottom,
}

pub enum Drag {
    Pan,
    Move { base: Document, start: Pos2 },
    /// `bounds` is the unrotated selection frame, `rot` its rotation (single layer only).
    Resize { base: Document, handle: (i8, i8), bounds: Rect, rot: f32 },
    Rotate { base: Document, center: Pos2, start_angle: f32, start_rot: f32, deg: f32 },
    Create { base: Document, start: Pos2, id: NodeId },
    Marquee { start: Pos2, base_sel: Vec<NodeId> },
    Vector { base: Document, id: NodeId, grab: crate::vector_edit::Grab },
    PenHandle,
    Pencil { points: Vec<Pos2> },
    /// Prototype tab: dragging a connection from a layer to a frame.
    Connect { from: NodeId },
}

/// What the pointer grabbed on the selection frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grip {
    /// Resize: -1 = min edge, 1 = max edge, 0 = untouched axis.
    Resize(i8, i8),
    /// Just outside a corner.
    Rotate,
}

/// Mouse-down state for the move tool, resolved into a click or a drag.
pub struct Press {
    pub world: Pos2,
    pub target: Option<NodeId>,
    pub handle: Option<Grip>,
    /// Shift-click on an already-selected node: deselect it if no drag follows.
    pub toggle_off: bool,
}

/// What to do once the user has dealt with unsaved changes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum After {
    Close,
    New,
    Open,
}

const CLIP_TAG: &str = "katachi/nodes:";

pub struct App {
    pub doc: Document,
    pub page: usize,
    pub view: View,
    pub view_ready: bool,
    pub tool: Tool,
    pub sel: Vec<NodeId>,
    pub hover: Option<NodeId>,
    pub drag: Option<Drag>,
    pub press: Option<Press>,
    pub pen: Vec<crate::doc::PathPoint>,
    pub editing_text: Option<NodeId>,
    pub focus_text: bool,
    pub renaming: Option<(NodeId, String)>,
    pub renaming_page: Option<(usize, String)>,
    pub guides: Vec<[Pos2; 2]>,
    pub frame_labels: Vec<(NodeId, Rect)>,
    pub layer_drag: Option<NodeId>,
    pub canvas_rect: Option<Rect>,
    pub textures: crate::render::Textures,
    /// Top-level frame pressed on its empty area: selected on click, marquee on drag.
    pub pending_frame: Option<NodeId>,
    /// Zoom to this layer on the next frame (after page switches settle).
    pub zoom_target: Option<NodeId>,
    pub left_tab_assets: bool,
    /// Last tool used in each toolbar group (move, frame, shape, pen, text).
    pub tool_groups: [Tool; 5],
    pub presenting: Option<crate::prototype::Presentation>,
    /// Right panel shows the Prototype tab instead of Design.
    pub prototype_tab: bool,
    /// Pressed the prototype connect handle on this layer.
    pub connecting: Option<NodeId>,
    pub vector_edit: Option<crate::vector_edit::VectorEdit>,
    pub vector_grabbed: Option<crate::vector_edit::Grab>,
    pub vector_start: Pos2,
    /// Cmd+\ hides the side panels.
    pub ui_hidden: bool,
    pub rulers: bool,
    /// Properties copied with Cmd+Alt+C.
    pub style_clip: Option<Node>,
    pub layer_search: Option<String>,
    /// This session's crash-recovery autosave.
    recovery: PathBuf,
    last_autosave: f64,
    /// Autosaves from sessions that crashed, offered on launch.
    pub recoverable: Vec<PathBuf>,
    undo: Vec<Document>,
    redo: Vec<Document>,
    in_gesture: bool,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    pub export_scale: f32,
    pub export_svg: bool,
    pub confirm: Option<After>,
    pub prev_sel: Vec<NodeId>,
    allow_close: bool,
    pub toast: Option<(String, f64)>,
    title: String,
    screenshot: Option<(PathBuf, u32)>,
}

impl App {
    pub fn new(path: Option<PathBuf>) -> Self {
        let mut app = App {
            doc: Document::default(),
            page: 0,
            view: View { pan: Vec2::ZERO, zoom: 1.0 },
            view_ready: false,
            tool: Tool::Move,
            sel: vec![],
            hover: None,
            drag: None,
            press: None,
            pen: vec![],
            editing_text: None,
            focus_text: false,
            renaming: None,
            renaming_page: None,
            guides: vec![],
            frame_labels: vec![],
            layer_drag: None,
            canvas_rect: None,
            textures: Default::default(),
            pending_frame: None,
            zoom_target: None,
            left_tab_assets: false,
            tool_groups: [Tool::Move, Tool::Frame, Tool::Rectangle, Tool::Pen, Tool::Text],
            presenting: None,
            prototype_tab: false,
            connecting: None,
            vector_edit: None,
            vector_grabbed: None,
            vector_start: Pos2::ZERO,
            ui_hidden: false,
            rulers: false,
            style_clip: None,
            layer_search: None,
            recovery: crate::files::recovery_dir().join(format!(
                "{}-{}.katachi",
                std::process::id(),
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis())
            )),
            last_autosave: 0.0,
            recoverable: vec![],
            undo: vec![],
            redo: vec![],
            in_gesture: false,
            path: None,
            dirty: false,
            export_scale: 1.0,
            export_svg: false,
            confirm: None,
            prev_sel: vec![],
            allow_close: false,
            toast: None,
            title: String::new(),
            screenshot: std::env::var_os("KATACHI_SCREENSHOT").map(|p| (PathBuf::from(p), 0)),
        };
        app.recoverable = crate::files::orphaned_recoveries(&app.recovery);
        if let Some(p) = path {
            app.open_path(p);
        }
        app
    }

    fn document_json(&self) -> String {
        serde_json::json!({ "katachi": 1, "document": self.doc }).to_string()
    }

    /// Every 30 s while there are unsaved changes, snapshot to the recovery folder.
    pub fn autosave(&mut self, now: f64) {
        if self.dirty && now - self.last_autosave > 30.0 {
            self.last_autosave = now;
            let _ = crate::files::write_atomic(&self.recovery, self.document_json().as_bytes());
        }
    }

    /// The work is saved or deliberately discarded: drop this session's autosave.
    pub fn clear_recovery(&self) {
        let _ = std::fs::remove_file(&self.recovery);
    }

    /// Replace the current document state, keeping session-level things.
    fn reset_to(&mut self, fresh: App) {
        self.clear_recovery();
        let screenshot = self.screenshot.take();
        let recoverable = std::mem::take(&mut self.recoverable);
        *self = App { screenshot, recoverable, ..fresh };
    }

    // ---------- undo ----------

    /// Snapshot before a discrete change.
    pub fn checkpoint(&mut self) {
        // ponytail: whole-document snapshots, switch to an op log when docs get big or multiplayer lands
        self.undo.push(self.doc.clone());
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.dirty = true;
    }

    /// Snapshot once per continuous gesture (slider scrub, typing in a field).
    pub fn begin_edit(&mut self) {
        if !self.in_gesture {
            self.checkpoint();
            self.in_gesture = true;
        }
    }

    pub fn undo(&mut self) {
        if let Some(d) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.doc, d));
            self.after_history();
        }
    }

    pub fn redo(&mut self) {
        if let Some(d) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.doc, d));
            self.after_history();
        }
    }

    fn after_history(&mut self) {
        self.dirty = true;
        self.vector_edit = None;
        self.editing_text = None;
        self.page = self.page.min(self.doc.pages.len() - 1);
        self.sel.retain(|id| self.doc.nodes.contains_key(id));
    }

    // ---------- selection & structure ----------

    pub fn set_tool(&mut self, t: Tool) {
        if self.tool == Tool::Pen && t != Tool::Pen {
            self.finish_pen(false);
        }
        self.end_text_edit();
        self.tool = t;
        if let Some(g) = crate::panels::TOOL_GROUPS.iter().position(|g| g.iter().any(|(tt, _, _)| *tt == t)) {
            self.tool_groups[g] = t;
        }
    }

    pub fn select_page(&mut self, i: usize) {
        if i != self.page {
            self.end_text_edit();
            self.page = i;
            self.sel.clear();
            self.view_ready = false;
        }
    }

    /// Which node a click on `hit` should select: groups and nested frames are entered
    /// only when the current selection is already inside them (Figma's click-through rules).
    pub fn selectable(&self, hit: NodeId, deep: bool) -> NodeId {
        if deep {
            return hit;
        }
        let mut chain = self.doc.ancestors(hit);
        chain.reverse();
        chain.push(hit);
        for (i, &cur) in chain.iter().enumerate() {
            let n = self.doc.node(cur);
            // Top-level frames are click-through; instances act like groups.
            let top_frame = matches!(n.kind, Kind::Frame | Kind::Component | Kind::ComponentSet | Kind::Section) && n.parent.is_none();
            let entered = self.sel.iter().any(|&s| s != cur && self.doc.is_ancestor(cur, s));
            if i + 1 == chain.len() || !(top_frame || entered) {
                return cur;
            }
        }
        hit
    }

    pub fn delete_selection(&mut self) {
        if self.sel.is_empty() {
            return;
        }
        self.checkpoint();
        for id in std::mem::take(&mut self.sel) {
            self.doc.remove(id);
        }
        self.doc.reflow();
    }

    /// Top-most selected roots only (drop nodes whose ancestor is also selected).
    pub fn sel_roots(&self) -> Vec<NodeId> {
        self.sel
            .iter()
            .copied()
            .filter(|&id| !self.sel.iter().any(|&o| o != id && self.doc.is_ancestor(o, id)))
            .collect()
    }

    pub fn group_selection(&mut self, as_frame: bool) {
        let roots = self.sel_roots();
        if roots.is_empty() {
            return;
        }
        self.checkpoint();
        if let Some(g) = self.doc.group(&roots) {
            if as_frame {
                let r = self.doc.node(g).rect;
                let name = self.doc.auto_name("Frame");
                let n = self.doc.node_mut(g);
                *n = Node { id: n.id, parent: n.parent, children: n.children.clone(), name, ..Node::new(Kind::Frame, r) };
            }
            self.sel = vec![g];
        }
    }

    pub fn flip_selection(&mut self, horizontal: bool) {
        let roots = self.sel_roots();
        if roots.is_empty() {
            return;
        }
        self.checkpoint();
        let c = self.doc.bounds(&roots).center();
        for id in roots {
            self.doc.flip(id, horizontal, if horizontal { c.x } else { c.y });
        }
        self.doc.reflow();
    }

    pub fn distribute_selection(&mut self, horizontal: bool) {
        let roots = self.sel_roots();
        if roots.len() < 3 {
            return;
        }
        self.checkpoint();
        self.doc.distribute(&roots, horizontal);
        self.doc.reflow();
    }

    /// Ctrl+Alt+T: line the selection up along its dominant direction with even gaps.
    pub fn tidy_up(&mut self) {
        let roots = self.sel_roots();
        if roots.len() < 2 {
            return;
        }
        let b = self.doc.bounds(&roots);
        let horizontal = b.width() >= b.height();
        self.checkpoint();
        let mut items: Vec<(NodeId, Rect)> = roots.iter().map(|&i| (i, self.doc.node(i).aabb())).collect();
        items.sort_by(|a, c| if horizontal { a.1.min.x.total_cmp(&c.1.min.x) } else { a.1.min.y.total_cmp(&c.1.min.y) });
        // Average existing gap, at least zero.
        let gaps: Vec<f32> = items.windows(2).map(|w| if horizontal { w[1].1.min.x - w[0].1.max.x } else { w[1].1.min.y - w[0].1.max.y }).collect();
        let gap = (gaps.iter().sum::<f32>() / gaps.len() as f32).max(0.0).round();
        let mut cursor = if horizontal { b.min.x } else { b.min.y };
        for (id, r) in items {
            let d = if horizontal { vec2(cursor - r.min.x, b.min.y - r.min.y) } else { vec2(b.min.x - r.min.x, cursor - r.min.y) };
            self.doc.translate(id, d);
            cursor += if horizontal { r.width() } else { r.height() } + gap;
        }
        self.doc.reflow();
    }

    /// Tab / Shift+Tab: next or previous sibling of the first selected layer.
    pub fn select_sibling(&mut self, dir: i32) {
        let Some(&id) = self.sel.first() else { return };
        let list = self.doc.children_of(self.page, self.doc.node(id).parent).clone();
        let Some(i) = list.iter().position(|c| *c == id) else { return };
        // Layers list is top-first, so "next" walks down the z order.
        let j = (i as i32 - dir).rem_euclid(list.len() as i32) as usize;
        self.sel = vec![list[j]];
    }

    /// Cmd+Alt+V: apply copied appearance to the selection.
    pub fn paste_style(&mut self) {
        let Some(src) = self.style_clip.clone() else { return };
        if self.sel.is_empty() {
            return;
        }
        self.checkpoint();
        for id in self.sel.clone() {
            let n = self.doc.node_mut(id);
            n.fills = src.fills.clone();
            n.strokes = src.strokes.clone();
            n.stroke_width = src.stroke_width;
            n.shadows = src.shadows.clone();
            n.opacity = src.opacity;
            if n.kind == Kind::Rectangle || n.kind.is_frame() {
                n.radius = src.radius;
            }
            if n.kind == Kind::Text && src.kind == Kind::Text {
                n.font_family = src.font_family.clone();
                n.font_weight = src.font_weight;
                n.font_size = src.font_size;
                n.line_height = src.line_height;
                n.letter_spacing = src.letter_spacing;
                n.text_align = src.text_align;
                n.decoration = src.decoration;
                n.text_case = src.text_case;
            }
        }
    }

    /// Combine the selection into a live boolean, or change the op of a selected boolean.
    pub fn boolean_selection(&mut self, op: BoolOp) {
        let roots = self.sel_roots();
        match roots.as_slice() {
            [] => {}
            [one] if self.doc.node(*one).kind == Kind::Boolean => {
                self.checkpoint();
                let n = self.doc.node_mut(*one);
                n.bool_op = op;
                if ["Union", "Subtract", "Intersect", "Exclude"].contains(&n.name.as_str()) {
                    n.name = format!("{op:?}");
                }
                self.doc.reflow();
            }
            [_] => {}
            many => {
                self.checkpoint();
                if let Some(b) = self.doc.boolean(many, op) {
                    self.sel = vec![b];
                }
            }
        }
    }

    /// Shift+A: turn selected frames into auto layout, or wrap other selections in a new one.
    pub fn add_auto_layout(&mut self) {
        let roots = self.sel_roots();
        if roots.is_empty() {
            return;
        }
        let frames_only = roots.iter().all(|&id| {
            let n = self.doc.node(id);
            n.kind.is_frame() && n.layout.mode == LayoutMode::None
        });
        if !frames_only {
            self.group_selection(true);
        } else {
            self.checkpoint();
        }
        for id in self.sel.clone() {
            // Guess direction from how the children are spread out, like Figma.
            let kids = self.doc.node(id).children.clone();
            let b = self.doc.bounds(&kids);
            let spread_x = kids.windows(2).map(|w| (self.doc.node(w[1]).rect.center().x - self.doc.node(w[0]).rect.center().x).abs()).sum::<f32>();
            let spread_y = kids.windows(2).map(|w| (self.doc.node(w[1]).rect.center().y - self.doc.node(w[0]).rect.center().y).abs()).sum::<f32>();
            let horizontal = spread_x >= spread_y;
            // Keep the visual order: sort children along the chosen axis.
            let mut sorted = kids.clone();
            sorted.sort_by(|a, b| {
                let (ra, rb) = (self.doc.node(*a).rect.center(), self.doc.node(*b).rect.center());
                if horizontal { ra.x.total_cmp(&rb.x) } else { ra.y.total_cmp(&rb.y) }
            });
            let fr = self.doc.node(id).rect;
            let pad = if b.is_finite() { (b.min.x - fr.min.x).max(0.0).round() } else { 0.0 };
            let n = self.doc.node_mut(id);
            n.children = sorted;
            n.layout = AutoLayout { mode: if horizontal { LayoutMode::Horizontal } else { LayoutMode::Vertical }, gap: 10.0, padding: [pad; 4], ..Default::default() };
            n.sizing = [Sizing::Hug, Sizing::Hug];
        }
        self.doc.reflow();
    }

    pub fn remove_auto_layout(&mut self) {
        let frames: Vec<NodeId> = self.sel.iter().copied().filter(|&id| self.doc.node(id).layout.mode != LayoutMode::None).collect();
        if frames.is_empty() {
            return;
        }
        self.checkpoint();
        for id in frames {
            let n = self.doc.node_mut(id);
            n.layout.mode = LayoutMode::None;
            n.sizing = [Sizing::Fixed; 2];
        }
    }

    pub fn ungroup_selection(&mut self) {
        let targets: Vec<NodeId> =
            self.sel.iter().copied().filter(|id| self.doc.node(*id).kind.is_container()).collect();
        if targets.is_empty() {
            return;
        }
        self.checkpoint();
        let mut out: Vec<NodeId> = self.sel.iter().copied().filter(|id| !targets.contains(id)).collect();
        for g in targets {
            out.extend(self.doc.ungroup(g));
        }
        self.sel = out;
    }

    pub fn reorder_selection(&mut self, order: Order) {
        if self.sel.is_empty() {
            return;
        }
        self.checkpoint();
        let mut ids = self.doc.sort_by_z(&self.sel);
        if matches!(order, Order::Forward | Order::Front) {
            ids.reverse();
        }
        for id in ids {
            self.doc.reorder(id, order);
        }
    }

    pub fn select_all(&mut self) {
        let parent = self.sel.first().and_then(|&s| self.doc.node(s).parent);
        self.sel = self.doc.children_of(self.page, parent).clone();
    }

    pub fn nudge(&mut self, d: Vec2) {
        if self.sel.is_empty() {
            return;
        }
        self.checkpoint();
        for id in self.sel_roots() {
            self.doc.translate(id, d);
        }
        self.doc.reflow();
    }

    pub fn toggle_visible(&mut self) {
        if let Some(&f) = self.sel.first() {
            self.checkpoint();
            let v = !self.doc.node(f).visible;
            for &id in &self.sel {
                self.doc.node_mut(id).visible = v;
            }
        }
    }

    pub fn toggle_locked(&mut self) {
        if let Some(&f) = self.sel.first() {
            self.checkpoint();
            let v = !self.doc.node(f).locked;
            for &id in &self.sel {
                self.doc.node_mut(id).locked = v;
            }
            if v {
                self.sel.clear();
            }
        }
    }

    pub fn align(&mut self, a: Align) {
        let roots = self.sel_roots();
        let Some(&first) = roots.first() else { return };
        // One layer aligns to its parent frame; several align to their shared bounds.
        let target = if roots.len() == 1 {
            match self.doc.node(first).parent {
                Some(p) => self.doc.node(p).rect,
                None => return,
            }
        } else {
            self.doc.bounds(&roots)
        };
        self.checkpoint();
        for id in roots {
            let r = self.doc.node(id).rect;
            let d = match a {
                Align::Left => vec2(target.min.x - r.min.x, 0.0),
                Align::HCenter => vec2(target.center().x - r.center().x, 0.0),
                Align::Right => vec2(target.max.x - r.max.x, 0.0),
                Align::Top => vec2(0.0, target.min.y - r.min.y),
                Align::VCenter => vec2(0.0, target.center().y - r.center().y),
                Align::Bottom => vec2(0.0, target.max.y - r.max.y),
            };
            self.doc.translate(id, vec2(d.x.round(), d.y.round()));
        }
        self.doc.reflow();
    }

    // ---------- clipboard ----------

    pub fn copy(&mut self, ctx: &egui::Context) {
        let roots = self.sel_roots();
        if roots.is_empty() {
            return;
        }
        let frag = self.doc.extract(&roots);
        if let Ok(json) = serde_json::to_string(&frag) {
            ctx.copy_text(format!("{CLIP_TAG}{json}"));
        }
    }

    pub fn paste_text(&mut self, text: &str) {
        if let Some(frag) = text.strip_prefix(CLIP_TAG).and_then(|j| serde_json::from_str::<Vec<Node>>(j).ok()) {
            self.checkpoint();
            // Paste into a selected frame, otherwise next to the original.
            let parent = match self.sel.as_slice() {
                [one] if self.doc.node(*one).kind.is_frame() => Some(*one),
                [one, ..] => self.doc.node(*one).parent,
                [] => None,
            };
            // Pasting a main component that still exists makes an instance of it.
            let in_frag: std::collections::HashSet<NodeId> = frag.iter().map(|n| n.id).collect();
            let mut out = vec![];
            for root in frag.iter().filter(|n| !n.parent.is_some_and(|p| in_frag.contains(&p))) {
                let live = self.doc.nodes.get(&root.id).is_some_and(|n| n.kind == Kind::Component);
                if root.kind == Kind::Component && live {
                    out.push(self.doc.instantiate(root.id, self.page, parent, None, root.rect.min));
                } else {
                    let sub: Vec<Node> = frag.iter().filter(|n| n.id == root.id || self.frag_descends(&frag, n.id, root.id)).cloned().collect();
                    out.extend(self.doc.paste(&sub, self.page, parent, None));
                }
            }
            self.doc.reflow();
            self.sel = out;
        } else if crate::svg_import::looks_like_svg(text) {
            let at = self.view_center_world();
            self.import_svg(text.as_bytes(), "Vector", at, true);
        } else if !text.trim().is_empty() {
            // Plain text becomes a text layer, like Figma.
            self.checkpoint();
            let mut n = Node::new(Kind::Text, Rect::from_min_size(self.view_center_world(), Vec2::ZERO));
            n.text = text.to_string();
            let id = self.doc.insert(self.page, None, None, n);
            self.sel = vec![id];
        }
    }

    pub fn duplicate_selection(&mut self) {
        let roots = self.sel_roots();
        if roots.is_empty() {
            return;
        }
        self.checkpoint();
        self.sel = self.duplicate_or_instance(&roots);
    }

    /// Duplicate layers; main components produce instances instead, as in Figma.
    pub fn duplicate_or_instance(&mut self, roots: &[NodeId]) -> Vec<NodeId> {
        let mut out = vec![];
        for id in self.doc.sort_by_z(roots) {
            let n = self.doc.node(id);
            if n.kind == Kind::Component {
                let (page, parent, at) = (self.doc.page_of(id), n.parent, n.rect.min);
                let index = self.doc.index_in_parent(id) + 1;
                out.push(self.doc.instantiate(id, page, parent, Some(index), at));
            } else {
                out.extend(self.doc.duplicate(&[id]));
            }
        }
        out
    }

    /// Cmd+Alt+K: frames become components; anything else is wrapped in a new one.
    pub fn create_component(&mut self) {
        let roots = self.sel_roots();
        if roots.is_empty() {
            return;
        }
        self.checkpoint();
        let id = match roots.as_slice() {
            [one] if self.doc.node(*one).kind == Kind::Frame => *one,
            _ => {
                self.group_selection(true);
                let id = self.sel[0];
                self.doc.node_mut(id).fills.clear();
                id
            }
        };
        let name = self.doc.auto_name("Component");
        let n = self.doc.node_mut(id);
        n.kind = Kind::Component;
        if n.name.starts_with("Frame ") {
            n.name = name;
        }
        self.sel = vec![id];
    }

    /// Use the selection as a mask (or stop). Several layers get grouped first, bottom one masking.
    pub fn toggle_mask(&mut self) {
        let roots = self.sel_roots();
        let Some(&first) = roots.first() else { return };
        self.checkpoint();
        if roots.len() == 1 {
            let n = self.doc.node_mut(first);
            n.is_mask = !n.is_mask;
            return;
        }
        if let Some(g) = self.doc.group(&roots) {
            let bottom = self.doc.node(g).children[0];
            self.doc.node_mut(bottom).is_mask = true;
            self.doc.node_mut(g).name = "Mask group".into();
            self.sel = vec![g];
        }
    }

    pub fn combine_as_variants(&mut self) {
        let roots = self.sel_roots();
        if roots.len() < 2 || roots.iter().any(|&r| self.doc.node(r).kind != Kind::Component) {
            return;
        }
        self.checkpoint();
        if let Some(set) = self.doc.combine_as_variants(&roots) {
            self.sel = vec![set];
        }
    }

    /// Copy the last variant in a set as a new one with the next value of the first property.
    pub fn add_variant(&mut self, set: NodeId) {
        let Some(&last) = self.doc.node(set).children.last() else { return };
        self.checkpoint();
        let copy = self.doc.duplicate(&[last])[0];
        let r = self.doc.node(last).rect;
        self.doc.translate(copy, vec2(r.width() + 20.0, 0.0));
        let mut pairs = crate::doc::variant_pairs(&self.doc.node(last).name);
        if let Some((_, v)) = pairs.first_mut() {
            let n = self.doc.node(set).children.len();
            *v = format!("Variant{n}");
        }
        self.doc.node_mut(copy).name = pairs.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(", ");
        let kids = self.doc.node(set).children.clone();
        let b = self.doc.bounds(&kids).expand(20.0);
        self.doc.node_mut(set).rect = b;
        self.sel = vec![copy];
    }

    pub fn detach_selection(&mut self) {
        let insts: Vec<NodeId> = self.sel.iter().copied().filter(|&id| self.doc.node(id).kind == Kind::Instance).collect();
        if insts.is_empty() {
            return;
        }
        self.checkpoint();
        for i in insts {
            self.doc.detach_instance(i);
        }
    }

    /// The instance a layer belongs to (itself or nearest instance ancestor).
    pub fn owning_instance(&self, id: NodeId) -> Option<NodeId> {
        std::iter::once(id).chain(self.doc.ancestors(id)).find(|a| self.doc.node(*a).kind == Kind::Instance)
    }

    pub fn go_to_main(&mut self, inst: NodeId) {
        let Some(c) = self.doc.node(inst).component.filter(|c| self.doc.nodes.contains_key(c)) else {
            self.toast("Main component is missing".into());
            return;
        };
        let page = self.doc.page_of(c);
        if page != self.page {
            self.select_page(page);
        }
        self.sel = vec![c];
        self.zoom_target = Some(c);
    }

    fn frag_descends(&self, frag: &[Node], id: NodeId, root: NodeId) -> bool {
        let mut cur = frag.iter().find(|n| n.id == id).and_then(|n| n.parent);
        while let Some(p) = cur {
            if p == root {
                return true;
            }
            cur = frag.iter().find(|n| n.id == p).and_then(|n| n.parent);
        }
        false
    }

    pub fn view_center_world(&self) -> Pos2 {
        self.view.to_world(self.canvas_rect.map_or(Pos2::ZERO, |r| r.center())).round()
    }

    // ---------- text & pen ----------

    pub fn end_text_edit(&mut self) {
        if let Some(id) = self.editing_text.take()
            && self.doc.nodes.get(&id).is_some_and(|n| n.text.trim().is_empty()) {
                self.doc.remove(id);
                self.sel.retain(|s| *s != id);
                self.doc.reflow();
            }
    }

    pub fn finish_pen(&mut self, closed: bool) {
        let pts = std::mem::take(&mut self.pen);
        if pts.len() >= 2 {
            self.checkpoint();
            let parent = self.frame_at(pts[0].p, &[]);
            let id = self.doc.insert(self.page, parent, None, Node::vector(&pts, closed));
            self.sel = vec![id];
            self.tool = Tool::Move;
        }
    }

    /// Deepest visible frame containing `p`, ignoring `exclude` subtrees.
    pub fn frame_at(&self, p: Pos2, exclude: &[NodeId]) -> Option<NodeId> {
        fn walk(app: &App, list: &[NodeId], p: Pos2, ex: &[NodeId]) -> Option<NodeId> {
            for &id in list.iter().rev() {
                let n = app.doc.node(id);
                if ex.contains(&id) || !n.visible || n.locked {
                    continue;
                }
                if n.kind.is_frame() && n.rect.contains(n.to_local(p)) {
                    return walk(app, &n.children, p, ex).or(Some(id));
                }
                if n.kind == Kind::Group
                    && let Some(f) = walk(app, &n.children, p, ex) {
                        return Some(f);
                    }
            }
            None
        }
        walk(self, &self.doc.pages[self.page].children, p, exclude)
    }

    // ---------- files ----------

    pub fn open_path(&mut self, p: PathBuf) {
        let parsed = std::fs::read_to_string(&p)
            .map_err(|e| e.to_string())
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).map_err(|e| e.to_string()))
            .and_then(|v| serde_json::from_value::<Document>(v["document"].clone()).map_err(|e| e.to_string()));
        match parsed {
            Ok(doc) => {
                self.reset_to(App::new(None));
                self.doc = doc;
                crate::files::add_recent(&p);
                self.path = Some(p);
            }
            Err(e) => self.toast(format!("Couldn't open file: {e}")),
        }
    }

    pub fn open_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new().add_filter("Katachi", &["katachi"]).pick_file() {
            self.open_path(p);
        }
    }

    pub fn save(&mut self, save_as: bool) -> bool {
        let path = match (&self.path, save_as) {
            (Some(p), false) => p.clone(),
            _ => match rfd::FileDialog::new().add_filter("Katachi", &["katachi"]).set_file_name("Untitled.katachi").save_file() {
                Some(p) => p,
                None => return false,
            },
        };
        match crate::files::write_atomic(&path, self.document_json().as_bytes()) {
            Ok(()) => {
                crate::files::add_recent(&path);
                self.path = Some(path);
                self.dirty = false;
                self.clear_recovery();
                true
            }
            Err(e) => {
                self.toast(format!("Couldn't save: {e}"));
                false
            }
        }
    }

    pub fn export(&mut self) {
        let ids = if self.sel.is_empty() { self.doc.pages[self.page].children.clone() } else { self.sel_roots() };
        if ids.is_empty() {
            return;
        }
        let name = if ids.len() == 1 { self.doc.node(ids[0]).name.clone() } else { "Export".into() };
        let ext = if self.export_svg { "svg" } else { "png" };
        let suffix = if self.export_scale != 1.0 && !self.export_svg { format!("@{}x", self.export_scale) } else { String::new() };
        let Some(path) = rfd::FileDialog::new().add_filter(ext.to_uppercase(), &[ext]).set_file_name(format!("{name}{suffix}.{ext}")).save_file()
        else {
            return;
        };
        // A slice exports whatever lies inside it.
        let svg = match ids.as_slice() {
            [one] if self.doc.node(*one).kind == Kind::Slice => {
                let n = self.doc.node(*one);
                crate::export::region_svg(&self.doc, self.page, n.aabb(), n.id)
            }
            _ => crate::export::to_svg(&self.doc, &ids),
        };
        let data = if self.export_svg {
            Ok(svg.into_bytes())
        } else {
            crate::export::svg_to_png(&self.doc, &svg, self.export_scale)
        };
        match data.and_then(|d| std::fs::write(&path, d).map_err(|e| e.to_string())) {
            Ok(()) => self.toast(format!("Exported {}", path.file_name().unwrap_or_default().to_string_lossy())),
            Err(e) => self.toast(format!("Export failed: {e}")),
        }
    }

    pub fn request_new(&mut self) {
        if self.dirty { self.confirm = Some(After::New) } else { self.new_file() }
    }

    pub fn request_open(&mut self) {
        if self.dirty { self.confirm = Some(After::Open) } else { self.open_dialog() }
    }

    pub fn new_file(&mut self) {
        self.reset_to(App::new(None));
    }

    /// Open an autosave from a crashed session as unsaved work.
    pub fn recover(&mut self, p: &std::path::Path) {
        let parsed = std::fs::read_to_string(p)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| serde_json::from_value::<Document>(v["document"].clone()).ok());
        if let Some(doc) = parsed {
            self.reset_to(App::new(None));
            self.doc = doc;
            self.dirty = true;
            // Re-home the autosave before deleting the old one, so a second crash loses nothing.
            let _ = crate::files::write_atomic(&self.recovery, self.document_json().as_bytes());
            let _ = std::fs::remove_file(p);
            self.recoverable.retain(|q| q != p);
        }
    }

    fn recovery_prompt(&mut self, ctx: &egui::Context) {
        if self.recoverable.is_empty() {
            return;
        }
        let mut action: Option<(PathBuf, bool)> = None;
        egui::Modal::new("recover".into()).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.heading("Recover unsaved work?");
            ui.label("Katachi didn't close cleanly last time. These autosaves are available:");
            ui.add_space(6.0);
            for p in &self.recoverable {
                let when = std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).map_or(String::new(), |e| {
                    let m = e.as_secs() / 60;
                    if m < 60 { format!("{m} min ago") } else { format!("{} h ago", m / 60) }
                });
                ui.horizontal(|ui| {
                    ui.label(when);
                    if ui.button("Recover").clicked() {
                        action = Some((p.clone(), true));
                    }
                    if ui.button("Discard").clicked() {
                        action = Some((p.clone(), false));
                    }
                });
            }
        });
        match action {
            Some((p, true)) => self.recover(&p),
            Some((p, false)) => {
                let _ = std::fs::remove_file(&p);
                self.recoverable.retain(|q| *q != p);
            }
            None => {}
        }
    }

    /// Place SVG data as a frame centered (or top-left if `!centered`) on `at`.
    pub fn import_svg(&mut self, data: &[u8], name: &str, at: Pos2, centered: bool) -> Option<NodeId> {
        let before = self.doc.clone();
        self.checkpoint();
        let parent = self.frame_at(at, &[]);
        match crate::svg_import::import(&mut self.doc, data, name, self.page, parent, at) {
            Ok((f, images)) => {
                for (id, bytes) in images {
                    let key = self.add_image(bytes);
                    let mut p = crate::doc::Paint::solid(crate::doc::Rgba::WHITE);
                    p.convert(crate::doc::PaintKind::Image);
                    p.image = key;
                    self.doc.node_mut(id).fills = vec![p];
                }
                if centered {
                    let size = self.doc.node(f).rect.size();
                    self.doc.translate(f, (-size / 2.0).round());
                }
                self.sel = vec![f];
                self.tool = Tool::Move;
                Some(f)
            }
            Err(e) => {
                self.doc = before;
                self.undo.pop();
                self.toast(format!("Couldn't import SVG: {e}"));
                None
            }
        }
    }

    /// File > Place image / Import: SVG, PNG, or JPEG at the center of the view.
    pub fn import_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("Images", &["svg", "png", "jpg", "jpeg"]).pick_file() else { return };
        let Ok(bytes) = std::fs::read(&path) else { return };
        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let at = self.view_center_world();
        if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("svg")) {
            self.import_svg(&bytes, &name, at, true);
        } else {
            self.place_image(bytes, &name, at);
        }
    }

    /// A raster image as an image-filled rectangle at natural size, centered on `at`.
    pub fn place_image(&mut self, bytes: Vec<u8>, name: &str, at: Pos2) -> Option<NodeId> {
        let Ok(img) = image::load_from_memory(&bytes) else {
            self.toast(format!("Can't import {name}"));
            return None;
        };
        self.checkpoint();
        let size = vec2(img.width() as f32, img.height() as f32);
        let key = self.add_image(bytes);
        let mut n = Node::new(Kind::Rectangle, Rect::from_center_size(at.round(), size));
        let mut p = crate::doc::Paint::solid(crate::doc::Rgba::WHITE);
        p.convert(crate::doc::PaintKind::Image);
        p.image = key;
        n.fills = vec![p];
        n.name = name.to_string();
        let parent = self.frame_at(at, &[]);
        let id = self.doc.insert(self.page, parent, None, n);
        self.sel = vec![id];
        self.tool = Tool::Move;
        Some(id)
    }

    /// Cmd+E: merge the selection into one vector with exact outlines.
    pub fn flatten_selection(&mut self) {
        let roots = self.sel_roots();
        if roots.is_empty() || roots.iter().all(|&id| self.doc.node(id).kind == Kind::Vector && roots.len() == 1) {
            return;
        }
        let Some(mut v) = crate::svg_import::outline(&self.doc, &roots) else { return };
        self.checkpoint();
        let top = *self.doc.sort_by_z(&roots).last().unwrap();
        let (parent, index) = (self.doc.node(top).parent, self.doc.index_in_parent(top));
        v.name = if roots.len() == 1 { self.doc.node(roots[0]).name.clone() } else { "Vector".into() };
        let id = self.doc.insert(self.page, parent, Some(index + 1), v);
        for r in roots {
            self.doc.remove(r);
        }
        self.doc.reflow();
        self.sel = vec![id];
    }

    /// Store image bytes in the document, keyed by content hash.
    pub fn add_image(&mut self, bytes: Vec<u8>) -> String {
        use std::hash::{Hash, Hasher};
        let mut h = std::hash::DefaultHasher::new();
        bytes.hash(&mut h);
        let key = format!("{:016x}", h.finish());
        self.doc.images.entry(key.clone()).or_insert_with(|| std::sync::Arc::new(bytes));
        key
    }

    pub fn toast(&mut self, msg: String) {
        self.toast = Some((msg, 0.0));
    }

    // ---------- shortcuts ----------

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let typing = ctx.egui_wants_keyboard_input();
        let events = ctx.input(|i| i.events.clone());
        for e in events {
            match e {
                Event::Copy if !typing => self.copy(ctx),
                Event::Cut if !typing => {
                    self.copy(ctx);
                    self.delete_selection();
                }
                Event::Paste(t) if !typing => self.paste_text(&t),
                Event::Key { key, pressed: true, modifiers: m, .. } => self.key(ctx, key, m, typing),
                _ => {}
            }
        }
    }

    fn key(&mut self, ctx: &egui::Context, key: Key, m: Modifiers, typing: bool) {
        let cmd = m.command;
        // Global shortcuts that work even while a field has focus.
        match (cmd, m.shift, key) {
            (true, false, Key::S) => return void(self.save(false)),
            (true, true, Key::S) => return void(self.save(true)),
            (true, false, Key::O) => return self.request_open(),
            (true, false, Key::N) => return self.request_new(),
            (true, true, Key::E) => return self.export(),
            _ => {}
        }
        if key == Key::Escape && self.editing_text.is_some() {
            return self.end_text_edit();
        }
        if self.vector_edit.is_some() && !typing && !cmd {
            match key {
                Key::Escape | Key::Enter => return self.exit_vector_edit(),
                Key::Delete | Key::Backspace => return self.vector_delete_points(),
                _ => {}
            }
        }
        if typing {
            return;
        }
        let step = if m.shift { 10.0 } else { 1.0 };
        // Ctrl+Cmd+M: use as mask.
        if m.ctrl && m.mac_cmd && key == Key::M {
            return self.toggle_mask();
        }
        // Ctrl+Alt arrangement shortcuts (Figma on macOS).
        if m.ctrl && m.alt && !m.mac_cmd {
            match key {
                Key::H => return self.distribute_selection(true),
                Key::V => return self.distribute_selection(false),
                Key::T => return self.tidy_up(),
                _ => {}
            }
        }
        match (cmd, m.shift, m.alt, key) {
            (true, false, _, Key::Z) => self.undo(),
            (true, true, _, Key::Z) | (true, false, _, Key::Y) => self.redo(),
            (true, false, false, Key::D) => self.duplicate_selection(),
            (true, false, true, Key::C) => self.style_clip = self.sel.first().map(|&id| self.doc.node(id).clone()),
            (true, false, true, Key::V) => self.paste_style(),
            (true, false, false, Key::R) => {
                if let Some(&id) = self.sel.first() {
                    self.left_tab_assets = false;
                    self.renaming = Some((id, self.doc.node(id).name.clone()));
                }
            }
            (true, false, false, Key::Backslash) => self.ui_hidden = !self.ui_hidden,
            (false, true, false, Key::R) => self.rulers = !self.rulers,
            (false, false, true, Key::L) => {
                for n in self.doc.nodes.values_mut() {
                    n.expanded = false;
                }
            }
            (false, true, false, Key::H) => self.flip_selection(true),
            (false, true, false, Key::V) => self.flip_selection(false),
            (false, shift, false, Key::Tab) => {
                // Tab cycles layers in Figma; stop egui from moving widget focus instead.
                ctx.memory_mut(|m| m.move_focus(egui::FocusDirection::None));
                self.select_sibling(if shift { -1 } else { 1 });
            }
            (false, true, false, Key::Enter) => {
                let parents: Vec<NodeId> = self.sel.iter().filter_map(|&s| self.doc.node(s).parent).collect();
                if !parents.is_empty() {
                    self.sel = parents;
                    self.sel.dedup();
                }
            }
            (true, false, false, Key::A) => self.select_all(),
            (true, false, true, Key::G) => self.group_selection(true),
            (true, false, true, Key::K) => self.create_component(),
            (true, false, true, Key::Enter) => self.present(ctx.input(|i| i.time)),
            (true, true, false, Key::K) => self.import_dialog(),
            (true, false, false, Key::E) => self.flatten_selection(),
            (true, false, true, Key::B) => self.detach_selection(),
            (true, false, false, Key::G) => self.group_selection(false),
            (true, true, false, Key::G) => self.ungroup_selection(),
            (true, true, false, Key::H) => self.toggle_visible(),
            (true, true, false, Key::L) => self.toggle_locked(),
            (false, true, true, Key::U) => self.boolean_selection(BoolOp::Union),
            (false, true, true, Key::S) => self.boolean_selection(BoolOp::Subtract),
            (false, true, true, Key::I) => self.boolean_selection(BoolOp::Intersect),
            (false, true, true, Key::E) => self.boolean_selection(BoolOp::Exclude),
            (false, true, false, Key::A) => self.add_auto_layout(),
            (false, true, true, Key::A) => self.remove_auto_layout(),
            (true, false, false, Key::CloseBracket) => self.reorder_selection(Order::Forward),
            (true, false, false, Key::OpenBracket) => self.reorder_selection(Order::Backward),
            (true, false, true, Key::CloseBracket) => self.reorder_selection(Order::Front),
            (true, false, true, Key::OpenBracket) => self.reorder_selection(Order::Back),
            (true, _, _, Key::Equals | Key::Plus) => self.zoom_step(ctx, 2.0),
            (true, _, _, Key::Minus) => self.zoom_step(ctx, 0.5),
            (false, true, _, Key::Num0) => self.zoom_to(ctx, None, true),
            (false, true, _, Key::Num1) => self.zoom_to(ctx, None, false),
            (false, true, _, Key::Num2) => {
                let s = self.sel_roots();
                if !s.is_empty() {
                    self.zoom_to(ctx, Some(s), false)
                }
            }
            (false, _, _, Key::Delete | Key::Backspace) => self.delete_selection(),
            (false, _, _, Key::ArrowLeft) => self.nudge(vec2(-step, 0.0)),
            (false, _, _, Key::ArrowRight) => self.nudge(vec2(step, 0.0)),
            (false, _, _, Key::ArrowUp) => self.nudge(vec2(0.0, -step)),
            (false, _, _, Key::ArrowDown) => self.nudge(vec2(0.0, step)),
            (false, false, false, Key::Escape) => {
                if !self.pen.is_empty() {
                    self.finish_pen(false);
                } else if self.tool != Tool::Move {
                    self.set_tool(Tool::Move);
                } else {
                    // Step out to the parent, then clear.
                    let parent = self.sel.first().and_then(|&s| self.doc.node(s).parent);
                    self.sel = parent.into_iter().collect();
                }
            }
            (false, false, false, Key::Enter) => {
                if !self.pen.is_empty() {
                    self.finish_pen(false);
                } else if let [one] = self.sel[..] {
                    let n = self.doc.node(one);
                    if n.kind == Kind::Vector {
                        self.enter_vector_edit(one);
                    } else if n.kind == Kind::Text {
                        self.checkpoint();
                        self.editing_text = Some(one);
                        self.focus_text = true;
                    } else if !n.children.is_empty() {
                        self.sel = n.children.clone();
                    }
                }
            }
            (false, false, false, Key::V) => self.set_tool(Tool::Move),
            (false, false, false, Key::K) => self.set_tool(Tool::Scale),
            (false, true, false, Key::S) => self.set_tool(Tool::Section),
            (false, false, false, Key::S) => self.set_tool(Tool::Slice),
            (false, true, false, Key::L) => self.set_tool(Tool::Arrow),
            (false, true, false, Key::P) => self.set_tool(Tool::Pencil),
            (false, false, false, Key::H) => self.set_tool(Tool::Hand),
            (false, false, false, Key::F | Key::A) => self.set_tool(Tool::Frame),
            (false, false, false, Key::R) => self.set_tool(Tool::Rectangle),
            (false, false, false, Key::O) => self.set_tool(Tool::Ellipse),
            (false, false, false, Key::L) => self.set_tool(Tool::Line),
            (false, false, false, Key::P) => self.set_tool(Tool::Pen),
            (false, false, false, Key::T) => self.set_tool(Tool::Text),
            _ => {}
        }
    }

    pub fn zoom_step(&mut self, ctx: &egui::Context, f: f32) {
        let c = ctx.content_rect().center();
        self.view.zoom_at(c, f);
    }

    /// Zoom to fit `ids` (None = whole page); `actual` = 100% centered on content.
    pub fn zoom_to(&mut self, ctx: &egui::Context, ids: Option<Vec<NodeId>>, actual: bool) {
        let screen = self.canvas_rect.unwrap_or(ctx.content_rect());
        if actual {
            let c = self.view.to_world(screen.center());
            self.view = View { zoom: 1.0, pan: screen.center().to_vec2() - c.to_vec2() };
            return;
        }
        let ids = ids.unwrap_or_else(|| self.doc.pages[self.page].children.clone());
        let r = self.doc.bounds(&ids);
        if r.is_finite() {
            self.view = View::fit(r, screen, 256.0);
        }
    }
}

fn void(_: bool) {}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.presenting.is_some() {
            egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| self.presentation_ui(ui));
            return;
        }
        self.shortcuts(&ctx);
        if !self.ui_hidden {
            self.left_panel(ui);
            self.right_panel(ui);
        }
        self.canvas_panel(ui);
        self.close_guard(&ctx);
        self.recovery_prompt(&ctx);
        self.autosave(ctx.input(|i| i.time));
        self.debug_screenshot(&ctx);

        // A gesture (and its single undo step) ends when the pointer is up and no field is focused.
        if !ctx.input(|i| i.pointer.any_down()) && !ctx.egui_wants_keyboard_input() {
            self.in_gesture = false;
        }
        let name = self.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned());
        let title = format!("{}{} – Katachi", name.as_deref().unwrap_or("Untitled"), if self.dirty { " (Edited)" } else { "" });
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }
}

impl App {
    /// Never lose work: closing, New, or Open with unsaved changes asks first.
    fn close_guard(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && self.dirty && !self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Some(After::Close);
        }
        let Some(after) = self.confirm else { return };
        let mut choice = None;
        egui::Modal::new("confirm-unsaved".into()).show(ctx, |ui| {
            ui.set_width(320.0);
            ui.heading("Save changes?");
            ui.label("Your changes will be lost if you don't save them.");
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    choice = Some(true);
                }
                if ui.button("Don't save").clicked() {
                    choice = Some(false);
                }
                if ui.button("Cancel").clicked() {
                    self.confirm = None;
                }
            });
        });
        let Some(save) = choice else { return };
        if save && !self.save(false) {
            return;
        }
        self.confirm = None;
        match after {
            After::Close => {
                self.clear_recovery();
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            After::New => self.new_file(),
            After::Open => {
                self.dirty = false;
                self.clear_recovery();
                self.open_dialog();
            }
        }
    }

    /// `KATACHI_SCREENSHOT=out.png katachi file` renders a few frames, saves a PNG, and quits.
    fn debug_screenshot(&mut self, ctx: &egui::Context) {
        let Some((path, frames)) = &mut self.screenshot else { return };
        *frames += 1;
        if *frames == 10 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        ctx.request_repaint();
        let shot = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = shot {
            let [w, h] = img.size;
            let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
            if let Some(pix) = resvg::tiny_skia::Pixmap::from_vec(bytes, resvg::tiny_skia::IntSize::from_wh(w as u32, h as u32).unwrap()) {
                let _ = pix.save_png(path.clone());
            }
            self.allow_close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}
