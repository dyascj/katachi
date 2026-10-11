//! Comments: pins on the canvas with threads, replies, and resolve.

use crate::app::{App, Tool};
use crate::doc::Comment;
use egui::{Color32, FontId, Painter, Pos2, Rect, RichText, Stroke, Ui, vec2};

pub fn author() -> String {
    let user = std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_default();
    let mut c = user.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => "You".into(),
    }
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn ago(t: u64) -> String {
    let s = now().saturating_sub(t);
    match s {
        0..=59 => "now".into(),
        60..=3599 => format!("{}m", s / 60),
        3600..=86399 => format!("{}h", s / 3600),
        _ => format!("{}d", s / 86400),
    }
}

/// Color for an author's avatar, stable per name.
fn avatar(name: &str) -> Color32 {
    let palette = [
        Color32::from_rgb(0x0D, 0x99, 0xFF),
        Color32::from_rgb(0x97, 0x47, 0xFF),
        Color32::from_rgb(0x14, 0xAE, 0x5C),
        Color32::from_rgb(0xF2, 0x48, 0x22),
        Color32::from_rgb(0xFF, 0xA6, 0x29),
    ];
    palette[name.bytes().map(|b| b as usize).sum::<usize>() % palette.len()]
}

impl App {
    pub fn comments_visible(&self) -> bool {
        self.tool == Tool::Comment || self.show_comments
    }

    /// Comment tool click: start a draft pin there, anchored to the frame under it.
    pub fn start_comment(&mut self, w: Pos2) {
        let anchor = self.doc.hit(self.page, w, 0.0).map(|h| crate::prototype::top_frame(&self.doc, h));
        let at = match anchor {
            Some(a) => (w - self.doc.node(a).rect.min).to_pos2(),
            None => w,
        };
        self.draft_comment = Some((Comment { id: 0, page: self.page, at, anchor, messages: vec![], resolved: false }, String::new()));
        self.open_comment = None;
    }

    pub fn draw_comment_pins(&mut self, painter: &Painter) -> Vec<(u64, Rect)> {
        let mut hits = vec![];
        if !self.comments_visible() {
            return hits;
        }
        for c in self.doc.comments.iter().filter(|c| c.page == self.page && !c.resolved) {
            let p = self.view.to_screen(self.doc.comment_pos(c));
            let who = c.messages.first().map_or("?".to_string(), |m| m.0.clone());
            let r = Rect::from_min_size(p - vec2(0.0, 28.0), vec2(28.0, 28.0));
            // A speech-bubble pin: rounded on three corners, pointed at the bottom-left.
            painter.rect_filled(r, egui::CornerRadius { nw: 14, ne: 14, se: 14, sw: 0 }, Color32::WHITE);
            painter.rect_stroke(r, egui::CornerRadius { nw: 14, ne: 14, se: 14, sw: 0 }, Stroke::new(1.0, Color32::from_gray(0xD0)), egui::StrokeKind::Inside);
            painter.circle_filled(r.center(), 11.0, avatar(&who));
            painter.text(r.center(), egui::Align2::CENTER_CENTER, who.chars().next().unwrap_or('?').to_string(), FontId::proportional(11.0), Color32::WHITE);
            if self.open_comment == Some(c.id) {
                painter.rect_stroke(r.expand(2.0), 16.0, Stroke::new(2.0, crate::canvas::BLUE), egui::StrokeKind::Outside);
            }
            hits.push((c.id, r));
        }
        if let Some((d, _)) = &self.draft_comment {
            let p = self.view.to_screen(self.doc.comment_pos(d));
            let r = Rect::from_min_size(p - vec2(0.0, 28.0), vec2(28.0, 28.0));
            painter.rect_filled(r, egui::CornerRadius { nw: 14, ne: 14, se: 14, sw: 0 }, crate::canvas::BLUE);
            painter.text(r.center(), egui::Align2::CENTER_CENTER, "+", FontId::proportional(16.0), Color32::WHITE);
        }
        hits
    }

