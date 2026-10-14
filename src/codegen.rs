//! Dev Mode code: CSS, SwiftUI, and Jetpack Compose for a selected layer.

use crate::canvas::num;
use crate::doc::{
    AxisAlign, BlendMode, Decoration, Document, EffectKind, Kind, LayoutMode, LineHeight, NodeId, Paint, PaintKind, Rgba, Sizing,
    StrokeAlign, TextAlign, TextCase,
};
use std::fmt::Write;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Css,
    SwiftUi,
    Compose,
}

/// A variable's default code name per platform (0 Web, 1 iOS, 2 Android), from its slash name:
/// "color/brand primary" is `var(--color-brand-primary)`, `colorBrandPrimary`, `color_brand_primary`.
pub fn code_name(name: &str, platform: usize) -> String {
    let words: Vec<String> = name.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(|w| w.to_lowercase()).collect();
    match platform {
        0 => format!("var(--{})", words.join("-")),
        1 => words.iter().enumerate().map(|(i, w)| if i == 0 { w.clone() } else { w[..1].to_uppercase() + &w[1..] }).collect(),
        _ => words.join("_"),
    }
}

fn css_color(c: Rgba) -> String {
    if c.a() == 255 { format!("#{}", c.hex()) } else { format!("rgba({}, {}, {}, {})", c.0[0], c.0[1], c.0[2], num(c.a() as f32 / 255.0)) }
}

fn css_paint(p: &Paint) -> String {
    match p.kind {
        PaintKind::Solid => css_color(p.color),
        PaintKind::Linear | PaintKind::Radial => {
            let stops: Vec<String> = p.stops.iter().map(|(t, c)| format!("{} {}%", css_color(*c), num(t * 100.0))).collect();
            if p.kind == PaintKind::Linear {
                // CSS angles: 0deg points up, clockwise.
                let d = p.to - p.from;
                let deg = (d.x.atan2(-d.y).to_degrees() + 360.0) % 360.0;
                format!("linear-gradient({}deg, {})", num(deg), stops.join(", "))
            } else {
                format!("radial-gradient(50% 50% at {}% {}%, {})", num(p.from.x * 100.0), num(p.from.y * 100.0), stops.join(", "))
            }
        }
        PaintKind::Image => "url(image.png) center / cover no-repeat".into(),
    }
}

