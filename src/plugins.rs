//! Plugins: run JavaScript written against Figma's plugin API (a core subset) with QuickJS.
//! A plugin session keeps its JS state alive; each event (start, a UI message, a selection change)
//! runs against a fresh JS copy of the current page, and the changes come back as one batch.

use crate::doc::{Document, Kind, Node, NodeId, Paint, PaintKind, Rgba};
use egui::{Rect, pos2, vec2};
use serde_json::{Value, json};
use std::collections::HashMap;

/// `figma` global and node prototype, implemented over plain objects.
const PRELUDE: &str = r#"
const __nodes = new Map();
let __removed = [], __notes = [], __closed = false, __newId = 0, __errors = [], __outbox = [], __ui = null, __storage = {}, __result;
const __handlers = {}, __uiHandlers = [];
function __reset() { __removed = []; __notes = []; __errors = []; __outbox = []; __result = undefined; }
// A returned value as plain JSON: layers become { id, type, name }.
function __plain(v, depth) {
  depth = depth || 0;
  if (v === null || typeof v !== "object" || depth > 20) return typeof v === "function" || typeof v === "symbol" ? undefined : v;
  if (Object.getPrototypeOf(v) === __NodeProto) return { id: v.id, type: v.type, name: v.name };
  if (Array.isArray(v)) return v.map(x => __plain(x, depth + 1));
  const o = {};
  for (const k of Object.keys(v)) o[k] = __plain(v[k], depth + 1);
  return o;
}
function __safe(f) { try { Promise.resolve(f()).catch(e => __errors.push(String(e) + (e && e.stack ? '\n' + e.stack : ''))); } catch (e) { __errors.push(String(e) + (e && e.stack ? '\n' + e.stack : '')); } }
function __deliver(json) {
  const msg = JSON.parse(json);
  if (typeof figma.ui.onmessage === "function") __safe(() => figma.ui.onmessage(msg, { origin: "null" }));
  for (const h of __uiHandlers) __safe(() => h(msg, { origin: "null" }));
}
function __emit(type, arg) { for (const h of __handlers[type] || []) __safe(() => h(arg)); }
function __listens(type) { return (__handlers[type] || []).length > 0; }
function __link(n) {
  Object.setPrototypeOf(n, __NodeProto);
  __nodes.set(n.id, n);
  return n;
}
const __NodeProto = {
  resize(w, h) { this.width = w; this.height = h; },
  resizeWithoutConstraints(w, h) { this.resize(w, h); },
  remove() {
    __removed.push(this.id);
    if (this.parent) this.parent.children = this.parent.children.filter(c => c !== this);
    __nodes.delete(this.id);
    this.removed = true;
  },
  appendChild(c) { this.insertChild((this.children || []).length, c); },
  insertChild(i, c) {
    if (c.parent && c.parent.children) c.parent.children = c.parent.children.filter(x => x !== c);
    this.children = this.children || [];
    this.children.splice(i, 0, c);
    c.parent = this;
  },
  clone() {
    const copy = (n, parent) => {
      const c = __link(Object.assign({}, n, { id: "new:" + (++__newId), parent, children: undefined }));
      if (n.children) c.children = n.children.map(k => copy(k, c));
      c.fills = JSON.parse(JSON.stringify(n.fills || []));
      c.strokes = JSON.parse(JSON.stringify(n.strokes || []));
      return c;
    };
    const c = copy(this, this.parent);
    if (this.parent) { const s = this.parent.children; s.splice(s.indexOf(this) + 1, 0, c); }
    return c;
  },
  findAll(fn) {
    const out = [];
    const walk = n => { for (const c of n.children || []) { if (!fn || fn(c)) out.push(c); walk(c); } };
    walk(this);
    return out;
  },
  findOne(fn) { return this.findAll(fn)[0] || null; },
  findChildren(fn) { return (this.children || []).filter(fn || (() => true)); },
  findChild(fn) { return this.findChildren(fn)[0] || null; },
  findAllWithCriteria(c) { return this.findAll(n => !c.types || c.types.includes(n.type)); },
  setPluginData() {}, getPluginData() { return ""; },
  setRelaunchData() {},
  get absoluteBoundingBox() {
    let x = this.x, y = this.y;
    for (let p = this.parent; p && p.type !== "PAGE"; p = p.parent) { x += p.x; y += p.y; }
    return { x, y, width: this.width, height: this.height };
  },
};
function __create(type, defaults) {
  const n = __link(Object.assign({ id: "new:" + (++__newId), type, name: type[0] + type.slice(1).toLowerCase(), x: 0, y: 0, width: 100, height: 100,
    rotation: 0, opacity: 1, visible: true, locked: false, fills: [], strokes: [], strokeWeight: 1, cornerRadius: 0,
    children: ["FRAME", "GROUP", "COMPONENT"].includes(type) ? [] : undefined }, defaults || {}));
  figma.currentPage.appendChild(n);
  return n;
}
const __gray = [{ type: "SOLID", color: { r: 0.851, g: 0.851, b: 0.851 }, opacity: 1, visible: true }];
const figma = {
  apiVersion: "1.0.0", editorType: "figma", mixed: Symbol("mixed"),
  notify(m) { __notes.push(String(m)); return { cancel() {} }; },
  closePlugin(m) { if (m) __notes.push(String(m)); __closed = true; },
  showUI(html, opts) {
    opts = opts || {};
    __ui = { html: String(html), width: opts.width || 300, height: opts.height || 200, visible: opts.visible !== false, title: opts.title || null };
  },
  ui: {
    postMessage(m) { __outbox.push(JSON.stringify(m === undefined ? null : m)); },
    onmessage: undefined,
    on(type, cb) { if (type === "message") __uiHandlers.push(cb); },
    once(type, cb) { this.on(type, cb); },
    off(type, cb) { const i = __uiHandlers.indexOf(cb); if (i >= 0) __uiHandlers.splice(i, 1); },
    resize(w, h) { if (__ui) { __ui.width = w; __ui.height = h; } },
    show() { if (__ui) __ui.visible = true; },
    hide() { if (__ui) __ui.visible = false; },
    close() { __ui = null; },
  },
  on(type, cb) { (__handlers[type] = __handlers[type] || []).push(cb); },
  once(type, cb) { this.on(type, cb); },
  off(type, cb) { const l = __handlers[type] || []; const i = l.indexOf(cb); if (i >= 0) l.splice(i, 1); },
  loadFontAsync() { return Promise.resolve(); },
  listAvailableFontsAsync() { return Promise.resolve([]); },
  getNodeById(id) { return __nodes.get(id) || null; },
  getNodeByIdAsync(id) { return Promise.resolve(__nodes.get(id) || null); },
  createRectangle() { return __create("RECTANGLE", { fills: JSON.parse(JSON.stringify(__gray)) }); },
  createEllipse() { return __create("ELLIPSE", { fills: JSON.parse(JSON.stringify(__gray)) }); },
  createPolygon() { return __create("POLYGON", { fills: JSON.parse(JSON.stringify(__gray)), pointCount: 3 }); },
  createStar() { return __create("STAR", { fills: JSON.parse(JSON.stringify(__gray)), pointCount: 5 }); },
  createLine() { return __create("LINE", { height: 0, strokes: [{ type: "SOLID", color: { r: 0, g: 0, b: 0 }, opacity: 1, visible: true }] }); },
  createFrame() { return __create("FRAME", { fills: [{ type: "SOLID", color: { r: 1, g: 1, b: 1 }, opacity: 1, visible: true }] }); },
  createComponent() { return __create("COMPONENT", { fills: [{ type: "SOLID", color: { r: 1, g: 1, b: 1 }, opacity: 1, visible: true }] }); },
  createText() { return __create("TEXT", { characters: "", fontSize: 12, fontName: { family: "Inter", style: "Regular" },
    fills: [{ type: "SOLID", color: { r: 0, g: 0, b: 0 }, opacity: 1, visible: true }] }); },
  group(nodes, parent) {
    const g = __create("GROUP", { children: [] });
    const xs = nodes.map(n => n.x), ys = nodes.map(n => n.y);
    g.x = Math.min(...xs); g.y = Math.min(...ys);
    g.width = Math.max(...nodes.map(n => n.x + n.width)) - g.x; g.height = Math.max(...nodes.map(n => n.y + n.height)) - g.y;
    (parent || figma.currentPage).appendChild(g);
    for (const n of nodes) { n.x -= g.x; n.y -= g.y; g.appendChild(n); }
    return g;
  },
  viewport: { center: { x: 0, y: 0 }, zoom: 1, scrollAndZoomIntoView() {} },
  clientStorage: {
    getAsync(k) { return Promise.resolve(__storage[k]); },
    setAsync(k, v) { __storage[k] = v; return Promise.resolve(); },
    deleteAsync(k) { delete __storage[k]; return Promise.resolve(); },
    keysAsync() { return Promise.resolve(Object.keys(__storage)); },
  },
  util: { rgb(c) { return c; }, solidPaint(c) { return { type: "SOLID", color: c, opacity: 1, visible: true }; } },
};
function __load(page) {
  __nodes.clear();
  const link = (raw, parent) => {
    const n = __link(raw);
    n.parent = parent;
    if (raw.children) n.children = raw.children.map(c => link(c, n));
    return n;
  };
  const p = { type: "PAGE", id: "page", name: page.name, children: [] };
  p.children = page.children.map(c => link(c, p));
  Object.setPrototypeOf(p, __NodeProto);
  let sel = page.selection.map(id => __nodes.get(id)).filter(Boolean);
  Object.defineProperty(p, "selection", { get() { return sel; }, set(v) { sel = [...v]; } });
  figma.currentPage = p;
  figma.root = { type: "DOCUMENT", name: "Document", children: [p] };
}
function __collect() {
  const out = [];
  const walk = (n, parentId) => {
    for (const c of n.children || []) {
      const { parent, children, ...rest } = c;
      out.push(Object.assign(rest, { parentId, childIds: children ? children.map(k => k.id) : null }));
      walk(c, c.id);
    }
  };
  walk(figma.currentPage, null);
  return JSON.stringify({ nodes: out, top: figma.currentPage.children.map(c => c.id), removed: __removed, notes: __notes,
    selection: figma.currentPage.selection.map(n => n.id), closed: __closed, errors: __errors, outbox: __outbox, ui: __ui,
    storage: JSON.stringify(__storage), result: __result === undefined ? null : JSON.stringify(__plain(__result)) });
}
"#;

