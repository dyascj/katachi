//! Import Figma `.fig` files: a ZIP (or bare `fig-kiwi` payload) holding a Kiwi-encoded
//! document, with image blobs alongside. Maps Figma's node fields onto Katachi's model.

use crate::kiwi::{Schema, Value};
use std::collections::HashMap;

/// The decoded `Message` plus embedded images (hash hex -> bytes).
pub struct FigFile {
    pub message: Value,
    pub images: HashMap<String, Vec<u8>>,
}

fn fig_kiwi(data: &[u8]) -> Result<Value, String> {
    if !data.starts_with(b"fig-kiwi") && !data.starts_with(b"fig-jam.") {
        return Err("not a Figma file".into());
    }
    let mut chunks = vec![];
    let mut at = 12;
    while at + 4 <= data.len() {
        let n = u32::from_le_bytes(data[at..at + 4].try_into().unwrap()) as usize;
        at += 4;
        chunks.push(data.get(at..at + n).ok_or("truncated chunk")?);
        at += n;
    }
    let inflate = |c: &[u8]| -> Result<Vec<u8>, String> {
        if c.starts_with(&[0x28, 0xB5, 0x2F, 0xFD]) {
            let mut out = vec![];
            let mut dec = ruzstd::decoding::StreamingDecoder::new(c).map_err(|e| e.to_string())?;
            std::io::Read::read_to_end(&mut dec, &mut out).map_err(|e| e.to_string())?;
            Ok(out)
        } else {
            miniz_oxide::inflate::decompress_to_vec(c).map_err(|e| format!("{e:?}"))
        }
    };
    let schema = Schema::decode(&inflate(chunks.first().ok_or("missing schema")?)?)?;
    schema.decode_message("Message", &inflate(chunks.get(1).ok_or("missing data")?)?)
}

pub fn read(data: &[u8]) -> Result<FigFile, String> {
    if !data.starts_with(b"PK") {
        return Ok(FigFile { message: fig_kiwi(data)?, images: HashMap::new() });
    }
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(data)).map_err(|e| e.to_string())?;
    let mut canvas = vec![];
    let mut images = HashMap::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| e.to_string())?;
        let name = f.name().map(|n| n.to_string()).unwrap_or_default();
        let mut buf = vec![];
        std::io::Read::read_to_end(&mut f, &mut buf).map_err(|e| e.to_string())?;
        if name == "canvas.fig" {
            canvas = buf;
        } else if let Some(hash) = name.strip_prefix("images/").filter(|h| !h.is_empty()) {
            images.insert(hash.to_string(), buf);
        }
    }
    Ok(FigFile { message: fig_kiwi(&canvas)?, images })
}

// ---------- mapping onto Katachi ----------

use crate::doc::{Bindable, ListKind, ListLine, CompProp, PropTarget, PropValue, VarId, VarValue, 
    AutoLayout, AxisAlign, Cap, Constraint, Decoration, Document, EffectKind, Join, Kind, LayoutMode, LineHeight, Node, NodeId, Page, Paint,
    PaintKind, PathPoint, Rgba, ScaleMode, Shadow, StrokeAlign, SubPath, TextAlign, TextCase, TextSizing, VAlign,
};
use egui::{Pos2, Rect, Vec2, pos2, vec2};

/// 2x3 affine transform (Figma's m00..m12).
#[derive(Clone, Copy, Debug)]
struct Affine([f32; 6]);

