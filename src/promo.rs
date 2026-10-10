//! The promo film: Katachi renders its own launch video. The real app runs headlessly (driven by
//! scripted pointer and keyboard input and by its MCP tools), and a second egui scene composites it
//! with motion graphics. `cargo test --release promo_video -- --ignored --nocapture` writes
//! target/promo/katachi.mp4 (PROMO_STILLS=30 also saves every 30th frame for review).

use crate::app::App;
use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, ColorImage, FontFamily, FontId, Galley, Key, Modifiers, Pos2, Rect, Shape, Stroke, TextureHandle, TextureOptions, Vec2, pos2, vec2};
use egui_kittest::Harness;
use std::io::Write;
use std::sync::Arc;

const W: f32 = 1920.0;
const H: f32 = 1080.0;
const FPS: f32 = 60.0;
/// The app window renders at this size (points) and twice the pixels, for crisp text when scaled.
const AW: f32 = 1440.0;
const AH: f32 = 900.0;
const APP_PPP: f32 = 2.0;

const INK: Color32 = Color32::from_rgb(0x0E, 0x0D, 0x14);
const VIOLET: Color32 = Color32::from_rgb(0x8E, 0x72, 0xFF);
const CORAL: Color32 = Color32::from_rgb(0xFF, 0x5A, 0x3C);
const RUST: Color32 = Color32::from_rgb(0xDE, 0x6A, 0x2D);
const SUN: Color32 = Color32::from_rgb(0xFF, 0xC9, 0x4D);
const MINT: Color32 = Color32::from_rgb(0x3D, 0xDC, 0x97);
const SKY: Color32 = Color32::from_rgb(0x4D, 0xA8, 0xFF);
const PINK: Color32 = Color32::from_rgb(0xFF, 0x6F, 0xB5);

// ---------- easing ----------

fn clamp01(x: f32) -> f32 {
    x.clamp(0.0, 1.0)
}
/// Progress of `t` through [a, b].
fn seg(t: f32, a: f32, b: f32) -> f32 {
    clamp01((t - a) / (b - a))
}
fn out3(x: f32) -> f32 {
    1.0 - (1.0 - clamp01(x)).powi(3)
}
fn inout(x: f32) -> f32 {
    let x = clamp01(x);
    if x < 0.5 { 4.0 * x * x * x } else { 1.0 - (-2.0 * x + 2.0).powi(3) / 2.0 }
}
/// A springy settle with a little overshoot.
fn spring(x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    1.0 - (-6.5 * x).exp() * (11.0 * x).cos()
}
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
fn alpha(c: Color32, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (clamp01(a) * c.a() as f32) as u8)
}

// ---------- the composited scene ----------

#[derive(Default, Clone, Copy, PartialEq)]
enum Scene {
    #[default]
    Intro,
    App,
    Outro,
}

/// A cursor drawn over the film, in app points (mapped through the window).
#[derive(Clone)]
struct Cursor {
    at: Pos2,
    color: Color32,
    name: String,
}

/// A full-screen title card: word, field color, word color, progress 0..1.
#[derive(Clone)]
struct Card {
    word: String,
    field: Color32,
    lead: Color32,
    ink: Color32,
    k: f32,
}

#[derive(Default)]
struct Comp {
    styled: bool,
    scene: Scene,
    /// Seconds since the scene started, and since the film started.
    t: f32,
    film_t: f32,
    app: Option<ColorImage>,
    app_tex: Option<TextureHandle>,
    logo: Option<TextureHandle>,
    /// Where the app window sits: center, scale (1 = 1440 film pixels wide), rotation.
    win_center: Pos2,
    win_scale: f32,
    win_rot: f32,
    /// Circle reveal from the previous look (dark-mode wipe): previous image, center, radius.
    wipe: Option<(ColorImage, Pos2, f32)>,
    wipe_tex: Option<TextureHandle>,
    /// The flat field behind the window, and how much of the soft wallpaper shows instead.
    bg: Color32,
    wallpaper: f32,
    cursors: Vec<Cursor>,
    /// The prompt bubble: text, typed fraction, appear progress.
    prompt: Option<(String, f32, f32)>,
    chips: Vec<(String, Color32, f32)>,
    card: Option<Card>,
}

fn font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("Inter Bold".into()))
}

/// Display type: bold and tightly tracked.
fn display(p: &egui::Painter, s: &str, size: f32, color: Color32) -> Arc<Galley> {
    let mut job = LayoutJob::default();
    job.append(s, 0.0, TextFormat { font_id: font(size), color, extra_letter_spacing: -size * 0.04, ..Default::default() });
    p.layout_job(job)
}

/// A soft radial blob (triangle fan fading to transparent).
fn blob(mesh: &mut Mesh, c: Pos2, r: f32, color: Color32) {
    let base = mesh.vertices.len() as u32;
    mesh.vertices.push(Vertex { pos: c, uv: WHITE_UV, color });
    let n = 64;
    for i in 0..=n {
        let a = i as f32 / n as f32 * std::f32::consts::TAU;
        mesh.vertices.push(Vertex { pos: c + vec2(a.cos(), a.sin()) * r, uv: WHITE_UV, color: Color32::TRANSPARENT });
    }
    for i in 0..n {
        mesh.indices.extend([base, base + 1 + i, base + 2 + i]);
    }
}

fn screen() -> Rect {
    Rect::from_min_size(Pos2::ZERO, vec2(W, H))
}

