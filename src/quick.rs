//! Quick actions (Cmd / or Cmd K), as in Figma: type to find any command, tool, plugin or layer,
//! arrow to pick, Enter to run.

use crate::app::{App, Tool};
use crate::doc::{BoolOp, NodeId};
use egui::{Key, RichText};

type Run = fn(&mut App, &egui::Context);

/// Commands: label, shortcut, action.
const COMMANDS: &[(&str, &str, Run)] = &[
    ("Move tool", "V", |a, _| a.set_tool(Tool::Move)),
    ("Frame tool", "F", |a, _| a.set_tool(Tool::Frame)),
    ("Section tool", "⇧S", |a, _| a.set_tool(Tool::Section)),
    ("Rectangle tool", "R", |a, _| a.set_tool(Tool::Rectangle)),
    ("Ellipse tool", "O", |a, _| a.set_tool(Tool::Ellipse)),
    ("Line tool", "L", |a, _| a.set_tool(Tool::Line)),
    ("Pen tool", "P", |a, _| a.set_tool(Tool::Pen)),
    ("Pencil tool", "⇧P", |a, _| a.set_tool(Tool::Pencil)),
    ("Text tool", "T", |a, _| a.set_tool(Tool::Text)),
    ("Comment tool", "C", |a, _| a.set_tool(Tool::Comment)),
    ("Hand tool", "H", |a, _| a.set_tool(Tool::Hand)),
    ("Zoom to fit", "⇧1", |a, c| a.zoom_to(c, None, false)),
    ("Zoom to 100%", "⇧0", |a, c| a.zoom_to(c, None, true)),
    ("Zoom to selection", "⇧2", |a, c| {
        let s = a.sel.clone();
        if !s.is_empty() {
            a.zoom_to(c, Some(s), false)
        }
    }),
    ("Add auto layout", "⇧A", |a, _| a.add_auto_layout()),
    ("Remove auto layout", "⌥⇧A", |a, _| a.remove_auto_layout()),
    ("Create component", "⌥⌘K", |a, _| a.create_component()),
    ("Combine as variants", "", |a, _| a.combine_as_variants()),
    ("Add variant", "", |a, _| {
        if let Some(&c) = a.sel.first() {
            a.add_variant_to(c)
        }
    }),
    ("Detach instance", "⌥⌘B", |a, _| a.detach_selection()),
    ("Group selection", "⌘G", |a, _| a.group_selection(false)),
    ("Frame selection", "⌥⌘G", |a, _| a.group_selection(true)),
    ("Ungroup", "⇧⌘G", |a, _| a.ungroup_selection()),
    ("Union selection", "⌥⇧U", |a, _| a.boolean_selection(BoolOp::Union)),
    ("Subtract selection", "⌥⇧S", |a, _| a.boolean_selection(BoolOp::Subtract)),
    ("Intersect selection", "⌥⇧I", |a, _| a.boolean_selection(BoolOp::Intersect)),
    ("Exclude selection", "⌥⇧E", |a, _| a.boolean_selection(BoolOp::Exclude)),
    ("Flatten", "⌘E", |a, _| a.flatten_selection()),
    ("Outline stroke", "⌥⌘O", |a, _| a.outline_stroke_selection()),
    ("Use as mask", "⌃⌘M", |a, _| a.toggle_mask()),
    ("Duplicate", "⌘D", |a, _| a.duplicate_selection()),
    ("Delete", "⌫", |a, _| a.delete_selection()),
    ("Select all", "⌘A", |a, _| a.select_all()),
    ("Select matching layers", "⌥⌘A", |a, _| a.select_matching()),
    ("Show/Hide selection", "⇧⌘H", |a, _| a.toggle_visible()),
    ("Lock/Unlock selection", "⇧⌘L", |a, _| a.toggle_locked()),
    ("Rulers", "⇧R", |a, _| a.rulers = !a.rulers),
    ("Remove all guides", "", |a, _| a.remove_all_guides()),
    ("Outline mode", "⌘Y", |a, _| a.outline_mode = !a.outline_mode),
    ("Layout guides", "⌃G", |a, _| a.show_grids = !a.show_grids),
    ("Hide UI", "⌘\\", |a, _| a.ui_hidden = !a.ui_hidden),
    ("Dev Mode", "⇧D", |a, _| a.dev_mode = !a.dev_mode),
    ("Present", "⌥⌘↵", |a, c| a.present(c.input(|i| i.time))),
    ("Comments", "⇧C", |a, _| a.show_comments = !a.show_comments),
    ("Local variables", "", |a, _| a.show_variables = true),
    ("Find and replace", "⌘F", |a, _| {
        a.find.0 = true;
        a.find.3 = true;
    }),
    ("Version history", "", |a, _| a.show_history = true),
    ("Settings", "⌘,", |a, _| a.open_settings(crate::settings::Tab::Account)),
    ("Appearance (theme and glass)", "", |a, _| a.open_settings(crate::settings::Tab::Appearance)),
    ("Keyboard shortcuts", "", |a, _| a.open_settings(crate::settings::Tab::Shortcuts)),
    ("Manage plugins", "", |a, _| a.open_settings(crate::settings::Tab::Plugins)),
    ("Run script (plugin console)", "", |a, _| a.show_console = true),
    ("Export selection", "⇧⌘E", |a, c| a.export(c)),
    ("Place image", "⇧⌘K", |a, _| a.import_dialog()),
    ("Save", "⌘S", |a, _| {
        a.save(false);
    }),
    ("Save as", "⇧⌘S", |a, _| {
        a.save(true);
    }),
    ("New design file", "⌘N", |a, _| a.request_new()),
    ("Open file", "⌘O", |a, _| a.request_open()),
    ("Back to files", "", |a, _| a.want = Some(crate::app::Want::Home)),
];

