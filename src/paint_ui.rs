//! Fill, stroke, and effect editors for the design panel (Figma-style rows with popovers).

use crate::app::App;
use crate::canvas::num;
use crate::doc::{Paint, PaintKind, Rgba, Shadow};
use crate::panels::{Icon, icon_button};
use egui::{Color32, DragValue, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

/// A color picker asks for the eyedropper under this key; the pick comes back to it under
/// its own id `.with("eyedropped")`.
const EYEDROP_REQUEST: &str = "eyedrop-request";

/// Where an eyedropper pick goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EyeTarget {
    /// A color picker, by id.
    Picker(egui::Id),
    /// The selection's top fill (I on the canvas).
    Fill,
}

/// Figma's eyedropper: a loupe over a screenshot of the window, taken once when it starts.
pub struct Eyedrop {
    pub target: EyeTarget,
    pub shot: Option<std::sync::Arc<egui::ColorImage>>,
    /// The screenshot is requested on the first frame, before the loupe is drawn, so it isn't in it.
    asked: bool,
}

impl App {
    pub fn start_eyedrop(&mut self, target: EyeTarget) {
        self.eyedrop = Some(Eyedrop { target, shot: None, asked: false });
    }

    /// Runs before the panels each frame: starts, feeds, and draws the eyedropper.
    pub fn eyedropper(&mut self, ctx: &egui::Context) {
        if let Some(id) = ctx.data_mut(|d| d.remove_temp::<Option<egui::Id>>(egui::Id::new(EYEDROP_REQUEST)).flatten()) {
            self.start_eyedrop(EyeTarget::Picker(id));
        }
        let Some(e) = &mut self.eyedrop else { return };
        // Escape cancels, and is used up here so it doesn't also close the picker.
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.eyedrop = None;
            return;
        }
        if !e.asked {
            e.asked = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            ctx.request_repaint();
            return;
        }
        let shot = ctx.input(|i| {
            i.raw.events.iter().find_map(|ev| match ev {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if shot.is_some() {
            e.shot = shot;
        }
        // The screenshot is in physical pixels.
        let ppp = ctx.pixels_per_point();
        let sample = |p: Pos2| -> Option<Rgba> {
            let img = e.shot.as_ref()?;
            let (x, y) = ((p.x * ppp).floor(), (p.y * ppp).floor());
            let [w, h] = img.size;
            let inside = x >= 0.0 && y >= 0.0 && (x as usize) < w && (y as usize) < h;
            inside.then(|| img.pixels[y as usize * w + x as usize]).map(|c| Rgba::rgb(c.r(), c.g(), c.b()))
        };
        let screen = ctx.content_rect();
        let resp = egui::Area::new(egui::Id::new("eyedropper"))
            .order(egui::Order::Tooltip)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let (_, resp) = ui.allocate_exact_size(screen.size(), Sense::click());
                let Some(p) = resp.hover_pos() else { return resp };
                ui.ctx().set_cursor_icon(egui::CursorIcon::None);
                let painter = ui.painter();
                // A magnified ring of the pixels around the pointer, the center one framed.
                const N: i32 = 6;
                const CELL: f32 = 10.0;
                let radius = (N as f32 + 0.5) * CELL;
                for dy in -N..=N {
                    for dx in -N..=N {
                        let off = vec2(dx as f32, dy as f32);
                        if off.length() * CELL > radius - CELL * 0.4 {
                            continue;
                        }
                        if let Some(c) = sample(p + off / ppp) {
                            painter.rect_filled(Rect::from_center_size(p + off * CELL, vec2(CELL, CELL)), 0.0, c.to32());
                        }
                    }
                }
                painter.circle_stroke(p, radius, Stroke::new(4.0, Color32::WHITE));
                painter.circle_stroke(p, radius + 2.5, Stroke::new(1.0, Color32::from_black_alpha(90)));
                let mid = Rect::from_center_size(p, vec2(CELL, CELL));
                painter.rect_stroke(mid, 0.0, Stroke::new(1.0, Color32::WHITE), StrokeKind::Outside);
                painter.rect_stroke(mid.expand(1.0), 0.0, Stroke::new(1.0, Color32::BLACK), StrokeKind::Outside);
                if let Some(c) = sample(p) {
                    let g = painter.layout_no_wrap(format!("#{}", c.hex()), FontId::monospace(11.0), Color32::WHITE);
                    let pill = Rect::from_center_size(p + vec2(0.0, radius + 16.0), g.size() + vec2(12.0, 6.0));
                    painter.rect_filled(pill, 4.0, Color32::from_black_alpha(200));
                    painter.galley(pill.min + vec2(6.0, 3.0), g, Color32::WHITE);
                }
                resp
            })
            .inner;
        let pick = resp.clicked().then(|| resp.interact_pointer_pos()).flatten().and_then(sample);
        if let Some(c) = pick {
            let target = e.target;
            self.eyedrop = None;
            match target {
                EyeTarget::Picker(id) => ctx.data_mut(|d| {
                    d.insert_temp(id.with("eyedropped"), Some(c));
                }),
                EyeTarget::Fill => self.fill_selection(c),
            }
        }
    }

    /// Set the selection's top fill to `c`, keeping its opacity (a non-solid top fill is replaced).
    pub fn fill_selection(&mut self, c: Rgba) {
        self.checkpoint();
        for id in self.sel.clone() {
            let n = self.doc.node_mut(id);
            if n.kind == crate::doc::Kind::Group {
                continue;
            }
            match n.fills.last_mut() {
                Some(p) if p.kind == PaintKind::Solid => p.color = c.with_alpha(p.color.a()),
                _ => n.fills = vec![Paint::solid(c)],
            }
            // A recolored fill no longer follows its style or variable.
            n.fill_style = None;
            n.bindings.retain(|(b, _)| *b != crate::doc::Bindable::Fill);
        }
        self.doc.reflow();
    }
}

pub fn parse_hex(s: &str) -> Option<Rgba> {
    let s = s.trim().trim_start_matches('#');
    let s = match s.len() {
        3 => s.chars().flat_map(|c| [c, c]).collect(),
        6 => s.to_string(),
        _ => return None,
    };
    let v = u32::from_str_radix(&s, 16).ok()?;
    Some(Rgba::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

/// A text field showing `shown` that commits a parse of its text on Enter or focus loss.
fn commit_field<T>(ui: &mut Ui, id: egui::Id, shown: String, width: f32, parse: impl Fn(&str) -> Option<T>) -> Option<T> {
    let mut buf = ui.data_mut(|d| d.get_temp::<String>(id)).unwrap_or(shown);
    let resp = ui.add(egui::TextEdit::singleline(&mut buf).id(id).desired_width(width).font(FontId::monospace(11.0)));
    let mut out = None;
    if resp.lost_focus() {
        out = parse(&buf);
        ui.data_mut(|d| d.remove::<String>(id));
    } else if resp.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, buf));
    } else {
        ui.data_mut(|d| d.remove::<String>(id));
    }
    out
}

