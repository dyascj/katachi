//! Katachi's layers, styles, and variables as Figma plugin-API JSON (what plugin_api.js loads),
//! and a plugin's edits applied back. Only properties a plugin changed are written, so auto
//! layout sizing, style links, and instance overrides aren't clobbered by unchanged values.

use crate::doc::*;
use egui::{Pos2, Rect, pos2, vec2};
use serde_json::{Value, json};
use std::collections::HashMap;

pub fn figma_type(k: Kind) -> &'static str {
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
        "COMPONENT_SET" => Kind::ComponentSet,
        "INSTANCE" => Kind::Instance,
        "BOOLEAN_OPERATION" => Kind::Boolean,
        "POLYGON" => Kind::Polygon,
        "STAR" => Kind::Star,
        "SECTION" => Kind::Section,
        "SLICE" => Kind::Slice,
        "LINE" | "VECTOR" => Kind::Vector,
        _ => Kind::Rectangle,
    }
}

// ---------- values ----------

fn unit(b: u8) -> f64 {
    (b as f64 / 255.0 * 10000.0).round() / 10000.0
}
fn byte(x: f64) -> u8 {
    (x.clamp(0.0, 1.0) * 255.0).round() as u8
}
fn rgb(c: Rgba) -> Value {
    json!({ "r": unit(c.0[0]), "g": unit(c.0[1]), "b": unit(c.0[2]) })
}
fn rgba(c: Rgba) -> Value {
    json!({ "r": unit(c.0[0]), "g": unit(c.0[1]), "b": unit(c.0[2]), "a": unit(c.0[3]) })
}
fn f(v: &Value, k: &str) -> Option<f64> {
    v.get(k)?.as_f64()
}
fn color_from(c: &Value, alpha: f64) -> Rgba {
    let a = f(c, "a").unwrap_or(1.0) * alpha;
    Rgba([byte(f(c, "r").unwrap_or(0.0)), byte(f(c, "g").unwrap_or(0.0)), byte(f(c, "b").unwrap_or(0.0)), byte(a)])
}
fn alias(id: VarId) -> Value {
    json!({ "type": "VARIABLE_ALIAS", "id": format!("VariableID:{id}") })
}

fn inverse(m: [[f64; 3]; 2]) -> Option<[[f64; 3]; 2]> {
    let [[a, b, c], [d, e, f]] = m;
    let det = a * e - b * d;
    if det.abs() < 1e-12 {
        return None;
    }
    Some([[e / det, -b / det, (b * f - c * e) / det], [-d / det, a / det, (c * d - a * f) / det]])
}
fn map(m: [[f64; 3]; 2], x: f64, y: f64) -> Pos2 {
    pos2((m[0][0] * x + m[0][1] * y + m[0][2]) as f32, (m[1][0] * x + m[1][1] * y + m[1][2]) as f32)
}

