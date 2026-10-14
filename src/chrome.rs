//! The look of everything that floats: popovers, menus, modals and tooltips, as in Figma. Always
//! solid (glass only applies to the side panels), 12-point corners, a soft shadow, a 40-point
//! title row with a close button, 16-point padding, and near-black menus in both themes.
//! Measurements come from a study of Figma's desktop app (see the Katachi Tests folder).

use egui::{Color32, CornerRadius, Frame, Margin, Response, RichText, Sense, Shadow, Stroke, Ui, vec2};

pub const RADIUS: u8 = 12;
/// Figma's menus: near-black in both themes, white text, blue highlight.
pub const MENU_BG: Color32 = Color32::from_rgb(0x1E, 0x1E, 0x1E);
pub const MENU_TEXT: Color32 = Color32::from_rgb(0xF5, 0xF5, 0xF5);
pub const MENU_MUTED: Color32 = Color32::from_rgb(0xA5, 0xA5, 0xA5);
/// Hover in menus is gray; the current choice in a select menu is blue.
pub const MENU_HOVER: Color32 = Color32::from_rgb(0x38, 0x38, 0x38);
pub const MENU_CURRENT: Color32 = Color32::from_rgb(0x0D, 0x99, 0xFF);

pub fn shadow() -> Shadow {
    Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(76) }
}

/// Install the floating-surface style for both themes (called from `appearance::apply`).
pub fn style(s: &mut egui::Style) {
    s.visuals.window_corner_radius = CornerRadius::same(RADIUS);
    s.visuals.menu_corner_radius = CornerRadius::same(RADIUS);
    s.visuals.window_shadow = shadow();
    s.visuals.popup_shadow = shadow();
    s.spacing.window_margin = Margin::same(12);
    s.spacing.menu_margin = Margin::same(8);
    s.spacing.menu_spacing = 2.0;
    // Tooltips: a short dark label, as in Figma.
    s.interaction.tooltip_delay = 0.35;
}

/// A solid popover surface.
pub fn popover_frame(ui: &Ui) -> Frame {
    Frame::new()
        .fill(ui.visuals().window_fill)
        .corner_radius(CornerRadius::same(RADIUS))
        .stroke(Stroke::new(1.0, crate::appearance::border(ui.ctx())))
        .shadow(shadow())
        .inner_margin(Margin::ZERO)
}

/// The title row of a popover or modal: title on the left, close on the right, a hairline under
/// it. Returns true when the close button was clicked.
pub fn header(ui: &mut Ui, title: &str) -> bool {
    let mut close = false;
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.horizontal(|ui| {
        ui.set_min_height(40.0);
        ui.add_space(16.0);
        ui.label(RichText::new(title).strong().size(12.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(8.0);
            close = close_button(ui).clicked();
        });
    });
    let r = ui.max_rect();
    ui.painter().hline(r.x_range(), ui.cursor().min.y, Stroke::new(1.0, crate::appearance::border(ui.ctx())));
    close
}

/// A 24-point ✕ button.
pub fn close_button(ui: &mut Ui) -> Response {
    let (r, resp) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Close"));
    if resp.hovered() {
        ui.painter().rect_filled(r, 5.0, crate::appearance::subtle(ui.ctx()));
    }
    let c = r.center();
    let s = Stroke::new(1.3, crate::appearance::icon(ui.ctx()));
    ui.painter().line_segment([c - vec2(4.5, 4.5), c + vec2(4.5, 4.5)], s);
    ui.painter().line_segment([c + vec2(-4.5, 4.5), c + vec2(4.5, -4.5)], s);
    resp
}