/// Hex text field that commits on Enter / focus loss, keeping alpha.
pub fn hex_field(ui: &mut Ui, id: egui::Id, color: &mut Rgba, width: f32) -> bool {
    commit_field(ui, id, color.hex(), width, parse_hex).map(|c| *color = c.with_alpha(color.a())).is_some()
}

/// Opacity percentage field for a color's alpha.
pub fn alpha_field(ui: &mut Ui, color: &mut Rgba) -> bool {
    let mut pct = (color.a() as f32 / 255.0 * 100.0).round();
    let changed = ui.add(DragValue::new(&mut pct).range(0.0..=100.0).suffix("%").speed(0.5)).changed();
    if changed {
        *color = color.with_alpha((pct / 100.0 * 255.0).round() as u8);
    }
    changed
}

/// sRGB to hue, saturation, and brightness, each 0..=1 (Figma's HSB).
pub fn to_hsb(c: Rgba) -> [f32; 3] {
    let [r, g, b] = [c.0[0], c.0[1], c.0[2]].map(|v| v as f32 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    [h / 6.0, if max == 0.0 { 0.0 } else { d / max }, max]
}

pub fn from_hsb([h, s, v]: [f32; 3], a: u8) -> Rgba {
    let f = |n: f32| {
        let k = (n + h.rem_euclid(1.0) * 6.0) % 6.0;
        ((v - v * s * k.min(4.0 - k).clamp(0.0, 1.0)) * 255.0).round().clamp(0.0, 255.0) as u8
    };
    Rgba([f(5.0), f(3.0), f(1.0), a])
}

/// HSB to HSL (hue kept), and back.
pub fn hsb_to_hsl([h, s, v]: [f32; 3]) -> [f32; 3] {
    let l = v * (1.0 - s / 2.0);
    let m = l.min(1.0 - l);
    [h, if m <= 0.0 { 0.0 } else { (v - l) / m }, l]
}

pub fn hsl_to_hsb([h, s, l]: [f32; 3]) -> [f32; 3] {
    let v = l + s * l.min(1.0 - l);
    [h, if v <= 0.0 { 0.0 } else { 2.0 * (1.0 - l / v) }, v]
}

/// `rgb(...)`, or `rgba(...)` when translucent.
pub fn css_rgba(c: Rgba) -> String {
    let [r, g, b, a] = c.0;
    if a == 255 { format!("rgb({r}, {g}, {b})") } else { format!("rgba({r}, {g}, {b}, {})", num(a as f32 / 255.0)) }
}

/// A CSS color: hex, `rgb()`, or `rgba()`.
pub fn parse_css(s: &str) -> Option<Rgba> {
    let s = s.trim();
    let Some(body) = s.strip_prefix("rgba(").or_else(|| s.strip_prefix("rgb(")).and_then(|b| b.strip_suffix(')')) else {
        return parse_hex(s);
    };
    let v: Vec<f32> = body.split([',', ' ', '/']).filter(|t| !t.is_empty()).map(|t| t.parse().ok()).collect::<Option<_>>()?;
    let ch = |x: f32| x.round().clamp(0.0, 255.0) as u8;
    match v[..] {
        [r, g, b] => Some(Rgba::rgb(ch(r), ch(g), ch(b))),
        [r, g, b, a] => Some(Rgba([ch(r), ch(g), ch(b), ch(a * 255.0)])),
        _ => None,
    }
}

/// The picker's number formats, as in Figma's dropdown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ColorFormat {
    #[default]
    Hex,
    Rgb,
    Css,
    Hsl,
    Hsb,
}

