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

/// A running plugin: its JS runtime stays alive between events.
pub struct Session {
    rt: rquickjs::Runtime,
    ctx: rquickjs::Context,
    pub name: String,
    storage: Option<(std::path::PathBuf, String)>,
    all_pages: bool,
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
        // Other pages load only for small documents or plugins that ask for them (like Figma's dynamic pages).
        let all_pages = doc.nodes.len() < 20_000 || code.contains("loadAllPagesAsync") || code.contains("figma.root");
        let mut s = Session { rt, ctx, name: name.to_string(), storage, all_pages };
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
        let snap = crate::plugin_doc::snapshot(doc, page, sel, self.all_pages).to_string();
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
        let ids = crate::plugin_doc::apply(doc, &out);
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
    Session::start(doc, page, sel, "Script", None, code, None).map(|(_, out)| out)
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
            inst2.children[0].fills = [figma.util.solidPaint("#ff0000")];

            const a = figma.createComponent(); a.name = "State=Default";
            const b = figma.createComponent(); b.name = "State=Hover"; b.x = 200;
            const set = figma.combineAsVariants([a, b], figma.currentPage);
            set.name = "Chip";
            const chip = a.createInstance();
            chip.setProperties({ State: "Hover" });
            return { btn, card, inst, inst2, set, chip };
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
        assert_eq!(d.node(btn.children[0]).prop_bind, Some(("Label".to_string(), PropTarget::Text)));
        // Variable bindings resolve in the frame's explicit mode.
        let card = d.node(id("card"));
        assert!(card.bindings.contains(&(Bindable::Gap, bg.id + 1)) || card.bindings.iter().any(|(b, _)| *b == Bindable::Gap));
        assert_eq!(card.fills[0].color, Rgba::rgb(0, 0, 0), "dark mode");
        assert_eq!(card.layout.gap, 12.0);
        let title = d.node(card.children[0]);
        assert_eq!(title.font_size, 28.0, "text style applied");
        // Instances: property values and overrides on synced layers.
        let inst = d.node(id("inst"));
        assert_eq!((inst.kind, inst.component), (Kind::Instance, Some(id("btn"))));
        assert_eq!(inst.prop_values, vec![("Label".to_string(), PropValue::Text("Save".into()))]);
        assert_eq!(d.node(inst.children[0]).text, "Save");
        let inst2 = d.node(id("inst2"));
        assert_eq!(d.node(inst2.children[0]).fills[0].color, Rgba::rgb(255, 0, 0), "override kept");
        assert_eq!(d.node(inst2.children[0]).text, "Button");
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
}