/// Dark menu styling (context menus, dropdowns, the main menu), for any theme.
pub fn menu_style(s: &mut egui::Style) {
    let v = &mut s.visuals;
    v.window_fill = MENU_BG;
    v.panel_fill = MENU_BG;
    v.window_stroke = Stroke::new(1.0, Color32::from_gray(0x38));
    v.override_text_color = Some(MENU_TEXT);
    v.weak_text_color = Some(MENU_MUTED);
    for w in [&mut v.widgets.inactive, &mut v.widgets.noninteractive] {
        w.weak_bg_fill = Color32::TRANSPARENT;
        w.bg_fill = Color32::TRANSPARENT;
        w.fg_stroke.color = MENU_TEXT;
        w.bg_stroke = Stroke::NONE;
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_gray(0x3A));
    // Disabled items fade toward the menu's own color, so they read the same in both themes.
    v.widgets.noninteractive.weak_bg_fill = MENU_BG;
    v.disabled_alpha = 0.45;
    for w in [&mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.weak_bg_fill = MENU_HOVER;
        w.bg_fill = MENU_HOVER;
        w.fg_stroke.color = Color32::WHITE;
        w.bg_stroke = Stroke::NONE;
        w.corner_radius = CornerRadius::same(5);
    }
    v.widgets.inactive.corner_radius = CornerRadius::same(5);
    v.selection.bg_fill = MENU_CURRENT;
    v.extreme_bg_color = Color32::from_gray(0x2C);
    s.spacing.button_padding = vec2(8.0, 4.0);
    s.spacing.interact_size.y = 24.0;
}

/// Right-click menu on a response, styled as a Figma menu.
pub fn context_menu(resp: &Response, add: impl FnOnce(&mut Ui)) {
    egui::Popup::context_menu(resp).style(menu_style).show(add);
}

/// `ui.menu_button`, styled as a Figma menu (submenus of it inherit the style).
pub fn menu_button<'a, R>(ui: &mut Ui, atoms: impl egui::IntoAtoms<'a>, add: impl FnOnce(&mut Ui) -> R) -> Response {
    use egui::containers::menu::{MenuButton, MenuConfig, MenuState, SubMenu, SubMenuButton, is_in_menu};
    let config = MenuConfig::new().style(menu_style);
    if !is_in_menu(ui) {
        return MenuButton::new(atoms).config(config).ui(ui, add).0;
    }
    // Inside a light popover: egui frames a submenu with the parent's style before applying
    // the submenu's own, which gives white text on a white menu. Draw the button normally,
    // then open the submenu from a dark-styled scope.
    let resp = ui.add(egui::Button::new(atoms).right_text(SubMenuButton::RIGHT_ARROW));
    // A pick closes just the submenu, a frame later (closing it during the click would let the
    // popover take the click as one outside it), so the popover stays open as in Figma.
    let sub = SubMenu::id_from_widget_id(resp.id);
    if ui.data_mut(|d| d.remove_temp::<bool>(sub.with("picked"))).unwrap_or(false) {
        MenuState::from_ui(ui, |s, _| s.open_item = s.open_item.filter(|o| *o != sub));
    }
    let shown = ui.scope(|ui| {
        menu_style(ui.style_mut());
        SubMenu::new().config(config.close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)).show(ui, &resp, add)
    });
    if shown.inner.is_some_and(|r| r.response.contains_pointer()) && ui.input(|i| i.pointer.any_click()) {
        ui.data_mut(|d| d.insert_temp(sub.with("picked"), true));
    }
    resp
}

/// Width of the right panel; popovers from it open just left of it, as in Figma.
const RIGHT_PANEL: f32 = 248.0;

/// Keep open popovers open through the next click: it edits what they show (a gradient
/// handle on the canvas, a color picked with the eyedropper).
pub fn pin_popovers(ctx: &egui::Context, on: bool) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("pin-popovers"), on));
}

