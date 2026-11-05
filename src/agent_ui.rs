//! The Agent panel (Cmd L): chat with a coding agent installed on this machine while it works on
//! the file through Katachi's MCP server. Each tab has its own conversation, and an agent keeps
//! working on its tab when you switch to another.

use crate::agent::{self, Event, Harness, Installed, Run};
use crate::app::App;
use crate::appearance as theme;
use egui::{Align, Color32, Key, Layout, Modifiers, RichText, ScrollArea, Ui, vec2};

#[derive(Default)]
pub struct Chat {
    pub open: bool,
    input: String,
    items: Vec<Item>,
    /// The agent's own session, to continue the conversation (and which agent it belongs to).
    session: Option<(Harness, String)>,
    run: Option<Run>,
    /// When the turn started, and the undo history's length then.
    started: f64,
    undo_from: usize,
    /// The undo history's length when the last turn ended, if it changed the file.
    undo_to: Option<usize>,
    focus: bool,
}

enum Item {
    You(String),
    Agent(String),
    Step(String),
    Error(String),
}

const INPUT: &str = "agent-input";
const SUGGESTIONS: [&str; 4] = [
    "Design a sign-in screen for a mobile app",
    "Tidy the selection with auto layout",
    "Make a dark version of the selected frame",
    "Turn the selection into a component with hover and pressed variants",
];

impl App {
    pub fn toggle_agent(&mut self) {
        self.agent.open = !self.agent.open;
        self.agent.focus = self.agent.open;
    }

    /// Read what the agent did since last frame. Runs for every tab, on screen or not.
    pub fn agent_poll(&mut self, now: f64) {
        let Some(run) = &self.agent.run else { return };
        let events: Vec<Event> = run.events.try_iter().collect();
        let harness = self.agent_choice();
        for e in events {
            let items = &mut self.agent.items;
            match e {
                Event::Session(id) => {
                    if let Some(h) = harness {
                        self.agent.session.get_or_insert((h, id));
                    }
                }
                Event::Text { text, delta } => match items.last_mut() {
                    Some(Item::Agent(t)) if delta => t.push_str(&text),
                    _ => items.push(Item::Agent(text.trim().to_string())),
                },
                Event::Tool(step) => {
                    // A run of the same step (five edits in a row) shows once.
                    if !matches!(items.last(), Some(Item::Step(s)) if *s == step) {
                        items.push(Item::Step(step));
                    }
                }
                Event::Error(msg) => items.push(Item::Error(friendly_error(harness, &msg))),
                Event::Exit { .. } => {
                    self.agent.run = None;
                    let to = self.undo_len();
                    self.agent.undo_to = (to > self.agent.undo_from).then_some(to);
                    self.agent.started = now;
                    break;
                }
            }
        }
    }

    /// The agent to use: the one picked last, if it's still installed, else the first found.
    fn agent_choice(&self) -> Option<Harness> {
        let found = agent::found_now()?;
        let installed = |h: Harness| found.agents.iter().any(|a| a.harness == h);
        self.prefs.agent.filter(|&h| installed(h)).or_else(|| found.agents.first().map(|a| a.harness))
    }

    fn agent_installed(&self) -> Option<Installed> {
        let h = self.agent_choice()?;
        agent::found_now()?.agents.iter().find(|a| a.harness == h).cloned()
    }

    /// What the agent should know about where the person is, sent with every message.
    fn agent_context(&self) -> String {
        let page = &self.doc.pages[self.page].name;
        let mut s = format!("[Katachi: file \"{}\", page \"{}\"", self.display_name(), page);
        if self.sel.is_empty() {
            s.push_str(", nothing selected]");
        } else {
            let names: Vec<String> = self.sel.iter().take(10).map(|&id| {
                let n = self.doc.node(id);
                format!("{} \"{}\" ({id})", crate::mcp_tools::kind_tag(n.kind), n.name)
            }).collect();
            let more = if self.sel.len() > 10 { format!(" and {} more", self.sel.len() - 10) } else { String::new() };
            s.push_str(&format!(", selected: {}{more}]", names.join(", ")));
        }
        s
    }

