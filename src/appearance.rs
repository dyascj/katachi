//! Appearance: light / dark / system theme, and "glass": translucent, tinted panels over a
//! blurred desktop (the canvas stays solid). Device-local, saved in the data directory.

use egui::{Color32, Context, FontId, TextStyle, Theme};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemePref {
    Light,
    Dark,
    #[default]
    System,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    pub theme: ThemePref,
    pub glass: bool,
    /// Panel opacity with glass on, 0.15..=1.
    pub panel_opacity: f32,
    /// Desktop blur behind the window (macOS), 1..=64.
    pub blur: u8,
    /// Panel tint: hue in degrees and saturation 0..=1.
    pub hue: f32,
    pub saturation: f32,
}

impl Default for Appearance {
    fn default() -> Self {
        Self { theme: ThemePref::System, glass: false, panel_opacity: 0.8, blur: 24, hue: 240.0, saturation: 0.0 }
    }
}

fn path() -> std::path::PathBuf {
    crate::files::data_dir().join("appearance.json")
}

pub fn load() -> Appearance {
    std::fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn save(a: &Appearance) {
    let _ = crate::files::write_atomic(&path(), serde_json::to_string_pretty(a).unwrap_or_default().as_bytes());
}

fn hsl(h: f32, s: f32, l: f32) -> Color32 {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = (h.rem_euclid(360.0)) / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let u = |v: f32| ((v + m).clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgb(u(r), u(g), u(b))
}

/// Base panel color for a theme, tinted when glass is on.
fn panel(a: &Appearance, dark: bool) -> Color32 {
    if a.glass {
        let c = hsl(a.hue, a.saturation, if dark { 0.11 } else { 0.97 });
        Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (a.panel_opacity.clamp(0.15, 1.0) * 255.0) as u8)
    } else if dark {
        Color32::from_gray(0x2C)
    } else {
        Color32::WHITE
    }
}

/// Install fonts-independent style for both themes and pick the active one.
pub fn apply(ctx: &Context, a: &Appearance) {
    ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(match a.theme {
        ThemePref::Light => egui::SystemTheme::Light,
        ThemePref::Dark => egui::SystemTheme::Dark,
        ThemePref::System => egui::SystemTheme::SystemDefault,
    }));
    ctx.set_theme(match a.theme {
        ThemePref::Light => egui::ThemePreference::Light,
        ThemePref::Dark => egui::ThemePreference::Dark,
        ThemePref::System => egui::ThemePreference::System,
    });
    for theme in [Theme::Light, Theme::Dark] {
        let dark = theme == Theme::Dark;
        let base = panel(a, dark);
        ctx.style_mut_of(theme, |s| {
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
            s.visuals = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
            let v = &mut s.visuals;
            v.panel_fill = base;
            // Floating windows, popovers and menus are always solid, glass or not (as in Figma).
            v.window_fill = if dark { Color32::from_gray(0x2C) } else { Color32::WHITE };
            let (field, hover, line, text, sel) = if dark {
                (Color32::from_gray(0x38), Color32::from_gray(0x44), Color32::from_gray(0x44), Color32::from_gray(0xF0), Color32::from_rgb(0x1F, 0x4F, 0x7A))
            } else {
                (Color32::from_gray(0xF5), Color32::from_gray(0xEB), Color32::from_gray(0xE6), Color32::from_gray(0x1E), Color32::from_rgb(0xBD, 0xE3, 0xFF))
            };
            // With glass, fields are a translucent wash so the tint shows through.
            let wash = |c: Color32| if a.glass { Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 150) } else { c };
            v.extreme_bg_color = wash(field);
            v.faint_bg_color = wash(field);
            v.override_text_color = Some(text);
            v.selection.bg_fill = sel;
            v.selection.stroke.color = crate::canvas::BLUE;
            v.widgets.noninteractive.bg_stroke.color = line;
            v.widgets.noninteractive.bg_fill = base;
            for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
                w.corner_radius = egui::CornerRadius::same(5);
            }
            v.widgets.inactive.weak_bg_fill = wash(field);
            v.widgets.inactive.bg_fill = wash(field);
            v.widgets.hovered.weak_bg_fill = wash(hover);
            v.widgets.hovered.bg_fill = wash(hover);
            v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, if dark { Color32::from_gray(0x5A) } else { Color32::from_gray(0xD0) });
            v.window_stroke = egui::Stroke::new(1.0, line);
            crate::chrome::style(s);
        });
    }
}

fn is_dark(ctx: &Context) -> bool {
    ctx.global_style().visuals.dark_mode
}
fn pick(ctx: &Context, light: Color32, dark: Color32) -> Color32 {
    if is_dark(ctx) { dark } else { light }
}