/// CSS declarations for one layer, positioned relative to its nearest frame.
pub fn css(doc: &Document, id: NodeId) -> String {
    let n = doc.node(id);
    let mut out = format!("/* {} */\n", n.name);
    let mut decl = |k: &str, v: String| {
        let _ = writeln!(out, "{k}: {v};");
    };
    let parent = n.parent.map(|p| doc.node(p));
    let in_flow = parent.is_some_and(|p| p.layout.mode != LayoutMode::None) && !n.absolute;
    if !in_flow {
        let origin = doc.ancestors(id).into_iter().find(|a| doc.node(*a).kind.is_frame()).map(|f| doc.node(f).rect.min).unwrap_or_default();
        decl("position", "absolute".into());
        decl("left", format!("{}px", num(n.rect.min.x - origin.x)));
        decl("top", format!("{}px", num(n.rect.min.y - origin.y)));
    }
    let (w, h) = (n.rect.width(), n.rect.height());
    match n.sizing[0] {
        Sizing::Fill if in_flow => decl("flex", "1 0 0".into()),
        Sizing::Hug => decl("width", "fit-content".into()),
        _ => decl("width", format!("{}px", num(w))),
    }
    match n.sizing[1] {
        Sizing::Fill if in_flow => decl("align-self", "stretch".into()),
        Sizing::Hug => decl("height", "fit-content".into()),
        _ => decl("height", format!("{}px", num(h))),
    }
    for (axis, dim) in [(0, "width"), (1, "height")] {
        if let Some(v) = n.min_size[axis] {
            decl(&format!("min-{dim}"), format!("{}px", num(v)));
        }
        if let Some(v) = n.max_size[axis] {
            decl(&format!("max-{dim}"), format!("{}px", num(v)));
        }
    }
    if n.layout.mode != LayoutMode::None {
        let l = n.layout;
        decl("display", "flex".into());
        decl("flex-direction", if l.mode == LayoutMode::Horizontal { "row" } else { "column" }.into());
        let a = |x: AxisAlign| match x {
            AxisAlign::Min => "flex-start",
            AxisAlign::Center => "center",
            AxisAlign::Max => "flex-end",
        };
        decl("justify-content", if l.space_between { "space-between".into() } else { a(l.main).to_string() });
        decl("align-items", a(l.cross).into());
        if !l.space_between && l.gap != 0.0 {
            decl("gap", format!("{}px", num(l.gap)));
        }
        if l.wrap && l.mode == LayoutMode::Horizontal {
            decl("flex-wrap", "wrap".into());
            if l.cross_gap != 0.0 {
                decl("row-gap", format!("{}px", num(l.cross_gap)));
            }
        }
        let [t, r, b, lf] = l.padding;
        if [t, r, b, lf] != [0.0; 4] {
            decl("padding", format!("{}px {}px {}px {}px", num(t), num(r), num(b), num(lf)));
        }
    }
    if n.kind == Kind::Text {
        if let Some(c) = n.first_fill() {
            decl("color", css_color(c));
        }
        decl("font-family", format!("\"{}\"", n.font_family));
        decl("font-size", format!("{}px", num(n.font_size)));
        decl("font-weight", n.font_weight.to_string());
        decl(
            "line-height",
            match n.line_height {
                LineHeight::Auto => "normal".into(),
                LineHeight::Px(px) => format!("{}px", num(px)),
                LineHeight::Percent(p) => format!("{}%", num(p)),
            },
        );
        if n.letter_spacing != 0.0 {
            decl("letter-spacing", format!("{}px", num(n.letter_spacing / 100.0 * n.font_size)));
        }
        if n.text_align != TextAlign::Left {
            decl("text-align", format!("{:?}", n.text_align).to_lowercase());
        }
        match n.text_case {
            TextCase::Upper => decl("text-transform", "uppercase".into()),
            TextCase::Lower => decl("text-transform", "lowercase".into()),
            TextCase::Title => decl("text-transform", "capitalize".into()),
            TextCase::Original => {}
        }
        match n.decoration {
            Decoration::Underline => decl("text-decoration", "underline".into()),
            Decoration::Strikethrough => decl("text-decoration", "line-through".into()),
            Decoration::None => {}
        }
        if n.truncate {
            decl("overflow", "hidden".into());
            decl("text-overflow", "ellipsis".into());
            if n.max_lines > 1 {
                decl("display", "-webkit-box".into());
                decl("-webkit-box-orient", "vertical".into());
                decl("-webkit-line-clamp", n.max_lines.to_string());
            } else {
                decl("white-space", "nowrap".into());
            }
        }
    } else {
        let fills: Vec<String> = n.fills.iter().rev().filter(|f| f.visible).map(css_paint).collect();
        match fills.as_slice() {
            [] => {}
            [one] => decl("background", one.clone()),
            many => decl("background", many.join(", ")),
        }
    }
    if n.kind == Kind::Ellipse {
        decl("border-radius", "50%".into());
    } else if let Some([tl, tr, br, bl]) = n.corner_radii {
        decl("border-radius", format!("{}px {}px {}px {}px", num(tl), num(tr), num(br), num(bl)));
    } else if n.radius > 0.0 {
        decl("border-radius", format!("{}px", num(n.radius)));
    }
    if let Some(s) = n.strokes.iter().rev().find(|s| s.visible).filter(|_| n.stroke_width > 0.0) {
        let style = if n.dash.is_empty() { "solid" } else { "dashed" };
        let border = format!("{}px {style} {}", num(n.stroke_width), css_paint(s));
        if n.stroke_align == StrokeAlign::Outside {
            decl("outline", border);
        } else {
            decl("border", border);
        }
    }
    let shadows: Vec<String> = n
        .shadows
        .iter()
        .filter(|s| s.visible && s.kind.is_shadow())
        .map(|s| {
            format!(
                "{}{}px {}px {}px {}px {}",
                if s.kind == EffectKind::InnerShadow { "inset " } else { "" },
                num(s.offset.x),
                num(s.offset.y),
                num(s.blur),
                num(s.spread),
                css_color(s.color)
            )
        })
        .collect();
    if !shadows.is_empty() {
        decl("box-shadow", shadows.join(", "));
    }
    for s in n.shadows.iter().filter(|s| s.visible) {
        match s.kind {
            EffectKind::LayerBlur => decl("filter", format!("blur({}px)", num(s.blur / 2.0))),
            EffectKind::BackgroundBlur => decl("backdrop-filter", format!("blur({}px)", num(s.blur / 2.0))),
            _ => {}
        }
    }
    if n.opacity < 1.0 {
        decl("opacity", num(n.opacity));
    }
    if n.rotation != 0.0 {
        decl("transform", format!("rotate({}deg)", num(-n.rotation)));
    }
    if n.blend != BlendMode::Normal {
        decl("mix-blend-mode", n.blend.css().into());
    }
    if n.clip && n.kind.is_frame() {
        decl("overflow", "hidden".into());
    }
    out
}