    fn agent_send(&mut self, ctx: &egui::Context, text: String) {
        let text = text.trim().to_string();
        let Some(agent) = self.agent_installed() else { return };
        let Some(found) = agent::found_now() else { return };
        if text.is_empty() || self.agent.run.is_some() {
            return;
        }
        let Some(port) = self.mcp_port() else {
            self.agent.items.push(Item::Error("Katachi's MCP server isn't running, so agents can't reach this file.".into()));
            return;
        };
        // A conversation stays with the agent that started it.
        if self.agent.session.as_ref().is_some_and(|(h, _)| *h != agent.harness) {
            self.agent.session = None;
        }
        let first = self.agent.session.is_none();
        let prompt = if first { format!("{PREAMBLE}\n\n{}\n\n{text}", self.agent_context()) } else { format!("{}\n\n{text}", self.agent_context()) };
        let cwd = self.path.as_ref().and_then(|p| p.parent()).filter(|d| d.is_dir()).map(|d| d.to_path_buf()).unwrap_or_else(|| crate::files::data_dir().join("agent"));
        let _ = std::fs::create_dir_all(&cwd);
        let url = format!("http://127.0.0.1:{port}/mcp/{}", self.tab_id);
        let req = agent::Request {
            prompt: &prompt,
            session: self.agent.session.as_ref().map(|(_, s)| s.as_str()),
            model: None,
            mcp_url: &url,
            cwd: &cwd,
            path: &found.path,
        };
        self.agent.items.push(Item::You(text));
        match Run::start(&agent, &req, ctx) {
            Ok(run) => {
                self.agent.run = Some(run);
                self.agent.started = ctx.input(|i| i.time);
                self.agent.undo_from = self.undo_len();
                self.agent.undo_to = None;
            }
            Err(e) => self.agent.items.push(Item::Error(format!("Couldn't start {}: {e}", agent.harness.label()))),
        }
    }

    /// Undo everything the last turn changed, as long as nothing was edited after it.
    fn agent_undo_turn(&mut self) {
        let Some(to) = self.agent.undo_to.take() else { return };
        if self.undo_len() != to {
            return;
        }
        for _ in self.agent.undo_from..to {
            self.undo();
        }
    }

