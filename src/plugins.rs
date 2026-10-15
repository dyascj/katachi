//! Plugins: run JavaScript written against Figma's plugin API (a core subset) with QuickJS.
//! A plugin session keeps its JS state alive; each event (start, a UI message, a selection change)
//! runs against a fresh JS copy of the current page, and the changes come back as one batch.

use crate::doc::{Document, NodeId};
use serde_json::Value;

/// The `figma` global (see plugin_api.js).
const PRELUDE: &str = include_str!("plugin_api.js");

pub struct Outcome {
    pub notes: Vec<String>,
    pub selection: Vec<NodeId>,
    /// Messages for the plugin's UI (JSON).
    pub outbox: Vec<String>,
    /// The UI the plugin wants shown, if any.
    pub ui: Option<UiRequest>,
    pub closed: bool,
    /// The page the plugin left current.
    pub page: usize,
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

/// `KATACHI_PROFILE=1`: time each phase of a plugin run on stderr.
pub fn lap(label: &str, t: &mut std::time::Instant) {
    if std::env::var_os("KATACHI_PROFILE").is_some() {
        eprintln!("  {label}: {:?}", t.elapsed());
    }
    *t = std::time::Instant::now();
}

/// How long one event's code may run before it's stopped (an endless loop would freeze the app).
const TIME_LIMIT: std::time::Duration = std::time::Duration::from_secs(if cfg!(test) { 3 } else { 10 });

/// A running plugin: its JS runtime stays alive between events.
pub struct Session {
    rt: rquickjs::Runtime,
    ctx: rquickjs::Context,
    pub name: String,
    storage: Option<(std::path::PathBuf, String)>,
    all_pages: bool,
    /// When the current event's code must stop; none between events.
    deadline: std::sync::Arc<std::sync::Mutex<Option<std::time::Instant>>>,
    /// Enabled libraries (id, file), for figma.importComponentByKeyAsync.
    libs: Vec<(String, std::sync::Arc<Document>)>,
}

impl Session {
    /// Start a plugin: define `figma` (and `__html__`), then run its main code.
    /// `id` names its clientStorage file; `html` is the manifest's UI file; `libs` are the
    /// document's enabled libraries.
    #[allow(clippy::too_many_arguments)]
    pub fn start(doc: &mut Document, page: usize, sel: &[NodeId], name: &str, id: Option<&str>, code: &str, html: Option<&str>, libs: Vec<(String, std::sync::Arc<Document>)>) -> Result<(Session, Outcome), String> {
        let rt = rquickjs::Runtime::new().map_err(|e| e.to_string())?;
        rt.set_memory_limit(512 << 20);
        let deadline: std::sync::Arc<std::sync::Mutex<Option<std::time::Instant>>> = Default::default();
        let d = deadline.clone();
        rt.set_interrupt_handler(Some(Box::new(move || d.lock().unwrap().is_some_and(|t| std::time::Instant::now() > t))));
        let ctx = rquickjs::Context::full(&rt).map_err(|e| e.to_string())?;
        let storage = id.map(|id| {
            let safe: String = id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
            let path = crate::files::data_dir().join("plugin-storage").join(format!("{safe}.json"));
            let saved = std::fs::read_to_string(&path).unwrap_or_else(|_| "{}".into());
            (path, saved)
        });
        // Other pages load only for small documents or plugins that ask for them (like Figma's dynamic pages).
        let all_pages = doc.nodes.len() < 20_000 || code.contains("loadAllPagesAsync") || code.contains("figma.root");
        let mut s = Session { rt, ctx, name: name.to_string(), storage, all_pages, deadline, libs };
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

    /// `run`, stopped if it takes longer than `TIME_LIMIT`. Plugins keep running across events.
    fn step(&mut self, doc: &mut Document, page: usize, sel: &[NodeId], js: &str) -> Result<Outcome, String> {
        *self.deadline.lock().unwrap() = Some(std::time::Instant::now() + TIME_LIMIT);
        let out = self.run(doc, page, sel, js);
        let late = self.deadline.lock().unwrap().take().is_some_and(|t| std::time::Instant::now() > t);
        if late && out.is_err() {
            return Err("Script took too long and was stopped".into());
        }
        out
    }

    /// Run pending promise jobs until none are left.
    fn settle(&self) -> Result<(), String> {
        for _ in 0..100_000 {
            match self.rt.execute_pending_job() {
                Ok(true) => continue,
                Ok(false) => break,
                Err(e) => return Err(format!("{e:?}")),
            }
        }
        Ok(())
    }

    /// Load the page into JS, run `js`, settle promises, and apply the changes.
    fn run(&mut self, doc: &mut Document, page: usize, sel: &[NodeId], js: &str) -> Result<Outcome, String> {
        let mut t = std::time::Instant::now();
        let snap = crate::plugin_doc::snapshot(doc, page, sel, self.all_pages).to_string();
        lap("snapshot", &mut t);
        self.eval(&format!("__reset(); __load({snap});"))?;
        self.eval(js)?;
        self.settle()?;
        // figma.importComponentByKeyAsync: copy the library components the script is waiting
        // on into the document, hand them over, and let it carry on.
        for _ in 0..8 {
            let keys: Vec<String> = self.ctx.with(|ctx| ctx.eval::<String, _>("__importKeys()")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
            if keys.is_empty() {
                break;
            }
            let roots: Vec<NodeId> = keys.iter().filter_map(|k| crate::library::import_key(doc, &self.libs, k)).collect();
            let trees: Vec<Value> = roots.into_iter().map(|root| crate::plugin_doc::tree(doc, root)).collect();
            self.eval(&format!("__resolveImports({});", Value::Array(trees)))?;
            self.settle()?;
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
        lap("run + collect", &mut t);
        let ids = crate::plugin_doc::apply(doc, &out);
        lap("apply", &mut t);
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
            page: out["current"].as_u64().map_or(page, |p| p as usize).min(doc.pages.len().saturating_sub(1)),
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
    Session::start(doc, page, sel, "Script", None, code, None, vec![]).map(|(_, out)| out)
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

/// Forget an installed plugin (its files stay where they are).
pub fn uninstall(manifest: &std::path::Path) {
    let mut all = installed();
    all.retain(|x| x.manifest != manifest);
    let _ = crate::files::write_atomic(&crate::files::data_dir().join("plugins.json"), serde_json::to_string(&all).unwrap_or_default().as_bytes());
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
    use crate::doc::{Kind, Node, Rgba};
    use egui::{Rect, pos2, vec2};

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
        let (mut s, out) = Session::start(&mut d, 0, &[], "Shapes", None, code, Some("<p>UI</p>"), vec![]).unwrap();
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

    #[test]
    fn endless_scripts_are_stopped_and_the_plugin_lives_on() {
        let mut d = Document::default();
        let err = run(&mut d, 0, &[], "while (true) {}").err().unwrap();
        assert_eq!(err, "Script took too long and was stopped");
        let err = run(&mut d, 0, &[], "await Promise.resolve(); for (;;) {}").err().unwrap();
        assert_eq!(err, "Script took too long and was stopped", "async code too");
        // The limit is per event: a UI plugin that sat idle longer than it still answers.
        let code = "figma.showUI(__html__); figma.ui.onmessage = () => figma.createRectangle();";
        let (mut s, _) = Session::start(&mut d, 0, &[], "Idle", None, code, Some(""), vec![]).unwrap();
        std::thread::sleep(TIME_LIMIT + std::time::Duration::from_millis(100));
        s.message(&mut d, 0, &[], "{}").unwrap();
        assert_eq!(d.nodes.len(), 1);
    }

    #[test]
    fn design_system_api_variables_styles_components_variants() {
        use crate::doc::{Bindable, LayoutMode, PropTarget, PropValue, Sizing, StyleValue, VarValue};
        let mut d = Document::default();
        let code = r##"
            const coll = figma.variables.createVariableCollection("Theme");
            coll.renameMode(coll.modes[0].modeId, "Light");
            const dark = coll.addMode("Dark");
            const bg = figma.variables.createVariable("bg/surface", coll, "COLOR");
            bg.setValueForMode(coll.defaultModeId, { r: 1, g: 1, b: 1, a: 1 });
            bg.setValueForMode(dark, { r: 0, g: 0, b: 0, a: 1 });
            const space = figma.variables.createVariable("space/md", coll.id, "FLOAT");
            space.setValueForMode(coll.defaultModeId, 12);
            space.setValueForMode(dark, 12);

            const h1 = figma.createTextStyle();
            h1.name = "Heading/H1"; h1.fontSize = 28; h1.fontName = { family: "Inter", style: "Bold" };
            const brand = figma.createPaintStyle();
            brand.name = "Brand/Primary"; brand.paints = [figma.util.solidPaint("#8E72FF")];

            const btn = figma.createComponent();
            btn.name = "Button";
            btn.layoutMode = "HORIZONTAL";
            btn.paddingLeft = btn.paddingRight = 16; btn.paddingTop = btn.paddingBottom = 8; btn.itemSpacing = 8;
            btn.primaryAxisSizingMode = "AUTO"; btn.counterAxisSizingMode = "AUTO";
            btn.cornerRadius = 8; btn.fillStyleId = brand.id;
            const label = figma.createText(); label.characters = "Button"; btn.appendChild(label);
            const prop = btn.addComponentProperty("Label", "TEXT", "Button");
            label.componentPropertyReferences = { characters: prop };

            const card = figma.createFrame();
            card.name = "Card"; card.layoutMode = "VERTICAL"; card.x = 300;
            card.fills = [figma.variables.setBoundVariableForPaint(figma.util.solidPaint("#ffffff"), "color", bg)];
            card.setBoundVariable("itemSpacing", space);
            card.setExplicitVariableModeForCollection(coll, dark);
            const title = figma.createText(); title.characters = "Title"; title.textStyleId = h1.id; card.appendChild(title);
            const inst = btn.createInstance(); card.appendChild(inst);
            inst.setProperties({ Label: "Save" });
            const inst2 = btn.createInstance(); card.appendChild(inst2);
            const big = btn.createInstance(); big.resize(300, 60); big.layoutPositioning = "ABSOLUTE";
            inst2.children[0].fills = [figma.util.solidPaint("#ff0000")];

            const a = figma.createComponent(); a.name = "State=Default";
            const b = figma.createComponent(); b.name = "State=Hover"; b.x = 200;
            const set = figma.combineAsVariants([a, b], figma.currentPage);
            set.name = "Chip";
            const chip = a.createInstance();
            chip.setProperties({ State: "Hover" });
            return { btn, card, inst, inst2, set, chip, big };
        "##;
        let out = run(&mut d, 0, &[], code).unwrap();
        let r: serde_json::Value = serde_json::from_str(out.result.as_deref().unwrap()).unwrap();
        let id = |k: &str| r[k]["id"].as_str().unwrap().parse::<NodeId>().unwrap();
        // Variables and styles.
        let theme = &d.collections[0];
        assert_eq!((theme.name.as_str(), theme.modes.clone()), ("Theme", vec!["Light".to_string(), "Dark".to_string()]));
        let bg = d.variables.iter().find(|v| v.name == "bg/surface").unwrap();
        assert_eq!(bg.values, vec![VarValue::Color(Rgba::rgb(255, 255, 255)), VarValue::Color(Rgba::rgb(0, 0, 0))]);
        let h1 = d.styles.iter().find(|s| s.name == "Heading/H1").unwrap();
        assert!(matches!(&h1.value, StyleValue::Text(t) if t.font_size == 28.0 && t.font_weight == 700));
        // The component: auto layout that hugs, linked to the paint style, with a text property.
        let btn = d.node(id("btn"));
        assert_eq!(btn.kind, Kind::Component);
        assert_eq!((btn.layout.mode, btn.layout.padding, btn.layout.gap, btn.sizing), (LayoutMode::Horizontal, [8., 16., 8., 16.], 8., [Sizing::Hug, Sizing::Hug]));
        assert_eq!(btn.fills[0].color, Rgba::rgb(0x8E, 0x72, 0xFF), "paint style applied");
        assert_eq!(btn.comp_props[0].name, "Label");
        assert_eq!(d.node(btn.children[0]).prop_bind, vec![("Label".to_string(), PropTarget::Text)]);
        // Variable bindings resolve in the frame's explicit mode.
        let card = d.node(id("card"));
        assert!(card.bindings.contains(&(Bindable::Gap, bg.id + 1)) || card.bindings.iter().any(|(b, _)| *b == Bindable::Gap));
        assert_eq!(card.fills[0].color, Rgba::rgb(0, 0, 0), "dark mode");
        assert_eq!(card.layout.gap, 12.0);
        let title = d.node(card.children[0]);
        assert_eq!(title.font_size, 28.0, "text style applied");
        let tid = card.children[0];
        run(&mut d, 0, &[], &format!("figma.getNodeById('{tid}').fontSize = 40;")).unwrap();
        assert_eq!((d.node(tid).font_size, d.node(tid).text_style), (40.0, None), "a manual size detaches the style");
        // Instances: property values and overrides on synced layers.
        let inst = d.node(id("inst"));
        assert_eq!((inst.kind, inst.component), (Kind::Instance, Some(id("btn"))));
        assert_eq!(inst.prop_values, vec![("Label".to_string(), PropValue::Text("Save".into()))]);
        assert_eq!(d.node(inst.children[0]).text, "Save");
        let inst2 = d.node(id("inst2"));
        assert_eq!(d.node(inst2.children[0]).fills[0].color, Rgba::rgb(255, 0, 0), "override kept");
        assert_eq!(d.node(inst2.children[0]).text, "Button");
        assert_eq!(d.node(id("big")).rect.size(), vec2(300., 60.), "a new instance keeps the size it was given");
        // Variants.
        let set = d.node(id("set"));
        assert_eq!((set.kind, set.name.as_str(), set.children.len()), (Kind::ComponentSet, "Chip", 2));
        let hover = set.children.iter().copied().find(|c| d.node(*c).name == "State=Hover").unwrap();
        assert_eq!(d.node(id("chip")).component, Some(hover), "setProperties switched the variant");
        // A second run sees everything it made.
        let out = run(&mut d, 0, &[], r#"
            const v = figma.variables.getLocalVariables("COLOR")[0];
            const s = figma.getLocalTextStyles()[0];
            const btn = figma.currentPage.findOne(n => n.type === "COMPONENT" && n.name === "Button");
            return [v.name, s.fontSize, btn.componentPropertyDefinitions.Label.defaultValue, btn.layoutMode, btn.primaryAxisSizingMode];
        "#).unwrap();
        assert_eq!(out.result.as_deref(), Some(r#"["bg/surface",28,"Button","HORIZONTAL","AUTO"]"#));
    }

    #[test]
    fn pages_hold_components_used_elsewhere() {
        let mut d = Document::default();
        let out = run(&mut d, 0, &[], r#"
            const home = figma.currentPage;
            home.name = "Screens";
            const comps = figma.createPage();
            comps.name = "Components";
            await figma.setCurrentPageAsync(comps);
            const c = figma.createComponent(); c.name = "Btn";
            await figma.setCurrentPageAsync(home);
            const i = c.createInstance(); i.x = 50;
            figma.currentPage = comps;
            return { pages: figma.root.children.map(p => p.name), c, i };
        "#).unwrap();
        let names: Vec<&str> = d.pages.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["Screens", "Components"]);
        let r: serde_json::Value = serde_json::from_str(out.result.as_deref().unwrap()).unwrap();
        let id = |k: &str| r[k]["id"].as_str().unwrap().parse::<NodeId>().unwrap();
        assert_eq!((d.page_of(id("c")), d.page_of(id("i"))), (1, 0));
        assert_eq!(d.node(id("i")).component, Some(id("c")));
        assert_eq!(out.page, 1, "the script left Components current");
        // The next run sees both pages.
        let out = run(&mut d, 1, &[], "return figma.root.findAll(n => n.type === 'INSTANCE').length").unwrap();
        assert_eq!(out.result.as_deref(), Some("1"));
    }

    #[test]
    fn svg_layers_become_components() {
        let mut d = Document::default();
        let out = run(&mut d, 0, &[], r##"
            const f = figma.createNodeFromSvg('<svg width="24" height="24" viewBox="0 0 24 24"><path d="M6 6l12 12" stroke="#000" stroke-width="2"/></svg>');
            const c = figma.createComponentFromNode(f);
            c.name = "Icon/x";
            return c;
        "##).unwrap();
        let r: serde_json::Value = serde_json::from_str(out.result.as_deref().unwrap()).unwrap();
        let c = d.node(r["id"].as_str().unwrap().parse().unwrap());
        assert_eq!((c.kind, c.name.as_str()), (Kind::Component, "Icon/x"));
        assert_eq!(c.rect.size(), vec2(24., 24.));
        assert!(!c.children.is_empty());
    }

    #[test]
    fn untouched_hugging_frames_stay_hugging() {
        let mut d = Document::default();
        run(&mut d, 0, &[], r#"
            const b = figma.createFrame(); b.name = "Board"; b.layoutMode = "VERTICAL";
            b.primaryAxisSizingMode = "AUTO"; b.counterAxisSizingMode = "AUTO";
            const a = figma.createComponent(); a.name = "State=A"; a.resize(40, 40);
            const c = figma.createComponent(); c.name = "State=B"; c.resize(40, 40); c.x = 60;
            const set = figma.combineAsVariants([a, c], b);
        "#).unwrap();
        run(&mut d, 0, &[], r#"
            const set = figma.currentPage.findOne(n => n.type === "COMPONENT_SET");
            set.resize(200, 120);
        "#).unwrap();
        let b = d.nodes.values().find(|n| n.name == "Board").unwrap();
        assert_eq!(b.sizing, [crate::doc::Sizing::Hug; 2], "{:?}", b.rect);
    }

    #[test]
    fn nested_instances_keep_overrides_and_swap_cleanly() {
        let mut d = Document::default();
        let out = run(&mut d, 0, &[], r#"
            const a = figma.createComponent(); a.name = "Icon/a"; a.resize(24, 24);
            const ra = figma.createRectangle(); ra.name = "A shape"; a.appendChild(ra); ra.resize(24, 24);
            ra.constraints = { horizontal: "SCALE", vertical: "SCALE" };
            const b = figma.createComponent(); b.name = "Icon/b"; b.resize(24, 24); b.x = 50;
            const eb = figma.createEllipse(); eb.name = "B shape"; b.appendChild(eb); eb.resize(24, 24);
            eb.constraints = { horizontal: "SCALE", vertical: "SCALE" };
            const btn = figma.createComponent(); btn.name = "Button"; btn.x = 100; btn.resize(80, 30);
            const ic = a.createInstance(); btn.appendChild(ic); ic.resize(16, 16); ic.name = "Icon"; ic.visible = false;
            btn.addComponentProperty("Show icon", "BOOLEAN", false);
            btn.addComponentProperty("Icon", "INSTANCE_SWAP", a.id);
            ic.componentPropertyReferences = { visible: "Show icon", mainComponent: "Icon" };
            const plain = btn.createInstance(); plain.y = 100;
            const swapped = btn.createInstance(); swapped.y = 200;
            swapped.setProperties({ "Show icon": true, Icon: b.id });
            return { plain, swapped, b };
        "#).unwrap();
        let r: serde_json::Value = serde_json::from_str(out.result.as_deref().unwrap()).unwrap();
        let id = |k: &str| r[k]["id"].as_str().unwrap().parse::<NodeId>().unwrap();
        let icon_of = |d: &Document, i: NodeId| d.node(d.node(i).children[0]).clone();
        let plain = icon_of(&d, id("plain"));
        assert!(!plain.visible, "Show icon defaults off in instances");
        assert_eq!(plain.rect.size(), vec2(16., 16.));
        let sw = icon_of(&d, id("swapped"));
        assert!(sw.visible);
        assert_eq!(sw.component, Some(id("b")));
        assert_eq!(sw.rect.size(), vec2(16., 16.), "a swap keeps the instance's size");
        assert_eq!(d.node(sw.children[0]).name, "B shape");
        assert_eq!(d.node(sw.children[0]).rect.size(), vec2(16., 16.), "swapped layers scale to fit");
        // Moving the outer instance carries the swapped icon's layers along.
        let before = d.node(sw.children[0]).rect.min - sw.rect.min;
        d.translate(id("swapped"), vec2(37., 11.));
        let btn = d.nodes.values().find(|n| n.name == "Button" && n.kind == Kind::Component).unwrap().id;
        d.node_mut(btn).fills.clear();
        d.reflow();
        let sw = icon_of(&d, id("swapped"));
        assert_eq!(d.node(sw.children[0]).rect.min - sw.rect.min, before, "swapped layers stay inside their instance");
        let count = d.nodes.len();
        d.reflow();
        d.reflow();
        assert_eq!(d.nodes.len(), count, "no layers churn between reflows");
        assert_eq!(icon_of(&d, id("swapped")).component, Some(id("b")));
    }

    #[test]
    fn resized_instances_scale_vectors_once_and_nested_swaps_stick() {
        let mut d = Document::default();
        // SVG layers exist after the run that imports them.
        run(&mut d, 0, &[], r##"
            const ic = figma.createComponentFromNode(figma.createNodeFromSvg('<svg width="24" height="24" viewBox="0 0 24 24"><path d="M4 4h16v16H4z" stroke="#000" fill="none"/></svg>'));
            ic.name = "Icon/box";
        "##).unwrap();
        let out = run(&mut d, 0, &[], r##"
            const ic = figma.currentPage.findOne(n => n.name === "Icon/box");
            for (const v of ic.findAll(() => true)) v.constraints = { horizontal: "SCALE", vertical: "SCALE" };
            const g = figma.createComponent(); g.name = "Brand/g"; g.resize(18, 18); g.x = 40;
            const m = figma.createComponent(); m.name = "Brand/m"; m.resize(18, 18); m.x = 80;
            const sso = figma.createComponent(); sso.name = "SSO"; sso.x = 120; sso.resize(200, 40);
            const logo = g.createInstance(); sso.appendChild(logo); logo.name = "Logo";
            const big = ic.createInstance(); big.resize(48, 48); big.y = 100;
            const b2 = sso.createInstance(); b2.y = 200;
            b2.findOne(n => n.name === "Logo").swapComponent(m);
            return { big, b2, m };
        "##).unwrap();
        let r: serde_json::Value = serde_json::from_str(out.result.as_deref().unwrap()).unwrap();
        let id = |k: &str| r[k]["id"].as_str().unwrap().parse::<NodeId>().unwrap();
        let path_width = |d: &Document| {
            let v = d.node(d.node(id("big")).children[0]);
            let xs: Vec<f32> = v.paths.iter().flat_map(|p| p.points.iter().map(|pt| pt.p.x)).collect();
            xs.iter().cloned().fold(f32::MIN, f32::max) - xs.iter().cloned().fold(f32::MAX, f32::min)
        };
        let w = path_width(&d);
        assert!((w - 32.).abs() < 0.5, "16px square drawn at 2x: {w}");
        // Reopening the file resyncs every instance (and so does editing the component): the scale must not compound.
        for i in 0..3 {
            d = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
            let c = d.nodes.values().find(|n| n.name == "Icon/box").unwrap().id;
            d.node_mut(c).opacity -= 0.1 * i as f32;
            d.reflow();
        }
        assert!((path_width(&d) - 32.).abs() < 0.5, "{}", path_width(&d));
        let logo = d.node(d.node(id("b2")).children[0]);
        assert_eq!(logo.component, Some(id("m")), "a swapped nested instance stays swapped");
    }
}

#[cfg(test)]
mod prototype_tests {
    use super::*;

    #[test]
    fn reactions_and_flows_through_the_api() {
        let mut d = Document::default();
        let out = run(&mut d, 0, &[], r#"
            const a = figma.createFrame(); a.name = "Login";
            const b = figma.createFrame(); b.name = "Dashboard"; b.x = 200;
            const btn = figma.createRectangle(); a.appendChild(btn);
            await btn.setReactionsAsync([{ trigger: { type: "ON_CLICK" }, actions: [{ type: "NODE", destinationId: b.id, navigation: "NAVIGATE", transition: { type: "DISSOLVE", duration: 0.2, easing: { type: "EASE_OUT" } } }] }]);
            b.reactions = [{ trigger: { type: "AFTER_TIMEOUT", timeout: 1.5 }, actions: [{ type: "BACK" }] }];
            figma.currentPage.flowStartingPoints = [{ nodeId: a.id, name: "Sign in" }];
            return { a, b, btn };
        "#).unwrap();
        let r: serde_json::Value = serde_json::from_str(out.result.as_deref().unwrap()).unwrap();
        let id = |k: &str| r[k]["id"].as_str().unwrap().parse::<NodeId>().unwrap();
        let rx = &d.node(id("btn")).reactions[0];
        assert_eq!(rx.action, crate::doc::Action::Navigate(id("b")));
        assert_eq!(rx.transition.kind, crate::doc::TransitionKind::Dissolve);
        assert_eq!(d.node(id("b")).reactions[0].trigger, crate::doc::Trigger::AfterDelay(1500));
        assert_eq!(d.pages[0].flows, vec![id("a")]);
        let back = run(&mut d, 0, &[], "return figma.currentPage.findOne(n => n.type === 'RECTANGLE').reactions[0].actions[0].navigation").unwrap();
        assert_eq!(back.result.as_deref(), Some("\"NAVIGATE\""));
    }
}

/// `KATACHI_DOC=file.katachi KATACHI_JS=script.js cargo test --release plugin_bench -- --ignored --nocapture`
#[cfg(test)]
#[test]
#[ignore]
fn plugin_bench() {
    let (Some(doc), Some(js)) = (std::env::var_os("KATACHI_DOC"), std::env::var_os("KATACHI_JS")) else { return };
    let mut d = crate::doc::parse_file(&std::fs::read_to_string(doc).unwrap()).unwrap();
    let t = std::time::Instant::now();
    d.reflow();
    eprintln!("load reflow: {:?} ({} nodes)", t.elapsed(), d.nodes.len());
    let t = std::time::Instant::now();
    let r = run(&mut d, 0, &[], &std::fs::read_to_string(js).unwrap());
    eprintln!("run: {:?} -> {:?} ({} nodes)", t.elapsed(), r.map(|o| o.result), d.nodes.len());
}