/// The glass wallpaper: a dusk field with drifting color.
fn wallpaper(p: &egui::Painter, t: f32, a: f32) {
    p.rect_filled(screen(), 0.0, alpha(Color32::from_rgb(0x14, 0x11, 0x22), a));
    let mut m = Mesh::default();
    let wob = |k: f32| (t * 0.4 + k).sin();
    blob(&mut m, pos2(300.0 + 80.0 * wob(0.0), 260.0 + 50.0 * wob(1.3)), 760.0, alpha(VIOLET, 0.9 * a));
    blob(&mut m, pos2(1650.0 + 60.0 * wob(2.1), 240.0 + 70.0 * wob(0.7)), 700.0, alpha(CORAL, 0.85 * a));
    blob(&mut m, pos2(1450.0 + 90.0 * wob(3.4), 960.0 + 40.0 * wob(2.2)), 720.0, alpha(SUN, 0.7 * a));
    blob(&mut m, pos2(420.0 + 60.0 * wob(4.0), 980.0 + 50.0 * wob(0.2)), 640.0, alpha(SKY, 0.8 * a));
    p.add(Shape::mesh(m));
}

/// A textured quad, rotated about its center.
fn quad(p: &egui::Painter, tex: egui::TextureId, center: Pos2, size: Vec2, rot: f32, tint: Color32) {
    let (s, c) = rot.sin_cos();
    let corner = |dx: f32, dy: f32| center + vec2(dx * c - dy * s, dx * s + dy * c);
    let (hw, hh) = (size.x / 2.0, size.y / 2.0);
    let mut m = Mesh::with_texture(tex);
    for (pt, uv) in [(corner(-hw, -hh), pos2(0.0, 0.0)), (corner(hw, -hh), pos2(1.0, 0.0)), (corner(hw, hh), pos2(1.0, 1.0)), (corner(-hw, hh), pos2(0.0, 1.0))] {
        m.vertices.push(Vertex { pos: pt, uv, color: tint });
    }
    m.indices.extend([0, 1, 2, 0, 2, 3]);
    p.add(Shape::mesh(m));
}

/// A textured disc clipped to the window (for the circle reveal).
fn disc(p: &egui::Painter, tex: egui::TextureId, frame: Rect, center: Pos2, r: f32) {
    let mut m = Mesh::with_texture(tex);
    let uv = |q: Pos2| pos2((q.x - frame.min.x) / frame.width(), (q.y - frame.min.y) / frame.height());
    let clamp = |q: Pos2| pos2(q.x.clamp(frame.min.x, frame.max.x), q.y.clamp(frame.min.y, frame.max.y));
    let c = clamp(center);
    m.vertices.push(Vertex { pos: c, uv: uv(c), color: Color32::WHITE });
    let n = 128;
    for i in 0..=n {
        let a = i as f32 / n as f32 * std::f32::consts::TAU;
        let q = clamp(center + vec2(a.cos(), a.sin()) * r);
        m.vertices.push(Vertex { pos: q, uv: uv(q), color: Color32::WHITE });
    }
    for i in 0..n as u32 {
        m.indices.extend([0, 1 + i, 2 + i]);
    }
    p.add(Shape::mesh(m));
}

fn cursor(p: &egui::Painter, at: Pos2, color: Color32, name: &str, s: f32) {
    let pts = [vec2(0.0, 0.0), vec2(0.0, 26.0), vec2(7.0, 20.0), vec2(12.0, 31.0), vec2(16.5, 29.0), vec2(11.5, 18.5), vec2(20.0, 18.5)].map(|v| at + v * s);
    let edge = Stroke::new(2.0 * s, Color32::WHITE);
    p.add(Shape::convex_polygon(vec![pts[0], pts[1], pts[2], pts[6]], color, edge));
    p.add(Shape::convex_polygon(vec![pts[2], pts[3], pts[4], pts[5]], color, edge));
    let g = p.layout_no_wrap(name.to_string(), FontId::new(19.0 * s, FontFamily::Name("Inter SemiBold".into())), Color32::WHITE);
    let r = Rect::from_min_size(at + vec2(18.0, 30.0) * s, g.size() + vec2(22.0, 12.0) * s);
    p.rect_filled(r, 10.0 * s, color);
    p.galley(r.min + vec2(11.0, 6.0) * s, g, Color32::WHITE);
}

/// Words that spring up one after another and leave together.
fn kinetic(p: &egui::Painter, words: &[(&str, Color32)], center: Pos2, size: f32, t: f32, stagger: f32, out: f32) {
    let galleys: Vec<_> = words.iter().map(|(w, c)| display(p, w, size, *c)).collect();
    let space = size * 0.24;
    let total: f32 = galleys.iter().map(|g| g.size().x).sum::<f32>() + space * (galleys.len() as f32 - 1.0);
    let mut x = center.x - total / 2.0;
    for (i, g) in galleys.into_iter().enumerate() {
        let start = i as f32 * stagger;
        let k = spring(seg(t, start, start + 0.9));
        let fade = seg(t, start, start + 0.2) * (1.0 - out);
        let y = center.y - g.size().y / 2.0 + (1.0 - k) * size * 0.6 - out3(out) * size * 0.4;
        let w = g.size().x;
        p.galley_with_override_text_color(pos2(x, y), g, alpha(words[i].1, fade));
        x += w + space;
    }
}

