//! Agents: run the coding agents installed on this machine (Claude Code, Codex, Gemini CLI,
//! OpenCode, Cursor) headless against Katachi's MCP server and stream what they do into the
//! Agent panel. Each agent keeps its own sign-in, model, and session history; Katachi starts it,
//! hands it the prompt, and reads its JSON event stream.

use serde_json::{Value, json};
use std::collections::VecDeque;
use std::ffi::{OsStr, OsString};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Harness {
    Claude,
    Codex,
    Gemini,
    OpenCode,
    Cursor,
}

impl Harness {
    pub const ALL: [Harness; 5] = [Harness::Claude, Harness::Codex, Harness::Gemini, Harness::OpenCode, Harness::Cursor];

    pub fn label(self) -> &'static str {
        match self {
            Harness::Claude => "Claude Code",
            Harness::Codex => "Codex",
            Harness::Gemini => "Gemini CLI",
            Harness::OpenCode => "OpenCode",
            Harness::Cursor => "Cursor",
        }
    }

    /// The command it installs.
    pub fn program(self) -> &'static str {
        match self {
            Harness::Claude => "claude",
            Harness::Codex => "codex",
            Harness::Gemini => "gemini",
            Harness::OpenCode => "opencode",
            Harness::Cursor => "cursor-agent",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Installed {
    pub harness: Harness,
    pub exe: PathBuf,
}

/// What detection found: the agents, and the PATH to run them with.
pub struct Found {
    pub agents: Vec<Installed>,
    pub path: OsString,
}

static FOUND: OnceLock<Mutex<Option<Arc<Found>>>> = OnceLock::new();
static SCANNING: AtomicBool = AtomicBool::new(false);

/// The installed agents, once a background scan has found them. The first call starts the scan;
/// `rescan` starts another (after installing one).
pub fn found(ctx: &egui::Context) -> Option<Arc<Found>> {
    let have = found_now();
    if have.is_none() {
        rescan(ctx);
    }
    have
}

/// What the last scan found, without starting one.
pub fn found_now() -> Option<Arc<Found>> {
    FOUND.get_or_init(|| Mutex::new(None)).lock().unwrap().clone()
}

/// Tests: pretend these were found.
#[cfg(test)]
pub fn set_found(found: Found) {
    *FOUND.get_or_init(|| Mutex::new(None)).lock().unwrap() = Some(Arc::new(found));
}

pub fn rescan(ctx: &egui::Context) {
    if SCANNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let path = search_path();
        let agents = Harness::ALL.iter().filter_map(|&harness| which(harness.program(), &path).map(|exe| Installed { harness, exe })).collect();
        *FOUND.get_or_init(|| Mutex::new(None)).lock().unwrap() = Some(Arc::new(Found { agents, path }));
        SCANNING.store(false, Ordering::SeqCst);
        ctx.request_repaint();
    });
}

pub fn scanning() -> bool {
    SCANNING.load(Ordering::SeqCst)
}

/// The PATH agents are found and run with. Apps opened from the Dock or an app menu don't get
/// the PATH your terminal has, and agents installed with npm, Homebrew, or their own installers
/// live in its folders (and need `node` from it), so ask the login shell first.
fn search_path() -> OsString {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut add = |d: PathBuf| {
        if !d.as_os_str().is_empty() && !dirs.contains(&d) {
            dirs.push(d);
        }
    };
    #[cfg(unix)]
    if let Some(p) = login_shell_path() {
        std::env::split_paths(&p).for_each(&mut add);
    }
    if let Some(p) = std::env::var_os("PATH") {
        std::env::split_paths(&p).for_each(&mut add);
    }
    // Where installers put agents, in case the shell didn't say.
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    if let Some(home) = &home {
        for d in [".local/bin", ".npm-global/bin", ".bun/bin", ".volta/bin", ".opencode/bin", ".cargo/bin"] {
            add(home.join(d));
        }
    }
    if cfg!(windows) {
        if let Some(app_data) = std::env::var_os("APPDATA") {
            add(PathBuf::from(app_data).join("npm"));
        }
    } else {
        for d in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"] {
            add(PathBuf::from(d));
        }
    }
    std::env::join_paths(dirs).unwrap_or_default()
}

