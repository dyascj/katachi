//! Fill, stroke, and effect editors for the design panel (Figma-style rows with popovers).

use crate::canvas::num;
use crate::doc::{Paint, PaintKind, Rgba, Shadow};
use crate::panels::{Icon, icon_button};
use egui::{Color32, DragValue, FontId, Popup, PopupCloseBehavior, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

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

pub enum PaintEvent {
    None,
    Changed,
    Remove,
    /// The user picked an image file for this paint.
    Image(Vec<u8>),
}

/// One Figma-style paint row: swatch (opens editor), value, opacity, visibility, remove.
pub fn paint_row(ui: &mut Ui, id: egui::Id, paint: &mut Paint) -> PaintEvent {
    let mut ev = PaintEvent::None;
    ui.horizontal(|ui| {
        let sw = swatch(ui, paint);
        Popup::menu(&sw).id(id.with("popup")).close_behavior(PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            ui.set_width(240.0);
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
        Popup::menu(&open).id(id.with("popup")).close_behavior(PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            ui.set_width(230.0);
            egui::ComboBox::from_id_salt(id.with("kind")).selected_text(s.kind.label()).show_ui(ui, |ui| {
                for k in [EffectKind::DropShadow, EffectKind::InnerShadow, EffectKind::LayerBlur, EffectKind::BackgroundBlur] {
                    changed |= ui.selectable_value(&mut s.kind, k, k.label()).changed();
                }
            });
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
