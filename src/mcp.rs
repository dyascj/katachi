//! MCP server: lets AI agents read and edit the open document, like Figma's Dev Mode MCP server
//! but with write access. Streamable HTTP at http://127.0.0.1:3855/mcp (KATACHI_MCP_PORT to change);
//! `katachi mcp` bridges stdio clients to it. Tool calls run on the UI thread between frames.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

pub const DEFAULT_PORT: u16 = 3855;
const PROTOCOL: &str = "2025-06-18";

pub fn port() -> u16 {
    std::env::var("KATACHI_MCP_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(DEFAULT_PORT)
}

/// A tool call waiting for the UI thread.
pub struct Call {
    /// The tab it's for, when the agent was started from one (`/mcp/<tab>`); otherwise the
    /// tab on screen.
    pub tab: Option<u64>,
    pub name: String,
    pub args: Value,
    pub reply: Sender<Value>,
    /// After this the client has given up, so the call mustn't run (it would run twice when
    /// the client retries).
    pub deadline: Instant,
}

/// How long a client waits for the UI thread to run its call.
const CALL_TIMEOUT: Duration = Duration::from_secs(600);
/// Protocol versions this server speaks; others are answered with the newest.
const VERSIONS: [&str; 3] = ["2024-11-05", "2025-03-26", PROTOCOL];

/// Start listening; tool calls arrive on the returned channel (the UI thread polls it). When
/// another Katachi already has the port, this one listens on a free port instead, so its own
/// Agent panel still reaches its own documents.
pub fn start(ctx: egui::Context, port: u16) -> Option<(u16, Receiver<Call>)> {
    let listener = TcpListener::bind(("127.0.0.1", port)).or_else(|_| TcpListener::bind(("127.0.0.1", 0))).ok()?;
    let port = listener.local_addr().ok()?.port();
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let (tx, ctx) = (tx.clone(), ctx.clone());
            std::thread::spawn(move || {
                let _ = serve(stream, &tx, &ctx);
            });
        }
    });
    Some((port, rx))
}