/// A Figma popover toggled by `button`: solid, titled, with a close button, opening to the left
/// of the right panel (or below the button elsewhere), and closing on a click outside or Escape.
pub fn popover<R>(button: &Response, id: egui::Id, title: &str, width: f32, add: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    let ctx = button.ctx.clone();
    let pinned = ctx.data(|d| d.get_temp(egui::Id::new("pin-popovers"))).unwrap_or(false);
    let edge = ctx.content_rect().max.x - RIGHT_PANEL;
    let in_panel = button.rect.min.x >= edge - 1.0;
    let mut p = egui::Popup::from_toggle_button_response(button)
        .id(id)
        .close_behavior(if pinned { egui::PopupCloseBehavior::IgnoreClicks } else { egui::PopupCloseBehavior::CloseOnClickOutside })
        // Ordinary widget styling inside (egui's popups default to menu styling).
        .style(|_: &mut egui::Style| {})
        .frame(Frame::NONE)
        .width(width);
    if in_panel {
        let top = egui::pos2(edge, button.rect.min.y);
        p = p.anchor(egui::Rect::from_min_max(top, top)).align(egui::RectAlign::LEFT_START).gap(1.0);
    }
    p.show(|ui| {
        popover_frame(ui)
            .show(ui, |ui| {
                ui.set_width(width);
                if header(ui, title) {
                    ui.close();
                }
                Frame::NONE.inner_margin(Margin::same(16)).show(ui, add).inner
            })
            .inner
    })
    .map(|r| r.inner)
}

/// A floating utility window (Version history, Find and replace, Local variables...): the
/// popover look, a title row with a close button, draggable, no dimming.
pub fn window<R>(ctx: &egui::Context, title: &str, open: &mut bool, width: f32, add: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    let mut close = false;
    // Below the tab strip, centered, never over the traffic lights.
    let screen = ctx.content_rect();
    let room = egui::Rect::from_min_max(egui::pos2(screen.min.x, screen.min.y + crate::shell::STRIP_H + 8.0), screen.max);
    let out = egui::Window::new(title)
        .open(open)
        .pivot(egui::Align2::CENTER_TOP)
        .default_pos(egui::pos2(screen.center().x, room.min.y + 48.0))
        .constrain_to(room)
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .default_width(width)
        .frame(Frame::new().fill(ctx.global_style().visuals.window_fill).corner_radius(CornerRadius::same(RADIUS)).stroke(Stroke::new(1.0, crate::appearance::border(ctx))).shadow(shadow()))
        .show(ctx, |ui| {
            ui.set_width(width);
            close = header(ui, title);
            Frame::NONE.inner_margin(Margin::same(16)).show(ui, add).inner
        })
        .and_then(|r| r.inner);
    if close {
        *open = false;
    }
    out
}

/// A Figma dialog for a decision: dimmed backdrop, title and close, body, then a footer of
/// buttons on the right. Returns the index of the clicked button, or `Some(usize::MAX)` when
/// closed with the X, Escape or a click outside.
pub fn dialog(ctx: &egui::Context, id: &str, title: &str, width: f32, body: impl FnOnce(&mut Ui), buttons: &[(&str, bool)]) -> Option<usize> {
    let mut pick = None;
    let m = egui::Modal::new(egui::Id::new(id)).frame(Frame::NONE).backdrop_color(Color32::from_black_alpha(128)).show(ctx, |ui| {
        popover_frame(ui).show(ui, |ui| {
            ui.set_width(width);
            if header(ui, title) {
                pick = Some(usize::MAX);
            }
            Frame::NONE.inner_margin(Margin::same(16)).show(ui, body);
            let r = ui.max_rect();
            ui.painter().hline(r.x_range(), ui.cursor().min.y, Stroke::new(1.0, crate::appearance::border(ui.ctx())));
            ui.horizontal(|ui| {
                ui.set_min_height(48.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(16.0);
                    // Rightmost first: the primary action sits at the far right.
                    for (i, (label, primary)) in buttons.iter().enumerate().rev() {
                        let b = if *primary {
                            egui::Button::new(RichText::new(*label).color(Color32::WHITE)).fill(crate::canvas::BLUE)
                        } else {
                            egui::Button::new(*label)
                        };
                        if ui.add(b.min_size(vec2(0.0, 26.0)).corner_radius(6)).clicked() {
                            pick = Some(i);
                        }
                    }
                });
            });
        });
    });
    if pick.is_none() && m.should_close() {
        pick = Some(usize::MAX);
    }
    pick
}

