//! What each MCP tool does, run on the UI thread against the open document.

use crate::app::App;
use crate::doc::{Document, Kind, NodeId, VarValue};
use crate::mcp::{fail, image, text};
use serde_json::{Value, json};
use std::fmt::Write;

pub fn kind_tag(k: Kind) -> &'static str {
    match k {
        Kind::Frame => "frame",
        Kind::Group => "group",
        Kind::Rectangle => "rectangle",
        Kind::Ellipse => "ellipse",
        Kind::Vector => "vector",
        Kind::Text => "text",
        Kind::Component => "component",
        Kind::Instance => "instance",
        Kind::ComponentSet => "component-set",
        Kind::Boolean => "boolean-operation",
        Kind::Polygon => "polygon",
        Kind::Star => "star",
        Kind::Section => "section",
        Kind::Slice => "slice",
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Figma-style XML outline: positions relative to the parent.
pub fn metadata(doc: &Document, ids: &[NodeId], limit: usize) -> String {
    fn walk(doc: &Document, id: NodeId, depth: usize, out: &mut String, count: &mut usize, limit: usize) {
        if *count >= limit {
            return;
        }
        *count += 1;
        let n = doc.node(id);
        let origin = n.parent.map_or(egui::Pos2::ZERO, |p| doc.node(p).rect.min);
        let pad = "  ".repeat(depth);
        let r = n.rect;
        let num = crate::canvas::num;
        let _ = write!(
            out,
            r#"{pad}<{} id="{id}" name="{}" x="{}" y="{}" width="{}" height="{}""#,
            kind_tag(n.kind),
            esc(&n.name),
            num(r.min.x - origin.x),
            num(r.min.y - origin.y),
            num(r.width()),
            num(r.height())
        );
        if !n.visible {
            out.push_str(r#" hidden="true""#);
        }
        if n.layout.mode != crate::doc::LayoutMode::None {
            let _ = write!(out, r#" layout="{:?}""#, n.layout.mode);
        }
        if let Some(c) = n.component.and_then(|c| doc.nodes.get(&c)) {
            let _ = write!(out, r#" component="{}""#, esc(&c.name));
        }
        if n.kind == Kind::Text {
            let t: String = n.text.chars().take(200).collect();
            let _ = writeln!(out, ">{}</{}>", esc(&t), kind_tag(n.kind));
        } else if n.children.is_empty() {
            out.push_str(" />\n");
        } else {
            out.push_str(">\n");
            for &c in &n.children {
                walk(doc, c, depth + 1, out, count, limit);
            }
            let _ = writeln!(out, "{pad}</{}>", kind_tag(n.kind));
        }
    }
    let mut out = String::new();
    let mut count = 0;
    for &id in ids {
        walk(doc, id, 0, &mut out, &mut count, limit);
    }
    if count >= limit {
        let _ = writeln!(out, "<!-- stopped after {limit} layers; ask for a node_id to see more -->");
    }
    out
}

fn var_json(v: &VarValue) -> Value {
    match v {
        VarValue::Color(c) => json!(format!("#{}{}", c.hex(), if c.a() == 255 { String::new() } else { format!("{:02X}", c.a()) })),
        VarValue::Number(n) => json!(n),
        VarValue::Text(t) => json!(t),
        VarValue::Bool(b) => json!(b),
        VarValue::Alias(a) => json!({ "alias": a.to_string() }),
    }
}

impl App {
    /// Layers a tool works on: `node_id`, else the selection, else the page.
    fn mcp_targets(&self, args: &Value) -> Result<Vec<NodeId>, String> {
        if let Some(s) = args["node_id"].as_str().filter(|s| !s.is_empty()) {
            let id: NodeId = s.parse().map_err(|_| format!("bad node id {s}"))?;
            return if self.doc.nodes.contains_key(&id) { Ok(vec![id]) } else { Err(format!("no layer {s}")) };
        }
        let roots = self.sel_roots();
        Ok(if roots.is_empty() { self.doc.pages[self.page].children.clone() } else { roots })
    }

    pub fn mcp_tool(&mut self, ctx: &egui::Context, name: &str, args: &Value) -> Value {
        // What an agent changes glows on the canvas for a moment.
        let before = matches!(name, "execute_code" | "import_svg" | "place_image" | "undo").then(|| self.doc.nodes.clone());
        let out = match self.mcp_tool_inner(ctx, name, args) {
            Ok(v) => v,
            Err(e) => fail(e),
        };
        if let Some(before) = before {
            self.mark_agent_changes(&before, ctx.input(|i| i.time));
        }
        out
    }

    fn mcp_tool_inner(&mut self, ctx: &egui::Context, name: &str, args: &Value) -> Result<Value, String> {
        Ok(match name {
            "get_metadata" => {
                let ids = self.mcp_targets(args)?;
                let page = &self.doc.pages[self.page].name;
                text(format!("<!-- page \"{}\" -->\n{}", esc(page), metadata(&self.doc, &ids, 3000)))
            }
            "get_selection" => {
                let sel: Vec<Value> = self.sel.iter().map(|&id| {
                    let n = self.doc.node(id);
                    json!({ "id": id.to_string(), "type": kind_tag(n.kind), "name": n.name })
                }).collect();
                text(Value::Array(sel).to_string())
            }
            "get_screenshot" => {
                let ids = self.mcp_targets(args)?;
                if ids.is_empty() {
                    return Err("nothing to show: the page is empty".into());
                }
                let mut t = std::time::Instant::now();
                self.fit_all_texts(ctx);
                crate::plugins::lap("fit texts", &mut t);
                let b = self.doc.bounds(&ids);
                // Keep images a reasonable size for a model to look at.
                let want = args["scale"].as_f64().unwrap_or(1.0) as f32;
                let scale = want.min(4096.0 / b.width().max(b.height()).max(1.0)).max(0.01);
                let svg = crate::export::to_svg(&self.doc, &ids);
                let png = crate::export::encode(&self.doc, &svg, crate::doc::ExportFormat::Png, scale)?;
                image(&png)
            }
            "get_code" => {
                let ids = self.mcp_targets(args)?;
                let lang = match args["language"].as_str() {
                    Some("swiftui") => crate::codegen::Lang::SwiftUi,
                    Some("compose") => crate::codegen::Lang::Compose,
                    Some("tailwind") => crate::codegen::Lang::Tailwind,
                    _ => crate::codegen::Lang::Css,
                };
                let mut out = String::new();
                let mut count = 0;
                for &root in &ids {
                    for id in std::iter::once(root).chain(self.doc.descendants(root)) {
                        count += 1;
                        if count > 400 {
                            break;
                        }
                        let n = self.doc.node(id);
                        let _ = writeln!(out, "// {} ({}, {})\n{}\n", n.name, kind_tag(n.kind), id, crate::codegen::generate(&self.doc, id, lang));
                    }
                }
                text(out)
            }
            "get_variable_defs" => {
                let d = &self.doc;
                let collections: Vec<Value> = d.collections.iter().map(|c| {
                    let vars: Vec<Value> = d.variables.iter().filter(|v| v.collection == c.id).map(|v| {
                        let values: serde_json::Map<String, Value> = c.modes.iter().zip(&v.values).map(|(m, val)| (m.clone(), var_json(val))).collect();
                        json!({ "id": v.id.to_string(), "name": v.name, "values": values })
                    }).collect();
                    json!({ "id": c.id.to_string(), "name": c.name, "modes": c.modes, "variables": vars })
                }).collect();
                let styles: Vec<Value> = d.styles.iter().map(|s| json!({ "id": s.id.to_string(), "name": s.name, "value": format!("{:?}", s.value) })).collect();
                text(json!({ "collections": collections, "styles": styles }).to_string())
            }
            "get_components" => {
                let d = &self.doc;
                let mut out = vec![];
                for (pi, p) in d.pages.iter().enumerate() {
                    for id in p.children.iter().flat_map(|&r| std::iter::once(r).chain(d.descendants(r))) {
                        let n = d.node(id);
                        match n.kind {
                            Kind::ComponentSet => out.push(json!({
                                "id": id.to_string(), "type": "COMPONENT_SET", "name": n.name, "page": pi,
                                "variants": n.children.iter().map(|c| json!({ "id": c.to_string(), "name": d.node(*c).name })).collect::<Vec<_>>(),
                                "properties": n.comp_props.iter().map(|p| json!({ "name": p.name, "default": format!("{:?}", p.default) })).collect::<Vec<_>>(),
                            })),
                            Kind::Component if !n.parent.is_some_and(|p| d.node(p).kind == Kind::ComponentSet) => out.push(json!({
                                "id": id.to_string(), "type": "COMPONENT", "name": n.name, "page": pi,
                                "width": n.rect.width(), "height": n.rect.height(),
                                "properties": n.comp_props.iter().map(|p| json!({ "name": p.name, "default": format!("{:?}", p.default) })).collect::<Vec<_>>(),
                            })),
                            _ => {}
                        }
                    }
                }
                // Enabled libraries' components; execute_code places one with
                // `(await figma.importComponentByKeyAsync(key)).createInstance()`.
                for (r, lib) in self.enabled_libs() {
                    for (page, id, name) in crate::library::components(&lib) {
                        let n = lib.node(id);
                        out.push(json!({
                            "key": format!("{}:{id}", r.id), "type": "COMPONENT", "name": name, "library": r.name, "page": lib.pages[page].name,
                            "width": n.rect.width(), "height": n.rect.height(),
                        }));
                    }
                }
                text(Value::Array(out).to_string())
            }
            "execute_code" => {
                let code = args["code"].as_str().ok_or("missing code")?;
                let before = self.doc.clone();
                let (sel, libs) = (self.sel.clone(), self.plugin_libs());
                let out = match crate::plugins::Session::start(&mut self.doc, self.page, &sel, "MCP", None, code, None, libs) {
                    Ok((_, out)) => out,
                    Err(e) => {
                        self.doc = before;
                        return Err(e);
                    }
                };
                self.keep_changes(before);
                if out.page != self.page {
                    self.select_page(out.page);
                }
                self.sel = out.selection.clone();
                self.fit_all_texts(ctx);
                let mut msg = String::new();
                if let Some(r) = &out.result {
                    msg.push_str(r);
                }
                for n in &out.notes {
                    let _ = write!(msg, "\nnotify: {n}");
                }
                text(if msg.is_empty() { "Done".into() } else { msg.trim().to_string() })
            }
            "list_pages" => text(json!({
                "current": self.page,
                "pages": self.doc.pages.iter().enumerate().map(|(i, p)| json!({ "index": i, "name": p.name, "layers": p.children.len() })).collect::<Vec<_>>(),
            }).to_string()),
            "set_page" => {
                let want = &args["page"];
                let found = match want {
                    Value::Number(n) => n.as_u64().map(|i| i as usize).filter(|&i| i < self.doc.pages.len()),
                    Value::String(s) => self.doc.pages.iter().position(|p| p.name == *s),
                    _ => None,
                };
                let i = match (found, want.as_str(), args["create"].as_bool()) {
                    (Some(i), _, _) => i,
                    (None, Some(name), Some(true)) => {
                        self.checkpoint();
                        self.doc.pages.push(crate::doc::Page { name: name.to_string(), children: vec![], background: crate::doc::PAGE_BG, flows: vec![], view: None, guides: vec![], prototype: Default::default() });
                        self.doc.pages.len() - 1
                    }
                    _ => return Err(format!("no page {want}")),
                };
                self.select_page(i);
                text(format!("Current page: {} ({i})", self.doc.pages[i].name))
            }
            "select" => {
                let ids: Vec<NodeId> = args["ids"].as_array().into_iter().flatten().filter_map(|v| v.as_str()?.parse().ok()).filter(|id| self.doc.nodes.contains_key(id)).collect();
                if let Some(&first) = ids.first() {
                    let page = self.doc.page_of(first);
                    self.select_page(page);
                }
                self.sel = ids.clone();
                if args["zoom"].as_bool().unwrap_or(false) && !ids.is_empty() {
                    self.zoom_to(ctx, Some(ids.clone()), false);
                }
                text(format!("Selected {} layers", ids.len()))
            }
            "import_svg" => {
                let svg = args["svg"].as_str().ok_or("missing svg")?;
                let at = egui::pos2(args["x"].as_f64().unwrap_or(0.0) as f32, args["y"].as_f64().unwrap_or(0.0) as f32);
                let id = self.import_svg(svg.as_bytes(), args["name"].as_str().unwrap_or("Vector"), at, false).ok_or("couldn't read the SVG")?;
                self.mcp_reparent(id, args)?;
                text(json!({ "id": id.to_string() }).to_string())
            }
            "place_image" => {
                let path = args["path"].as_str().ok_or("missing path")?;
                let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
                let name = std::path::Path::new(path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                let id = self.place_image(bytes, &name, egui::Pos2::ZERO).ok_or("not a PNG or JPEG")?;
                let r = self.doc.node(id).rect;
                let size = egui::vec2(args["width"].as_f64().map_or(r.width(), |w| w as f32), args["height"].as_f64().map_or(r.height(), |h| h as f32));
                let at = egui::pos2(args["x"].as_f64().unwrap_or(0.0) as f32, args["y"].as_f64().unwrap_or(0.0) as f32);
                self.doc.set_rect(id, egui::Rect::from_min_size(at, size));
                self.mcp_reparent(id, args)?;
                text(json!({ "id": id.to_string() }).to_string())
            }
            "save" => {
                let ok = match args["path"].as_str() {
                    Some(p) => self.save_to(p.into()),
                    None if self.path.is_some() => self.save(false),
                    None => return Err("the document has no file yet; pass a path".into()),
                };
                if !ok {
                    return Err("couldn't save".into());
                }
                text(format!("Saved {}", self.path.as_ref().map(|p| p.display().to_string()).unwrap_or_default()))
            }
            "open_file" => {
                if self.dirty {
                    return Err("there are unsaved changes; save first".into());
                }
                let path = args["path"].as_str().ok_or("missing path")?;
                if !self.open_path(path.into()) {
                    return Err(format!("couldn't open {path}"));
                }
                text(format!("Opened {path}: {} pages", self.doc.pages.len()))
            }
            "undo" => {
                if self.undo_len() == 0 || self.drag.is_some() {
                    return Err("nothing to undo".into());
                }
                self.undo();
                text("Undone")
            }
            "find_nodes" => {
                let roots = match args["node_id"].as_str() {
                    Some(_) => self.mcp_targets(args)?,
                    None => self.doc.pages[self.page].children.clone(),
                };
                let (name, txt, kind) = (args["name"].as_str(), args["text"].as_str(), args["type"].as_str());
                if name.is_none() && txt.is_none() && kind.is_none() {
                    return Err("give a name, text, or type to look for".into());
                }
                let limit = args["limit"].as_u64().unwrap_or(100) as usize;
                let d = &self.doc;
                let found: Vec<Value> = roots
                    .iter()
                    .flat_map(|&r| std::iter::once(r).chain(d.descendants(r)))
                    .map(|id| d.node(id))
                    .filter(|n| name.is_none_or(|p| glob(p, &n.name)) && txt.is_none_or(|p| n.kind == Kind::Text && glob(p, &n.text)) && kind.is_none_or(|k| k.eq_ignore_ascii_case(kind_tag(n.kind))))
                    .take(limit)
                    .map(|n| {
                        let mut v = json!({ "id": n.id.to_string(), "type": kind_tag(n.kind), "name": n.name, "x": n.rect.min.x, "y": n.rect.min.y, "width": n.rect.width(), "height": n.rect.height() });
                        if n.kind == Kind::Text {
                            v["text"] = n.text.chars().take(120).collect::<String>().into();
                        }
                        v
                    })
                    .collect();
                text(Value::Array(found).to_string())
            }
            "export" => {
                let dir = std::path::PathBuf::from(args["dir"].as_str().ok_or("missing dir")?);
                std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                let ids: Vec<NodeId> = match args["node_ids"].as_array() {
                    Some(list) => list.iter().map(|v| v.as_str().and_then(|s| s.parse().ok()).filter(|id| self.doc.nodes.contains_key(id)).ok_or(format!("no layer {v}"))).collect::<Result<_, _>>()?,
                    None => self.sel_roots(),
                };
                self.fit_all_texts(ctx);
                let mut jobs = if ids.is_empty() {
                    let page = &self.doc.pages[self.page];
                    vec![(page.children.clone(), page.name.clone(), self.export_default.clone())]
                } else {
                    self.export_jobs(&ids)
                };
                // A format or size given here replaces the layers' own settings.
                if args["format"].is_string() || args["size"].is_string() {
                    let mut preset = crate::doc::ExportSetting::default();
                    if let Some(f) = args["format"].as_str() {
                        preset.format = crate::doc::ExportFormat::ALL.into_iter().find(|x| x.ext() == f).ok_or(format!("unknown format {f}"))?;
                    }
                    if let Some(sz) = args["size"].as_str()
                        && !preset.set_size(sz)
                    {
                        return Err(format!("bad size {sz}; use 2x, 512w, or 512h"));
                    }
                    jobs.dedup_by(|a, b| a.0 == b.0);
                    jobs.iter_mut().for_each(|j| j.2 = preset.clone());
                }
                let names = crate::export::unique_names(jobs.iter().map(|(_, name, p)| crate::app::export_file_name(name, p)).collect());
                let targets: Vec<std::path::PathBuf> = names.into_iter().map(|f| dir.join(f)).collect();
                self.render_exports(&jobs, &targets)?;
                text(json!({ "files": targets.iter().map(|p| p.display().to_string()).collect::<Vec<_>>() }).to_string())
            }
            "get_comments" => {
                let (all, resolved) = (args["all_pages"].as_bool().unwrap_or(false), args["include_resolved"].as_bool().unwrap_or(false));
                let threads: Vec<Value> = self
                    .doc
                    .comments
                    .iter()
                    .filter(|c| (all || c.page == self.page) && (resolved || !c.resolved))
                    .map(|c| {
                        let anchor = c.anchor.filter(|a| self.doc.nodes.contains_key(a)).map(|a| json!({ "id": a.to_string(), "name": self.doc.node(a).name }));
                        json!({
                            "id": c.id.to_string(), "page": self.doc.pages.get(c.page).map(|p| p.name.as_str()), "resolved": c.resolved, "layer": anchor,
                            "messages": c.messages.iter().map(|(who, what, _)| json!({ "author": who, "text": what })).collect::<Vec<_>>(),
                        })
                    })
                    .collect();
                text(Value::Array(threads).to_string())
            }
            "reply_to_comment" => {
                let id: u64 = args["comment_id"].as_str().and_then(|s| s.parse().ok()).ok_or("bad comment_id")?;
                let reply = args["text"].as_str().map(str::trim).filter(|t| !t.is_empty());
                let resolve = args["resolve"].as_bool().unwrap_or(false);
                let i = self.doc.comments.iter().position(|c| c.id == id).ok_or(format!("no comment {id}"))?;
                if reply.is_none() && !resolve {
                    return Err("give text, or resolve: true".into());
                }
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
                let c = &mut self.doc.comments[i];
                if let Some(t) = reply {
                    c.messages.push(("Agent".into(), t.to_string(), now));
                }
                c.resolved |= resolve;
                self.dirty = true;
                text(if resolve { "Replied and resolved" } else { "Replied" })
            }
            other => return Err(format!("unknown tool {other}")),
        })
    }

    /// Export jobs written to their targets; an error stops at the first failure.
    pub fn render_exports(&self, jobs: &[crate::app::ExportJob], targets: &[std::path::PathBuf]) -> Result<(), String> {
        for ((ids, _, p), path) in jobs.iter().zip(targets) {
            // A slice exports whatever lies inside it.
            let (svg, size) = match ids.as_slice() {
                [one] if self.doc.node(*one).kind == Kind::Slice => {
                    let n = self.doc.node(*one);
                    (crate::export::region_svg(&self.doc, self.page, n.aabb(), n.id), n.aabb().size())
                }
                _ => (crate::export::to_svg(&self.doc, ids), crate::export::export_bounds(&self.doc, ids).size()),
            };
            let data = crate::export::encode_preset(&self.doc, &svg, p, size)?;
            std::fs::write(path, data).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        Ok(())
    }

    /// Move a new layer into `parent_id` (keeping its position relative to that parent).
    fn mcp_reparent(&mut self, id: NodeId, args: &Value) -> Result<(), String> {
        let Some(p) = args["parent_id"].as_str() else { return Ok(()) };
        let parent: NodeId = p.parse().map_err(|_| format!("bad parent id {p}"))?;
        if !self.doc.nodes.get(&parent).is_some_and(|n| n.kind.is_container()) {
            return Err(format!("{p} can't hold layers"));
        }
        let origin = self.doc.node(parent).rect.min.to_vec2();
        self.doc.translate(id, origin);
        let page = self.doc.page_of(parent);
        self.doc.reparent(id, page, Some(parent), None);
        self.doc.reflow();
        Ok(())
    }
}

/// Case-insensitive match: `*` is a wildcard; without one, the pattern may appear anywhere.
fn glob(pattern: &str, s: &str) -> bool {
    let (p, s) = (pattern.to_lowercase(), s.to_lowercase());
    if !p.contains('*') {
        return s.contains(&p);
    }
    let parts: Vec<&str> = p.split('*').collect();
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if !s.starts_with(first) || s.len() < first.len() + last.len() || !s.ends_with(last) {
        return false;
    }
    let mut rest = &s[first.len()..s.len() - last.len()];
    for mid in &parts[1..parts.len() - 1] {
        match rest.find(mid) {
            Some(i) => rest = &rest[i + mid.len()..],
            None => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use crate::app::App;
    use serde_json::json;

    #[test]
    fn agents_build_and_read_back_through_tools() {
        let mut app = App::new(None);
        let ctx = egui::Context::default();
        // Fonts load on the first pass.
        ctx.begin_pass(Default::default());
        let _ = ctx.end_pass();
        let made = app.mcp_tool(&ctx, "execute_code", &json!({ "code": r#"
            const card = figma.createFrame();
            card.name = "Card <1>";
            card.resize(200, 100);
            const t = figma.createText();
            t.characters = "Hi";
            card.appendChild(t);
            t.x = 10; t.y = 12;
            return { card, n: 2 };
        "# }));
        let out: serde_json::Value = serde_json::from_str(made["content"][0]["text"].as_str().unwrap()).unwrap();
        let id = out["card"]["id"].as_str().unwrap().to_string();
        assert!(id.parse::<crate::doc::NodeId>().is_ok(), "new layers come back with real ids: {out}");
        let meta = app.mcp_tool(&ctx, "get_metadata", &json!({ "node_id": id }));
        let xml = meta["content"][0]["text"].as_str().unwrap();
        assert!(xml.contains(r#"<frame id="1" name="Card &lt;1&gt;" x="0" y="0" width="200" height="100">"#), "{xml}");
        assert!(xml.contains(r#"x="10" y="12""#) && xml.contains(">Hi</text>"), "{xml}");
        assert!(!xml.contains(r#"y="12" width="0""#), "text is measured right away: {xml}");
        let shot = app.mcp_tool(&ctx, "get_screenshot", &json!({}));
        assert_eq!(shot["content"][0]["mimeType"], "image/png");
        // One undo step per call; errors come back as tool errors.
        app.mcp_tool(&ctx, "undo", &json!({}));
        assert!(app.doc.nodes.is_empty());
        let bad = app.mcp_tool(&ctx, "execute_code", &json!({ "code": "figma.nope()" }));
        assert_eq!(bad["isError"], true);
        let page = app.mcp_tool(&ctx, "set_page", &json!({ "page": "Screens", "create": true }));
        assert!(page["content"][0]["text"].as_str().unwrap().contains("Screens"));
        assert_eq!(app.page, 1);
    }

    fn reply(v: &serde_json::Value) -> &str {
        v["content"][0]["text"].as_str().unwrap()
    }

    #[test]
    fn find_export_comments_and_marks() {
        crate::tests::isolate_data();
        let mut app = App::new(None);
        let ctx = egui::Context::default();
        ctx.begin_pass(Default::default());
        let _ = ctx.end_pass();
        app.mcp_tool(&ctx, "execute_code", &json!({ "code": r#"
            const card = figma.createFrame(); card.name = "Pricing card"; card.resize(120, 80);
            const t = figma.createText(); t.characters = "Pro plan"; card.appendChild(t);
            const r = figma.createRectangle(); r.name = "Badge"; r.x = 300;
        "# }));
        // Only the outermost new layers glow: the card and the badge, not the text inside the card.
        let marked: Vec<String> = app.agent_marked().into_iter().map(|id| app.doc.node(id).name.clone()).collect();
        assert_eq!(marked.len(), 2, "{marked:?}");
        assert!(marked.contains(&"Pricing card".to_string()) && marked.contains(&"Badge".to_string()));

        let found: serde_json::Value = serde_json::from_str(reply(&app.mcp_tool(&ctx, "find_nodes", &json!({ "name": "pricing*" })))).unwrap();
        assert_eq!(found.as_array().unwrap().len(), 1);
        let found: serde_json::Value = serde_json::from_str(reply(&app.mcp_tool(&ctx, "find_nodes", &json!({ "text": "pro" })))).unwrap();
        assert_eq!(found[0]["text"], "Pro plan");
        assert_eq!(app.mcp_tool(&ctx, "find_nodes", &json!({}))["isError"], true);

        let dir = std::env::temp_dir().join(format!("katachi-mcp-export-{}", std::process::id()));
        let card = found[0]["id"].as_str().map(|_| app.doc.pages[0].children[0].to_string()).unwrap();
        let out: serde_json::Value = serde_json::from_str(reply(&app.mcp_tool(&ctx, "export", &json!({ "node_ids": [card], "format": "png", "size": "2x", "dir": dir })))).unwrap();
        let file = std::path::PathBuf::from(out["files"][0].as_str().unwrap());
        assert!(file.ends_with("Pricing card@2x.png"), "{file:?}");
        let png = image::load_from_memory(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!((png.width(), png.height()), (240, 160));
        assert_eq!(app.mcp_tool(&ctx, "export", &json!({ "dir": dir, "size": "huge" }))["isError"], true);
        let _ = std::fs::remove_dir_all(&dir);

        app.doc.comments.push(crate::doc::Comment { id: 7, page: 0, at: egui::Pos2::ZERO, anchor: None, messages: vec![("Ana".into(), "Make the badge red".into(), 0)], resolved: false });
        let threads: serde_json::Value = serde_json::from_str(reply(&app.mcp_tool(&ctx, "get_comments", &json!({})))).unwrap();
        assert_eq!(threads[0]["messages"][0]["text"], "Make the badge red");
        app.mcp_tool(&ctx, "reply_to_comment", &json!({ "comment_id": "7", "text": "Done", "resolve": true }));
        assert!(app.doc.comments[0].resolved && app.doc.comments[0].messages.len() == 2);
        let open: serde_json::Value = serde_json::from_str(reply(&app.mcp_tool(&ctx, "get_comments", &json!({})))).unwrap();
        assert!(open.as_array().unwrap().is_empty(), "resolved threads are hidden by default");
    }

    #[test]
    fn globs() {
        use super::glob;
        assert!(glob("card", "Pricing Card") && glob("pri*card", "Pricing card") && glob("*", "") && glob("a*b*c", "aXbYc"));
        assert!(!glob("pri*card", "Pricing cards") && !glob("x*", "ax") && !glob("ab*ba", "aba"));
    }
}