/// `$PATH` as an interactive login shell sets it (nvm and friends load in rc files).
#[cfg(unix)]
fn login_shell_path() -> Option<OsString> {
    use std::io::Read;
    use std::time::{Duration, Instant};
    const MARK: &str = "__KATACHI_PATH__";
    let shell = std::env::var_os("SHELL").unwrap_or_else(|| "/bin/sh".into());
    let mut child = Command::new(shell)
        .args(["-l", "-i", "-c", &format!("printf '\\n{MARK}%s\\n' \"$PATH\"")])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    // An rc file that waits for input must not hang detection.
    let deadline = Instant::now() + Duration::from_secs(5);
    while child.try_wait().ok()?.is_none() {
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let mut out = String::new();
    child.stdout.take()?.read_to_string(&mut out).ok()?;
    out.lines().find_map(|l| l.strip_prefix(MARK)).filter(|p| !p.is_empty()).map(OsString::from)
}

/// The first `program` on `path`, with Windows' executable extensions (npm installs `.cmd` shims).
fn which(program: &str, path: &OsStr) -> Option<PathBuf> {
    let exts: Vec<String> = if cfg!(windows) {
        let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
        pathext.split(';').filter(|e| !e.is_empty()).map(str::to_ascii_lowercase).collect()
    } else {
        vec![String::new()]
    };
    std::env::split_paths(path).flat_map(|d| exts.iter().map(move |e| d.join(format!("{program}{e}")))).find(|p| executable(p))
}

fn executable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        p.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

/// One turn of a conversation.
pub struct Request<'a> {
    pub prompt: &'a str,
    /// The agent's session to continue, from an earlier turn's `Event::Session`.
    pub session: Option<&'a str>,
    pub model: Option<&'a str>,
    pub mcp_url: &'a str,
    /// Where the agent works: the file's folder, or Katachi's own scratch folder.
    pub cwd: &'a Path,
    pub path: &'a OsStr,
}

/// What an agent did, normalized across agents.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The agent's session id, to continue the conversation next turn.
    Session(String),
    /// Reply text. `delta` text continues the previous chunk; otherwise it's a new paragraph.
    Text { text: String, delta: bool },
    /// It called a tool (a short description).
    Tool(String),
    Error(String),
    /// The process ended.
    Exit { ok: bool },
}

/// The MCP server name agents see, so tools show up as `katachi` tools.
const SERVER: &str = "katachi";

