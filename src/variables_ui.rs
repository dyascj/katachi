//! Local variables window, property bindings, and variable modes.

use crate::app::App;
use crate::canvas::num;
use crate::doc::{Bindable, Document, Kind, NodeId, Rgba, VarId, VarValue};
use crate::panels::{Icon, icon_button};
use egui::{DragValue, RichText, Ui};
use std::mem::discriminant;

fn type_name(v: &VarValue) -> &'static str {
    match v {
        VarValue::Color(_) => "Color",
        VarValue::Number(_) => "Number",
        VarValue::Text(_) => "String",
        VarValue::Bool(_) => "Boolean",
        VarValue::Alias(_) => "Alias",
    }
}

/// How a Figma scope reads in "Show in", as in Figma's variable editor.
fn scope_label(scope: &str) -> &'static str {
    match scope {
        "ALL_FILLS" => "Fill",
        "STROKE_COLOR" => "Stroke",
        "EFFECT_COLOR" => "Effects",
        "OPACITY" => "Layer opacity",
        "CORNER_RADIUS" => "Corner radius",
        "WIDTH_HEIGHT" => "Width and height",
        "GAP" => "Gap",
        "STROKE_FLOAT" => "Stroke",
        "EFFECT_FLOAT" => "Effects",
        "FONT_SIZE" => "Font size",
        "LINE_HEIGHT" => "Line height",
        "LETTER_SPACING" => "Letter spacing",
        "PARAGRAPH_SPACING" => "Paragraph spacing",
        "FONT_WEIGHT" => "Font weight",
        "FONT_FAMILY" => "Font family",
        "TEXT_CONTENT" => "Text content",
        _ => "",
    }
}

/// Variables whose values could stand in for `v`'s: same type, and not aliasing back to it.
fn alias_targets(doc: &Document, v: VarId) -> Vec<(VarId, String)> {
    let Some(kind) = doc.resolve_first(v).map(|x| discriminant(&x)) else { return vec![] };
    doc.variables
        .iter()
        .filter(|o| o.id != v && !doc.aliases_to(o.id, v) && doc.resolve_first(o.id).is_some_and(|x| discriminant(&x) == kind))
        .map(|o| (o.id, o.name.clone()))
        .collect()
}

/// Edit one variable value in place. A value can also point at another variable of the same
/// type (Figma's alias), picked from the hexagon menu. Returns whether it changed.
fn value_editor(ui: &mut Ui, id: egui::Id, v: &mut VarValue, targets: &[(VarId, String)], alias: Option<(String, Option<VarValue>)>) -> bool {
    let mut changed = match v {
        VarValue::Color(c) => {
            let mut rgb = [c.0[0], c.0[1], c.0[2]];
            let mut changed = egui::color_picker::color_edit_button_srgb(ui, &mut rgb).changed();
            if changed {
                *c = Rgba([rgb[0], rgb[1], rgb[2], c.0[3]]);
            }
            changed |= crate::paint_ui::hex_field(ui, id, c, 64.0);
            changed
        }
        VarValue::Number(n) => ui.add(DragValue::new(n).speed(0.5).custom_formatter(|x, _| num(x as f32))).changed(),
        VarValue::Text(t) => ui.add(egui::TextEdit::singleline(t).desired_width(90.0)).changed(),
        VarValue::Bool(b) => ui.checkbox(b, "").changed(),
        VarValue::Alias(_) => {
            let (name, resolved) = alias.unwrap_or(("Missing".into(), None));
            let btn = ui.add(egui::Button::new(RichText::new(name).color(crate::canvas::BLUE)).frame(false));
            let mut detach = false;
            egui::Popup::menu(&btn).id(id.with("alias")).style(crate::chrome::menu_style).show(|ui| {
                if ui.button("Detach alias").clicked() {
                    detach = true;
                    ui.close();
                }
            });
            if detach && let Some(r) = resolved {
                *v = r;
                return true;
            }
            false
        }
    };
    if !matches!(v, VarValue::Alias(_)) && !targets.is_empty() {
        let btn = icon_button(ui, Icon::Variable, false, 20.0, "Create alias");
        egui::Popup::menu(&btn).id(id.with("alias-pick")).style(crate::chrome::menu_style).show(|ui| {
            egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
                for (tid, name) in targets {
                    if ui.button(name).clicked() {
                        *v = VarValue::Alias(*tid);
                        changed = true;
                        ui.close();
                    }
                }
            });
        });
    }
    changed
}