fn swift_color(c: Rgba) -> String {
    let f = |v: u8| num(v as f32 / 255.0);
    let alpha = if c.a() == 255 { String::new() } else { format!(", opacity: {}", f(c.a())) };
    format!("Color(red: {}, green: {}, blue: {}{alpha})", f(c.0[0]), f(c.0[1]), f(c.0[2]))
}

pub fn swiftui(doc: &Document, id: NodeId) -> String {
    let n = doc.node(id);
    let fill = n.fills.iter().rev().find(|f| f.visible && f.kind == PaintKind::Solid).map(|f| f.color);
    let mut s = match n.kind {
        Kind::Text => {
            let weight = match n.font_weight {
                0..=349 => ".light",
                350..=449 => ".regular",
                450..=549 => ".medium",
                550..=649 => ".semibold",
                _ => ".bold",
            };
            let mut t = format!("Text({:?})\n    .font(.custom({:?}, size: {}).weight({weight}))", n.text, n.font_family, num(n.font_size));
            if let Some(c) = fill {
                let _ = write!(t, "\n    .foregroundColor({})", swift_color(c));
            }
            t
        }
        Kind::Ellipse => format!("Ellipse()\n    .fill({})", fill.map_or("Color.clear".into(), swift_color)),
        _ if n.layout.mode != LayoutMode::None => {
            let stack = if n.layout.mode == LayoutMode::Horizontal { "HStack" } else { "VStack" };
            format!("{stack}(spacing: {}) {{\n    // {} children\n}}", num(n.layout.gap), n.children.len())
        }
        _ => {
            let shape = if n.radius > 0.0 { format!("RoundedRectangle(cornerRadius: {})", num(n.radius)) } else { "Rectangle()".into() };
            format!("{shape}\n    .fill({})", fill.map_or("Color.clear".into(), swift_color))
        }
    };
    if n.kind != Kind::Text {
        let _ = write!(s, "\n    .frame(width: {}, height: {})", num(n.rect.width()), num(n.rect.height()));
    }
    if let Some(sh) = n.shadows.iter().find(|s| s.visible && s.kind == EffectKind::DropShadow) {
        let _ = write!(s, "\n    .shadow(color: {}, radius: {}, x: {}, y: {})", swift_color(sh.color), num(sh.blur / 2.0), num(sh.offset.x), num(sh.offset.y));
    }
    if n.opacity < 1.0 {
        let _ = write!(s, "\n    .opacity({})", num(n.opacity));
    }
    if n.rotation != 0.0 {
        let _ = write!(s, "\n    .rotationEffect(.degrees({}))", num(-n.rotation));
    }
    s
}

fn compose_color(c: Rgba) -> String {
    format!("Color(0x{:02X}{})", c.a(), c.hex())
}