/// The command line for one turn, and the prompt to write to stdin (`None`: it's an argument).
/// Prompts go through stdin where the agent supports it: no length limits, and no quoting
/// rules for Windows' `.cmd` shims.
fn command(agent: &Installed, req: &Request, scratch: &Path) -> std::io::Result<(Command, Option<String>)> {
    let mut cmd = agent_command(&agent.exe, req.path);
    let url = req.mcp_url;
    let stdin = match agent.harness {
        Harness::Claude => {
            let mcp = json!({ "mcpServers": { SERVER: { "type": "http", "url": url } } });
            cmd.args(["-p", "--output-format", "stream-json", "--verbose"]);
            // `=` keeps these variadic options from swallowing the next arguments.
            cmd.arg(format!("--mcp-config={mcp}")).arg(format!("--allowedTools=mcp__{SERVER}"));
            if let Some(s) = req.session {
                cmd.args(["--resume", s]);
            }
            if let Some(m) = req.model {
                cmd.args(["--model", m]);
            }
            Some(req.prompt.to_string())
        }
        Harness::Codex => {
            cmd.arg("exec");
            if let Some(s) = req.session {
                cmd.args(["resume", s]);
            }
            cmd.args(["--json", "--skip-git-repo-check", "-c", &format!("mcp_servers.{SERVER}.url={}", toml_string(url))]);
            // Headless Codex can't ask before each tool call; Katachi's tools are the point.
            cmd.args(["-c", &format!("mcp_servers.{SERVER}.default_tools_approval_mode=\"approve\"")]);
            if let Some(m) = req.model {
                cmd.args(["-m", m]);
            }
            cmd.arg("-");
            Some(req.prompt.to_string())
        }
        Harness::Gemini => {
            // Gemini reads MCP servers only from settings files; a system settings file of our own
            // adds Katachi without touching the person's settings.
            let settings = scratch.join("gemini-settings.json");
            let cfg = json!({ "mcpServers": { SERVER: { "httpUrl": url, "trust": true } } });
            std::fs::write(&settings, cfg.to_string())?;
            cmd.env("GEMINI_CLI_SYSTEM_SETTINGS_PATH", &settings);
            cmd.args(["-o", "stream-json"]);
            if let Some(s) = req.session {
                cmd.args(["--resume", s]);
            }
            if let Some(m) = req.model {
                cmd.args(["-m", m]);
            }
            cmd.args(["-p", ""]);
            Some(req.prompt.to_string())
        }
        Harness::OpenCode => {
            let cfg = json!({ "mcp": { SERVER: { "type": "remote", "url": url, "enabled": true } } });
            cmd.env("OPENCODE_CONFIG_CONTENT", cfg.to_string());
            cmd.args(["run", "--format", "json"]);
            if let Some(s) = req.session {
                cmd.args(["--session", s]);
            }
            if let Some(m) = req.model {
                cmd.args(["-m", m]);
            }
            cmd.arg(req.prompt);
            None
        }
        Harness::Cursor => {
            // Cursor reads MCP servers from the project's .cursor/mcp.json, so it works in a
            // folder of ours that has one.
            let dir = scratch.join("cursor");
            std::fs::create_dir_all(dir.join(".cursor"))?;
            let cfg = json!({ "mcpServers": { SERVER: { "url": url } } });
            std::fs::write(dir.join(".cursor/mcp.json"), cfg.to_string())?;
            cmd.current_dir(dir);
            cmd.args(["-p", "--output-format", "stream-json", "--force"]);
            if let Some(s) = req.session {
                cmd.args(["--resume", s]);
            }
            if let Some(m) = req.model {
                cmd.args(["--model", m]);
            }
            cmd.arg(req.prompt);
            None
        }
    };
    Ok((cmd, stdin))
}

/// A command for an agent: on its PATH, without a console window, and not refused for nesting.
fn agent_command(exe: &Path, path: &OsStr) -> Command {
    let mut cmd = Command::new(exe);
    // Started from inside a Claude Code session, Claude would refuse to nest.
    cmd.env("PATH", path).env_remove("CLAUDECODE");
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x0800_0000);
    cmd
}

/// The agent's own commands to remove and add Katachi as an MCP server, for using it outside
/// Katachi. OpenCode and Cursor have no such command.
fn server_commands(h: Harness, url: &str) -> Option<[Vec<&str>; 2]> {
    Some(match h {
        Harness::Claude => [vec!["mcp", "remove", "--scope", "user", SERVER], vec!["mcp", "add", "--scope", "user", "--transport", "http", SERVER, url]],
        Harness::Codex => [vec!["mcp", "remove", SERVER], vec!["mcp", "add", SERVER, "--url", url]],
        Harness::Gemini => [vec!["mcp", "remove", "--scope", "user", SERVER], vec!["mcp", "add", "--scope", "user", "--transport", "http", "--trust", SERVER, url]],
        Harness::OpenCode | Harness::Cursor => return None,
    })
}

pub fn can_connect(h: Harness) -> bool {
    server_commands(h, "").is_some()
}

