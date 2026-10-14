//! Preferences and the Settings modal (Cmd ,): one large tabbed window, as in Figma, holding
//! everything that used to be spread over separate windows and menus.

use crate::app::App;
use egui::{Color32, RichText, Sense, Stroke, Ui, vec2};
use serde::{Deserialize, Serialize};

/// Device-local preferences, saved in the data directory.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// Shown on comments and to collaborators; empty = the system user name.
    pub name: String,
    pub snap_to_objects: bool,
    pub highlight_on_hover: bool,
    pub keep_tool_selected: bool,
    pub show_dimensions: bool,
    pub nudge: f32,
    pub big_nudge: f32,
    pub check_updates: bool,
    /// Linux: the desktop's own title bar above the tabs instead of tabs in the title bar.
    pub system_title_bar: bool,
    /// Shift ': the pixel grid at 800% zoom and above.
    pub pixel_grid: bool,
    /// Shift Cmd ': positions and sizes round to whole pixels.
    pub snap_to_pixels: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs { name: String::new(), snap_to_objects: true, highlight_on_hover: true, keep_tool_selected: false, show_dimensions: true, nudge: 1.0, big_nudge: 10.0, check_updates: true, system_title_bar: false, pixel_grid: true, snap_to_pixels: true }
    }
}

fn path() -> std::path::PathBuf {
    crate::files::data_dir().join("preferences.json")
}

pub fn load() -> Prefs {
    std::fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn save(p: &Prefs) {
    // Tests share one data folder and run in parallel: keep each one's preferences to itself.
    if cfg!(test) {
        return;
    }
    let _ = crate::files::write_atomic(&path(), serde_json::to_string_pretty(p).unwrap_or_default().as_bytes());
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Account,
    Appearance,
    Canvas,
    Plugins,
    Agents,
    Updates,
    Shortcuts,
}

const TABS: [(Tab, &str); 7] = [
    (Tab::Account, "Account"),
    (Tab::Appearance, "Appearance"),
    (Tab::Canvas, "Canvas"),
    (Tab::Plugins, "Plugins"),
    (Tab::Agents, "AI agents"),
    (Tab::Updates, "Updates"),
    (Tab::Shortcuts, "Shortcuts"),
];

const SHORTCUTS: &[(&str, &str)] = &[
    ("Quick actions", "⌘ /  or  ⌘ K"),
    ("Move, Scale, Hand", "V, K, H"),
    ("Frame, Section, Slice", "F or A, ⇧ S, S"),
    ("Rectangle, Line, Arrow, Ellipse", "R, L, ⇧ L, O"),
    ("Pen, Pencil, Text, Comment", "P, ⇧ P, T, C"),
    ("Undo, redo", "⌘ Z, ⇧ ⌘ Z"),
    ("Copy, paste, cut, duplicate", "⌘ C, ⌘ V, ⌘ X, ⌘ D"),
    ("Copy and paste properties", "⌥ ⌘ C, ⌥ ⌘ V"),
    ("Group, frame selection, ungroup", "⌘ G, ⌥ ⌘ G, ⇧ ⌘ G"),
    ("Auto layout, remove auto layout", "⇧ A, ⌥ ⇧ A"),
    ("Create component, detach instance", "⌥ ⌘ K, ⌥ ⌘ B"),
    ("Union, subtract, intersect, exclude", "⌥ ⇧ U, S, I, E"),
    ("Flatten, use as mask", "⌘ E, ⌃ ⌘ M"),
    ("Bring forward, send backward", "⌘ ], ⌘ ["),
    ("Bring to front, send to back", "⌥ ⌘ ], ⌥ ⌘ ["),
    ("Flip horizontal, vertical", "⇧ H, ⇧ V"),
    ("Distribute, tidy up", "⌃ ⌥ H / V, ⌃ ⌥ T"),
    ("Show or hide, lock", "⇧ ⌘ H, ⇧ ⌘ L"),
    ("Next and previous layer, select parent", "Tab, ⇧ Tab, ⇧ ↵"),
    ("Zoom to fit, 100%, selection", "⇧ 1, ⇧ 0, ⇧ 2"),
    ("Zoom tool, next and previous frame", "Z, N, ⇧ N"),
    ("Rulers, layout guides, hide UI, outline", "⇧ R, ⌃ G, ⌘ \\, ⌘ Y"),
    ("Pixel grid, snap to pixel grid", "⇧ ', ⇧ ⌘ '"),
    ("Dev Mode, present", "⇧ D, ⌥ ⌘ ↵"),
    ("Save, open, place image, export", "⌘ S, ⌘ O, ⇧ ⌘ K, ⇧ ⌘ E"),
    ("Find and replace", "⌘ F"),
    ("Next and previous tab, close tab", "⌃ Tab, ⌃ ⇧ Tab, ⌘ W"),
    ("Settings", "⌘ ,"),
];

/// A section heading inside a settings tab.
fn section(ui: &mut Ui, title: &str) {
    ui.add_space(18.0);
    ui.label(RichText::new(title).size(14.0));
    ui.add_space(6.0);
}

/// A row: label (and an optional hint under it) on the left, the control on the right.
fn row(ui: &mut Ui, label: &str, hint: &str, control: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.set_min_height(30.0);
        ui.vertical(|ui| {
            ui.label(label);
            if !hint.is_empty() {
                ui.label(RichText::new(hint).small().color(crate::appearance::muted(ui.ctx())));
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), control);
    });
}