impl ColorFormat {
    const ALL: [ColorFormat; 5] = [ColorFormat::Hex, ColorFormat::Rgb, ColorFormat::Css, ColorFormat::Hsl, ColorFormat::Hsb];
    fn label(self) -> &'static str {
        match self {
            ColorFormat::Hex => "Hex",
            ColorFormat::Rgb => "RGB",
            ColorFormat::Css => "CSS",
            ColorFormat::Hsl => "HSL",
            ColorFormat::Hsb => "HSB",
        }
    }
}

/// Colors the picker offers below its fields: the current page's, then color styles.
#[derive(Clone, Debug, Default)]
pub struct Swatches {
    pub page: Vec<Rgba>,
    pub styles: Vec<(String, Rgba)>,
}

const SWATCHES: &str = "picker-swatches";

impl App {
    /// Every solid color on the current page, and the solid color styles.
    pub fn swatches(&self) -> Swatches {
        let mut page = vec![];
        for &r in &self.doc.pages[self.page].children {
            for id in std::iter::once(r).chain(self.doc.descendants(r)) {
                let n = self.doc.node(id);
                for p in n.fills.iter().chain(&n.strokes).filter(|p| p.visible && p.kind == PaintKind::Solid) {
                    if !page.contains(&p.color) {
                        page.push(p.color);
                    }
                }
            }
        }
        let styles = self
            .doc
            .styles
            .iter()
            .filter_map(|s| match &s.value {
                crate::doc::StyleValue::Paint(ps) => ps.last().filter(|p| p.kind == PaintKind::Solid).map(|p| (s.name.clone(), p.color)),
                _ => None,
            })
            .collect();
        Swatches { page, styles }
    }

    /// Hand the swatches to any color picker drawn this frame (only while a popover is open).
    pub fn offer_swatches(&self, ctx: &egui::Context) {
        if egui::Popup::is_any_open(ctx) {
            ctx.data_mut(|d| d.insert_temp(egui::Id::new(SWATCHES), std::sync::Arc::new(self.swatches())));
        }
    }
}

/// The dotted handle that drags a paint row to reorder it.
pub fn grip(ui: &mut Ui) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(8.0, 20.0), Sense::drag());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Drag to reorder"));
    let c = if resp.hovered() || resp.dragged() { ui.visuals().text_color() } else { crate::appearance::faint(ui.ctx()) };
    for dx in [-1.5, 1.5] {
        for dy in [-4.0, 0.0, 4.0] {
            ui.painter().circle_filled(r.center() + vec2(dx, dy), 1.0, c);
        }
    }
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    resp
}

/// A small clickable color chip.
fn chip(ui: &mut Ui, c: Rgba, tip: &str) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tip));
    if c.a() < 255 {
        ui.painter().rect_filled(r, 3.0, Color32::WHITE);
        ui.painter().rect_filled(Rect::from_min_size(r.min, r.size() / 2.0), 0.0, Color32::from_gray(0xD8));
        ui.painter().rect_filled(Rect::from_min_size(r.center(), r.size() / 2.0), 0.0, Color32::from_gray(0xD8));
    }
    ui.painter().rect_filled(r, 3.0, c.to32());
    ui.painter().rect_stroke(r, 3.0, Stroke::new(1.0, Color32::from_black_alpha(30)), StrokeKind::Inside);
    resp.on_hover_text(tip)
}

