//! Canvas rendering: every node becomes a lyon path in screen space, tessellated into egui meshes.

use crate::doc::{Cap, Document, EffectKind, Join, Kind, Node, NodeId, Paint, PaintKind, StrokeAlign};
use egui::{ColorImage, TextureHandle, TextureId, TextureOptions};
use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{Color32, Painter, Pos2, Rect, Vec2, pos2, vec2};
use lyon::math::{Angle, Box2D, Transform, point, vector};
use lyon::path::builder::BorderRadii;
use lyon::path::{Path, Winding};
use lyon::tessellation::{
    BuffersBuilder, FillOptions, FillTessellator, FillVertex, LineCap, LineJoin, StrokeOptions, StrokeTessellator,
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
        Kind::Polygon | Kind::Star => {
            let pts: Vec<Pos2> = n.star_points().iter().map(|p| v.to_screen(*p)).collect();
            // Grow (stroke alignment) pushes points away from the center.
            let c = r.center();
            let pts: Vec<Pos2> = pts.iter().map(|p| *p + (*p - c).normalized() * grow).collect();
            b.begin(point(pts[0].x, pts[0].y));
            for p in &pts[1..] {
                b.line_to(point(p.x, p.y));
            }
            b.end(true);
        }
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
            for sp in n.paths.iter().filter(|sp| sp.points.len() >= 2 || (sp.closed && !sp.points.is_empty())) {
                let pts = &sp.points;
                b.begin(s(pts[0].p));
                let segs = if sp.closed { pts.len() } else { pts.len() - 1 };
                for i in 0..segs {
                    let (a, c) = (pts[i], pts[(i + 1) % pts.len()]);
                    b.cubic_bezier_to(s(a.p + a.h_out), s(c.p + c.h_in), s(c.p));
                }
                b.end(sp.closed);
            }
        }
        _ => {
            let lim = r.width().min(r.height()) / 2.0;
            let [tl, tr, br, bl] = n.radii().map(|x| (x * v.zoom + grow).clamp(0.0, lim));
            let bx = Box2D::new(point(r.min.x, r.min.y), point(r.max.x, r.max.y));
            let radii = BorderRadii { top_left: tl, top_right: tr, bottom_right: br, bottom_left: bl };
            b.add_rounded_rectangle(&bx, &radii, Winding::Positive);
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

fn fill_geo(path: &Path, even_odd: bool) -> VertexBuffers<Pos2, u32> {
    let mut geo = VertexBuffers::new();
    let rule = if even_odd { lyon::tessellation::FillRule::EvenOdd } else { lyon::tessellation::FillRule::NonZero };
    let _ = FillTessellator::new().tessellate_path(
        path,
        &FillOptions::tolerance(0.15).with_fill_rule(rule),
        &mut BuffersBuilder::new(&mut geo, |v: FillVertex| pos2(v.position().x, v.position().y)),
    );
    geo
}

fn stroke_geo(path: &Path, width: f32) -> VertexBuffers<Pos2, u32> {
    stroke_geo_styled(path, width, Cap::None, Join::Miter)
}

fn stroke_geo_styled(path: &Path, width: f32, cap: Cap, join: Join) -> VertexBuffers<Pos2, u32> {
    let cap = match cap {
        Cap::None => LineCap::Butt,
        Cap::Round => LineCap::Round,
        Cap::Square => LineCap::Square,
    };
    let join = match join {
        Join::Miter => LineJoin::Miter,
        Join::Round => LineJoin::Round,
        Join::Bevel => LineJoin::Bevel,
    };
    let mut geo = VertexBuffers::new();
    let _ = StrokeTessellator::new().tessellate_path(
        path,
        &StrokeOptions::tolerance(0.15).with_line_width(width).with_line_join(join).with_line_cap(cap),
        &mut BuffersBuilder::new(&mut geo, |v: StrokeVertex| pos2(v.position().x, v.position().y)),
    );
    geo
}

pub fn fill_mesh(path: &Path, color: Color32) -> Mesh {
    to_mesh(fill_geo(path, false), color)
}

pub fn stroke_mesh(path: &Path, width: f32, color: Color32) -> Mesh {
    to_mesh(stroke_geo(path, width), color)
}

/// Endpoints of open contours that carry markers: (world tip, outward direction, marker).
pub fn marker_ends(n: &Node) -> Vec<(Pos2, Vec2, crate::doc::Marker)> {
    use crate::doc::Marker;
    if n.kind != Kind::Vector || (n.start_marker == Marker::None && n.end_marker == Marker::None) {
        return vec![];
    }
    let o = n.rect.min.to_vec2();
    let mut out = vec![];
    for sp in n.paths.iter().filter(|sp| !sp.closed && sp.points.len() >= 2) {
        let (a, b) = (sp.points[0], sp.points[1]);
        let (y, z) = (sp.points[sp.points.len() - 2], sp.points[sp.points.len() - 1]);
        // Tangent at each end: the handle if there is one, else toward the neighbor.
        let start_dir = if a.h_out != Vec2::ZERO { -a.h_out } else { a.p - (b.p + b.h_in) };
        let end_dir = if z.h_in != Vec2::ZERO { -z.h_in } else { z.p - (y.p + y.h_out) };
        if n.start_marker != Marker::None {
            out.push((crate::doc::rotate(a.p + o, n.rect.center(), n.rotation), rot_vec(start_dir, n.rotation), n.start_marker));
        }
        if n.end_marker != Marker::None {
            out.push((crate::doc::rotate(z.p + o, n.rect.center(), n.rotation), rot_vec(end_dir, n.rotation), n.end_marker));
        }
    }
    out
}

fn rot_vec(v: Vec2, deg: f32) -> Vec2 {
    crate::doc::rotate(Pos2::ZERO + v, Pos2::ZERO, deg).to_vec2()
}

/// Marker outline points at `tip` pointing along `dir`, sized for stroke width `w` (screen units).
pub fn marker_shape(tip: Pos2, dir: Vec2, m: crate::doc::Marker, w: f32) -> (Vec<Pos2>, bool) {
    use crate::doc::Marker;
    let d = dir.normalized();
    let n = d.rot90();
    let s = (w * 3.0).max(6.0);
    match m {
        Marker::Arrow => (vec![tip - d * s + n * s * 0.6, tip, tip - d * s - n * s * 0.6], false),
        Marker::Triangle => (vec![tip + d * s * 0.3, tip - d * s * 0.9 + n * s * 0.6, tip - d * s * 0.9 - n * s * 0.6], true),
        Marker::Circle => ((0..16).map(|i| tip + Vec2::angled(i as f32 / 16.0 * std::f32::consts::TAU) * s * 0.4).collect(), true),
        Marker::None => (vec![], false),
    }
}

fn draw_marker(painter: &Painter, tip: Pos2, dir: Vec2, m: crate::doc::Marker, w: f32, c: Color32) {
    let (pts, filled) = marker_shape(tip, dir, m, w);
    if pts.len() < 2 {
        return;
    }
    let mut b = Path::builder();
    b.begin(point(pts[0].x, pts[0].y));
    for p in &pts[1..] {
        b.line_to(point(p.x, p.y));
    }
    b.end(filled);
    let path = b.build();
    if filled {
        painter.add(fill_mesh(&path, c));
    } else {
        painter.add(to_mesh(stroke_geo_styled(&path, w, Cap::Round, Join::Round), c));
    }
}

/// Approximate a Gaussian blur (std dev `sigma` px) with three box blurs per axis.
pub fn blur_rgba(px: &mut [u8], w: usize, h: usize, sigma: f32) {
    if sigma < 0.5 || w == 0 || h == 0 {
        return;
    }
    let r = (((12.0 * sigma * sigma / 3.0) + 1.0).sqrt() / 2.0).floor().max(1.0) as usize;
    let mut buf = vec![0u8; px.len()];
    let pass = |src: &[u8], dst: &mut [u8], len: usize, lines: usize, at: &dyn Fn(usize, usize) -> usize| {
        let win = (2 * r + 1) as u32;
        for l in 0..lines {
            let mut acc = [0u32; 4];
            let px_at = |i: isize| at(l, i.clamp(0, len as isize - 1) as usize) * 4;
            for i in -(r as isize)..=(r as isize) {
                let o = px_at(i);
                for c in 0..4 {
                    acc[c] += src[o + c] as u32;
                }
            }
            for i in 0..len {
                let o = at(l, i) * 4;
                for c in 0..4 {
                    dst[o + c] = (acc[c] / win) as u8;
                }
                let (add, sub) = (px_at(i as isize + r as isize + 1), px_at(i as isize - r as isize));
                for c in 0..4 {
                    acc[c] = acc[c] + src[add + c] as u32 - src[sub + c] as u32;
                }
            }
        }
    };
    for _ in 0..3 {
        pass(px, &mut buf, w, h, &|y, x| y * w + x);
        pass(&buf, px, h, w, &|x, y| y * w + x);
    }
}

/// Cut a path into dashes following `pattern` (on, off, on, off, ...), in screen units.
pub fn dashed(path: &Path, pattern: &[f32]) -> Path {
    use lyon::path::Event;
    use lyon::path::iterator::PathIterator;
    let pattern: Vec<f32> = if pattern.len() % 2 == 1 { pattern.iter().chain(pattern).copied().collect() } else { pattern.to_vec() };
    let total: f32 = pattern.iter().sum();
    let mut b = Path::builder();
    if total <= 0.0 {
        return path.clone();
    }
    let (mut idx, mut left, mut on, mut open) = (0usize, pattern[0], true, false);
    for ev in path.iter().flattened(0.15) {
        match ev {
            Event::Begin { at } => {
                (idx, left, on) = (0, pattern[0], true);
                b.begin(at);
                open = true;
            }
            Event::Line { from, to } | Event::End { last: from, first: to, close: true } => {
                let mut a = from;
                let mut seg = (to - from).length();
                while seg > 0.0 {
                    let step = seg.min(left);
                    let p = a + (to - a).normalize() * step;
                    if on {
                        if !open {
                            b.begin(a);
                            open = true;
                        }
                        b.line_to(p);
                    }
                    seg -= step;
                    left -= step;
                    a = p;
                    if left <= 1e-4 {
                        if on && open {
                            b.end(false);
                            open = false;
                        }
                        idx = (idx + 1) % pattern.len();
                        left = pattern[idx];
                        on = idx % 2 == 0;
                    }
                }
                if matches!(ev, Event::End { .. }) && open {
                    b.end(false);
                    open = false;
                }
            }
            Event::End { .. } => {
                if open {
                    b.end(false);
                    open = false;
                }
            }
            _ => {}
        }
    }
    if open {
        b.end(false);
    }
    b.build()
}

/// GPU textures for gradients and image fills, rebuilt only when a paint changes.
#[derive(Default)]
pub struct Textures {
    gradients: HashMap<u64, TextureHandle>,
    /// Layers drawn through resvg (blurs, inner shadows), keyed by SVG content and scale.
    rasters: HashMap<u64, TextureHandle>,
    used: HashSet<u64>,
    images: HashMap<String, Option<(TextureHandle, Vec2)>>,
}

const GRAD_RES: usize = 256;

impl Textures {
    /// Drop gradient textures nobody drew last frame.
    pub fn end_frame(&mut self) {
        let used = std::mem::take(&mut self.used);
        self.gradients.retain(|k, _| used.contains(k));
        self.rasters.retain(|k, _| used.contains(k));
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

    /// SVG rendered by resvg at `scale` (snapped to half-octaves so zooming doesn't re-render every frame).
    fn raster(&mut self, ctx: &egui::Context, svg: &str, scale: f32) -> Option<TextureId> {
        let scale = 2f32.powf((scale.max(0.01).log2() * 2.0).ceil() / 2.0);
        let mut h = DefaultHasher::new();
        (svg, scale.to_bits()).hash(&mut h);
        let key = h.finish();
        self.used.insert(key);
        if let Some(t) = self.rasters.get(&key) {
            return Some(t.id());
        }
        let (px, size) = crate::export::rasterize(svg, scale)?;
        let img = ColorImage::from_rgba_premultiplied(size, &px);
        let t = ctx.load_texture(format!("raster-{key}"), img, TextureOptions::LINEAR);
        let id = t.id();
        self.rasters.insert(key, t);
        Some(id)
    }

    /// Blurred backdrop masked to a layer's shape (see `export::backdrop_parts`).
    fn backdrop(&mut self, ctx: &egui::Context, (content, mask, _, blur): (String, String, Rect, f32), scale: f32) -> Option<TextureId> {
        let scale = 2f32.powf((scale.max(0.01).log2() * 2.0).ceil() / 2.0);
        let mut h = DefaultHasher::new();
        (&content, &mask, blur.to_bits(), scale.to_bits()).hash(&mut h);
        let key = h.finish();
        self.used.insert(key);
        if let Some(t) = self.rasters.get(&key) {
            return Some(t.id());
        }
        let (mut px, size) = crate::export::rasterize(&content, scale)?;
        let (m, msize) = crate::export::rasterize(&mask, scale)?;
        if msize != size {
            return None;
        }
        blur_rgba(&mut px, size[0], size[1], blur / 2.0 * scale);
        for (p, a) in px.chunks_exact_mut(4).zip(m.chunks_exact(4).map(|c| c[3] as u32)) {
            for c in p.iter_mut() {
                *c = (*c as u32 * a / 255) as u8;
            }
        }
        let t = ctx.load_texture(format!("backdrop-{key}"), ColorImage::from_rgba_premultiplied(size, &px), TextureOptions::LINEAR);
        let id = t.id();
        self.rasters.insert(key, t);
        Some(id)
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

/// Paint one layer and its children (prototype playback).
pub fn draw_subtree(painter: &Painter, sc: &mut Scene, id: NodeId, alpha: f32) {
    draw_node(painter, sc, id, alpha);
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
    let ppp = painter.ctx().pixels_per_point();
    if n.shadows.iter().any(|s| s.visible && s.kind == EffectKind::BackgroundBlur)
        && let Some(parts) = crate::export::backdrop_parts(sc.doc, id) {
            let region = parts.2;
            if let Some(t) = sc.tex.backdrop(painter.ctx(), parts, v.zoom * ppp) {
                painter.image(t, v.rect_to_screen(region), Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
            }
        }
    if Some(id) != sc.hide && n.shadows.iter().any(|s| s.visible && matches!(s.kind, EffectKind::InnerShadow | EffectKind::LayerBlur)) {
        // egui can't blur on the GPU: draw this layer (and its children) through resvg, exactly like export.
        let svg = crate::export::to_svg(sc.doc, &[id]);
        let region = crate::export::export_bounds(sc.doc, &[id]);
        if let Some(t) = sc.tex.raster(painter.ctx(), &svg, v.zoom * ppp) {
            let tint = Color32::WHITE.gamma_multiply(alpha);
            painter.image(t, v.rect_to_screen(region), Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), tint);
        }
        return;
    }
    if n.kind == Kind::Boolean {
        // Operands aren't drawn themselves; the result carries the paint.
        if let Some(r) = crate::boolean::as_vector(sc.doc, id) {
            draw_shadows(painter, &r, &v, alpha);
            draw_content(painter, sc, &r, a);
        }
        return;
    }
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
    for s in n.shadows.iter().filter(|s| s.visible && s.kind == EffectKind::DropShadow) {
        let z = v.zoom;
        let color = s.color.to32().gamma_multiply(a);
        let rect_like = (n.kind == Kind::Rectangle || n.kind.is_frame()) && n.rotation == 0.0;
        if rect_like {
            let r = v.rect_to_screen(n.rect).translate(s.offset * z).expand(s.spread * z);
            let [tl, tr, br, bl] = n.radii().map(|x| ((x + s.spread).max(0.0) * z).min(255.0) as u8);
            let cr = egui::CornerRadius { nw: tl, ne: tr, se: br, sw: bl };
            painter.add(egui::epaint::RectShape::filled(r, cr, color).with_blur_width(s.blur * z));
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
    if matches!(n.kind, Kind::Group | Kind::Slice) {
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
    if n.has_area() {
        for f in n.fills.iter().filter(|f| f.visible) {
            paint_geo(painter, sc, n, fill_geo(&path, n.even_odd), f, a);
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
        let spath = if n.dash.iter().any(|d| *d > 0.0) { dashed(&spath, &n.dash.iter().map(|d| d * v.zoom).collect::<Vec<_>>()) } else { spath };
        for s in n.strokes.iter().filter(|s| s.visible) {
            paint_geo(painter, sc, n, stroke_geo_styled(&spath, w, n.cap, n.join), s, a);
        }
        if let Some(c) = n.strokes.iter().rev().find(|s| s.visible).map(|s| s.color.to32().gamma_multiply(a)) {
            for (tip, dir, m) in marker_ends(n) {
                draw_marker(painter, v.to_screen(tip), dir, m, w, c);
            }
        }
    }
}