/// Shortcut text for this platform. Labels are written with macOS glyphs ("⌥⌘K"); off macOS
/// they read "Ctrl+Alt+K", and the few chords Figma binds to the Control key on a Mac use
/// their Windows and Linux equivalents.
pub fn keys(s: &str) -> String {
    if cfg!(target_os = "macos") { s.to_string() } else { keys_pc(s) }
}

fn keys_pc(s: &str) -> String {
    const MODS: [char; 4] = ['⌃', '⌥', '⇧', '⌘'];
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if !MODS.contains(&c) {
            match c {
                '⌫' => out.push_str("Backspace"),
                '↵' => out.push_str("Enter"),
                _ => out.push(c),
            }
            i += 1;
            continue;
        }
        // A chord: modifiers (maybe space-separated), then the key up to a space, comma or paren.
        let mut mods = vec![];
        while i < chars.len() && (MODS.contains(&chars[i]) || (chars[i] == ' ' && chars.get(i + 1).is_some_and(|n| MODS.contains(n)))) {
            if chars[i] != ' ' {
                mods.push(chars[i]);
            }
            i += 1;
        }
        while chars.get(i) == Some(&' ') {
            i += 1;
        }
        let start = i;
        while i < chars.len() && (i == start || ![' ', ',', ')'].contains(&chars[i])) {
            i += 1;
        }
        let key: String = chars[start..i].iter().collect();
        let key = match key.as_str() {
            "⌫" => "Backspace".to_string(),
            "↵" => "Enter".to_string(),
            _ => key,
        };
        let has = |m: char| mods.contains(&m);
        let special = match (has('⌃'), has('⌥'), has('⌘'), key.as_str()) {
            (true, false, false, "G") => Some("Ctrl+Shift+4".to_string()),
            (true, false, true, "M") => Some("Ctrl+Alt+M".to_string()),
            (true, true, false, "H" | "V" | "T") => Some(format!("Alt+Shift+{key}")),
            _ => None,
        };
        out.push_str(&special.unwrap_or_else(|| {
            let mut parts = vec![];
            if has('⌃') || has('⌘') {
                parts.push("Ctrl");
            }
            if has('⌥') {
                parts.push("Alt");
            }
            if has('⇧') {
                parts.push("Shift");
            }
            parts.push(&key);
            parts.join("+")
        }));
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn shortcut_labels_off_macos() {
        let k = super::keys_pc;
        assert_eq!(k("⌥⇧⌘K"), "Ctrl+Alt+Shift+K");
        assert_eq!(k("⇧⌘S"), "Ctrl+Shift+S");
        assert_eq!(k("⌃G"), "Ctrl+Shift+4");
        assert_eq!(k("⌃⌘M"), "Ctrl+Alt+M");
        assert_eq!(k("Distribute horizontal spacing (⌃⌥H)"), "Distribute horizontal spacing (Alt+Shift+H)");
        assert_eq!(k("⌘ /  or  ⌘ K"), "Ctrl+/  or  Ctrl+K");
        assert_eq!(k("⌘ E, ⌃ ⌘ M"), "Ctrl+E, Ctrl+Alt+M");
        assert_eq!(k("Tab, ⇧ Tab, ⇧ ↵"), "Tab, Shift+Tab, Shift+Enter");
        assert_eq!(k("⌃ Tab, ⌃ ⇧ Tab, ⌘ W"), "Ctrl+Tab, Ctrl+Shift+Tab, Ctrl+W");
        assert_eq!(k("Section  ⇧S"), "Section  Shift+S");
        assert_eq!(k("⌘+"), "Ctrl++");
        assert_eq!(k("V, K, H"), "V, K, H");
    }
}
