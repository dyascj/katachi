//! Canvas rendering: every node becomes a lyon path in screen space, tessellated into egui meshes.

use crate::doc::{Document, Kind, Node, NodeId, StrokeAlign};
use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{Color32, FontFamily, FontId, Painter, Pos2, Rect, Vec2, pos2};
use lyon::math::{Angle, Box2D, point, vector};
use lyon::path::builder::BorderRadii;
use lyon::path::{Path, Winding};
use lyon::tessellation::{
    BuffersBuilder, FillOptions, FillTessellator, FillVertex, LineJoin, StrokeOptions, StrokeTessellator,
    StrokeVertex, VertexBuffers,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// Screen position of the world origin.
    pub pan: Vec2,
    pub zoom: f32,
}

impl View {
    pub fn to_screen(self, p: Pos2) -> Pos2 {
        (p.to_vec2() * self.zoom + self.pan).to_pos2()
    }
    pub fn to_world(self, p: Pos2) -> Pos2 {
        ((p.to_vec2() - self.pan) / self.zoom).to_pos2()
    }
    pub fn rect_to_screen(self, r: Rect) -> Rect {
        Rect::from_min_max(self.to_screen(r.min), self.to_screen(r.max))
    }
    /// Zoom by `factor`, keeping world point under `anchor` (screen) fixed.
    pub fn zoom_at(&mut self, anchor: Pos2, factor: f32) {
        let w = self.to_world(anchor);
        self.zoom = (self.zoom * factor).clamp(0.02, 256.0);
        self.pan = anchor.to_vec2() - w.to_vec2() * self.zoom;
    }
    /// Fit world rect `r` into screen rect `screen`, never zooming past 100%.
    pub fn fit(r: Rect, screen: Rect, max_zoom: f32) -> Self {
        if !r.is_positive() {
            return View { pan: screen.center().to_vec2() - r.center().to_vec2(), zoom: 1.0 };
        }
        let pad = screen.shrink(48.0);
        let zoom = (pad.width() / r.width()).min(pad.height() / r.height()).clamp(0.02, max_zoom);
        View { zoom, pan: screen.center().to_vec2() - r.center().to_vec2() * zoom }
    }
}

pub fn font_family(weight: u16) -> FontFamily {
    match weight {
        0..=449 => FontFamily::Proportional,
        450..=549 => FontFamily::Name("Inter Medium".into()),
        550..=649 => FontFamily::Name("Inter SemiBold".into()),
        _ => FontFamily::Name("Inter Bold".into()),
    }
}

/// Node outline in screen space, grown outward by `grow` px (negative = inset).
pub fn node_path(n: &Node, v: &View, grow: f32) -> Path {
    let mut b = Path::builder();
    let r = v.rect_to_screen(n.rect).expand(grow);
    if n.kind != Kind::Vector && (r.width() <= 0.0 || r.height() <= 0.0) {
        return b.build();
    }
    match n.kind {
        Kind::Ellipse => {
            let c = r.center();
            b.add_ellipse(
                point(c.x, c.y),
                vector(r.width() / 2.0, r.height() / 2.0),
                Angle::zero(),
                Winding::Positive,
            );
        }
        Kind::Vector => {
            let o = n.rect.min.to_vec2();
            let s = |p: Pos2| {
                let q = v.to_screen(p + o);
                point(q.x, q.y)
            };
            let pts = &n.points;
            if pts.len() >= 2 {
                b.begin(s(pts[0].p));
                let segs = if n.closed { pts.len() } else { pts.len() - 1 };
                for i in 0..segs {
                    let (a, c) = (pts[i], pts[(i + 1) % pts.len()]);
                    b.cubic_bezier_to(s(a.p + a.h_out), s(c.p + c.h_in), s(c.p));
                }
                b.end(n.closed);
            }
        }
        _ => {
            let rad = (n.radius * v.zoom + grow).clamp(0.0, r.width().min(r.height()) / 2.0);
            let bx = Box2D::new(point(r.min.x, r.min.y), point(r.max.x, r.max.y));
            b.add_rounded_rectangle(&bx, &BorderRadii::new(rad), Winding::Positive);
        }
    }
    b.build()
}