/// Split "color/brand/primary" into its group ("color/brand") and leaf name.
fn group_of(name: &str) -> (Option<&str>, &str) {
    match name.rsplit_once('/') {
        Some((g, leaf)) => (Some(g.trim()), leaf.trim()),
        None => (None, name),
    }
}

impl App {
    /// Figma's "Local variables" table: collections, modes as columns, variables as rows, with
    /// slash names grouped under collapsible group rows.
    pub fn variables_window(&mut self, ctx: &egui::Context) {
        if !self.show_variables {
            return;
        }
        let mut open = true;
        let before = (self.doc.collections.clone(), self.doc.variables.clone());
        crate::chrome::window(ctx, "Local variables", &mut open, 760.0, |ui| {
            if self.doc.collections.is_empty() {
                ui.label("Variables store reusable values like colors and spacing, with modes such as Light and Dark.");
                if ui.button("Create collection").clicked() {
                    let id = self.doc.add_collection("Collection".into());
                    self.var_collection = Some(id);
                }
                return;
            }
            let current = self.var_collection.filter(|c| self.doc.collections.iter().any(|x| x.id == *c)).unwrap_or(self.doc.collections[0].id);
            self.var_collection = Some(current);
            // Collections in a left column, the selected one's table on the right, as in Figma.
            ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(160.0);
                ui.label(egui::RichText::new("Collections").small().color(crate::appearance::muted(ui.ctx())));
                for c in &self.doc.collections {
                    if ui.add_sized([160.0, 26.0], egui::Button::selectable(c.id == current, &c.name)).clicked() {
                        self.var_collection = Some(c.id);
                    }
                }
                if ui.add(egui::Button::new("+  New collection").frame(false)).clicked() {
                    let id = self.doc.add_collection(format!("Collection {}", self.doc.collections.len() + 1));
                    self.var_collection = Some(id);
                }
            });
            ui.separator();
            ui.vertical(|ui| {
            let ci = self.doc.collections.iter().position(|c| c.id == current).unwrap();
            ui.horizontal(|ui| {
                ui.label("Collection");
                ui.text_edit_singleline(&mut self.doc.collections[ci].name);
            });
            let modes = self.doc.collections[ci].modes.len();
            // Ungrouped variables first, then each group in the order it first appears.
            let mine: Vec<usize> = (0..self.doc.variables.len()).filter(|&i| self.doc.variables[i].collection == current).collect();
            let mut groups: Vec<Option<String>> = vec![None];
            for &vi in &mine {
                let g = group_of(&self.doc.variables[vi].name).0.map(String::from);
                if !groups.contains(&g) {
                    groups.push(g);
                }
            }
            let collapsed_id = ui.id().with(("var-collapsed", current));
            let mut collapsed: Vec<String> = ui.data(|d| d.get_temp(collapsed_id)).unwrap_or_default();
            egui::ScrollArea::both().max_height(340.0).show(ui, |ui| {
                egui::Grid::new("vars").striped(true).min_col_width(140.0).show(ui, |ui| {
                    ui.label(RichText::new("Name").strong());
                    for m in 0..modes {
                        ui.add(egui::TextEdit::singleline(&mut self.doc.collections[ci].modes[m]).desired_width(90.0));
                    }
                    if ui.small_button("+ Mode").clicked() {
                        let n = self.doc.collections[ci].modes.len() + 1;
                        self.doc.collections[ci].modes.push(format!("Mode {n}"));
                        for v in self.doc.variables.iter_mut().filter(|v| v.collection == current) {
                            let last = v.values.last().cloned().unwrap_or(VarValue::Number(0.0));
                            v.values.push(last);
                        }
                    }
                    ui.end_row();
                    let mut delete = None;
                    for (gi, group) in groups.iter().enumerate() {
                        let rows: Vec<usize> = mine.iter().copied().filter(|&vi| group_of(&self.doc.variables[vi].name).0 == group.as_deref()).collect();
                        if let Some(g) = group {
                            // Group row: collapse toggle and the group's name; renaming moves every variable in it.
                            let shut = collapsed.contains(g);
                            let mut name = g.clone();
                            ui.horizontal(|ui| {
                                if icon_button(ui, Icon::Chevron(!shut), false, 18.0, if shut { "Expand group" } else { "Collapse group" }).clicked() {
                                    if shut { collapsed.retain(|x| x != g) } else { collapsed.push(g.clone()) }
                                }
                                ui.add(egui::TextEdit::singleline(&mut name).id(ui.id().with(("var-group", current, gi))).desired_width(130.0).font(egui::TextStyle::Body));
                            });
                            ui.end_row();
                            if name != *g {
                                let name = name.trim().trim_matches('/').to_string();
                                for &vi in &rows {
                                    let v = &mut self.doc.variables[vi];
                                    let leaf = group_of(&v.name).1.to_string();
                                    v.name = if name.is_empty() { leaf } else { format!("{name}/{leaf}") };
                                }
                            }
                            if shut {
                                continue;
                            }
                        }
                        for vi in rows {
                            if self.var_row(ui, vi, modes, group.is_some()) {
                                delete = Some(self.doc.variables[vi].id);
                            }
                            ui.end_row();
                        }
                    }
                    if let Some(id) = delete {
                        self.doc.variables.retain(|v| v.id != id);
                        for n in self.doc.nodes.values_mut() {
                            if n.bindings.iter().any(|(_, v)| *v == id) {
                                std::sync::Arc::make_mut(n).bindings.retain(|(_, v)| *v != id);
                            }
                        }
                    }
                });
            });
            ui.data_mut(|d| d.insert_temp(collapsed_id, collapsed));
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Create variable:");
                for (label, v) in [
                    ("Color", VarValue::Color(Rgba::rgb(0x0D, 0x99, 0xFF))),
                    ("Number", VarValue::Number(8.0)),
                    ("String", VarValue::Text("Text".into())),
                    ("Boolean", VarValue::Bool(true)),
                ] {
                    if ui.button(label).clicked() {
                        let n = self.doc.variables.iter().filter(|x| x.collection == current).count() + 1;
                        self.doc.add_variable(current, format!("{} {n}", label.to_lowercase()), v);
                    }
                }
            });
            });
            });
        });
        if (self.doc.collections.clone(), self.doc.variables.clone()) != before {
            // Record the edit for undo, then re-resolve bound layers.
            let (c, v) = (self.doc.collections.clone(), self.doc.variables.clone());
            self.doc.collections = before.0;
            self.doc.variables = before.1;
            self.begin_edit();
            self.doc.collections = c;
            self.doc.variables = v;
            self.doc.reflow();
        }
        self.show_variables = open;
    }

    /// One variable's row: type, name (the leaf inside a group), a value per mode, then its
    /// settings (scopes, code syntax, grouping) and delete. Returns whether delete was clicked.
    fn var_row(&mut self, ui: &mut Ui, vi: usize, modes: usize, grouped: bool) -> bool {
        let vid = self.doc.variables[vi].id;
        let targets = alias_targets(&self.doc, vid);
        let resolved = self.doc.resolve_first(vid);
        // Aliases show the target's name; detaching keeps the value it resolves to.
        let aliases: Vec<Option<(String, Option<VarValue>)>> = self.doc.variables[vi]
            .values
            .iter()
            .map(|x| match x {
                VarValue::Alias(a) => Some((self.doc.variable(*a).map_or("Missing".into(), |t| t.name.clone()), self.doc.resolve_first(*a))),
                _ => None,
            })
            .collect();
        ui.horizontal(|ui| {
            if grouped {
                ui.add_space(18.0);
            }
            let t = resolved.as_ref().map_or("Alias", type_name);
            ui.label(RichText::new(&t[..1]).color(crate::appearance::faint(ui.ctx())).monospace()).on_hover_text(t);
            let (group, leaf) = group_of(&self.doc.variables[vi].name);
            let group = group.map(String::from);
            let mut leaf = leaf.to_string();
            if ui.add(egui::TextEdit::singleline(&mut leaf).desired_width(if grouped { 132.0 } else { 150.0 })).changed() {
                self.doc.variables[vi].name = match group {
                    Some(g) => format!("{g}/{leaf}"),
                    None => leaf,
                };
            }
        });
        for m in 0..modes {
            let id = ui.id().with(("var", vid, m));
            if let Some(v) = self.doc.variables[vi].values.get_mut(m) {
                let alias = aliases[m].clone();
                ui.horizontal(|ui| value_editor(ui, id, v, &targets, alias));
            }
        }
        let mut delete = false;
        ui.horizontal(|ui| {
            let edit = icon_button(ui, Icon::Styles, false, 20.0, "Edit variable");
            crate::chrome::popover(&edit, ui.id().with(("var-edit", vid)), "Edit variable", 260.0, |ui| self.var_settings(ui, vi));
            delete = icon_button(ui, Icon::Minus, false, 20.0, "Delete variable").clicked();
        });
        delete
    }

    /// Figma's variable editor: full name, "Show in" scopes, and code syntax per platform.
    fn var_settings(&mut self, ui: &mut Ui, vi: usize) {
        let v = &mut self.doc.variables[vi];
        ui.label(RichText::new("Name").small());
        ui.add(egui::TextEdit::singleline(&mut v.name).desired_width(f32::INFINITY));
        if ui.button("New group with variable").clicked() {
            let leaf = group_of(&v.name).1.to_string();
            let taken: Vec<String> = self.doc.variables.iter().filter_map(|x| group_of(&x.name).0.map(String::from)).collect();
            let g = (1..).map(|k| format!("Group {k}")).find(|g| !taken.contains(g)).unwrap();
            self.doc.variables[vi].name = format!("{g}/{leaf}");
        }
        let kind = self.doc.resolve_first(self.doc.variables[vi].id);
        let v = &mut self.doc.variables[vi];
        // Scopes apply to colors, numbers and strings; booleans show everywhere, as in Figma.
        let mut scopes: Vec<&str> = Bindable::ALL.into_iter().filter(|b| kind.as_ref().is_some_and(|k| b.accepts(k))).map(|b| b.scope()).filter(|s| !scope_label(s).is_empty()).collect();
        scopes.dedup();
        if !scopes.is_empty() {
            ui.separator();
            ui.label(RichText::new("Show in").small());
            let mut all = v.scopes.is_empty();
            if ui.checkbox(&mut all, "All supported properties").changed() {
                v.scopes = if all { vec![] } else { Bindable::ALL.into_iter().filter(|b| scopes.contains(&b.scope())).collect() };
            }
            if !all {
                let mut seen = vec![];
                for s in scopes {
                    let label = scope_label(s);
                    if seen.contains(&label) {
                        continue;
                    }
                    seen.push(label);
                    let props: Vec<Bindable> = Bindable::ALL.into_iter().filter(|b| scope_label(b.scope()) == label && kind.as_ref().is_some_and(|k| b.accepts(k))).collect();
                    let mut on = props.iter().all(|p| v.scopes.contains(p));
                    if ui.checkbox(&mut on, label).changed() {
                        v.scopes.retain(|p| !props.contains(p));
                        if on {
                            v.scopes.extend(props);
                        }
                    }
                }
            }
        }
        ui.separator();
        ui.label(RichText::new("Code syntax").small());
        for (i, platform) in ["Web", "iOS", "Android"].into_iter().enumerate() {
            ui.horizontal(|ui| {
                ui.add_sized([52.0, 18.0], egui::Label::new(platform));
                ui.add(egui::TextEdit::singleline(&mut v.code_syntax[i]).hint_text(crate::codegen::code_name(&v.name, i)).desired_width(f32::INFINITY));
            });
        }
    }

    /// The variables `prop` can use, by collection, as menu buttons. Returns the one clicked.
    pub fn variable_menu(&self, ui: &mut Ui, prop: Bindable) -> Option<VarId> {
        let mut pick = None;
        let mut any = false;
        for c in &self.doc.collections {
            let list: Vec<_> = self.doc.variables.iter().filter(|v| v.collection == c.id && v.shows_in(prop) && self.doc.resolve_first(v.id).is_some_and(|x| prop.accepts(&x))).collect();
            if list.is_empty() {
                continue;
            }
            any = true;
            ui.label(RichText::new(&c.name).small().color(crate::chrome::MENU_MUTED));
            for v in list {
                let swatch = match self.doc.resolve_first(v.id) {
                    Some(VarValue::Color(c)) => RichText::new("■ ").color(c.to32()),
                    _ => RichText::new(""),
                };
                let mut job = egui::text::LayoutJob::default();
                swatch.append_to(&mut job, ui.style(), egui::FontSelection::Default, egui::Align::Center);
                RichText::new(&v.name).append_to(&mut job, ui.style(), egui::FontSelection::Default, egui::Align::Center);
                if ui.button(job).clicked() {
                    pick = Some(v.id);
                    ui.close();
                }
            }
        }
        if !any {
            ui.label(RichText::new("No variables for this property").color(crate::chrome::MENU_MUTED));
        }
        pick
    }

    /// Bind `prop` on every selected layer to `var` (one undo step).
    pub fn bind_selection(&mut self, prop: Bindable, var: VarId) {
        self.checkpoint();
        for &s in &self.sel.clone() {
            let b = &mut self.doc.node_mut(s).bindings;
            b.retain(|(p, _)| *p != prop);
            b.push((prop, var));
        }
        self.doc.reflow();
    }

    /// Design panel: bind selected layers' properties to variables, and pick variable modes.
    pub fn variables_section(&mut self, ui: &mut Ui, id: NodeId) {
        if self.doc.variables.is_empty() && self.doc.collections.is_empty() {
            return;
        }
        let n = self.doc.node(id).clone();
        let in_layout = n.parent.is_some_and(|p| self.doc.node(p).layout.mode != crate::doc::LayoutMode::None);
        ui.separator();
        let mut add: Option<(Bindable, u64)> = None;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Variables").strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let btn = icon_button(ui, Icon::Plus, false, 22.0, "Bind a property");
                egui::Popup::menu(&btn).id(ui.id().with(("bind", id))).style(crate::chrome::menu_style).show(|ui| {
                    for prop in Bindable::ALL {
                        use Bindable as B;
                        let usable = match prop {
                            B::Text | B::FontSize | B::LineHeight | B::LetterSpacing | B::ParagraphSpacing | B::FontFamily | B::FontWeight => n.kind == Kind::Text,
                            B::Gap | B::Padding => n.kind.is_frame(),
                            B::Radius => n.kind == Kind::Rectangle || n.kind.is_frame(),
                            B::MinWidth | B::MaxWidth | B::MinHeight | B::MaxHeight => n.kind.is_frame() || in_layout,
                            B::EffectColor | B::EffectBlur | B::EffectSpread | B::EffectX | B::EffectY => !n.shadows.is_empty(),
                            _ => true,
                        };
                        if !usable {
                            continue;
                        }
                        crate::chrome::menu_button(ui, prop.label(), |ui| {
                            if let Some(v) = self.variable_menu(ui, prop) {
                                add = Some((prop, v));
                            }
                        });
                    }
                });
            });
        });
        if let Some((prop, var)) = add {
            self.bind_selection(prop, var);
        }
        let mut unbind = None;
        for (prop, var) in &n.bindings {
            ui.horizontal(|ui| {
                ui.label(prop.label());
                let name = self.doc.variable(*var).map_or("Missing".into(), |v| v.name.clone());
                ui.label(RichText::new(name).color(crate::canvas::BLUE));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon_button(ui, Icon::Detach, false, 22.0, "Detach variable").clicked() {
                        unbind = Some(*prop);
                    }
                });
            });
        }
        if let Some(prop) = unbind {
            self.checkpoint();
            for &s in &self.sel.clone() {
                self.doc.node_mut(s).bindings.retain(|(p, _)| *p != prop);
            }
        }
        // Mode pickers for collections with several modes (frames and pages scope them).
        if n.kind.is_frame() {
            for c in self.doc.collections.clone().into_iter().filter(|c| c.modes.len() > 1) {
                let explicit = n.modes.iter().find(|(cid, _)| *cid == c.id).map(|(_, m)| *m);
                let mut pick = explicit;
                ui.horizontal(|ui| {
                    ui.label(&c.name);
                    let shown = match pick {
                        Some(m) => c.modes.get(m).cloned().unwrap_or_default(),
                        None => format!("Auto ({})", c.modes[self.doc.mode_for(id, c.id).min(c.modes.len() - 1)]),
                    };
                    egui::ComboBox::from_id_salt(("mode", id, c.id)).selected_text(shown).popup_style(crate::chrome::menu_style.into()).show_ui(ui, |ui| {
                        ui.selectable_value(&mut pick, None, "Auto");
                        for (i, m) in c.modes.iter().enumerate() {
                            ui.selectable_value(&mut pick, Some(i), m);
                        }
                    });
                });
                if pick != explicit {
                    self.checkpoint();
                    let modes = &mut self.doc.node_mut(id).modes;
                    modes.retain(|(cid, _)| *cid != c.id);
                    if let Some(m) = pick {
                        modes.push((c.id, m));
                    }
                    self.doc.reflow();
                }
            }
        }
    }
}