/// Add Katachi to the agent's own MCP servers (replacing an older entry), off the UI thread.
/// The outcome lands in egui's temporary data under `id`.
pub fn connect(agent: &Installed, path: &OsStr, url: &str, ctx: &egui::Context, id: egui::Id) {
    let Some([remove, add]) = server_commands(agent.harness, url) else { return };
    let (remove, add): (Vec<String>, Vec<String>) = (remove.into_iter().map(String::from).collect(), add.into_iter().map(String::from).collect());
    let (exe, path, ctx) = (agent.exe.clone(), path.to_os_string(), ctx.clone());
    ctx.data_mut(|d| d.insert_temp(id, "Adding...".to_string()));
    std::thread::spawn(move || {
        let _ = agent_command(&exe, &path).args(&remove).stdin(Stdio::null()).output();
        let msg = match agent_command(&exe, &path).args(&add).stdin(Stdio::null()).output() {
            Ok(o) if o.status.success() => "Added".to_string(),
            Ok(o) => String::from_utf8_lossy(&o.stderr).lines().chain(String::from_utf8_lossy(&o.stdout).lines()).find(|l| !l.trim().is_empty()).unwrap_or("Failed").to_string(),
            Err(e) => e.to_string(),
        };
        ctx.data_mut(|d| d.insert_temp(id, msg));
        ctx.request_repaint();
    });
}

fn toml_string(s: &str) -> String {
    Value::String(s.into()).to_string()
}

/// A running turn. Dropping it stops the agent.
pub struct Run {
    pid: u32,
    done: Arc<AtomicBool>,
    pub events: Receiver<Event>,
}

impl Run {
    /// Start a turn; `ctx` repaints as events arrive.
    pub fn start(agent: &Installed, req: &Request, ctx: &egui::Context) -> std::io::Result<Run> {
        let scratch = crate::files::data_dir().join("agent");
        std::fs::create_dir_all(&scratch)?;
        let (mut cmd, stdin) = command(agent, req, &scratch)?;
        if agent.harness != Harness::Cursor {
            cmd.current_dir(req.cwd);
        }
        cmd.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // Its own process group, so Stop ends the agent and everything it started.
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
        let mut child = cmd.spawn()?;
        if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
            std::thread::spawn(move || {
                let _ = pipe.write_all(text.as_bytes());
            });
        }
        let (tx, rx) = channel();
        let tail = Arc::new(Mutex::new(VecDeque::new()));
        if let Some(err) = child.stderr.take() {
            let tail = tail.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(err).lines().map_while(Result::ok) {
                    let mut t = tail.lock().unwrap();
                    if t.len() == 8 {
                        t.pop_front();
                    }
                    t.push_back(line);
                }
            });
        }
        let (harness, done, pid) = (agent.harness, Arc::new(AtomicBool::new(false)), child.id());
        let out = child.stdout.take();
        let (finished, ctx) = (done.clone(), ctx.clone());
        std::thread::spawn(move || {
            let mut errored = false;
            if let Some(out) = out {
                let mut reader = BufReader::new(out);
                let mut line = Vec::new();
                while reader.read_until(b'\n', &mut line).is_ok_and(|n| n > 0) {
                    for e in parse(harness, &String::from_utf8_lossy(&line)) {
                        errored |= matches!(e, Event::Error(_));
                        if tx.send(e).is_err() {
                            break;
                        }
                        ctx.request_repaint();
                    }
                    line.clear();
                }
            }
            let ok = child.wait().is_ok_and(|s| s.success());
            finished.store(true, Ordering::SeqCst);
            if !ok && !errored {
                let tail: Vec<String> = tail.lock().unwrap().iter().filter(|l| !l.trim().is_empty()).cloned().collect();
                let msg = if tail.is_empty() { format!("{} stopped without an answer.", harness.label()) } else { tail.join("\n") };
                let _ = tx.send(Event::Error(msg));
            }
            let _ = tx.send(Event::Exit { ok: ok && !errored });
            ctx.request_repaint();
        });
        Ok(Run { pid, done, events: rx })
    }

    pub fn running(&self) -> bool {
        !self.done.load(Ordering::SeqCst)
    }

    /// End the agent and whatever it started.
    pub fn stop(&self) {
        if !self.running() {
            return;
        }
        let pid = self.pid.to_string();
        #[cfg(unix)]
        let mut kill = {
            let mut c = Command::new("kill");
            c.args(["-TERM", &format!("-{pid}")]);
            c
        };
        #[cfg(windows)]
        let mut kill = {
            let mut c = Command::new("taskkill");
            c.args(["/T", "/F", "/PID", &pid]);
            std::os::windows::process::CommandExt::creation_flags(&mut c, 0x0800_0000);
            c
        };
        let _ = kill.stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        self.stop();
    }
}