/// Figma's gradientTransform maps the layer's unit square onto gradient space, where a linear
/// gradient runs (0, 0.5) -> (1, 0.5) and a radial one is centered at (0.5, 0.5) with radius 0.5.
fn gradient_transform(p: &Paint) -> Value {
    let (a, d) = (p.from, p.to - p.from);
    let (a, d) = ((a.x as f64, a.y as f64), (d.x as f64, d.y as f64));
    let m = if p.kind == PaintKind::Linear {
        [[d.0, -d.1, a.0 + 0.5 * d.1], [d.1, d.0, a.1 - 0.5 * d.0]]
    } else {
        [[2.0 * d.0, -2.0 * d.1, a.0 - d.0 + d.1], [2.0 * d.1, 2.0 * d.0, a.1 - d.1 - d.0]]
    };
    let t = inverse(m).unwrap_or([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
    json!([[t[0][0], t[0][1], t[0][2]], [t[1][0], t[1][1], t[1][2]]])
}

pub fn paint_json(p: &Paint, bound: Option<VarId>) -> Value {
    let mut v = match p.kind {
        PaintKind::Solid => json!({ "type": "SOLID", "color": rgb(p.color), "opacity": unit(p.color.a()) }),
        PaintKind::Linear | PaintKind::Radial => json!({
            "type": if p.kind == PaintKind::Linear { "GRADIENT_LINEAR" } else { "GRADIENT_RADIAL" },
            "gradientStops": p.stops.iter().map(|(t, c)| json!({ "position": t, "color": rgba(*c) })).collect::<Vec<_>>(),
            "gradientTransform": gradient_transform(p),
            "opacity": unit(p.color.a()),
        }),
        PaintKind::Image => {
            let a = p.adjust;
            json!({
                "type": "IMAGE", "imageHash": p.image, "opacity": unit(p.color.a()), "scalingFactor": p.tile_scale,
                "scaleMode": match p.scale_mode { ScaleMode::Fill => "FILL", ScaleMode::Fit => "FIT", ScaleMode::Stretch => "CROP", ScaleMode::Tile => "TILE" },
                "filters": { "exposure": a.exposure, "contrast": a.contrast, "saturation": a.saturation, "temperature": a.temperature, "tint": a.tint, "highlights": a.highlights, "shadows": a.shadows },
            })
        }
    };
    v["visible"] = json!(p.visible);
    v["blendMode"] = json!("NORMAL");
    if let Some(id) = bound {
        v["boundVariables"] = json!({ "color": alias(id) });
    }
    v
}

pub fn paint_from(v: &Value, doc: &Document) -> Option<Paint> {
    let opacity = f(v, "opacity").unwrap_or(1.0);
    let mut p = match v.get("type")?.as_str()? {
        "SOLID" => Paint::solid(color_from(v.get("color")?, opacity)),
        t @ ("GRADIENT_LINEAR" | "GRADIENT_RADIAL" | "GRADIENT_ANGULAR" | "GRADIENT_DIAMOND") => {
            let mut p = Paint::solid(Rgba::WHITE.with_alpha(byte(opacity)));
            p.kind = if t == "GRADIENT_LINEAR" { PaintKind::Linear } else { PaintKind::Radial };
            p.stops = v["gradientStops"].as_array()?.iter().map(|s| (f(s, "position").unwrap_or(0.0) as f32, color_from(&s["color"], 1.0))).collect();
            let t: Option<[[f64; 3]; 2]> = v["gradientTransform"].as_array().and_then(|rows| {
                let row = |i: usize| -> Option<[f64; 3]> {
                    let r = rows.get(i)?.as_array()?;
                    Some([r.first()?.as_f64()?, r.get(1)?.as_f64()?, r.get(2)?.as_f64()?])
                };
                Some([row(0)?, row(1)?])
            });
            let m = t.and_then(inverse).unwrap_or([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
            if p.kind == PaintKind::Linear {
                (p.from, p.to) = (map(m, 0.0, 0.5), map(m, 1.0, 0.5));
            } else {
                (p.from, p.to) = (map(m, 0.5, 0.5), map(m, 1.0, 0.5));
            }
            p
        }
        "IMAGE" => {
            let hash = v["imageHash"].as_str()?;
            if !doc.images.contains_key(hash) {
                return None;
            }
            let mut p = Paint::solid(Rgba::WHITE.with_alpha(byte(opacity)));
            p.kind = PaintKind::Image;
            p.image = hash.to_string();
            p.scale_mode = match v["scaleMode"].as_str() {
                Some("FIT") => ScaleMode::Fit,
                Some("CROP") | Some("STRETCH") => ScaleMode::Stretch,
                Some("TILE") => ScaleMode::Tile,
                _ => ScaleMode::Fill,
            };
            p.tile_scale = f(v, "scalingFactor").unwrap_or(1.0) as f32;
            if let Some(fl) = v.get("filters") {
                let g = |k: &str| f(fl, k).unwrap_or(0.0) as f32;
                p.adjust = ImageAdjust { exposure: g("exposure"), contrast: g("contrast"), saturation: g("saturation"), temperature: g("temperature"), tint: g("tint"), highlights: g("highlights"), shadows: g("shadows") };
            }
            p
        }
        _ => return None,
    };
    p.visible = v["visible"].as_bool().unwrap_or(true);
    Some(p)
}

fn effect_json(s: &Shadow) -> Value {
    json!({
        "type": match s.kind { EffectKind::DropShadow => "DROP_SHADOW", EffectKind::InnerShadow => "INNER_SHADOW", EffectKind::LayerBlur => "LAYER_BLUR", EffectKind::BackgroundBlur => "BACKGROUND_BLUR" },
        "color": rgba(s.color), "offset": { "x": s.offset.x, "y": s.offset.y }, "radius": s.blur, "spread": s.spread,
        "visible": s.visible, "blendMode": "NORMAL", "showShadowBehindNode": false,
    })
}

fn effect_from(v: &Value) -> Option<Shadow> {
    let kind = match v["type"].as_str()? {
        "DROP_SHADOW" => EffectKind::DropShadow,
        "INNER_SHADOW" => EffectKind::InnerShadow,
        "LAYER_BLUR" => EffectKind::LayerBlur,
        "BACKGROUND_BLUR" => EffectKind::BackgroundBlur,
        _ => return None,
    };
    Some(Shadow {
        kind,
        color: v.get("color").map_or(Rgba([0, 0, 0, 64]), |c| color_from(c, 1.0)),
        offset: vec2(f(&v["offset"], "x").unwrap_or(0.0) as f32, f(&v["offset"], "y").unwrap_or(0.0) as f32),
        blur: f(v, "radius").unwrap_or(0.0) as f32,
        spread: f(v, "spread").unwrap_or(0.0) as f32,
        visible: v["visible"].as_bool().unwrap_or(true),
    })
}

pub fn weight_of(style: &str) -> (u16, bool) {
    let s = style.to_lowercase().replace([' ', '-'], "");
    let italic = s.contains("italic");
    let s = s.replace("italic", "");
    let w = match s.as_str() {
        "thin" | "hairline" => 100,
        "extralight" | "ultralight" => 200,
        "light" => 300,
        "medium" => 500,
        "semibold" | "demibold" => 600,
        "bold" => 700,
        "extrabold" | "ultrabold" => 800,
        "black" | "heavy" => 900,
        _ => 400,
    };
    (w, italic)
}

fn line_height_json(l: LineHeight) -> Value {
    match l {
        LineHeight::Auto => json!({ "unit": "AUTO" }),
        LineHeight::Px(v) => json!({ "unit": "PIXELS", "value": v }),
        LineHeight::Percent(v) => json!({ "unit": "PERCENT", "value": v }),
    }
}
fn line_height_from(v: &Value) -> LineHeight {
    match (v["unit"].as_str(), f(v, "value")) {
        (Some("PIXELS"), Some(x)) => LineHeight::Px(x as f32),
        (Some("PERCENT"), Some(x)) => LineHeight::Percent(x as f32),
        _ => LineHeight::Auto,
    }
}
fn letter_spacing_from(v: &Value, size: f32) -> f32 {
    let x = f(v, "value").unwrap_or(0.0) as f32;
    if v["unit"].as_str() == Some("PIXELS") { x / size.max(1.0) * 100.0 } else { x }
}
fn case_json(c: TextCase) -> &'static str {
    match c {
        TextCase::Original => "ORIGINAL",
        TextCase::Upper => "UPPER",
        TextCase::Lower => "LOWER",
        TextCase::Title => "TITLE",
    }
}
fn case_from(s: &str) -> TextCase {
    match s {
        "UPPER" => TextCase::Upper,
        "LOWER" => TextCase::Lower,
        "TITLE" => TextCase::Title,
        _ => TextCase::Original,
    }
}
fn deco_json(d: Decoration) -> &'static str {
    match d {
        Decoration::None => "NONE",
        Decoration::Underline => "UNDERLINE",
        Decoration::Strikethrough => "STRIKETHROUGH",
    }
}
fn deco_from(s: &str) -> Decoration {
    match s {
        "UNDERLINE" => Decoration::Underline,
        "STRIKETHROUGH" => Decoration::Strikethrough,
        _ => Decoration::None,
    }
}
fn sizing_json(s: Sizing) -> &'static str {
    match s {
        Sizing::Fixed => "FIXED",
        Sizing::Hug => "HUG",
        Sizing::Fill => "FILL",
    }
}
fn align_json(a: AxisAlign) -> &'static str {
    match a {
        AxisAlign::Min => "MIN",
        AxisAlign::Center => "CENTER",
        AxisAlign::Max => "MAX",
    }
}
fn align_from(s: &str) -> AxisAlign {
    match s {
        "CENTER" => AxisAlign::Center,
        "MAX" => AxisAlign::Max,
        _ => AxisAlign::Min,
    }
}
fn constraint_json(c: Constraint) -> &'static str {
    match c {
        Constraint::Min => "MIN",
        Constraint::Max => "MAX",
        Constraint::Stretch => "STRETCH",
        Constraint::Center => "CENTER",
        Constraint::Scale => "SCALE",
    }
}
fn constraint_from(s: &str) -> Constraint {
    match s {
        "MAX" => Constraint::Max,
        "STRETCH" => Constraint::Stretch,
        "CENTER" => Constraint::Center,
        "SCALE" => Constraint::Scale,
        _ => Constraint::Min,
    }
}
fn blend_json(b: BlendMode) -> String {
    if b == BlendMode::Normal { "PASS_THROUGH".into() } else { b.css().replace('-', "_").to_uppercase() }
}

fn prop_json(v: &PropValue) -> (&'static str, Value) {
    match v {
        PropValue::Bool(b) => ("BOOLEAN", json!(b)),
        PropValue::Text(t) => ("TEXT", json!(t)),
        PropValue::Swap(id) => ("INSTANCE_SWAP", json!(id.to_string())),
    }
}

// ---------- layers ----------

