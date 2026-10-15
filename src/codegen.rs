//! Dev Mode code: CSS, SwiftUI, and Jetpack Compose for a selected layer.

use crate::canvas::num;
use crate::doc::{
    AxisAlign, Bindable, BlendMode, Decoration, Document, EffectKind, Kind, LayoutMode, LineHeight, Node, NodeId, Paint, PaintKind, Rgba,
    Sizing, StrokeAlign, TextAlign, TextCase,
};
use std::fmt::Write;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Css,
    Tailwind,
    SwiftUi,
    Compose,
}

impl Lang {
    pub const ALL: [Lang; 4] = [Lang::Css, Lang::Tailwind, Lang::SwiftUi, Lang::Compose];

    pub fn label(self) -> &'static str {
        match self {
            Lang::Css => "CSS",
            Lang::Tailwind => "Tailwind",
            Lang::SwiftUi => "SwiftUI",
            Lang::Compose => "Compose",
        }
    }
}

/// The code for a variable-bound property, as Dev Mode writes it: the variable's code syntax for
/// the platform (or a name derived from its own), with the raw value as the CSS fallback.
fn bound(doc: &Document, n: &Node, prop: Bindable, platform: usize, raw: String) -> String {
    let Some(v) = n.bindings.iter().find(|(p, _)| *p == prop).and_then(|(_, v)| doc.variable(*v)) else { return raw };
    let name = if v.code_syntax[platform].is_empty() { code_name(&v.name, platform) } else { v.code_syntax[platform].clone() };
    match name.strip_prefix("var(").and_then(|x| x.strip_suffix(')')) {
        Some(inner) if platform == 0 && !inner.contains(',') => format!("var({inner}, {raw})"),
        _ => name,
    }
}

/// A linked style's name, for the comment Dev Mode puts above what it sets.
fn style_name(doc: &Document, id: Option<crate::doc::StyleId>) -> Option<&str> {
    id.and_then(|s| doc.style(s)).map(|s| s.name.as_str())
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
        PaintKind::Linear | PaintKind::Radial | PaintKind::Angular | PaintKind::Diamond => {
            let pct = |t: f32| format!("{}%", num(t * 100.0));
            let stops: Vec<String> = p.stops.iter().map(|(t, c)| format!("{} {}", css_color(*c), pct(*t))).collect();
            let at = format!("{} {}", pct(p.from.x), pct(p.from.y));
            // CSS angles: 0deg points up, clockwise.
            let d = p.to - p.from;
            let deg = (d.x.atan2(-d.y).to_degrees() + 360.0) % 360.0;
            match p.kind {
                PaintKind::Linear => format!("linear-gradient({}deg, {})", num(deg), stops.join(", ")),
                // Figma's angular starts toward the end handle and turns clockwise, as conic-gradient does.
                PaintKind::Angular => format!("conic-gradient(from {}deg at {at}, {})", num(deg), stops.join(", ")),
                _ => {
                    // CSS has no diamond gradient (or rotated ellipses): an axis-aligned ellipse is closest.
                    let w = p.width_point() - p.from;
                    let (rx, ry) = if d.x.abs() >= d.y.abs() { (d.length(), w.length()) } else { (w.length(), d.length()) };
                    format!("radial-gradient({} {} at {at}, {})", pct(rx), pct(ry), stops.join(", "))
                }
            }
        }
        PaintKind::Image => "url(image.png) center / cover no-repeat".into(),
    }
}