impl Affine {
    const ID: Affine = Affine([1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
    fn of(v: Option<&Value>) -> Affine {
        let Some(t) = v else { return Affine::ID };
        let g = |k| t.f(k).unwrap_or(0.0);
        Affine([g("m00"), g("m01"), g("m02"), g("m10"), g("m11"), g("m12")])
    }
    fn then(self, local: Affine) -> Affine {
        let [a, b, c, d, e, f] = self.0;
        let [la, lb, lc, ld, le, lf] = local.0;
        Affine([a * la + b * ld, a * lb + b * le, a * lc + b * lf + c, d * la + e * ld, d * lb + e * le, d * lc + e * lf + f])
    }
    fn apply(self, p: Pos2) -> Pos2 {
        let [a, b, c, d, e, f] = self.0;
        pos2(a * p.x + b * p.y + c, d * p.x + e * p.y + f)
    }
    fn inverse(self) -> Affine {
        let [a, b, c, d, e, f] = self.0;
        let det = a * e - b * d;
        if det.abs() < 1e-9 {
            return Affine::ID;
        }
        let (ia, ib, id, ie) = (e / det, -b / det, -d / det, a / det);
        Affine([ia, ib, -(ia * c + ib * f), id, ie, -(id * c + ie * f)])
    }
}

fn rgba(c: Option<&Value>, opacity: f32) -> Rgba {
    let Some(c) = c else { return Rgba::BLACK };
    let ch = |k| (c.f(k).unwrap_or(0.0).clamp(0.0, 1.0) * 255.0).round() as u8;
    let a = (c.f("a").unwrap_or(1.0) * opacity).clamp(0.0, 1.0);
    Rgba([ch("r"), ch("g"), ch("b"), (a * 255.0).round() as u8])
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn paints(list: &[Value]) -> Vec<Paint> {
    let mut out = vec![];
    for p in list {
        let visible = p.b("visible").unwrap_or(true);
        let opacity = p.f("opacity").unwrap_or(1.0);
        let kind = p.s("type").unwrap_or("SOLID");
        let mut paint = match kind {
            "SOLID" => Paint::solid(rgba(p.get("color"), opacity)),
            "GRADIENT_LINEAR" | "GRADIENT_RADIAL" | "GRADIENT_ANGULAR" | "GRADIENT_DIAMOND" => {
                let mut g = Paint::solid(Rgba::WHITE.with_alpha((opacity * 255.0) as u8));
                g.kind = crate::doc::gradient_kind(kind);
                g.stops = p.arr("stops").iter().map(|st| (st.f("position").unwrap_or(0.0), rgba(st.get("color"), 1.0))).collect();
                // Figma's gradient transform maps the layer's unit square into gradient space,
                // where the gradient runs along x at y = 0.5.
                let inv = Affine::of(p.get("transform")).inverse();
                if g.kind == PaintKind::Linear {
                    g.from = inv.apply(pos2(0.0, 0.5));
                    g.to = inv.apply(pos2(1.0, 0.5));
                } else {
                    g.from = inv.apply(pos2(0.5, 0.5));
                    g.to = inv.apply(pos2(1.0, 0.5));
                    g.width_to = crate::doc::explicit_width(&g, inv.apply(pos2(0.5, 1.0)));
                }
                g
            }
            "IMAGE" => {
                let mut i = Paint::solid(Rgba::WHITE.with_alpha((opacity * 255.0) as u8));
                i.kind = PaintKind::Image;
                i.image = p.get("image").and_then(|im| im.get("hash")).map(|h| hex(&h.bytes())).unwrap_or_default();
                i.scale_mode = match p.s("imageScaleMode") {
                    Some("FIT") => ScaleMode::Fit,
                    Some("TILE") => ScaleMode::Tile,
                    Some("STRETCH") => ScaleMode::Stretch,
                    _ => ScaleMode::Fill,
                };
                i.tile_scale = p.f("scale").unwrap_or(1.0);
                if let Some(f) = p.get("paintFilter") {
                    let g = |k: &str| f.f(k).unwrap_or(0.0);
                    i.adjust = crate::doc::ImageAdjust {
                        exposure: g("exposure"),
                        contrast: g("contrast"),
                        saturation: g("saturation"),
                        temperature: g("temperature"),
                        tint: g("tint"),
                        highlights: g("highlights"),
                        shadows: g("shadows"),
                    };
                }
                i
            }
            _ => continue,
        };
        paint.visible = visible;
        paint.blend = crate::doc::BlendMode::from_figma(p.s("blendMode"));
        out.push(paint);
    }
    out
}

/// Path blob: command bytes followed by little-endian f32 coordinates in the layer's space.
fn blob_paths(blob: &[u8], to_world: Affine) -> Vec<SubPath> {
    let mut out = vec![];
    let mut cur: Vec<PathPoint> = vec![];
    let mut at = 0;
    let f = |at: &mut usize| -> Option<f32> {
        let v = f32::from_le_bytes(blob.get(*at..*at + 4)?.try_into().ok()?);
        *at += 4;
        Some(if v.is_finite() { v } else { 0.0 })
    };
    let finish = |cur: &mut Vec<PathPoint>, out: &mut Vec<SubPath>, closed: bool| {
        if closed && cur.len() > 1 && cur[0].p.distance(cur[cur.len() - 1].p) < 0.01 {
            let last = cur.pop().unwrap();
            cur[0].h_in = last.h_in;
        }
        if !cur.is_empty() {
            out.push(SubPath { points: std::mem::take(cur), closed });
        }
    };
    while at < blob.len() {
        let cmd = blob[at];
        at += 1;
        let pt = |at: &mut usize| -> Option<Pos2> { Some(to_world.apply(pos2(f(at)?, f(at)?))) };
        match cmd {
            0 => finish(&mut cur, &mut out, true),
            1 => {
                finish(&mut cur, &mut out, false);
                let Some(p) = pt(&mut at) else { break };
                cur.push(PathPoint { p, ..Default::default() });
            }
            2 => {
                let Some(p) = pt(&mut at) else { break };
                cur.push(PathPoint { p, ..Default::default() });
            }
            3 => {
                let (Some(c), Some(p)) = (pt(&mut at), pt(&mut at)) else { break };
                if let Some(prev) = cur.last_mut() {
                    prev.h_out = (c - prev.p) * (2.0 / 3.0);
                }
                cur.push(PathPoint { p, h_in: (c - p) * (2.0 / 3.0), h_out: Vec2::ZERO });
            }
            4 => {
                let (Some(c1), Some(c2), Some(p)) = (pt(&mut at), pt(&mut at), pt(&mut at)) else { break };
                if let Some(prev) = cur.last_mut() {
                    prev.h_out = c1 - prev.p;
                }
                cur.push(PathPoint { p, h_in: c2 - p, h_out: Vec2::ZERO });
            }
            _ => break,
        }
    }
    finish(&mut cur, &mut out, false);
    out
}

fn weight_of(style: &str) -> u16 {
    let s = style.to_lowercase().replace([' ', '-'], "");
    for (k, w) in [("thin", 100), ("extralight", 200), ("ultralight", 200), ("light", 300), ("medium", 500), ("semibold", 600), ("demibold", 600), ("extrabold", 800), ("ultrabold", 800), ("bold", 700), ("black", 900), ("heavy", 900)] {
        if s.contains(k) {
            return w;
        }
    }
    400
}

fn constraint(s: Option<&str>) -> Constraint {
    match s {
        Some("MAX") => Constraint::Max,
        Some("STRETCH") => Constraint::Stretch,
        Some("CENTER") => Constraint::Center,
        Some("SCALE") => Constraint::Scale,
        _ => Constraint::Min,
    }
}

fn align(s: Option<&str>) -> AxisAlign {
    match s {
        Some("CENTER") => AxisAlign::Center,
        Some("MAX") => AxisAlign::Max,
        Some("BASELINE") => AxisAlign::Baseline,
        _ => AxisAlign::Min,
    }
}

type Guid = (u64, u64);

fn guid(v: Option<&Value>) -> Option<Guid> {
    let g = v?;
    Some((g.u("sessionID")?, g.u("localID")?))
}

/// Build a Katachi document from a decoded `.fig`.
pub fn to_document(fig: &FigFile) -> Result<Document, String> {
    let changes = fig.message.arr("nodeChanges");
    let blobs: Vec<Vec<u8>> = fig.message.arr("blobs").iter().map(|b| b.get("bytes").map(|x| x.bytes()).unwrap_or_default()).collect();
    let by_guid: HashMap<Guid, &Value> = changes.iter().filter_map(|n| Some((guid(n.get("guid"))?, n))).collect();
    // Children of each node, in Figma's fractional-index order.
    let mut kids: HashMap<Guid, Vec<(String, Guid)>> = HashMap::new();
    for n in changes {
        if let (Some(me), Some(pi)) = (guid(n.get("guid")), n.get("parentIndex"))
            && let Some(p) = guid(pi.get("guid")) {
                kids.entry(p).or_default().push((pi.s("position").unwrap_or("").to_string(), me));
            }
    }
    for v in kids.values_mut() {
        v.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    }
    let doc_root = changes.iter().find(|n| n.s("type") == Some("DOCUMENT")).and_then(|n| guid(n.get("guid"))).ok_or("no document node")?;
    let mut doc = Document { pages: vec![], ..Default::default() };
    for (k, v) in &fig.images {
        doc.images.insert(k.clone(), std::sync::Arc::new(v.clone()));
    }
    let mut ids: HashMap<Guid, NodeId> = HashMap::new();
    let mut instances: Vec<(NodeId, Guid)> = vec![];
    for (_, page_guid) in kids.get(&doc_root).cloned().unwrap_or_default() {
        let pg = by_guid[&page_guid];
        if pg.s("type") != Some("CANVAS") {
            continue;
        }
        // Figma keeps component masters for instances on a hidden canvas; bring it in as a page.
        let name = if pg.b("internalOnly") == Some(true) { "Internal components".to_string() } else { pg.s("name").unwrap_or("Page").to_string() };
        let bg = pg.get("backgroundColor").map(|c| rgba(Some(c), 1.0).with_alpha(255)).unwrap_or(crate::doc::PAGE_BG);
        doc.pages.push(Page { name, children: vec![], background: bg, flows: vec![], view: None, guides: vec![], prototype: Default::default() });
        let page = doc.pages.len() - 1;
        for (_, child) in kids.get(&page_guid).cloned().unwrap_or_default() {
            import_node(&mut doc, &by_guid, &kids, &blobs, child, page, None, Affine::ID, &mut ids, &mut instances);
        }
    }
    if doc.pages.is_empty() {
        doc.pages.push(Page { name: "Page 1".into(), children: vec![], background: crate::doc::PAGE_BG, flows: vec![], view: None, guides: vec![], prototype: Default::default() });
    }
    // Instances point at their main components once everything exists; sync builds their layers.
    for (inst, sym) in instances {
        match ids.get(&sym) {
            Some(&c) => doc.node_mut(inst).component = Some(c),
            None => doc.node_mut(inst).kind = Kind::Frame,
        }
    }
    import_variables(&mut doc, changes, &by_guid, &ids);
    // Component properties: definitions, the layers bound to them, and instance values.
    let mut defs: HashMap<Guid, (String, String)> = HashMap::new();
    for n in changes {
        for d in n.arr("componentPropDefs") {
            if let Some(id) = guid(d.get("id")) {
                defs.insert(id, (d.s("name").unwrap_or("").to_string(), d.s("type").unwrap_or("BOOL").to_string()));
            }
        }
    }
    let props = Props { defs: &defs, ids: &ids };
    for (g, &id) in &ids {
        let v = by_guid[g];
        let n = doc.node_mut(id);
        for d in v.arr("componentPropDefs") {
            let ty = d.s("type").unwrap_or("BOOL");
            if let Some(default) = props.value(d.get("initialValue"), ty) {
                n.comp_props.push(CompProp { name: d.s("name").unwrap_or("").to_string(), default, preferred: vec![] });
            }
        }
        n.prop_bind = v.arr("componentPropRefs").iter().filter(|r| r.b("isDeleted") != Some(true)).filter_map(|r| {
            let target = match r.s("componentPropNodeField")? {
                "TEXT_DATA" => PropTarget::Text,
                "VISIBLE" => PropTarget::Visible,
                "OVERRIDDEN_SYMBOL_ID" => PropTarget::Swap,
                _ => return None,
            };
            Some((defs.get(&guid(r.get("defID"))?)?.0.clone(), target))
        }).collect();
        props.assign(n, v.arr("componentPropAssignments"));
    }
    doc.reflow();
    // Overrides address layers inside instances by a path of main-component layer ids (or override keys).
    let mut keys: HashMap<Guid, NodeId> = ids.clone();
    for (g, &id) in &ids {
        if let Some(k) = guid(by_guid[g].get("overrideKey")) {
            keys.insert(k, id);
        }
    }
    let mut overrides: Vec<(NodeId, Vec<Guid>, &Value)> = vec![];
    for (g, &id) in &ids {
        if doc.nodes.get(&id).is_some_and(|n| n.kind == Kind::Instance) {
            for o in by_guid[g].get("symbolData").map(|s| s.arr("symbolOverrides")).unwrap_or_default() {
                let path: Vec<Guid> = o.get("guidPath").map(|p| p.arr("guids").iter().filter_map(|x| guid(Some(x))).collect()).unwrap_or_default();
                if !path.is_empty() {
                    overrides.push((id, path, o));
                }
            }
        }
    }
    overrides.sort_by_key(|o| o.1.len());
    // Swaps first, shallowest first, so deeper overrides find the swapped-in layers.
    let mut depth = 0;
    for (inst, path, o) in overrides.iter().filter(|o| o.2.get("overriddenSymbolID").is_some() || o.2.get("componentPropAssignments").is_some()) {
        if path.len() != depth {
            doc.reflow();
            depth = path.len();
        }
        if let Some(t) = resolve(&doc, *inst, path, &keys) {
            if let Some(&c) = guid(o.get("overriddenSymbolID")).and_then(|s| ids.get(&s)) {
                doc.node_mut(t).component = Some(c);
            }
            props.assign(doc.node_mut(t), o.arr("componentPropAssignments"));
        }
    }
    doc.reflow();
    for (inst, path, o) in &overrides {
        if let Some(t) = resolve(&doc, *inst, path, &keys) {
            apply_override(doc.node_mut(t), o);
        }
    }
    doc.reflow();
    Ok(doc)
}

/// Variable collections, variables, layer bindings, and explicit modes.
fn import_variables(doc: &mut Document, changes: &[Value], by_guid: &HashMap<Guid, &Value>, ids: &HashMap<Guid, NodeId>) {
    let live = |n: &&Value, ty: &str| n.s("type") == Some(ty) && n.b("isSoftDeleted") != Some(true);
    // Sets and variables are referenced by guid, or by library key for ones that came from a library.
    let refs = |n: &Value| -> Vec<String> { [guid(n.get("guid")).map(|g| format!("{g:?}")), n.s("key").map(String::from)].into_iter().flatten().collect() };
    let ref_of = |r: Option<&Value>| -> Option<String> {
        let r = r?;
        guid(r.get("guid")).map(|g| format!("{g:?}")).or_else(|| r.get("assetRef").and_then(|a| a.s("key")).map(String::from))
    };
    let mut sets: HashMap<String, (u64, HashMap<Guid, usize>)> = HashMap::new();
    for n in changes.iter().filter(|n| live(n, "VARIABLE_SET")) {
        let mut modes: Vec<&Value> = n.arr("variableSetModes").iter().collect();
        modes.sort_by(|a, b| a.s("sortPosition").unwrap_or("").as_bytes().cmp(b.s("sortPosition").unwrap_or("").as_bytes()));
        let id = doc.add_collection(n.s("name").unwrap_or("Collection").to_string());
        doc.collections.last_mut().unwrap().modes = modes.iter().map(|m| m.s("name").unwrap_or("Mode").to_string()).collect();
        let index: HashMap<Guid, usize> = modes.iter().enumerate().filter_map(|(i, m)| Some((guid(m.get("id"))?, i))).collect();
        for r in refs(n) {
            sets.insert(r, (id, index.clone()));
        }
    }
    let mut vars: HashMap<String, VarId> = HashMap::new();
    let mut aliases: Vec<(VarId, usize, String)> = vec![];
    for n in changes.iter().filter(|n| live(n, "VARIABLE")) {
        let Some((coll, modes)) = ref_of(n.get("variableSetID")).and_then(|r| sets.get(&r)) else { continue };
        let entries = n.get("variableDataValues").map(|v| v.arr("entries")).unwrap_or_default();
        let id = doc.add_variable(*coll, n.s("name").unwrap_or("Variable").to_string(), VarValue::Number(0.0));
        for e in entries {
            let Some(&m) = guid(e.get("modeID")).and_then(|g| modes.get(&g)) else { continue };
            let Some(val) = e.get("variableData").and_then(|d| d.get("value")) else { continue };
            let value = if let Some(c) = val.get("colorValue") {
                VarValue::Color(rgba(Some(c), 1.0))
            } else if let Some(f) = val.f("floatValue") {
                VarValue::Number(f)
            } else if let Some(t) = val.s("textValue") {
                VarValue::Text(t.to_string())
            } else if let Some(b) = val.b("boolValue") {
                VarValue::Bool(b)
            } else if let Some(r) = ref_of(val.get("alias")) {
                aliases.push((id, m, r));
                continue;
            } else {
                continue;
            };
            if let Some(v) = doc.variables.last_mut().unwrap().values.get_mut(m) {
                *v = value;
            }
        }
        // "Show in" scopes and code syntax (WEB, ANDROID, iOS), as the plugin API names them.
        let var = doc.variables.last_mut().unwrap();
        var.scopes = n.arr("variableScopes").iter().filter_map(|s| match s { crate::kiwi::Value::Str(v) | crate::kiwi::Value::Enum(v) => Some(v.as_str()), _ => None }).flat_map(Bindable::from_scope).collect();
        var.scopes.dedup();
        for e in n.get("codeSyntax").map(|c| c.arr("entries")).unwrap_or_default() {
            let slot = match e.s("platform") {
                Some("WEB") => 0,
                Some("iOS") => 1,
                Some("ANDROID") => 2,
                _ => continue,
            };
            var.code_syntax[slot] = e.s("value").unwrap_or_default().to_string();
        }
        for r in refs(n) {
            vars.insert(r, id);
        }
    }
    for (id, m, r) in aliases {
        if let (Some(&target), Some(v)) = (vars.get(&r), doc.variables.iter_mut().find(|v| v.id == id)) {
            v.values[m] = VarValue::Alias(target);
        }
    }
    if vars.is_empty() {
        return;
    }
    let bound = |d: Option<&Value>| ref_of(d?.get("value")?.get("alias")).and_then(|r| vars.get(&r).copied());
    for (g, &id) in ids {
        let v = by_guid[g];
        let mut bindings = vec![];
        // Katachi binds the top paint.
        if let Some(var) = bound(v.arr("fillPaints").last().and_then(|p| p.get("colorVar"))) {
            bindings.push((Bindable::Fill, var));
        }
        if let Some(var) = bound(v.arr("strokePaints").last().and_then(|p| p.get("colorVar"))) {
            bindings.push((Bindable::Stroke, var));
        }
        for e in v.get("variableConsumptionMap").map(|m| m.arr("entries")).unwrap_or_default() {
            let field = match e.s("variableField").unwrap_or("") {
                "OPACITY" => Bindable::Opacity,
                "CORNER_RADIUS" => Bindable::Radius,
                "WIDTH" => Bindable::Width,
                "HEIGHT" => Bindable::Height,
                "STACK_SPACING" => Bindable::Gap,
                "STACK_PADDING_LEFT" | "STACK_PADDING_TOP" | "STACK_PADDING_RIGHT" | "STACK_PADDING_BOTTOM" => Bindable::Padding,
                "STROKE_WEIGHT" => Bindable::StrokeWidth,
                "FONT_SIZE" => Bindable::FontSize,
                "TEXT_DATA" => Bindable::Text,
                "VISIBLE" => Bindable::Visible,
                "LINE_HEIGHT" => Bindable::LineHeight,
                "LETTER_SPACING" => Bindable::LetterSpacing,
                "PARAGRAPH_SPACING" => Bindable::ParagraphSpacing,
                "FONT_FAMILY" => Bindable::FontFamily,
                "FONT_WEIGHT" | "FONT_STYLE" => Bindable::FontWeight,
                "MIN_WIDTH" => Bindable::MinWidth,
                "MAX_WIDTH" => Bindable::MaxWidth,
                "MIN_HEIGHT" => Bindable::MinHeight,
                "MAX_HEIGHT" => Bindable::MaxHeight,
                _ => continue,
            };
            if let Some(var) = bound(e.get("variableData")) {
                bindings.retain(|(b, _)| *b != field);
                bindings.push((field, var));
            }
        }
        let modes: Vec<(u64, usize)> = v
            .get("variableModeBySetMap")
            .map(|m| m.arr("entries"))
            .unwrap_or_default()
            .iter()
            .filter_map(|e| {
                let (coll, modes) = sets.get(&ref_of(e.get("variableSetID"))?)?;
                Some((*coll, *modes.get(&guid(e.get("variableModeID"))?)?))
            })
            .collect();
        let n = doc.node_mut(id);
        n.bindings = bindings;
        n.modes = modes;
    }
}

struct Props<'a> {
    defs: &'a HashMap<Guid, (String, String)>,
    ids: &'a HashMap<Guid, NodeId>,
}

impl Props<'_> {
    fn value(&self, v: Option<&Value>, ty: &str) -> Option<PropValue> {
        match ty {
            "BOOL" => Some(PropValue::Bool(v.and_then(|v| v.b("boolValue")).unwrap_or(true))),
            "TEXT" => Some(PropValue::Text(v?.get("textValue")?.s("characters")?.to_string())),
            "INSTANCE_SWAP" => Some(PropValue::Swap(*self.ids.get(&guid(v?.get("guidValue"))?)?)),
            _ => None,
        }
    }

    /// Instance property values from `componentPropAssignments`.
    fn assign(&self, n: &mut Node, list: &[Value]) {
        for a in list {
            let Some((name, ty)) = guid(a.get("defID")).and_then(|d| self.defs.get(&d)) else { continue };
            if let Some(val) = self.value(a.get("value"), ty) {
                n.prop_values.retain(|(k, _)| k != name);
                n.prop_values.push((name.clone(), val));
            }
        }
    }
}

/// The layer inside `inst` an override path points at: each step finds the descendant that mirrors that main-component layer.
fn resolve(doc: &Document, inst: NodeId, path: &[Guid], keys: &HashMap<Guid, NodeId>) -> Option<NodeId> {
    let mut cur = inst;
    for g in path {
        let target = *keys.get(g)?;
        // A path naming the main component itself means the instance layer.
        if doc.nodes[&cur].component == Some(target) {
            continue;
        }
        cur = doc.descendants(cur).into_iter().find(|&d| {
            let mut s = doc.nodes[&d].source;
            for _ in 0..32 {
                match s {
                    Some(x) if x == target => return true,
                    Some(x) => s = doc.nodes.get(&x).and_then(|n| n.source),
                    None => return false,
                }
            }
            false
        })?;
    }
    Some(cur)
}

/// Apply the fields an instance override sets.
fn apply_override(n: &mut Node, o: &Value) {
    if let Some(x) = o.s("name") {
        n.name = x.to_string();
    }
    if let Some(b) = o.b("visible") {
        n.visible = b;
    }
    if let Some(x) = o.f("opacity") {
        n.opacity = x;
    }
    if o.get("fillPaints").is_some() {
        n.fills = paints(o.arr("fillPaints"));
    }
    if o.get("strokePaints").is_some() && n.kind != Kind::Text {
        n.strokes = paints(o.arr("strokePaints"));
    }
    if let Some(w) = o.f("strokeWeight") {
        n.stroke_width = w;
    }
    if let Some(r) = o.f("cornerRadius") {
        n.radius = r;
        n.corner_radii = None;
    }
    let corners = ["rectangleTopLeftCornerRadius", "rectangleTopRightCornerRadius", "rectangleBottomRightCornerRadius", "rectangleBottomLeftCornerRadius"];
    if corners.iter().any(|k| o.f(k).is_some()) {
        let cur = n.corner_radii.unwrap_or([n.radius; 4]);
        let c: [f32; 4] = std::array::from_fn(|i| o.f(corners[i]).unwrap_or(cur[i]));
        n.corner_radii = if c.iter().all(|x| (x - n.radius).abs() < 0.01) { None } else { Some(c) };
    }
    if let Some(p) = o.s("stackPositioning") {
        n.absolute = p == "ABSOLUTE";
    }
    if o.get("effects").is_some() {
        n.shadows = effects(o);
    }
    if n.kind == Kind::Text {
        text_props(n, o);
        // Fixed text boxes wrap at their own width, which an override may change.
        if let Some(sz) = o.get("size") {
            n.rect = Rect::from_min_size(n.rect.min, vec2(sz.f("x").unwrap_or(n.rect.width()), sz.f("y").unwrap_or(n.rect.height())));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn import_node(
    doc: &mut Document,
    by_guid: &HashMap<Guid, &Value>,
    kids: &HashMap<Guid, Vec<(String, Guid)>>,
    blobs: &[Vec<u8>],
    g: Guid,
    page: usize,
    parent: Option<NodeId>,
    parent_world: Affine,
    ids: &mut HashMap<Guid, NodeId>,
    instances: &mut Vec<(NodeId, Guid)>,
) {
    let v = by_guid[&g];
    let ty = v.s("type").unwrap_or("");
    let world = parent_world.then(Affine::of(v.get("transform")));
    let size = v.get("size").map_or(Vec2::ZERO, |s| vec2(s.f("x").unwrap_or(0.0), s.f("y").unwrap_or(0.0)));
    // Box geometry: center and rotation from the world matrix.
    let center = world.apply(pos2(size.x / 2.0, size.y / 2.0));
    let rotation = -world.0[3].atan2(world.0[0]).to_degrees();
    let rotation = if rotation.abs() < 0.01 { 0.0 } else { crate::doc::normalize_deg(rotation) };
    let rect = Rect::from_center_size(center, size);
    let geometry = |key: &str| -> (Vec<SubPath>, bool) {
        let mut out = vec![];
        let mut even_odd = false;
        for gm in v.arr(key) {
            if let Some(b) = gm.u("commandsBlob").and_then(|i| blobs.get(i as usize)) {
                out.extend(blob_paths(b, world));
                even_odd |= gm.s("windingRule") == Some("ODD");
            }
        }
        (out, even_odd)
    };
    let fills = paints(v.arr("fillPaints"));
    let strokes = paints(v.arr("strokePaints"));
    let mut n = match ty {
        "FRAME" | "SECTION" | "SYMBOL" => {
            let kind = match ty {
                "SECTION" => Kind::Section,
                "SYMBOL" => Kind::Component,
                _ if v.b("isStateGroup") == Some(true) => Kind::ComponentSet,
                _ if v.b("resizeToFit") == Some(true) => Kind::Group,
                _ => Kind::Frame,
            };
            let mut n = Node::new(kind, rect);
            n.clip = kind != Kind::Group && v.b("frameMaskDisabled") != Some(true);
            n
        }
        "INSTANCE" => {
            let mut n = Node::new(Kind::Instance, rect);
            n.clip = v.b("frameMaskDisabled") != Some(true);
            if let Some(sym) = v.get("symbolData").and_then(|s| guid(s.get("symbolID"))) {
                instances.push((0, sym));
            }
            n
        }
        "RECTANGLE" | "ROUNDED_RECTANGLE" => Node::new(Kind::Rectangle, rect),
        "ELLIPSE" => Node::new(Kind::Ellipse, rect),
        "TEXT" => Node::new(Kind::Text, rect),
        "LINE" => {
            let a = world.apply(Pos2::ZERO);
            let b = world.apply(pos2(size.x, 0.0));
            Node::vector(&[PathPoint { p: a, ..Default::default() }, PathPoint { p: b, ..Default::default() }], false)
        }
        "VECTOR" | "STAR" | "REGULAR_POLYGON" | "BOOLEAN_OPERATION" => {
            let (fill_paths, eo) = geometry("fillGeometry");
            if !fill_paths.is_empty() {
                let mut n = Node::vector_paths(fill_paths);
                n.even_odd = eo;
                n
            } else {
                // Stroke-only: Figma's stroke outline geometry filled with the stroke paint is exact.
                let (stroke_paths, eo) = geometry("strokeGeometry");
                if stroke_paths.is_empty() {
                    return;
                }
                let mut n = Node::vector_paths(stroke_paths);
                n.even_odd = eo;
                n.fills = strokes.clone();
                n.strokes.clear();
                n.stroke_width = 0.0;
                n.name = v.s("name").unwrap_or("Vector").to_string();
                finish_common(&mut n, v);
                let id = doc.insert(page, parent, None, n);
                ids.insert(g, id);
                return;
            }
        }
        _ => return,
    };
    let vectorish = n.kind == Kind::Vector;
    if !vectorish && n.kind != Kind::Text {
        n.rotation = rotation;
    }
    n.name = v.s("name").unwrap_or("").to_string();
    n.fills = fills;
    n.strokes = strokes;
    n.stroke_width = v.f("strokeWeight").unwrap_or(1.0);
    if v.b("borderStrokeWeightsIndependent") == Some(true) {
        let w = |side: &str| if v.b(&format!("border{side}Hidden")) == Some(true) { 0.0 } else { v.f(&format!("border{side}Weight")).unwrap_or(n.stroke_width) };
        n.stroke_sides = Some([w("Top"), w("Right"), w("Bottom"), w("Left")]);
    }
    n.stroke_align = match v.s("strokeAlign") {
        Some("INSIDE") => StrokeAlign::Inside,
        Some("OUTSIDE") => StrokeAlign::Outside,
        _ => StrokeAlign::Center,
    };
    if vectorish {
        n.stroke_align = StrokeAlign::Center;
    }
    let r = v.f("cornerRadius").unwrap_or(0.0);
    n.radius = r;
    let corners = ["rectangleTopLeftCornerRadius", "rectangleTopRightCornerRadius", "rectangleBottomRightCornerRadius", "rectangleBottomLeftCornerRadius"].map(|k| v.f(k).unwrap_or(r));
    if corners.iter().any(|c| (c - r).abs() > 0.01) {
        n.corner_radii = Some(corners);
    }
    if n.kind == Kind::Text {
        n.rotation = rotation;
        n.text_sizing = TextSizing::Fixed;
        n.font_size = 12.0;
        text_props(&mut n, v);
        n.strokes.clear();
        // Text boxes keep the file's size; the top-left comes from the unrotated box.
        n.rect = Rect::from_center_size(center, size);
    }
    // Auto layout (sizes stay as stored so layout reproduces Figma's positions).
    n.layout.mode = match v.s("stackMode") {
        Some("HORIZONTAL") => LayoutMode::Horizontal,
        Some("VERTICAL") => LayoutMode::Vertical,
        Some("GRID") => LayoutMode::Grid,
        _ => LayoutMode::None,
    };
    if n.layout.mode != LayoutMode::None {
        let pl = v.f("stackHorizontalPadding").or(v.f("stackPaddingLeft")).unwrap_or(0.0);
        let pt = v.f("stackVerticalPadding").or(v.f("stackPaddingTop")).unwrap_or(0.0);
        let pr = v.f("stackPaddingRight").unwrap_or(pl);
        let pb = v.f("stackPaddingBottom").unwrap_or(pt);
        n.layout = AutoLayout {
            mode: n.layout.mode,
            gap: v.f("stackSpacing").unwrap_or(0.0),
            space_between: v.s("stackPrimaryAlignItems") == Some("SPACE_BETWEEN"),
            padding: [pt, pr, pb, pl],
            main: align(v.s("stackPrimaryAlignItems")),
            cross: align(v.s("stackCounterAlignItems")),
            wrap: v.s("stackWrap") == Some("WRAP"),
            strokes_included: v.b("bordersTakeSpace") == Some(true),
            cross_gap: v.f("stackCounterSpacing").filter(|g| g.is_finite()).or(v.f("stackSpacing")).unwrap_or(0.0),
            reverse_z: v.b("stackReverseZIndex") == Some(true),
            grid: [v.f("gridColumnCount").unwrap_or(0.0) as u32, v.f("gridRowCount").unwrap_or(0.0) as u32],
            tracks: [None; 2],
        };
        if n.layout.mode == LayoutMode::Grid {
            n.layout.gap = v.f("gridColumnGap").unwrap_or(n.layout.gap);
            n.layout.cross_gap = v.f("gridRowGap").unwrap_or(n.layout.cross_gap);
        }
    }
    n.grid_span = [v.f("gridColumnSpan").unwrap_or(1.0).max(1.0) as u32, v.f("gridRowSpan").unwrap_or(1.0).max(1.0) as u32];
    // Unset bounds are stored as 0 (min) and infinity (max).
    let bound = |k: &str, axis: &str| v.get(k).and_then(|s| s.get("value")).and_then(|s| s.f(axis)).filter(|x| x.is_finite() && *x > 0.0);
    n.min_size = [bound("minSize", "x"), bound("minSize", "y")];
    n.max_size = [bound("maxSize", "x"), bound("maxSize", "y")];
    n.absolute = v.s("stackPositioning") == Some("ABSOLUTE");
    n.constraints = [constraint(v.s("horizontalConstraint")), constraint(v.s("verticalConstraint"))];
    // Prototype scrolling.
    n.overflow = match v.s("scrollDirection") {
        Some("VERTICAL") => crate::doc::Overflow::Vertical,
        Some("HORIZONTAL") => crate::doc::Overflow::Horizontal,
        Some("BOTH") => crate::doc::Overflow::Both,
        _ => crate::doc::Overflow::None,
    };
    n.scroll_pos = match v.s("scrollBehavior") {
        Some("FIXED_WHEN_CHILD_OF_SCROLLING_FRAME") => crate::doc::ScrollPos::Fixed,
        Some("STICKY_SCROLLS") => crate::doc::ScrollPos::Sticky,
        _ => crate::doc::ScrollPos::Scroll,
    };
    // Layers start collapsed, as in Figma.
    n.expanded = false;
    finish_common(&mut n, v);
    let id = doc.insert(page, parent, None, n);
    ids.insert(g, id);
    if let Some(last) = instances.last_mut().filter(|(i, _)| *i == 0) {
        last.0 = id;
    }
    // Vector-like layers already baked their children (boolean operands) into geometry.
    if !vectorish {
        for (_, c) in kids.get(&g).cloned().unwrap_or_default() {
            import_node(doc, by_guid, kids, blobs, c, page, Some(id), world, ids, instances);
        }
    }
}

/// Text properties present on `v` (a node or an instance override).
fn text_props(n: &mut Node, v: &Value) {
    if let Some(td) = v.get("textData") {
        if let Some(t) = td.s("characters") {
            if t != n.text {
                n.spans.clear();
            }
            n.text = t.to_string();
        }
        if !td.arr("lines").is_empty() {
            n.lists = td
                .arr("lines")
                .iter()
                .map(|l| {
                    let kind = match l.s("lineType") {
                        Some("ORDERED_LIST") => ListKind::Number,
                        Some("UNORDERED_LIST") => ListKind::Bullet,
                        _ => ListKind::Plain,
                    };
                    // List levels count from 1; plain paragraphs sit at 0.
                    let indent = if kind == ListKind::Plain { 0 } else { l.u("indentationLevel").unwrap_or(1).saturating_sub(1) as u8 };
                    ListLine { kind, indent }
                })
                .collect();
            while n.lists.last().is_some_and(|l| l.kind == ListKind::Plain) {
                n.lists.pop();
            }
        }
    }
    if let Some(f) = v.get("fontName") {
        n.font_family = f.s("family").unwrap_or("Inter").to_string();
        n.font_weight = weight_of(f.s("style").unwrap_or("Regular"));
        n.italic = f.s("style").unwrap_or("").to_lowercase().contains("italic");
    }
    if let Some(sz) = v.f("fontSize") {
        n.font_size = sz;
    }
    if let Some(ps) = v.f("paragraphSpacing") {
        n.paragraph_spacing = ps;
    }
    if let Some(pi) = v.f("paragraphIndent") {
        n.paragraph_indent = pi;
    }
    n.vertical_trim = v.s("leadingTrim") == Some("CAP_HEIGHT");
    if let Some(lh) = v.get("lineHeight") {
        let val = lh.f("value").unwrap_or(100.0);
        n.line_height = match lh.s("units") {
            Some("PIXELS") => LineHeight::Px(val),
            Some("PERCENT") if (val - 100.0).abs() > 0.01 => LineHeight::Percent(val),
            Some("RAW") => LineHeight::Percent(val * 100.0),
            _ => LineHeight::Auto,
        };
    }
    if let Some(ls) = v.get("letterSpacing") {
        let val = ls.f("value").unwrap_or(0.0);
        n.letter_spacing = if ls.s("units") == Some("PIXELS") { val / n.font_size.max(1.0) * 100.0 } else { val };
    }
    if let Some(a) = v.s("textAlignHorizontal") {
        n.text_align = match a {
            "CENTER" => TextAlign::Center,
            "RIGHT" => TextAlign::Right,
            "JUSTIFIED" => TextAlign::Justify,
            _ => TextAlign::Left,
        };
    }
    if let Some(a) = v.s("textAlignVertical") {
        n.text_valign = match a {
            "CENTER" => VAlign::Middle,
            "BOTTOM" => VAlign::Bottom,
            _ => VAlign::Top,
        };
    }
    if let Some(a) = v.s("textAutoResize") {
        n.text_sizing = match a {
            "WIDTH_AND_HEIGHT" => TextSizing::AutoWidth,
            "HEIGHT" => TextSizing::AutoHeight,
            _ => TextSizing::Fixed,
        };
    }
    if let Some(d) = v.s("textDecoration") {
        n.decoration = match d {
            "UNDERLINE" => Decoration::Underline,
            "STRIKETHROUGH" => Decoration::Strikethrough,
            _ => Decoration::None,
        };
    }
    if let Some(t) = v.s("textTruncation") {
        n.truncate = t == "ENDING";
    }
    if let Some(m) = v.u("maxLines") {
        n.max_lines = m as u32;
    }
    if let Some(c) = v.s("textCase") {
        n.text_case = match c {
            "UPPER" => TextCase::Upper,
            "LOWER" => TextCase::Lower,
            "TITLE" => TextCase::Title,
            _ => TextCase::Original,
        };
    }
}

/// Properties every layer type shares.
fn finish_common(n: &mut Node, v: &Value) {
    n.visible = v.b("visible").unwrap_or(true);
    n.description = v.s("description").unwrap_or_default().to_string();
    n.exposed = v.b("isExposedInstance") == Some(true);
    n.opacity = v.f("opacity").unwrap_or(1.0);
    n.blend = crate::doc::BlendMode::from_figma(v.s("blendMode"));
    n.is_mask = v.b("mask") == Some(true);
    n.cap = match v.s("strokeCap") {
        Some("ROUND") => Cap::Round,
        Some("SQUARE") => Cap::Square,
        _ => Cap::None,
    };
    n.join = match v.s("strokeJoin") {
        Some("ROUND") => Join::Round,
        Some("BEVEL") => Join::Bevel,
        _ => Join::Miter,
    };
    n.dash = v.arr("dashPattern").iter().filter_map(|d| if let Value::Float(f) = d { Some(*f) } else { None }).collect();
    n.shadows = effects(v);
    n.exports = v
        .arr("exportSettings")
        .iter()
        .map(|e| {
            use crate::doc::{ExportFormat, ExportSetting, ExportSize};
            let mut out = ExportSetting { suffix: e.s("suffix").unwrap_or("").to_string(), ..Default::default() };
            out.format = match e.s("imageType") {
                Some("JPEG") => ExportFormat::Jpg,
                Some("SVG") => ExportFormat::Svg,
                Some("PDF") => ExportFormat::Pdf,
                _ => ExportFormat::Png,
            };
            let c = e.get("constraint");
            let value = c.and_then(|c| c.f("value")).filter(|v| *v > 0.0).unwrap_or(1.0);
            match c.and_then(|c| c.s("type")) {
                Some("CONTENT_WIDTH") => out.size = ExportSize::Width(value),
                Some("CONTENT_HEIGHT") => out.size = ExportSize::Height(value),
                _ => out.scale = value,
            }
            out
        })
        .collect();
}

fn effects(v: &Value) -> Vec<Shadow> {
    v
        .arr("effects")
        .iter()
        .filter_map(|e| {
            let kind = match e.s("type")? {
                "DROP_SHADOW" => EffectKind::DropShadow,
                "INNER_SHADOW" => EffectKind::InnerShadow,
                "FOREGROUND_BLUR" => EffectKind::LayerBlur,
                "BACKGROUND_BLUR" => EffectKind::BackgroundBlur,
                _ => return None,
            };
            let off = e.get("offset").map_or(Vec2::ZERO, |o| vec2(o.f("x").unwrap_or(0.0), o.f("y").unwrap_or(0.0)));
            Some(Shadow { kind, color: rgba(e.get("color"), 1.0), offset: off, blur: e.f("radius").unwrap_or(0.0), spread: e.f("spread").unwrap_or(0.0), visible: e.b("visible").unwrap_or(true) })
        })
        .collect()
}

/// Read a `.fig` file into a Katachi document.
pub fn import(data: &[u8]) -> Result<Document, String> {
    to_document(&read(data)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kv(j: serde_json::Value) -> Value {
        match j {
            serde_json::Value::Bool(b) => Value::Bool(b),
            serde_json::Value::Number(n) => n.as_u64().map_or(Value::Float(n.as_f64().unwrap_or(0.0) as f32), Value::UInt),
            serde_json::Value::String(s) => Value::Str(s),
            serde_json::Value::Array(a) => Value::Array(a.into_iter().map(kv).collect()),
            serde_json::Value::Object(m) => Value::Object(m.into_iter().map(|(k, v)| (k, kv(v))).collect()),
            serde_json::Value::Null => Value::Array(vec![]),
        }
    }

    #[test]
    fn instance_overrides_and_props_resolve() {
        use serde_json::json;
        let g = |l: u64| json!({ "sessionID": 1, "localID": l });
        let at = |l: u64, pos: &str| json!({ "guid": g(l), "position": pos });
        let xf = |x: f32, y: f32| json!({ "m00": 1.0, "m01": 0.0, "m02": x, "m10": 0.0, "m11": 1.0, "m12": y });
        let msg = json!({ "nodeChanges": [
            { "guid": g(0), "type": "DOCUMENT" },
            { "guid": g(1), "type": "CANVAS", "name": "Page", "parentIndex": at(0, "a") },
            { "guid": g(2), "type": "SYMBOL", "name": "Button", "parentIndex": at(1, "a"), "size": { "x": 100, "y": 40 }, "transform": xf(0., 0.),
              "componentPropDefs": [{ "id": g(90), "name": "Icon", "type": "BOOL", "initialValue": { "boolValue": true } }] },
            { "guid": g(3), "type": "TEXT", "name": "Label", "parentIndex": at(2, "a"), "size": { "x": 80, "y": 20 }, "transform": xf(10., 10.),
              "textData": { "characters": "Button" }, "overrideKey": g(50) },
            { "guid": g(4), "type": "ELLIPSE", "name": "Icon", "parentIndex": at(2, "b"), "size": { "x": 10, "y": 10 }, "transform": xf(85., 15.),
              "componentPropRefs": [{ "defID": g(90), "componentPropNodeField": "VISIBLE" }] },
            { "guid": g(5), "type": "INSTANCE", "name": "Button", "parentIndex": at(1, "b"), "size": { "x": 100, "y": 40 }, "transform": xf(200., 0.),
              "symbolData": { "symbolID": g(2), "symbolOverrides": [
                  { "guidPath": { "guids": [g(50)] }, "textData": { "characters": "Submit" } },
                  { "guidPath": { "guids": [g(2)] }, "opacity": 0.5 },
              ] },
              "componentPropAssignments": [{ "defID": g(90), "value": { "boolValue": false } }] },
        ] });
        let doc = to_document(&FigFile { message: kv(msg), images: HashMap::new() }).unwrap();
        let inst = doc.nodes.values().find(|n| n.kind == Kind::Instance).unwrap();
        assert_eq!(inst.opacity, 0.5, "path naming the main component targets the instance");
        let kids: Vec<&Node> = inst.children.iter().map(|c| doc.node(*c)).collect();
        assert_eq!(kids[0].text, "Submit", "override found through the override key");
        assert!(!kids[1].visible, "boolean property hides the bound layer");
        let comp = doc.nodes.values().find(|n| n.kind == Kind::Component).unwrap();
        assert_eq!(doc.node(comp.children[0]).text, "Button", "main component untouched");
    }

    #[test]
    fn variables_bindings_and_modes_import() {
        use serde_json::json;
        let g = |l: u64| json!({ "sessionID": 1, "localID": l });
        let at = |l: u64, pos: &str| json!({ "guid": g(l), "position": pos });
        let xf = json!({ "m00": 1.0, "m01": 0.0, "m02": 0.0, "m10": 0.0, "m11": 1.0, "m12": 0.0 });
        let color = |r: f32| json!({ "colorValue": { "r": r, "g": 0.0, "b": 0.0, "a": 1.0 } });
        let msg = json!({ "nodeChanges": [
            { "guid": g(0), "type": "DOCUMENT" },
            { "guid": g(1), "type": "CANVAS", "name": "Page", "parentIndex": at(0, "a") },
            { "guid": g(10), "type": "VARIABLE_SET", "name": "Theme", "key": "lib-set",
              "variableSetModes": [{ "id": g(11), "name": "Light", "sortPosition": "a" }, { "id": g(12), "name": "Dark", "sortPosition": "b" }] },
            { "guid": g(20), "type": "VARIABLE", "name": "bg", "variableSetID": { "guid": g(10) },
              "variableDataValues": { "entries": [{ "modeID": g(11), "variableData": { "value": color(1.0) } }, { "modeID": g(12), "variableData": { "value": color(0.0) } }] } },
            { "guid": g(2), "type": "FRAME", "name": "Dark frame", "parentIndex": at(1, "a"), "size": { "x": 100, "y": 100 }, "transform": xf,
              "variableModeBySetMap": { "entries": [{ "variableSetID": { "assetRef": { "key": "lib-set" } }, "variableModeID": g(12) }] } },
            { "guid": g(3), "type": "RECTANGLE", "name": "Box", "parentIndex": at(2, "a"), "size": { "x": 10, "y": 10 }, "transform": xf,
              "fillPaints": [{ "type": "SOLID", "color": { "r": 0.0, "g": 0.0, "b": 0.0, "a": 1.0 }, "opacity": 1.0, "visible": true,
                "colorVar": { "dataType": "ALIAS", "value": { "alias": { "guid": g(20) } } } }] },
        ] });
        let mut doc = to_document(&FigFile { message: kv(msg), images: HashMap::new() }).unwrap();
        assert_eq!(doc.collections[0].modes, vec!["Light", "Dark"]);
        let rect = doc.nodes.values().find(|n| n.kind == Kind::Rectangle).unwrap().id;
        assert_eq!(doc.node(rect).fills[0].color, Rgba::rgb(0, 0, 0), "dark mode value");
        let frame = doc.node(rect).parent.unwrap();
        doc.node_mut(frame).modes.clear();
        doc.reflow();
        assert_eq!(doc.node(rect).fills[0].color, Rgba::rgb(255, 0, 0), "switching back to the default mode");
    }

    #[test]
    fn scroll_behavior_imports() {
        use serde_json::json;
        let g = |l: u64| json!({ "sessionID": 1, "localID": l });
        let at = |l: u64, pos: &str| json!({ "guid": g(l), "position": pos });
        let xf = json!({ "m00": 1.0, "m01": 0.0, "m02": 0.0, "m10": 0.0, "m11": 1.0, "m12": 0.0 });
        let msg = json!({ "nodeChanges": [
            { "guid": g(0), "type": "DOCUMENT" },
            { "guid": g(1), "type": "CANVAS", "name": "Page", "parentIndex": at(0, "a") },
            { "guid": g(2), "type": "FRAME", "name": "List", "parentIndex": at(1, "a"), "size": { "x": 100, "y": 100 }, "transform": xf, "scrollDirection": "VERTICAL" },
            { "guid": g(3), "type": "RECTANGLE", "name": "Header", "parentIndex": at(2, "a"), "size": { "x": 10, "y": 10 }, "transform": xf, "scrollBehavior": "STICKY_SCROLLS" },
        ] });
        let doc = to_document(&FigFile { message: kv(msg), images: HashMap::new() }).unwrap();
        let frame = doc.nodes.values().find(|n| n.kind == Kind::Frame).unwrap();
        assert_eq!(frame.overflow, crate::doc::Overflow::Vertical);
        assert_eq!(doc.node(frame.children[0]).scroll_pos, crate::doc::ScrollPos::Sticky);
    }

    /// Import a real file and report what came through.
    #[test]
    #[ignore]
    fn fig_import_real() {
        let Some(path) = std::env::var_os("KATACHI_FIG") else { return };
        let t = std::time::Instant::now();
        let doc = import(&std::fs::read(path).unwrap()).unwrap();
        let mut kinds: HashMap<String, usize> = HashMap::new();
        for n in doc.nodes.values() {
            *kinds.entry(format!("{:?}", n.kind)).or_default() += 1;
        }
        eprintln!("imported in {:?}: pages {:?}, {} nodes, {} images, kinds {kinds:?}", t.elapsed(), doc.pages.iter().map(|p| (&p.name, p.children.len())).collect::<Vec<_>>(), doc.nodes.len(), doc.images.len());
        // Export a sample card for a visual check.
        if let Some(png) = std::env::var_os("KATACHI_FIG_PNG") {
            let page = doc.pages.iter().position(|p| p.name == "UI Cards").unwrap_or(0);
            let section = doc.pages[page].children[1];
            let card = doc.node(section).children[std::env::var("KATACHI_FIG_CARD").ok().and_then(|c| c.parse().ok()).unwrap_or(0)];
            eprintln!("card: {} {:?}", doc.node(card).name, doc.node(card).rect);
            std::fs::write(png, crate::export::to_png(&doc, &[card], 1.0).unwrap()).unwrap();
        }
        if let Some(out) = std::env::var_os("KATACHI_FIG_OUT") {
            let json = serde_json::json!({ "katachi": 1, "document": doc });
            std::fs::write(out, json.to_string()).unwrap();
        }
    }

    /// `KATACHI_FIG=/path/file.fig cargo test fig_dump -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn fig_dump() {
        let Some(path) = std::env::var_os("KATACHI_FIG") else { return };
        let fig = read(&std::fs::read(path).unwrap()).unwrap();
        let changes = fig.message.arr("nodeChanges");
        let mut kinds: HashMap<String, usize> = HashMap::new();
        for n in changes {
            *kinds.entry(n.s("type").unwrap_or("?").to_string()).or_default() += 1;
        }
        let mut k: Vec<_> = kinds.into_iter().collect();
        k.sort_by_key(|x| std::cmp::Reverse(x.1));
        eprintln!("message keys: {:?}", match &fig.message { Value::Object(m) => m.keys().cloned().collect::<Vec<_>>(), _ => vec![] });
        eprintln!("nodes: {} images: {} blobs: {}", changes.len(), fig.images.len(), fig.message.arr("blobs").len());
        eprintln!("types: {k:?}");
        let want = std::env::var("KATACHI_FIG_TYPE").unwrap_or("RECTANGLE".into());
        if let Some(n) = changes.iter().find(|n| n.s("type") == Some(&want)) {
            if let Value::Object(m) = n {
                for (key, v) in m {
                    let s = format!("{v:?}");
                    eprintln!("  {key}: {}", &s[..s.len().min(220)]);
                }
            }
        }
    }
}