/// One layer as the plugin API sees it (without children).
pub fn node_json(doc: &Document, id: NodeId) -> Value {
    let n = doc.node(id);
    let parent = n.parent.map(|p| doc.node(p));
    let origin = parent.map_or(Pos2::ZERO, |p| p.rect.min);
    let r = n.rect;
    let bound = |b: Bindable| n.bindings.iter().find(|(k, _)| *k == b).map(|(_, v)| *v);
    let last = |len: usize, i: usize| i + 1 == len;
    let paints = |ps: &[Paint], b: Bindable| ps.iter().enumerate().map(|(i, p)| paint_json(p, bound(b).filter(|_| last(ps.len(), i)))).collect::<Vec<_>>();
    let corners = n.corner_radii.unwrap_or([n.radius; 4]);
    let mut v = json!({
        "id": id.to_string(), "type": figma_type(n.kind), "name": n.name,
        "x": r.min.x - origin.x, "y": r.min.y - origin.y, "width": r.width(), "height": r.height(),
        "rotation": n.rotation, "opacity": n.opacity, "visible": n.visible, "locked": n.locked,
        "blendMode": blend_json(n.blend), "isMask": n.is_mask,
        "fills": paints(&n.fills, Bindable::Fill), "strokes": paints(&n.strokes, Bindable::Stroke),
        "strokeWeight": n.stroke_width,
        "strokeAlign": match n.stroke_align { StrokeAlign::Inside => "INSIDE", StrokeAlign::Center => "CENTER", StrokeAlign::Outside => "OUTSIDE" },
        "dashPattern": n.dash, "strokeCap": match n.cap { Cap::None => "NONE", Cap::Round => "ROUND", Cap::Square => "SQUARE" },
        "strokeJoin": match n.join { Join::Miter => "MITER", Join::Round => "ROUND", Join::Bevel => "BEVEL" },
        "effects": n.shadows.iter().map(effect_json).collect::<Vec<_>>(),
        "cornerRadius": n.radius,
        "topLeftRadius": corners[0], "topRightRadius": corners[1], "bottomRightRadius": corners[2], "bottomLeftRadius": corners[3],
        "constraints": { "horizontal": constraint_json(n.constraints[0]), "vertical": constraint_json(n.constraints[1]) },
        "layoutPositioning": if n.absolute { "ABSOLUTE" } else { "AUTO" },
        "layoutSizingHorizontal": sizing_json(n.sizing[0]), "layoutSizingVertical": sizing_json(n.sizing[1]),
        "minWidth": n.min_size[0], "minHeight": n.min_size[1], "maxWidth": n.max_size[0], "maxHeight": n.max_size[1],
        "fillStyleId": n.fill_style.map(|s| format!("S:{s}")).unwrap_or_default(),
        "strokeStyleId": n.stroke_style.map(|s| format!("S:{s}")).unwrap_or_default(),
        "effectStyleId": n.effect_style.map(|s| format!("S:{s}")).unwrap_or_default(),
    });
    // The child's sizing along its auto-layout parent's axes, in the older layoutGrow/layoutAlign terms.
    let parent_mode = parent.map_or(LayoutMode::None, |p| p.layout.mode);
    let (main, cross) = if parent_mode == LayoutMode::Horizontal { (0, 1) } else { (1, 0) };
    v["layoutGrow"] = json!(if parent_mode != LayoutMode::None && n.sizing[main] == Sizing::Fill { 1 } else { 0 });
    v["layoutAlign"] = json!(if parent_mode != LayoutMode::None && n.sizing[cross] == Sizing::Fill { "STRETCH" } else { "INHERIT" });
    if let Some(s) = n.stroke_sides {
        for (k, w) in ["strokeTopWeight", "strokeRightWeight", "strokeBottomWeight", "strokeLeftWeight"].iter().zip(s) {
            v[*k] = json!(w);
        }
    }
    let mut bv = serde_json::Map::new();
    for (b, var) in &n.bindings {
        let keys: &[&str] = match b {
            Bindable::Opacity => &["opacity"],
            Bindable::Radius => &["topLeftRadius", "topRightRadius", "bottomRightRadius", "bottomLeftRadius"],
            Bindable::Width => &["width"],
            Bindable::Height => &["height"],
            Bindable::Gap => &["itemSpacing"],
            Bindable::Padding => &["paddingLeft", "paddingRight", "paddingTop", "paddingBottom"],
            Bindable::StrokeWidth => &["strokeWeight"],
            Bindable::FontSize => &["fontSize"],
            Bindable::Text => &["characters"],
            Bindable::Visible => &["visible"],
            Bindable::Fill | Bindable::Stroke => &[],
        };
        for k in keys {
            bv.insert(k.to_string(), alias(*var));
        }
    }
    v["boundVariables"] = Value::Object(bv);
    v["explicitVariableModes"] = Value::Object(n.modes.iter().map(|(c, m)| (format!("VariableCollectionId:{c}"), json!(format!("{c}:{m}")))).collect());
    if let Some((name, t)) = &n.prop_bind {
        let field = match t {
            PropTarget::Text => "characters",
            PropTarget::Visible => "visible",
            PropTarget::Swap => "mainComponent",
        };
        v["componentPropertyReferences"] = json!({ field: name });
    }
    if n.kind.is_frame() {
        let l = n.layout;
        let h = l.mode == LayoutMode::Horizontal;
        let (ma, ca) = if h { (0, 1) } else { (1, 0) };
        let [pt, pr, pb, pl] = l.padding;
        v["layoutMode"] = json!(match l.mode {
            LayoutMode::None => "NONE",
            LayoutMode::Horizontal => "HORIZONTAL",
            LayoutMode::Vertical => "VERTICAL",
        });
        v["layoutWrap"] = json!(if l.wrap { "WRAP" } else { "NO_WRAP" });
        v["itemSpacing"] = json!(l.gap);
        v["counterAxisSpacing"] = json!(l.cross_gap);
        v["paddingLeft"] = json!(pl);
        v["paddingRight"] = json!(pr);
        v["paddingTop"] = json!(pt);
        v["paddingBottom"] = json!(pb);
        v["primaryAxisAlignItems"] = json!(if l.space_between { "SPACE_BETWEEN" } else { align_json(l.main) });
        v["counterAxisAlignItems"] = json!(align_json(l.cross));
        v["primaryAxisSizingMode"] = json!(if n.sizing[ma] == Sizing::Hug { "AUTO" } else { "FIXED" });
        v["counterAxisSizingMode"] = json!(if n.sizing[ca] == Sizing::Hug { "AUTO" } else { "FIXED" });
        v["clipsContent"] = json!(n.clip);
        v["strokesIncludedInLayout"] = json!(l.strokes_included);
    }
    if n.kind == Kind::Text {
        v["characters"] = json!(n.text);
        v["fontSize"] = json!(n.font_size);
        v["fontName"] = json!({ "family": n.font_family, "style": crate::text::weight_name(n.font_weight) });
        v["textAlignHorizontal"] = json!(match n.text_align {
            TextAlign::Left => "LEFT",
            TextAlign::Center => "CENTER",
            TextAlign::Right => "RIGHT",
            TextAlign::Justify => "JUSTIFIED",
        });
        v["textAlignVertical"] = json!(match n.text_valign {
            VAlign::Top => "TOP",
            VAlign::Middle => "CENTER",
            VAlign::Bottom => "BOTTOM",
        });
        v["textAutoResize"] = json!(match (n.text_sizing, n.truncate) {
            (_, true) => "TRUNCATE",
            (TextSizing::AutoWidth, _) => "WIDTH_AND_HEIGHT",
            (TextSizing::AutoHeight, _) => "HEIGHT",
            (TextSizing::Fixed, _) => "NONE",
        });
        v["lineHeight"] = line_height_json(n.line_height);
        v["letterSpacing"] = json!({ "unit": "PERCENT", "value": n.letter_spacing });
        v["textCase"] = json!(case_json(n.text_case));
        v["textDecoration"] = json!(deco_json(n.decoration));
        v["paragraphSpacing"] = json!(n.paragraph_spacing);
        v["textTruncation"] = json!(if n.truncate { "ENDING" } else { "DISABLED" });
        v["maxLines"] = if n.max_lines > 0 { json!(n.max_lines) } else { Value::Null };
        v["textStyleId"] = json!(n.text_style.map(|s| format!("S:{s}")).unwrap_or_default());
    }
    if matches!(n.kind, Kind::Polygon | Kind::Star) {
        v["pointCount"] = json!(n.point_count);
        v["innerRadius"] = json!(n.inner_ratio);
    }
    if n.kind == Kind::Boolean {
        v["booleanOperation"] = json!(format!("{:?}", n.bool_op).to_uppercase());
    }
    if matches!(n.kind, Kind::Component | Kind::ComponentSet) {
        let mut defs = serde_json::Map::new();
        for p in &n.comp_props {
            let (t, d) = prop_json(&p.default);
            defs.insert(p.name.clone(), json!({ "type": t, "defaultValue": d }));
        }
        if n.kind == Kind::ComponentSet {
            for (k, opts) in doc.variant_props(id) {
                defs.insert(k, json!({ "type": "VARIANT", "defaultValue": opts.first(), "variantOptions": opts }));
            }
        }
        v["componentPropertyDefinitions"] = Value::Object(defs);
    }
    if n.kind == Kind::Instance
        && let Some(c) = n.component.filter(|c| doc.nodes.contains_key(c))
    {
        v["mainComponentId"] = json!(c.to_string());
        let mut props = serde_json::Map::new();
        for p in doc.props_of(c) {
            let val = n.prop_values.iter().find(|(k, _)| *k == p.name).map_or(p.default.clone(), |(_, v)| v.clone());
            let (t, d) = prop_json(&val);
            props.insert(p.name, json!({ "type": t, "value": d }));
        }
        for (k, val) in variant_pairs(&doc.node(c).name) {
            props.insert(k, json!({ "type": "VARIANT", "value": val }));
        }
        v["componentProperties"] = Value::Object(props);
    }
    v
}