fn figma_type(k: Kind) -> &'static str {
    match k {
        Kind::Frame => "FRAME",
        Kind::Group => "GROUP",
        Kind::Rectangle => "RECTANGLE",
        Kind::Ellipse => "ELLIPSE",
        Kind::Vector => "VECTOR",
        Kind::Text => "TEXT",
        Kind::Component => "COMPONENT",
        Kind::Instance => "INSTANCE",
        Kind::ComponentSet => "COMPONENT_SET",
        Kind::Boolean => "BOOLEAN_OPERATION",
        Kind::Polygon => "POLYGON",
        Kind::Star => "STAR",
        Kind::Section => "SECTION",
        Kind::Slice => "SLICE",
    }
}

fn kind_of(t: &str) -> Kind {
    match t {
        "FRAME" => Kind::Frame,
        "GROUP" => Kind::Group,
        "ELLIPSE" => Kind::Ellipse,
        "TEXT" => Kind::Text,
        "COMPONENT" => Kind::Component,
        "POLYGON" => Kind::Polygon,
        "STAR" => Kind::Star,
        "SECTION" => Kind::Section,
        "LINE" | "VECTOR" => Kind::Vector,
        _ => Kind::Rectangle,
    }
}

fn paint_json(p: &Paint) -> Value {
    let c = |c: Rgba| json!({ "r": c.0[0] as f32 / 255.0, "g": c.0[1] as f32 / 255.0, "b": c.0[2] as f32 / 255.0 });
    match p.kind {
        PaintKind::Solid => json!({ "type": "SOLID", "color": c(p.color), "opacity": p.alpha(), "visible": p.visible }),
        PaintKind::Linear | PaintKind::Radial => json!({
            "type": if p.kind == PaintKind::Linear { "GRADIENT_LINEAR" } else { "GRADIENT_RADIAL" },
            "gradientStops": p.stops.iter().map(|(t, s)| json!({ "position": t, "color": { "r": s.0[0] as f32 / 255.0, "g": s.0[1] as f32 / 255.0, "b": s.0[2] as f32 / 255.0, "a": s.a() as f32 / 255.0 } })).collect::<Vec<_>>(),
            "opacity": p.alpha(), "visible": p.visible,
        }),
        PaintKind::Image => json!({ "type": "IMAGE", "imageHash": p.image, "scaleMode": "FILL", "opacity": p.alpha(), "visible": p.visible }),
    }
}

