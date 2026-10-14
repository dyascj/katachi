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

/// Hex text field that commits on Enter / focus loss, keeping alpha.
pub fn hex_field(ui: &mut Ui, id: egui::Id, color: &mut Rgba, width: f32) -> bool {
    let mut buf = ui.data_mut(|d| d.get_temp::<String>(id)).unwrap_or_else(|| color.hex());
    let resp = ui.add(egui::TextEdit::singleline(&mut buf).id(id).desired_width(width).font(FontId::monospace(11.0)));
    let mut changed = false;
    if resp.lost_focus() {
        if let Some(c) = parse_hex(&buf) {
            *color = c.with_alpha(color.a());
            changed = true;
        }
        ui.data_mut(|d| d.remove::<String>(id));
    } else if resp.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, buf));
    } else {
        ui.data_mut(|d| d.remove::<String>(id));
    }
    changed
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

/// Figma's color picker: a saturation and brightness square, then thin hue and opacity sliders
/// with round knobs, then hex and opacity.
fn rgb_picker(ui: &mut Ui, color: &mut Rgba) -> bool {
    use egui::ecolor::Hsva;
    use egui::epaint::{Mesh, Vertex, WHITE_UV};
    let id = ui.id().with("figma-picker");
    let mut changed = false;
    if let Some(c) = ui.data_mut(|d| d.remove_temp::<Option<Rgba>>(id.with("eyedropped")).flatten()) {
        *color = c.with_alpha(color.a());
        changed = true;
    }
    let rgb = Color32::from_rgb(color.0[0], color.0[1], color.0[2]);
    let mut hsv = Hsva::from(rgb);
    // Grays and black have no hue: keep the last one so the square doesn't jump to red.
    let kept: Option<(Color32, f32)> = ui.data(|d| d.get_temp(id));
    if let Some((c, h)) = kept
        && c == rgb
    {
        hsv.h = h;
    }
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
    let pure = Color32::from(Hsva::new(hsv.h, 1.0, 1.0, 1.0));
    let mut m = Mesh::default();
    quad(&mut m, sq, [Color32::WHITE, pure, pure, Color32::WHITE]);
    quad(&mut m, sq, [Color32::TRANSPARENT, Color32::TRANSPARENT, Color32::BLACK, Color32::BLACK]);
    ui.painter().add(m);
    ui.painter().rect_stroke(sq, 4.0, Stroke::new(1.0, Color32::from_black_alpha(25)), StrokeKind::Inside);
    if let Some(p) = resp.interact_pointer_pos() {
        hsv.s = ((p.x - sq.min.x) / sq.width()).clamp(0.0, 1.0);
        hsv.v = 1.0 - ((p.y - sq.min.y) / sq.height()).clamp(0.0, 1.0);
        changed = true;
    }
    knob(ui, egui::pos2(sq.min.x + hsv.s * sq.width(), sq.min.y + (1.0 - hsv.v) * sq.height()), Color32::from(Hsva::new(hsv.h, hsv.s, hsv.v, 1.0)));
    ui.add_space(6.0);
    // Hue.
    let (bar, resp) = ui.allocate_exact_size(vec2(w, 12.0), Sense::click_and_drag());
    let mut m = Mesh::default();
    for i in 0..6 {
        let (a, b) = (i as f32 / 6.0, (i + 1) as f32 / 6.0);
        let r = Rect::from_min_max(egui::pos2(bar.min.x + a * bar.width(), bar.min.y), egui::pos2(bar.min.x + b * bar.width(), bar.max.y));
        let (ca, cb) = (Color32::from(Hsva::new(a, 1.0, 1.0, 1.0)), Color32::from(Hsva::new(b, 1.0, 1.0, 1.0)));
        quad(&mut m, r, [ca, cb, cb, ca]);
    }
    ui.painter().add(m);
    if let Some(p) = resp.interact_pointer_pos() {
        hsv.h = ((p.x - bar.min.x) / bar.width()).clamp(0.0, 0.9999);
        changed = true;
    }
    knob(ui, egui::pos2(bar.min.x + hsv.h * bar.width(), bar.center().y), pure);
    ui.add_space(6.0);
    // Opacity.
    let (bar, resp) = ui.allocate_exact_size(vec2(w, 12.0), Sense::click_and_drag());
    for i in 0..(bar.width() / 6.0) as usize {
        for j in 0..2 {
            if (i + j) % 2 == 0 {
                let r = Rect::from_min_size(egui::pos2(bar.min.x + i as f32 * 6.0, bar.min.y + j as f32 * 6.0), vec2(6.0, 6.0)).intersect(bar);
                ui.painter().rect_filled(r, 0.0, Color32::from_gray(0xD8));
            }
        }
    }
    let solid = Color32::from(Hsva::new(hsv.h, hsv.s, hsv.v, 1.0));
    let mut m = Mesh::default();
    quad(&mut m, bar, [Color32::TRANSPARENT, solid, solid, Color32::TRANSPARENT]);
    ui.painter().add(m);
    let mut a = color.a() as f32 / 255.0;
    if let Some(p) = resp.interact_pointer_pos() {
        a = ((p.x - bar.min.x) / bar.width()).clamp(0.0, 1.0);
        changed = true;
    }
    knob(ui, egui::pos2(bar.min.x + a * bar.width(), bar.center().y), solid);
    if changed {
        let c = Color32::from(Hsva::new(hsv.h, hsv.s, hsv.v, 1.0));
        *color = Rgba([c.r(), c.g(), c.b(), (a * 255.0).round() as u8]);
        ui.data_mut(|d| d.insert_temp(id, (c, hsv.h)));
    }
    ui.add_space(8.0);
    // Hex and opacity, as Figma shows under the sliders.
    ui.horizontal(|ui| {
        if icon_button(ui, Icon::Eyedropper, false, 22.0, "Pick color  I").clicked() {
            ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new(EYEDROP_REQUEST), Some(id)));
        }
        ui.label(egui::RichText::new("Hex").color(crate::appearance::muted(ui.ctx())));
        changed |= hex_field(ui, id.with("hex"), color, 80.0);
        changed |= alpha_field(ui, color);
    });
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

/// A bare color row: swatch with picker, hex, and opacity.
pub fn color_row(ui: &mut Ui, id: egui::Id, color: &mut Rgba) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        let sw = swatch(ui, &Paint::solid(*color));
        crate::chrome::popover(&sw, id.with("popup"), "Color", 240.0, |ui| {
            changed |= rgb_picker(ui, color);
        });
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
    // Paint type as a dropdown, as in Figma's header.
    crate::chrome::menu_button(ui, format!("{:?}", paint.kind), |ui| {
        for k in PaintKind::ALL {
            if ui.selectable_label(paint.kind == k, format!("{k:?}")).clicked() && paint.kind != k {
                paint.convert(k);
                ev = Some(PaintEvent::Changed);
            }
        }
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
                    if ui.add(DragValue::new(&mut deg).suffix("°").speed(1.0)).changed() {
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