/// An on/off switch, as Figma's settings use.
pub fn toggle(ui: &mut Ui, on: &mut bool) -> egui::Response {
    let (r, mut resp) = ui.allocate_exact_size(vec2(32.0, 16.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, *on, ""));
    let t = ui.ctx().animate_bool_responsive(resp.id, *on);
    let off = if ui.visuals().dark_mode { Color32::from_gray(0x55) } else { Color32::from_gray(0xC8) };
    let bg = crate::canvas::BLUE.lerp_to_gamma(off, 1.0 - t);
    ui.painter().rect_filled(r, 8.0, bg);
    let c = egui::pos2(egui::lerp(r.min.x + 8.0..=r.max.x - 8.0, t), r.center().y);
    ui.painter().circle_filled(c, 6.0, Color32::WHITE);
    resp
}

impl App {
    pub fn open_settings(&mut self, tab: Tab) {
        self.settings_tab = Some(tab);
    }

    /// Flip a canvas preference from a shortcut or menu, and remember it.
    pub fn toggle_pref(&mut self, f: fn(&mut Prefs) -> &mut bool) {
        let on = f(&mut self.prefs);
        *on = !*on;
        save(&self.prefs);
    }

    /// The Settings modal. Returns whether appearance changed (so the caller re-applies it).
    pub fn settings_window(&mut self, ctx: &egui::Context) {
        let Some(mut tab) = self.settings_tab else { return };
        let before = (self.appearance.clone(), self.prefs.clone());
        let mut close = false;
        let screen = ctx.content_rect();
        // Figma's Settings: about 650 by 496, tabs in the header, over a 50% black backdrop.
        let size = vec2(650.0_f32.min(screen.width() - 40.0), 496.0_f32.min(screen.height() - 60.0));
        let modal = egui::Modal::new(egui::Id::new("settings")).frame(egui::Frame::NONE).backdrop_color(Color32::from_black_alpha(128)).show(ctx, |ui| {
            crate::chrome::popover_frame(ui).show(ui, |ui| {
                ui.set_width(size.x);
                ui.set_height(size.y);
                ui.spacing_mut().item_spacing.y = 4.0;
                // The header is the tab row (no title), with the close button at the right.
                ui.horizontal(|ui| {
                    ui.set_min_height(40.0);
                    ui.add_space(8.0);
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for (t, label) in TABS {
                        let on = tab == t;
                        let text = RichText::new(label).size(12.0);
                        let text = if on { text.strong() } else { text.color(crate::appearance::muted(ui.ctx())) };
                        // The selected tab is a gray capsule, as in Figma.
                        let fill = if on { crate::appearance::subtle(ui.ctx()) } else { Color32::TRANSPARENT };
                        let resp = ui.add(egui::Button::new(text).fill(fill).stroke(Stroke::NONE).corner_radius(6).min_size(vec2(0.0, 26.0)));
                        resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, on, label));
                        if resp.clicked() {
                            tab = t;
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(8.0);
                        close |= crate::chrome::close_button(ui).clicked();
                    });
                });
                let r = ui.max_rect();
                ui.painter().hline(r.x_range(), ui.cursor().min.y, Stroke::new(1.0, crate::appearance::border(ui.ctx())));
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    egui::Frame::NONE.inner_margin(egui::Margin { left: 32, right: 32, top: 8, bottom: 24 }).show(ui, |ui| {
                        ui.set_width(size.x - 64.0);
                        match tab {
                            Tab::Account => self.account_tab(ui),
                            Tab::Appearance => {
                                appearance_tab(ui, &mut self.appearance);
                                if cfg!(target_os = "linux") {
                                    section(ui, "Window");
                                    row(ui, "Use the system title bar", "Your desktop's title bar above the tabs, instead of tabs in the title bar", |ui| {
                                        toggle(ui, &mut self.prefs.system_title_bar);
                                    });
                                    if self.prefs.system_title_bar != crate::shell::uses_system_title_bar() {
                                        ui.label(RichText::new("Restart Katachi to switch title bars.").color(crate::appearance::muted(ui.ctx())));
                                    }
                                }
                            }
                            Tab::Canvas => canvas_tab(ui, &mut self.prefs),
                            Tab::Plugins => self.plugins_tab(ui),
                            Tab::Agents => self.agents_tab(ui),
                            Tab::Updates => self.updates_tab(ui),
                            Tab::Shortcuts => shortcuts_tab(ui),
                        }
                    });
                });
            });
        });
        if modal.should_close() || close {
            self.settings_tab = None;
        } else if self.settings_tab.is_some() {
            self.settings_tab = Some(tab);
        }
        if self.appearance != before.0 {
            crate::appearance::apply(ctx, &self.appearance);
            crate::appearance::save(&self.appearance);
        }
        if self.prefs != before.1 {
            save(&self.prefs);
        }
    }

    fn account_tab(&mut self, ui: &mut Ui) {
        section(ui, "Name");
        let mut name = self.prefs.name.clone();
        row(ui, "Your name", "Shown on your comments and to collaborators", |ui| {
            ui.add(egui::TextEdit::singleline(&mut name).hint_text(crate::comments::system_name()).desired_width(200.0));
        });
        if name != self.prefs.name {
            self.prefs.name = name;
            self.collab_name = crate::comments::author_from(&self.prefs);
        }
        section(ui, "Theme");
        theme_row(ui, &mut self.appearance);
        section(ui, "Files");
        row(ui, "Data folder", "Autosaves, recent files, thumbnails and plugins", |ui| {
            if ui.button(if cfg!(target_os = "macos") { "Show in Finder" } else { "Show in folder" }).clicked() {
                crate::plugin_view::open_url(&crate::files::data_dir().display().to_string());
            }
        });
    }

    fn plugins_tab(&mut self, ui: &mut Ui) {
        section(ui, "Installed plugins");
        let installed = crate::plugins::installed();
        if installed.is_empty() {
            ui.label(RichText::new("No plugins yet. Import one from its manifest.json to get started.").color(crate::appearance::muted(ui.ctx())));
        }
        for p in installed {
            row(ui, &p.name, &p.manifest.display().to_string(), |ui| {
                if ui.button("Remove").clicked() {
                    crate::plugins::uninstall(&p.manifest);
                }
                if ui.button("Run").clicked() {
                    match crate::plugins::load(&p) {
                        Ok((code, html, id)) => {
                            self.settings_tab = None;
                            self.run_plugin(&p.name, Some(&id), &code, html.as_deref());
                        }
                        Err(e) => self.toast(format!("{}: {e}", p.name)),
                    }
                }
            });
        }
        section(ui, "Development");
        row(ui, "Import plugin from manifest", "A Figma-style manifest.json", |ui| {
            if ui.button("Import…").clicked()
                && let Some(f) = rfd::FileDialog::new().add_filter("Plugin manifest", &["json"]).pick_file()
            {
                match crate::plugins::install(&f) {
                    Ok(p) => self.toast(format!("Installed {}", p.name)),
                    Err(e) => self.toast(format!("Couldn't import plugin: {e}")),
                }
            }
        });
        row(ui, "Run a script", "Plugin API JavaScript against this file", |ui| {
            if ui.button("Open console").clicked() {
                self.settings_tab = None;
                self.show_console = true;
            }
        });
    }

    fn agents_tab(&mut self, ui: &mut Ui) {
        let port = crate::mcp::port();
        section(ui, "MCP server");
        let listening = self.mcp_listening();
        row(ui, "Status", if listening { "Agents can read and edit the file on screen" } else { "Another Katachi window is using the port" }, |ui| {
            let (c, t) = if listening { (Color32::from_rgb(0x14, 0xAE, 0x5C), format!("Listening on 127.0.0.1:{port}")) } else { (Color32::from_rgb(0xE0, 0x4E, 0x3A), "Not running".into()) };
            ui.label(RichText::new(t).color(c));
        });
        section(ui, "Connect an agent");
        row(ui, "Claude Code", "Adds Katachi as an HTTP MCP server", |ui| {
            if ui.button("Copy command").clicked() {
                ui.ctx().copy_text(format!("claude mcp add --transport http katachi http://127.0.0.1:{port}/mcp"));
                self.toast("Copied. Run it in a terminal.".into());
            }
        });
        row(ui, "Claude Desktop, Cursor and others", "A stdio config that runs `katachi mcp`", |ui| {
            if ui.button("Copy config").clicked() {
                let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "katachi".into());
                let cfg = serde_json::json!({ "mcpServers": { "katachi": { "command": exe, "args": ["mcp"] } } });
                ui.ctx().copy_text(serde_json::to_string_pretty(&cfg).unwrap_or_default());
                self.toast("Copied MCP config".into());
            }
        });
    }

    fn updates_tab(&mut self, ui: &mut Ui) {
        section(ui, "Katachi");
        row(ui, "Version", "", |ui| {
            ui.label(env!("CARGO_PKG_VERSION"));
        });
        if let Some(why) = crate::update::managed() {
            row(ui, "Updates", why, |_| {});
        } else {
            row(ui, "Check for updates automatically", "When one is out, Update now appears at the top left", |ui| {
                toggle(ui, &mut self.prefs.check_updates);
            });
        }
        row(ui, "Release notes", "What changed in each version", |ui| {
            if ui.button("Open").clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab("https://github.com/dyascj/katachi/releases"));
            }
        });
        row(ui, "Source", "Katachi is free and open source (MIT)", |ui| {
            if ui.button("GitHub").clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab("https://github.com/dyascj/katachi"));
            }
        });
    }
}

