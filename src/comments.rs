//! Comments: pins on the canvas with threads, replies, resolve, @mentions and unread dots.

use crate::app::{App, Tool};
use crate::doc::Comment;
use egui::{Color32, FontId, Painter, Pos2, Rect, RichText, Stroke, Ui, vec2};

/// Your name on comments and in multiplayer: the one set in Settings, or the system user name.
pub fn author() -> String {
    author_from(&crate::settings::load())
}

pub fn author_from(p: &crate::settings::Prefs) -> String {
    if p.name.trim().is_empty() { system_name() } else { p.name.trim().to_string() }
}

pub fn system_name() -> String {
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

/// The comments panel's filter, as in Figma.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Filter {
    All,
    #[default]
    Unresolved,
    Resolved,
    /// Threads you wrote in.
    Mine,
    /// Threads that @mention you.
    Mentions,
}

impl Filter {
    pub const ALL: [(Filter, &str); 5] =
        [(Filter::All, "All"), (Filter::Unresolved, "Unresolved"), (Filter::Resolved, "Resolved"), (Filter::Mine, "Mine"), (Filter::Mentions, "Mentions")];
}

/// The "@name" being typed at the end of `text`: what follows its last "@" that starts a word.
/// (Names can have spaces, so the query can too, but not a line break.)
pub fn mention_query(text: &str) -> Option<&str> {
    let at = text.rfind('@')?;
    let q = &text[at + 1..];
    let starts_word = text[..at].chars().next_back().is_none_or(char::is_whitespace);
    (starts_word && !q.contains('\n') && q.chars().count() <= 40).then_some(q)
}

/// Finish the "@name" being typed with `name`.
pub fn complete_mention(text: &mut String, name: &str) {
    if let Some(at) = text.rfind('@') {
        text.truncate(at + 1);
        text.push_str(name);
        text.push(' ');
    }
}

/// Whether `text` @mentions `name` (as a whole name, not the start of a longer one).
pub fn mentions(text: &str, name: &str) -> bool {
    let tag = format!("@{name}");
    text.match_indices(&tag).any(|(i, _)| !text[i + tag.len()..].starts_with(|c: char| c.is_alphanumeric()))
}

/// Message text with @mentions of `people` (or any "@word") in bold blue.
fn mention_job(text: &str, people: &[String], color: Color32, width: f32) -> egui::text::LayoutJob {
    let plain = egui::TextFormat { font_id: FontId::proportional(12.0), color, ..Default::default() };
    let tag = egui::TextFormat { font_id: FontId::new(12.0, egui::FontFamily::Name("Inter SemiBold".into())), color: crate::canvas::BLUE, ..Default::default() };
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = width;
    let (mut from, mut i) = (0, 0);
    while let Some(off) = text[i..].find('@') {
        let at = i + off;
        let rest = &text[at + 1..];
        let starts_word = text[..at].chars().next_back().is_none_or(char::is_whitespace);
        let known = people.iter().filter(|p| !p.is_empty() && rest.starts_with(p.as_str())).map(|p| p.len()).max();
        let len = known.unwrap_or_else(|| rest.find(|c: char| !c.is_alphanumeric()).unwrap_or(rest.len()));
        if starts_word && len > 0 {
            job.append(&text[from..at], 0.0, plain.clone());
            job.append(&text[at..at + 1 + len], 0.0, tag.clone());
            from = at + 1 + len;
        }
        i = at + 1 + len;
    }
    job.append(&text[from..], 0.0, plain);
    job
}

impl App {
    /// People to @mention: everyone in the session and everyone who has commented.
    pub fn people(&self) -> Vec<String> {
        let mut names: Vec<String> = self.collab.iter().flat_map(|c| c.peers.values().map(|p| p.name.clone())).collect();
        names.push(self.collab_name.clone());
        names.extend(self.doc.comments.iter().flat_map(|c| c.messages.iter().map(|m| m.0.clone())));
        names.retain(|n| !n.trim().is_empty());
        names.sort();
        names.dedup();
        names
    }

    /// Others wrote in this thread since you last read it.
    pub fn unread(&self, c: &Comment) -> bool {
        c.messages.iter().skip(self.comment_read.get(&c.id).copied().unwrap_or(0)).any(|m| m.0 != self.collab_name)
    }

    /// Mark a thread read, or unread from the last message someone else wrote.
    pub fn mark_read(&mut self, id: u64, read: bool) {
        let Some(c) = self.doc.comments.iter().find(|c| c.id == id) else { return };
        let upto = if read { c.messages.len() } else { c.messages.iter().rposition(|m| m.0 != self.collab_name).unwrap_or(c.messages.len()) };
        self.comment_read.insert(id, upto);
    }

    /// A file just opened: what's in it counts as read; only what others add later is new.
    pub fn read_all_comments(&mut self) {
        self.comment_read = self.doc.comments.iter().map(|c| (c.id, c.messages.len())).collect();
    }

    /// Threads on this page that pass the panel's filter, oldest first.
    pub fn filtered_comments(&self) -> Vec<Comment> {
        let me = &self.collab_name;
        let keep = |c: &Comment| match self.comment_filter {
            Filter::All => true,
            Filter::Unresolved => !c.resolved,
            Filter::Resolved => c.resolved,
            Filter::Mine => c.messages.iter().any(|m| &m.0 == me),
            Filter::Mentions => c.messages.iter().any(|m| mentions(&m.1, me)),
        };
        self.doc.comments.iter().filter(|c| c.page == self.page && keep(c)).cloned().collect()
    }