fn paint_from(v: &Value, original: Option<&Paint>) -> Option<Paint> {
    let f = |c: &Value, k: &str| c.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0) as f32;
    let byte = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    let opacity = v.get("opacity").and_then(|x| x.as_f64()).unwrap_or(1.0) as f32;
    let visible = v.get("visible").and_then(|x| x.as_bool()).unwrap_or(true);
    let mut p = match v.get("type")?.as_str()? {
        "SOLID" => {
            let c = v.get("color")?;
            Paint::solid(Rgba([byte(f(c, "r")), byte(f(c, "g")), byte(f(c, "b")), byte(opacity)]))
        }
        // Other paint types round-trip unchanged when the plugin left them alone.
        _ => original.cloned()?,
    };
    p.visible = visible;
    Some(p)
}

/// The current page as Figma-shaped JSON (x/y relative to the parent, like the plugin API).
fn snapshot(doc: &Document, page: usize, sel: &[NodeId]) -> Value {
    fn node(doc: &Document, id: NodeId, origin: egui::Pos2) -> Value {
        let n = doc.node(id);
        let r = n.rect;
        let mut v = json!({
            "id": id.to_string(), "type": figma_type(n.kind), "name": n.name,
            "x": r.min.x - origin.x, "y": r.min.y - origin.y, "width": r.width(), "height": r.height(),
            "rotation": n.rotation, "opacity": n.opacity, "visible": n.visible, "locked": n.locked,
            "fills": n.fills.iter().map(paint_json).collect::<Vec<_>>(),
            "strokes": n.strokes.iter().map(paint_json).collect::<Vec<_>>(),
            "strokeWeight": n.stroke_width, "cornerRadius": n.radius,
        });
        if n.kind == Kind::Text {
            v["characters"] = json!(n.text);
            v["fontSize"] = json!(n.font_size);
            v["fontName"] = json!({ "family": n.font_family, "style": crate::text::weight_name(n.font_weight) });
        }
        if matches!(n.kind, Kind::Polygon | Kind::Star) {
            v["pointCount"] = json!(n.point_count);
        }
        if n.kind.is_container() {
            v["children"] = json!(n.children.iter().map(|&c| node(doc, c, r.min)).collect::<Vec<_>>());
        }
        v
    }
    let p = &doc.pages[page];
    json!({
        "name": p.name,
        "children": p.children.iter().map(|&c| node(doc, c, egui::Pos2::ZERO)).collect::<Vec<_>>(),
        "selection": sel.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
    })
}

