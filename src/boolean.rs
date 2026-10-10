//! Live boolean groups: the shape is recomputed from the operand children whenever it's needed.

use crate::doc::{BoolOp, Document, Kind, Node, NodeId, PathPoint, SubPath};
use crate::render::{View, node_path};
use egui::{Pos2, Vec2, pos2};
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;
use lyon::path::iterator::PathIterator;

type Contours = Vec<Vec<[f64; 2]>>;

const IDENTITY: View = View { pan: Vec2::ZERO, zoom: 1.0 };

/// Closed outlines of one operand in world space.
fn operand(doc: &Document, id: NodeId) -> Contours {
    let n = doc.node(id);
    if !n.visible {
        return vec![];
    }
    match n.kind {
        Kind::Boolean => result_contours(doc, id),
        // Groups and frames contribute the union of their children.
        Kind::Group | Kind::Frame | Kind::Component | Kind::Instance if !n.children.is_empty() => {
            let mut acc: Contours = vec![];
            for &c in &n.children {
                acc = union(&acc, &operand(doc, c));
            }
            if n.kind != Kind::Group {
                acc = union(&acc, &shape_contours(n));
            }
            acc
        }
        Kind::Text => vec![],
        _ => shape_contours(n),
    }
}

fn shape_contours(n: &Node) -> Contours {
    // ponytail: booleans run on flattened curves (0.1 px tolerance); results are polygons, not beziers
    let path = node_path(n, &IDENTITY, 0.0);
    let mut out: Contours = vec![];
    let mut cur: Vec<[f64; 2]> = vec![];
    for ev in path.iter().flattened(0.1) {
        use lyon::path::Event;
        match ev {
            Event::Begin { at } => cur = vec![[at.x as f64, at.y as f64]],
            Event::Line { to, .. } => cur.push([to.x as f64, to.y as f64]),
            Event::End { .. } => {
                if cur.len() >= 3 {
                    out.push(std::mem::take(&mut cur));
                }
            }
            _ => {}
        }
    }
    out
}

fn op(a: &Contours, b: &Contours, rule: OverlayRule) -> Contours {
    if a.is_empty() && b.is_empty() {
        return vec![];
    }
    a.overlay(b, rule, FillRule::NonZero).into_iter().flatten().collect()
}

fn union(a: &Contours, b: &Contours) -> Contours {
    if a.is_empty() {
        return op(b, &vec![], OverlayRule::Subject);
    }
    op(a, b, OverlayRule::Union)
}

fn result_contours(doc: &Document, id: NodeId) -> Contours {
    let n = doc.node(id);
    let mut kids = n.children.iter().map(|&c| operand(doc, c)).filter(|c| !c.is_empty());
    let Some(first) = kids.next() else { return vec![] };
    // Subtract removes the upper layers from the bottom one, like Figma.
    kids.fold(op(&first, &vec![], OverlayRule::Subject), |acc, k| match n.bool_op {
        BoolOp::Union => op(&acc, &k, OverlayRule::Union),
        BoolOp::Subtract => op(&acc, &k, OverlayRule::Difference),
        BoolOp::Intersect => op(&acc, &k, OverlayRule::Intersect),
        BoolOp::Exclude => op(&acc, &k, OverlayRule::Xor),
    })
}

/// The boolean's resulting outline as absolute contours.
pub fn result(doc: &Document, id: NodeId) -> Vec<SubPath> {
    result_contours(doc, id)
        .into_iter()
        .map(|c| SubPath {
            points: c.into_iter().map(|[x, y]| PathPoint { p: pos2(x as f32, y as f32), ..Default::default() }).collect(),
            closed: true,
        })
        .collect()
}

/// A stand-in vector carrying the boolean's paint, for drawing, hit testing, and export.
pub fn as_vector(doc: &Document, id: NodeId) -> Option<Node> {
    let n = doc.node(id);
    let paths = result(doc, id);
    if paths.is_empty() {
        return None;
    }
    let mut v = Node::vector_paths(paths);
    v.id = n.id;
    v.fills = n.fills.clone();
    v.strokes = n.strokes.clone();
    v.stroke_width = n.stroke_width;
    v.stroke_align = crate::doc::StrokeAlign::Center;
    v.shadows = n.shadows.clone();
    v.opacity = n.opacity;
    v.even_odd = true;
    Some(v)
}

/// World point inside the boolean's result.
pub fn contains(doc: &Document, id: NodeId, p: Pos2) -> bool {
    let polys: Vec<(Vec<Pos2>, bool)> = result(doc, id).into_iter().map(|sp| (sp.points.iter().map(|pt| pt.p).collect(), true)).collect();
    crate::doc::contains(&polys, p, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Rect, vec2};

    fn sq(d: &mut Document, parent: Option<NodeId>, x: f32, y: f32, w: f32) -> NodeId {
        d.insert(0, parent, None, Node::new(Kind::Rectangle, Rect::from_min_size(pos2(x, y), vec2(w, w))))
    }

    fn area(paths: &[SubPath]) -> f32 {
        // Shoelace, signed by orientation; outer and hole contours come out opposite.
        paths
            .iter()
            .map(|sp| {
                let p = &sp.points;
                (0..p.len()).map(|i| {
                    let (a, b) = (p[i].p, p[(i + 1) % p.len()].p);
                    a.x * b.y - b.x * a.y
                }).sum::<f32>() / 2.0
            })
            .sum::<f32>()
            .abs()
    }

    #[test]
    fn ops_combine_operands() {
        let mut d = Document::default();
        let a = sq(&mut d, None, 0., 0., 10.);
        let b = sq(&mut d, None, 5., 5., 10.);
        let g = d.boolean(&[a, b], BoolOp::Union).unwrap();
        assert_eq!(d.node(g).kind, Kind::Boolean);
        assert_eq!(d.node(g).rect, Rect::from_min_size(pos2(0., 0.), vec2(15., 15.)));
        assert!((area(&result(&d, g)) - 175.).abs() < 0.5);
        for (o, want) in [(BoolOp::Subtract, 75.), (BoolOp::Intersect, 25.), (BoolOp::Exclude, 150.)] {
            d.node_mut(g).bool_op = o;
            assert!((area(&result(&d, g)) - want).abs() < 0.5, "{o:?}: {}", area(&result(&d, g)));
        }
        // Hit testing follows the result: the intersection hole is empty under Exclude.
        assert_eq!(d.hit(0, pos2(7., 7.), 0.), None);
        assert_eq!(d.hit(0, pos2(2., 2.), 0.), Some(a), "click reaches the operand under the result");
        assert!(contains(&d, g, pos2(2., 2.)));
    }
}