/// A two-layer color wipe up the screen with one giant word, then out the top.
fn title_card(p: &egui::Painter, c: &Card) {
    let lead_in = inout(seg(c.k, 0.0, 0.2));
    let field_in = inout(seg(c.k, 0.05, 0.27));
    let lead_out = inout(seg(c.k, 0.72, 0.93));
    let field_out = inout(seg(c.k, 0.78, 1.0));
    let band = |cover: f32, gone: f32| Rect::from_min_max(pos2(0.0, H * (1.0 - cover) - H * gone), pos2(W, H - H * gone));
    p.rect_filled(band(lead_in, field_out), 0.0, c.lead);
    p.rect_filled(band(field_in, lead_out), 0.0, c.field);
    let clip = band(field_in, lead_out);
    if clip.height() > 1.0 {
        let p = p.with_clip_rect(clip);
        kinetic(&p, &[(&c.word, c.ink)], pos2(W / 2.0, H / 2.0 - H * lead_out), 230.0, (c.k - 0.12) * 1.2, 0.0, 0.0);
    }
}

fn prompt_bubble(p: &egui::Painter, text: &str, typed: f32, appear: f32) {
    let a = out3(appear);
    let s = spring(appear);
    let n = (text.chars().count() as f32 * clamp01(typed)).round() as usize;
    let shown: String = text.chars().take(n).collect();
    let g = p.layout(shown, font(44.0), Color32::WHITE, 760.0);
    let size = vec2(820.0, g.size().y.max(52.0) + 64.0);
    let r = Rect::from_min_size(pos2(W - 120.0 - size.x, H - 140.0 - size.y + (1.0 - s) * 80.0), size);
    p.add(Shape::Rect(egui::epaint::RectShape::filled(r.translate(vec2(0.0, 24.0)), 40.0, alpha(Color32::BLACK, 0.3 * a)).with_blur_width(60.0)));
    p.rect_filled(r, 40.0, alpha(VIOLET, a));
    p.galley_with_override_text_color(r.min + vec2(32.0, 32.0), g.clone(), alpha(Color32::WHITE, a));
    // A blinking caret while typing.
    if typed < 1.0 || (p.ctx().input(|i| i.time) * 2.0).fract() < 0.5 {
        let end = g.rows.last().map(|row| pos2(row.rect().max.x, row.rect().min.y)).unwrap_or_default();
        p.rect_filled(Rect::from_min_size(r.min + vec2(36.0, 34.0) + end.to_vec2(), vec2(4.0, 48.0)), 2.0, alpha(Color32::WHITE, a));
    }
    cursor(p, r.right_bottom() + vec2(-70.0, -26.0), SKY, "Claude", 1.1 * a.max(0.01));
}

fn compose(ui: &mut egui::Ui, c: &mut Comp) {
    let ctx = ui.ctx().clone();
    // Fonts load on the frame after they're set.
    if !c.styled {
        crate::setup_style(&ctx);
        c.styled = true;
        return;
    }
    let p = ui.painter().clone();
    let t = c.t;
    match c.scene {
        Scene::Intro => {
            p.rect_filled(screen(), 0.0, INK);
            let out = out3(seg(t, 3.5, 4.1));
            kinetic(&p, &[("Figma.", Color32::WHITE)], pos2(W / 2.0, H / 2.0 - 110.0), 200.0, t - 0.3, 0.0, out);
            kinetic(&p, &[("Rebuilt", Color32::WHITE), ("in", Color32::WHITE), ("Rust.", RUST)], pos2(W / 2.0, H / 2.0 + 110.0), 200.0, t - 1.2, 0.18, out);
        }
        Scene::App => {
            p.rect_filled(screen(), 0.0, c.bg);
            if c.wallpaper > 0.0 {
                wallpaper(&p, c.film_t, c.wallpaper);
            }
            if let Some(img) = c.app.take() {
                match &mut c.app_tex {
                    Some(tex) => tex.set(img, TextureOptions::LINEAR),
                    None => c.app_tex = Some(ctx.load_texture("app", img, TextureOptions::LINEAR)),
                }
            }
            if let Some(tex) = &c.app_tex {
                let size = vec2(AW, AH) * c.win_scale;
                let r = Rect::from_center_size(c.win_center + vec2(0.0, 30.0 * c.win_scale), size);
                p.add(Shape::Rect(egui::epaint::RectShape::filled(r, 24.0 * c.win_scale, alpha(Color32::from_rgb(0x10, 0x0A, 0x20), 0.35)).with_blur_width(110.0 * c.win_scale)));
                quad(&p, tex.id(), c.win_center, size, c.win_rot, Color32::WHITE);
                if let Some((img, center, radius)) = c.wipe.take() {
                    match &mut c.wipe_tex {
                        Some(tx) => tx.set(img, TextureOptions::LINEAR),
                        None => c.wipe_tex = Some(ctx.load_texture("wipe", img, TextureOptions::LINEAR)),
                    }
                    // The old look stays outside the growing circle.
                    let frame = Rect::from_center_size(c.win_center, size);
                    quad(&p, c.wipe_tex.as_ref().unwrap().id(), c.win_center, size, 0.0, Color32::WHITE);
                    disc(&p, tex.id(), frame, center, radius);
                }
            }
            for cu in &c.cursors {
                let at = c.win_center + (cu.at.to_vec2() - vec2(AW, AH) / 2.0) * c.win_scale;
                cursor(&p, at, cu.color, &cu.name, 1.15);
            }
            if let Some((text, typed, appear)) = &c.prompt {
                prompt_bubble(&p, text, *typed, *appear);
            }
            for (i, (label, col, k)) in c.chips.iter().enumerate() {
                let s = spring(*k);
                if *k <= 0.0 {
                    continue;
                }
                let g = display(&p, label, 56.0, Color32::WHITE);
                let pos = pos2(1270.0 + (i % 2) as f32 * 60.0, 150.0 + i as f32 * 128.0);
                let r = Rect::from_min_size(pos, g.size() + vec2(80.0, 40.0));
                let r = Rect::from_center_size(r.center(), r.size() * s.max(0.0));
                p.rect_filled(r, r.height() / 2.0, *col);
                if s > 0.6 {
                    p.galley(r.center() - g.size() / 2.0, g, Color32::WHITE);
                }
            }
        }
        Scene::Outro => {
            p.rect_filled(screen(), 0.0, INK);
            if let Some(logo) = &c.logo {
                let k = spring(seg(t, 0.1, 1.3));
                quad(&p, logo.id(), pos2(W / 2.0, H / 2.0 - 170.0), vec2(250.0, 250.0) * k, (1.0 - k) * -0.5, Color32::WHITE);
            }
            kinetic(&p, &[("Katachi", Color32::WHITE)], pos2(W / 2.0, H / 2.0 + 60.0), 150.0, t - 0.5, 0.0, 0.0);
            kinetic(&p, &[("Figma,", Color32::WHITE), ("rebuilt", Color32::WHITE), ("in", Color32::WHITE), ("Rust.", RUST)], pos2(W / 2.0, H / 2.0 + 200.0), 64.0, t - 1.0, 0.09, 0.0);
            let a = out3(seg(t, 1.9, 2.6));
            let g = display(&p, "github.com/dyascj/katachi", 40.0, alpha(Color32::from_gray(150), a));
            p.galley(pos2(W / 2.0 - g.size().x / 2.0, H / 2.0 + 300.0 + (1.0 - a) * 20.0), g, Color32::WHITE);
        }
    }
    if let Some(card) = &c.card {
        title_card(&p, card);
    }
}