/// Figma's color picker: a saturation and brightness square, the eyedropper beside thin hue and
/// opacity sliders, the color in a chosen format, then swatches from the page and color styles.
fn rgb_picker(ui: &mut Ui, color: &mut Rgba) -> bool {
    use egui::epaint::{Mesh, Vertex, WHITE_UV};
    let id = ui.id().with("figma-picker");
    let mut changed = false;
    if let Some(c) = ui.data_mut(|d| d.remove_temp::<Option<Rgba>>(id.with("eyedropped")).flatten()) {
        *color = c.with_alpha(color.a());
        changed = true;
    }
    // Grays have no hue and black no saturation: keep the last ones so the knobs don't jump.
    let mut hsb = match ui.data(|d| d.get_temp::<(Rgba, [f32; 3])>(id)) {
        Some((c, k)) if c == color.with_alpha(255) => k,
        _ => to_hsb(*color),
    };
    let solid = |k: [f32; 3]| from_hsb(k, 255).to32();
    let w = ui.available_width();
    let quad = |m: &mut Mesh, r: Rect, [tl, tr, br, bl]: [Color32; 4]| {
        let base = m.vertices.len() as u32;
        for (p, c) in [(r.left_top(), tl), (r.right_top(), tr), (r.right_bottom(), br), (r.left_bottom(), bl)] {
            m.vertices.push(Vertex { pos: p, uv: WHITE_UV, color: c });
        }
        m.indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    };
    let knob = |ui: &Ui, c: egui::Pos2, fill: Color32| {
        ui.painter().circle(c, 6.0, fill, Stroke::new(2.0, Color32::WHITE));
        ui.painter().circle_stroke(c, 7.0, Stroke::new(1.0, Color32::from_black_alpha(60)));
    };
    // Saturation (x) and brightness (y).
    let (sq, resp) = ui.allocate_exact_size(vec2(w, w), Sense::click_and_drag());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, "Saturation and brightness"));
    let pure = solid([hsb[0], 1.0, 1.0]);
    let mut m = Mesh::default();
    quad(&mut m, sq, [Color32::WHITE, pure, pure, Color32::WHITE]);
    quad(&mut m, sq, [Color32::TRANSPARENT, Color32::TRANSPARENT, Color32::BLACK, Color32::BLACK]);
    ui.painter().add(m);
    ui.painter().rect_stroke(sq, 4.0, Stroke::new(1.0, Color32::from_black_alpha(25)), StrokeKind::Inside);
    if let Some(p) = resp.interact_pointer_pos() {
        hsb[1] = ((p.x - sq.min.x) / sq.width()).clamp(0.0, 1.0);
        hsb[2] = 1.0 - ((p.y - sq.min.y) / sq.height()).clamp(0.0, 1.0);
        changed = true;
    }
    knob(ui, egui::pos2(sq.min.x + hsb[1] * sq.width(), sq.min.y + (1.0 - hsb[2]) * sq.height()), solid(hsb));
    ui.add_space(8.0);
    let mut a = color.a() as f32 / 255.0;
    ui.horizontal(|ui| {
        if icon_button(ui, Icon::Eyedropper, false, 24.0, "Pick color  I").clicked() {
            ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new(EYEDROP_REQUEST), Some(id)));
        }
        ui.add_space(4.0);
        ui.vertical(|ui| {
            let w = ui.available_width();
            // Hue.
            let (bar, resp) = ui.allocate_exact_size(vec2(w, 12.0), Sense::click_and_drag());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, "Hue"));
            let mut m = Mesh::default();
            for i in 0..6 {
                let (x0, x1) = (i as f32 / 6.0, (i + 1) as f32 / 6.0);
                let r = Rect::from_min_max(egui::pos2(bar.min.x + x0 * bar.width(), bar.min.y), egui::pos2(bar.min.x + x1 * bar.width(), bar.max.y));
                let (ca, cb) = (solid([x0, 1.0, 1.0]), solid([x1 % 1.0, 1.0, 1.0]));
                quad(&mut m, r, [ca, cb, cb, ca]);
            }
            ui.painter().add(m);
            if let Some(p) = resp.interact_pointer_pos() {
                hsb[0] = ((p.x - bar.min.x) / bar.width()).clamp(0.0, 0.9999);
                changed = true;
            }
            knob(ui, egui::pos2(bar.min.x + hsb[0] * bar.width(), bar.center().y), solid([hsb[0], 1.0, 1.0]));
            ui.add_space(4.0);
            // Opacity.
            let (bar, resp) = ui.allocate_exact_size(vec2(w, 12.0), Sense::click_and_drag());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, "Opacity"));
            for i in 0..(bar.width() / 6.0).ceil() as usize {
                for j in 0..2 {
                    if (i + j) % 2 == 0 {
                        let r = Rect::from_min_size(egui::pos2(bar.min.x + i as f32 * 6.0, bar.min.y + j as f32 * 6.0), vec2(6.0, 6.0)).intersect(bar);
                        ui.painter().rect_filled(r, 0.0, Color32::from_gray(0xD8));
                    }
                }
            }
            let mut m = Mesh::default();
            quad(&mut m, bar, [Color32::TRANSPARENT, solid(hsb), solid(hsb), Color32::TRANSPARENT]);
            ui.painter().add(m);
            if let Some(p) = resp.interact_pointer_pos() {
                a = ((p.x - bar.min.x) / bar.width()).clamp(0.0, 1.0);
                changed = true;
            }
            knob(ui, egui::pos2(bar.min.x + a * bar.width(), bar.center().y), solid(hsb));
        });
    });
    if changed {
        *color = from_hsb(hsb, (a * 255.0).round() as u8);
    }
    ui.add_space(8.0);
    // The color in the chosen format (remembered across pickers, as in Figma).
    let fmt_id = egui::Id::new("color-format");
    let mut fmt: ColorFormat = ui.data(|d| d.get_temp(fmt_id)).unwrap_or_default();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        crate::chrome::menu_button(ui, fmt.label(), |ui| {
            for f in ColorFormat::ALL {
                if ui.selectable_label(fmt == f, f.label()).clicked() {
                    fmt = f;
                }
            }
        });
        let small = |ui: &mut Ui, v: &mut f32, max: f32| ui.add_sized([34.0, 20.0], DragValue::new(v).range(0.0..=max).speed(0.5).max_decimals(0)).changed();
        match fmt {
            ColorFormat::Hex => changed |= hex_field(ui, id.with("hex"), color, 64.0),
            ColorFormat::Css => {
                if let Some(c) = commit_field(ui, id.with("css"), css_rgba(*color), ui.available_width(), parse_css) {
                    *color = c;
                    changed = true;
                }
            }
            ColorFormat::Rgb => {
                let mut ch = color.0.map(|v| v as f32);
                let mut edit = false;
                for v in &mut ch[..3] {
                    edit |= small(ui, v, 255.0);
                }
                if edit {
                    *color = Rgba(ch.map(|v| v.round() as u8));
                    changed = true;
                }
            }
            ColorFormat::Hsl | ColorFormat::Hsb => {
                let hsl = fmt == ColorFormat::Hsl;
                let k = if hsl { hsb_to_hsl(hsb) } else { hsb };
                let mut v = [k[0] * 360.0, k[1] * 100.0, k[2] * 100.0].map(f32::round);
                let mut edit = small(ui, &mut v[0], 360.0);
                edit |= small(ui, &mut v[1], 100.0);
                edit |= small(ui, &mut v[2], 100.0);
                if edit {
                    let k = [(v[0] / 360.0).min(0.9999), v[1] / 100.0, v[2] / 100.0];
                    hsb = if hsl { hsl_to_hsb(k) } else { k };
                    *color = from_hsb(hsb, color.a());
                    changed = true;
                }
            }
        }
        if fmt != ColorFormat::Css {
            changed |= alpha_field(ui, color);
        }
    });
    ui.data_mut(|d| d.insert_temp(fmt_id, fmt));
    if changed {
        // Keep the knobs where they were dragged unless the color moved away from them.
        let k = if *color == from_hsb(hsb, color.a()) { hsb } else { to_hsb(*color) };
        ui.data_mut(|d| d.insert_temp(id, (color.with_alpha(255), k)));
    }
    // Swatches: colors on this page, then color styles. A click applies one.
    let sw: Option<std::sync::Arc<Swatches>> = ui.data(|d| d.get_temp(egui::Id::new(SWATCHES)));
    if let Some(sw) = sw {
        let page: Vec<(String, Rgba)> = sw.page.iter().take(40).map(|c| (format!("#{}", c.hex()), *c)).collect();
        for (title, list) in [("On this page", &page), ("Color styles", &sw.styles)] {
            if list.is_empty() {
                continue;
            }
            ui.add_space(8.0);
            ui.label(egui::RichText::new(title).small().color(crate::appearance::muted(ui.ctx())));
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                for (tip, c) in list {
                    if chip(ui, *c, tip).clicked() {
                        *color = *c;
                        changed = true;
                    }
                }
            });
        }
    }
    changed
}