    pub fn agent_panel(&mut self, ui: &mut Ui) {
        if !self.agent.open {
            return;
        }
        let ctx = ui.ctx().clone();
        let found = agent::found(&ctx);
        egui::Panel::right("agent").resizable(true).default_size(340.0).min_size(280.0).max_size(640.0).show(ui, |ui| {
            ui.add_space(6.0);
            self.agent_header(ui, found.as_deref());
            ui.separator();
            let Some(found) = found else {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new("Looking for agents").color(theme::muted(&ctx)));
                });
                return;
            };
            if found.agents.is_empty() {
                self.agent_none_installed(ui);
                return;
            }
            // The composer sits at the bottom; the conversation fills the rest.
            egui::Panel::bottom("agent-composer").resizable(false).show_separator_line(false).frame(egui::Frame::NONE.inner_margin(egui::Margin { left: 0, right: 0, top: 6, bottom: 8 })).show(ui, |ui| self.agent_composer(ui));
            self.agent_transcript(ui);
        });
    }

    fn agent_header(&mut self, ui: &mut Ui, found: Option<&agent::Found>) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Agent").strong());
            let current = self.agent_choice();
            if let Some(found) = found.filter(|f| !f.agents.is_empty()) {
                let label = current.map_or("Choose", Harness::label);
                ui.add_enabled_ui(self.agent.run.is_none(), |ui| {
                    egui::ComboBox::from_id_salt("agent-harness").selected_text(label).width(120.0).show_ui(ui, |ui| {
                        for h in Harness::ALL {
                            let installed = found.agents.iter().any(|a| a.harness == h);
                            let resp = ui.add_enabled(installed, egui::Button::selectable(current == Some(h), h.label()));
                            let resp = if installed { resp } else { resp.on_disabled_hover_text(format!("Not installed (no `{}` on your PATH)", h.program())) };
                            if resp.clicked() {
                                self.prefs.agent = Some(h);
                                crate::settings::save(&self.prefs);
                            }
                        }
                    });
                });
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if crate::panels::icon_button(ui, crate::panels::Icon::Close, false, 22.0, "Close  ⌘L").clicked() {
                    self.agent.open = false;
                }
                let can_clear = self.agent.run.is_none() && !self.agent.items.is_empty();
                if ui.add_enabled(can_clear, egui::Button::new("New chat").frame(false)).on_hover_text("Start a new conversation").clicked() {
                    self.agent.items.clear();
                    self.agent.session = None;
                    self.agent.undo_to = None;
                    self.agent.focus = true;
                }
            });
        });
    }

    fn agent_none_installed(&mut self, ui: &mut Ui) {
        let muted = theme::muted(ui.ctx());
        ui.add_space(8.0);
        ui.label(RichText::new("No agents found").strong());
        ui.label(RichText::new("Katachi works with the coding agents installed on this computer, using your own sign-in. Install one, sign in once in a terminal, then look again.").color(muted));
        ui.add_space(6.0);
        for (name, url) in [
            ("Claude Code", "https://docs.anthropic.com/en/docs/claude-code"),
            ("Codex", "https://developers.openai.com/codex/cli"),
            ("Gemini CLI", "https://github.com/google-gemini/gemini-cli"),
            ("OpenCode", "https://opencode.ai"),
            ("Cursor CLI", "https://cursor.com/cli"),
        ] {
            ui.hyperlink_to(name, url);
        }
        ui.add_space(8.0);
        ui.add_enabled_ui(!agent::scanning(), |ui| {
            if ui.button("Look again").clicked() {
                agent::rescan(ui.ctx());
            }
        });
    }

    fn agent_transcript(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let (muted, faint) = (theme::muted(&ctx), theme::faint(&ctx));
        let name = self.agent_choice().map_or("the agent", Harness::label);
        let mut send = None;
        let mut undo = false;
        ScrollArea::vertical().auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
            ui.set_width(ui.available_width());
            if self.agent.items.is_empty() {
                ui.add_space(8.0);
                ui.label(RichText::new(format!("Ask {name} to design or change something in this file. It sees the canvas and your selection, and every change can be undone.")).color(muted));
                ui.add_space(8.0);
                for s in SUGGESTIONS {
                    if ui.add(egui::Button::new(RichText::new(s).color(theme::ink(&ctx))).wrap()).clicked() {
                        send = Some(s.to_string());
                    }
                }
                return;
            }
            for item in &self.agent.items {
                ui.add_space(4.0);
                match item {
                    Item::You(t) => {
                        egui::Frame::new().fill(theme::child_bg(&ctx)).corner_radius(8).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.add(egui::Label::new(RichText::new(t).color(theme::ink(&ctx))).selectable(true));
                        });
                    }
                    Item::Agent(t) => {
                        ui.add(egui::Label::new(t.as_str()).selectable(true));
                    }
                    Item::Step(s) => {
                        ui.label(RichText::new(format!("·  {s}")).small().color(faint));
                    }
                    Item::Error(e) => {
                        ui.add(egui::Label::new(RichText::new(e).color(Color32::from_rgb(0xE0, 0x4E, 0x3A))).selectable(true));
                    }
                }
            }
            ui.add_space(6.0);
            if self.agent.run.is_some() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    let secs = (ctx.input(|i| i.time) - self.agent.started).max(0.0) as u64;
                    ui.label(RichText::new(format!("Working  {}:{:02}", secs / 60, secs % 60)).color(muted));
                });
                ctx.request_repaint_after(std::time::Duration::from_millis(500));
            } else if self.agent.undo_to.is_some_and(|to| to == self.undo_len()) {
                let steps = self.agent.undo_to.unwrap_or(0) - self.agent.undo_from;
                if ui.button("Undo these changes").on_hover_text(format!("Undo the {steps} change{} from this reply", if steps == 1 { "" } else { "s" })).clicked() {
                    undo = true;
                }
            }
        });
        if undo {
            self.agent_undo_turn();
        }
        if let Some(s) = send {
            self.agent_send(&ctx, s);
        }
    }

    fn agent_composer(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let id = egui::Id::new(INPUT);
        let running = self.agent.run.is_some();
        // Enter sends, Shift Enter makes a new line.
        let enter = !running && ctx.memory(|m| m.has_focus(id)) && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
        let name = self.agent_choice().map_or("the agent", Harness::label);
        let hint = if self.sel.is_empty() { format!("Ask {name}") } else { format!("Ask {name} about the selection") };
        let edit = egui::TextEdit::multiline(&mut self.agent.input).id(id).hint_text(hint).desired_rows(3).desired_width(f32::INFINITY).return_key(Some(egui::KeyboardShortcut::new(Modifiers::SHIFT, Key::Enter)));
        let resp = ui.add(edit);
        if std::mem::take(&mut self.agent.focus) {
            resp.request_focus();
        }
        ui.horizontal(|ui| {
            let sel = match self.sel.len() {
                0 => "Whole page".to_string(),
                1 => self.doc.node(self.sel[0]).name.clone(),
                n => format!("{n} layers"),
            };
            ui.add(egui::Label::new(RichText::new(sel).small().color(theme::muted(&ctx))).truncate()).on_hover_text("The agent is told what's selected");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if running {
                    if ui.button("Stop").clicked()
                        && let Some(run) = &self.agent.run
                    {
                        run.stop();
                    }
                } else {
                    let can = !self.agent.input.trim().is_empty();
                    let send = ui.add_enabled(can, egui::Button::new("Send").min_size(vec2(56.0, 0.0))).on_hover_text("Send  Enter");
                    if (send.clicked() || enter) && can {
                        let text = std::mem::take(&mut self.agent.input);
                        self.agent_send(&ctx, text);
                        self.agent.focus = true;
                    }
                }
            });
        });
    }
}