/// CSS declarations for one layer, positioned relative to its nearest frame.
pub fn css(doc: &Document, id: NodeId) -> String {
    let n = doc.node(id);
    let mut out = format!("/* {} */\n", n.name);
    macro_rules! decl {
        ($k:expr, $v:expr) => {{
            let v: String = $v;
            let _ = writeln!(out, "{}: {v};", $k);
        }};
    }
    macro_rules! note {
        ($s:expr) => {
            if let Some(s) = style_name(doc, $s) {
                let _ = writeln!(out, "/* {s} */");
            }
        };
    }
    let var = |p: Bindable, raw: String| bound(doc, n, p, 0, raw);
    let parent = n.parent.map(|p| doc.node(p));
    let in_flow = parent.is_some_and(|p| p.layout.mode != LayoutMode::None) && !n.absolute;
    if !in_flow {
        let origin = doc.ancestors(id).into_iter().find(|a| doc.node(*a).kind.is_frame()).map(|f| doc.node(f).rect.min).unwrap_or_default();
        decl!("position", "absolute".into());
        decl!("left", format!("{}px", num(n.rect.min.x - origin.x)));
        decl!("top", format!("{}px", num(n.rect.min.y - origin.y)));
    }
    if in_flow && parent.is_some_and(|p| p.layout.mode == LayoutMode::Grid) {
        for (span, prop) in n.grid_span.iter().zip(["grid-column", "grid-row"]) {
            if *span > 1 {
                decl!(prop, format!("span {span}"));
            }
        }
    }
    let (w, h) = (n.rect.width(), n.rect.height());
    match n.sizing[0] {
        Sizing::Fill if in_flow => decl!("flex", "1 0 0".into()),
        Sizing::Hug => decl!("width", "fit-content".into()),
        _ => decl!("width", var(Bindable::Width, format!("{}px", num(w)))),
    }
    match n.sizing[1] {
        Sizing::Fill if in_flow => decl!("align-self", "stretch".into()),
        Sizing::Hug => decl!("height", "fit-content".into()),
        _ => decl!("height", var(Bindable::Height, format!("{}px", num(h)))),
    }
    for (axis, dim, min, max) in [(0, "width", Bindable::MinWidth, Bindable::MaxWidth), (1, "height", Bindable::MinHeight, Bindable::MaxHeight)] {
        if let Some(v) = n.min_size[axis] {
            decl!(format!("min-{dim}"), var(min, format!("{}px", num(v))));
        }
        if let Some(v) = n.max_size[axis] {
            decl!(format!("max-{dim}"), var(max, format!("{}px", num(v))));
        }
    }
    if n.layout.mode == LayoutMode::Grid {
        let l = n.layout;
        let track = |t: Option<f32>| t.map_or("1fr".to_string(), |v| format!("{}px", num(v)));
        decl!("display", "grid".into());
        decl!("grid-template-columns", format!("repeat({}, {})", l.grid[0].max(1), track(l.tracks[0])));
        if l.grid[1] > 0 {
            decl!("grid-template-rows", format!("repeat({}, {})", l.grid[1], track(l.tracks[1])));
        }
        decl!("gap", format!("{}px {}px", num(l.cross_gap), num(l.gap)));
        let [t, r, b, lf] = l.padding;
        if [t, r, b, lf] != [0.0; 4] {
            decl!("padding", format!("{}px {}px {}px {}px", num(t), num(r), num(b), num(lf)));
        }
    } else if n.layout.mode != LayoutMode::None {
        let l = n.layout;
        decl!("display", "flex".into());
        decl!("flex-direction", if l.mode == LayoutMode::Horizontal { "row" } else { "column" }.into());
        let a = |x: AxisAlign| match x {
            AxisAlign::Min => "flex-start",
            AxisAlign::Baseline => "baseline",
            AxisAlign::Center => "center",
            AxisAlign::Max => "flex-end",
        };
        decl!("justify-content", if l.space_between { "space-between".into() } else { a(l.main).to_string() });
        decl!("align-items", a(l.cross).into());
        if !l.space_between && l.gap != 0.0 {
            decl!("gap", var(Bindable::Gap, format!("{}px", num(l.gap))));
        }
        if l.wrap && l.mode == LayoutMode::Horizontal {
            decl!("flex-wrap", "wrap".into());
            if l.cross_gap != 0.0 {
                decl!("row-gap", format!("{}px", num(l.cross_gap)));
            }
        }
        let [t, r, b, lf] = l.padding;
        if [t, r, b, lf] != [0.0; 4] {
            let raw = format!("{}px {}px {}px {}px", num(t), num(r), num(b), num(lf));
            decl!("padding", if t == r && r == b && b == lf { var(Bindable::Padding, format!("{}px", num(t))) } else { raw });
        }
    }
    if n.kind == Kind::Text {
        if let Some(c) = n.first_fill() {
            note!(n.fill_style);
            decl!("color", var(Bindable::Fill, css_color(c)));
        }
        note!(n.text_style);
        decl!("font-family", var(Bindable::FontFamily, format!("\"{}\"", n.font_family)));
        decl!("font-size", var(Bindable::FontSize, format!("{}px", num(n.font_size))));
        decl!("font-weight", var(Bindable::FontWeight, n.font_weight.to_string()));
        let lh = match n.line_height {
            LineHeight::Auto => "normal".into(),
            LineHeight::Px(px) => format!("{}px", num(px)),
            LineHeight::Percent(p) => format!("{}%", num(p)),
        };
        decl!("line-height", var(Bindable::LineHeight, lh));
        if n.letter_spacing != 0.0 {
            decl!("letter-spacing", var(Bindable::LetterSpacing, format!("{}px", num(n.letter_spacing / 100.0 * n.font_size))));
        }
        if n.text_align != TextAlign::Left {
            decl!("text-align", format!("{:?}", n.text_align).to_lowercase());
        }
        if n.paragraph_indent > 0.0 {
            decl!("text-indent", format!("{}px", num(n.paragraph_indent)));
        }
        if n.vertical_trim {
            decl!("text-box", "trim-both cap alphabetic".into());
        }
        match n.text_case {
            TextCase::Upper => decl!("text-transform", "uppercase".into()),
            TextCase::Lower => decl!("text-transform", "lowercase".into()),
            TextCase::Title => decl!("text-transform", "capitalize".into()),
            TextCase::Original => {}
        }
        match n.decoration {
            Decoration::Underline => decl!("text-decoration", "underline".into()),
            Decoration::Strikethrough => decl!("text-decoration", "line-through".into()),
            Decoration::None => {}
        }
        if n.truncate {
            decl!("overflow", "hidden".into());
            decl!("text-overflow", "ellipsis".into());
            if n.max_lines > 1 {
                decl!("display", "-webkit-box".into());
                decl!("-webkit-box-orient", "vertical".into());
                decl!("-webkit-line-clamp", n.max_lines.to_string());
            } else {
                decl!("white-space", "nowrap".into());
            }
        }
    } else {
        let fills: Vec<String> = n.fills.iter().rev().filter(|f| f.visible).map(css_paint).collect();
        if !fills.is_empty() {
            note!(n.fill_style);
        }
        match fills.as_slice() {
            [] => {}
            [one] => decl!("background", var(Bindable::Fill, one.clone())),
            many => decl!("background", many.join(", ")),
        }
        if n.fills.iter().any(|f| f.visible && f.blend != BlendMode::Normal) {
            let modes: Vec<&str> = n.fills.iter().rev().filter(|f| f.visible).map(|f| f.blend.css()).collect();
            decl!("background-blend-mode", modes.join(", "));
        }
    }
    if n.kind == Kind::Ellipse {
        decl!("border-radius", "50%".into());
    } else if let Some([tl, tr, br, bl]) = n.corner_radii {
        decl!("border-radius", format!("{}px {}px {}px {}px", num(tl), num(tr), num(br), num(bl)));
    } else if n.radius > 0.0 {
        decl!("border-radius", var(Bindable::Radius, format!("{}px", num(n.radius))));
    }
    if let Some(s) = n.strokes.iter().rev().find(|s| s.visible).filter(|_| n.stroke_width > 0.0) {
        let style = if n.dash.is_empty() { "solid" } else { "dashed" };
        note!(n.stroke_style);
        let border = format!("{} {style} {}", var(Bindable::StrokeWidth, format!("{}px", num(n.stroke_width))), var(Bindable::Stroke, css_paint(s)));
        if n.stroke_align == StrokeAlign::Outside {
            decl!("outline", border);
        } else {
            decl!("border", border);
        }
    }
    let shadows: Vec<String> = n
        .shadows
        .iter()
        .filter(|s| s.visible && s.kind.is_shadow())
        .enumerate()
        .map(|(i, s)| {
            let color = if i == 0 { var(Bindable::EffectColor, css_color(s.color)) } else { css_color(s.color) };
            format!(
                "{}{}px {}px {}px {}px {}",
                if s.kind == EffectKind::InnerShadow { "inset " } else { "" },
                num(s.offset.x),
                num(s.offset.y),
                num(s.blur),
                num(s.spread),
                color
            )
        })
        .collect();
    if !shadows.is_empty() {
        note!(n.effect_style);
        decl!("box-shadow", shadows.join(", "));
    }
    for s in n.shadows.iter().filter(|s| s.visible) {
        match s.kind {
            EffectKind::LayerBlur => decl!("filter", format!("blur({}px)", num(s.blur / 2.0))),
            EffectKind::BackgroundBlur => decl!("backdrop-filter", format!("blur({}px)", num(s.blur / 2.0))),
            _ => {}
        }
    }
    if n.opacity < 1.0 {
        decl!("opacity", var(Bindable::Opacity, num(n.opacity)));
    }
    if n.rotation != 0.0 {
        decl!("transform", format!("rotate({}deg)", num(-n.rotation)));
    }
    if n.blend != BlendMode::Normal {
        decl!("mix-blend-mode", n.blend.css().into());
    }
    if n.clip && n.kind.is_frame() {
        decl!("overflow", "hidden".into());
    }
    out
}