fn theme_row(ui: &mut Ui, a: &mut crate::appearance::Appearance) {
    use crate::appearance::ThemePref;
    row(ui, "Theme", "", |ui| {
        let label = |t: ThemePref| match t {
            ThemePref::Light => "Light",
            ThemePref::Dark => "Dark",
            ThemePref::System => "System theme",
        };
        egui::ComboBox::from_id_salt("settings-theme").selected_text(label(a.theme)).width(150.0).popup_style(crate::chrome::menu_style.into()).show_ui(ui, |ui| {
            for t in [ThemePref::System, ThemePref::Light, ThemePref::Dark] {
                ui.selectable_value(&mut a.theme, t, label(t));
            }
        });
    });
}

fn appearance_tab(ui: &mut Ui, a: &mut crate::appearance::Appearance) {
    section(ui, "Theme");
    theme_row(ui, a);
    section(ui, "Glass");
    row(ui, "Glass panels", "Translucent side panels over a blurred desktop. The canvas and popovers stay solid.", |ui| {
        toggle(ui, &mut a.glass);
    });
    if a.glass && !a.glass_on() {
        ui.label(RichText::new("Restart Katachi to turn glass on.").color(crate::appearance::muted(ui.ctx())));
    }
    ui.add_enabled_ui(a.glass, |ui| {
        let mut pct = (a.panel_opacity * 100.0).round();
        row(ui, "Panel opacity", "", |ui| {
            if ui.add(egui::Slider::new(&mut pct, 15.0..=100.0).suffix("%")).changed() {
                a.panel_opacity = pct / 100.0;
            }
        });
        if cfg!(target_os = "macos") {
            row(ui, "Desktop blur", "", |ui| {
                ui.add(egui::Slider::new(&mut a.blur, 1..=64));
            });
        }
        row(ui, "Tint hue", "", |ui| {
            ui.add(egui::Slider::new(&mut a.hue, 0.0..=360.0).suffix("°"));
        });
        let mut sat = (a.saturation * 100.0).round();
        row(ui, "Tint strength", "", |ui| {
            if ui.add(egui::Slider::new(&mut sat, 0.0..=100.0).suffix("%")).changed() {
                a.saturation = sat / 100.0;
            }
        });
    });
    ui.add_space(10.0);
    if ui.button("Reset appearance").clicked() {
        *a = crate::appearance::Appearance::default();
    }
}

