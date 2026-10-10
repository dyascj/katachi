//! SVG import (paste, drop, File > Import) and Flatten, both built on usvg's parsed tree.

use crate::doc::{Document, Kind, Node, NodeId, Paint, PaintKind, PathPoint, Rgba, StrokeAlign, SubPath};
use egui::{Pos2, Rect, Vec2, pos2, vec2};
use resvg::tiny_skia::{PathSegment, Point, Transform};
use resvg::usvg;

fn options() -> usvg::Options<'static> {
    let mut opt = usvg::Options::default();
    for f in crate::export::FONTS {
        opt.fontdb_mut().load_font_data(f.to_vec());
    }
    opt
}

pub fn looks_like_svg(text: &str) -> bool {
    let t = text.trim_start();
    t.starts_with("<svg") || (t.starts_with("<?xml") && t.contains("<svg"))
}

fn to_pos(ts: Transform, p: Point) -> Pos2 {
    let mut q = p;
    ts.map_point(&mut q);
    pos2(q.x, q.y)
}

/// Absolute contours of a usvg path, in our anchor-and-handle form.
fn contours(path: &usvg::Path) -> Vec<SubPath> {
    let ts = path.abs_transform();
    let mut out = vec![];
    let mut cur: Vec<PathPoint> = vec![];
    let finish = |cur: &mut Vec<PathPoint>, out: &mut Vec<SubPath>, closed: bool| {
        if closed && cur.len() > 1 && cur.first().unwrap().p.distance(cur.last().unwrap().p) < 1e-3 {
            // The closing segment landed back on the start: fold it into the first anchor.
            let last = cur.pop().unwrap();
            cur[0].h_in = last.h_in;
        }
        if !cur.is_empty() {
            out.push(SubPath { points: std::mem::take(cur), closed });
        }
    };
    for seg in path.data().segments() {
        match seg {
            PathSegment::MoveTo(p) => {
                finish(&mut cur, &mut out, false);
                cur.push(PathPoint { p: to_pos(ts, p), ..Default::default() });
            }
            PathSegment::LineTo(p) => cur.push(PathPoint { p: to_pos(ts, p), ..Default::default() }),
            PathSegment::QuadTo(c, p) => {
                let (c, p) = (to_pos(ts, c), to_pos(ts, p));
                if let Some(prev) = cur.last_mut() {
                    prev.h_out = (c - prev.p) * (2.0 / 3.0);
                }
                cur.push(PathPoint { p, h_in: (c - p) * (2.0 / 3.0), h_out: Vec2::ZERO });
            }
            PathSegment::CubicTo(c1, c2, p) => {
                let (c1, c2, p) = (to_pos(ts, c1), to_pos(ts, c2), to_pos(ts, p));
                if let Some(prev) = cur.last_mut() {
                    prev.h_out = c1 - prev.p;
                }
                cur.push(PathPoint { p, h_in: c2 - p, h_out: Vec2::ZERO });
            }
            PathSegment::Close => finish(&mut cur, &mut out, true),
        }
    }
    finish(&mut cur, &mut out, false);
    out
}

fn rgba(c: usvg::Color, opacity: f32) -> Rgba {
    Rgba([c.red, c.green, c.blue, (opacity.clamp(0.0, 1.0) * 255.0).round() as u8])
}

/// usvg paint -> our paint, with gradient handles mapped into the layer's unit square.
fn paint(p: &usvg::Paint, opacity: f32, ts: Transform, bounds: Rect) -> Paint {
    let unit = |x: f32, y: f32, gts: Transform| {
        let w = to_pos(ts.pre_concat(gts), Point::from_xy(x, y));
        pos2((w.x - bounds.min.x) / bounds.width().max(1e-6), (w.y - bounds.min.y) / bounds.height().max(1e-6))
    };
    let stops = |s: &[usvg::Stop]| s.iter().map(|s| (s.offset().get(), rgba(s.color(), s.opacity().get()))).collect();
    let alpha = Rgba::WHITE.with_alpha((opacity * 255.0).round() as u8);
    match p {
        usvg::Paint::Color(c) => Paint::solid(rgba(*c, opacity)),
        usvg::Paint::LinearGradient(g) => Paint {
            kind: PaintKind::Linear,
            color: alpha,
            stops: stops(g.stops()),
            from: unit(g.x1(), g.y1(), g.transform()),
            to: unit(g.x2(), g.y2(), g.transform()),
            ..Paint::solid(Rgba::WHITE)
        },
        usvg::Paint::RadialGradient(g) => Paint {
            kind: PaintKind::Radial,
            color: alpha,
            stops: stops(g.stops()),
            from: unit(g.cx(), g.cy(), g.transform()),
            to: unit(g.cx() + g.r().get(), g.cy(), g.transform()),
            ..Paint::solid(Rgba::WHITE)
        },
        usvg::Paint::Pattern(_) => Paint::solid(Rgba([0xD9, 0xD9, 0xD9, (opacity * 255.0) as u8])),
    }
}