pub struct Outcome {
    pub notes: Vec<String>,
    pub selection: Vec<NodeId>,
    /// Messages for the plugin's UI (JSON).
    pub outbox: Vec<String>,
    /// The UI the plugin wants shown, if any.
    pub ui: Option<UiRequest>,
    pub closed: bool,
    /// What the code returned (JSON), with new layers' ids filled in.
    pub result: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UiRequest {
    pub html: String,
    pub width: f32,
    pub height: f32,
    pub visible: bool,
}

/// A running plugin: its JS runtime stays alive between events.
pub struct Session {
    rt: rquickjs::Runtime,
    ctx: rquickjs::Context,
    pub name: String,
    storage: Option<(std::path::PathBuf, String)>,
}

impl Session {
    /// Start a plugin: define `figma` (and `__html__`), then run its main code.
    /// `id` names its clientStorage file; `html` is the manifest's UI file.
    pub fn start(doc: &mut Document, page: usize, sel: &[NodeId], name: &str, id: Option<&str>, code: &str, html: Option<&str>) -> Result<(Session, Outcome), String> {
        let rt = rquickjs::Runtime::new().map_err(|e| e.to_string())?;
        rt.set_memory_limit(512 << 20);
        let ctx = rquickjs::Context::full(&rt).map_err(|e| e.to_string())?;
        let storage = id.map(|id| {
            let safe: String = id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
            let path = crate::files::data_dir().join("plugin-storage").join(format!("{safe}.json"));
            let saved = std::fs::read_to_string(&path).unwrap_or_else(|_| "{}".into());
            (path, saved)
        });
        let mut s = Session { rt, ctx, name: name.to_string(), storage };
        let init = format!(
            "globalThis.__html__ = {}; __storage = {};",
            serde_json::to_string(html.unwrap_or("")).unwrap_or_default(),
            s.storage.as_ref().map_or("{}".to_string(), |(_, j)| if serde_json::from_str::<Value>(j).is_ok() { j.clone() } else { "{}".into() })
        );
        s.eval(PRELUDE)?;
        s.eval(&init)?;
        // Plugins are often async; errors are collected rather than thrown.
        let out = s.step(doc, page, sel, &format!("__safe(async () => {{ __result = await (async () => {{\n{code}\n}})(); }});"))?;
        Ok((s, out))
    }