fn to_mesh(geo: VertexBuffers<Pos2, u32>, color: Color32) -> Mesh {
    Mesh {
        indices: geo.indices,
        vertices: geo.vertices.into_iter().map(|pos| Vertex { pos, uv: WHITE_UV, color }).collect(),
        ..Default::default()
    }
}

pub fn fill_mesh(path: &Path, color: Color32) -> Mesh {
    let mut geo = VertexBuffers::new();
    let _ = FillTessellator::new().tessellate_path(
        path,
        &FillOptions::tolerance(0.15),
        &mut BuffersBuilder::new(&mut geo, |v: FillVertex| pos2(v.position().x, v.position().y)),
    );
    to_mesh(geo, color)
}

pub fn stroke_mesh(path: &Path, width: f32, color: Color32) -> Mesh {
    let mut geo = VertexBuffers::new();
    let _ = StrokeTessellator::new().tessellate_path(
        path,
        &StrokeOptions::tolerance(0.15).with_line_width(width).with_line_join(LineJoin::Miter),
        &mut BuffersBuilder::new(&mut geo, |v: StrokeVertex| pos2(v.position().x, v.position().y)),
    );
    to_mesh(geo, color)
}

/// Paint a page. `hide` skips one node's own content (used while editing text in place).
pub fn draw_page(painter: &Painter, doc: &Document, page: usize, v: &View, hide: Option<NodeId>) {
    for &id in &doc.pages[page].children {
        draw_node(painter, doc, id, v, 1.0, hide);
    }
}

// ponytail: opacity multiplies into colors per node, not true group compositing; use offscreen layers when overlaps matter
fn draw_node(painter: &Painter, doc: &Document, id: NodeId, v: &View, alpha: f32, hide: Option<NodeId>) {
    let n = doc.node(id);
    if !n.visible {
        return;
    }
    let screen = v.rect_to_screen(n.rect);
    let a = alpha * n.opacity;
    if Some(id) != hide && painter.clip_rect().intersects(screen.expand(n.stroke_width * v.zoom + 2.0)) {
        draw_content(painter, n, v, a);
    }
    if n.children.is_empty() {
        return;
    }
    let child_painter = if n.kind == Kind::Frame && n.clip {
        painter.with_clip_rect(painter.clip_rect().intersect(screen))
    } else {
        painter.clone()
    };
    for &c in &n.children {
        draw_node(&child_painter, doc, c, v, a, hide);
    }
}

fn draw_content(painter: &Painter, n: &Node, v: &View, a: f32) {
    if n.kind == Kind::Group {
        return;
    }
    if n.kind == Kind::Text {
        if let Some(c) = n.first_fill() {
            let galley = painter.layout_no_wrap(
                n.text.clone(),
                FontId::new(n.font_size * v.zoom, font_family(n.font_weight)),
                c.gamma_multiply(a),
            );
            painter.galley(v.to_screen(n.rect.min), galley, c);
        }
        return;
    }
    let path = node_path(n, v, 0.0);
    for f in n.fills.iter().filter(|f| f.visible) {
        if n.kind != Kind::Vector || n.closed {
            painter.add(fill_mesh(&path, f.color.gamma_multiply(a)));
        }
    }
    if n.strokes.iter().any(|s| s.visible) && n.stroke_width > 0.0 {
        let w = n.stroke_width * v.zoom;
        let grow = match n.stroke_align {
            StrokeAlign::Inside => -w / 2.0,
            StrokeAlign::Center => 0.0,
            StrokeAlign::Outside => w / 2.0,
        };
        let spath = if grow == 0.0 || n.kind == Kind::Vector { path } else { node_path(n, v, grow) };
        for s in n.strokes.iter().filter(|s| s.visible) {
            painter.add(stroke_mesh(&spath, w, s.color.gamma_multiply(a)));
        }
    }
}

/// Size of a text node's content at 100% zoom.
pub fn measure_text(painter: &Painter, n: &Node) -> Vec2 {
    let text = if n.text.is_empty() { " ".to_string() } else { n.text.clone() };
    painter
        .layout_no_wrap(text, FontId::new(n.font_size, font_family(n.font_weight)), Color32::BLACK)
        .size()
}