fn canvas_tab(ui: &mut Ui, p: &mut Prefs) {
    section(ui, "Selection");
    row(ui, "Highlight layers on hover", "Outline the layer under the pointer", |ui| {
        toggle(ui, &mut p.highlight_on_hover);
    });
    row(ui, "Show dimensions on objects", "The size label under a selection", |ui| {
        toggle(ui, &mut p.show_dimensions);
    });
    section(ui, "Tools");
    row(ui, "Keep tool selected after use", "Draw several shapes in a row", |ui| {
        toggle(ui, &mut p.keep_tool_selected);
    });
    row(ui, "Snap to objects", "Smart guides to other layers' edges and centers", |ui| {
        toggle(ui, &mut p.snap_to_objects);
    });
    row(ui, "Snap to pixel grid", "Positions and sizes land on whole pixels", |ui| {
        toggle(ui, &mut p.snap_to_pixels);
    });
    row(ui, "Pixel grid", "Show pixel boundaries at 800% zoom and above", |ui| {
        toggle(ui, &mut p.pixel_grid);
    });
    section(ui, "Nudge amount");
    row(ui, "Small nudge", "Arrow keys", |ui| {
        ui.add(egui::DragValue::new(&mut p.nudge).range(0.01..=100.0).speed(0.1));
    });
    row(ui, "Big nudge", "Shift and arrow keys", |ui| {
        ui.add(egui::DragValue::new(&mut p.big_nudge).range(0.1..=1000.0).speed(0.5));
    });
}

fn shortcuts_tab(ui: &mut Ui) {
    section(ui, "Keyboard shortcuts");
    let muted = crate::appearance::muted(ui.ctx());
    for (what, keys) in SHORTCUTS {
        row(ui, what, "", |ui| {
            ui.label(RichText::new(crate::chrome::keys(keys)).color(muted));
        });
    }
}