pub fn compose(doc: &Document, id: NodeId) -> String {
    let n = doc.node(id);
    let fill = n.fills.iter().rev().find(|f| f.visible && f.kind == PaintKind::Solid).map(|f| f.color);
    if n.kind == Kind::Text {
        let mut t = format!(
            "Text(\n    text = {:?},\n    fontSize = {}.sp,\n    fontWeight = FontWeight.W{},\n    fontFamily = FontFamily({:?})",
            n.text,
            num(n.font_size),
            (n.font_weight / 100 * 100).clamp(100, 900),
            n.font_family
        );
        if let Some(c) = fill {
            let _ = write!(t, ",\n    color = {}", compose_color(c));
        }
        t.push_str("\n)");
        return t;
    }
    let shape = match n.kind {
        Kind::Ellipse => "CircleShape".to_string(),
        _ if n.radius > 0.0 => format!("RoundedCornerShape({}.dp)", num(n.radius)),
        _ => "RectangleShape".into(),
    };
    let mut m = format!("Modifier\n    .size(width = {}.dp, height = {}.dp)", num(n.rect.width()), num(n.rect.height()));
    if let Some(sh) = n.shadows.iter().find(|s| s.visible && s.kind == EffectKind::DropShadow) {
        let _ = write!(m, "\n    .shadow({}.dp, {shape})", num(sh.blur / 2.0));
    }
    if let Some(c) = fill {
        let _ = write!(m, "\n    .background({}, {shape})", compose_color(c));
    }
    if let Some(s) = n.strokes.iter().rev().find(|s| s.visible && s.kind == PaintKind::Solid) {
        let _ = write!(m, "\n    .border({}.dp, {}, {shape})", num(n.stroke_width), compose_color(s.color));
    }
    if n.opacity < 1.0 {
        let _ = write!(m, "\n    .alpha({}f)", num(n.opacity));
    }
    match n.layout.mode {
        LayoutMode::Horizontal => format!("Row(\n    modifier = {m},\n    horizontalArrangement = Arrangement.spacedBy({}.dp)\n) {{ }}", num(n.layout.gap)),
        LayoutMode::Vertical => format!("Column(\n    modifier = {m},\n    verticalArrangement = Arrangement.spacedBy({}.dp)\n) {{ }}", num(n.layout.gap)),
        LayoutMode::None => format!("Box(\n    modifier = {m}\n)"),
    }
}

pub fn generate(doc: &Document, id: NodeId, lang: Lang) -> String {
    match lang {
        Lang::Css => css(doc, id),
        Lang::SwiftUi => swiftui(doc, id),
        Lang::Compose => compose(doc, id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::{AutoLayout, Node, Shadow};
    use egui::{Rect, pos2, vec2};

    #[test]
    fn css_covers_layout_type_and_effects() {
        let mut d = Document::default();
        let mut f = Node::new(Kind::Frame, Rect::from_min_size(pos2(0., 0.), vec2(300., 100.)));
        f.layout = AutoLayout { mode: LayoutMode::Horizontal, gap: 12., padding: [8., 16., 8., 16.], cross: AxisAlign::Center, ..Default::default() };
        f.radius = 8.;
        f.shadows = vec![Shadow::default()];
        let f = d.insert(0, None, None, f);
        let mut t = Node::new(Kind::Text, Rect::from_min_size(pos2(16., 8.), vec2(50., 20.)));
        t.text = "Hi".into();
        t.font_weight = 600;
        t.text_case = TextCase::Upper;
        let t = d.insert(0, Some(f), None, t);
        let css = css(&d, f);
        for want in ["display: flex;", "flex-direction: row;", "gap: 12px;", "padding: 8px 16px 8px 16px;", "align-items: center;", "border-radius: 8px;", "box-shadow: 0px 4px 4px 0px rgba(0, 0, 0, 0.25);", "background: #FFFFFF;"] {
            assert!(css.contains(want), "missing {want}:\n{css}");
        }
        let tc = super::css(&d, t);
        assert!(tc.contains("font-weight: 600;") && tc.contains("text-transform: uppercase;") && !tc.contains("position"), "{tc}");
        assert!(swiftui(&d, t).contains("Text(\"Hi\")") && swiftui(&d, t).contains(".semibold"));
        assert!(compose(&d, f).starts_with("Row("));
    }
}
