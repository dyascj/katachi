//! SVG export from the model; PNG export rasterizes that SVG with resvg so both always match.

use crate::doc::{BlendMode, Cap, Decoration, Document, EffectKind, Join, Kind, Node, NodeId, Paint, PaintKind, Rgba, Shadow, StrokeAlign, TextAlign};
use egui::{Pos2, Rect, Vec2};
use std::fmt::Write;

pub const FONTS: [&[u8]; 4] = [
    include_bytes!("../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../assets/fonts/Inter-SemiBold.ttf"),
    include_bytes!("../assets/fonts/Inter-Bold.ttf"),
];


/// `#RRGGBB` plus opacity (1.0 = opaque).
fn color_parts(c: Rgba) -> (String, f32) {
    (format!("#{}", c.hex()), c.a() as f32 / 255.0)
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Shape element for `n` grown by `grow` units, `attrs` appended.
fn shape(n: &Node, o: Vec2, grow: f32, attrs: &str) -> String {
    let r = n.rect.translate(-o).expand(grow);
    match n.kind {
        Kind::Ellipse => format!(
            r#"<ellipse cx="{}" cy="{}" rx="{}" ry="{}" {attrs}/>"#,
            r.center().x,
            r.center().y,
            r.width().max(0.0) / 2.0,
            r.height().max(0.0) / 2.0
        ),
        Kind::Polygon | Kind::Star => {
            let c = n.rect.center();
            let pts: Vec<String> = n
                .star_points()
                .iter()
                .map(|p| {
                    let q = *p + (*p - c).normalized() * grow - o;
                    format!("{} {}", q.x, q.y)
                })
                .collect();
            format!(r#"<polygon points="{}" {attrs}/>"#, pts.join(" "))
        }
        Kind::Vector => format!(
            r#"<path d="{}"{} {attrs}/>"#,
            path_data(n, o),
            if n.even_odd { r#" fill-rule="evenodd""# } else { "" }
        ),
        _ => {
            let lim = r.width().min(r.height()).max(0.0) / 2.0;
            let [tl, tr, br, bl] = n.radii().map(|x| (x + grow).clamp(0.0, lim));
            if tl == tr && tr == br && br == bl {
                format!(
                    r#"<rect x="{}" y="{}" width="{}" height="{}"{} {attrs}/>"#,
                    r.min.x,
                    r.min.y,
                    r.width().max(0.0),
                    r.height().max(0.0),
                    if tl > 0.0 { format!(r#" rx="{tl}""#) } else { String::new() }
                )
            } else {
                // Independent corners need a path with an arc per corner.
                let (x0, y0, x1, y1) = (r.min.x, r.min.y, r.max.x, r.max.y);
                format!(
                    r#"<path d="M{} {y0}H{}A{tr} {tr} 0 0 1 {x1} {}V{}A{br} {br} 0 0 1 {} {y1}H{}A{bl} {bl} 0 0 1 {x0} {}V{}A{tl} {tl} 0 0 1 {} {y0}Z" {attrs}/>"#,
                    x0 + tl,
                    x1 - tr,
                    y0 + tr,
                    y1 - br,
                    x1 - br,
                    x0 + bl,
                    y1 - bl,
                    y0 + tl,
                    x0 + tl
                )
            }
        }
    }
}

pub fn path_data(n: &Node, o: Vec2) -> String {
    let base = n.rect.min.to_vec2() - o;
    let p = |q: Pos2| {
        let a = q + base;
        format!("{} {}", a.x, a.y)
    };
    let mut d = String::new();
    for sp in n.paths.iter().filter(|sp| !sp.points.is_empty()) {
        let pts = &sp.points;
        let _ = write!(d, "M{}", p(pts[0].p));
        let segs = if sp.closed { pts.len() } else { pts.len() - 1 };
        for i in 0..segs {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            if a.h_out == Vec2::ZERO && b.h_in == Vec2::ZERO {
                let _ = write!(d, "L{}", p(b.p));
            } else {
                let _ = write!(d, "C{} {} {}", p(a.p + a.h_out), p(b.p + b.h_in), p(b.p));
            }
        }
        if sp.closed {
            d.push('Z');
        }
    }
    d
}

/// Dash, cap, and join attributes beyond SVG's defaults.
fn stroke_style(n: &Node) -> String {
    let mut s = String::new();
    if n.dash.iter().any(|d| *d > 0.0) {
        let _ = write!(s, r#" stroke-dasharray="{}""#, n.dash.iter().map(|d| d.to_string()).collect::<Vec<_>>().join(" "));
    }
    match n.cap {
        Cap::Round => s.push_str(r#" stroke-linecap="round""#),
        Cap::Square => s.push_str(r#" stroke-linecap="square""#),
        Cap::None => {}
    }
    match n.join {
        Join::Round => s.push_str(r#" stroke-linejoin="round""#),
        Join::Bevel => s.push_str(r#" stroke-linejoin="bevel""#),
        Join::Miter => {}
    }
    s
}

/// How far shadows reach past a layer's bounds.
fn shadow_reach(n: &Node) -> f32 {
    n.shadows
        .iter()
        .filter(|s| s.visible && s.kind != EffectKind::BackgroundBlur)
        .map(|s| s.blur * 2.0 + s.spread.max(0.0) + s.offset.x.abs().max(s.offset.y.abs()))
        .fold(0.0, f32::max)
}

struct Svg<'a> {
    doc: &'a Document,
    o: Vec2,
    out: String,
    defs: String,
    next: usize,
}

impl Svg<'_> {
    fn id(&mut self, prefix: &str) -> String {
        self.next += 1;
        format!("{prefix}{}", self.next)
    }

    /// `fill="..."`-style attributes for a paint, adding gradient/pattern defs as needed.
    fn paint(&mut self, name: &str, p: &Paint, n: &Node) -> String {
        let r = n.rect.translate(-self.o);
        match p.kind {
            PaintKind::Solid => {
                let (c, a) = color_parts(p.color);
                if a < 1.0 { format!(r#"{name}="{c}" {name}-opacity="{a:.3}""#) } else { format!(r#"{name}="{c}""#) }
            }
            PaintKind::Linear | PaintKind::Radial => {
                let id = self.id("paint");
                // Defined in the layer's unit square, mapped onto it.
                let m = format!("matrix({} 0 0 {} {} {})", r.width().max(1e-3), r.height().max(1e-3), r.min.x, r.min.y);
                let geo = if p.kind == PaintKind::Linear {
                    format!(r#"<linearGradient id="{id}" x1="{}" y1="{}" x2="{}" y2="{}""#, p.from.x, p.from.y, p.to.x, p.to.y)
                } else {
                    format!(r#"<radialGradient id="{id}" cx="{}" cy="{}" r="{}""#, p.from.x, p.from.y, (p.to - p.from).length())
                };
                let _ = write!(self.defs, r#"{geo} gradientUnits="userSpaceOnUse" gradientTransform="{m}">"#);
                for (t, c) in &p.stops {
                    let (c, a) = color_parts(*c);
                    let _ = write!(self.defs, r#"<stop offset="{t}" stop-color="{c}" stop-opacity="{a:.3}"/>"#);
                }
                self.defs.push_str(if p.kind == PaintKind::Linear { "</linearGradient>" } else { "</radialGradient>" });
                format!(r#"{name}="url(#{id})" {name}-opacity="{:.3}""#, p.alpha())
            }
            PaintKind::Image => {
                let Some(bytes) = self.doc.images.get(&p.image) else { return format!(r#"{name}="none""#) };
                use base64::Engine;
                let mime = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) { "image/png" } else { "image/jpeg" };
                let data = base64::engine::general_purpose::STANDARD.encode(bytes.as_slice());
                let id = self.id("image");
                use crate::doc::ScaleMode;
                let (x, y, w, h) = (r.min.x, r.min.y, r.width(), r.height());
                let href = format!("data:{mime};base64,{data}");
                if p.scale_mode == ScaleMode::Tile {
                    let dims = image::ImageReader::new(std::io::Cursor::new(bytes.as_slice())).with_guessed_format().ok().and_then(|r| r.into_dimensions().ok());
                    let (iw, ih) = dims.map_or((w, h), |(a, b)| (a as f32 * p.tile_scale, b as f32 * p.tile_scale));
                    let _ = write!(
                        self.defs,
                        r#"<pattern id="{id}" patternUnits="userSpaceOnUse" x="{x}" y="{y}" width="{iw}" height="{ih}"><image href="{href}" x="0" y="0" width="{iw}" height="{ih}" preserveAspectRatio="none"/></pattern>"#
                    );
                } else {
                    let aspect = match p.scale_mode {
                        ScaleMode::Fit => "xMidYMid meet",
                        ScaleMode::Stretch => "none",
                        _ => "xMidYMid slice",
                    };
                    let _ = write!(
                        self.defs,
                        r#"<pattern id="{id}" patternUnits="userSpaceOnUse" x="{x}" y="{y}" width="{w}" height="{h}"><image href="{href}" x="{x}" y="{y}" width="{w}" height="{h}" preserveAspectRatio="{aspect}"/></pattern>"#
                    );
                }
                format!(r#"{name}="url(#{id})" {name}-opacity="{:.3}""#, p.alpha())
            }
        }
    }

    /// Paint a standalone vector (no children), e.g. a boolean's result.
    fn node_like(&mut self, v: &Node) {
        let o = self.o;
        let wrap = v.opacity < 1.0;
        if wrap {
            let _ = write!(self.out, r#"<g opacity="{:.3}">"#, v.opacity);
        }
        let shadow = self.open_shadows(v);
        for f in v.fills.iter().filter(|f| f.visible) {
            let a = self.paint("fill", f, v);
            self.out.push_str(&shape(v, o, 0.0, &a));
        }
        for s in v.strokes.iter().filter(|s| s.visible) {
            let attrs = format!(r#"fill="none" {} stroke-width="{}""#, self.paint("stroke", s, v), v.stroke_width);
            self.out.push_str(&shape(v, o, 0.0, &attrs));
        }
        if shadow {
            self.out.push_str("</g>");
        }
        if wrap {
            self.out.push_str("</g>");
        }
    }

    /// Opens a filter group for the layer's own drop and inner shadows. Returns whether it did.
    fn open_shadows(&mut self, n: &Node) -> bool {
        let fx: Vec<Shadow> = n.shadows.iter().filter(|s| s.visible && s.kind.is_shadow()).copied().collect();
        if fx.is_empty() {
            return false;
        }
        let id = self.id("effects");
        let reg = n.aabb().translate(-self.o).expand(shadow_reach(n));
        let _ = write!(
            self.defs,
            r#"<filter id="{id}" filterUnits="userSpaceOnUse" x="{}" y="{}" width="{}" height="{}" color-interpolation-filters="sRGB">"#,
            reg.min.x,
            reg.min.y,
            reg.width(),
            reg.height()
        );
        for (i, s) in fx.iter().enumerate() {
            let (c, a) = color_parts(s.color);
            if s.kind == EffectKind::DropShadow {
                let _ = write!(self.defs, r#"<feFlood flood-color="{c}" flood-opacity="{a:.3}"/><feComposite in2="SourceAlpha" operator="in""#);
                if s.spread > 0.0 {
                    let _ = write!(self.defs, r#"/><feMorphology operator="dilate" radius="{}""#, s.spread);
                }
                let _ = write!(self.defs, r#"/><feGaussianBlur stdDeviation="{}"/><feOffset dx="{}" dy="{}" result="e{i}"/>"#, s.blur / 2.0, s.offset.x, s.offset.y);
            } else {
                // Inner shadow: the shape's inverse, shifted and blurred, kept only inside the shape.
                let _ = write!(
                    self.defs,
                    r#"<feComponentTransfer in="SourceAlpha"><feFuncA type="table" tableValues="1 0"/></feComponentTransfer><feOffset dx="{}" dy="{}"/><feGaussianBlur stdDeviation="{}" result="inv{i}"/><feFlood flood-color="{c}" flood-opacity="{a:.3}"/><feComposite in2="inv{i}" operator="in"/><feComposite in2="SourceAlpha" operator="in" result="e{i}"/>"#,
                    s.offset.x,
                    s.offset.y,
                    s.blur / 2.0
                );
            }
        }
        self.defs.push_str("<feMerge>");
        for (i, s) in fx.iter().enumerate() {
            if s.kind == EffectKind::DropShadow {
                let _ = write!(self.defs, r#"<feMergeNode in="e{i}"/>"#);
            }
        }
        self.defs.push_str(r#"<feMergeNode in="SourceGraphic"/>"#);
        for (i, s) in fx.iter().enumerate() {
            if s.kind == EffectKind::InnerShadow {
                let _ = write!(self.defs, r#"<feMergeNode in="e{i}"/>"#);
            }
        }
        self.defs.push_str("</feMerge></filter>");
        let _ = write!(self.out, r#"<g filter="url(#{id})">"#);
        true
    }

    /// Opens a layer-blur group around the whole layer (children included).
    fn open_blur(&mut self, n: &Node) -> bool {
        let Some(b) = n.shadows.iter().find(|s| s.visible && s.kind == EffectKind::LayerBlur) else { return false };
        let id = self.id("blur");
        let reg = n.aabb().translate(-self.o).expand(b.blur * 2.0);
        let _ = write!(
            self.defs,
            r#"<filter id="{id}" filterUnits="userSpaceOnUse" x="{}" y="{}" width="{}" height="{}"><feGaussianBlur stdDeviation="{}"/></filter>"#,
            reg.min.x,
            reg.min.y,
            reg.width(),
            reg.height(),
            b.blur / 2.0
        );
        let _ = write!(self.out, r#"<g filter="url(#{id})">"#);
        true
    }

    fn node(&mut self, id: NodeId) {
        let doc = self.doc;
        let n = doc.node(id);
        if !n.visible {
            return;
        }
        if n.is_mask && n.parent.is_none() {
            // A mask with nothing to clip shows nothing.
            return;
        }
        if n.kind == Kind::Boolean {
            // Export the computed result as one path; operands stay internal.
            if let Some(v) = crate::boolean::as_vector(doc, id) {
                self.node_like(&v);
            }
            return;
        }
        let o = self.o;
        let blended = n.blend != BlendMode::Normal;
        if blended {
            let _ = write!(self.out, r#"<g style="mix-blend-mode:{}">"#, n.blend.css());
        }
        let wrap = n.opacity < 1.0;
        if wrap {
            let _ = write!(self.out, r#"<g opacity="{:.3}">"#, n.opacity);
        }
        let c = n.rect.center() - o;
        let turn = if n.rotation != 0.0 && n.kind != Kind::Group {
            format!(r#" transform="rotate({} {} {})""#, -n.rotation, c.x, c.y)
        } else {
            String::new()
        };
        let blurred = self.open_blur(n);
        // Groups cast their children's shadow; everything else shadows its own shape.
        let group_shadow = n.kind == Kind::Group && self.open_shadows(n);
        let own_shadow = n.kind != Kind::Group && self.open_shadows(n);
        if !turn.is_empty() {
            let _ = write!(self.out, "<g{turn}>");
        }
        match n.kind {
            Kind::Group | Kind::Slice => {}
            Kind::Text => {
                if let Some(f) = n.fills.iter().rev().find(|f| f.visible) {
                    let r = n.rect.translate(-o);
                    let fill = self.paint("fill", f, n);
                    let (anchor, x) = match n.text_align {
                        TextAlign::Center => ("middle", r.center().x),
                        TextAlign::Right => ("end", r.max.x),
                        _ => ("start", r.min.x),
                    };
                    let mut attrs = format!(
                        r#"font-family="{}" font-size="{}" font-weight="{}" text-anchor="{anchor}" {fill}"#,
                        esc(&n.font_family),
                        n.font_size,
                        n.font_weight
                    );
                    if n.letter_spacing != 0.0 {
                        let _ = write!(attrs, r#" letter-spacing="{}""#, n.letter_spacing / 100.0 * n.font_size);
                    }
                    match n.decoration {
                        Decoration::Underline => attrs.push_str(r#" text-decoration="underline""#),
                        Decoration::Strikethrough => attrs.push_str(r#" text-decoration="line-through""#),
                        Decoration::None => {}
                    }
                    let lines: Vec<String> = if n.lines.is_empty() {
                        crate::text::apply_case(&n.text, n.text_case).split('\n').map(|l| format!("{l}\n")).collect()
                    } else {
                        n.lines.clone()
                    };
                    let top = r.min.y + crate::text::draw_offset(n, lines.len()).y;
                    let pitch = crate::text::line_pitch(n);
                    let _ = write!(self.out, r#"<text {attrs} xml:space="preserve">"#);
                    let mut offset = 0;
                    for (i, line) in lines.iter().enumerate() {
                        let y = top + n.font_size * crate::text::ASCENT + pitch * i as f32;
                        let body: Vec<char> = line.trim_end_matches('\n').chars().collect();
                        // Split the line at span boundaries so each run carries its own style.
                        let mut cuts = vec![0, body.len()];
                        for sp in &n.spans {
                            for b in [sp.start, sp.end] {
                                if b > offset && b < offset + body.len() {
                                    cuts.push(b - offset);
                                }
                            }
                        }
                        cuts.sort();
                        cuts.dedup();
                        for (k, w) in cuts.windows(2).enumerate() {
                            let text: String = body[w[0]..w[1]].iter().collect();
                            let (a, b) = (offset + w[0], offset + w[1]);
                            let mut style = String::new();
                            if let Some(sp) = n.spans.iter().find(|sp| sp.start <= a && sp.end >= b) {
                                let st = &sp.style;
                                if let Some(w) = st.font_weight {
                                    let _ = write!(style, r#" font-weight="{w}""#);
                                }
                                if let Some(sz) = st.font_size {
                                    let _ = write!(style, r#" font-size="{sz}""#);
                                }
                                if let Some(f) = &st.font_family {
                                    let _ = write!(style, r#" font-family="{}""#, esc(f));
                                }
                                if let Some(c) = st.color {
                                    let (c, a) = color_parts(c);
                                    let _ = write!(style, r#" fill="{c}" fill-opacity="{a:.3}""#);
                                }
                                if st.italic == Some(true) {
                                    style.push_str(r#" font-style="italic""#);
                                }
                                match st.decoration {
                                    Some(Decoration::Underline) => style.push_str(r#" text-decoration="underline""#),
                                    Some(Decoration::Strikethrough) => style.push_str(r#" text-decoration="line-through""#),
                                    _ => {}
                                }
                            }
                            let pos = if k == 0 { format!(r#" x="{x}" y="{y}""#) } else { String::new() };
                            let _ = write!(self.out, r#"<tspan{pos}{style}>{}</tspan>"#, esc(&text));
                        }
                        offset += line.chars().count();
                    }
                    self.out.push_str("</text>");
                }
            }
            _ => {
                if n.has_area() {
                    for f in n.fills.iter().filter(|f| f.visible) {
                        let a = self.paint("fill", f, n);
                        self.out.push_str(&shape(n, o, 0.0, &a));
                    }
                }
                if n.stroke_width > 0.0 {
                    let grow = match (n.kind, n.stroke_align) {
                        (Kind::Vector, _) | (_, StrokeAlign::Center) => 0.0,
                        (_, StrokeAlign::Inside) => -n.stroke_width / 2.0,
                        (_, StrokeAlign::Outside) => n.stroke_width / 2.0,
                    };
                    for s in n.strokes.iter().filter(|s| s.visible) {
                        let attrs = format!(r#"fill="none" {} stroke-width="{}"{}"#, self.paint("stroke", s, n), n.stroke_width, stroke_style(n));
                        self.out.push_str(&shape(n, o, grow, &attrs));
                    }
                    if let Some(s) = n.strokes.iter().rev().find(|s| s.visible) {
                        let (c, a) = color_parts(s.color);
                        for (tip, dir, m) in crate::render::marker_ends(n) {
                            let (pts, filled) = crate::render::marker_shape(tip - o, dir, m, n.stroke_width);
                            let d: Vec<String> = pts.iter().map(|p| format!("{} {}", p.x, p.y)).collect();
                            let style = if filled {
                                format!(r#"fill="{c}" fill-opacity="{a:.3}""#)
                            } else {
                                format!(r#"fill="none" stroke="{c}" stroke-opacity="{a:.3}" stroke-width="{}" stroke-linecap="round" stroke-linejoin="round""#, n.stroke_width)
                            };
                            let _ = write!(self.out, r#"<path d="M{}{}" {style}/>"#, d.join("L"), if filled { "Z" } else { "" });
                        }
                    }
                }
            }
        }
        if !turn.is_empty() {
            self.out.push_str("</g>");
        }
        if own_shadow {
            self.out.push_str("</g>");
        }
        if !n.children.is_empty() {
            if n.kind.is_frame() && n.clip {
                let cid = self.id("clip");
                let _ = write!(self.defs, r#"<clipPath id="{cid}">{}</clipPath>"#, shape(n, o, 0.0, turn.trim()));
                let _ = write!(self.out, r#"<g clip-path="url(#{cid})">"#);
            } else {
                self.out.push_str("<g>");
            }
            // A mask layer clips everything above it in this parent (alpha mask, like Figma).
            let mut masks_open = 0;
            for &c in &n.children {
                let cn = doc.node(c);
                if cn.is_mask && cn.visible {
                    let mid = self.id("mask");
                    let reg = n.aabb().union(cn.aabb()).translate(-o).expand(shadow_reach(cn) + 1.0);
                    let body = std::mem::take(&mut self.out);
                    let mut plain = cn.clone();
                    plain.is_mask = false;
                    self.node_like_full(&plain, c);
                    let mask_body = std::mem::replace(&mut self.out, body);
                    let _ = write!(
                        self.defs,
                        r#"<mask id="{mid}" maskUnits="userSpaceOnUse" x="{}" y="{}" width="{}" height="{}" style="mask-type:alpha">{mask_body}</mask>"#,
                        reg.min.x,
                        reg.min.y,
                        reg.width(),
                        reg.height()
                    );
                    let _ = write!(self.out, r#"<g mask="url(#{mid})">"#);
                    masks_open += 1;
                } else {
                    self.node(c);
                }
            }
            for _ in 0..masks_open {
                self.out.push_str("</g>");
            }
            self.out.push_str("</g>");
        }
        if group_shadow {
            self.out.push_str("</g>");
        }
        if blurred {
            self.out.push_str("</g>");
        }
        if wrap {
            self.out.push_str("</g>");
        }
        if blended {
            self.out.push_str("</g>");
        }
    }

    /// Render a modified copy of a document node (e.g. a mask without its mask flag).
    fn node_like_full(&mut self, n: &Node, id: NodeId) {
        let mut d = self.doc.clone();
        *d.node_mut(id) = n.clone();
        let mut sub = Svg { doc: &d, o: self.o, out: String::new(), defs: String::new(), next: self.next + 1000 };
        sub.node(id);
        self.next = sub.next;
        self.defs.push_str(&sub.defs);
        self.out.push_str(&sub.out);
    }
}

/// Export bounds: layers plus their drop shadows, like Figma.
pub fn export_bounds(doc: &Document, ids: &[NodeId]) -> Rect {
    let r = ids.iter().fold(Rect::NOTHING, |r, id| {
        let n = doc.node(*id);
        let mut b = n.aabb();
        for s in n.shadows.iter().filter(|s| s.visible) {
            match s.kind {
                EffectKind::DropShadow => b = b.union(n.aabb().translate(s.offset).expand(s.blur + s.spread)),
                EffectKind::LayerBlur => b = b.union(n.aabb().expand(s.blur)),
                _ => {}
            }
        }
        r.union(b)
    });
    if r.is_finite() { r } else { Rect::ZERO }
}

pub fn to_svg(doc: &Document, ids: &[NodeId]) -> String {
    let ids = doc.sort_by_z(ids);
    let r = export_bounds(doc, &ids);
    let mut svg = Svg { doc, o: r.min.to_vec2(), out: String::new(), defs: String::new(), next: 0 };
    for &id in &ids {
        svg.node(id);
    }
    let (w, h) = (r.width().max(1.0), r.height().max(1.0));
    format!(
        r#"<svg width="{w}" height="{h}" viewBox="0 0 {w} {h}" fill="none" xmlns="http://www.w3.org/2000/svg">{}{}</svg>"#,
        if svg.defs.is_empty() { String::new() } else { format!("<defs>{}</defs>", svg.defs) },
        svg.out
    )
}

/// What's beneath a background-blurred layer (canvas only; SVG can't express backdrop blur):
/// an unblurred backdrop and a mask of the layer's shape, both over `region` (padded by the
/// blur so edges blur against real content), plus the blur radius.
pub fn backdrop_parts(doc: &Document, id: NodeId) -> Option<(String, String, Rect, f32)> {
    let n = doc.node(id);
    let blur = n.shadows.iter().find(|s| s.visible && s.kind == EffectKind::BackgroundBlur)?.blur;
    let page = doc.page_of(id);
    // Everything painted before this layer: at each level of its ancestry, drop the
    // layer (or ancestor's later siblings) and whatever sits above it.
    let mut tmp = doc.clone();
    let mut cur = id;
    let mut first = true;
    loop {
        let parent = tmp.node(cur).parent;
        let list = tmp.children_of(page, parent).clone();
        let i = list.iter().position(|c| *c == cur).unwrap_or(0);
        for &later in &list[if first { i } else { i + 1 }..] {
            tmp.remove(later);
        }
        first = false;
        match parent {
            Some(p) => cur = p,
            None => break,
        }
    }
    let region = n.aabb().expand(blur);
    let mut svg = Svg { doc: &tmp, o: region.min.to_vec2(), out: String::new(), defs: String::new(), next: 0 };
    let bg = doc.pages[page].background;
    let _ = write!(svg.out, r##"<rect x="0" y="0" width="{}" height="{}" fill="#{}"/>"##, region.width(), region.height(), bg.hex());
    for r in tmp.pages[page].children.clone() {
        svg.node(r);
    }
    let (w, h) = (region.width().max(1.0), region.height().max(1.0));
    let head = format!(r#"<svg width="{w}" height="{h}" viewBox="0 0 {w} {h}" xmlns="http://www.w3.org/2000/svg">"#);
    let content = format!("{head}<defs>{}</defs>{}</svg>", svg.defs, svg.out);
    let turn = if n.rotation != 0.0 {
        let c = n.rect.center() - svg.o;
        format!(r#"transform="rotate({} {} {})""#, -n.rotation, c.x, c.y)
    } else {
        String::new()
    };
    let mask = format!(r##"{head}{}</svg>"##, shape(n, svg.o, 0.0, &format!(r##"fill="#FFFFFF" {turn}"##)));
    Some((content, mask, region, blur))
}

/// Rasterize SVG text at `scale` (premultiplied RGBA pixels and size).
pub fn rasterize(svg: &str, scale: f32) -> Option<(Vec<u8>, [usize; 2])> {
    let mut opt = resvg::usvg::Options::default();
    for f in FONTS {
        opt.fontdb_mut().load_font_data(f.to_vec());
    }
    let tree = resvg::usvg::Tree::from_str(svg, &opt).ok()?;
    let size = tree.size();
    let (w, h) = ((size.width() * scale).ceil() as u32, (size.height() * scale).ceil() as u32);
    let mut pix = resvg::tiny_skia::Pixmap::new(w.clamp(1, 4096), h.clamp(1, 4096))?;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pix.as_mut());
    Some((pix.data().to_vec(), [pix.width() as usize, pix.height() as usize]))
}

/// Everything on `page` inside `region` (a slice), clipped to it.
pub fn region_svg(doc: &Document, page: usize, region: Rect, skip: NodeId) -> String {
    let mut svg = Svg { doc, o: region.min.to_vec2(), out: String::new(), defs: String::new(), next: 0 };
    for &r in &doc.pages[page].children {
        if r != skip && doc.node(r).aabb().intersects(region) {
            svg.node(r);
        }
    }
    let (w, h) = (region.width().max(1.0), region.height().max(1.0));
    format!(
        r#"<svg width="{w}" height="{h}" viewBox="0 0 {w} {h}" fill="none" xmlns="http://www.w3.org/2000/svg"><defs>{}</defs>{}</svg>"#,
        svg.defs, svg.out
    )
}

#[cfg(test)]
pub fn to_png(doc: &Document, ids: &[NodeId], scale: f32) -> Result<Vec<u8>, String> {
    svg_to_png(doc, &to_svg(doc, ids), scale)
}

/// Encode export SVG in a format: SVG as-is, PNG/JPG rasterized at `scale`, PDF as vectors.
pub fn encode(doc: &Document, svg: &str, format: crate::doc::ExportFormat, scale: f32) -> Result<Vec<u8>, String> {
    use crate::doc::ExportFormat;
    match format {
        ExportFormat::Svg => Ok(svg.as_bytes().to_vec()),
        ExportFormat::Png => svg_to_png(doc, svg, scale),
        ExportFormat::Jpg => {
            // JPEG has no alpha: flatten onto white.
            let png = svg_to_png(doc, svg, scale)?;
            let img = image::load_from_memory(&png).map_err(|e| e.to_string())?.to_rgba8();
            let mut rgb = image::RgbImage::new(img.width(), img.height());
            for (x, y, p) in img.enumerate_pixels() {
                let a = p.0[3] as u32;
                let c = |v: u8| ((v as u32 * a + 255 * (255 - a)) / 255) as u8;
                rgb.put_pixel(x, y, image::Rgb([c(p.0[0]), c(p.0[1]), c(p.0[2])]));
            }
            let mut out = std::io::Cursor::new(vec![]);
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 92).encode_image(&rgb).map_err(|e| e.to_string())?;
            Ok(out.into_inner())
        }
        ExportFormat::Pdf => {
            let mut opt = svg2pdf::usvg::Options::default();
            if doc.nodes.values().any(|n| n.kind == Kind::Text && n.font_family != "Inter") {
                opt.fontdb_mut().load_system_fonts();
            }
            for f in FONTS {
                opt.fontdb_mut().load_font_data(f.to_vec());
            }
            let tree = svg2pdf::usvg::Tree::from_str(svg, &opt).map_err(|e| e.to_string())?;
            svg2pdf::to_pdf(&tree, Default::default(), Default::default()).map_err(|e| e.to_string())
        }
    }
}

pub fn svg_to_png(doc: &Document, svg: &str, scale: f32) -> Result<Vec<u8>, String> {
    let mut opt = resvg::usvg::Options::default();
    // System fonts only when a layer uses one; Inter is always bundled.
    let needs_system = doc.nodes.values().any(|n| n.kind == Kind::Text && n.font_family != "Inter");
    if needs_system {
        opt.fontdb = std::sync::Arc::new(crate::text::system_fonts().clone());
    }
    for f in FONTS {
        opt.fontdb_mut().load_font_data(f.to_vec());
    }
    let tree = resvg::usvg::Tree::from_str(svg, &opt).map_err(|e| e.to_string())?;
    let size = tree.size();
    let (w, h) = ((size.width() * scale).ceil() as u32, (size.height() * scale).ceil() as u32);
    let mut pix = resvg::tiny_skia::Pixmap::new(w.max(1), h.max(1)).ok_or("export too large")?;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pix.as_mut());
    pix.encode_png().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::{Node, PathPoint};
    use egui::{pos2, vec2};

    #[test]
    fn svg_and_png_roundtrip() {
        let mut d = Document::default();
        let f = d.insert(0, None, None, Node::new(Kind::Frame, Rect::from_min_size(pos2(10., 10.), vec2(40., 20.))));
        d.insert(0, Some(f), None, Node::new(Kind::Ellipse, Rect::from_min_size(pos2(40., 20.), vec2(20., 20.))));
        let mut t = Node::new(Kind::Text, Rect::from_min_size(pos2(12., 12.), vec2(30., 15.)));
        t.text = "Hi & <bye>".into();
        d.insert(0, Some(f), None, t);
        let pts = [PathPoint { p: pos2(10., 10.), ..Default::default() }, PathPoint { p: pos2(20., 30.), ..Default::default() }];
        d.insert(0, Some(f), None, Node::vector(&pts, false));
        let svg = to_svg(&d, &[f]);
        assert!(svg.contains(r#"width="40" height="20""#), "{svg}");
        assert!(svg.contains("clip-path"), "{svg}");
        assert!(svg.contains("<tspan"), "{svg}");
        assert!(svg.contains("Hi &amp; &lt;bye&gt;"), "{svg}");
        assert!(svg.contains("M0 0L10 20"), "{svg}");
        let png = to_png(&d, &[f], 2.0).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        // 80x40 at 2x: width/height live in the IHDR chunk
        assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 80);
        assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), 40);
    }

    #[test]
    fn gradients_images_and_shadows_render() {
        let mut d = Document::default();
        let mut n = Node::new(Kind::Rectangle, Rect::from_min_size(pos2(0., 0.), vec2(100., 50.)));
        let mut g = Paint::solid(Rgba::rgb(255, 0, 0));
        g.convert(PaintKind::Linear);
        g.from = pos2(0., 0.5);
        g.to = pos2(1., 0.5);
        n.fills = vec![g];
        n.shadows = vec![crate::doc::Shadow::default()];
        let a = d.insert(0, None, None, n);
        let svg = to_svg(&d, &[a]);
        assert!(svg.contains("<linearGradient") && svg.contains("feGaussianBlur"), "{svg}");
        // Bounds grow to fit the shadow (blur 4, y offset 4).
        assert!(svg.contains(r#"width="108" height="58""#), "{svg}");
        // Left edge red, right edge transparent: check pixels of the rasterized export.
        let png = to_png(&d, &[a], 1.0).unwrap();
        let img = image::load_from_memory(&png).unwrap().to_rgba8();
        let left = img.get_pixel(6, 30).0;
        let right = img.get_pixel(100, 30).0;
        assert!(left[0] > 200 && left[3] > 200, "{left:?}");
        assert!(right[3] < 120, "{right:?}");

        // Image fill: a 2x1 PNG (red, blue) stretched over a square, cover-cropped.
        let mut buf = std::io::Cursor::new(vec![]);
        image::RgbaImage::from_fn(2, 1, |x, _| if x == 0 { image::Rgba([255, 0, 0, 255]) } else { image::Rgba([0, 0, 255, 255]) })
            .write_to(&mut buf, image::ImageFormat::Png)
            .unwrap();
        d.images.insert("img".into(), std::sync::Arc::new(buf.into_inner()));
        let mut p = Paint::solid(Rgba::WHITE);
        p.convert(PaintKind::Image);
        p.image = "img".into();
        let mut sq = Node::new(Kind::Rectangle, Rect::from_min_size(pos2(500., 0.), vec2(40., 40.)));
        sq.fills = vec![p];
        let b = d.insert(0, None, None, sq);
        let svg = to_svg(&d, &[b]);
        assert!(svg.contains("data:image/png;base64,"), "{svg}");
        let png = to_png(&d, &[b], 1.0).unwrap();
        let img = image::load_from_memory(&png).unwrap().to_rgba8();
        assert!(img.get_pixel(5, 20).0[0] > 200, "left half red");
        assert!(img.get_pixel(35, 20).0[2] > 200, "right half blue");
    }

    #[test]
    fn corners_and_dashes_export() {
        let mut d = Document::default();
        let mut n = Node::new(Kind::Rectangle, Rect::from_min_size(pos2(0., 0.), vec2(40., 20.)));
        n.corner_radii = Some([0., 10., 0., 4.]);
        n.strokes = vec![Paint::solid(Rgba::BLACK)];
        n.dash = vec![4., 2.];
        n.cap = Cap::Round;
        let a = d.insert(0, None, None, n);
        let svg = to_svg(&d, &[a]);
        assert!(svg.contains("A10 10 0 0 1 40 10"), "{svg}");
        assert!(svg.contains(r#"stroke-dasharray="4 2" stroke-linecap="round""#), "{svg}");
        to_png(&d, &[a], 1.0).unwrap();
    }

    #[test]
    fn masks_clip_and_blend_modes_export() {
        let mut d = Document::default();
        let g = d.insert(0, None, None, Node::new(Kind::Group, Rect::NOTHING));
        let mut m = Node::new(Kind::Ellipse, Rect::from_min_size(pos2(0., 0.), vec2(40., 40.)));
        m.is_mask = true;
        d.insert(0, Some(g), None, m);
        let mut sq = Node::new(Kind::Rectangle, Rect::from_min_size(pos2(0., 0.), vec2(40., 40.)));
        sq.fills = vec![Paint::solid(Rgba::rgb(255, 0, 0))];
        sq.blend = BlendMode::Multiply;
        d.insert(0, Some(g), None, sq);
        d.reflow();
        let svg = to_svg(&d, &[g]);
        assert!(svg.contains("mask-type:alpha") && svg.contains("mix-blend-mode:multiply"), "{svg}");
        let png = to_png(&d, &[g], 1.0).unwrap();
        let img = image::load_from_memory(&png).unwrap().to_rgba8();
        assert!(img.get_pixel(20, 20).0[3] > 200, "center kept");
        assert!(img.get_pixel(2, 2).0[3] < 30, "corner masked away");
    }

    #[test]
    fn image_fit_letterboxes_and_tile_repeats() {
        use crate::doc::ScaleMode;
        let mut d = Document::default();
        let mut buf = std::io::Cursor::new(vec![]);
        image::RgbaImage::from_fn(2, 1, |x, _| if x == 0 { image::Rgba([255, 0, 0, 255]) } else { image::Rgba([0, 0, 255, 255]) })
            .write_to(&mut buf, image::ImageFormat::Png)
            .unwrap();
        d.images.insert("img".into(), std::sync::Arc::new(buf.into_inner()));
        let layer = |d: &mut Document, mode: ScaleMode, x: f32| {
            let mut p = Paint::solid(Rgba::WHITE);
            p.convert(PaintKind::Image);
            p.image = "img".into();
            p.scale_mode = mode;
            p.tile_scale = 10.0;
            let mut n = Node::new(Kind::Rectangle, Rect::from_min_size(pos2(x, 0.), vec2(40., 40.)));
            n.fills = vec![p];
            d.insert(0, None, None, n)
        };
        let fit = layer(&mut d, ScaleMode::Fit, 0.);
        let img = image::load_from_memory(&to_png(&d, &[fit], 1.0).unwrap()).unwrap().to_rgba8();
        assert!(img.get_pixel(20, 3).0[3] < 30, "letterbox band above the 2:1 image");
        assert!(img.get_pixel(5, 20).0[0] > 200, "image in the middle");
        let tile = layer(&mut d, ScaleMode::Tile, 100.);
        let img = image::load_from_memory(&to_png(&d, &[tile], 1.0).unwrap()).unwrap().to_rgba8();
        // 2x1 image at 10x is 20x10 per tile: red then blue, repeating.
        assert!(img.get_pixel(5, 5).0[0] > 200 && img.get_pixel(15, 5).0[2] > 200, "first tile");
        assert!(img.get_pixel(25, 15).0[0] > 200, "repeats");
    }

    #[test]
    fn jpg_and_pdf_encode() {
        use crate::doc::ExportFormat;
        let mut d = Document::default();
        let mut t = Node::new(Kind::Text, Rect::from_min_size(pos2(0., 0.), vec2(60., 20.)));
        t.text = "PDF".into();
        let a = d.insert(0, None, None, t);
        d.insert(0, None, None, Node::new(Kind::Ellipse, Rect::from_min_size(pos2(0., 30.), vec2(40., 40.))));
        let svg = to_svg(&d, &[a]);
        let jpg = encode(&d, &svg, ExportFormat::Jpg, 2.0).unwrap();
        assert_eq!(&jpg[..2], &[0xFF, 0xD8], "JPEG magic");
        let pdf = encode(&d, &svg, ExportFormat::Pdf, 1.0).unwrap();
        assert!(pdf.starts_with(b"%PDF"), "PDF magic");
    }

    #[test]
    fn rich_text_exports_styled_runs() {
        use crate::doc::{SpanStyle, TextSpan};
        let mut d = Document::default();
        let mut t = Node::new(Kind::Text, Rect::from_min_size(pos2(0., 0.), vec2(100., 20.)));
        t.text = "Hello world".into();
        t.spans = vec![TextSpan { start: 6, end: 11, style: SpanStyle { font_weight: Some(700), color: Some(Rgba::rgb(255, 0, 0)), ..Default::default() } }];
        let a = d.insert(0, None, None, t);
        let svg = to_svg(&d, &[a]);
        assert!(svg.contains(r#"<tspan x="0" y="#) && svg.contains(r##"<tspan font-weight="700" fill="#FF0000""##), "{svg}");
        assert!(svg.contains(">Hello </tspan>") && svg.contains(">world</tspan>"), "{svg}");
    }

    #[test]
    fn rotated_export_uses_rotated_bounds() {
        let mut d = Document::default();
        let a = d.insert(0, None, None, Node::new(Kind::Rectangle, Rect::from_min_size(pos2(0., 0.), vec2(100., 10.))));
        d.rotate(a, 90.0, pos2(50., 5.));
        let svg = to_svg(&d, &[a]);
        assert!(svg.contains(r#"width="10" height="100""#), "{svg}");
        assert!(svg.contains(r#"transform="rotate(-90 5 50)""#), "{svg}");
    }
}