/// One line of an agent's JSON output, as events. Lines that aren't JSON (logs) are skipped.
pub fn parse(harness: Harness, line: &str) -> Vec<Event> {
    let Ok(v) = serde_json::from_str::<Value>(line.trim()) else {
        return vec![];
    };
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
    let kind = v.get("type").and_then(Value::as_str).unwrap_or("");
    let mut out = vec![];
    match harness {
        // Cursor's stream-json follows Claude Code's.
        Harness::Claude | Harness::Cursor => match kind {
            "system" if s(&v, "subtype").as_deref() == Some("init") => out.extend(s(&v, "session_id").map(Event::Session)),
            "assistant" => {
                for block in v.pointer("/message/content").and_then(Value::as_array).into_iter().flatten() {
                    match block.get("type").and_then(Value::as_str) {
                        Some("text") => out.extend(s(block, "text").filter(|t| !t.trim().is_empty()).map(|text| Event::Text { text, delta: false })),
                        Some("tool_use") => out.extend(s(block, "name").map(|n| Event::Tool(describe(&n)))),
                        _ => {}
                    }
                }
            }
            "tool_call" if s(&v, "subtype").as_deref() == Some("started") => {
                let name = v.get("tool_call").and_then(Value::as_object).and_then(|o| o.iter().next()).map(|(k, call)| {
                    call.pointer("/args/name").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| k.trim_end_matches("ToolCall").to_string())
                });
                out.extend(name.map(|n| Event::Tool(describe(&n))));
            }
            "result" => {
                let failed = v.get("is_error").and_then(Value::as_bool).unwrap_or(false) || s(&v, "subtype").is_some_and(|t| t != "success");
                if failed {
                    let msg = s(&v, "result").filter(|r| !r.is_empty()).or_else(|| s(&v, "subtype")).unwrap_or_else(|| "The agent stopped with an error.".into());
                    out.push(Event::Error(msg));
                }
            }
            _ => {}
        },
        Harness::Codex => match kind {
            "thread.started" => out.extend(s(&v, "thread_id").map(Event::Session)),
            "item.started" | "item.completed" => {
                let item = &v["item"];
                match (kind, item.get("type").and_then(Value::as_str)) {
                    ("item.completed", Some("agent_message")) => out.extend(s(item, "text").map(|text| Event::Text { text, delta: false })),
                    ("item.started", Some("mcp_tool_call")) => out.extend(s(item, "tool").map(|n| Event::Tool(describe(&n)))),
                    ("item.started", Some("command_execution")) => out.extend(s(item, "command").map(|c| Event::Tool(format!("Ran {c}")))),
                    ("item.started", Some("web_search")) => out.push(Event::Tool("Searched the web".into())),
                    _ => {}
                }
            }
            "turn.failed" => out.push(Event::Error(v.pointer("/error/message").and_then(Value::as_str).unwrap_or("The turn failed.").to_string())),
            "error" => out.extend(s(&v, "message").map(Event::Error)),
            _ => {}
        },
        Harness::Gemini => match kind {
            "init" => out.extend(s(&v, "session_id").map(Event::Session)),
            "message" if s(&v, "role").as_deref() == Some("assistant") => {
                let delta = v.get("delta").and_then(Value::as_bool).unwrap_or(false);
                out.extend(s(&v, "content").map(|text| Event::Text { text, delta }));
            }
            "tool_use" => out.extend(s(&v, "tool_name").map(|n| Event::Tool(describe(&n)))),
            "error" => out.extend(s(&v, "message").map(Event::Error)),
            "result" if s(&v, "status").is_some_and(|st| st != "success") => {
                out.push(Event::Error(v.pointer("/error/message").and_then(Value::as_str).unwrap_or("The agent stopped with an error.").to_string()));
            }
            _ => {}
        },
        Harness::OpenCode => {
            out.extend(s(&v, "sessionID").map(Event::Session));
            match kind {
                "text" => out.extend(v.pointer("/part/text").and_then(Value::as_str).map(|t| Event::Text { text: t.to_string(), delta: false })),
                "tool_use" => out.extend(v.pointer("/part/tool").and_then(Value::as_str).map(|n| Event::Tool(describe(n)))),
                "error" => {
                    let e = &v["error"];
                    let msg = e.pointer("/data/message").and_then(Value::as_str).or_else(|| e.get("name").and_then(Value::as_str)).unwrap_or("The agent stopped with an error.");
                    out.push(Event::Error(msg.to_string()));
                }
                _ => {}
            }
        }
    }
    out
}