    /// The @mention suggestions under a comment field: picking one completes the name.
    fn mention_menu(&self, ui: &mut Ui, text: &mut String, field: egui::Id) {
        let Some(q) = mention_query(text).map(str::to_lowercase) else { return };
        let hits: Vec<String> = self.people().into_iter().filter(|p| p.to_lowercase().starts_with(&q) && p.to_lowercase() != q).take(5).collect();
        if hits.is_empty() {
            return;
        }
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            for name in hits {
                if ui.add(egui::Button::new(RichText::new(format!("@{name}")).color(crate::canvas::BLUE)).frame(false)).clicked() {
                    complete_mention(text, &name);
                    // Keep typing after the name.
                    if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), field) {
                        let end = egui::text::CCursor::new(text.chars().count());
                        st.cursor.set_char_range(Some(egui::text::CCursorRange::one(end)));
                        st.store(ui.ctx(), field);
                    }
                    ui.memory_mut(|m| m.request_focus(field));
                }
            }
        });
    }

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
            if self.unread(c) {
                painter.circle(r.right_top() + vec2(-2.0, 2.0), 5.0, crate::canvas::BLUE, Stroke::new(1.5, Color32::WHITE));
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
                    let field = egui::Id::new("comment-draft-text");
                    let r = ui.add(egui::TextEdit::multiline(&mut text).id(field).hint_text("Add a comment, @ to mention").desired_rows(2).desired_width(f32::INFINITY));
                    r.request_focus();
                    self.mention_menu(ui, &mut text, field);
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
                c.messages.push((self.collab_name.clone(), text.trim().to_string(), now()));
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
        // Open threads are read, replies arriving meanwhile included.
        self.mark_read(id, true);
        let people = self.people();
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
                        if c.messages.iter().any(|m| m.0 != self.collab_name) && ui.small_button("Mark as unread").clicked() {
                            action = Some("unread");
                        }
                    });
                });
                ui.separator();
                // Editing one of your own messages: (index, draft), kept in egui's memory.
                let edit_id = egui::Id::new(("comment-edit", id));
                let mut editing: Option<(usize, String)> = ui.data(|d| d.get_temp(edit_id));
                let me = self.collab_name.clone();
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
                            ui.label(mention_job(text, &people, ui.visuals().text_color(), ui.available_width()));
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
                let field = egui::Id::new(("comment-reply-text", id));
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut reply).id(field).hint_text("Reply, @ to mention").desired_width(190.0));
                    if ui.add_enabled(!reply.trim().is_empty(), egui::Button::new("Send")).clicked() {
                        action = Some("reply");
                    }
                });
                self.mention_menu(ui, &mut reply, field);
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
            Some("unread") => {
                self.mark_read(id, false);
                self.open_comment = None;
            }
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
                    x.messages.push((self.collab_name.clone(), reply.trim().to_string(), now()));
                }
                reply.clear();
            }
            _ => {}
        }
        self.comment_reply = reply;
    }

    /// Right panel list of threads on this page while commenting, filtered as picked.
    pub fn comments_panel(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Comments").strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = Filter::ALL.iter().find(|f| f.0 == self.comment_filter).map_or("", |f| f.1);
                egui::ComboBox::from_id_salt("comment-filter").selected_text(label).width(110.0).popup_style(crate::chrome::menu_style.into()).show_ui(ui, |ui| {
                    for (f, name) in Filter::ALL {
                        ui.selectable_value(&mut self.comment_filter, f, name);
                    }
                });
            });
        });
        ui.separator();
        let list = self.filtered_comments();
        if list.is_empty() {
            let hint = if self.doc.comments.iter().any(|c| c.page == self.page) { "No comments match this filter." } else { "Click anywhere on the canvas to leave a comment." };
            ui.label(RichText::new(hint).color(crate::appearance::muted(ui.ctx())));
        }
        let people = self.people();
        for c in list.iter().rev() {
            let Some((who, text, t)) = c.messages.first() else { continue };
            let unread = self.unread(c);
            let r = ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    if unread {
                        let (d, _) = ui.allocate_exact_size(vec2(8.0, 8.0), egui::Sense::hover());
                        ui.painter().circle_filled(d.center(), 4.0, crate::canvas::BLUE);
                    }
                    ui.label(RichText::new(who).strong());
                    ui.label(RichText::new(ago(*t)).color(crate::appearance::faint(ui.ctx())).small());
                    if c.resolved {
                        ui.label(RichText::new("Resolved").color(Color32::from_rgb(0x14, 0xAE, 0x5C)).small());
                    }
                });
                ui.label(mention_job(text, &people, ui.visuals().text_color(), ui.available_width()));
                if c.messages.len() > 1 {
                    ui.label(RichText::new(format!("{} replies", c.messages.len() - 1)).color(crate::canvas::BLUE).small());
                }
            });
            let row = ui.interact(r.response.rect, ui.id().with(("comment-row", c.id)), egui::Sense::click());
            let mut read = None;
            crate::chrome::context_menu(&row, |ui| {
                if ui.button(if unread { "Mark as read" } else { "Mark as unread" }).clicked() {
                    read = Some(unread);
                }
            });
            if let Some(read) = read {
                self.mark_read(c.id, read);
            }
            if row.clicked() {
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