fn style_json(s: &Style) -> Value {
    let mut v = json!({ "id": format!("S:{}", s.id), "name": s.name, "description": "" });
    match &s.value {
        StyleValue::Paint(p) => {
            v["type"] = json!("PAINT");
            v["paints"] = json!(p.iter().map(|p| paint_json(p, None)).collect::<Vec<_>>());
        }
        StyleValue::Effect(e) => {
            v["type"] = json!("EFFECT");
            v["effects"] = json!(e.iter().map(effect_json).collect::<Vec<_>>());
        }
        StyleValue::Text(t) => {
            v["type"] = json!("TEXT");
            v["fontName"] = json!({ "family": t.font_family, "style": crate::text::weight_name(t.font_weight) });
            v["fontSize"] = json!(t.font_size);
            v["lineHeight"] = line_height_json(t.line_height);
            v["letterSpacing"] = json!({ "unit": "PERCENT", "value": t.letter_spacing });
            v["textCase"] = json!(case_json(t.text_case));
            v["textDecoration"] = json!(deco_json(t.decoration));
        }
    }
    v
}

fn var_type(doc: &Document, v: &Variable) -> &'static str {
    let mut cur = v.values.first().cloned();
    for _ in 0..8 {
        match cur {
            Some(VarValue::Color(_)) => return "COLOR",
            Some(VarValue::Number(_)) => return "FLOAT",
            Some(VarValue::Text(_)) => return "STRING",
            Some(VarValue::Bool(_)) => return "BOOLEAN",
            Some(VarValue::Alias(a)) => cur = doc.variable(a).and_then(|v| v.values.first().cloned()),
            None => break,
        }
    }
    "FLOAT"
}

fn var_value_json(v: &VarValue) -> Value {
    match v {
        VarValue::Color(c) => rgba(*c),
        VarValue::Number(n) => json!(n),
        VarValue::Text(t) => json!(t),
        VarValue::Bool(b) => json!(b),
        VarValue::Alias(a) => alias(*a),
    }
}