/// A tool call as the panel shows it: Katachi's own tools in words, others by name.
fn describe(name: &str) -> String {
    let short = name.strip_prefix("mcp__katachi__").or_else(|| name.strip_prefix("katachi__")).or_else(|| name.strip_prefix("katachi_")).unwrap_or(name);
    match short {
        "get_metadata" => "Read the layers".into(),
        "get_selection" => "Checked the selection".into(),
        "get_screenshot" => "Looked at the canvas".into(),
        "get_code" => "Read the code".into(),
        "get_variable_defs" => "Read variables and styles".into(),
        "get_components" => "Read the components".into(),
        "execute_code" => "Edited the design".into(),
        "list_pages" => "Listed the pages".into(),
        "set_page" => "Switched pages".into(),
        "select" => "Selected layers".into(),
        "import_svg" => "Imported SVG".into(),
        "place_image" => "Placed an image".into(),
        "save" => "Saved the file".into(),
        "undo" => "Undid a change".into(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(t: &str) -> Event {
        Event::Text { text: t.into(), delta: false }
    }

    #[test]
    fn claude_stream_json() {
        let h = Harness::Claude;
        assert_eq!(parse(h, r#"{"type":"system","subtype":"init","session_id":"s1","tools":[]}"#), [Event::Session("s1".into())]);
        assert_eq!(parse(h, r#"{"type":"system","subtype":"hook_started","session_id":"s1"}"#), []);
        let msg = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Making it blue"},{"type":"tool_use","name":"mcp__katachi__execute_code","input":{}}]}}"#;
        assert_eq!(parse(h, msg), [text("Making it blue"), Event::Tool("Edited the design".into())]);
        assert_eq!(parse(h, r#"{"type":"result","subtype":"success","is_error":false,"result":"done"}"#), []);
        assert_eq!(parse(h, r#"{"type":"result","subtype":"success","is_error":true,"result":"Not logged in"}"#), [Event::Error("Not logged in".into())]);
        assert_eq!(parse(h, "not json"), []);
    }

    #[test]
    fn codex_jsonl() {
        let h = Harness::Codex;
        assert_eq!(parse(h, r#"{"type":"thread.started","thread_id":"t1"}"#), [Event::Session("t1".into())]);
        assert_eq!(parse(h, r#"{"type":"item.completed","item":{"id":"i","type":"agent_message","text":"hi"}}"#), [text("hi")]);
        assert_eq!(parse(h, r#"{"type":"item.started","item":{"type":"mcp_tool_call","server":"katachi","tool":"get_screenshot"}}"#), [Event::Tool("Looked at the canvas".into())]);
        // Config warnings come as error items; they aren't failures.
        assert_eq!(parse(h, r#"{"type":"item.completed","item":{"type":"error","message":"ignoring a setting"}}"#), []);
        assert_eq!(parse(h, r#"{"type":"turn.failed","error":{"message":"usage limit"}}"#), [Event::Error("usage limit".into())]);
    }

    #[test]
    fn gemini_opencode_and_cursor() {
        assert_eq!(parse(Harness::Gemini, r#"{"type":"init","session_id":"g1","model":"m"}"#), [Event::Session("g1".into())]);
        assert_eq!(parse(Harness::Gemini, r#"{"type":"message","role":"assistant","content":"Hel","delta":true}"#), [Event::Text { text: "Hel".into(), delta: true }]);
        assert_eq!(parse(Harness::Gemini, r#"{"type":"message","role":"user","content":"prompt"}"#), []);
        assert_eq!(parse(Harness::Gemini, r#"{"type":"tool_use","tool_name":"execute_code","tool_id":"1"}"#), [Event::Tool("Edited the design".into())]);
        assert_eq!(parse(Harness::OpenCode, r#"{"type":"text","sessionID":"o1","part":{"type":"text","text":"ok"}}"#), [Event::Session("o1".into()), text("ok")]);
        assert_eq!(
            parse(Harness::OpenCode, r#"{"type":"error","sessionID":"o1","error":{"name":"UnknownError","data":{"message":"Token refresh failed: 401"}}}"#),
            [Event::Session("o1".into()), Event::Error("Token refresh failed: 401".into())]
        );
        assert_eq!(
            parse(Harness::Cursor, r#"{"type":"tool_call","subtype":"started","tool_call":{"mcpToolCall":{"args":{"name":"katachi-get_metadata"}}}}"#),
            [Event::Tool("katachi-get_metadata".into())]
        );
    }

    #[test]
    fn finds_programs_on_a_path() {
        let dir = std::env::temp_dir().join(format!("katachi-which-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let name = if cfg!(windows) { "fake-agent.cmd" } else { "fake-agent" };
        let exe = dir.join(name);
        std::fs::write(&exe, "").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(which("fake-agent", dir.as_os_str()), None, "not executable yet");
            std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let path = std::env::join_paths([PathBuf::from("/nonexistent"), dir.clone()]).unwrap();
        assert_eq!(which("fake-agent", &path), Some(exe));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn prompts_go_through_stdin_and_mcp_config_names_katachi() {
        let agent = Installed { harness: Harness::Claude, exe: "claude".into() };
        let scratch = std::env::temp_dir();
        let req = Request { prompt: "make it \"blue\"\n& red", session: Some("s1"), model: None, mcp_url: "http://127.0.0.1:3855/mcp", cwd: &scratch, path: OsStr::new("") };
        let (cmd, stdin) = command(&agent, &req, &scratch).unwrap();
        let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(stdin.as_deref(), Some("make it \"blue\"\n& red"));
        assert!(args.iter().any(|a| a.starts_with("--mcp-config=") && a.contains("http://127.0.0.1:3855/mcp")), "{args:?}");
        assert!(args.contains(&"--allowedTools=mcp__katachi".to_string()));
        assert!(args.windows(2).any(|w| w == ["--resume", "s1"]));
        let codex = Installed { harness: Harness::Codex, exe: "codex".into() };
        let (cmd, _) = command(&codex, &req, &scratch).unwrap();
        let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(args[..3], ["exec", "resume", "s1"]);
        assert!(args.contains(&r#"mcp_servers.katachi.url="http://127.0.0.1:3855/mcp""#.to_string()));
        assert_eq!(args.last().map(String::as_str), Some("-"));
    }
}