fn path_node(p: &usvg::Path) -> Option<Node> {
    if !p.is_visible() {
        return None;
    }
    let subs = contours(p);
    if subs.is_empty() {
        return None;
    }
    let mut n = Node::vector_paths(subs);
    n.strokes.clear();
    n.stroke_align = StrokeAlign::Center;
    let ts = p.abs_transform();
    let bounds = n.rect;
    if let Some(f) = p.fill() {
        n.fills = vec![paint(f.paint(), f.opacity().get(), ts, bounds)];
        n.even_odd = f.rule() == usvg::FillRule::EvenOdd;
    }
    if let Some(s) = p.stroke() {
        n.strokes = vec![paint(s.paint(), s.opacity().get(), ts, bounds)];
        // Strokes scale with the transform's average scale.
        let (sx, sy) = ts.get_scale();
        n.stroke_width = s.width().get() * ((sx + sy) / 2.0);
    }
    n.name = if p.id().is_empty() { "Vector".into() } else { p.id().into() };
    Some(n)
}

/// Add usvg children under `parent` (None = page root).
fn add_children(doc: &mut Document, g: &usvg::Group, page: usize, parent: Option<NodeId>, images: &mut Vec<(NodeId, Vec<u8>)>) {
    for child in g.children() {
        match child {
            usvg::Node::Group(sub) => {
                let mut n = Node::new(Kind::Group, Rect::NOTHING);
                n.opacity = sub.opacity().get();
                if !sub.id().is_empty() {
                    n.name = sub.id().into();
                }
                let id = doc.insert(page, parent, None, n);
                add_children(doc, sub, page, Some(id), images);
            }
            usvg::Node::Path(p) => {
                if let Some(n) = path_node(p) {
                    doc.insert(page, parent, None, n);
                }
            }
            usvg::Node::Text(t) => {
                // Text arrives as outlines, matching what the SVG showed.
                let mut n = Node::new(Kind::Group, Rect::NOTHING);
                n.name = "Text".into();
                let id = doc.insert(page, parent, None, n);
                add_children(doc, t.flattened(), page, Some(id), images);
            }
            usvg::Node::Image(img) => {
                let bytes = match img.kind() {
                    usvg::ImageKind::PNG(b) | usvg::ImageKind::JPEG(b) => b.to_vec(),
                    _ => continue,
                };
                let b = img.abs_bounding_box();
                let r = Rect::from_min_size(pos2(b.x(), b.y()), vec2(b.width(), b.height()));
                let mut n = Node::new(Kind::Rectangle, r);
                n.name = "Image".into();
                let id = doc.insert(page, parent, None, n);
                images.push((id, bytes));
            }
        }
    }
}

/// Place an SVG as a frame (like Figma) with its top-left at `at`. Returns the frame and
/// any embedded raster images the caller should store and assign as image fills.
pub fn import(doc: &mut Document, data: &[u8], name: &str, page: usize, parent: Option<NodeId>, at: Pos2) -> Result<(NodeId, Vec<(NodeId, Vec<u8>)>), String> {
    let tree = usvg::Tree::from_data(data, &options()).map_err(|e| e.to_string())?;
    let size = tree.size();
    let mut frame = Node::new(Kind::Frame, Rect::from_min_size(Pos2::ZERO, vec2(size.width(), size.height())));
    frame.fills.clear();
    frame.name = name.to_string();
    let f = doc.insert(page, parent, None, frame);
    let mut images = vec![];
    add_children(doc, tree.root(), page, Some(f), &mut images);
    doc.translate(f, at.to_vec2());
    doc.reflow();
    Ok((f, images))
}

