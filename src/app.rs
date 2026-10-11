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
    /// C: drop comment pins.
    Comment,
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

impl Drag {
    /// The document as it was when the drag began (drags re-apply themselves to it each frame).
    pub fn base_mut(&mut self) -> Option<&mut Document> {
        match self {
            Drag::Move { base, .. } | Drag::Resize { base, .. } | Drag::Rotate { base, .. } | Drag::Create { base, .. } | Drag::Vector { base, .. } => Some(base),
            _ => None,
        }
    }
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
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum After {
    Close,
    CloseTab,
}

/// Something only the window (the tab strip) can do, asked for by a tab.
#[derive(Clone, Debug, PartialEq)]
pub enum Want {
    NewTab,
    Open(PathBuf),
    OpenDialog,
    CloseTab,
    Home,
}

/// A plugin with a UI, running until it closes.
pub struct PluginRun {
    pub session: crate::plugins::Session,
    pub ui: crate::plugins::UiRequest,
    view: Option<crate::plugin_view::PluginView>,
    /// Messages from the UI, filled by the webview.
    inbox: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    /// Messages for the UI, held until it has loaded.
    outbox: Vec<String>,
    ready: bool,
    pos: Option<Pos2>,
    last_sel: Vec<NodeId>,
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
    /// The press that closed the pen path; its release isn't a click.
    pub pen_closed: bool,
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
    /// Assets panel previews: component -> (content key, texture).
    pub asset_thumbs: std::collections::HashMap<NodeId, (u64, egui::TextureHandle)>,
    /// Last tool used in each toolbar group (move, frame, shape, pen, text).
    pub tool_groups: [Tool; 6],
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
    pub outline_mode: bool,
    pub show_grids: bool,
    pub show_history: bool,
    /// A bare title strip over the app (the promo film); the window's tab strip replaces it normally.
    pub own_title_bar: bool,
    /// Theme and glass settings, and the glass state last given to the window.
    pub appearance: crate::appearance::Appearance,
    pub show_appearance: bool,
    glass_applied: Option<(bool, u8)>,
    /// The plugin that's running with a UI, if any.
    pub plugin: Option<PluginRun>,
    /// MCP tool calls from agents, once the server is listening.
    mcp: Option<std::sync::mpsc::Receiver<crate::mcp::Call>>,
    mcp_tried: bool,
    /// `KATACHI_PLUGIN=path/manifest.json`: run a plugin at startup (development).
    debug_plugin: Option<PathBuf>,
    /// The last layers copied here, for Paste to replace.
    pub clip_frag: Option<Vec<Node>>,
    /// Find and replace (Cmd F): (open, query, replacement, focus the field next frame).
    pub find: (bool, String, String, bool),
    /// Plugins > Run script console.
    pub show_console: bool,
    /// Quick actions (Cmd / or Cmd K): query and highlighted row.
    pub quick: Option<(String, usize)>,
    pub console_code: String,
    pub show_variables: bool,
    /// Dev Mode: inspect and code instead of design controls.
    pub dev_mode: bool,
    pub code_lang: crate::codegen::Lang,
    pub show_comments: bool,
    pub show_resolved: bool,
    pub draft_comment: Option<(crate::doc::Comment, String)>,
    pub open_comment: Option<u64>,
    pub comment_reply: String,
    pub comment_hits: Vec<(u64, Rect)>,
    pub var_collection: Option<u64>,
    /// Properties copied with Cmd+Alt+C.
    pub style_clip: Option<Node>,
    pub layer_search: Option<String>,
    /// This session's crash-recovery autosave.
    recovery: PathBuf,
    last_autosave: f64,
    /// Bumped when the autosave is cleared, so a background autosave that finishes late is dropped.
    autosave_gen: std::sync::Arc<std::sync::Mutex<u64>>,
    /// Autosaves from sessions that crashed, offered on launch.
    pub recoverable: Vec<PathBuf>,
    undo: Vec<Document>,
    redo: Vec<Document>,
    /// In a multiplayer session: nodes this user changed after each undo checkpoint.
    undo_touched: Vec<std::collections::HashSet<NodeId>>,
    redo_touched: Vec<std::collections::HashSet<NodeId>>,
    pub collab: Option<crate::collab::Collab>,
    pub show_collab: bool,
    pub collab_name: String,
    pub collab_address: String,
    pub collab_port: u16,
    /// Pointer in world space this frame (shared as presence).
    pub pointer_world: Option<Pos2>,
    /// Text layers' last measured inputs, so unchanged text isn't re-laid out every frame.
    pub text_keys: std::collections::HashMap<NodeId, u64>,
    in_gesture: bool,
    pub path: Option<PathBuf>,
    /// Name for an unsaved document that came from somewhere (an imported .fig).
    pub draft_name: Option<String>,
    pub want: Option<Want>,
    pub dirty: bool,
    pub export_scale: f32,
    pub export_svg: bool,
    pub confirm: Option<After>,
    pub prev_sel: Vec<NodeId>,
    pub allow_close: bool,
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
            pen_closed: false,
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
            asset_thumbs: Default::default(),
            tool_groups: [Tool::Move, Tool::Frame, Tool::Rectangle, Tool::Pen, Tool::Text, Tool::Comment],
            presenting: None,
            prototype_tab: false,
            connecting: None,
            vector_edit: None,
            vector_grabbed: None,
            vector_start: Pos2::ZERO,
            ui_hidden: false,
            rulers: false,
            outline_mode: false,
            show_grids: true,
            show_history: false,
            find: (false, String::new(), String::new(), false),
            clip_frag: None,
            plugin: None,
            own_title_bar: false,
            appearance: crate::appearance::load(),
            show_appearance: false,
            glass_applied: None,
            mcp: None,
            mcp_tried: cfg!(test),
            debug_plugin: std::env::var_os("KATACHI_PLUGIN").map(PathBuf::from),
            show_console: false,
            quick: None,
            console_code: "for (const n of figma.currentPage.selection) {\n  n.rotation += 15;\n}\nfigma.notify(`Rotated ${figma.currentPage.selection.length} layers`);\n".into(),
            show_variables: false,
            dev_mode: false,
            code_lang: crate::codegen::Lang::Css,
            show_comments: false,
            show_resolved: false,
            draft_comment: None,
            open_comment: None,
            comment_reply: String::new(),
            comment_hits: vec![],
            var_collection: None,
            style_clip: None,
            layer_search: None,
            recovery: {
                // Unique per window, even for two opened in the same millisecond.
                static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
                crate::files::recovery_dir().join(format!(
                    "{}-{}-{}.katachi",
                    std::process::id(),
                    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis()),
                    SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                ))
            },
            last_autosave: 0.0,
            autosave_gen: Default::default(),
            recoverable: vec![],
            undo: vec![],
            redo: vec![],
            undo_touched: vec![],
            redo_touched: vec![],
            collab: None,
            show_collab: false,
            collab_name: crate::comments::author(),
            collab_address: String::new(),
            collab_port: 7878,
            pointer_world: None,
            text_keys: Default::default(),
            in_gesture: false,
            path: None,
            draft_name: None,
            want: None,
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
        // Debug aid for screenshots: start on a given page.
        if let Some(pg) = std::env::var("KATACHI_PAGE").ok().and_then(|p| p.parse::<usize>().ok()) {
            app.page = pg.min(app.doc.pages.len() - 1);
        }
        app
    }

    /// The file's name, as tabs and the title show it.
    pub fn display_name(&self) -> String {
        match (&self.path, &self.draft_name) {
            (Some(p), _) => p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
            (None, Some(n)) => n.clone(),
            (None, None) => "Untitled".into(),
        }
    }

    /// Take over the session (MCP server, appearance, crash recovery) from the tab that had it.
    pub fn take_session(&mut self, from: &mut App) {
        self.mcp = from.mcp.take();
        self.mcp_tried = from.mcp_tried;
        self.appearance = from.appearance.clone();
        self.glass_applied = from.glass_applied;
        self.recoverable = std::mem::take(&mut from.recoverable);
        self.screenshot = from.screenshot.take();
    }

    /// The session's own work, for frames where this app isn't on screen (Home).
    pub fn session_frame(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        self.recovery_prompt(ctx);
        self.appearance_frame(ctx, frame);
        self.mcp_frame(ctx);
        self.debug_screenshot(ctx);
    }

    fn document_json(&self) -> String {
        crate::doc::file_json(&self.doc)
    }

    /// Every 30 s while there are unsaved changes, snapshot to the recovery folder. The copy is
    /// written on another thread so big files don't stall the canvas.
    pub fn autosave(&mut self, now: f64) {
        if self.dirty && now - self.last_autosave > 30.0 {
            self.last_autosave = now;
            let (doc, path, gen_lock) = (self.doc.clone(), self.recovery.clone(), self.autosave_gen.clone());
            let generation = *gen_lock.lock().unwrap();
            std::thread::spawn(move || {
                let json = crate::doc::file_json(&doc);
                // Skip the write if the work was saved or discarded meanwhile.
                let g = gen_lock.lock().unwrap();
                if *g == generation {
                    let _ = crate::files::write_atomic(&path, json.as_bytes());
                }
            });
        }
    }

    /// The work is saved or deliberately discarded: drop this session's autosave.
    pub fn clear_recovery(&self) {
        let mut g = self.autosave_gen.lock().unwrap();
        *g += 1;
        let _ = std::fs::remove_file(&self.recovery);
    }

    /// Replace the current document state, keeping session-level things.
    fn reset_to(&mut self, fresh: App) {
        self.clear_recovery();
        let screenshot = self.screenshot.take();
        let recoverable = std::mem::take(&mut self.recoverable);
        // The MCP server and the window's appearance belong to the session, not the document.
        let (mcp, mcp_tried, appearance, glass_applied) = (self.mcp.take(), self.mcp_tried, self.appearance.clone(), self.glass_applied);
        *self = App { screenshot, recoverable, mcp, mcp_tried, appearance, glass_applied, ..fresh };
    }

    // ---------- undo ----------

    /// Snapshot before a discrete change.
    pub fn checkpoint(&mut self) {
        // Attribute pending multiplayer changes to the step they belong to before starting a new one.
        self.sync_collab(f64::MAX);
        // ponytail: whole-document snapshots, switch to an op log when docs get big
        self.undo.push(self.doc.clone());
        self.undo_touched.push(Default::default());
        if self.undo.len() > 200 {
            self.undo.remove(0);
            self.undo_touched.remove(0);
        }
        self.redo.clear();
        self.redo_touched.clear();
        self.dirty = true;
    }

    /// Exchange changes with the session; remember which nodes this user touched for undo.
    pub fn sync_collab(&mut self, now: f64) {
        let Some(c) = self.collab.as_mut() else { return };
        c.me.page = self.page;
        c.me.selection = self.sel.clone();
        c.me.cursor = self.pointer_world;
        let (touched, remote) = c.tick(&mut self.doc, now);
        // A drag rebuilds the document from its starting copy every frame; that copy needs
        // others' changes too, or they'd vanish (and go out as deletions) mid-drag.
        if !remote.is_empty()
            && let Some(base) = self.drag.as_mut().and_then(|d| d.base_mut())
        {
            crate::collab::apply(base, &remote);
        }
        if !c.connected {
            self.collab = None;
            self.toast("Disconnected from the session".into());
            return;
        }
        if let Some(t) = self.undo_touched.last_mut() {
            t.extend(touched);
        }
        self.sel.retain(|id| self.doc.nodes.contains_key(id));
    }

    /// Bring back only the given nodes from a snapshot (multiplayer undo leaves others' work alone).
    fn restore_nodes(&mut self, from: &Document, ids: &std::collections::HashSet<NodeId>) {
        for id in ids {
            match from.nodes.get(id) {
                Some(n) => {
                    self.doc.nodes.insert(*id, n.clone());
                }
                None => {
                    self.doc.nodes.remove(id);
                }
            }
        }
        for id in ids {
            for pg in &mut self.doc.pages {
                pg.children.retain(|c| c != id);
            }
        }
        for id in ids {
            for (p, pg) in from.pages.iter().enumerate() {
                if let (Some(i), Some(dst)) = (pg.children.iter().position(|c| c == id), self.doc.pages.get_mut(p)) {
                    dst.children.insert(i.min(dst.children.len()), *id);
                }
            }
        }
        crate::collab::repair(&mut self.doc);
        self.doc.reflow();
    }

    /// Snapshot once per continuous gesture (slider scrub, typing in a field).
    pub fn begin_edit(&mut self) {
        if !self.in_gesture {
            self.checkpoint();
            self.in_gesture = true;
        }
    }

    pub fn undo(&mut self) {
        self.sync_collab(f64::MAX);
        if let Some(d) = self.undo.pop() {
            let touched = self.undo_touched.pop().unwrap_or_default();
            if self.collab.is_some() {
                let current = self.doc.clone();
                self.restore_nodes(&d, &touched);
                self.redo.push(current);
            } else {
                self.redo.push(std::mem::replace(&mut self.doc, d));
            }
            self.redo_touched.push(touched);
            self.after_history();
        }
    }

    pub fn redo(&mut self) {
        self.sync_collab(f64::MAX);
        if let Some(d) = self.redo.pop() {
            let touched = self.redo_touched.pop().unwrap_or_default();
            if self.collab.is_some() {
                let current = self.doc.clone();
                self.restore_nodes(&d, &touched);
                self.undo.push(current);
            } else {
                self.undo.push(std::mem::replace(&mut self.doc, d));
            }
            self.undo_touched.push(touched);
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

    /// Remember the current page's viewport (pages reopen where they were left).
    pub fn remember_view(&mut self) {
        if let Some(r) = self.canvas_rect
            && self.view_ready
        {
            let c = self.view.to_world(r.center());
            self.doc.pages[self.page].view = Some([c.x, c.y, self.view.zoom]);
        }
    }

    pub fn select_page(&mut self, i: usize) {
        if i != self.page {
            self.remember_view();
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
                // Like Figma, a frame wrapped around a selection has no fill and doesn't clip.
                *n = Node { id: n.id, parent: n.parent, children: n.children.clone(), name, fills: vec![], clip: false, ..Node::new(Kind::Frame, r) };
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
        // A group gets auto layout by becoming a frame, as in Figma (its children lay out directly).
        let converted = matches!(roots[..], [g] if self.doc.node(g).kind == Kind::Group);
        if let [g] = roots[..]
            && converted
        {
            self.checkpoint();
            let n = self.doc.node_mut(g);
            n.kind = Kind::Frame;
            n.fills.clear();
            n.clip = false;
            n.name = n.name.replacen("Group", "Frame", 1);
            self.sel = vec![g];
        }
        let roots = self.sel_roots();
        let frames_only = roots.iter().all(|&id| {
            let n = self.doc.node(id);
            n.kind.is_frame() && n.layout.mode == LayoutMode::None
        });
        if !frames_only {
            self.group_selection(true);
        } else if !converted {
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
        self.clip_frag = Some(frag);
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

    /// Shift Cmd R: put the copied layers where each selected layer is, replacing it.
    pub fn paste_to_replace(&mut self) {
        let Some(frag) = self.clip_frag.clone() else {
            self.toast("Copy something first".into());
            return;
        };
        let targets = self.doc.sort_by_z(&self.sel_roots());
        if targets.is_empty() {
            return;
        }
        self.checkpoint();
        let mut out = vec![];
        for t in targets {
            let (page, parent, index, center) = (self.doc.page_of(t), self.doc.node(t).parent, self.doc.index_in_parent(t), self.doc.node(t).aabb().center());
            let ids = self.doc.paste(&frag, page, parent, Some(index));
            let b = self.doc.bounds(&ids);
            for &id in &ids {
                self.doc.translate(id, center - b.center());
            }
            self.doc.remove(t);
            out.extend(ids);
        }
        self.doc.reflow();
        self.sel = out;
    }

    /// Alt Cmd A: every layer on the page that looks like the selected one.
    pub fn select_matching(&mut self) {
        let Some(&first) = self.sel.first() else { return };
        let a = self.doc.node(first).clone();
        let same = |b: &Node| {
            b.kind == a.kind
                && b.fills == a.fills
                && b.strokes == a.strokes
                && b.stroke_width == a.stroke_width
                && b.shadows == a.shadows
                && b.radius == a.radius
                && b.component == a.component
                && (a.kind != Kind::Text || (b.font_family == a.font_family && b.font_size == a.font_size && b.font_weight == a.font_weight))
        };
        let page = self.doc.pages[self.page].children.clone();
        self.sel = page
            .iter()
            .flat_map(|&r| std::iter::once(r).chain(self.doc.descendants(r)))
            .filter(|&id| {
                let n = self.doc.node(id);
                n.visible && !n.locked && same(n)
            })
            .collect();
    }

    /// Copy the selection as SVG markup, a PNG image, or CSS.
    pub fn copy_as(&mut self, ctx: &egui::Context, what: &str) {
        let roots = self.sel_roots();
        if roots.is_empty() {
            return;
        }
        match what {
            "svg" => ctx.copy_text(crate::export::to_svg(&self.doc, &roots)),
            "css" => ctx.copy_text(roots.iter().map(|&id| crate::codegen::css(&self.doc, id)).collect::<Vec<_>>().join("\n\n")),
            _ => match crate::export::encode(&self.doc, &crate::export::to_svg(&self.doc, &roots), crate::doc::ExportFormat::Png, 2.0).ok().and_then(|png| image::load_from_memory(&png).ok()) {
                Some(img) => {
                    let rgba = img.to_rgba8();
                    ctx.copy_image(egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()));
                    self.toast("Copied as PNG".into());
                }
                None => self.toast("Couldn't render the selection".into()),
            },
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

    /// Figma's "Add variant" on a component: a variant gets a sibling in its set; a lone
    /// component becomes a set with a second variant.
    pub fn add_variant_to(&mut self, c: NodeId) {
        if self.doc.node(c).kind != Kind::Component {
            return;
        }
        if let Some(set) = self.doc.node(c).parent.filter(|p| self.doc.node(*p).kind == Kind::ComponentSet) {
            return self.add_variant(set);
        }
        self.checkpoint();
        let Some(&dup) = self.doc.duplicate(&[c]).first() else { return };
        if self.doc.combine_as_variants(&[c, dup]).is_some() {
            self.sel = vec![dup];
            self.doc.reflow();
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
            // Closing a path keeps the pen, as in Figma: Escape then leaves it with the path selected.
            if !closed {
                self.tool = Tool::Move;
            }
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
        if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("fig")) {
            return self.import_fig(&p);
        }
        let parsed = std::fs::read_to_string(&p)
            .map_err(|e| e.to_string())
            .and_then(|s| crate::doc::parse_file(&s));
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

    /// Open a Figma `.fig` as a new, unsaved document (the original is never written to).
    pub fn import_fig(&mut self, p: &std::path::Path) {
        let result = std::fs::read(p).map_err(|e| e.to_string()).and_then(|b| crate::fig_import::import(&b));
        match result {
            Ok(doc) => {
                self.reset_to(App::new(None));
                self.doc = doc;
                self.dirty = true;
                let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                self.draft_name = Some(name.clone());
                self.toast(format!("Imported {name}: {} layers. Save to keep it as a .katachi file.", self.doc.nodes.len()));
            }
            Err(e) => self.toast(format!("Couldn't import: {e}")),
        }
    }

    pub fn save(&mut self, save_as: bool) -> bool {
        let path = match (&self.path, save_as) {
            (Some(p), false) => p.clone(),
            _ => match rfd::FileDialog::new().add_filter("Katachi", &["katachi"]).set_file_name(format!("{}.katachi", self.display_name())).save_file() {
                Some(p) => p,
                None => return false,
            },
        };
        self.save_to(path)
    }

    pub fn save_to(&mut self, path: PathBuf) -> bool {
        self.remember_view();
        let json = self.document_json();
        match crate::files::write_atomic(&path, json.as_bytes()) {
            Ok(()) => {
                crate::files::save_version(&path, json.as_bytes());
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

    /// Export the selection (or page) with each layer's presets, or the panel's default.
    pub fn export(&mut self, ctx: &egui::Context) {
        self.fit_all_texts(ctx);
        use crate::doc::{ExportFormat, ExportSetting};
        let ids = if self.sel.is_empty() { self.doc.pages[self.page].children.clone() } else { self.sel_roots() };
        if ids.is_empty() {
            return;
        }
        let fallback = ExportSetting { scale: self.export_scale, format: if self.export_svg { ExportFormat::Svg } else { ExportFormat::Png }, suffix: String::new() };
        // Each job: (ids to render, base name, preset).
        let mut jobs: Vec<(Vec<NodeId>, String, ExportSetting)> = vec![];
        if self.sel.is_empty() {
            jobs.push((ids.clone(), self.doc.pages[self.page].name.clone(), fallback.clone()));
        } else {
            for &id in &ids {
                let n = self.doc.node(id);
                let presets = if n.exports.is_empty() { vec![fallback.clone()] } else { n.exports.clone() };
                for p in presets {
                    jobs.push((vec![id], n.name.clone(), p));
                }
            }
        }
        let file_name = |name: &str, p: &ExportSetting| {
            let scale = if p.format.raster() && p.scale != 1.0 { format!("@{}x", crate::canvas::num(p.scale)) } else { String::new() };
            format!("{}{}{}.{}", name.replace('/', "-"), p.suffix, scale, p.format.ext())
        };
        // One file: ask where. Several: ask for a folder.
        let targets: Vec<std::path::PathBuf> = if jobs.len() == 1 {
            let (_, name, p) = &jobs[0];
            let ext = p.format.ext();
            match rfd::FileDialog::new().add_filter(ext.to_uppercase(), &[ext]).set_file_name(file_name(name, p)).save_file() {
                Some(path) => vec![path],
                None => return,
            }
        } else {
            match rfd::FileDialog::new().pick_folder() {
                Some(dir) => jobs.iter().map(|(_, name, p)| dir.join(file_name(name, p))).collect(),
                None => return,
            }
        };
        let mut done = 0;
        for ((ids, _, p), path) in jobs.iter().zip(&targets) {
            // A slice exports whatever lies inside it.
            let svg = match ids.as_slice() {
                [one] if self.doc.node(*one).kind == Kind::Slice => {
                    let n = self.doc.node(*one);
                    crate::export::region_svg(&self.doc, self.page, n.aabb(), n.id)
                }
                _ => crate::export::to_svg(&self.doc, ids),
            };
            match crate::export::encode(&self.doc, &svg, p.format, p.scale).and_then(|d| std::fs::write(path, d).map_err(|e| e.to_string())) {
                Ok(()) => done += 1,
                Err(e) => return self.toast(format!("Export failed: {e}")),
            }
        }
        self.toast(if done == 1 {
            format!("Exported {}", targets[0].file_name().unwrap_or_default().to_string_lossy())
        } else {
            format!("Exported {done} files")
        });
    }

    /// New and Open make tabs.
    pub fn request_new(&mut self) {
        self.want = Some(Want::NewTab);
    }

    pub fn request_open(&mut self) {
        self.want = Some(Want::OpenDialog);
    }

    #[cfg(test)]
    pub fn new_file(&mut self) {
        self.reset_to(App::new(None));
    }

    /// Open an autosave from a crashed session as unsaved work.
    pub fn recover(&mut self, p: &std::path::Path) {
        let parsed = std::fs::read_to_string(p)
            .ok()
            .and_then(|s| crate::doc::parse_file(&s).ok());
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

    /// File > Version history: every save of this file, restorable (undoably).
    fn history_window(&mut self, ctx: &egui::Context) {
        if !self.show_history {
            return;
        }
        let Some(path) = self.path.clone() else {
            self.show_history = false;
            self.toast("Save the file to start its version history".into());
            return;
        };
        let mut open = true;
        let mut restore = None;
        egui::Window::new("Version history").open(&mut open).collapsible(false).resizable(false).default_width(300.0).show(ctx, |ui| {
            let versions = crate::files::versions(&path);
            if versions.is_empty() {
                ui.label("No saved versions yet.");
            }
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                for (i, (p, age)) in versions.iter().enumerate() {
                    let when = match age {
                        0..=59 => "Just now".to_string(),
                        60..=3599 => format!("{} min ago", age / 60),
                        3600..=86399 => format!("{} h ago", age / 3600),
                        _ => format!("{} days ago", age / 86400),
                    };
                    ui.horizontal(|ui| {
                        ui.label(if i == 0 { format!("{when} (latest save)") } else { when });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Restore").clicked() {
                                restore = Some(p.clone());
                            }
                        });
                    });
                }
            });
        });
        if let Some(p) = restore {
            let doc = std::fs::read_to_string(&p)
                .ok()
                .and_then(|s| crate::doc::parse_file(&s).ok());
            if let Some(doc) = doc {
                self.checkpoint();
                self.doc = doc;
                self.sel.clear();
                self.page = 0;
                self.view_ready = false;
                self.toast("Version restored (undo with ⌘Z)".into());
            }
        }
        self.show_history = open;
    }

    /// Run plugin code on the current page as one undoable step.
    /// Run plugin code on the current page as one undoable step; plugins that show a UI keep running.
    pub fn run_plugin(&mut self, name: &str, id: Option<&str>, code: &str, html: Option<&str>) {
        self.plugin = None;
        let before = self.doc.clone();
        let sel = self.sel.clone();
        let mut session = None;
        let res = crate::plugins::Session::start(&mut self.doc, self.page, &sel, name, id, code, html).map(|(s, out)| {
            session = Some(s);
            out
        });
        let Some(out) = self.plugin_apply(name, before, res) else { return };
        match (session, out.ui) {
            (Some(session), Some(ui)) if !out.closed => {
                self.plugin = Some(PluginRun { session, ui, view: None, inbox: Default::default(), outbox: out.outbox, ready: false, pos: None, last_sel: self.sel.clone() });
            }
            _ if out.notes.is_empty() => self.toast(format!("Ran {name}")),
            _ => {}
        }
    }

    /// Make the changes since `before` one undo step (if there were any).
    pub fn keep_changes(&mut self, before: Document) {
        if self.doc.nodes != before.nodes || self.doc.pages != before.pages || self.doc.styles != before.styles || self.doc.variables != before.variables || self.doc.collections != before.collections {
            let after = std::mem::replace(&mut self.doc, before);
            self.checkpoint();
            self.doc = after;
        }
    }

    /// Keep a plugin step's changes as one undo step, or roll them back on error.
    fn plugin_apply(&mut self, name: &str, before: Document, res: Result<crate::plugins::Outcome, String>) -> Option<crate::plugins::Outcome> {
        match res {
            Ok(out) => {
                self.keep_changes(before);
                if out.page != self.page {
                    self.select_page(out.page);
                }
                self.sel = out.selection.clone();
                if let Some(n) = out.notes.last() {
                    self.toast(n.clone());
                }
                if std::env::var_os("KATACHI_PLUGIN").is_some() {
                    for n in &out.notes {
                        eprintln!("{name}: {n}");
                    }
                }
                Some(out)
            }
            Err(e) => {
                self.doc = before;
                self.toast(format!("{name} failed: {}", e.lines().next().unwrap_or_default()));
                eprintln!("{name}: {e}");
                None
            }
        }
    }

    /// macOS: the title bar strip under the traffic lights, in the panel color (glass when glass is on).
    /// Drag to move the window, double-click to zoom.
    fn title_bar(&mut self, ui: &mut egui::Ui) {
        if !self.own_title_bar {
            return;
        }
        let ctx = ui.ctx().clone();
        let fill = ui.visuals().panel_fill;
        let line = crate::appearance::border(&ctx);
        egui::Panel::top("titlebar").exact_size(28.0).resizable(false).show_separator_line(false).frame(egui::Frame::NONE.fill(fill)).show(ui, |ui| {
            let r = ui.max_rect();
            let resp = ui.interact(r, egui::Id::new("titlebar-drag"), egui::Sense::click_and_drag());
            if resp.drag_started() {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            if resp.double_clicked() {
                let max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
            }
            ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, &self.title, egui::FontId::proportional(12.5), crate::appearance::muted(&ctx));
            ui.painter().hline(r.x_range(), r.max.y - 0.5, egui::Stroke::new(1.0, line));
        });
    }

    /// The appearance window, and keeping the window's desktop blur in step with glass.
    fn appearance_frame(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        if self.show_appearance {
            let mut open = true;
            if crate::appearance::window(ctx, &mut open, &mut self.appearance) {
                crate::appearance::apply(ctx, &self.appearance);
                crate::appearance::save(&self.appearance);
            }
            self.show_appearance = open;
        }
        let want = (self.appearance.glass, self.appearance.blur);
        if self.glass_applied != Some(want) {
            crate::appearance::set_window_glass(frame, want.0, want.1);
            self.glass_applied = Some(want);
        }
    }

    /// Start the MCP server on the first frame, then answer tool calls between frames.
    fn mcp_frame(&mut self, ctx: &egui::Context) {
        if !self.mcp_tried {
            self.mcp_tried = true;
            self.mcp = crate::mcp::start(ctx.clone());
        }
        let calls: Vec<crate::mcp::Call> = self.mcp.as_ref().map(|rx| rx.try_iter().collect()).unwrap_or_default();
        for c in calls {
            let result = self.mcp_tool(ctx, &c.name, &c.args);
            let _ = c.reply.send(result);
            ctx.request_repaint();
        }
    }

    pub fn mcp_listening(&self) -> bool {
        self.mcp.is_some()
    }

    /// A running plugin with a UI: pass messages both ways, report selection changes, draw its panel.
    fn plugin_frame(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        if let Some(manifest) = self.debug_plugin.take() {
            let p = crate::plugins::Installed { name: "Plugin".into(), manifest };
            match crate::plugins::load(&p) {
                Ok((code, html, id)) => self.run_plugin(&id, Some(&id), &code, html.as_deref()),
                Err(e) => self.toast(e),
            }
        }
        let Some(mut run) = self.plugin.take() else { return };
        let name = run.session.name.clone();
        let mut outs = vec![];
        let inbox: Vec<String> = std::mem::take(&mut *run.inbox.lock().unwrap());
        for m in inbox {
            if m == crate::plugin_view::READY {
                run.ready = true;
                continue;
            }
            let (before, sel) = (self.doc.clone(), self.sel.clone());
            let res = run.session.message(&mut self.doc, self.page, &sel, &m);
            outs.extend(self.plugin_apply(&name, before, res));
        }
        if self.sel != run.last_sel {
            let (before, sel) = (self.doc.clone(), self.sel.clone());
            match run.session.emit(&mut self.doc, self.page, &sel, "selectionchange") {
                Ok(None) => {}
                res => outs.extend(self.plugin_apply(&name, before, res.map(|o| o.unwrap()))),
            }
            run.last_sel = self.sel.clone();
        }
        for o in outs {
            run.outbox.extend(o.outbox);
            match o.ui {
                Some(ui) if !o.closed => run.ui = ui,
                _ => return,
            }
        }
        // The panel: a title bar drawn by egui, the plugin's HTML in a native webview below it.
        // A UI in a browser tab gets just a title bar here.
        let in_window = run.view.as_ref().is_none_or(|v| v.in_window());
        let size = if in_window { vec2(run.ui.width, run.ui.height) } else { vec2(run.ui.width.max(260.0), 30.0) };
        let canvas = self.canvas_rect.unwrap_or(ctx.content_rect());
        let pos = *run.pos.get_or_insert(egui::pos2(canvas.max.x - size.x - 16.0, canvas.min.y + 16.0));
        let title = 30.0;
        let (mut close, mut drag) = (false, Vec2::ZERO);
        if run.ui.visible {
            egui::Area::new(egui::Id::new("plugin-panel")).fixed_pos(pos).order(egui::Order::Foreground).show(ctx, |ui| {
                egui::Frame::window(ui.style()).inner_margin(0).show(ui, |ui| {
                    let (bar, resp) = ui.allocate_exact_size(vec2(size.x, title), egui::Sense::drag());
                    ui.painter().text(bar.left_center() + vec2(12.0, 0.0), egui::Align2::LEFT_CENTER, &name, egui::FontId::proportional(13.0), ui.visuals().strong_text_color());
                    let x = Rect::from_center_size(bar.right_center() - vec2(16.0, 0.0), vec2(20.0, 20.0));
                    let xr = ui.interact(x, ui.id().with("plugin-close"), egui::Sense::click());
                    let c = if xr.hovered() { ui.visuals().strong_text_color() } else { ui.visuals().weak_text_color() };
                    let s = egui::Stroke::new(1.5, c);
                    let r = x.shrink(5.0);
                    ui.painter().line_segment([r.left_top(), r.right_bottom()], s);
                    ui.painter().line_segment([r.right_top(), r.left_bottom()], s);
                    close = xr.clicked();
                    drag = resp.drag_delta();
                    if in_window {
                        ui.allocate_exact_size(size, egui::Sense::hover());
                    } else if let Some(url) = run.view.as_ref().and_then(|v| v.url()) {
                        ui.horizontal(|ui| {
                            ui.add_space(12.0);
                            ui.label("The plugin's window is open in your browser.");
                            if ui.link("Reopen").clicked() {
                                crate::plugin_view::open_url(url);
                            }
                        });
                        ui.add_space(8.0);
                    }
                });
            });
        }
        if close {
            let (before, sel) = (self.doc.clone(), self.sel.clone());
            if let Ok(Some(o)) = run.session.emit(&mut self.doc, self.page, &sel, "close") {
                self.plugin_apply(&name, before, Ok(o));
            }
            return;
        }
        run.pos = Some(pos + drag);
        let body = Rect::from_min_size(run.pos.unwrap() + vec2(0.0, title), size);
        match &run.view {
            Some(v) => {
                v.place(body, ctx.zoom_factor());
                v.show(run.ui.visible);
            }
            None => match crate::plugin_view::PluginView::open(frame, ctx, &run.ui.html, body, run.inbox.clone()) {
                Ok(v) => run.view = Some(v),
                Err(e) => {
                    self.toast(format!("{name}: {e}"));
                    return;
                }
            },
        }
        if run.ready
            && let Some(v) = &run.view
        {
            for m in run.outbox.drain(..) {
                v.post(&m);
            }
        }
        self.plugin = Some(run);
    }

    fn find_window(&mut self, ctx: &egui::Context) {
        if !self.find.0 {
            return;
        }
        let mut open = true;
        let (mut jump, mut replace) = (None, false);
        let q = self.find.1.trim().to_lowercase();
        // Matches across every page: layer names, and the content of text layers.
        let hits: Vec<(usize, NodeId)> = if q.is_empty() {
            vec![]
        } else {
            let mut hits = vec![];
            for (pi, p) in self.doc.pages.iter().enumerate() {
                for id in p.children.iter().flat_map(|&r| std::iter::once(r).chain(self.doc.descendants(r))) {
                    let n = self.doc.node(id);
                    if n.name.to_lowercase().contains(&q) || (n.kind == Kind::Text && n.text.to_lowercase().contains(&q)) {
                        hits.push((pi, id));
                    }
                }
            }
            hits
        };
        egui::Window::new("Find and replace").open(&mut open).collapsible(false).default_width(320.0).show(ctx, |ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut self.find.1).hint_text("Find layers and text").desired_width(f32::INFINITY));
            if std::mem::take(&mut self.find.3) {
                r.request_focus();
            }
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.find.2).hint_text("Replace text with").desired_width(ui.available_width() - 90.0));
                let texts = hits.iter().filter(|(_, id)| self.doc.node(*id).kind == Kind::Text && self.doc.node(*id).text.to_lowercase().contains(&q)).count();
                replace = ui.add_enabled(texts > 0, egui::Button::new("Replace all")).clicked();
            });
            if !q.is_empty() {
                ui.label(egui::RichText::new(format!("{} results", hits.len())).weak());
            }
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                for &(pi, id) in hits.iter().take(500) {
                    let n = self.doc.node(id);
                    let detail = if n.kind == Kind::Text && n.text.to_lowercase().contains(&q) { n.text.lines().find(|l| l.to_lowercase().contains(&q)).unwrap_or("").to_string() } else { String::new() };
                    let page = if self.doc.pages.len() > 1 { format!("{}: ", self.doc.pages[pi].name) } else { String::new() };
                    let label = if detail.is_empty() { format!("{page}{}", n.name) } else { format!("{page}{}  \u{201C}{detail}\u{201D}", n.name) };
                    if ui.add(egui::Button::new(label).truncate().frame(false)).clicked() {
                        jump = Some((pi, id));
                    }
                }
            });
        });
        if let Some((pi, id)) = jump {
            self.select_page(pi);
            self.sel = vec![id];
            self.zoom_to(ctx, Some(vec![id]), false);
        }
        if replace {
            self.checkpoint();
            let with = self.find.2.clone();
            let mut count = 0;
            for &(_, id) in &hits {
                let n = self.doc.node_mut(id);
                if n.kind == Kind::Text {
                    count += crate::doc::replace_text(n, &q, &with);
                }
            }
            self.doc.reflow();
            self.toast(format!("Replaced {count} matches"));
        }
        self.find.0 = open;
    }

    fn console_window(&mut self, ctx: &egui::Context) {
        if !self.show_console {
            return;
        }
        let mut open = true;
        let mut run = false;
        egui::Window::new("Run script").open(&mut open).collapsible(false).default_width(420.0).show(ctx, |ui| {
            ui.label(egui::RichText::new("JavaScript against Figma's plugin API (figma.currentPage, createRectangle, notify...)").weak());
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                ui.add(egui::TextEdit::multiline(&mut self.console_code).code_editor().desired_rows(12).desired_width(f32::INFINITY));
            });
            run = ui.button("Run").clicked();
        });
        if run {
            let code = self.console_code.clone();
            self.run_plugin("Script", None, &code, None);
        }
        self.show_console = open;
    }

    /// Host or join a live session.
    fn collab_window(&mut self, ctx: &egui::Context) {
        if !self.show_collab {
            return;
        }
        let mut open = true;
        let mut action: Option<&str> = None;
        egui::Window::new("Collaborate").open(&mut open).collapsible(false).resizable(false).default_width(320.0).show(ctx, |ui| {
            if let Some(c) = &self.collab {
                ui.label(egui::RichText::new(if c.hosting { "Hosting a live session" } else { "In a live session" }).strong());
                ui.horizontal(|ui| {
                    ui.label("Address");
                    ui.monospace(&c.address);
                    if ui.small_button("Copy").clicked() {
                        ui.ctx().copy_text(c.address.clone());
                    }
                });
                ui.separator();
                ui.label(format!("{} (you)", c.me.name));
                for p in c.peers.values() {
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                        ui.painter().circle_filled(r.center(), 6.0, crate::collab::PEER_COLORS[p.color % crate::collab::PEER_COLORS.len()]);
                        ui.label(&p.name);
                    });
                }
                ui.separator();
                if ui.button("Leave session").clicked() {
                    action = Some("leave");
                }
            } else {
                ui.horizontal(|ui| {
                    ui.label("Your name");
                    ui.text_edit_singleline(&mut self.collab_name);
                });
                ui.separator();
                ui.label(egui::RichText::new("Host").strong());
                ui.label("Share this file live with people on your network.");
                ui.horizontal(|ui| {
                    ui.label("Port");
                    ui.add(egui::DragValue::new(&mut self.collab_port).range(1024..=65535));
                    if ui.button("Start session").clicked() {
                        action = Some("host");
                    }
                });
                ui.separator();
                ui.label(egui::RichText::new("Join").strong());
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.collab_address).hint_text("192.168.1.20:7878").desired_width(180.0));
                    if ui.add_enabled(!self.collab_address.trim().is_empty(), egui::Button::new("Join")).clicked() {
                        action = Some("join");
                    }
                });
            }
        });
        match action {
            Some("leave") => self.collab = None,
            Some("host") => self.host_session(ctx),
            Some("join") => {
                if self.dirty {
                    self.toast("Save or discard your changes before joining".into());
                } else {
                    let addr = self.collab_address.trim().to_string();
                    self.join_session(ctx, &addr, false);
                }
            }
            _ => {}
        }
        self.show_collab = open;
    }

    pub fn host_session(&mut self, ctx: &egui::Context) {
        let listener = std::net::TcpListener::bind(("0.0.0.0", self.collab_port)).or_else(|_| std::net::TcpListener::bind(("0.0.0.0", 0)));
        match listener {
            Ok(l) => {
                let port = l.local_addr().map_or(self.collab_port, |a| a.port());
                crate::collab::serve(l, self.doc.clone());
                let shared = format!("{}:{port}", crate::collab::local_ip());
                self.join_session(ctx, &format!("127.0.0.1:{port}"), true);
                if let Some(c) = self.collab.as_mut() {
                    c.address = shared;
                }
            }
            Err(e) => self.toast(format!("Couldn't start a session: {e}")),
        }
    }

    pub fn join_session(&mut self, ctx: &egui::Context, addr: &str, hosting: bool) {
        let joined = crate::collab::connect(addr, Some(ctx.clone()))
            .and_then(|link| crate::collab::Collab::join(link, addr.to_string(), self.collab_name.clone(), hosting));
        match joined {
            Ok((c, doc)) => {
                if !hosting {
                    // A joined file is new to this machine: no path until saved.
                    self.path = None;
                    self.dirty = false;
                    self.view_ready = false;
                }
                self.doc = doc;
                self.sel.clear();
                self.page = 0;
                self.undo.clear();
                self.undo_touched.clear();
                self.redo.clear();
                self.redo_touched.clear();
                self.collab = Some(c);
                self.toast(if hosting { "Session started".into() } else { "Joined the session".into() });
            }
            Err(e) => self.toast(format!("Couldn't connect: {e}")),
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

    /// Alt Cmd O: strokes become filled vectors. Layers that also have fills become a group of both.
    pub fn outline_stroke_selection(&mut self) {
        let targets: Vec<NodeId> = self
            .sel_roots()
            .into_iter()
            .filter(|&id| {
                let n = self.doc.node(id);
                n.kind != Kind::Text && n.stroke_width > 0.0 && n.strokes.iter().any(|s| s.visible)
            })
            .collect();
        if targets.is_empty() {
            return;
        }
        self.checkpoint();
        let mut out = vec![];
        for id in targets {
            let n = self.doc.node(id).clone();
            let paths = crate::boolean::stroke_outline(&n);
            if paths.is_empty() {
                continue;
            }
            let mut v = Node::vector_paths(paths);
            v.name = n.name.clone();
            v.fills = n.strokes.iter().filter(|s| s.visible).cloned().collect();
            v.strokes.clear();
            v.stroke_width = 0.0;
            v.opacity = n.opacity;
            v.even_odd = false;
            let (page, parent, index) = (self.doc.page_of(id), n.parent, self.doc.index_in_parent(id));
            let vid = self.doc.insert(page, parent, Some(index + 1), v);
            if n.fills.iter().any(|f| f.visible) {
                self.doc.node_mut(id).strokes.clear();
                if let Some(g) = self.doc.group(&[id, vid]) {
                    self.doc.node_mut(g).name = n.name.clone();
                    out.push(g);
                }
            } else {
                self.doc.remove(id);
                out.push(vid);
            }
        }
        self.doc.reflow();
        self.sel = out;
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
            (true, false, Key::Slash | Key::K) if !m.alt => return self.toggle_quick_actions(),
            (true, false, Key::S) => return void(self.save(false)),
            (true, true, Key::S) => return void(self.save(true)),
            (true, false, Key::O) => return self.request_open(),
            (true, false, Key::N) => return self.request_new(),
            (true, true, Key::E) => return self.export(ctx),
            (true, false, Key::F) => {
                self.find.0 = true;
                self.find.3 = true;
                return;
            }
            (true, true, Key::Num7) => return self.toggle_list(ctx, crate::doc::ListKind::Number),
            (true, true, Key::Num8) => return self.toggle_list(ctx, crate::doc::ListKind::Bullet),
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
        // Ctrl+G: show/hide layout guides.
        if m.ctrl && !m.mac_cmd && !m.alt && key == Key::G {
            self.show_grids = !self.show_grids;
            return;
        }
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
            (true, true, _, Key::Z) => self.redo(),
            (true, false, false, Key::D) => self.duplicate_selection(),
            (true, false, true, Key::C) => self.style_clip = self.sel.first().map(|&id| self.doc.node(id).clone()),
            (true, false, true, Key::V) => self.paste_style(),
            (true, true, false, Key::R) => self.paste_to_replace(),
            (true, false, false, Key::Y) => self.outline_mode = !self.outline_mode,
            (true, false, true, Key::O) => self.outline_stroke_selection(),
            (true, false, true, Key::A) => self.select_matching(),
            (true, true, false, Key::C) => self.copy_as(ctx, "png"),
            (true, false, false, Key::R) => {
                if let Some(&id) = self.sel.first() {
                    self.left_tab_assets = false;
                    self.renaming = Some((id, self.doc.node(id).name.clone()));
                }
            }
            (true, false, false, Key::Backslash) => self.ui_hidden = !self.ui_hidden,
            (false, true, false, Key::R) => self.rulers = !self.rulers,
            (false, true, false, Key::D) => self.dev_mode = !self.dev_mode,
            (false, false, true, Key::L) => {
                for n in self.doc.nodes.values_mut() {
                    if n.expanded {
                        std::sync::Arc::make_mut(n).expanded = false;
                    }
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
            (false, false, false, Key::C) => self.set_tool(Tool::Comment),
            (false, true, false, Key::C) => self.show_comments = !self.show_comments,
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
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        // Glass lets the (blurred) desktop through wherever panels are translucent.
        if self.appearance.glass { [0.0; 4] } else { visuals.panel_fill.to_normalized_gamma_f32() }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.presenting.is_some() {
            egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| self.presentation_ui(ui));
            return;
        }
        self.shortcuts(&ctx);
        self.title_bar(ui);
        if !self.ui_hidden {
            self.left_panel(ui);
            self.right_panel(ui);
        }
        self.canvas_panel(ui);
        self.close_guard(&ctx);
        let now = ctx.input(|i| i.time);
        self.sync_collab(now);
        if self.collab.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        self.collab_window(&ctx);
        self.recovery_prompt(&ctx);
        self.history_window(&ctx);
        self.console_window(&ctx);
        self.find_window(&ctx);
        self.quick_actions(&ctx);
        self.plugin_frame(&ctx, frame);
        self.appearance_frame(&ctx, frame);
        self.mcp_frame(&ctx);
        self.variables_window(&ctx);
        self.autosave(ctx.input(|i| i.time));
        self.debug_screenshot(&ctx);

        // A gesture (and its single undo step) ends when the pointer is up and no field is focused.
        if !ctx.input(|i| i.pointer.any_down()) && !ctx.egui_wants_keyboard_input() {
            self.in_gesture = false;
        }
        let title = format!("{}{} – Katachi", self.display_name(), if self.dirty { " (Edited)" } else { "" });
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
            After::CloseTab => {
                self.clear_recovery();
                self.allow_close = true;
                self.want = Some(Want::CloseTab);
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