fn serve(mut stream: TcpStream, tx: &Sender<Call>, ctx: &egui::Context) -> std::io::Result<()> {
    // A client that vanished mustn't hold its thread forever; agents' idle keep-alive
    // connections stay well inside this.
    stream.set_read_timeout(Some(Duration::from_secs(900)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let mut parts = line.split_whitespace();
        let (method, path) = (parts.next().unwrap_or("").to_string(), parts.next().unwrap_or("").to_string());
        let mut len = 0;
        // Web pages can reach localhost too, and so can plugin UIs served from it: only native
        // clients, which send no Origin, with a localhost Host (against DNS rebinding) may drive
        // the document.
        let mut foreign = false;
        loop {
            let mut h = String::new();
            reader.read_line(&mut h)?;
            let h = h.trim();
            if h.is_empty() {
                break;
            }
            if let Some((k, v)) = h.split_once(':')
                && k.eq_ignore_ascii_case("content-length")
            {
                len = v.trim().parse().unwrap_or(0);
            }
            if let Some((k, v)) = h.split_once(':')
                && (k.eq_ignore_ascii_case("origin") || (k.eq_ignore_ascii_case("host") && !local_origin(v.trim())))
            {
                foreign = true;
            }
        }
        // Refuse before reading a body the sender chose the size of.
        if foreign || len > 32 << 20 {
            let status = if foreign { "403 Forbidden" } else { "413 Content Too Large" };
            write!(stream, "HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")?;
            return Ok(());
        }
        let mut body = vec![0; len];
        reader.read_exact(&mut body)?;
        // `/mcp`, or `/mcp/<tab>` for an agent working on one tab.
        let route = path.split('?').next().unwrap_or("").strip_prefix("/mcp").and_then(|rest| match rest {
            "" => Some(None),
            r => r.strip_prefix('/').and_then(|t| t.parse::<u64>().ok()).map(Some),
        });
        let (status, out) = match (method.as_str(), route) {
            ("POST", Some(tab)) => match serde_json::from_slice::<Value>(&body) {
                Ok(Value::Array(batch)) if batch.is_empty() => ("400 Bad Request", error(Value::Null, -32600, "empty batch").to_string()),
                Ok(Value::Array(batch)) => {
                    let replies: Vec<Value> = batch.iter().filter_map(|m| handle(m, tab, tx, ctx)).collect();
                    if replies.is_empty() { ("202 Accepted", String::new()) } else { ("200 OK", Value::Array(replies).to_string()) }
                }
                Ok(m) => match handle(&m, tab, tx, ctx) {
                    Some(r) => ("200 OK", r.to_string()),
                    None => ("202 Accepted", String::new()),
                },
                Err(e) => ("400 Bad Request", error(Value::Null, -32700, &e.to_string()).to_string()),
            },
            ("GET", Some(_)) => ("405 Method Not Allowed", String::new()),
            ("DELETE", Some(_)) => ("200 OK", String::new()),
            _ => ("404 Not Found", String::new()),
        };
        let ctype = if out.is_empty() { "" } else { "Content-Type: application/json\r\n" };
        write!(stream, "HTTP/1.1 {status}\r\n{ctype}Content-Length: {}\r\nMcp-Session-Id: katachi\r\n\r\n{out}", out.len())?;
        stream.flush()?;
    }
}

/// An Origin or Host header naming this machine: `http://localhost:1234`, `127.0.0.1:3855`, `[::1]`.
fn local_origin(v: &str) -> bool {
    let host = v.split_once("://").map_or(v, |(_, h)| h);
    let host = host.split('/').next().unwrap_or("");
    let name = if host.starts_with('[') { host.split(']').next().map(|h| &h[1..]).unwrap_or("") } else { host.split(':').next().unwrap_or("") };
    matches!(name, "localhost" | "127.0.0.1" | "::1")
}

fn error(id: Value, code: i64, msg: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": msg } })
}

/// One JSON-RPC message; notifications get no reply.
fn handle(m: &Value, tab: Option<u64>, tx: &Sender<Call>, ctx: &egui::Context) -> Option<Value> {
    if !m.is_object() {
        return Some(error(Value::Null, -32600, "not a JSON-RPC message"));
    }
    let id = m.get("id")?.clone();
    let ok = |result: Value| json!({ "jsonrpc": "2.0", "id": id.clone(), "result": result });
    Some(match m["method"].as_str().unwrap_or("") {
        "initialize" => ok(json!({
            "protocolVersion": m["params"]["protocolVersion"].as_str().filter(|v| VERSIONS.contains(v)).unwrap_or(PROTOCOL),
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "katachi", "version": env!("CARGO_PKG_VERSION") },
            "instructions": INSTRUCTIONS,
        })),
        "ping" => ok(json!({})),
        "tools/list" => ok(json!({ "tools": tools() })),
        "tools/call" => {
            let (reply, wait) = channel();
            let name = m["params"]["name"].as_str().unwrap_or("").to_string();
            if !tools().iter().any(|t| t["name"] == name.as_str()) {
                return Some(error(id, -32602, &format!("unknown tool {name}")));
            }
            let args = m["params"]["arguments"].clone();
            if tx.send(Call { tab, name, args, reply, deadline: Instant::now() + CALL_TIMEOUT }).is_err() {
                return Some(error(id, -32603, "Katachi is closing"));
            }
            // macOS App Nap would let a backgrounded window sleep through the call.
            let _awake = Awake::new();
            ctx.request_repaint();
            match wait.recv_timeout(CALL_TIMEOUT) {
                Ok(result) => ok(result),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => error(id, -32603, "Katachi stopped while running the tool"),
                Err(_) => error(id, -32603, "Katachi didn't answer in time (is the window open?)"),
            }
        }
        other => error(id, -32601, &format!("unknown method {other}")),
    })
}

