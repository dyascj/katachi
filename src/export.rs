//! SVG export from the model; PNG export rasterizes that SVG with resvg so both always match.

use crate::doc::{Document, Kind, Node, NodeId, Paint, StrokeAlign};
use egui::{Color32, Pos2, Rect, Vec2};
use std::fmt::Write;

pub const FONTS: [&[u8]; 4] = [
    include_bytes!("../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../assets/fonts/Inter-SemiBold.ttf"),
    include_bytes!("../assets/fonts/Inter-Bold.ttf"),
];

/// Inter's ascender / line height in ems (hhea), matching egui's layout of the same font.
const ASCENT: f32 = 0.96875;
const LINE: f32 = 1.2109375;

fn hex(c: Color32) -> String {
    format!("#{:02X}{:02X}{:02X}", c.r(), c.g(), c.b())
}

fn paint_attr(name: &str, p: &Paint) -> String {
    let [r, g, b, a] = p.color.to_srgba_unmultiplied();
    let mut s = format!(r#"{name}="{}""#, hex(Color32::from_rgb(r, g, b)));
    if a < 255 {
        let _ = write!(s, r#" {name}-opacity="{:.3}""#, a as f32 / 255.0);
    }
    s
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

fn node_svg(doc: &Document, id: NodeId, o: Vec2, out: &mut String, defs: &mut String) {
    let n = doc.node(id);
    if !n.visible {
        return;
    }
    let wrap = n.opacity < 1.0;
    if wrap {
        let _ = write!(out, r#"<g opacity="{:.3}">"#, n.opacity);
    }
    let c = n.rect.center() - o;
    let turn = if n.rotation != 0.0 && n.kind != Kind::Group {
        format!(r#" transform="rotate({} {} {})""#, -n.rotation, c.x, c.y)
    } else {
        String::new()
    };
    if !turn.is_empty() {
        let _ = write!(out, "<g{turn}>");
    }
    match n.kind {
        Kind::Group => {}
        Kind::Text => {
            if let Some(f) = n.fills.iter().rev().find(|f| f.visible) {
                let r = n.rect.translate(-o);
                let _ = write!(
                    out,
                    r#"<text font-family="Inter" font-size="{}" font-weight="{}" {} xml:space="preserve">"#,
                    n.font_size,
                    n.font_weight,
                    paint_attr("fill", f)
                );
                for (i, line) in n.text.split('\n').enumerate() {
                    let y = r.min.y + n.font_size * (ASCENT + LINE * i as f32);
                    let _ = write!(out, r#"<tspan x="{}" y="{}">{}</tspan>"#, r.min.x, y, esc(line));
                }
                out.push_str("</text>");
            }
        }
        _ => {
            if n.kind != Kind::Vector || n.closed {
                for f in n.fills.iter().filter(|f| f.visible) {
                    out.push_str(&shape(n, o, 0.0, &paint_attr("fill", f)));
                }
            }
            if n.stroke_width > 0.0 {
                let grow = match (n.kind, n.stroke_align) {
                    (Kind::Vector, _) | (_, StrokeAlign::Center) => 0.0,
                    (_, StrokeAlign::Inside) => -n.stroke_width / 2.0,
                    (_, StrokeAlign::Outside) => n.stroke_width / 2.0,
                };
                for s in n.strokes.iter().filter(|s| s.visible) {
                    let attrs = format!(r#"fill="none" {} stroke-width="{}""#, paint_attr("stroke", s), n.stroke_width);
                    out.push_str(&shape(n, o, grow, &attrs));
                }
            }
        }
    }
    if !turn.is_empty() {
        out.push_str("</g>");
    }
    if !n.children.is_empty() {
        if n.kind == Kind::Frame && n.clip {
            let cid = format!("clip{id}");
            let _ = write!(defs, r#"<clipPath id="{cid}">{}</clipPath>"#, shape(n, o, 0.0, turn.trim()));
            let _ = write!(out, r#"<g clip-path="url(#{cid})">"#);
        } else {
            out.push_str("<g>");
        }
        for &c in &n.children {
            node_svg(doc, c, o, out, defs);
        }
        out.push_str("</g>");
    }
    if wrap {
        out.push_str("</g>");
    }
}

/// Export bounds: visible extent including outside strokes.
pub fn export_bounds(doc: &Document, ids: &[NodeId]) -> Rect {
    let r = doc.bounds(ids);
    if r.is_finite() { r } else { Rect::ZERO }
}

pub fn to_svg(doc: &Document, ids: &[NodeId]) -> String {
    let ids = doc.sort_by_z(ids);
    let r = export_bounds(doc, &ids);
    let o = r.min.to_vec2();
    let (mut body, mut defs) = (String::new(), String::new());
    for &id in &ids {
        node_svg(doc, id, o, &mut body, &mut defs);
    }
    let (w, h) = (r.width().max(1.0), r.height().max(1.0));
    format!(
        r#"<svg width="{w}" height="{h}" viewBox="0 0 {w} {h}" fill="none" xmlns="http://www.w3.org/2000/svg">{}{body}</svg>"#,
        if defs.is_empty() { String::new() } else { format!("<defs>{defs}</defs>") }
    )
}

pub fn to_png(doc: &Document, ids: &[NodeId], scale: f32) -> Result<Vec<u8>, String> {
    let svg = to_svg(doc, ids);
    let mut opt = resvg::usvg::Options::default();
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
        assert!(svg.contains("Hi &amp; &lt;bye&gt;"), "{svg}");
        assert!(svg.contains("M0 0L10 20"), "{svg}");
        let png = to_png(&d, &[f], 2.0).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        // 80x40 at 2x: width/height live in the IHDR chunk
        assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 80);
        assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), 40);
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
