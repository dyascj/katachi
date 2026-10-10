//! Canvas rendering: every node becomes a lyon path in screen space, tessellated into egui meshes.

use crate::doc::{Document, Kind, Node, NodeId, Paint, PaintKind, StrokeAlign};
use egui::{ColorImage, TextureHandle, TextureId, TextureOptions};
use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{Color32, Painter, Pos2, Rect, Vec2, pos2, vec2};
use lyon::math::{Angle, Box2D, Transform, point, vector};
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
    let path = b.build();
    if n.rotation == 0.0 {
        return path;
    }
    // Screen y points down, so a counter-clockwise turn is a negative angle.
    let c = v.to_screen(n.rect.center());
    let t = Transform::translation(-c.x, -c.y)
        .then_rotate(Angle::degrees(-n.rotation))
        .then_translate(vector(c.x, c.y));
    path.transformed(&t)
}

fn to_mesh(geo: VertexBuffers<Pos2, u32>, color: Color32) -> Mesh {
    Mesh {
        indices: geo.indices,
        vertices: geo.vertices.into_iter().map(|pos| Vertex { pos, uv: WHITE_UV, color }).collect(),
        ..Default::default()
    }
}

fn fill_geo(path: &Path) -> VertexBuffers<Pos2, u32> {
    let mut geo = VertexBuffers::new();
    let _ = FillTessellator::new().tessellate_path(
        path,
        &FillOptions::tolerance(0.15),
        &mut BuffersBuilder::new(&mut geo, |v: FillVertex| pos2(v.position().x, v.position().y)),
    );
    geo
}

fn stroke_geo(path: &Path, width: f32) -> VertexBuffers<Pos2, u32> {
    let mut geo = VertexBuffers::new();
    let _ = StrokeTessellator::new().tessellate_path(
        path,
        &StrokeOptions::tolerance(0.15).with_line_width(width).with_line_join(LineJoin::Miter),
        &mut BuffersBuilder::new(&mut geo, |v: StrokeVertex| pos2(v.position().x, v.position().y)),
    );
    geo
}

pub fn fill_mesh(path: &Path, color: Color32) -> Mesh {
    to_mesh(fill_geo(path), color)
}

pub fn stroke_mesh(path: &Path, width: f32, color: Color32) -> Mesh {
    to_mesh(stroke_geo(path, width), color)
}

/// GPU textures for gradients and image fills, rebuilt only when a paint changes.
#[derive(Default)]
pub struct Textures {
    gradients: HashMap<u64, TextureHandle>,
    used: HashSet<u64>,
    images: HashMap<String, Option<(TextureHandle, Vec2)>>,
}

const GRAD_RES: usize = 256;

impl Textures {
    /// Drop gradient textures nobody drew last frame.
    pub fn end_frame(&mut self) {
        let used = std::mem::take(&mut self.used);
        self.gradients.retain(|k, _| used.contains(k));
    }

    fn gradient(&mut self, ctx: &egui::Context, p: &Paint) -> TextureId {
        let mut h = DefaultHasher::new();
        (p.kind as u8, p.from.x.to_bits(), p.from.y.to_bits(), p.to.x.to_bits(), p.to.y.to_bits()).hash(&mut h);
        for (t, c) in &p.stops {
            (t.to_bits(), c.0).hash(&mut h);
        }
        let key = h.finish();
        self.used.insert(key);
        self.gradients
            .entry(key)
            .or_insert_with(|| {
                // ponytail: gradients are a 256² texture stretched over the layer; render analytically if banding shows on huge layers
                let px = (0..GRAD_RES * GRAD_RES)
                    .map(|i| {
                        let q = pos2(((i % GRAD_RES) as f32 + 0.5) / GRAD_RES as f32, ((i / GRAD_RES) as f32 + 0.5) / GRAD_RES as f32);
                        p.sample(p.gradient_t(q)).to32()
                    })
                    .collect();
                let img = ColorImage { size: [GRAD_RES, GRAD_RES], pixels: px, source_size: Vec2::splat(GRAD_RES as f32) };
                ctx.load_texture(format!("gradient-{key}"), img, TextureOptions::LINEAR)
            })
            .id()
    }

    pub fn image(&mut self, ctx: &egui::Context, doc: &Document, key: &str) -> Option<(TextureId, Vec2)> {
        let entry = self.images.entry(key.to_string()).or_insert_with(|| {
            let bytes = doc.images.get(key)?;
            let img = image::load_from_memory(bytes).ok()?.to_rgba8();
            let size = [img.width() as usize, img.height() as usize];
            let color = ColorImage::from_rgba_unmultiplied(size, img.as_raw());
            Some((ctx.load_texture(format!("image-{key}"), color, TextureOptions::LINEAR), vec2(size[0] as f32, size[1] as f32)))
        });
        entry.as_ref().map(|(t, s)| (t.id(), *s))
    }
}

/// Everything a draw pass needs besides the painter.
pub struct Scene<'a> {
    pub doc: &'a Document,
    pub view: View,
    pub hide: Option<NodeId>,
    pub tex: &'a mut Textures,
}