/// Checkerboard-backed swatch showing a paint; returns the click response.
fn swatch(ui: &mut Ui, p: &Paint) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Color swatch"));
    let painter = ui.painter_at(rect);
    for i in 0..4 {
        let (x, y) = ((i % 2) as f32, (i / 2) as f32);
        let q = Rect::from_min_size(rect.min + vec2(x * 10.0, y * 10.0), vec2(10.0, 10.0));
        painter.rect_filled(q, 0.0, if (i % 3) == 0 { Color32::WHITE } else { Color32::from_gray(0xE0) });
    }
    match p.kind {
        PaintKind::Solid => {
            painter.rect_filled(rect, 0.0, p.color.to32());
        }
        PaintKind::Image => {
            painter.rect_filled(rect, 0.0, Color32::from_gray(0xC8));
            painter.circle_filled(rect.center() + vec2(3.0, -3.0), 2.5, Color32::WHITE);
            painter.line_segment([rect.left_bottom() + vec2(2.0, -3.0), rect.center() + vec2(-1.0, 1.0)], Stroke::new(1.5, Color32::WHITE));
        }
        _ => {
            let n = 10;
            for i in 0..n {
                let t = (i as f32 + 0.5) / n as f32;
                let q = Rect::from_min_max(pos2(rect.min.x + rect.width() * i as f32 / n as f32, rect.min.y), pos2(rect.min.x + rect.width() * (i + 1) as f32 / n as f32, rect.max.y));
                painter.rect_filled(q, 0.0, p.sample(t).to32());
            }
        }
    }
    painter.rect_stroke(rect, 2.0, Stroke::new(1.0, Color32::from_black_alpha(25)), StrokeKind::Inside);
    resp
}

/// A color swatch that opens the color picker.
pub fn color_swatch(ui: &mut Ui, id: egui::Id, color: &mut Rgba) -> bool {
    let sw = swatch(ui, &Paint::solid(*color));
    crate::chrome::popover(&sw, id.with("popup"), "Color", 240.0, |ui| rgb_picker(ui, color)).unwrap_or(false)
}

/// A bare color row: swatch with picker, hex, and opacity.
pub fn color_row(ui: &mut Ui, id: egui::Id, color: &mut Rgba) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        changed |= color_swatch(ui, id, color);
        changed |= hex_field(ui, id.with("hex"), color, 56.0);
        changed |= alpha_field(ui, color);
    });
    changed
}

pub enum PaintEvent {
    None,
    Changed,
    Remove,
    /// The user picked an image file for this paint.
    Image(Vec<u8>),
}

/// One Figma-style paint row: swatch (opens editor), value, opacity, visibility, remove.
pub fn paint_row(ui: &mut Ui, id: egui::Id, paint: &mut Paint, title: &str) -> PaintEvent {
    let mut ev = PaintEvent::None;
    ui.horizontal(|ui| {
        let sw = swatch(ui, paint);
        crate::chrome::popover(&sw, id.with("popup"), title, 240.0, |ui| {
            if let Some(e) = paint_editor(ui, id, paint) {
                ev = e;
            }
        });
        match paint.kind {
            PaintKind::Solid => {
                if hex_field(ui, id.with("hex"), &mut paint.color, 56.0) {
                    ev = PaintEvent::Changed;
                }
            }
            k => {
                ui.add_sized([56.0, 20.0], egui::Label::new(format!("{k:?}")));
            }
        }
        if alpha_field(ui, &mut paint.color) {
            ev = PaintEvent::Changed;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icon_button(ui, Icon::Minus, false, 22.0, "Remove").clicked() {
                ev = PaintEvent::Remove;
            }
            if icon_button(ui, Icon::Eye(paint.visible), false, 22.0, "Toggle visibility").clicked() {
                paint.visible = !paint.visible;
                ev = PaintEvent::Changed;
            }
        });
    });
    ev
}