/// What a result does.
#[derive(Clone)]
enum Hit {
    Command(usize),
    Plugin(crate::plugins::Installed),
    Layer(NodeId, usize),
}

/// Every word of the query appears in the text (case-insensitive).
fn matches(text: &str, q: &str) -> bool {
    let t = text.to_lowercase();
    q.split_whitespace().all(|w| t.contains(&w.to_lowercase()))
}

impl App {
    pub fn toggle_quick_actions(&mut self) {
        self.quick = if self.quick.is_some() { None } else { Some((String::new(), 0)) };
    }

    pub fn quick_actions(&mut self, ctx: &egui::Context) {
        let Some((mut q, mut pick)) = self.quick.take() else { return };
        // Tabs, as in Figma: All, Layers, Plugins (0, 1, 2).
        let tab_id = egui::Id::new("quick-tab");
        let mut tab: u8 = ctx.data(|d| d.get_temp(tab_id)).unwrap_or(0);
        // Results: commands, then plugins, then layers by name (up to 10).
        let mut hits: Vec<(Hit, String, String)> = vec![];
        if tab == 0 {
            for (i, (label, short, _)) in COMMANDS.iter().enumerate() {
                if matches(label, &q) {
                    hits.push((Hit::Command(i), label.to_string(), crate::chrome::keys(short)));
                }
            }
        }
        if tab != 1 {
            for p in crate::plugins::installed() {
                if matches(&p.name, &q) {
                    hits.push((Hit::Plugin(p.clone()), format!("Run {}", p.name), "Plugin".into()));
                }
            }
        }
        if tab != 2 && (!q.trim().is_empty() || tab == 1) {
            for (pi, page) in self.doc.pages.iter().enumerate() {
                let mut stack = page.children.clone();
                while let Some(id) = stack.pop() {
                    let n = self.doc.node(id);
                    if matches(&n.name, &q) {
                        hits.push((Hit::Layer(id, pi), n.name.clone(), format!("Layer on {}", page.name)));
                    }
                    stack.extend(n.children.iter().copied());
                }
            }
        }
        hits.truncate(10);
        pick = pick.min(hits.len().saturating_sub(1));
        let (up, down, enter, esc) = ctx.input_mut(|i| {
            (i.consume_key(egui::Modifiers::NONE, Key::ArrowUp), i.consume_key(egui::Modifiers::NONE, Key::ArrowDown), i.consume_key(egui::Modifiers::NONE, Key::Enter), i.consume_key(egui::Modifiers::NONE, Key::Escape))
        });
        if down {
            pick = (pick + 1).min(hits.len().saturating_sub(1));
        }
        if up {
            pick = pick.saturating_sub(1);
        }
        let mut run = enter.then(|| hits.get(pick).cloned()).flatten().map(|h| h.0);
        let screen = ctx.content_rect();
        egui::Area::new(egui::Id::new("quick-actions"))
            .fixed_pos(egui::pos2(screen.center().x - 256.0, screen.min.y + 140.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                // Figma's Actions window: 512 wide, search on top, tabs, 32-point rows, no title.
                crate::chrome::popover_frame(ui).inner_margin(8).show(ui, |ui| {
                    ui.set_width(496.0);
                    let r = ui.add(egui::TextEdit::singleline(&mut q).hint_text("Search actions, layers and plugins").desired_width(f32::INFINITY).font(egui::FontId::proportional(14.0)).margin(egui::vec2(10.0, 8.0)));
                    if !r.has_focus() {
                        r.request_focus();
                    }
                    if r.changed() {
                        pick = 0;
                    }
                    ui.horizontal(|ui| {
                        for (i, label) in ["All", "Layers", "Plugins"].into_iter().enumerate() {
                            let on = tab == i as u8;
                            let fill = if on { crate::appearance::subtle(ui.ctx()) } else { egui::Color32::TRANSPARENT };
                            if ui.add(egui::Button::new(RichText::new(label).size(12.0)).fill(fill).stroke(egui::Stroke::NONE).corner_radius(6)).clicked() {
                                tab = i as u8;
                                pick = 0;
                            }
                        }
                    });
                    ui.add_space(2.0);
                    // Rows sit flush at 32 points, as in Figma.
                    ui.spacing_mut().item_spacing.y = 0.0;
                    if hits.is_empty() {
                        ui.label(RichText::new("No matches").color(crate::appearance::muted(ui.ctx())));
                    }
                    for (i, (hit, label, detail)) in hits.iter().enumerate() {
                        let resp = ui.add_sized([ui.available_width(), 32.0], egui::Button::selectable(i == pick, "").frame(false));
                        let p = ui.painter();
                        p.text(resp.rect.left_center() + egui::vec2(8.0, 0.0), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(13.5), ui.visuals().text_color());
                        p.text(resp.rect.right_center() - egui::vec2(8.0, 0.0), egui::Align2::RIGHT_CENTER, detail, egui::FontId::proportional(12.0), crate::appearance::muted(ui.ctx()));
                        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
                        if resp.clicked() {
                            run = Some(hit.clone());
                        }
                        if resp.hovered() && ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
                            pick = i;
                        }
                    }
                });
            });
        ctx.data_mut(|d| d.insert_temp(tab_id, tab));
        if esc {
            ctx.data_mut(|d| d.insert_temp(tab_id, 0u8));
            return;
        }
        match run {
            Some(Hit::Command(i)) => (COMMANDS[i].2)(self, ctx),
            Some(Hit::Plugin(p)) => match crate::plugins::load(&p) {
                Ok((code, html, id)) => self.run_plugin(&p.name, Some(&id), &code, html.as_deref()),
                Err(e) => self.toast(format!("Couldn't run {}: {e}", p.name)),
            },
            Some(Hit::Layer(id, page)) => {
                self.select_page(page);
                self.sel = vec![id];
                self.zoom_target = Some(id);
            }
            None => self.quick = Some((q, pick)),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_word_must_match() {
        assert!(super::matches("Add auto layout", "auto add"));
        assert!(!super::matches("Add auto layout", "auto grid"));
        assert!(super::matches("Anything", ""));
    }
}