/// Paint tessellated geometry with a solid, gradient, or image paint.
fn paint_geo(painter: &Painter, sc: &mut Scene, n: &Node, geo: VertexBuffers<Pos2, u32>, p: &Paint, a: f32) {
    let ctx = painter.ctx();
    let v = sc.view;
    let tint = Color32::WHITE.gamma_multiply(p.alpha() * a);
    // Screen position -> the layer's unrotated unit square.
    let unit = |s: Pos2| {
        let l = crate::doc::rotate(v.to_world(s), n.rect.center(), -n.rotation);
        let r = n.rect;
        pos2((l.x - r.min.x) / r.width().max(1e-6), (l.y - r.min.y) / r.height().max(1e-6))
    };
    let (tex, uv): (TextureId, Box<dyn Fn(Pos2) -> Pos2>) = match p.kind {
        PaintKind::Solid => return void(painter.add(to_mesh(geo, p.color.to32().gamma_multiply(a)))),
        PaintKind::Linear | PaintKind::Radial => (sc.tex.gradient(ctx, p), Box::new(unit)),
        PaintKind::Image => match sc.tex.image(ctx, sc.doc, &p.image) {
            Some((id, size)) => {
                // "Fill" scale mode: cover the layer, centered.
                let (la, ia) = (n.rect.width() / n.rect.height().max(1e-6), size.x / size.y.max(1e-6));
                let (su, sv) = if la > ia { (1.0, ia / la) } else { (la / ia, 1.0) };
                (id, Box::new(move |s| {
                    let q = unit(s);
                    pos2(0.5 + (q.x - 0.5) * su, 0.5 + (q.y - 0.5) * sv)
                }))
            }
            None => return void(painter.add(to_mesh(geo, crate::doc::SHAPE_GRAY.to32().gamma_multiply(a)))),
        },
    };
    painter.add(Mesh {
        indices: geo.indices,
        vertices: geo.vertices.into_iter().map(|pos| Vertex { pos, uv: uv(pos), color: tint }).collect(),
        texture_id: tex,
    });
}

fn void<T>(_: T) {}

/// Paint a page.
pub fn draw_page(painter: &Painter, sc: &mut Scene, page: usize) {
    for &id in &sc.doc.pages[page].children {
        draw_node(painter, sc, id, 1.0);
    }
}

// ponytail: opacity multiplies into colors per node, not true group compositing; use offscreen layers when overlaps matter
fn draw_node(painter: &Painter, sc: &mut Scene, id: NodeId, alpha: f32) {
    let n = sc.doc.node(id);
    if !n.visible {
        return;
    }
    let v = sc.view;
    let screen = v.rect_to_screen(n.aabb());
    let a = alpha * n.opacity;
    let reach = n.stroke_width.max(n.shadows.iter().map(|s| s.blur + s.spread + s.offset.length()).fold(0.0, f32::max));
    if Some(id) != sc.hide && painter.clip_rect().intersects(screen.expand(reach * v.zoom + 2.0)) {
        draw_shadows(painter, n, &v, a);
        draw_content(painter, sc, n, a);
    }
    if n.children.is_empty() {
        return;
    }
    let child_painter = if n.kind.is_frame() && n.clip {
        painter.with_clip_rect(painter.clip_rect().intersect(screen))
    } else {
        painter.clone()
    };
    for &c in &n.children.clone() {
        draw_node(&child_painter, sc, c, a);
    }
}

fn draw_shadows(painter: &Painter, n: &Node, v: &View, a: f32) {
    for s in n.shadows.iter().filter(|s| s.visible) {
        let z = v.zoom;
        let color = s.color.to32().gamma_multiply(a);
        let rect_like = (n.kind == Kind::Rectangle || n.kind.is_frame()) && n.rotation == 0.0;
        if rect_like {
            let r = v.rect_to_screen(n.rect).translate(s.offset * z).expand(s.spread * z);
            let rad = ((n.radius + s.spread).max(0.0) * z).min(255.0) as u8;
            painter.add(egui::epaint::RectShape::filled(r, egui::CornerRadius::same(rad), color).with_blur_width(s.blur * z));
        } else if n.kind != Kind::Group && n.kind != Kind::Text {
            // ponytail: stacked offsets approximate the blur for non-rect shapes; exact in export
            let shifted = View { pan: v.pan + s.offset * z, zoom: z };
            const STEPS: usize = 6;
            for i in 0..STEPS {
                let grow = s.spread * z + s.blur * z * ((i as f32 + 0.5) / STEPS as f32 - 0.5);
                painter.add(fill_mesh(&node_path(n, &shifted, grow), color.gamma_multiply(1.0 / STEPS as f32 * 1.6)));
            }
        }
    }
}

fn draw_content(painter: &Painter, sc: &mut Scene, n: &Node, a: f32) {
    let v = sc.view;
    if n.kind == Kind::Group {
        return;
    }
    if n.kind == Kind::Text {
        if let Some(c) = n.first_fill().map(|c| c.to32().gamma_multiply(a)) {
            let (galley, _, _) = crate::text::layout(painter, n, v.zoom, c);
            let local = n.rect.min + crate::text::draw_offset(n, galley.rows.len());
            let pos = v.to_screen(crate::doc::rotate(local, n.rect.center(), n.rotation));
            painter.add(egui::epaint::TextShape::new(pos, galley, c).with_angle(-n.rotation.to_radians()));
        }
        return;
    }
    let path = node_path(n, &v, 0.0);
    if n.kind != Kind::Vector || n.closed {
        for f in n.fills.iter().filter(|f| f.visible) {
            paint_geo(painter, sc, n, fill_geo(&path), f, a);
        }
    }
    if n.strokes.iter().any(|s| s.visible) && n.stroke_width > 0.0 {
        let w = n.stroke_width * v.zoom;
        let grow = match n.stroke_align {
            StrokeAlign::Inside => -w / 2.0,
            StrokeAlign::Center => 0.0,
            StrokeAlign::Outside => w / 2.0,
        };
        let spath = if grow == 0.0 || n.kind == Kind::Vector { path } else { node_path(n, &v, grow) };
        for s in n.strokes.iter().filter(|s| s.visible) {
            paint_geo(painter, sc, n, stroke_geo(&spath, w), s, a);
        }
    }
}