/// What to tell the agent at the start of a conversation.
const PREAMBLE: &str = "You're working inside Katachi, a design tool like Figma, through its MCP server `katachi`. \
The person is looking at the file named below and wants you to work on the design itself. Look before you change things: \
get_metadata for the layer tree, get_screenshot to see it. Make changes with execute_code (Figma plugin API JavaScript; \
each call is one undo step), import_svg, and place_image. Put new work where it's visible, next to existing frames rather \
than on top of them. Don't create or edit files on disk unless asked. Keep replies short: say what you changed.";

/// Agents' sign-in failures in words that say what to do.
fn friendly_error(harness: Option<Harness>, msg: &str) -> String {
    let lower = msg.to_ascii_lowercase();
    let signed_out = ["not logged in", "login", "log in", "sign in", "authenticat", "unauthorized", "401", "api key", "token refresh"].iter().any(|k| lower.contains(k));
    match harness {
        Some(h) if signed_out => format!("{msg}\n\n{} isn't signed in. Run `{}` in a terminal once to sign in, then try again.", h.label(), h.program()),
        _ => msg.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_in_failures_say_what_to_do() {
        let e = friendly_error(Some(Harness::OpenCode), "Token refresh failed: 401");
        assert!(e.contains("Run `opencode` in a terminal"), "{e}");
        assert_eq!(friendly_error(Some(Harness::Claude), "usage limit reached"), "usage limit reached");
    }
}
