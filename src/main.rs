mod app;
mod boolean;
mod canvas;
mod doc;
mod export;
mod files;
mod paint_ui;
mod panels;
mod prototype;
mod render;
mod svg_import;
mod text;
mod variables_ui;
mod vector_edit;
#[cfg(test)]
mod tests;

use egui::{Color32, FontData, FontDefinitions, FontFamily, FontId, TextStyle};
use std::sync::Arc;

fn main() -> eframe::Result {
    let path = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Katachi")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([800.0, 500.0])
            .with_icon(eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon/icon.png")).unwrap_or_default()),
        multisampling: 4,
        ..Default::default()
    };
    eframe::run_native(
        "Katachi",
        options,
        Box::new(|cc| {
            setup_style(&cc.egui_ctx);
            Ok(Box::new(app::App::new(path)))
        }),
    )
}

fn setup_style(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let names = ["Inter", "Inter Medium", "Inter SemiBold", "Inter Bold"];
    for (name, bytes) in names.iter().zip(export::FONTS) {
        fonts.font_data.insert(name.to_string(), Arc::new(FontData::from_static(bytes)));
    }
    // Inter first, egui's bundled fonts after it as glyph fallbacks.
    let fallbacks = fonts.families[&FontFamily::Proportional].clone();
    fonts.families.get_mut(&FontFamily::Proportional).unwrap().insert(0, "Inter".into());
    for name in &names[1..] {
        let mut list = vec![name.to_string()];
        list.extend(fallbacks.iter().cloned());
        fonts.families.insert(FontFamily::Name((*name).into()), list);
    }
    ctx.set_fonts(fonts);
    ctx.options_mut(|o| o.zoom_with_keyboard = false);
    ctx.set_theme(egui::Theme::Light);
    ctx.style_mut_of(egui::Theme::Light, |s| {
        s.text_styles = [
            (TextStyle::Heading, FontId::proportional(13.0)),
            (TextStyle::Body, FontId::proportional(12.0)),
            (TextStyle::Button, FontId::proportional(12.0)),
            (TextStyle::Small, FontId::proportional(10.0)),
            (TextStyle::Monospace, FontId::monospace(11.0)),
        ]
        .into();
        s.spacing.button_padding = egui::vec2(8.0, 4.0);
        s.spacing.interact_size.y = 24.0;
        let v = &mut s.visuals;
        v.panel_fill = Color32::WHITE;
        v.window_fill = Color32::WHITE;
        v.extreme_bg_color = Color32::from_gray(0xF5);
        v.override_text_color = Some(Color32::from_gray(0x1E));
        v.selection.bg_fill = Color32::from_rgb(0xBD, 0xE3, 0xFF);
        v.selection.stroke.color = canvas::BLUE;
        v.widgets.noninteractive.bg_stroke.color = Color32::from_gray(0xE6);
        for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
            w.corner_radius = egui::CornerRadius::same(5);
        }
        v.widgets.inactive.weak_bg_fill = Color32::from_gray(0xF5);
        v.widgets.inactive.bg_fill = Color32::from_gray(0xF5);
        v.widgets.hovered.weak_bg_fill = Color32::from_gray(0xEB);
        v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, Color32::from_gray(0xD0));
    });
}