    /// Deliver a message from the plugin's UI (`parent.postMessage({ pluginMessage })`).
    pub fn message(&mut self, doc: &mut Document, page: usize, sel: &[NodeId], json: &str) -> Result<Outcome, String> {
        let arg = serde_json::to_string(json).unwrap_or_default();
        self.step(doc, page, sel, &format!("__deliver({arg});"))
    }

    /// Tell the plugin something happened (`figma.on(type, ...)`), if it's listening.
    pub fn emit(&mut self, doc: &mut Document, page: usize, sel: &[NodeId], event: &str) -> Result<Option<Outcome>, String> {
        let ev = serde_json::to_string(event).unwrap_or_default();
        if !self.ctx.with(|ctx| ctx.eval::<bool, _>(format!("__listens({ev})")).unwrap_or(false)) {
            return Ok(None);
        }
        self.step(doc, page, sel, &format!("__emit({ev});")).map(Some)
    }

    fn eval(&self, js: &str) -> Result<(), String> {
        self.ctx.with(|ctx| match ctx.eval::<(), _>(js) {
            Ok(()) => Ok(()),
            Err(rquickjs::Error::Exception) => Err(format!("{:?}", ctx.catch())),
            Err(e) => Err(e.to_string()),
        })
    }

    /// Load the page into JS, run `js`, settle promises, and apply the changes.
    fn step(&mut self, doc: &mut Document, page: usize, sel: &[NodeId], js: &str) -> Result<Outcome, String> {
        let snap = snapshot(doc, page, sel).to_string();
        self.eval(&format!("__reset(); __load({snap});"))?;
        self.eval(js)?;
        for _ in 0..100_000 {
            match self.rt.execute_pending_job() {
                Ok(true) => continue,
                Ok(false) => break,
                Err(e) => return Err(format!("{e:?}")),
            }
        }
        let result: String = self.ctx.with(|ctx| ctx.eval::<String, _>("__collect()").map_err(|e| e.to_string()))?;
        let out: Value = serde_json::from_str(&result).map_err(|e| e.to_string())?;
        if let Some(errs) = out["errors"].as_array().filter(|e| !e.is_empty()) {
            return Err(errs.iter().filter_map(|e| e.as_str()).collect::<Vec<_>>().join("\n"));
        }
        if let (Some((path, saved)), Some(now)) = (&mut self.storage, out["storage"].as_str())
            && now != saved
        {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = crate::files::write_atomic(path, now.as_bytes());
            *saved = now.to_string();
        }
        let ids = apply(doc, page, &out);
        let strings = |k: &str| out[k].as_array().map(|a| a.iter().filter_map(|n| n.as_str().map(String::from)).collect()).unwrap_or_default();
        let ui = out["ui"].as_object().map(|u| UiRequest {
            html: u.get("html").and_then(|h| h.as_str()).unwrap_or("").to_string(),
            width: u.get("width").and_then(|w| w.as_f64()).unwrap_or(300.0) as f32,
            height: u.get("height").and_then(|h| h.as_f64()).unwrap_or(200.0) as f32,
            visible: u.get("visible").and_then(|v| v.as_bool()).unwrap_or(true),
        });
        let result = out["result"].as_str().map(|r| {
            let mut r = r.to_string();
            // Longest ids first, so "new:1" doesn't clip "new:12".
            let mut fresh: Vec<(&String, &NodeId)> = ids.iter().filter(|(k, _)| k.starts_with("new:")).collect();
            fresh.sort_by_key(|(k, _)| std::cmp::Reverse(k.len()));
            for (k, v) in fresh {
                r = r.replace(&format!("\"{k}\""), &format!("\"{v}\""));
            }
            r
        });
        Ok(Outcome {
            result,
            notes: strings("notes"),
            selection: out["selection"].as_array().into_iter().flatten().filter_map(|v| ids.get(v.as_str()?).copied()).collect(),
            outbox: strings("outbox"),
            ui,
            closed: out["closed"].as_bool().unwrap_or(false),
        })
    }
}

/// Run plugin code once, headless, and apply what it changed.
#[cfg(test)]
pub fn run(doc: &mut Document, page: usize, sel: &[NodeId], code: &str) -> Result<Outcome, String> {
    Session::start(doc, page, sel, "Script", None, code, None).map(|(_, out)| out)
}

fn apply(doc: &mut Document, page: usize, out: &Value) -> HashMap<String, NodeId> {
    let nodes = out["nodes"].as_array().cloned().unwrap_or_default();
    // Removed layers first.
    for id in out["removed"].as_array().into_iter().flatten().filter_map(|v| v.as_str()?.parse::<NodeId>().ok()) {
        if doc.nodes.contains_key(&id) {
            doc.remove(id);
        }
    }
    // Map JS ids to document ids, creating new layers.
    let mut ids: HashMap<String, NodeId> = HashMap::new();
    for v in &nodes {
        let jid = v["id"].as_str().unwrap_or_default().to_string();
        match jid.parse::<NodeId>().ok().filter(|i| doc.nodes.contains_key(i)) {
            Some(i) => {
                ids.insert(jid, i);
            }
            None => {
                let kind = kind_of(v["type"].as_str().unwrap_or("RECTANGLE"));
                let mut n = Node::new(kind, Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 100.0)));
                if v["type"] == "LINE" {
                    n = Node::vector(&[Default::default(), crate::doc::PathPoint { p: pos2(100.0, 0.0), ..Default::default() }], false);
                }
                n.name = v["name"].as_str().unwrap_or("").to_string();
                let id = doc.insert(page, None, None, n);
                ids.insert(jid, id);
            }
        }
    }
    // Structure: page order, then each container's children, top-down.
    let top: Vec<NodeId> = out["top"].as_array().into_iter().flatten().filter_map(|v| ids.get(v.as_str()?).copied()).collect();
    for (i, &id) in top.iter().enumerate() {
        if doc.node(id).parent.is_some() || doc.index_in_parent(id) != i {
            doc.reparent(id, page, None, Some(i));
        }
    }
    for v in &nodes {
        let Some(&id) = v["id"].as_str().and_then(|s| ids.get(s)) else { continue };
        if let Some(kids) = v["childIds"].as_array() {
            for (i, k) in kids.iter().enumerate() {
                if let Some(&c) = k.as_str().and_then(|s| ids.get(s)) {
                    if doc.node(c).parent != Some(id) || doc.index_in_parent(c) != i {
                        doc.reparent(c, page, Some(id), Some(i));
                    }
                }
            }
        }
    }
    // Properties, parents before children so relative positions resolve.
    let order: Vec<&Value> = {
        let mut o: Vec<&Value> = nodes.iter().collect();
        o.sort_by_key(|v| v["id"].as_str().and_then(|s| ids.get(s)).map_or(0, |&i| doc.ancestors(i).len()));
        o
    };
    for v in order {
        let Some(&id) = v["id"].as_str().and_then(|s| ids.get(s)) else { continue };
        let origin = doc.node(id).parent.map_or(egui::Pos2::ZERO, |p| doc.node(p).rect.min);
        let num = |k: &str| v[k].as_f64().map(|x| x as f32);
        let old = doc.node(id).clone();
        let (x, y) = (num("x").unwrap_or(old.rect.min.x - origin.x), num("y").unwrap_or(old.rect.min.y - origin.y));
        let (w, h) = (num("width").unwrap_or(old.rect.width()).max(0.0), num("height").unwrap_or(old.rect.height()).max(0.0));
        let r = Rect::from_min_size(pos2(origin.x + x, origin.y + y), vec2(w, h));
        if r.min != old.rect.min {
            doc.translate(id, r.min - old.rect.min);
        }
        if r.size() != old.rect.size() {
            let cur = doc.node(id).rect;
            doc.set_rect(id, Rect::from_min_size(cur.min, r.size()));
        }
        let n = doc.node_mut(id);
        n.name = v["name"].as_str().unwrap_or(&old.name).to_string();
        n.rotation = num("rotation").unwrap_or(old.rotation);
        n.opacity = num("opacity").unwrap_or(old.opacity).clamp(0.0, 1.0);
        n.visible = v["visible"].as_bool().unwrap_or(old.visible);
        n.locked = v["locked"].as_bool().unwrap_or(old.locked);
        n.stroke_width = num("strokeWeight").unwrap_or(old.stroke_width);
        n.radius = num("cornerRadius").unwrap_or(old.radius);
        if let Some(fs) = v["fills"].as_array() {
            n.fills = fs.iter().enumerate().filter_map(|(i, p)| paint_from(p, old.fills.get(i))).collect();
        }
        if let Some(ss) = v["strokes"].as_array() {
            n.strokes = ss.iter().enumerate().filter_map(|(i, p)| paint_from(p, old.strokes.get(i))).collect();
        }
        if let Some(t) = v["characters"].as_str() {
            n.text = t.to_string();
        }
        n.font_size = num("fontSize").unwrap_or(old.font_size);
        if let Some(f) = v.get("fontName") {
            n.font_family = f["family"].as_str().unwrap_or(&old.font_family).to_string();
            if let Some(st) = f["style"].as_str() {
                n.font_weight = [100, 200, 300, 400, 500, 600, 700, 800, 900].into_iter().find(|w| crate::text::weight_name(*w).eq_ignore_ascii_case(st)).unwrap_or(old.font_weight);
            }
        }
        if let Some(pc) = v["pointCount"].as_u64() {
            n.point_count = pc as u32;
        }
    }
    doc.reflow();
    ids
}