/// A Tailwind length: the spacing scale when it lands on a step (4px each), else an arbitrary value.
fn tw_len(prefix: &str, v: f32) -> String {
    let steps = [0.0, 0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 16.0, 20.0, 24.0, 28.0, 32.0, 36.0, 40.0, 44.0, 48.0, 52.0, 56.0, 60.0, 64.0, 72.0, 80.0, 96.0];
    match steps.iter().find(|s| **s * 4.0 == v) {
        Some(s) => format!("{prefix}-{}", num(*s)),
        None => format!("{prefix}-[{}px]", num(v)),
    }
}

/// A Tailwind arbitrary value: spaces become underscores.
fn tw(prefix: &str, v: &str) -> String {
    format!("{prefix}-[{}]", v.replace(' ', "_"))
}

/// Tailwind classes for one layer, on the element Dev Mode would write for it.
pub fn tailwind(doc: &Document, id: NodeId) -> String {
    let n = doc.node(id);
    let mut c: Vec<String> = vec![];
    let var = |p: Bindable, raw: String| bound(doc, n, p, 0, raw);
    // A bound property uses the variable; plain lengths use the spacing scale.
    let len = |prefix: &str, p: Bindable, v: f32| if n.bindings.iter().any(|(b, _)| *b == p) { tw(prefix, &var(p, format!("{}px", num(v)))) } else { tw_len(prefix, v) };
    let parent = n.parent.map(|p| doc.node(p));
    let in_flow = parent.is_some_and(|p| p.layout.mode != LayoutMode::None) && !n.absolute;
    if !in_flow {
        let origin = doc.ancestors(id).into_iter().find(|a| doc.node(*a).kind.is_frame()).map(|f| doc.node(f).rect.min).unwrap_or_default();
        c.push("absolute".into());
        c.push(tw_len("left", n.rect.min.x - origin.x));
        c.push(tw_len("top", n.rect.min.y - origin.y));
    }
    match n.sizing[0] {
        Sizing::Fill if in_flow => c.push("flex-1".into()),
        Sizing::Hug => c.push("w-fit".into()),
        _ => c.push(len("w", Bindable::Width, n.rect.width())),
    }
    match n.sizing[1] {
        Sizing::Fill if in_flow => c.push("self-stretch".into()),
        Sizing::Hug => c.push("h-fit".into()),
        _ => c.push(len("h", Bindable::Height, n.rect.height())),
    }
    for (axis, d, min, max) in [(0, "w", Bindable::MinWidth, Bindable::MaxWidth), (1, "h", Bindable::MinHeight, Bindable::MaxHeight)] {
        if let Some(v) = n.min_size[axis] {
            c.push(len(&format!("min-{d}"), min, v));
        }
        if let Some(v) = n.max_size[axis] {
            c.push(len(&format!("max-{d}"), max, v));
        }
    }
    if n.layout.mode == LayoutMode::Grid {
        let l = n.layout;
        c.push("grid".into());
        c.push(format!("grid-cols-{}", l.grid[0].max(1)));
        if l.grid[1] > 0 {
            c.push(format!("grid-rows-{}", l.grid[1]));
        }
        c.push(format!("gap-x-[{}px]", num(l.gap)));
        c.push(format!("gap-y-[{}px]", num(l.cross_gap)));
    } else if n.layout.mode != LayoutMode::None {
        let l = n.layout;
        c.push("flex".into());
        c.push(if l.mode == LayoutMode::Horizontal { "flex-row" } else { "flex-col" }.into());
        let a = |x: AxisAlign| match x {
            AxisAlign::Min => "start",
            AxisAlign::Center => "center",
            AxisAlign::Max => "end",
            AxisAlign::Baseline => "baseline",
        };
        c.push(if l.space_between { "justify-between".into() } else { format!("justify-{}", a(l.main)) });
        c.push(format!("items-{}", a(l.cross)));
        if !l.space_between && l.gap != 0.0 {
            c.push(len("gap", Bindable::Gap, l.gap));
        }
        if l.wrap && l.mode == LayoutMode::Horizontal {
            c.push("flex-wrap".into());
        }
        let [t, r, b, lf] = l.padding;
        if t == r && r == b && b == lf {
            if t != 0.0 {
                c.push(len("p", Bindable::Padding, t));
            }
        } else {
            for (p, v) in [("pt", t), ("pr", r), ("pb", b), ("pl", lf)] {
                if v != 0.0 {
                    c.push(tw_len(p, v));
                }
            }
        }
    }
    let text = n.kind == Kind::Text;
    if text {
        if let Some(col) = n.first_fill() {
            c.push(tw("text", &var(Bindable::Fill, css_color(col))));
        }
        c.push(tw("font", &format!("'{}'", n.font_family)));
        // Font sizes aren't on the spacing scale; a variable needs Tailwind's length hint.
        let size = var(Bindable::FontSize, format!("{}px", num(n.font_size)));
        c.push(if size.starts_with("var(") { tw("text", &format!("length:{size}")) } else { tw("text", &size) });
        c.push(tw("font", &var(Bindable::FontWeight, n.font_weight.to_string())));
        match n.line_height {
            LineHeight::Auto => {}
            LineHeight::Px(px) => c.push(tw("leading", &var(Bindable::LineHeight, format!("{}px", num(px))))),
            LineHeight::Percent(p) => c.push(tw("leading", &num(p / 100.0))),
        }
        if n.letter_spacing != 0.0 {
            c.push(tw("tracking", &format!("{}px", num(n.letter_spacing / 100.0 * n.font_size))));
        }
        match n.text_align {
            TextAlign::Left => {}
            a => c.push(format!("text-{}", format!("{a:?}").to_lowercase())),
        }
        match n.text_case {
            TextCase::Upper => c.push("uppercase".into()),
            TextCase::Lower => c.push("lowercase".into()),
            TextCase::Title => c.push("capitalize".into()),
            TextCase::Original => {}
        }
        match n.decoration {
            Decoration::Underline => c.push("underline".into()),
            Decoration::Strikethrough => c.push("line-through".into()),
            Decoration::None => {}
        }
        if n.truncate {
            c.push(if n.max_lines > 1 { format!("line-clamp-{}", n.max_lines) } else { "truncate".into() });
        }
    } else if let [one] = n.fills.iter().filter(|f| f.visible).collect::<Vec<_>>()[..] {
        c.push(match one.kind {
            PaintKind::Solid => tw("bg", &var(Bindable::Fill, css_color(one.color))),
            _ => tw("background", &css_paint(one)),
        });
    }
    if n.kind == Kind::Ellipse {
        c.push("rounded-full".into());
    } else if let Some([tl, tr, br, bl]) = n.corner_radii {
        for (k, v) in [("tl", tl), ("tr", tr), ("br", br), ("bl", bl)] {
            c.push(format!("rounded-{k}-[{}px]", num(v)));
        }
    } else if n.radius > 0.0 {
        c.push(tw("rounded", &var(Bindable::Radius, format!("{}px", num(n.radius)))));
    }
    if let Some(s) = n.strokes.iter().rev().find(|s| s.visible && s.kind == PaintKind::Solid).filter(|_| n.stroke_width > 0.0) {
        c.push(if n.stroke_width == 1.0 { "border".into() } else { tw("border", &format!("{}px", num(n.stroke_width))) });
        c.push(tw("border", &var(Bindable::Stroke, css_color(s.color))));
        if !n.dash.is_empty() {
            c.push("border-dashed".into());
        }
    }
    let shadows: Vec<String> = n
        .shadows
        .iter()
        .filter(|s| s.visible && s.kind.is_shadow())
        .map(|s| format!("{}{}px_{}px_{}px_{}px_{}", if s.kind == EffectKind::InnerShadow { "inset_" } else { "" }, num(s.offset.x), num(s.offset.y), num(s.blur), num(s.spread), css_color(s.color).replace(' ', "")))
        .collect();
    if !shadows.is_empty() {
        c.push(format!("shadow-[{}]", shadows.join(",")));
    }
    for s in n.shadows.iter().filter(|s| s.visible) {
        match s.kind {
            EffectKind::LayerBlur => c.push(format!("blur-[{}px]", num(s.blur / 2.0))),
            EffectKind::BackgroundBlur => c.push(format!("backdrop-blur-[{}px]", num(s.blur / 2.0))),
            _ => {}
        }
    }
    if n.opacity < 1.0 {
        c.push(tw("opacity", &var(Bindable::Opacity, num(n.opacity))));
    }
    if n.rotation != 0.0 {
        c.push(format!("rotate-[{}deg]", num(-n.rotation)));
    }
    if n.clip && n.kind.is_frame() {
        c.push("overflow-hidden".into());
    }
    let class = c.join(" ");
    if text {
        format!("<p class=\"{class}\">{}</p>", n.text)
    } else {
        format!("<div class=\"{class}\"></div>")
    }
}

