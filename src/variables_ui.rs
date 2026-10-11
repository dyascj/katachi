//! Local variables window, property bindings, and variable modes.

use crate::app::App;
use crate::canvas::num;
use crate::doc::{Bindable, Kind, NodeId, Rgba, VarValue};
use crate::panels::{Icon, icon_button};
use egui::{DragValue, RichText, Ui};

fn type_name(v: &VarValue) -> &'static str {
    match v {
        VarValue::Color(_) => "Color",
        VarValue::Number(_) => "Number",
        VarValue::Text(_) => "String",
        VarValue::Bool(_) => "Boolean",
        VarValue::Alias(_) => "Alias",
    }
}

/// Edit one variable value in place. Returns whether it changed.
fn value_editor(ui: &mut Ui, id: egui::Id, v: &mut VarValue, names: &[(u64, String)]) -> bool {
    match v {
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
        VarValue::Alias(a) => {
            let name = names.iter().find(|(i, _)| i == a).map_or("Missing".into(), |(_, n)| n.clone());
            ui.label(RichText::new(format!("→ {name}")).color(crate::canvas::BLUE));
            false
        }
    }
}

impl App {
    /// Figma's "Local variables" table: collections, modes as columns, variables as rows.
    pub fn variables_window(&mut self, ctx: &egui::Context) {
        if !self.show_variables {
            return;
        }
        let mut open = true;
        let before = (self.doc.collections.clone(), self.doc.variables.clone());
        crate::chrome::window(ctx, "Local variables", &mut open, 680.0, |ui| {
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
            ui.horizontal(|ui| {
                for c in &self.doc.collections {
                    if ui.selectable_label(c.id == current, &c.name).clicked() {
                        self.var_collection = Some(c.id);
                    }
                }
                if ui.small_button("+ Collection").clicked() {
                    let id = self.doc.add_collection(format!("Collection {}", self.doc.collections.len() + 1));
                    self.var_collection = Some(id);
                }
            });
            ui.separator();
            let ci = self.doc.collections.iter().position(|c| c.id == current).unwrap();
            ui.horizontal(|ui| {
                ui.label("Collection");
                ui.text_edit_singleline(&mut self.doc.collections[ci].name);
            });
            let names: Vec<(u64, String)> = self.doc.variables.iter().map(|v| (v.id, v.name.clone())).collect();
            let modes = self.doc.collections[ci].modes.len();
            egui::ScrollArea::both().max_height(300.0).show(ui, |ui| {
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
                    for vi in 0..self.doc.variables.len() {
                        if self.doc.variables[vi].collection != current {
                            continue;
                        }
                        let vid = self.doc.variables[vi].id;
                        ui.horizontal(|ui| {
                            let t = type_name(&self.doc.variables[vi].values[0]);
                            ui.label(RichText::new(&t[..1]).color(crate::appearance::faint(ui.ctx())).monospace()).on_hover_text(t);
                            ui.add(egui::TextEdit::singleline(&mut self.doc.variables[vi].name).desired_width(150.0));
                        });
                        for m in 0..modes {
                            let id = ui.id().with(("var", vid, m));
                            if let Some(v) = self.doc.variables[vi].values.get_mut(m) {
                                ui.horizontal(|ui| value_editor(ui, id, v, &names));
                            }
                        }
                        if icon_button(ui, Icon::Minus, false, 20.0, "Delete variable").clicked() {
                            delete = Some(vid);
                        }
                        ui.end_row();
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

    /// Design panel: bind selected layers' properties to variables, and pick variable modes.
    pub fn variables_section(&mut self, ui: &mut Ui, id: NodeId) {
        if self.doc.variables.is_empty() && self.doc.collections.is_empty() {
            return;
        }
        let n = self.doc.node(id).clone();
        ui.separator();
        let mut add: Option<(Bindable, u64)> = None;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Variables").strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let btn = icon_button(ui, Icon::Plus, false, 22.0, "Bind a property");
                egui::Popup::menu(&btn).id(ui.id().with(("bind", id))).style(crate::chrome::menu_style).show(|ui| {
                    for prop in Bindable::ALL {
                        let usable = match prop {
                            Bindable::Text | Bindable::FontSize => n.kind == Kind::Text,
                            Bindable::Gap | Bindable::Padding => n.kind.is_frame(),
                            Bindable::Radius => n.kind == Kind::Rectangle || n.kind.is_frame(),
                            _ => true,
                        };
                        if !usable {
                            continue;
                        }
                        crate::chrome::menu_button(ui, format!("{prop:?}"), |ui| {
                            for v in self.doc.variables.iter().filter(|v| v.values.first().is_some_and(|x| prop.accepts(x) || matches!(x, VarValue::Alias(_)))) {
                                if ui.button(&v.name).clicked() {
                                    add = Some((prop, v.id));
                                    ui.close();
                                }
                            }
                        });
                    }
                });
            });
        });
        if let Some((prop, var)) = add {
            self.checkpoint();
            for &s in &self.sel.clone() {
                let b = &mut self.doc.node_mut(s).bindings;
                b.retain(|(p, _)| *p != prop);
                b.push((prop, var));
            }
            self.doc.reflow();
        }
        let mut unbind = None;
        for (prop, var) in &n.bindings {
            ui.horizontal(|ui| {
                ui.label(format!("{prop:?}"));
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