/// Theme-aware colors for custom-painted UI.
pub fn subtle(ctx: &Context) -> Color32 {
    ctx.global_style().visuals.extreme_bg_color
}
pub fn sel_bg(ctx: &Context) -> Color32 {
    pick(ctx, Color32::from_rgb(0xE5, 0xF4, 0xFF), Color32::from_rgb(0x3C, 0x43, 0x60))
}
pub fn child_bg(ctx: &Context) -> Color32 {
    pick(ctx, Color32::from_rgb(0xF2, 0xF9, 0xFF), Color32::from_rgb(0x22, 0x30, 0x3D))
}
/// Primary text and icons.
pub fn ink(ctx: &Context) -> Color32 {
    pick(ctx, Color32::from_gray(0x1E), Color32::from_gray(0xF0))
}
/// Secondary text and icons.
pub fn muted(ctx: &Context) -> Color32 {
    pick(ctx, Color32::from_gray(0x80), Color32::from_gray(0xA0))
}
/// Hints and timestamps.
pub fn faint(ctx: &Context) -> Color32 {
    pick(ctx, Color32::from_gray(0x99), Color32::from_gray(0x8A))
}
/// Row icons (eye, lock).
pub fn icon(ctx: &Context) -> Color32 {
    pick(ctx, Color32::from_gray(0x60), Color32::from_gray(0xB8))
}
/// Hidden or disabled marks.
pub fn dim(ctx: &Context) -> Color32 {
    pick(ctx, Color32::from_gray(0xB3), Color32::from_gray(0x60))
}
/// Raised surfaces (menus, cards, comment bubbles).
pub fn card(ctx: &Context) -> Color32 {
    ctx.global_style().visuals.window_fill
}
pub fn border(ctx: &Context) -> Color32 {
    ctx.global_style().visuals.widgets.noninteractive.bg_stroke.color
}

/// The canvas background for pages left at the default color follows the theme, as in Figma.
pub fn page_bg(ctx: &Context, bg: crate::doc::Rgba) -> Color32 {
    if bg == crate::doc::PAGE_BG && is_dark(ctx) { Color32::from_gray(0x1E) } else { bg.with_alpha(255).to32() }
}

/// macOS: with glass, the window is clear and the desktop behind it blurred (the same private
/// CoreGraphics call macOS's own translucent windows use); without, it's an ordinary opaque
/// window so the title bar draws normally. Elsewhere glass is translucency only.
#[cfg(target_os = "macos")]
pub fn set_window_glass(frame: &eframe::Frame, glass: bool, radius: u8) {
    use objc2::runtime::{AnyClass, AnyObject, Bool};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGSMainConnectionID() -> i32;
        fn CGSSetWindowBackgroundBlurRadius(connection: i32, window: i32, radius: i32) -> i32;
    }
    let Ok(handle) = frame.window_handle() else { return };
    let RawWindowHandle::AppKit(h) = handle.as_raw() else { return };
    let Some(color_class) = AnyClass::get(c"NSColor") else { return };
    unsafe {
        let view = h.ns_view.as_ptr() as *mut AnyObject;
        let window: *mut AnyObject = objc2::msg_send![view, window];
        if window.is_null() {
            return;
        }
        let color: *mut AnyObject = if glass { objc2::msg_send![color_class, clearColor] } else { objc2::msg_send![color_class, windowBackgroundColor] };
        let _: () = objc2::msg_send![window, setOpaque: Bool::new(!glass)];
        let _: () = objc2::msg_send![window, setBackgroundColor: color];
        let number: isize = objc2::msg_send![window, windowNumber];
        CGSSetWindowBackgroundBlurRadius(CGSMainConnectionID(), number as i32, if glass { radius as i32 } else { 0 });
    }
}

#[cfg(not(target_os = "macos"))]
pub fn set_window_glass(_frame: &eframe::Frame, _glass: bool, _radius: u8) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glass_tints_panels_translucent_and_settings_roundtrip() {
        let a = Appearance { glass: true, panel_opacity: 0.5, hue: 200.0, saturation: 0.5, ..Default::default() };
        let c = panel(&a, true);
        assert_eq!(c.a(), 127);
        assert!(c.b() > c.r(), "blue-ish tint for hue 200: {c:?}");
        assert_eq!(panel(&Appearance::default(), false), Color32::WHITE);
        let json = serde_json::to_string(&a).unwrap();
        assert_eq!(serde_json::from_str::<Appearance>(&json).unwrap(), a);
        assert_eq!(serde_json::from_str::<Appearance>("{\"glass\":true}").unwrap().panel_opacity, 0.8, "missing fields default");
    }
}