fn paint_editor(ui: &mut Ui, id: egui::Id, paint: &mut Paint) -> Option<PaintEvent> {
    let mut ev = None;
    // Paint type and blend mode as dropdowns, as in Figma's header.
    ui.horizontal(|ui| {
        crate::chrome::menu_button(ui, format!("{:?}", paint.kind), |ui| {
            for k in PaintKind::ALL {
                if ui.selectable_label(paint.kind == k, format!("{k:?}")).clicked() && paint.kind != k {
                    paint.convert(k);
                    ev = Some(PaintEvent::Changed);
                }
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            crate::chrome::menu_button(ui, format!("{:?}", paint.blend), |ui| {
                for b in crate::doc::BlendMode::ALL {
                    if ui.selectable_label(paint.blend == b, format!("{b:?}")).clicked() && paint.blend != b {
                        paint.blend = b;
                        ev = Some(PaintEvent::Changed);
                    }
                }
            });
        });
    });
    ui.separator();
    match paint.kind {
        PaintKind::Solid => {
            if rgb_picker(ui, &mut paint.color) {
                ev = Some(PaintEvent::Changed);
            }
        }
        PaintKind::Linear | PaintKind::Radial | PaintKind::Angular | PaintKind::Diamond => {
            if paint.kind == PaintKind::Linear {
                // Angle of the gradient axis, measured like Figma (0° = left to right).
                let d = paint.to - paint.from;
                let mut deg = (-d.y.atan2(d.x).to_degrees()).round();
                ui.horizontal(|ui| {
                    ui.label("Angle");
                    if ui.add(DragValue::new(&mut deg).range(-360.0..=360.0).suffix("°").speed(1.0)).changed() {
                        let a = -deg.to_radians();
                        let half = egui::Vec2::angled(a) * 0.5;
                        paint.from = pos2(0.5, 0.5) - half;
                        paint.to = pos2(0.5, 0.5) + half;
                        ev = Some(PaintEvent::Changed);
                    }
                });
            }
            let mut remove = None;
            let selected_id = id.with("stop");
            let mut sel: usize = ui.data(|d| d.get_temp(selected_id)).unwrap_or(0).min(paint.stops.len().saturating_sub(1));
            for i in 0..paint.stops.len() {
                ui.horizontal(|ui| {
                    let (t, c) = &mut paint.stops[i];
                    let mut pct = (*t * 100.0).round();
                    if ui.radio(sel == i, "").clicked() {
                        sel = i;
                    }
                    if ui.add(DragValue::new(&mut pct).range(0.0..=100.0).suffix("%").speed(0.5)).changed() {
                        *t = pct / 100.0;
                        ev = Some(PaintEvent::Changed);
                    }
                    if hex_field(ui, id.with(("stop-hex", i)), c, 56.0) {
                        ev = Some(PaintEvent::Changed);
                    }
                    if alpha_field(ui, c) {
                        ev = Some(PaintEvent::Changed);
                    }
                    if paint.stops.len() > 2 && icon_button(ui, Icon::Minus, false, 20.0, "Remove stop").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                paint.stops.remove(i);
                ev = Some(PaintEvent::Changed);
            }
            if ui.button("Add stop").clicked() {
                let t = 0.5;
                let c = paint.sample(t);
                paint.stops.push((t, c));
                sel = paint.stops.len() - 1;
                ev = Some(PaintEvent::Changed);
            }
            paint.stops.sort_by(|a, b| a.0.total_cmp(&b.0));
            if let Some((_, c)) = paint.stops.get_mut(sel)
                && rgb_picker(ui, c) {
                    ev = Some(PaintEvent::Changed);
                }
            ui.data_mut(|d| d.insert_temp(selected_id, sel));
        }
        PaintKind::Image => {
            if ui.button("Choose image…").clicked()
                && let Some(path) = rfd::FileDialog::new().add_filter("Image", &["png", "jpg", "jpeg"]).pick_file()
                    && let Ok(bytes) = std::fs::read(path) {
                        ev = Some(PaintEvent::Image(bytes));
                    }
            if paint.image.is_empty() {
                ui.label("No image selected");
            }
            ui.horizontal(|ui| {
                use crate::doc::ScaleMode;
                for m in [ScaleMode::Fill, ScaleMode::Fit, ScaleMode::Crop, ScaleMode::Stretch, ScaleMode::Tile] {
                    if ui.selectable_label(paint.scale_mode == m, format!("{m:?}")).clicked() && paint.scale_mode != m {
                        // Crop starts from what Fill shows.
                        if m == ScaleMode::Crop {
                            paint.tile_scale = 1.0;
                            paint.from = egui::Pos2::ZERO;
                        }
                        paint.scale_mode = m;
                        ev = Some(PaintEvent::Changed);
                    }
                }
            });
            if paint.scale_mode == crate::doc::ScaleMode::Crop {
                let mut zoom = paint.tile_scale * 100.0;
                let (mut ox, mut oy) = (paint.from.x * 100.0, paint.from.y * 100.0);
                ui.horizontal(|ui| {
                    ui.add_sized([76.0, 18.0], egui::Label::new("Zoom"));
                    if ui.add(egui::Slider::new(&mut zoom, 100.0..=800.0).suffix("%").logarithmic(true)).changed() {
                        paint.tile_scale = zoom / 100.0;
                        ev = Some(PaintEvent::Changed);
                    }
                });
                for (v, label) in [(&mut ox, "Horizontal"), (&mut oy, "Vertical")] {
                    ui.horizontal(|ui| {
                        ui.add_sized([76.0, 18.0], egui::Label::new(label));
                        if ui.add(egui::Slider::new(v, -100.0..=100.0).show_value(false)).changed() {
                            ev = Some(PaintEvent::Changed);
                        }
                    });
                }
                if matches!(ev, Some(PaintEvent::Changed)) {
                    paint.from = egui::Pos2::new(ox / 100.0, oy / 100.0);
                }
            }
            ui.separator();
            let a = &mut paint.adjust;
            for (v, label) in [
                (&mut a.exposure, "Exposure"),
                (&mut a.contrast, "Contrast"),
                (&mut a.saturation, "Saturation"),
                (&mut a.temperature, "Temperature"),
                (&mut a.tint, "Tint"),
                (&mut a.highlights, "Highlights"),
                (&mut a.shadows, "Shadows"),
            ] {
                let mut pct = (*v * 100.0).round();
                ui.horizontal(|ui| {
                    ui.add_sized([76.0, 18.0], egui::Label::new(label));
                    if ui.add(egui::Slider::new(&mut pct, -100.0..=100.0).show_value(false)).changed() {
                        *v = pct / 100.0;
                        ev = Some(PaintEvent::Changed);
                    }
                });
            }
            if paint.scale_mode == crate::doc::ScaleMode::Tile {
                let mut pct = paint.tile_scale * 100.0;
                if ui.add(DragValue::new(&mut pct).range(1.0..=1000.0).suffix("%").prefix("Scale ")).changed() {
                    paint.tile_scale = pct / 100.0;
                    ev = Some(PaintEvent::Changed);
                }
            }
        }
    }
    ev
}

/// An effect row: name (opens settings), visibility, remove. Returns (changed, remove).
pub fn shadow_row(ui: &mut Ui, id: egui::Id, s: &mut Shadow) -> (bool, bool) {
    use crate::doc::EffectKind;
    let (mut changed, mut remove) = (false, false);
    ui.horizontal(|ui| {
        let open = ui.add(egui::Button::new(s.kind.label()).frame(false));
        crate::chrome::popover(&open, id.with("popup"), s.kind.label(), 240.0, |ui| {
            // Effect type as a dropdown, as in Figma's header.
            crate::chrome::menu_button(ui, s.kind.label(), |ui| {
                for k in [EffectKind::DropShadow, EffectKind::InnerShadow, EffectKind::LayerBlur, EffectKind::BackgroundBlur] {
                    if ui.selectable_label(s.kind == k, k.label()).clicked() && s.kind != k {
                        s.kind = k;
                        changed = true;
                    }
                }
            });
            ui.separator();
            egui::Grid::new(id.with("grid")).num_columns(4).show(ui, |ui| {
                let mut field = |ui: &mut Ui, label: &str, v: &mut f32, min: f32| {
                    ui.label(label);
                    changed |= ui.add(DragValue::new(v).speed(0.5).range(min..=1000.0).custom_formatter(|v, _| num(v as f32))).changed();
                };
                if s.kind.is_shadow() {
                    field(ui, "X", &mut s.offset.x, -1000.0);
                    field(ui, "Y", &mut s.offset.y, -1000.0);
                    ui.end_row();
                    field(ui, "Blur", &mut s.blur, 0.0);
                    field(ui, "Spread", &mut s.spread, -1000.0);
                } else {
                    field(ui, "Blur", &mut s.blur, 0.0);
                }
                ui.end_row();
            });
            if s.kind.is_shadow() {
                // Swatch, hex and opacity; the swatch expands the full picker in place.
                ui.add_space(4.0);
                let open_id = id.with("color-open");
                let mut open: bool = ui.data(|d| d.get_temp(open_id)).unwrap_or(false);
                ui.horizontal(|ui| {
                    if swatch(ui, &Paint::solid(s.color)).on_hover_text("Color").clicked() {
                        open = !open;
                    }
                    changed |= hex_field(ui, id.with("hex"), &mut s.color, 64.0);
                    changed |= alpha_field(ui, &mut s.color);
                });
                ui.data_mut(|d| d.insert_temp(open_id, open));
                if open {
                    ui.add_space(6.0);
                    changed |= rgb_picker(ui, &mut s.color);
                }
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            remove = icon_button(ui, Icon::Minus, false, 22.0, "Remove").clicked();
            if icon_button(ui, Icon::Eye(s.visible), false, 22.0, "Toggle visibility").clicked() {
                s.visible = !s.visible;
                changed = true;
            }
        });
    });
    (changed, remove)
}

pub enum StylePick {
    Apply(crate::doc::StyleId),
    Create(String),
}

/// Small preview of a style's value.
pub fn style_preview(ui: &mut Ui, v: &crate::doc::StyleValue) {
    use crate::doc::StyleValue;
    match v {
        StyleValue::Paint(p) => {
            let first = p.last().cloned().unwrap_or_else(|| Paint::solid(Rgba::WHITE));
            swatch(ui, &first);
        }
        StyleValue::Text(t) => {
            ui.add_sized([20.0, 20.0], egui::Label::new(egui::RichText::new("Ag").size(11.0).strong())).on_hover_text(format!("{} {}", t.font_family, num(t.font_size)));
        }
        StyleValue::Grid(_) => {
            let (r, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
            let (r, s) = (r.shrink(4.0), Stroke::new(1.0, crate::appearance::muted(ui.ctx())));
            ui.painter().rect_stroke(r, 1.0, s, StrokeKind::Inside);
            for t in [1.0 / 3.0, 2.0 / 3.0] {
                ui.painter().vline(r.min.x + r.width() * t, r.y_range(), s);
                ui.painter().hline(r.x_range(), r.min.y + r.height() * t, s);
            }
        }
        StyleValue::Effect(_) => {
            let (r, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
            ui.painter().rect_filled(r.shrink(3.0).translate(vec2(1.0, 2.0)), 3.0, Color32::from_black_alpha(60));
            ui.painter().rect_filled(r.shrink(3.0), 3.0, crate::appearance::card(ui.ctx()));
            ui.painter().rect_stroke(r.shrink(3.0), 3.0, Stroke::new(1.0, crate::appearance::border(ui.ctx())), StrokeKind::Inside);
        }
    }
}

/// Styles button with a popover: pick an existing style of this kind, or name and create one.
pub fn style_menu(ui: &mut Ui, id: egui::Id, styles: &[crate::doc::Style], same_kind: impl Fn(&crate::doc::StyleValue) -> bool) -> Option<StylePick> {
    let mut pick = None;
    let btn = icon_button(ui, Icon::Styles, false, 22.0, "Styles");
    crate::chrome::popover(&btn, id, "Styles", 240.0, |ui| {
        ui.label(egui::RichText::new("Local styles").strong());
        let mut any = false;
        // Grouped by slash folders, as in Figma ("HUD / Title" under HUD).
        let mut list: Vec<&crate::doc::Style> = styles.iter().filter(|s| same_kind(&s.value)).collect();
        let split = |n: &str| n.rsplit_once('/').map(|(f, l)| (Some(f.trim().to_string()), l.trim().to_string())).unwrap_or((None, n.to_string()));
        list.sort_by_key(|s| (split(&s.name).0.is_some(), s.name.to_lowercase()));
        let mut folder: Option<String> = None;
        for s in list {
            any = true;
            let (f, leaf) = split(&s.name);
            if f.is_some() && f != folder {
                ui.label(egui::RichText::new(f.as_deref().unwrap_or_default()).small().color(crate::appearance::muted(ui.ctx())));
            }
            folder = f;
            ui.horizontal(|ui| {
                if folder.is_some() {
                    ui.add_space(8.0);
                }
                style_preview(ui, &s.value);
                if ui.add(egui::Button::new(&leaf).frame(false)).clicked() {
                    pick = Some(StylePick::Apply(s.id));
                    ui.close();
                }
            });
        }
        if !any {
            ui.label(egui::RichText::new("No styles yet").color(crate::appearance::faint(ui.ctx())));
        }
        ui.separator();
        let name_id = id.with("new-name");
        let mut name: String = ui.data(|d| d.get_temp(name_id)).unwrap_or_default();
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut name).hint_text("New style name").desired_width(140.0));
            if ui.add_enabled(!name.trim().is_empty(), egui::Button::new("Create")).clicked() {
                pick = Some(StylePick::Create(name.trim().to_string()));
                name.clear();
                ui.close();
            }
        });
        ui.data_mut(|d| d.insert_temp(name_id, name));
    });
    pick
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_formats_convert_both_ways() {
        let c = Rgba([200, 80, 40, 255]);
        assert_eq!(from_hsb(to_hsb(c), 255), c);
        let red = to_hsb(Rgba::rgb(255, 0, 0));
        assert_eq!(red, [0.0, 1.0, 1.0]);
        let hsl = hsb_to_hsl(red);
        assert!((hsl[1] - 1.0).abs() < 1e-6 && (hsl[2] - 0.5).abs() < 1e-6, "red is 0, 100%, 50% in HSL");
        assert_eq!(from_hsb(hsl_to_hsb(hsl), 255), Rgba::rgb(255, 0, 0));
        assert_eq!(parse_css("rgba(10, 20, 30, 0.5)"), Some(Rgba([10, 20, 30, 128])));
        assert_eq!(parse_css(&css_rgba(c.with_alpha(51))), Some(c.with_alpha(51)));
        assert_eq!(parse_css("#FFF"), Some(Rgba::WHITE));
        assert_eq!(parse_css("rgb(1, 2)"), None);
    }
}
