//! SVG export from the model; PNG export rasterizes that SVG with resvg so both always match.

use crate::doc::{Decoration, Document, Kind, Node, NodeId, Paint, PaintKind, Rgba, Shadow, StrokeAlign, TextAlign};
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
        Kind::Vector => format!(r#"<path d="{}" {attrs}/>"#, path_data(n, o)),
        _ => {
            let rad = (n.radius + grow).clamp(0.0, r.width().min(r.height()).max(0.0) / 2.0);
            format!(
                r#"<rect x="{}" y="{}" width="{}" height="{}"{} {attrs}/>"#,
                r.min.x,
                r.min.y,
                r.width().max(0.0),
                r.height().max(0.0),
                if rad > 0.0 { format!(r#" rx="{rad}""#) } else { String::new() }
            )
        }
    }
}

pub fn path_data(n: &Node, o: Vec2) -> String {
    let base = n.rect.min.to_vec2() - o;
    let p = |q: Pos2| {
        let a = q + base;
        format!("{} {}", a.x, a.y)
    };
    let pts = &n.points;
    let Some(first) = pts.first() else { return String::new() };
    let mut d = format!("M{}", p(first.p));
    let segs = if n.closed { pts.len() } else { pts.len() - 1 };
    for i in 0..segs {
        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
        if a.h_out == Vec2::ZERO && b.h_in == Vec2::ZERO {
            let _ = write!(d, "L{}", p(b.p));
        } else {
            let _ = write!(d, "C{} {} {}", p(a.p + a.h_out), p(b.p + b.h_in), p(b.p));
        }
    }
    if n.closed {
        d.push('Z');
    }
    d
}

/// How far shadows reach past a layer's bounds.
fn shadow_reach(n: &Node) -> f32 {
    n.shadows
        .iter()
        .filter(|s| s.visible)
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
                let _ = write!(
                    self.defs,
                    r#"<pattern id="{id}" patternUnits="userSpaceOnUse" x="{x}" y="{y}" width="{w}" height="{h}"><image href="data:{mime};base64,{data}" x="{x}" y="{y}" width="{w}" height="{h}" preserveAspectRatio="xMidYMid slice"/></pattern>"#,
                    x = r.min.x,
                    y = r.min.y,
                    w = r.width(),
                    h = r.height()
                );
                format!(r#"{name}="url(#{id})" {name}-opacity="{:.3}""#, p.alpha())
            }
        }
    }

    /// Opens a drop-shadow filter group if the layer has shadows. Returns whether it did.
    fn open_shadows(&mut self, n: &Node) -> bool {
        let shadows: Vec<Shadow> = n.shadows.iter().filter(|s| s.visible).copied().collect();
        if shadows.is_empty() {
            return false;
        }
        let id = self.id("shadow");
        let reg = n.aabb().translate(-self.o).expand(shadow_reach(n));
        let _ = write!(
            self.defs,
            r#"<filter id="{id}" filterUnits="userSpaceOnUse" x="{}" y="{}" width="{}" height="{}" color-interpolation-filters="sRGB">"#,
            reg.min.x,
            reg.min.y,
            reg.width(),
            reg.height()
        );
        for (i, s) in shadows.iter().enumerate() {
            let (c, a) = color_parts(s.color);
            let _ = write!(self.defs, r#"<feFlood flood-color="{c}" flood-opacity="{a:.3}"/><feComposite in2="SourceAlpha" operator="in""#);
            if s.spread > 0.0 {
                let _ = write!(self.defs, r#"/><feMorphology operator="dilate" radius="{}""#, s.spread);
            }
            let _ = write!(
                self.defs,
                r#"/><feGaussianBlur stdDeviation="{}"/><feOffset dx="{}" dy="{}" result="s{i}"/>"#,
                s.blur / 2.0,
                s.offset.x,
                s.offset.y
            );
        }
        self.defs.push_str("<feMerge>");
        for i in 0..shadows.len() {
            let _ = write!(self.defs, r#"<feMergeNode in="s{i}"/>"#);
        }
        self.defs.push_str(r#"<feMergeNode in="SourceGraphic"/></feMerge></filter>"#);
        let _ = write!(self.out, r#"<g filter="url(#{id})">"#);
        true
    }

    fn node(&mut self, id: NodeId) {
        let doc = self.doc;
        let n = doc.node(id);
        if !n.visible {
            return;
        }
        let o = self.o;
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
        // Groups cast their children's shadow; everything else shadows its own shape.
        let group_shadow = n.kind == Kind::Group && self.open_shadows(n);
        let own_shadow = n.kind != Kind::Group && self.open_shadows(n);
        if !turn.is_empty() {
            let _ = write!(self.out, "<g{turn}>");
        }
        match n.kind {
            Kind::Group => {}
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
                        crate::text::apply_case(&n.text, n.text_case).split('\n').map(str::to_string).collect()
                    } else {
                        n.lines.iter().map(|l| l.trim_end_matches('\n').to_string()).collect()
                    };
                    let top = r.min.y + crate::text::draw_offset(n, lines.len()).y;
                    let pitch = crate::text::line_pitch(n);
                    let _ = write!(self.out, r#"<text {attrs} xml:space="preserve">"#);
                    for (i, line) in lines.iter().enumerate() {
                        let y = top + n.font_size * crate::text::ASCENT + pitch * i as f32;
                        let _ = write!(self.out, r#"<tspan x="{x}" y="{y}">{}</tspan>"#, esc(line));
                    }
                    self.out.push_str("</text>");
                }
            }
            _ => {
                if n.kind != Kind::Vector || n.closed {
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
                        let attrs = format!(r#"fill="none" {} stroke-width="{}""#, self.paint("stroke", s, n), n.stroke_width);
                        self.out.push_str(&shape(n, o, grow, &attrs));
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
            for &c in &n.children {
                self.node(c);
            }
            self.out.push_str("</g>");
        }
        if group_shadow {
            self.out.push_str("</g>");
        }
        if wrap {
            self.out.push_str("</g>");
        }
    }
}

/// Export bounds: layers plus their drop shadows, like Figma.
pub fn export_bounds(doc: &Document, ids: &[NodeId]) -> Rect {
    let r = ids.iter().fold(Rect::NOTHING, |r, id| {
        let n = doc.node(*id);
        let mut b = n.aabb();
        for s in n.shadows.iter().filter(|s| s.visible) {
            b = b.union(n.aabb().translate(s.offset).expand(s.blur + s.spread));
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

pub fn to_png(doc: &Document, ids: &[NodeId], scale: f32) -> Result<Vec<u8>, String> {
    let svg = to_svg(doc, ids);
    let mut opt = resvg::usvg::Options::default();
    // System fonts only when a layer uses one; Inter is always bundled.
    let needs_system = doc.nodes.values().any(|n| n.kind == Kind::Text && n.font_family != "Inter");
    if needs_system {
        opt.fontdb = std::sync::Arc::new(crate::text::system_fonts().clone());
    }
    for f in FONTS {
        opt.fontdb_mut().load_font_data(f.to_vec());
    }
    let tree = resvg::usvg::Tree::from_str(&svg, &opt).map_err(|e| e.to_string())?;
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
    fn rotated_export_uses_rotated_bounds() {
        let mut d = Document::default();
        let a = d.insert(0, None, None, Node::new(Kind::Rectangle, Rect::from_min_size(pos2(0., 0.), vec2(100., 10.))));
        d.rotate(a, 90.0, pos2(50., 5.));
        let svg = to_svg(&d, &[a]);
        assert!(svg.contains(r#"width="10" height="100""#), "{svg}");
        assert!(svg.contains(r#"transform="rotate(-90 5 50)""#), "{svg}");
    }
}