/// Keeps macOS from napping the app while a tool call runs (NSProcessInfo activity).
struct Awake(#[cfg(target_os = "macos")] *mut objc2::runtime::AnyObject);

impl Awake {
    fn new() -> Self {
        #[cfg(target_os = "macos")]
        unsafe {
            use objc2::runtime::{AnyClass, AnyObject};
            let Some(pi) = AnyClass::get(c"NSProcessInfo") else { return Self(std::ptr::null_mut()) };
            let Some(ns_string) = AnyClass::get(c"NSString") else { return Self(std::ptr::null_mut()) };
            let info: *mut AnyObject = objc2::msg_send![pi, processInfo];
            let reason: *mut AnyObject = objc2::msg_send![ns_string, stringWithUTF8String: c"MCP tool call".as_ptr()];
            // NSActivityUserInitiatedAllowingIdleSystemSleep
            let activity: *mut AnyObject = objc2::msg_send![info, beginActivityWithOptions: 0x00FF_FFFFu64, reason: reason];
            let _: *mut AnyObject = objc2::msg_send![activity, retain];
            Self(activity)
        }
        #[cfg(not(target_os = "macos"))]
        Self()
    }
}

impl Drop for Awake {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        unsafe {
            use objc2::runtime::{AnyClass, AnyObject};
            if self.0.is_null() {
                return;
            }
            if let Some(pi) = AnyClass::get(c"NSProcessInfo") {
                let info: *mut AnyObject = objc2::msg_send![pi, processInfo];
                let _: () = objc2::msg_send![info, endActivity: self.0];
            }
            let _: () = objc2::msg_send![self.0, release];
        }
    }
}

const INSTRUCTIONS: &str = "Katachi is a Figma-compatible design tool. Start with get_selection and get_metadata to see \
what the person is looking at; get_screenshot shows it, and is how to check your work after changing it. Read more with \
get_code, get_variable_defs, get_components, find_nodes, and get_comments. Edit with execute_code, which runs JavaScript \
against Figma's plugin API (the `figma` global: figma.currentPage, createFrame, createText, auto layout properties, \
components, styles, and variables). Prefer the file's own styles, variables, and components over raw values. Node ids are \
strings; x and y are relative to the parent. Each execute_code call is one undo step; use `return` to send a value back. \
Talk to the person about layers by name, not by id.";

fn tool(name: &str, description: &str, props: Value, required: &[&str]) -> Value {
    json!({ "name": name, "description": description, "inputSchema": { "type": "object", "properties": props, "required": required } })
}