// ---------- the film ----------

struct Video(std::process::Child);

impl Video {
    fn new(path: &std::path::Path) -> Self {
        let child = std::process::Command::new("ffmpeg")
            .args(["-loglevel", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgba", "-s", "1920x1080", "-r", "60", "-i", "-"])
            .args(["-c:v", "libx264", "-preset", "slow", "-crf", "15", "-pix_fmt", "yuv420p", "-movflags", "+faststart"])
            .arg(path)
            .stdin(std::process::Stdio::piped())
            .spawn()
            .expect("ffmpeg");
        Self(child)
    }
    fn push(&mut self, img: &image::RgbaImage) {
        self.0.stdin.as_mut().unwrap().write_all(img.as_raw()).unwrap();
    }
    fn finish(mut self) {
        drop(self.0.stdin.take());
        self.0.wait().unwrap();
    }
}

/// Round the window's corners and draw the traffic lights on its title strip. The render is
/// premultiplied, so corners fade all four channels.
fn window_look(img: &mut image::RgbaImage) {
    let (w, h) = img.dimensions();
    let r = (12.0 * APP_PPP) as i64;
    for y in 0..r as u32 {
        for x in 0..r as u32 {
            let d = (((r - x as i64 - 1).pow(2) + (r - y as i64 - 1).pow(2)) as f32).sqrt();
            let a = clamp01(r as f32 - d);
            for (px, py) in [(x, y), (w - 1 - x, y), (x, h - 1 - y), (w - 1 - x, h - 1 - y)] {
                let p = img.get_pixel_mut(px, py);
                for k in 0..4 {
                    p.0[k] = (p.0[k] as f32 * a) as u8;
                }
            }
        }
    }
    for (i, c) in [[0xFF, 0x5F, 0x57], [0xFE, 0xBC, 0x2E], [0x28, 0xC8, 0x40]].iter().enumerate() {
        let (cx, cy, rad) = ((20.0 + i as f32 * 20.0) * APP_PPP, 14.0 * APP_PPP, 6.0 * APP_PPP);
        for y in (cy - rad) as u32..=(cy + rad) as u32 {
            for x in (cx - rad) as u32..=(cx + rad) as u32 {
                let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
                let a = clamp01(rad - d + 0.5);
                if a > 0.0 {
                    let p = img.get_pixel_mut(x, y);
                    for k in 0..3 {
                        p.0[k] = (p.0[k] as f32 * (1.0 - a) + c[k] as f32 * a) as u8;
                    }
                    p.0[3] = (p.0[3] as f32 * (1.0 - a) + 255.0 * a) as u8;
                }
            }
        }
    }
}

fn premul(img: &image::RgbaImage) -> ColorImage {
    ColorImage::from_rgba_premultiplied([img.width() as usize, img.height() as usize], img.as_raw())
}

struct Film {
    app: Harness<'static, App>,
    comp: Harness<'static, Comp>,
    video: Video,
    frames: u64,
    /// The pointer inside the app (points), drawn as "CJ"'s cursor.
    pointer: Option<Pos2>,
    last_app: Option<image::RgbaImage>,
    /// Save every Nth frame as a PNG (PROMO_STILLS=N) for review.
    stills: Option<u64>,
    /// A title card in flight and the frame it started on.
    card: Option<(Card, u64)>,
}

const CARD_SECS: f32 = 1.5;
/// Cards start this long before the section they introduce, so they cover the cut.
const CARD_LEAD: f32 = 0.45;

impl Film {
    /// One frame: advance the app (if shown), composite, encode.
    fn shot(&mut self, scene: Scene, t: f32, setup: impl FnOnce(&mut Comp)) {
        if scene == Scene::App {
            self.app.step();
            let mut img = self.app.render().expect("render app");
            window_look(&mut img);
            self.comp.state_mut().app = Some(premul(&img));
            self.last_app = Some(img);
        }
        let film_t = self.frames as f32 / FPS;
        let pointer = self.pointer;
        let card = self.card.as_ref().map(|(c, start)| Card { k: (self.frames - start) as f32 / (CARD_SECS * FPS), ..c.clone() }).filter(|c| c.k < 1.0);
        if card.is_none() {
            self.card = None;
        }
        {
            let c = self.comp.state_mut();
            c.scene = scene;
            c.t = t;
            c.film_t = film_t;
            c.cursors.clear();
            if let Some(at) = pointer {
                c.cursors.push(Cursor { at, color: VIOLET, name: "CJ".into() });
            }
            c.card = card;
            setup(c);
        }
        self.comp.step();
        let out = self.comp.render().expect("render film");
        self.video.push(&out);
        if let Some(every) = self.stills
            && self.frames % every == 0
        {
            out.save(format!("target/promo/stills/{:05}.png", self.frames)).unwrap();
        }
        self.frames += 1;
    }

    /// Run `secs` of film, calling `f(film, t, frame)` before each app frame.
    fn section(&mut self, secs: f32, mut f: impl FnMut(&mut Film, f32, usize)) {
        for i in 0..(secs * FPS) as usize {
            f(self, i as f32 / FPS, i);
        }
    }
    /// The same, with a title card for the next section starting near the end.
    fn section_to(&mut self, secs: f32, word: &str, field: Color32, lead: Color32, ink: Color32, mut f: impl FnMut(&mut Film, f32, usize)) {
        let cue = ((secs - CARD_LEAD) * FPS) as usize;
        self.section(secs, |film, t, i| {
            if i == cue {
                film.card = Some((Card { word: word.into(), field, lead, ink, k: 0.0 }, film.frames));
            }
            f(film, t, i)
        });
    }

    fn view(&mut self, center: Pos2, zoom: f32) {
        let v = &mut self.app.state_mut().view;
        v.pan = view_for(center, zoom);
        v.zoom = zoom;
    }
    /// Screen point (app points) of a world point.
    fn on_screen(&self, world: Pos2) -> Pos2 {
        self.app.state().view.to_screen(world)
    }
    fn move_pointer(&mut self, at: Pos2) {
        self.pointer = Some(at);
        self.app.event(egui::Event::PointerMoved(at));
    }
    fn button(&mut self, at: Pos2, down: bool) {
        self.app.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: down, modifiers: Modifiers::NONE });
    }
    fn key(&mut self, k: Key) {
        for pressed in [true, false] {
            self.app.event(egui::Event::Key { key: k, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
        }
    }
    /// Run code through the MCP server's execute_code tool, exactly as an agent would.
    fn mcp(&mut self, code: &str) {
        let ctx = self.app.ctx.clone();
        let out = self.app.state_mut().mcp_tool(&ctx, "execute_code", &serde_json::json!({ "code": code }));
        assert!(out["isError"] != true, "{out}");
    }
    fn set_theme(&mut self, theme: crate::appearance::ThemePref, glass: bool) {
        let ctx = self.app.ctx.clone();
        let a = &mut self.app.state_mut().appearance;
        a.theme = theme;
        a.glass = glass;
        let a = a.clone();
        crate::appearance::apply(&ctx, &a);
    }
}

/// Pan that puts world point `center` in the middle of the canvas (between the panels) at `zoom`.
fn view_for(center: Pos2, zoom: f32) -> Vec2 {
    // The canvas runs from x 240 to 1192 and y 28 to 900 in the 1440x900 window.
    let canvas = pos2((240.0 + 1192.0) / 2.0, (28.0 + 900.0) / 2.0);
    canvas.to_vec2() - center.to_vec2() * zoom
}

/// Place the window so app point `focus` lands on film point `at`, at `scale`.
fn camera(c: &mut Comp, focus: Pos2, at: Pos2, scale: f32) {
    c.win_scale = scale;
    c.win_center = at - (focus.to_vec2() - vec2(AW, AH) / 2.0) * scale;
    c.win_rot = 0.0;
}

/// A smooth wandering path for a collaborator's cursor.
fn wander(t: f32, base: Pos2, k: f32) -> Pos2 {
    base + vec2((t * 0.9 + k).sin() * 150.0 + (t * 1.7 + k * 2.0).sin() * 40.0, (t * 0.7 + k * 1.3).cos() * 90.0 + (t * 2.1 + k).sin() * 25.0)
}

const CANVAS_MID: Pos2 = pos2(716.0, 464.0);
const STAGE: Pos2 = pos2(W / 2.0, H / 2.0 + 30.0);

#[test]
#[ignore]
fn promo_video() {
    use crate::appearance::ThemePref;
    let out_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/promo");
    std::fs::create_dir_all(&out_dir).unwrap();
    let stills = std::env::var("PROMO_STILLS").ok().and_then(|s| s.parse().ok());
    if stills.is_some() {
        let _ = std::fs::remove_dir_all(out_dir.join("stills"));
        std::fs::create_dir_all(out_dir.join("stills")).unwrap();
    }
    let mut app = Harness::builder()
        .with_size(vec2(AW, AH))
        .with_pixels_per_point(APP_PPP)
        .with_step_dt(1.0 / FPS)
        .with_theme(egui::Theme::Light)
        .wgpu()
        .build_eframe(|cc| {
            crate::setup_style(&cc.egui_ctx);
            let mut a = App::new(None);
            a.recoverable.clear();
            a.own_title_bar = true;
            a.appearance = crate::appearance::Appearance { theme: ThemePref::Light, ..Default::default() };
            crate::appearance::apply(&cc.egui_ctx, &a.appearance);
            a
        });
    {
        let a = app.state_mut();
        crate::plugins::run(&mut a.doc, 0, &[], include_str!("../assets/promo/showcase.js")).expect("showcase");
        a.doc.pages[0].name = "Lumen".into();
        a.path = Some("Lumen.katachi".into());
        a.sel.clear();
    }
    app.run_steps(4);
    app.state_mut().dirty = false;
    let comp = Harness::builder().with_size(vec2(W, H)).with_step_dt(1.0 / FPS).wgpu().build_ui_state(compose, Comp::default());
    let mut film = Film { app, comp, video: Video::new(&out_dir.join("katachi.mp4")), frames: 0, pointer: None, last_app: None, stills, card: None };
    film.comp.run_steps(2);
    let logo = image::load_from_memory(include_bytes!("../assets/icon/icon.png")).unwrap().to_rgba8();
    let logo = ColorImage::from_rgba_unmultiplied([logo.width() as usize, logo.height() as usize], logo.as_raw());
    film.comp.state_mut().logo = Some(film.comp.ctx.load_texture("logo", logo, TextureOptions::LINEAR));
    let white = Color32::WHITE;

    // 1. "Figma. Rebuilt in Rust."
    film.section(4.3, |f, t, _| f.shot(Scene::Intro, t, |_| {}));

    // 2. The window springs up and the camera glides from the whole board onto the landing page.
    let (wide, close) = ((pos2(1350.0, 760.0), 0.3), (pos2(600.0, 400.0), 0.9));
    film.section_to(5.0, "Draw.", SUN, CORAL, INK, |f, t, _| {
        let k = inout(seg(t, 1.0, 4.6));
        f.view(wide.0.lerp(close.0, k), lerp(wide.1, close.1, k));
        f.shot(Scene::App, t, |c| {
            c.bg = VIOLET;
            let r = spring(seg(t, 0.0, 1.5));
            camera(c, pos2(AW / 2.0, AH / 2.0), STAGE + vec2(0.0, (1.0 - r) * 900.0), lerp(0.8, 1.0, inout(seg(t, 0.3, 5.0))));
            c.win_rot = (1.0 - r) * 0.16;
        });
    });

    // 3. Draw: frame, rectangle, ellipse and text with real input, then styled by the agent API.
    let spot = pos2(3500.0, 1300.0);
    film.view(spot, 1.0);
    let draw_cam = |c: &mut Comp, t: f32| {
        c.bg = SUN;
        camera(c, CANVAS_MID + vec2(-40.0, 0.0), STAGE, lerp(1.2, 1.32, t / 8.0));
    };
    let base = film.on_screen(spot);
    film.pointer = Some(base + vec2(320.0, 300.0));
    let mut st = 0.0;
    film.section(0.5, |f, t, _| f.shot(Scene::App, t, |c| draw_cam(c, t)));
    st += 0.5;
    let draws = [(Key::F, vec2(-320.0, -250.0), vec2(320.0, 250.0)), (Key::R, vec2(-260.0, -190.0), vec2(260.0, 40.0)), (Key::O, vec2(140.0, 80.0), vec2(250.0, 190.0))];
    for (tool, a, b) in draws {
        film.key(tool);
        let (pa, pb) = (base + a, base + b);
        let from = film.pointer.unwrap();
        film.section(0.95, |f, t, i| {
            let at = if t < 0.3 { from.lerp(pa, inout(t / 0.3)) } else { pa.lerp(pb, inout(seg(t, 0.33, 0.85))) };
            f.move_pointer(at);
            if i == 19 {
                f.button(pa, true);
            }
            if i == 53 {
                f.button(pb, false);
            }
            f.shot(Scene::App, st + t, |c| draw_cam(c, st + t));
        });
        st += 0.95;
    }
    film.key(Key::T);
    let tp = base + vec2(-260.0, 90.0);
    let from = film.pointer.unwrap();
    let word: Vec<char> = "Hello, Rust".chars().collect();
    film.section(1.3, |f, t, i| {
        f.move_pointer(from.lerp(tp, inout(seg(t, 0.0, 0.28))));
        if i == 18 {
            f.button(tp, true);
        }
        if i == 20 {
            f.button(tp, false);
        }
        if i >= 24 && (i - 24) % 3 == 0 && (i - 24) / 3 < word.len() {
            f.app.event(egui::Event::Text(word[(i - 24) / 3].to_string()));
        }
        f.shot(Scene::App, st + t, |c| draw_cam(c, st + t));
    });
    st += 1.3;
    film.key(Key::Escape);
    film.key(Key::Escape);
    let looks = [
        r##"const f = figma.currentPage.findOne(n => n.name === "Frame 1"); f.cornerRadius = 36; f.fills = [figma.util.solidPaint("#16141F")]; f.effects = [{ type: "DROP_SHADOW", color: { r: 0.2, g: 0.1, b: 0.4, a: 0.3 }, offset: { x: 0, y: 24 }, radius: 48, spread: 0, visible: true }]; figma.currentPage.selection = [f];"##,
        r##"const r = figma.currentPage.findOne(n => n.name === "Rectangle 1"); r.cornerRadius = 24; r.fills = [{ type: "GRADIENT_LINEAR", gradientTransform: [[1, 0, 0], [0, 1, 0]], gradientStops: [{ position: 0, color: { r: 0.557, g: 0.447, b: 1, a: 1 } }, { position: 1, color: { r: 1, g: 0.353, b: 0.235, a: 1 } }] }]; figma.currentPage.selection = [r];"##,
        r##"const e = figma.currentPage.findOne(n => n.name === "Ellipse 1"); e.fills = [figma.util.solidPaint("#FFC94D")]; figma.currentPage.selection = [e];"##,
        r##"const t = figma.currentPage.findOne(n => n.type === "TEXT" && n.characters === "Hello, Rust"); t.fontName = { family: "Inter", style: "Bold" }; t.fontSize = 52; t.fills = [figma.util.solidPaint("#FFFFFF")]; figma.currentPage.selection = [t];"##,
    ];
    let away = base + vec2(420.0, 300.0);
    let from = film.pointer.unwrap();
    film.section_to(2.1, "Systems.", MINT, SKY, INK, |f, t, i| {
        f.move_pointer(from.lerp(away, inout(seg(t, 0.0, 0.4))));
        if i % 20 == 4 && i / 20 < looks.len() {
            f.mcp(looks[i / 20]);
        }
        f.shot(Scene::App, st + t, |c| draw_cam(c, st + t));
    });

    // 4. Systems: the Button set, and an instance switching variants.
    film.mcp(r##"const set = figma.currentPage.findOne(n => n.name === "Button"); const i = set.children[0].createInstance(); set.parent.appendChild(i); i.x = 1600; i.y = 330; i.name = "Button"; figma.currentPage.selection = [i];"##);
    film.pointer = None;
    let ds = pos2(1940.0, 1225.0);
    film.view(ds, 1.25);
    let sys_cam = |c: &mut Comp, t: f32| {
        c.bg = MINT;
        camera(c, CANVAS_MID + vec2(150.0, 0.0), STAGE, lerp(1.3, 1.4, t / 4.0));
    };
    film.section_to(3.6, "Together.", CORAL, PINK, white, |f, t, i| {
        for (n, style) in ["Dark", "Soft", "Primary"].iter().enumerate() {
            if i == ((1.0 + n as f32 * 0.8) * FPS) as usize {
                f.mcp(&format!(r##"figma.currentPage.selection[0].setProperties({{ Style: "{style}" }});"##));
            }
        }
        f.shot(Scene::App, t, |c| sys_cam(c, t));
    });

    // 5. Together: collaborators' cursors drift over the landing page.
    film.app.state_mut().sel.clear();
    film.view(close.0, close.1);
    let people = [("Ava", PINK, 0.0, pos2(560.0, 360.0)), ("Theo", SKY, 2.0, pos2(860.0, 600.0)), ("Mara", MINT, 4.0, pos2(1000.0, 300.0)), ("CJ", VIOLET, 1.0, pos2(420.0, 640.0))];
    film.section_to(3.6, "Make it yours.", INK, VIOLET, white, |f, t, _| {
        f.view(close.0 + vec2(t * 30.0, 0.0), close.1 + t * 0.02);
        f.shot(Scene::App, t, |c| {
            c.bg = CORAL;
            camera(c, CANVAS_MID, STAGE, 1.1);
            for (n, (name, color, k, at)) in people.iter().enumerate() {
                let a = spring(seg(t, 0.5 + n as f32 * 0.22, 1.2 + n as f32 * 0.22));
                if a > 0.0 {
                    c.cursors.push(Cursor { at: wander(t, *at, *k) + vec2(0.0, (1.0 - a) * 60.0), color: *color, name: name.to_string() });
                }
            }
        });
    });

    // 6. Make it yours: a circle wipe into dark mode, then glass over the wallpaper.
    film.section(0.1, |f, t, _| f.shot(Scene::App, t, |c| c.bg = INK));
    let light = film.last_app.clone().unwrap();
    film.set_theme(ThemePref::Dark, false);
    film.section(2.0, |f, t, _| {
        f.view(close.0 + vec2(108.0 + t * 30.0, 0.0), close.1 + 0.072);
        f.shot(Scene::App, t, |c| {
            c.bg = INK;
            camera(c, CANVAS_MID, STAGE, 0.86);
            let frame = Rect::from_center_size(c.win_center, vec2(AW, AH) * c.win_scale);
            let r = 2000.0 * inout(seg(t, 0.55, 1.6));
            c.wipe = (r < 1990.0).then(|| (premul(&light), frame.right_top() + vec2(-160.0, 60.0), r));
        });
    });
    film.set_theme(ThemePref::Dark, true);
    film.section_to(2.6, "Just ask.", SKY, VIOLET, white, |f, t, _| {
        f.view(close.0 + vec2(168.0 + t * 30.0, 0.0), close.1 + 0.072);
        f.shot(Scene::App, 2.0 + t, |c| {
            c.wallpaper = out3(seg(t, 0.0, 0.5));
            camera(c, CANVAS_MID, STAGE, lerp(0.86, 0.82, t / 2.6));
        });
    });

    // 7. Just ask: an agent builds a pricing page through the MCP server.
    film.comp.state_mut().wallpaper = 0.0;
    film.set_theme(ThemePref::Dark, false);
    let pricing = pos2(-1500.0, 0.0);
    film.mcp(&format!(
        r##"const p = figma.createFrame(); p.name = "Lumen · Pricing"; p.x = {}; p.y = {}; p.resize(1280, 800); p.fills = [figma.util.solidPaint("#16141F")]; p.cornerRadius = 0;
        p.layoutMode = "VERTICAL"; p.paddingTop = 90; p.paddingLeft = p.paddingRight = 80; p.itemSpacing = 56; p.counterAxisAlignItems = "CENTER"; p.primaryAxisSizingMode = "FIXED"; p.counterAxisSizingMode = "FIXED";
        const h = figma.createText(); h.characters = "Simple pricing"; h.fontSize = 64; h.fontName = {{ family: "Inter", style: "Bold" }}; h.fills = [figma.util.solidPaint("#FFFFFF")]; p.appendChild(h);
        const row = figma.createFrame(); row.name = "Plans"; row.fills = []; row.layoutMode = "HORIZONTAL"; row.itemSpacing = 32; row.primaryAxisSizingMode = "AUTO"; row.counterAxisSizingMode = "AUTO"; p.appendChild(row); figma.currentPage.selection = [];"##,
        pricing.x, pricing.y
    ));
    let plans = [("Free", "$0", "#2A2738"), ("Pro", "$12", "grad"), ("Studio", "$29", "#2A2738")];
    let plan_js = |(name, price, bg): (&str, &str, &str)| {
        let fill = if bg == "grad" {
            r##"{ type: "GRADIENT_LINEAR", gradientTransform: [[1, 0, 0], [0, 1, 0]], gradientStops: [{ position: 0, color: { r: 0.557, g: 0.447, b: 1, a: 1 } }, { position: 1, color: { r: 1, g: 0.353, b: 0.235, a: 1 } }] }"##.to_string()
        } else {
            format!("figma.util.solidPaint(\"{bg}\")")
        };
        format!(
            r##"const row = figma.currentPage.findOne(n => n.name === "Plans"); const c = figma.createFrame(); c.name = "{name}"; c.resize(330, 440); c.cornerRadius = 32; c.fills = [{fill}];
            c.layoutMode = "VERTICAL"; c.paddingTop = c.paddingBottom = 40; c.paddingLeft = c.paddingRight = 36; c.itemSpacing = 14; c.primaryAxisSizingMode = "FIXED"; c.counterAxisSizingMode = "FIXED"; row.appendChild(c);
            const add = (s, size, style, a) => {{ const t = figma.createText(); t.characters = s; t.fontSize = size; t.fontName = {{ family: "Inter", style }}; t.fills = [figma.util.solidPaint("#FFFFFF")]; t.opacity = a; c.appendChild(t); }};
            add("{name}", 24, "Semi Bold", 0.85); add("{price}", 80, "Bold", 1); add("per month", 18, "Regular", 0.6);
            add("Unlimited mixes", 20, "Medium", 0.9); add("Live sessions", 20, "Medium", 0.9); add("Share anywhere", 20, "Medium", 0.9);
            figma.currentPage.selection = [c];"##
        )
    };
    let prompt = "Add a pricing page for Lumen";
    film.view(pricing + vec2(640.0, 420.0), 0.72);
    film.section_to(5.4, "Ship it.", PINK, CORAL, white, |f, t, i| {
        f.view(pricing + vec2(640.0, 420.0), 0.72 + t * 0.01);
        for (n, plan) in plans.iter().enumerate() {
            if i == ((2.5 + n as f32 * 0.6) * FPS) as usize {
                f.mcp(&plan_js(*plan));
            }
        }
        f.shot(Scene::App, t, |c| {
            c.bg = SKY;
            camera(c, CANVAS_MID, STAGE + vec2(-120.0, -60.0), 0.98);
            c.prompt = (t < 4.9).then(|| (prompt.to_string(), seg(t, 0.8, 2.0), seg(t, 0.4, 1.3) * (1.0 - seg(t, 4.0, 4.9))));
        });
    });
    film.comp.state_mut().prompt = None;

    // 8. Ship it: Dev Mode on the Pro card.
    film.app.event(egui::Event::PointerGone);
    film.set_theme(ThemePref::Light, false);
    {
        let a = film.app.state_mut();
        a.sel = a.doc.nodes.iter().filter(|(_, n)| n.name == "Pro").map(|(id, _)| *id).collect();
        a.dev_mode = true;
    }
    film.view(pricing + vec2(640.0, 430.0), 0.6);
    film.section(3.2, |f, t, _| {
        f.shot(Scene::App, t, |c| {
            c.bg = PINK;
            let z = inout(seg(t, 0.5, 2.6));
            camera(c, pos2(AW / 2.0, AH / 2.0).lerp(pos2(1180.0, 360.0), z), STAGE.lerp(pos2(W / 2.0 + 220.0, H / 2.0), z), lerp(1.0, 1.6, z));
        });
    });

    // 9. The rest of the toolbox, popping in beside the window.
    film.app.state_mut().dev_mode = false;
    film.app.state_mut().sel.clear();
    film.view(wide.0, 0.34);
    let chips = [("Prototyping", VIOLET), ("Plugins", CORAL), ("Variables", SKY), (".fig import", PINK), ("Vector networks", MINT), ("Auto layout", SUN)];
    film.section(3.4, |f, t, _| {
        f.shot(Scene::App, t, |c| {
            c.bg = INK;
            let k = inout(seg(t, 0.0, 0.7));
            camera(c, pos2(AW / 2.0, AH / 2.0), pos2(lerp(W / 2.0, 640.0, k), H / 2.0), lerp(1.6, 0.82, k));
            c.chips = chips.iter().enumerate().map(|(n, (s, col))| (s.to_string(), *col, seg(t, 0.5 + n as f32 * 0.16, 1.5 + n as f32 * 0.16))).collect();
        });
    });
    film.comp.state_mut().chips.clear();

    // 10. Outro.
    film.section(4.6, |f, t, _| f.shot(Scene::Outro, t, |_| {}));
    film.video.finish();
    eprintln!("wrote {} frames to target/promo/katachi.mp4", film.frames);
}