fn swift_color(c: Rgba) -> String {
    let f = |v: u8| num(v as f32 / 255.0);
    let alpha = if c.a() == 255 { String::new() } else { format!(", opacity: {}", f(c.a())) };
    format!("Color(red: {}, green: {}, blue: {}{alpha})", f(c.0[0]), f(c.0[1]), f(c.0[2]))
}

pub fn swiftui(doc: &Document, id: NodeId) -> String {
    let n = doc.node(id);
    let fill = n.fills.iter().rev().find(|f| f.visible && f.kind == PaintKind::Solid).map(|f| f.color);
    let swift_color = |c: Rgba| bound(doc, n, Bindable::Fill, 1, swift_color(c));
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
    let fill_color = |c: Rgba| bound(doc, n, Bindable::Fill, 2, compose_color(c));
    if n.kind == Kind::Text {
        let mut t = format!(
            "Text(\n    text = {:?},\n    fontSize = {}.sp,\n    fontWeight = FontWeight.W{},\n    fontFamily = FontFamily({:?})",
            n.text,
            num(n.font_size),
            (n.font_weight / 100 * 100).clamp(100, 900),
            n.font_family
        );
        if let Some(c) = fill {
            let _ = write!(t, ",\n    color = {}", fill_color(c));
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
        let _ = write!(m, "\n    .background({}, {shape})", fill_color(c));
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
        LayoutMode::Grid => format!(
            "LazyVerticalGrid(\n    columns = GridCells.Fixed({}),\n    modifier = {m},\n    horizontalArrangement = Arrangement.spacedBy({}.dp),\n    verticalArrangement = Arrangement.spacedBy({}.dp)\n) {{ }}",
            n.layout.grid[0].max(1),
            num(n.layout.gap),
            num(n.layout.cross_gap)
        ),
        LayoutMode::None => format!("Box(\n    modifier = {m}\n)"),
    }
}

pub fn generate(doc: &Document, id: NodeId, lang: Lang) -> String {
    match lang {
        Lang::Css => css(doc, id),
        Lang::Tailwind => tailwind(doc, id),
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

    #[test]
    fn angular_is_conic_and_gradients_roundtrip_through_plugins() {
        let mut p = crate::doc::Paint::solid(crate::doc::Rgba::rgb(255, 0, 0));
        p.convert(PaintKind::Angular);
        assert!(css_paint(&p).starts_with("conic-gradient(from 90deg at 50% 50%, #FF0000 0%"), "{}", css_paint(&p));
        p.convert(PaintKind::Diamond);
        p.width_to = Some(pos2(0.5, 0.75));
        let v = crate::plugin_doc::paint_json(&p, None);
        assert_eq!(v["type"], "GRADIENT_DIAMOND");
        let back = crate::plugin_doc::paint_from(&v, &Document::default(), None).unwrap();
        assert_eq!(back.kind, PaintKind::Diamond);
        assert!(back.width_to.is_some_and(|w| w.distance(pos2(0.5, 0.75)) < 1e-4) && back.to.distance(p.to) < 1e-4);
    }

    #[test]
    fn code_uses_variables_styles_and_tailwind() {
        use crate::doc::{Bindable, Paint, StyleValue, VarValue};
        let mut d = Document::default();
        let c = d.add_collection("Theme".into());
        let primary = d.add_variable(c, "color/Primary".into(), VarValue::Color(Rgba::rgb(255, 0, 0)));
        let space = d.add_variable(c, "space/md".into(), VarValue::Number(12.0));
        d.variables[1].code_syntax = ["var(--gap-md)".into(), "Spacing.md".into(), "spacing_md".into()];
        let brand = d.add_style("Brand / Red".into(), StyleValue::Paint(vec![Paint::solid(Rgba::rgb(255, 0, 0))]));
        let mut f = Node::new(Kind::Frame, Rect::from_min_size(pos2(0., 0.), vec2(300., 100.)));
        f.layout = AutoLayout { mode: LayoutMode::Horizontal, gap: 12., padding: [16.; 4], cross: AxisAlign::Center, ..Default::default() };
        f.bindings = vec![(Bindable::Fill, primary), (Bindable::Gap, space)];
        f.fill_style = Some(brand);
        f.radius = 8.;
        let f = d.insert(0, None, None, f);
        d.reflow();
        let css = css(&d, f);
        for want in ["/* Brand / Red */\nbackground: var(--color-primary, #FF0000);", "gap: var(--gap-md, 12px);"] {
            assert!(css.contains(want), "missing {want}:\n{css}");
        }
        let mut dot = Node::new(Kind::Ellipse, Rect::from_min_size(pos2(0., 0.), vec2(10., 10.)));
        dot.bindings = vec![(Bindable::Fill, primary)];
        let dot = d.insert(0, None, None, dot);
        assert!(swiftui(&d, dot).contains(".fill(colorPrimary)") && compose(&d, f).contains(".background(color_primary,"), "native code uses the variable names");
        let tw = tailwind(&d, f);
        for want in ["flex ", "flex-row", "items-center", "gap-[var(--gap-md,_12px)]", "p-4", "w-[300px]", "h-[100px]", "rounded-lg", "bg-[var(--color-primary,_#FF0000)]"] {
            assert!(tw.contains(want) || (want == "rounded-lg" && tw.contains("rounded-[8px]")), "missing {want}: {tw}");
        }
    }
}