fn tools() -> Vec<Value> {
    let node = json!({ "type": "string", "description": "Layer id; defaults to the selection, or the whole page" });
    vec![
        tool("get_metadata", "Outline of the current page (or one layer): ids, types, names, positions, sizes, and text, as XML.", json!({ "node_id": node }), &[]),
        tool("get_selection", "The selected layers' ids, types, and names.", json!({}), &[]),
        tool("get_screenshot", "A PNG of a layer, the selection, or the page.", json!({ "node_id": node, "scale": { "type": "number", "description": "Pixel density, default 1" } }), &[]),
        tool("get_code", "Code for a layer and its children: CSS (default), Tailwind, SwiftUI, Jetpack Compose, or a React component (inline styles, or Tailwind classes).", json!({ "node_id": node, "language": { "type": "string", "enum": ["css", "tailwind", "swiftui", "compose", "react", "react-tailwind"] } }), &[]),
        tool("get_variable_defs", "Variable collections, modes, and values, plus color, text, and effect styles.", json!({}), &[]),
        tool("get_components", "Main components and component sets with their properties and variants, then enabled libraries' components by key (place one with figma.importComponentByKeyAsync(key) in execute_code).", json!({}), &[]),
        tool(
            "execute_code",
            "Run JavaScript against Figma's plugin API on the current page (async allowed). Changes are one undo step. `return` a JSON-serializable value to get it back; nodes come back as {id, type, name}. Supported: create* (Frame, Text, Rectangle, Ellipse, Line, Polygon, Star, Component, Section), createNodeFromSvg, createInstance, combineAsVariants, group, union/subtract/intersect/exclude; layer properties including auto layout (layoutMode, layoutWrap, itemSpacing, counterAxisSpacing, padding*, primary/counterAxisAlignItems, primary/counterAxisSizingMode, layoutSizingHorizontal/Vertical, layoutGrow, layoutAlign, layoutPositioning, min/max sizes, itemReverseZIndex, grid: layoutMode GRID with gridColumnCount/gridRowCount/gridColumnGap/gridRowGap and gridColumnSpan/gridRowSpan), exportSettings, fills/strokes (solid, gradient, image), effects, corner radii, text (characters, fontName, fontSize, lineHeight, letterSpacing, alignment, textAutoResize, setRange*), constraints; styles (createPaintStyle/TextStyle/EffectStyle, *StyleId); variables (figma.variables.createVariableCollection, createVariable, setValueForMode, setBoundVariable, setBoundVariableForPaint, setExplicitVariableModeForCollection); component properties (addComponentProperty, componentPropertyReferences, setProperties). Text sizes settle after the call returns.",
            json!({ "code": { "type": "string" } }),
            &["code"],
        ),
        tool("list_pages", "Pages in the document, and which one is current.", json!({}), &[]),
        tool("set_page", "Switch to a page by index or name, creating it if `create` is true.", json!({ "page": { "anyOf": [{ "type": "string" }, { "type": "integer" }] }, "create": { "type": "boolean" } }), &["page"]),
        tool("select", "Select layers (and optionally zoom to them).", json!({ "ids": { "type": "array", "items": { "type": "string" } }, "zoom": { "type": "boolean" } }), &["ids"]),
        tool("import_svg", "Add SVG markup as vector layers.", json!({ "svg": { "type": "string" }, "x": { "type": "number" }, "y": { "type": "number" }, "parent_id": { "type": "string" }, "name": { "type": "string" } }), &["svg"]),
        tool("place_image", "Add an image file as a rectangle with an image fill.", json!({ "path": { "type": "string" }, "x": { "type": "number" }, "y": { "type": "number" }, "width": { "type": "number" }, "height": { "type": "number" }, "parent_id": { "type": "string" } }), &["path"]),
        tool("save", "Save the document (to `path` if given, as .katachi).", json!({ "path": { "type": "string" } }), &[]),
        tool("open_file", "Open a .katachi or Figma .fig file (unsaved changes must be saved first).", json!({ "path": { "type": "string" } }), &["path"]),
        tool("undo", "Undo the last change.", json!({}), &[]),
        tool(
            "find_nodes",
            "Find layers on the current page (or inside node_id) by name, text content, and type. Name and text match case-insensitively; `*` is a wildcard.",
            json!({
                "name": { "type": "string" }, "text": { "type": "string" },
                "type": { "type": "string", "description": "frame, group, rectangle, ellipse, text, vector, component, instance, ..." },
                "node_id": node, "limit": { "type": "integer", "description": "At most this many (default 100)" },
            }),
            &[],
        ),
        tool(
            "export",
            "Write layers to image or document files. Uses each layer's export settings unless `format` is given. Returns the files written.",
            json!({
                "node_ids": { "type": "array", "items": { "type": "string" }, "description": "Layers to export; default the selection, or the page" },
                "format": { "type": "string", "enum": ["png", "jpg", "webp", "svg", "pdf"] },
                "size": { "type": "string", "description": "Scale or size, as in Figma: 2x, 0.5x, 512w, 512h (default 1x)" },
                "dir": { "type": "string", "description": "Folder to write into (absolute path)" },
            }),
            &["dir"],
        ),
        tool("get_comments", "Comment threads on the current page: who said what, which layer each is pinned to, and whether it's resolved.", json!({ "all_pages": { "type": "boolean" }, "include_resolved": { "type": "boolean" } }), &[]),
        tool(
            "reply_to_comment",
            "Reply to a comment thread, for example to say what you changed. `resolve` marks the thread resolved.",
            json!({ "comment_id": { "type": "string" }, "text": { "type": "string" }, "resolve": { "type": "boolean" } }),
            &["comment_id"],
        ),
    ]
}

/// Tool result helpers.
pub fn text(s: impl Into<String>) -> Value {
    json!({ "content": [{ "type": "text", "text": s.into() }] })
}

pub fn fail(s: impl Into<String>) -> Value {
    json!({ "content": [{ "type": "text", "text": s.into() }], "isError": true })
}

pub fn image(png: &[u8]) -> Value {
    use base64::Engine;
    json!({ "content": [{ "type": "image", "data": base64::engine::general_purpose::STANDARD.encode(png), "mimeType": "image/png" }] })
}

/// `katachi mcp`: relay newline-delimited JSON-RPC on stdio to the running app.
pub fn stdio_bridge() {
    let port = port();
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        let reply = post(port, &line).unwrap_or_else(|e| {
            let id = serde_json::from_str::<Value>(&line).ok().and_then(|m| m.get("id").cloned());
            match id {
                Some(id) => error(id, -32000, &format!("Can't reach Katachi on port {port} ({e}). Open the Katachi app first.")).to_string(),
                None => String::new(),
            }
        });
        if !reply.is_empty() {
            let _ = writeln!(out, "{reply}");
            let _ = out.flush();
        }
    }
}