/// Every contour (absolute) and the first fill/stroke found under `id`, via an SVG round trip
/// so rotation, text, and shapes all come out as exact outlines.
pub fn outline(doc: &Document, ids: &[NodeId]) -> Option<Node> {
    let svg = crate::export::to_svg(doc, ids);
    let tree = usvg::Tree::from_str(&svg, &options()).ok()?;
    let origin = crate::export::export_bounds(doc, ids).min.to_vec2();
    let mut subs = vec![];
    let mut style: Option<Node> = None;
    fn walk(g: &usvg::Group, subs: &mut Vec<SubPath>, style: &mut Option<Node>) {
        for c in g.children() {
            match c {
                usvg::Node::Group(sub) => walk(sub, subs, style),
                usvg::Node::Text(t) => walk(t.flattened(), subs, style),
                usvg::Node::Path(p) => {
                    if let Some(n) = path_node(p) {
                        // Anchors are relative to each path's own box; make them absolute again.
                        let o = n.rect.min.to_vec2();
                        subs.extend(n.paths.iter().map(|sp| SubPath {
                            points: sp.points.iter().map(|pt| PathPoint { p: pt.p + o, ..*pt }).collect(),
                            closed: sp.closed,
                        }));
                        if style.is_none() {
                            *style = Some(n);
                        }
                    }
                }
                usvg::Node::Image(_) => {}
            }
        }
    }
    walk(tree.root(), &mut subs, &mut style);
    if subs.is_empty() {
        return None;
    }
    for p in subs.iter_mut().flat_map(|s| s.points.iter_mut()) {
        p.p += origin;
    }
    let mut n = Node::vector_paths(subs);
    if let Some(s) = style {
        // Gradient handles were relative to the first path; keep paints but re-anchor solids only.
        n.fills = s.fills;
        n.strokes = s.strokes;
        n.stroke_width = s.stroke_width;
        n.even_odd = s.even_odd;
    }
    Some(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_paths_groups_gradients_and_holes() {
        let svg = br##"<svg width="100" height="50" viewBox="0 0 100 50" xmlns="http://www.w3.org/2000/svg">
            <defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="#f00"/><stop offset="1" stop-color="#00f"/></linearGradient></defs>
            <g id="icons" transform="translate(10 0)">
              <rect id="box" x="0" y="0" width="20" height="20" fill="url(#g)"/>
              <path id="ring" fill-rule="evenodd" d="M40 0h20v20h-20zM45 5h10v10h-10z" fill="#123456" stroke="#000" stroke-width="2"/>
              <circle cx="80" cy="10" r="5" fill="#0f0" fill-opacity="0.5"/>
            </g>
        </svg>"##;
        let mut d = Document::default();
        let (f, _) = import(&mut d, svg, "icons.svg", 0, None, pos2(100., 100.)).unwrap();
        let frame = d.node(f).clone();
        assert_eq!((frame.kind, frame.rect), (Kind::Frame, Rect::from_min_size(pos2(100., 100.), vec2(100., 50.))));
        let g = frame.children[0];
        assert_eq!(d.node(g).kind, Kind::Group);
        let kids = d.node(g).children.clone();
        assert_eq!(kids.len(), 3);
        let bx = d.node(kids[0]);
        assert_eq!(bx.rect, Rect::from_min_size(pos2(110., 100.), vec2(20., 20.)));
        assert_eq!(bx.fills[0].kind, PaintKind::Linear);
        assert_eq!(bx.fills[0].from, pos2(0., 0.));
        assert_eq!(bx.fills[0].to, pos2(1., 0.));
        let ring = d.node(kids[1]);
        assert_eq!(ring.paths.len(), 2);
        assert!(ring.even_odd && ring.paths.iter().all(|p| p.closed));
        assert_eq!(ring.strokes[0].color, Rgba::BLACK);
        assert_eq!(d.hit(0, pos2(152., 110.), 0.), Some(kids[1]), "solid band hits");
        assert_eq!(d.hit(0, pos2(160., 110.), 0.), Some(f), "hole falls through to the frame");
        let dot = d.node(kids[2]);
        assert_eq!(dot.fills[0].color, Rgba([0, 255, 0, 128]));
        assert_eq!(dot.paths[0].points.len(), 4, "circle = 4 cubic anchors");
    }

    #[test]
    fn outline_turns_shapes_and_text_into_paths() {
        let mut d = Document::default();
        let mut t = Node::new(Kind::Text, Rect::from_min_size(pos2(0., 0.), vec2(40., 15.)));
        t.text = "Hi".into();
        let a = d.insert(0, None, None, t);
        let v = outline(&d, &[a]).unwrap();
        assert!(v.paths.len() >= 2, "H and i outlines: {}", v.paths.len());
        assert_eq!(v.fills[0].color, Rgba::BLACK);
        assert!(v.rect.width() > 5.0 && v.rect.min.x >= 0.0);
    }
}
