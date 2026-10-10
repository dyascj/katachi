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

use crate::doc::{
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
                g.kind = if kind == "GRADIENT_LINEAR" { PaintKind::Linear } else { PaintKind::Radial };
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
                i
            }
            _ => continue,
        };
        paint.visible = visible;
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
        Some(v)
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
        doc.pages.push(Page { name, children: vec![], background: bg, flows: vec![] });
        let page = doc.pages.len() - 1;
        for (_, child) in kids.get(&page_guid).cloned().unwrap_or_default() {
            import_node(&mut doc, &by_guid, &kids, &blobs, child, page, None, Affine::ID, &mut ids, &mut instances);
        }
    }
    if doc.pages.is_empty() {
        doc.pages.push(Page { name: "Page 1".into(), children: vec![], background: crate::doc::PAGE_BG, flows: vec![] });
    }
    // Instances point at their main components once everything exists; sync builds their layers.
    for (inst, sym) in instances {
        match ids.get(&sym) {
            Some(&c) => doc.node_mut(inst).component = Some(c),
            None => doc.node_mut(inst).kind = Kind::Frame,
        }
    }
    doc.reflow();
    Ok(doc)
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
        let td = v.get("textData");
        n.text = td.and_then(|t| t.s("characters")).unwrap_or("").to_string();
        if let Some(f) = v.get("fontName") {
            n.font_family = f.s("family").unwrap_or("Inter").to_string();
            n.font_weight = weight_of(f.s("style").unwrap_or("Regular"));
        }
        n.font_size = v.f("fontSize").unwrap_or(12.0);
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
        n.text_align = match v.s("textAlignHorizontal") {
            Some("CENTER") => TextAlign::Center,
            Some("RIGHT") => TextAlign::Right,
            Some("JUSTIFIED") => TextAlign::Justify,
            _ => TextAlign::Left,
        };
        n.text_valign = match v.s("textAlignVertical") {
            Some("CENTER") => VAlign::Middle,
            Some("BOTTOM") => VAlign::Bottom,
            _ => VAlign::Top,
        };
        n.text_sizing = match v.s("textAutoResize") {
            Some("WIDTH_AND_HEIGHT") => TextSizing::AutoWidth,
            Some("HEIGHT") => TextSizing::AutoHeight,
            _ => TextSizing::Fixed,
        };
        n.decoration = match v.s("textDecoration") {
            Some("UNDERLINE") => Decoration::Underline,
            Some("STRIKETHROUGH") => Decoration::Strikethrough,
            _ => Decoration::None,
        };
        n.truncate = v.s("textTruncation") == Some("ENDING");
        n.max_lines = v.u("maxLines").unwrap_or(0) as u32;
        n.text_case = match v.s("textCase") {
            Some("UPPER") => TextCase::Upper,
            Some("LOWER") => TextCase::Lower,
            Some("TITLE") => TextCase::Title,
            _ => TextCase::Original,
        };
        n.strokes.clear();
        // Text boxes keep the file's size; the top-left comes from the unrotated box.
        n.rect = Rect::from_center_size(center, size);
    }
    // Auto layout (sizes stay as stored so layout reproduces Figma's positions).
    n.layout.mode = match v.s("stackMode") {
        Some("HORIZONTAL") => LayoutMode::Horizontal,
        Some("VERTICAL") => LayoutMode::Vertical,
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
        };
    }
    n.absolute = v.s("stackPositioning") == Some("ABSOLUTE");
    n.constraints = [constraint(v.s("horizontalConstraint")), constraint(v.s("verticalConstraint"))];
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

/// Properties every layer type shares.
fn finish_common(n: &mut Node, v: &Value) {
    n.visible = v.b("visible").unwrap_or(true);
    n.opacity = v.f("opacity").unwrap_or(1.0);
    n.blend = crate::doc::BlendMode::ALL
        .into_iter()
        .find(|b| Some(b.css().replace('-', "_").to_uppercase().as_str()) == v.s("blendMode"))
        .unwrap_or_default();
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
    n.shadows = v
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
        .collect();
}

/// Read a `.fig` file into a Katachi document.
pub fn import(data: &[u8]) -> Result<Document, String> {
    to_document(&read(data)?)
}

#[cfg(test)]
mod tests {
    use super::*;

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