fn post(port: u16, body: &str) -> std::io::Result<String> {
    let mut s = TcpStream::connect(("127.0.0.1", port))?;
    write!(s, "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\n\r\n{body}", body.len())?;
    let mut reader = BufReader::new(s);
    let mut len = 0;
    loop {
        let mut h = String::new();
        reader.read_line(&mut h)?;
        let h = h.trim();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':')
            && k.eq_ignore_ascii_case("content-length")
        {
            len = v.trim().parse().unwrap_or(0);
        }
    }
    let mut buf = vec![0; len];
    reader.read_exact(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_local_origins_and_hosts_are_served() {
        for ok in ["http://localhost:5173", "http://127.0.0.1", "127.0.0.1:3855", "localhost", "http://[::1]:80"] {
            assert!(local_origin(ok), "{ok}");
        }
        for bad in ["https://evil.example", "http://localhost.evil.example", "null", "evil.example:3855", "http://127.0.0.1.nip.io"] {
            assert!(!local_origin(bad), "{bad}");
        }
    }

    #[test]
    fn jsonrpc_lifecycle_and_tool_calls_reach_the_ui_thread() {
        let (tx, rx) = channel::<Call>();
        let ctx = egui::Context::default();
        // Stand-in for the UI thread.
        std::thread::spawn(move || {
            for c in rx {
                let _ = c.reply.send(text(format!("{} {}", c.name, c.args["code"].as_str().unwrap_or(""))));
            }
        });
        let init = handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-03-26" } }), None, &tx, &ctx).unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26", "echoes the client's version");
        assert!(handle(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }), None, &tx, &ctx).is_none(), "notifications get no reply");
        let list = handle(&json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }), None, &tx, &ctx).unwrap();
        assert!(list["result"]["tools"].as_array().unwrap().iter().any(|t| t["name"] == "execute_code"));
        let call = handle(&json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "execute_code", "arguments": { "code": "return 1" } } }), None, &tx, &ctx).unwrap();
        assert_eq!(call["result"]["content"][0]["text"], "execute_code return 1");
        assert_eq!(handle(&json!({ "jsonrpc": "2.0", "id": 4, "method": "nope" }), None, &tx, &ctx).unwrap()["error"]["code"], -32601);
        let unknown = handle(&json!({ "jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": { "name": "rm_rf" } }), None, &tx, &ctx).unwrap();
        assert_eq!(unknown["error"]["code"], -32602, "unknown tools are protocol errors");
        assert_eq!(handle(&json!(42), None, &tx, &ctx).unwrap()["error"]["code"], -32600);
        let future = handle(&json!({ "jsonrpc": "2.0", "id": 6, "method": "initialize", "params": { "protocolVersion": "2099-01-01" } }), None, &tx, &ctx).unwrap();
        assert_eq!(future["result"]["protocolVersion"], PROTOCOL, "unknown versions get ours");
    }

    /// Over real HTTP: browsers (any Origin, even localhost plugin UIs) are refused before the
    /// body is read, and a tab route reaches the UI thread with its tab.
    #[test]
    fn http_refuses_browsers_and_routes_tabs() {
        let ctx = egui::Context::default();
        let (port, rx) = start(ctx, 0).unwrap();
        std::thread::spawn(move || {
            for c in rx {
                let _ = c.reply.send(text(format!("{:?}", c.tab)));
            }
        });
        let post = |path: &str, extra: &str| {
            let body = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_pages","arguments":{}}}"#;
            let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(s, "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{extra}Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
            let mut out = String::new();
            let mut r = BufReader::new(s);
            r.read_line(&mut out).unwrap();
            let mut len = 0;
            loop {
                let mut h = String::new();
                if r.read_line(&mut h).unwrap() == 0 || h.trim().is_empty() {
                    break;
                }
                if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; len];
            r.read_exact(&mut body).unwrap();
            out + &String::from_utf8_lossy(&body)
        };
        assert!(post("/mcp", "").contains("None"));
        assert!(post("/mcp/42", "").contains("Some(42)"));
        assert!(post("/mcp", "Origin: http://127.0.0.1:5555\r\n").contains("403"));
        assert!(post("/mcp/x", "").contains("404"));
    }
}