/// Pages (the current one, or all of them), styles, and variables as Figma-shaped JSON.
pub fn snapshot(doc: &Document, page: usize, sel: &[NodeId], all_pages: bool) -> Value {
    fn tree(doc: &Document, id: NodeId) -> Value {
        let mut v = node_json(doc, id);
        let n = doc.node(id);
        if n.kind.is_container() {
            v["children"] = json!(n.children.iter().map(|&c| tree(doc, c)).collect::<Vec<_>>());
        }
        v
    }
    json!({
        "pages": doc.pages.iter().enumerate().map(|(i, p)| {
            let loaded = all_pages || i == page;
            json!({ "name": p.name, "loaded": loaded, "children": if loaded { p.children.iter().map(|&c| tree(doc, c)).collect::<Vec<_>>() } else { vec![] } })
        }).collect::<Vec<_>>(),
        "current": page,
        "selection": sel.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        "styles": doc.styles.iter().map(style_json).collect::<Vec<_>>(),
        "collections": doc.collections.iter().map(|c| json!({
            "id": format!("VariableCollectionId:{}", c.id), "name": c.name,
            "modes": c.modes.iter().enumerate().map(|(i, m)| json!({ "modeId": format!("{}:{i}", c.id), "name": m })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "variables": doc.variables.iter().map(|v| {
            let modes: serde_json::Map<String, Value> = v.values.iter().enumerate().map(|(i, x)| (format!("{}:{i}", v.collection), var_value_json(x))).collect();
            json!({
                "id": format!("VariableID:{}", v.id), "name": v.name, "variableCollectionId": format!("VariableCollectionId:{}", v.collection),
                "resolvedType": var_type(doc, v), "valuesByMode": modes, "description": "", "scopes": ["ALL_SCOPES"],
            })
        }).collect::<Vec<_>>(),
    })
}

// ---------- applying changes ----------

/// Plugin ids (including ones made this run) to Katachi ids.
#[derive(Default)]
pub struct Refs {
    pub nodes: HashMap<String, NodeId>,
    styles: HashMap<String, StyleId>,
    vars: HashMap<String, VarId>,
    colls: HashMap<String, u64>,
    modes: HashMap<String, (u64, usize)>,
}

impl Refs {
    fn style(&self, s: &str) -> Option<StyleId> {
        self.styles.get(s).copied().or_else(|| s.strip_prefix("S:")?.parse().ok())
    }
    fn var(&self, s: &str) -> Option<VarId> {
        self.vars.get(s).copied().or_else(|| s.strip_prefix("VariableID:")?.parse().ok())
    }
    fn coll(&self, s: &str) -> Option<u64> {
        self.colls.get(s).copied().or_else(|| s.strip_prefix("VariableCollectionId:")?.parse().ok())
    }
    fn mode(&self, s: &str) -> Option<(u64, usize)> {
        self.modes.get(s).copied().or_else(|| {
            let (c, i) = s.split_once(':')?;
            Some((c.parse().ok()?, i.parse().ok()?))
        })
    }
    fn node(&self, s: &str) -> Option<NodeId> {
        self.nodes.get(s).copied().or_else(|| s.parse().ok())
    }
}

fn var_value_from(v: &Value, refs: &Refs) -> Option<VarValue> {
    Some(match v {
        Value::Bool(b) => VarValue::Bool(*b),
        Value::Number(n) => VarValue::Number(n.as_f64()? as f32),
        Value::String(s) => VarValue::Text(s.clone()),
        Value::Object(o) if o.get("type").and_then(|t| t.as_str()) == Some("VARIABLE_ALIAS") => VarValue::Alias(refs.var(o.get("id")?.as_str()?)?),
        Value::Object(_) => VarValue::Color(color_from(v, 1.0)),
        _ => return None,
    })
}

fn apply_styles_and_variables(doc: &mut Document, out: &Value, refs: &mut Refs) {
    let strs = |k: &str| out[k].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(String::from)).collect::<Vec<_>>();
    // Styles.
    let gone: Vec<StyleId> = strs("removedStyles").iter().filter_map(|s| refs.style(s)).collect();
    doc.styles.retain(|s| !gone.contains(&s.id));
    for s in out["styles"].as_array().into_iter().flatten() {
        let value = match s["type"].as_str() {
            Some("PAINT") => StyleValue::Paint(s["paints"].as_array().into_iter().flatten().filter_map(|p| paint_from(p, doc)).collect()),
            Some("EFFECT") => StyleValue::Effect(s["effects"].as_array().into_iter().flatten().filter_map(effect_from).collect()),
            Some("TEXT") => {
                let size = f(s, "fontSize").unwrap_or(12.0) as f32;
                StyleValue::Text(TextStyle {
                    font_family: s["fontName"]["family"].as_str().unwrap_or("Inter").to_string(),
                    font_weight: weight_of(s["fontName"]["style"].as_str().unwrap_or("Regular")).0,
                    font_size: size,
                    line_height: line_height_from(&s["lineHeight"]),
                    letter_spacing: letter_spacing_from(&s["letterSpacing"], size),
                    decoration: deco_from(s["textDecoration"].as_str().unwrap_or("")),
                    text_case: case_from(s["textCase"].as_str().unwrap_or("")),
                })
            }
            _ => continue,
        };
        let (jid, name) = (s["id"].as_str().unwrap_or(""), s["name"].as_str().unwrap_or("Style").to_string());
        match refs.style(jid).and_then(|id| doc.styles.iter_mut().find(|x| x.id == id)) {
            Some(existing) => {
                existing.name = name;
                existing.value = value;
            }
            None => {
                let id = doc.add_style(name, value);
                refs.styles.insert(jid.to_string(), id);
            }
        }
    }
    // Collections and their modes.
    let gone: Vec<u64> = strs("removedCollections").iter().filter_map(|s| refs.coll(s)).collect();
    doc.collections.retain(|c| !gone.contains(&c.id));
    doc.variables.retain(|v| !gone.contains(&v.collection));
    for c in out["collections"].as_array().into_iter().flatten() {
        let jid = c["id"].as_str().unwrap_or("");
        let id = match refs.coll(jid).filter(|id| doc.collections.iter().any(|x| x.id == *id)) {
            Some(id) => id,
            None => {
                let id = doc.add_collection(String::new());
                refs.colls.insert(jid.to_string(), id);
                id
            }
        };
        let modes: Vec<(String, String)> = c["modes"].as_array().into_iter().flatten().map(|m| (m["modeId"].as_str().unwrap_or("").to_string(), m["name"].as_str().unwrap_or("Mode").to_string())).collect();
        // Each mode keeps its values by id; added modes start as copies of the first.
        let old: Vec<Option<usize>> = modes.iter().map(|(mid, _)| refs.mode(mid).filter(|(cid, _)| *cid == id).map(|(_, i)| i)).collect();
        for v in doc.variables.iter_mut().filter(|v| v.collection == id) {
            let first = v.values.first().cloned().unwrap_or(VarValue::Number(0.0));
            v.values = old.iter().map(|o| o.and_then(|i| v.values.get(i).cloned()).unwrap_or_else(|| first.clone())).collect();
        }
        for (i, (mid, _)) in modes.iter().enumerate() {
            refs.modes.insert(mid.clone(), (id, i));
        }
        if let Some(coll) = doc.collections.iter_mut().find(|x| x.id == id) {
            coll.name = c["name"].as_str().unwrap_or("Collection").to_string();
            coll.modes = modes.into_iter().map(|(_, n)| n).collect();
            if coll.modes.is_empty() {
                coll.modes.push("Mode 1".into());
            }
        }
    }
    // Variables: create first so aliases between new ones resolve.
    let gone: Vec<VarId> = strs("removedVariables").iter().filter_map(|s| refs.var(s)).collect();
    doc.variables.retain(|v| !gone.contains(&v.id));
    let vars = out["variables"].as_array().cloned().unwrap_or_default();
    for v in &vars {
        let jid = v["id"].as_str().unwrap_or("");
        if refs.var(jid).is_some_and(|id| doc.variables.iter().any(|x| x.id == id)) {
            continue;
        }
        let Some(coll) = v["variableCollectionId"].as_str().and_then(|c| refs.coll(c)) else { continue };
        let id = doc.add_variable(coll, v["name"].as_str().unwrap_or("Variable").to_string(), VarValue::Number(0.0));
        refs.vars.insert(jid.to_string(), id);
    }
    for v in &vars {
        let Some(id) = v["id"].as_str().and_then(|s| refs.var(s)) else { continue };
        let values: Vec<(usize, VarValue)> = v["valuesByMode"].as_object().into_iter().flatten().filter_map(|(m, x)| Some((refs.mode(m)?.1, var_value_from(x, refs)?))).collect();
        if let Some(var) = doc.variables.iter_mut().find(|x| x.id == id) {
            var.name = v["name"].as_str().unwrap_or(&var.name).to_string();
            for (i, x) in values {
                if let Some(slot) = var.values.get_mut(i) {
                    *slot = x;
                }
            }
        }
    }
}

/// Write the properties of `v` that differ from `base` (all of them when there's no base).
fn apply_props(doc: &mut Document, id: NodeId, v: &Value, base: &Value, moved: bool, refs: &Refs) {
    let changed = |k: &str| v.get(k).is_some_and(|x| base.get(k) != Some(x));
    let num = |k: &str| f(v, k).map(|x| x as f32);
    let s = |k: &str| v[k].as_str().unwrap_or("");
    let origin = doc.node(id).parent.map_or(Pos2::ZERO, |p| doc.node(p).rect.min);
    let old = doc.node(id).rect;
    // Geometry: position relative to the parent, then size.
    if moved || changed("x") || changed("y") {
        let x = if moved || changed("x") { num("x").map_or(old.min.x, |x| origin.x + x) } else { old.min.x };
        let y = if moved || changed("y") { num("y").map_or(old.min.y, |y| origin.y + y) } else { old.min.y };
        doc.translate(id, pos2(x, y) - old.min);
    }
    if changed("width") || changed("height") {
        let cur = doc.node(id).rect;
        let size = vec2(num("width").unwrap_or(cur.width()).max(0.0), num("height").unwrap_or(cur.height()).max(0.0));
        if doc.node(id).kind == Kind::Vector && cur.height() == 0.0 {
            doc.node_mut(id).rect = Rect::from_min_size(cur.min, size);
        } else {
            doc.set_rect(id, Rect::from_min_size(cur.min, size));
        }
    }
    let parent_mode = doc.node(id).parent.map_or(LayoutMode::None, |p| doc.node(p).layout.mode);
    // Paints read the image table, so parse them before borrowing the layer.
    let paints = |k: &str| -> Option<Vec<Paint>> { changed(k).then(|| v[k].as_array().into_iter().flatten().filter_map(|p| paint_from(p, doc)).collect()) };
    let (fills, strokes) = (paints("fills"), paints("strokes"));
    // Type changes the API allows: createComponentFromNode, and detaching an instance.
    if changed("type") {
        match s("type") {
            "COMPONENT" if doc.node(id).kind.is_frame() || doc.node(id).kind == Kind::Group => doc.node_mut(id).kind = Kind::Component,
            "FRAME" if doc.node(id).kind == Kind::Instance => doc.detach_instance(id),
            _ => {}
        }
    }
    let n = doc.node_mut(id);
    if let Some(p) = fills {
        n.fills = p;
    }
    if let Some(p) = strokes {
        n.strokes = p;
    }
    if changed("name") {
        n.name = s("name").to_string();
    }
    if changed("rotation") {
        n.rotation = num("rotation").unwrap_or(0.0);
    }
    if changed("opacity") {
        n.opacity = num("opacity").unwrap_or(1.0).clamp(0.0, 1.0);
    }
    if changed("visible") {
        n.visible = v["visible"].as_bool().unwrap_or(true);
    }
    if changed("locked") {
        n.locked = v["locked"].as_bool().unwrap_or(false);
    }
    if changed("isMask") {
        n.is_mask = v["isMask"].as_bool().unwrap_or(false);
    }
    if changed("blendMode") {
        n.blend = BlendMode::ALL.into_iter().find(|b| blend_json(*b) == s("blendMode")).unwrap_or_default();
    }
    // Variables bound to the top paint.
    for (key, b) in [("fills", Bindable::Fill), ("strokes", Bindable::Stroke)] {
        if changed(key) {
            n.bindings.retain(|(k, _)| *k != b);
            let top = v[key].as_array().and_then(|a| a.last()).and_then(|p| p["boundVariables"]["color"]["id"].as_str()).and_then(|s| refs.var(s));
            if let Some(var) = top {
                n.bindings.push((b, var));
            }
        }
    }
    if changed("strokeWeight") {
        n.stroke_width = num("strokeWeight").unwrap_or(1.0).max(0.0);
        n.stroke_sides = None;
    }
    let sides = ["strokeTopWeight", "strokeRightWeight", "strokeBottomWeight", "strokeLeftWeight"];
    if sides.iter().any(|k| changed(k)) {
        let w = n.stroke_width;
        let s4: [f32; 4] = std::array::from_fn(|i| num(sides[i]).unwrap_or(w));
        n.stroke_sides = if s4.iter().all(|x| *x == w) { None } else { Some(s4) };
    }
    if changed("strokeAlign") {
        n.stroke_align = match s("strokeAlign") {
            "INSIDE" => StrokeAlign::Inside,
            "OUTSIDE" => StrokeAlign::Outside,
            _ => StrokeAlign::Center,
        };
    }
    if changed("dashPattern") {
        n.dash = v["dashPattern"].as_array().into_iter().flatten().filter_map(|x| x.as_f64().map(|x| x as f32)).collect();
    }
    if changed("strokeCap") {
        n.cap = match s("strokeCap") {
            "ROUND" => Cap::Round,
            "SQUARE" => Cap::Square,
            _ => Cap::None,
        };
    }
    if changed("strokeJoin") {
        n.join = match s("strokeJoin") {
            "ROUND" => Join::Round,
            "BEVEL" => Join::Bevel,
            _ => Join::Miter,
        };
    }
    if changed("effects") {
        n.shadows = v["effects"].as_array().into_iter().flatten().filter_map(effect_from).collect();
    }
    if changed("cornerRadius") && v["cornerRadius"].is_number() {
        n.radius = num("cornerRadius").unwrap_or(0.0).max(0.0);
        n.corner_radii = None;
    }
    let corner_keys = ["topLeftRadius", "topRightRadius", "bottomRightRadius", "bottomLeftRadius"];
    if corner_keys.iter().any(|k| changed(k)) {
        let cur = n.corner_radii.unwrap_or([n.radius; 4]);
        let c: [f32; 4] = std::array::from_fn(|i| num(corner_keys[i]).unwrap_or(cur[i]).max(0.0));
        n.corner_radii = if c.iter().all(|x| *x == c[0]) { None } else { Some(c) };
        if n.corner_radii.is_none() {
            n.radius = c[0];
        }
    }
    if changed("constraints") {
        n.constraints = [constraint_from(v["constraints"]["horizontal"].as_str().unwrap_or("")), constraint_from(v["constraints"]["vertical"].as_str().unwrap_or(""))];
    }
    if changed("layoutPositioning") {
        n.absolute = s("layoutPositioning") == "ABSOLUTE";
    }
    for (k, i, max) in [("minWidth", 0, false), ("minHeight", 1, false), ("maxWidth", 0, true), ("maxHeight", 1, true)] {
        if changed(k) {
            let val = num(k).filter(|x| x.is_finite() && *x > 0.0);
            if max { n.max_size[i] = val } else { n.min_size[i] = val }
        }
    }
    // Auto layout on frames.
    if n.kind.is_frame() {
        let l = &mut n.layout;
        if changed("layoutMode") {
            l.mode = match s("layoutMode") {
                "HORIZONTAL" => LayoutMode::Horizontal,
                "VERTICAL" => LayoutMode::Vertical,
                _ => LayoutMode::None,
            };
        }
        if changed("layoutWrap") {
            l.wrap = s("layoutWrap") == "WRAP";
        }
        if changed("itemSpacing") {
            l.gap = num("itemSpacing").unwrap_or(0.0);
        }
        if changed("counterAxisSpacing") {
            l.cross_gap = num("counterAxisSpacing").unwrap_or(0.0);
        }
        for (k, i) in [("paddingTop", 0), ("paddingRight", 1), ("paddingBottom", 2), ("paddingLeft", 3)] {
            if changed(k) {
                l.padding[i] = num(k).unwrap_or(0.0);
            }
        }
        if changed("primaryAxisAlignItems") {
            l.space_between = s("primaryAxisAlignItems") == "SPACE_BETWEEN";
            l.main = align_from(s("primaryAxisAlignItems"));
        }
        if changed("counterAxisAlignItems") {
            l.cross = align_from(s("counterAxisAlignItems"));
        }
        if changed("strokesIncludedInLayout") {
            l.strokes_included = v["strokesIncludedInLayout"].as_bool().unwrap_or(false);
        }
        if changed("clipsContent") {
            n.clip = v["clipsContent"].as_bool().unwrap_or(true);
        }
    }
    // Resizing stops hugging on that axis, as in Figma (explicit sizing below still wins).
    for (k, axis) in [("width", 0), ("height", 1)] {
        if changed(k) && n.sizing[axis] == Sizing::Hug && n.kind != Kind::Text {
            n.sizing[axis] = Sizing::Fixed;
        }
    }
    // Sizing: the frame's own hugging, then the older child terms, then layoutSizing* (most explicit) last.
    let (ma, ca) = if n.layout.mode == LayoutMode::Horizontal { (0, 1) } else { (1, 0) };
    if n.kind.is_frame() && n.layout.mode != LayoutMode::None {
        for (k, axis) in [("primaryAxisSizingMode", ma), ("counterAxisSizingMode", ca)] {
            if changed(k) || (changed("layoutMode") && v.get(k).is_some()) {
                n.sizing[axis] = if s(k) == "AUTO" { Sizing::Hug } else if n.sizing[axis] == Sizing::Hug { Sizing::Fixed } else { n.sizing[axis] };
            }
        }
    }
    if parent_mode != LayoutMode::None {
        let (pm, pc) = if parent_mode == LayoutMode::Horizontal { (0, 1) } else { (1, 0) };
        if changed("layoutGrow") {
            n.sizing[pm] = if num("layoutGrow").unwrap_or(0.0) > 0.0 { Sizing::Fill } else if n.sizing[pm] == Sizing::Fill { Sizing::Fixed } else { n.sizing[pm] };
        }
        if changed("layoutAlign") {
            n.sizing[pc] = if s("layoutAlign") == "STRETCH" { Sizing::Fill } else if n.sizing[pc] == Sizing::Fill { Sizing::Fixed } else { n.sizing[pc] };
        }
    }
    for (k, axis) in [("layoutSizingHorizontal", 0), ("layoutSizingVertical", 1)] {
        if changed(k) {
            n.sizing[axis] = match s(k) {
                "HUG" => Sizing::Hug,
                "FILL" => Sizing::Fill,
                _ => Sizing::Fixed,
            };
            if n.kind == Kind::Text && axis == 0 {
                n.text_sizing = if s(k) == "HUG" { TextSizing::AutoWidth } else if n.text_sizing == TextSizing::AutoWidth { TextSizing::AutoHeight } else { n.text_sizing };
            }
        }
    }
    // Text.
    if n.kind == Kind::Text {
        if changed("characters") {
            let t = s("characters").to_string();
            n.spans.clear();
            n.lists = shift_lists(&n.lists, &n.text, &t);
            n.text = t;
        }
        if changed("fontSize") {
            n.font_size = num("fontSize").unwrap_or(12.0).max(1.0);
        }
        if changed("fontName") {
            n.font_family = v["fontName"]["family"].as_str().unwrap_or("Inter").to_string();
            let (w, italic) = weight_of(v["fontName"]["style"].as_str().unwrap_or("Regular"));
            n.font_weight = w;
            n.spans.retain(|sp| sp.style.italic.is_none());
            if italic {
                let len = n.text.chars().count();
                n.spans = style_range(&n.spans, len, 0, len, &SpanStyle { italic: Some(true), ..Default::default() });
            }
        }
        if changed("textAlignHorizontal") {
            n.text_align = match s("textAlignHorizontal") {
                "CENTER" => TextAlign::Center,
                "RIGHT" => TextAlign::Right,
                "JUSTIFIED" => TextAlign::Justify,
                _ => TextAlign::Left,
            };
        }
        if changed("textAlignVertical") {
            n.text_valign = match s("textAlignVertical") {
                "CENTER" => VAlign::Middle,
                "BOTTOM" => VAlign::Bottom,
                _ => VAlign::Top,
            };
        }
        if changed("textAutoResize") {
            n.truncate = s("textAutoResize") == "TRUNCATE";
            n.text_sizing = match s("textAutoResize") {
                "WIDTH_AND_HEIGHT" => TextSizing::AutoWidth,
                "HEIGHT" => TextSizing::AutoHeight,
                _ => TextSizing::Fixed,
            };
        }
        if changed("lineHeight") {
            n.line_height = line_height_from(&v["lineHeight"]);
        }
        if changed("letterSpacing") || changed("fontSize") {
            n.letter_spacing = letter_spacing_from(&v["letterSpacing"], n.font_size);
        }
        if changed("textCase") {
            n.text_case = case_from(s("textCase"));
        }
        if changed("textDecoration") {
            n.decoration = deco_from(s("textDecoration"));
        }
        if changed("paragraphSpacing") {
            n.paragraph_spacing = num("paragraphSpacing").unwrap_or(0.0).max(0.0);
        }
        if changed("textTruncation") {
            n.truncate = s("textTruncation") == "ENDING";
        }
        if changed("maxLines") {
            n.max_lines = v["maxLines"].as_u64().unwrap_or(0) as u32;
        }
        // Styled ranges.
        let len = n.text.chars().count();
        for r in v["__spans"].as_array().into_iter().flatten() {
            let (a, b) = (r["start"].as_u64().unwrap_or(0) as usize, r["end"].as_u64().unwrap_or(0) as usize);
            let mut st = SpanStyle::default();
            if let Some(size) = f(r, "fontSize") {
                st.font_size = Some(size as f32);
            }
            if let Some(fname) = r.get("fontName") {
                st.font_family = fname["family"].as_str().map(String::from);
                let (w, italic) = weight_of(fname["style"].as_str().unwrap_or("Regular"));
                st.font_weight = Some(w);
                st.italic = italic.then_some(true);
            }
            if let Some(c) = r["fills"].as_array().and_then(|a| a.last()).filter(|p| p["type"] == "SOLID") {
                st.color = Some(color_from(&c["color"], f(c, "opacity").unwrap_or(1.0)));
            }
            if let Some(d) = r["textDecoration"].as_str() {
                st.decoration = Some(deco_from(d));
            }
            n.spans = style_range(&n.spans, len, a.min(len), b.min(len), &st);
        }
    }
    if changed("pointCount") {
        n.point_count = v["pointCount"].as_u64().unwrap_or(5).clamp(3, 60) as u32;
    }
    if changed("innerRadius") {
        n.inner_ratio = num("innerRadius").unwrap_or(0.38).clamp(0.01, 1.0);
    }
    if changed("booleanOperation") {
        n.bool_op = match s("booleanOperation") {
            "SUBTRACT" => BoolOp::Subtract,
            "INTERSECT" => BoolOp::Intersect,
            "EXCLUDE" => BoolOp::Exclude,
            _ => BoolOp::Union,
        };
    }
    // Style links (setting a property by hand detaches, like Figma; linking re-applies on reflow).
    for (k, slot) in [("fillStyleId", 0), ("strokeStyleId", 1), ("effectStyleId", 2), ("textStyleId", 3)] {
        if changed(k) {
            let st = Some(s(k)).filter(|x| !x.is_empty()).and_then(|x| refs.style(x));
            match slot {
                0 => n.fill_style = st,
                1 => n.stroke_style = st,
                2 => n.effect_style = st,
                _ => n.text_style = st,
            }
        }
    }
    if changed("fills") && !changed("fillStyleId") {
        n.fill_style = None;
    }
    if changed("strokes") && !changed("strokeStyleId") {
        n.stroke_style = None;
    }
    if changed("effects") && !changed("effectStyleId") {
        n.effect_style = None;
    }
    // Variables bound to properties.
    if changed("boundVariables") {
        let bv = &v["boundVariables"];
        n.bindings.retain(|(b, _)| matches!(b, Bindable::Fill | Bindable::Stroke));
        let pick = |keys: &[&str]| keys.iter().find_map(|k| bv[*k]["id"].as_str().and_then(|s| refs.var(s)));
        for (b, keys) in [
            (Bindable::Opacity, &["opacity"][..]),
            (Bindable::Radius, &["cornerRadius", "topLeftRadius", "topRightRadius", "bottomLeftRadius", "bottomRightRadius"][..]),
            (Bindable::Width, &["width"][..]),
            (Bindable::Height, &["height"][..]),
            (Bindable::Gap, &["itemSpacing"][..]),
            (Bindable::Padding, &["paddingLeft", "paddingRight", "paddingTop", "paddingBottom"][..]),
            (Bindable::StrokeWidth, &["strokeWeight"][..]),
            (Bindable::FontSize, &["fontSize"][..]),
            (Bindable::Text, &["characters"][..]),
            (Bindable::Visible, &["visible"][..]),
        ] {
            if let Some(var) = pick(keys) {
                n.bindings.push((b, var));
            }
        }
    }
    if changed("explicitVariableModes") {
        n.modes = v["explicitVariableModes"].as_object().into_iter().flatten().filter_map(|(_, m)| refs.mode(m.as_str()?)).collect();
    }
    // Component properties: definitions on components and sets, references on their layers.
    if changed("componentPropertyDefinitions") {
        n.comp_props = v["componentPropertyDefinitions"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(name, d)| {
                let default = match d["type"].as_str()? {
                    "BOOLEAN" => PropValue::Bool(d["defaultValue"].as_bool().unwrap_or(true)),
                    "TEXT" => PropValue::Text(d["defaultValue"].as_str().unwrap_or("").to_string()),
                    "INSTANCE_SWAP" => PropValue::Swap(refs.node(d["defaultValue"].as_str()?)?),
                    _ => return None,
                };
                // Figma suffixes names with "#id"; Katachi binds by plain name.
                Some(CompProp { name: name.split('#').next().unwrap_or(name).to_string(), default })
            })
            .collect();
    }
    if changed("componentPropertyReferences") {
        let r = &v["componentPropertyReferences"];
        let plain = |s: &str| s.split('#').next().unwrap_or(s).to_string();
        n.prop_bind = if let Some(p) = r["characters"].as_str() {
            Some((plain(p), PropTarget::Text))
        } else if let Some(p) = r["visible"].as_str() {
            Some((plain(p), PropTarget::Visible))
        } else {
            r["mainComponent"].as_str().map(|p| (plain(p), PropTarget::Swap))
        };
    }
}

/// Apply a plugin run's collected changes. Returns plugin ids -> Katachi ids.
pub fn apply(doc: &mut Document, out: &Value) -> HashMap<String, NodeId> {
    let mut refs = Refs::default();
    apply_styles_and_variables(doc, out, &mut refs);
    // Pages: existing ones keep their index ("page:i"); new ones are added at the end.
    let out_pages = out["pages"].as_array().cloned().unwrap_or_default();
    let mut page_of: Vec<usize> = vec![];
    for p in &out_pages {
        let name = p["name"].as_str().unwrap_or("Page").to_string();
        let i = match p["id"].as_str().and_then(|s| s.strip_prefix("page:")).and_then(|s| s.parse::<usize>().ok()).filter(|i| *i < doc.pages.len()) {
            Some(i) => i,
            None => {
                doc.pages.push(Page { name: name.clone(), children: vec![], background: PAGE_BG, flows: vec![] });
                doc.pages.len() - 1
            }
        };
        doc.pages[i].name = name;
        page_of.push(i);
    }
    let page_for = |v: &Value| v["page"].as_u64().and_then(|i| page_of.get(i as usize).copied()).unwrap_or(0);
    let nodes = out["nodes"].as_array().cloned().unwrap_or_default();
    let by_id: HashMap<&str, &Value> = nodes.iter().filter_map(|v| Some((v["id"].as_str()?, v))).collect();
    // Bases: how each existing layer looked before this run.
    let bases: HashMap<NodeId, Value> = nodes.iter().filter_map(|v| v["id"].as_str()?.parse::<NodeId>().ok()).filter(|id| doc.nodes.contains_key(id)).map(|id| (id, node_json(doc, id))).collect();
    for id in out["removed"].as_array().into_iter().flatten().filter_map(|v| v.as_str()?.parse::<NodeId>().ok()) {
        if doc.nodes.contains_key(&id) {
            doc.remove(id);
        }
    }
    // Instance internals made by createInstance are matched up after the instance syncs.
    let provisional = |v: &Value| v.get("__src").is_some();
    let mut fresh: Vec<String> = vec![];
    for v in &nodes {
        let jid = v["id"].as_str().unwrap_or_default().to_string();
        if let Some(i) = jid.parse::<NodeId>().ok().filter(|i| doc.nodes.contains_key(i)) {
            refs.nodes.insert(jid, i);
            continue;
        }
        if provisional(v) {
            continue;
        }
        let ty = v["type"].as_str().unwrap_or("RECTANGLE");
        let name = v["name"].as_str().unwrap_or("").to_string();
        let page = page_for(v);
        let id = if let Some(svg) = v["__svg"].as_str() {
            match crate::svg_import::import(doc, svg.as_bytes(), &name, page, None, Pos2::ZERO) {
                Ok((f, images)) => {
                    for (img, bytes) in images {
                        use std::hash::{Hash, Hasher};
                        let mut h = std::hash::DefaultHasher::new();
                        bytes.hash(&mut h);
                        let key = format!("{:016x}", h.finish());
                        doc.images.entry(key.clone()).or_insert_with(|| std::sync::Arc::new(bytes));
                        let mut p = Paint::solid(Rgba::WHITE);
                        p.convert(PaintKind::Image);
                        p.image = key;
                        doc.node_mut(img).fills = vec![p];
                    }
                    f
                }
                Err(_) => continue,
            }
        } else {
            let kind = kind_of(ty);
            let mut n = if ty == "LINE" || ty == "VECTOR" {
                Node::vector(&[PathPoint::default(), PathPoint { p: pos2(100.0, 0.0), ..Default::default() }], false)
            } else {
                Node::new(kind, Rect::from_min_size(Pos2::ZERO, vec2(100.0, 100.0)))
            };
            n.name = name;
            // Plugins set everything explicitly; start from blank paint like Figma's create* defaults.
            n.fills.clear();
            n.strokes.clear();
            if kind.is_frame() {
                n.clip = true;
            }
            doc.insert(page, None, None, n)
        };
        refs.nodes.insert(jid.clone(), id);
        fresh.push(jid);
    }
    // Structure: each loaded page's order, then each container's children.
    // Layers that change parent take the script's x/y relative to the new one, as in Figma.
    let mut moved: std::collections::HashSet<NodeId> = std::collections::HashSet::new();
    for (p, &page) in out_pages.iter().zip(&page_of) {
        let top: Vec<NodeId> = p["top"].as_array().into_iter().flatten().filter_map(|v| refs.nodes.get(v.as_str()?).copied()).collect();
        let loaded = p["loaded"].as_bool().unwrap_or(true);
        for (i, &id) in top.iter().enumerate() {
            let here = doc.node(id).parent.is_none() && doc.page_of(id) == page;
            if !here || (loaded && doc.index_in_parent(id) != i) {
                if doc.node(id).parent.is_some() {
                    moved.insert(id);
                }
                doc.reparent(id, page, None, Some(if loaded { i } else { doc.pages[page].children.len() }));
            }
        }
    }
    for v in &nodes {
        let Some(&id) = v["id"].as_str().and_then(|s| refs.nodes.get(s)) else { continue };
        let Some(kids) = v["childIds"].as_array() else { continue };
        if doc.node(id).kind == Kind::Instance && v.get("mainComponentId").is_some() && fresh.contains(&v["id"].as_str().unwrap_or("").to_string()) {
            continue;
        }
        for (i, k) in kids.iter().enumerate() {
            let Some(&c) = k.as_str().and_then(|s| refs.nodes.get(s)) else { continue };
            if doc.node(c).parent != Some(id) || doc.index_in_parent(c) != i {
                if doc.node(c).parent != Some(id) {
                    moved.insert(c);
                }
                let page = doc.page_of(id);
                doc.reparent(c, page, Some(id), Some(i));
            }
        }
    }
    // Properties, parents before children so relative positions resolve.
    let mut order: Vec<&Value> = nodes.iter().filter(|v| !provisional(v)).collect();
    order.sort_by_key(|v| v["id"].as_str().and_then(|s| refs.nodes.get(s)).map_or(0, |&i| doc.ancestors(i).len()));
    for v in order {
        let Some(&id) = v["id"].as_str().and_then(|s| refs.nodes.get(s)) else { continue };
        let base = bases.get(&id).cloned().unwrap_or(Value::Null);
        apply_props(doc, id, v, &base, moved.contains(&id), &refs);
    }
    // Instances: point at their main components, sync, then take the script's overrides.
    for v in &nodes {
        let Some(jid) = v["id"].as_str() else { continue };
        let Some(&inst) = refs.nodes.get(jid) else { continue };
        if v["type"] != "INSTANCE" {
            continue;
        }
        let comp = v["mainComponentId"].as_str().and_then(|c| refs.node(c)).filter(|c| doc.nodes.get(c).is_some_and(|n| n.kind == Kind::Component));
        let is_new = fresh.iter().any(|f| f == jid);
        match comp {
            Some(c) if is_new => {
                doc.node_mut(inst).kind = Kind::Instance;
                doc.node_mut(inst).component = Some(c);
                doc.sync_instance(inst);
                // A size the script gave the new instance (resize) wins over the component's.
                let cur = doc.node(inst).rect;
                let want = vec2(f(v, "width").map_or(cur.width(), |w| w as f32), f(v, "height").map_or(cur.height(), |h| h as f32));
                if (want - cur.size()).length() > 1e-3 {
                    for axis in 0..2 {
                        if want[axis] != cur.size()[axis] && doc.node(inst).sizing[axis] == Sizing::Hug {
                            doc.node_mut(inst).sizing[axis] = Sizing::Fixed;
                        }
                    }
                    doc.set_rect(inst, Rect::from_min_size(cur.min, want));
                }
            }
            Some(c) if doc.node(inst).component != Some(c) => doc.swap_component(inst, c),
            _ => {}
        }
        if v["__reset"].as_bool() == Some(true) {
            doc.reset_instance(inst);
        }
        if let Some(props) = v["__setProps"].as_object() {
            let comp = doc.node(inst).component;
            let set = comp.and_then(|c| doc.node(c).parent).filter(|p| doc.node(*p).kind == Kind::ComponentSet);
            let mut variant: Vec<(String, String)> = comp.map(|c| variant_pairs(&doc.node(c).name)).unwrap_or_default();
            let defs = comp.map(|c| doc.props_of(c)).unwrap_or_default();
            for (k, val) in props {
                let k = k.split('#').next().unwrap_or(k).to_string();
                if let Some(def) = defs.iter().find(|d| d.name == k) {
                    let pv = match (&def.default, val) {
                        (PropValue::Bool(_), Value::Bool(b)) => Some(PropValue::Bool(*b)),
                        (PropValue::Text(_), Value::String(s)) => Some(PropValue::Text(s.clone())),
                        (PropValue::Swap(_), Value::String(s)) => refs.node(s).map(PropValue::Swap),
                        _ => None,
                    };
                    if let Some(pv) = pv {
                        let n = doc.node_mut(inst);
                        n.prop_values.retain(|(name, _)| *name != k);
                        n.prop_values.push((k, pv));
                    }
                } else if let Some(s) = val.as_str() {
                    match variant.iter_mut().find(|(name, _)| *name == k) {
                        Some(slot) => slot.1 = s.to_string(),
                        None => variant.push((k, s.to_string())),
                    }
                }
            }
            if let Some(target) = set.and_then(|s| doc.find_variant(s, &variant))
                && Some(target) != comp
            {
                doc.swap_component(inst, target);
            }
        }
        // Provisional layers -> the synced layers they mirror; their edits become overrides.
        let mut stack: Vec<&Value> = v["childIds"].as_array().into_iter().flatten().filter_map(|c| by_id.get(c.as_str()?).copied()).collect();
        let mut matched = vec![];
        while let Some(p) = stack.pop() {
            stack.extend(p["childIds"].as_array().into_iter().flatten().filter_map(|c| by_id.get(c.as_str()?).copied()));
            let Some(src) = p["__src"].as_str().and_then(|s| refs.node(s)) else { continue };
            let found = doc.descendants(inst).into_iter().find(|&d| {
                let mut s = doc.node(d).source;
                for _ in 0..16 {
                    match s {
                        Some(x) if x == src => return true,
                        Some(x) => s = doc.nodes.get(&x).and_then(|n| n.source),
                        None => return false,
                    }
                }
                false
            });
            if let Some(d) = found {
                refs.nodes.insert(p["id"].as_str().unwrap_or("").to_string(), d);
                matched.push((d, p));
            }
        }
        for (d, p) in matched {
            let base = node_json(doc, d);
            apply_props(doc, d, p, &base, false, &refs);
        }
    }
    doc.reflow();
    refs.nodes
}