    /// Composer for a new comment and the open thread's popover.
    pub fn comment_popups(&mut self, ctx: &egui::Context) {
        if let Some((draft, mut text)) = self.draft_comment.take() {
            let p = self.view.to_screen(self.doc.comment_pos(&draft)) + vec2(36.0, -28.0);
            let mut keep = true;
            let mut post = false;
            egui::Area::new(egui::Id::new("comment-draft")).fixed_pos(p).order(egui::Order::Foreground).show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(240.0);
                    let r = ui.add(egui::TextEdit::multiline(&mut text).hint_text("Add a comment").desired_rows(2).desired_width(f32::INFINITY));
                    r.request_focus();
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            keep = false;
                        }
                        let enter = r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.command);
                        if ui.add_enabled(!text.trim().is_empty(), egui::Button::new("Post")).clicked() || (enter && !text.trim().is_empty()) {
                            post = true;
                        }
                    });
                });
            });
            if post {
                self.checkpoint();
                let id = self.doc.next_id;
                self.doc.next_id += 1;
                let mut c = draft;
                c.id = id;
                c.messages.push((author(), text.trim().to_string(), now()));
                self.doc.comments.push(c);
                self.open_comment = Some(id);
            } else if keep {
                self.draft_comment = Some((draft, text));
            }
        }
        let Some(id) = self.open_comment else { return };
        let Some(c) = self.doc.comments.iter().find(|c| c.id == id).cloned() else {
            self.open_comment = None;
            return;
        };
        let p = self.view.to_screen(self.doc.comment_pos(&c)) + vec2(36.0, -28.0);
        let mut action: Option<&str> = None;
        let mut edited: Option<(usize, String)> = None;
        let mut stop_editing = false;
        let mut reply = std::mem::take(&mut self.comment_reply);
        egui::Area::new(egui::Id::new(("comment-thread", id))).fixed_pos(p).order(egui::Order::Foreground).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(260.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Comment").strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::panels::icon_button(ui, crate::panels::Icon::Close, false, 20.0, "Close").clicked() {
                            action = Some("close");
                        }
                        if ui.small_button("Delete").clicked() {
                            action = Some("delete");
                        }
                        if ui.small_button(if c.resolved { "Reopen" } else { "Resolve" }).clicked() {
                            action = Some("resolve");
                        }
                    });
                });
                ui.separator();
                // Editing one of your own messages: (index, draft), kept in egui's memory.
                let edit_id = egui::Id::new(("comment-edit", id));
                let mut editing: Option<(usize, String)> = ui.data(|d| d.get_temp(edit_id));
                let me = author();
                for (i, (who, text, t)) in c.messages.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(20.0, 20.0), egui::Sense::hover());
                        ui.painter().circle_filled(r.center(), 10.0, avatar(who));
                        ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, who.chars().next().unwrap_or('?').to_string(), FontId::proportional(10.0), Color32::WHITE);
                        ui.label(RichText::new(who).strong());
                        ui.label(RichText::new(ago(*t)).color(crate::appearance::faint(ui.ctx())).small());
                        if *who == me && editing.is_none() {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("Edit").clicked() {
                                    editing = Some((i, text.clone()));
                                }
                            });
                        }
                    });
                    match &mut editing {
                        Some((at, draft)) if *at == i => {
                            ui.add(egui::TextEdit::multiline(draft).desired_rows(2).desired_width(f32::INFINITY));
                            ui.horizontal(|ui| {
                                if ui.small_button("Save").clicked() && !draft.trim().is_empty() {
                                    edited = Some((i, draft.trim().to_string()));
                                }
                                if ui.small_button("Cancel").clicked() || edited.is_some() {
                                    stop_editing = true;
                                }
                            });
                        }
                        _ => {
                            ui.label(text);
                        }
                    }
                    ui.add_space(4.0);
                }
                if stop_editing {
                    editing = None;
                }
                ui.data_mut(|d| {
                    if let Some(e) = editing {
                        d.insert_temp(edit_id, e);
                    } else {
                        d.remove::<(usize, String)>(edit_id);
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut reply).hint_text("Reply").desired_width(190.0));
                    if ui.add_enabled(!reply.trim().is_empty(), egui::Button::new("Send")).clicked() {
                        action = Some("reply");
                    }
                });
            });
        });
        if let Some((i, text)) = edited {
            self.checkpoint();
            if let Some(m) = self.doc.comments.iter_mut().find(|x| x.id == id).and_then(|c| c.messages.get_mut(i)) {
                m.1 = text;
            }
        }
        match action {
            Some("close") => self.open_comment = None,
            Some("delete") => {
                self.checkpoint();
                self.doc.comments.retain(|x| x.id != id);
                self.open_comment = None;
            }
            Some("resolve") => {
                self.checkpoint();
                if let Some(x) = self.doc.comments.iter_mut().find(|x| x.id == id) {
                    x.resolved = !x.resolved;
                }
                self.open_comment = None;
            }
            Some("reply") => {
                self.checkpoint();
                if let Some(x) = self.doc.comments.iter_mut().find(|x| x.id == id) {
                    x.messages.push((author(), reply.trim().to_string(), now()));
                }
                reply.clear();
            }
            _ => {}
        }
        self.comment_reply = reply;
    }

    /// Right panel list of threads on this page while commenting.
    pub fn comments_panel(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Comments").strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.checkbox(&mut self.show_resolved, "Resolved");
            });
        });
        ui.separator();
        let list: Vec<Comment> = self.doc.comments.iter().filter(|c| c.page == self.page && (self.show_resolved || !c.resolved)).cloned().collect();
        if list.is_empty() {
            ui.label(RichText::new("Click anywhere on the canvas to leave a comment.").color(crate::appearance::muted(ui.ctx())));
        }
        for c in list.iter().rev() {
            let Some((who, text, t)) = c.messages.first() else { continue };
            let r = ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(who).strong());
                    ui.label(RichText::new(ago(*t)).color(crate::appearance::faint(ui.ctx())).small());
                    if c.resolved {
                        ui.label(RichText::new("Resolved").color(Color32::from_rgb(0x14, 0xAE, 0x5C)).small());
                    }
                });
                ui.label(text);
                if c.messages.len() > 1 {
                    ui.label(RichText::new(format!("{} replies", c.messages.len() - 1)).color(crate::canvas::BLUE).small());
                }
            });
            if ui.interact(r.response.rect, ui.id().with(("comment-row", c.id)), egui::Sense::click()).clicked() {
                self.open_comment = Some(c.id);
                let p = self.doc.comment_pos(c);
                if let Some(cr) = self.canvas_rect {
                    self.view.pan = cr.center().to_vec2() - p.to_vec2() * self.view.zoom;
                }
            }
            ui.separator();
        }
    }
}