// ---------- installed plugins ----------

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Installed {
    pub name: String,
    pub manifest: std::path::PathBuf,
}

pub fn installed() -> Vec<Installed> {
    std::fs::read_to_string(crate::files::data_dir().join("plugins.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Register a plugin from its Figma-style `manifest.json`.
pub fn install(manifest: &std::path::Path) -> Result<Installed, String> {
    let m: Value = serde_json::from_str(&std::fs::read_to_string(manifest).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let name = m["name"].as_str().ok_or("manifest has no name")?.to_string();
    m["main"].as_str().ok_or("manifest has no main")?;
    let p = Installed { name, manifest: manifest.to_path_buf() };
    let mut all = installed();
    all.retain(|x| x.manifest != p.manifest);
    all.push(p.clone());
    crate::files::write_atomic(&crate::files::data_dir().join("plugins.json"), serde_json::to_string(&all).unwrap_or_default().as_bytes()).map_err(|e| e.to_string())?;
    Ok(p)
}

/// A plugin's code, its UI's HTML (manifest `ui`), and its id.
pub fn load(p: &Installed) -> Result<(String, Option<String>, String), String> {
    let m: Value = serde_json::from_str(&std::fs::read_to_string(&p.manifest).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let main = m["main"].as_str().ok_or("manifest has no main")?;
    let code = std::fs::read_to_string(p.manifest.with_file_name(main)).map_err(|e| e.to_string())?;
    // `ui` is a file, or (for multiple UIs) a map of names to files; the first one is `__html__`.
    let ui = m["ui"].as_str().or_else(|| m["ui"].as_object().and_then(|o| o.values().next()).and_then(|v| v.as_str()));
    let html = ui.and_then(|f| std::fs::read_to_string(p.manifest.with_file_name(f)).ok());
    Ok((code, html, m["id"].as_str().unwrap_or(&p.name).to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_api_edits_creates_and_removes() {
        let mut d = Document::default();
        let f = d.insert(0, None, None, Node::new(Kind::Frame, Rect::from_min_size(pos2(100., 100.), vec2(200., 200.))));
        let a = d.insert(0, Some(f), None, Node::new(Kind::Rectangle, Rect::from_min_size(pos2(110., 110.), vec2(20., 20.))));
        let b = d.insert(0, None, None, Node::new(Kind::Ellipse, Rect::from_min_size(pos2(0., 0.), vec2(10., 10.))));
        let code = r#"
            const sel = figma.currentPage.selection;
            for (const n of sel) { n.name = "Picked"; n.x += 5; n.fills = [{ type: "SOLID", color: { r: 1, g: 0, b: 0 } }]; }
            const frame = figma.currentPage.findOne(n => n.type === "FRAME");
            const r = figma.createRectangle();
            r.resize(30, 40);
            frame.appendChild(r);
            r.x = 50; r.y = 60;
            await figma.loadFontAsync({ family: "Inter", style: "Regular" });
            const t = figma.createText();
            t.characters = "Made by a plugin";
            figma.currentPage.findAll(n => n.type === "ELLIPSE").forEach(n => n.remove());
            figma.notify(`Done: ${figma.currentPage.findAll().length} layers`);
            figma.closePlugin();
        "#;
        let out = run(&mut d, 0, &[a], code).unwrap();
        assert_eq!(out.notes, vec!["Done: 4 layers".to_string()]);
        assert_eq!(d.node(a).name, "Picked");
        assert_eq!(d.node(a).rect.min, pos2(115., 110.), "x is relative to the parent frame");
        assert_eq!(d.node(a).fills[0].color, Rgba::rgb(255, 0, 0));
        assert!(!d.nodes.contains_key(&b), "removed");
        let kids = d.node(f).children.clone();
        assert_eq!(kids.len(), 2);
        let r = d.node(kids[1]);
        assert_eq!(r.rect, Rect::from_min_size(pos2(150., 160.), vec2(30., 40.)));
        assert!(d.nodes.values().any(|n| n.kind == Kind::Text && n.text == "Made by a plugin"));
        // Errors come back as messages, not crashes.
        let err0 = run(&mut d, 0, &[], "figma.notify(undefinedThing.x)").err().unwrap();
        assert!(err0.contains("undefinedThing"), "{err0}");
        let err = run(&mut d, 0, &[], "throw new Error('nope')").err().unwrap();
        assert!(err.contains("nope"), "{err}");
    }

    #[test]
    fn ui_plugins_keep_running_and_talk_both_ways() {
        let mut d = Document::default();
        let code = r#"
            figma.showUI(__html__, { width: 240, height: 120 });
            figma.ui.postMessage({ type: "hello", layers: figma.currentPage.children.length });
            figma.ui.onmessage = async (msg) => {
                if (msg.type === "create") {
                    for (let i = 0; i < msg.count; i++) { const r = figma.createRectangle(); r.x = i * 150; }
                    figma.ui.postMessage({ type: "made", count: msg.count });
                }
                if (msg.type === "done") figma.closePlugin("Bye");
            };
            figma.on("selectionchange", () => figma.ui.postMessage({ type: "sel", n: figma.currentPage.selection.length }));
        "#;
        let (mut s, out) = Session::start(&mut d, 0, &[], "Shapes", None, code, Some("<p>UI</p>")).unwrap();
        let ui = out.ui.unwrap();
        assert_eq!((ui.html.as_str(), ui.width, ui.height), ("<p>UI</p>", 240.0, 120.0));
        assert_eq!(out.outbox, vec![r#"{"type":"hello","layers":0}"#.to_string()]);
        let out = s.message(&mut d, 0, &[], r#"{"type":"create","count":3}"#).unwrap();
        assert_eq!(d.nodes.len(), 3, "the handler ran against the live document");
        assert_eq!(out.outbox, vec![r#"{"type":"made","count":3}"#.to_string()]);
        let first = *d.pages[0].children.first().unwrap();
        let out = s.emit(&mut d, 0, &[first], "selectionchange").unwrap().unwrap();
        assert_eq!(out.outbox, vec![r#"{"type":"sel","n":1}"#.to_string()]);
        assert!(s.emit(&mut d, 0, &[], "currentpagechange").unwrap().is_none(), "not listening");
        let out = s.message(&mut d, 0, &[], r#"{"type":"done"}"#).unwrap();
        assert!(out.closed && out.notes == vec!["Bye".to_string()]);
    }
}
