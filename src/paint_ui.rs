//! Fill, stroke, and effect editors for the design panel (Figma-style rows with popovers).

use crate::canvas::num;
use crate::doc::{Paint, PaintKind, Rgba, Shadow};
use crate::panels::{Icon, icon_button};
use egui::{Color32, DragValue, FontId, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

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

/// Inline RGB picker (alpha lives in its own field, as in Figma).
fn rgb_picker(ui: &mut Ui, color: &mut Rgba) -> bool {
    let mut c = Color32::from_rgb(color.0[0], color.0[1], color.0[2]);
    let changed = egui::color_picker::color_picker_color32(ui, &mut c, egui::color_picker::Alpha::Opaque);
    if changed {
        *color = Rgba([c.r(), c.g(), c.b(), color.a()]);
    }
    changed
}

/// Checkerboard-backed swatch showing a paint; returns the click response.
fn swatch(ui: &mut Ui, p: &Paint) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
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
        PaintKind::Linear | PaintKind::Radial => {
            let n = 10;
            for i in 0..n {
                let t = (i as f32 + 0.5) / n as f32;
                let q = Rect::from_min_max(pos2(rect.min.x + rect.width() * i as f32 / n as f32, rect.min.y), pos2(rect.min.x + rect.width() * (i + 1) as f32 / n as f32, rect.max.y));
                painter.rect_filled(q, 0.0, p.sample(t).to32());
            }
        }
        PaintKind::Image => {
            painter.rect_filled(rect, 0.0, Color32::from_gray(0xC8));
            painter.circle_filled(rect.center() + vec2(3.0, -3.0), 2.5, Color32::WHITE);
            painter.line_segment([rect.left_bottom() + vec2(2.0, -3.0), rect.center() + vec2(-1.0, 1.0)], Stroke::new(1.5, Color32::WHITE));
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
    ui.horizontal(|ui| {
        for k in [PaintKind::Solid, PaintKind::Linear, PaintKind::Radial, PaintKind::Image] {
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
        PaintKind::Linear | PaintKind::Radial => {
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
            // The type picker sits in the editor itself: a dropdown here opens a second popup,
            // and clicking in it would count as a click outside and close the editor.
            egui::Grid::new(id.with("kind")).num_columns(2).spacing([4.0, 4.0]).show(ui, |ui| {
                for (i, k) in [EffectKind::DropShadow, EffectKind::InnerShadow, EffectKind::LayerBlur, EffectKind::BackgroundBlur].into_iter().enumerate() {
                    changed |= ui.add_sized([108.0, 22.0], egui::Button::selectable(s.kind == k, k.label())).clicked() && {
                        s.kind = k;
                        true
                    };
                    if i % 2 == 1 {
                        ui.end_row();
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
                ui.horizontal(|ui| {
                    changed |= hex_field(ui, id.with("hex"), &mut s.color, 56.0);
                    changed |= alpha_field(ui, &mut s.color);
                });
                changed |= rgb_picker(ui, &mut s.color);
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
