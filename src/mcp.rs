//! MCP server: lets AI agents read and edit the open document, like Figma's Dev Mode MCP server
//! but with write access. Streamable HTTP at http://127.0.0.1:3855/mcp (KATACHI_MCP_PORT to change);
//! `katachi mcp` bridges stdio clients to it. Tool calls run on the UI thread between frames.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

pub const DEFAULT_PORT: u16 = 3855;
const PROTOCOL: &str = "2025-06-18";

pub fn port() -> u16 {
    std::env::var("KATACHI_MCP_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(DEFAULT_PORT)
}

/// A tool call waiting for the UI thread.
pub struct Call {
    pub name: String,
    pub args: Value,
    pub reply: Sender<Value>,
}

/// Start listening; tool calls arrive on the returned channel (the UI thread polls it).
pub fn start(ctx: egui::Context) -> Option<Receiver<Call>> {
    let listener = TcpListener::bind(("127.0.0.1", port())).ok()?;
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let (tx, ctx) = (tx.clone(), ctx.clone());
            std::thread::spawn(move || {
                let _ = serve(stream, &tx, &ctx);
            });
        }
    });
    Some(rx)
}

fn serve(mut stream: TcpStream, tx: &Sender<Call>, ctx: &egui::Context) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let mut parts = line.split_whitespace();
        let (method, path) = (parts.next().unwrap_or("").to_string(), parts.next().unwrap_or("").to_string());
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
        let mut body = vec![0; len];
        reader.read_exact(&mut body)?;
        let (status, out) = match (method.as_str(), path.split('?').next().unwrap_or("")) {
            ("POST", "/mcp") => match serde_json::from_slice::<Value>(&body) {
                Ok(Value::Array(batch)) => {
                    let replies: Vec<Value> = batch.iter().filter_map(|m| handle(m, tx, ctx)).collect();
                    if replies.is_empty() { ("202 Accepted", String::new()) } else { ("200 OK", Value::Array(replies).to_string()) }
                }
                Ok(m) => match handle(&m, tx, ctx) {
                    Some(r) => ("200 OK", r.to_string()),
                    None => ("202 Accepted", String::new()),
                },
                Err(e) => ("400 Bad Request", error(Value::Null, -32700, &e.to_string()).to_string()),
            },
            ("GET", "/mcp") => ("405 Method Not Allowed", String::new()),
            ("DELETE", "/mcp") => ("200 OK", String::new()),
            _ => ("404 Not Found", String::new()),
        };
        let ctype = if out.is_empty() { "" } else { "Content-Type: application/json\r\n" };
        write!(stream, "HTTP/1.1 {status}\r\n{ctype}Content-Length: {}\r\nMcp-Session-Id: katachi\r\n\r\n{out}", out.len())?;
        stream.flush()?;
    }
}

fn error(id: Value, code: i64, msg: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": msg } })
}

/// One JSON-RPC message; notifications get no reply.
fn handle(m: &Value, tx: &Sender<Call>, ctx: &egui::Context) -> Option<Value> {
    let id = m.get("id")?.clone();
    let ok = |result: Value| json!({ "jsonrpc": "2.0", "id": id.clone(), "result": result });
    Some(match m["method"].as_str().unwrap_or("") {
        "initialize" => ok(json!({
            "protocolVersion": m["params"]["protocolVersion"].as_str().unwrap_or(PROTOCOL),
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "katachi", "version": env!("CARGO_PKG_VERSION") },
            "instructions": INSTRUCTIONS,
        })),
        "ping" => ok(json!({})),
        "tools/list" => ok(json!({ "tools": tools() })),
        "tools/call" => {
            let (reply, wait) = channel();
            let name = m["params"]["name"].as_str().unwrap_or("").to_string();
            let args = m["params"]["arguments"].clone();
            if tx.send(Call { name, args, reply }).is_err() {
                return Some(error(id, -32603, "Katachi is closing"));
            }
            ctx.request_repaint();
            match wait.recv_timeout(Duration::from_secs(120)) {
                Ok(result) => ok(result),
                Err(_) => error(id, -32603, "Katachi didn't answer in time (is the window open?)"),
            }
        }
        other => error(id, -32601, &format!("unknown method {other}")),
    })
}

const INSTRUCTIONS: &str = "Katachi is a Figma-compatible design tool. Read the open document with get_metadata, \
get_screenshot, get_code, get_variable_defs, and get_components. Edit it with execute_code, which runs JavaScript \
against Figma's plugin API (the `figma` global: figma.currentPage, createFrame, createText, auto layout properties, \
components, styles, and variables). Node ids are strings; x and y are relative to the parent. Each execute_code call \
is one undo step. Use `return` to send a value back.";

fn tool(name: &str, description: &str, props: Value, required: &[&str]) -> Value {
    json!({ "name": name, "description": description, "inputSchema": { "type": "object", "properties": props, "required": required } })
}

fn tools() -> Vec<Value> {
    let node = json!({ "type": "string", "description": "Layer id; defaults to the selection, or the whole page" });
    vec![
        tool("get_metadata", "Outline of the current page (or one layer): ids, types, names, positions, sizes, and text, as XML.", json!({ "node_id": node }), &[]),
        tool("get_selection", "The selected layers' ids, types, and names.", json!({}), &[]),
        tool("get_screenshot", "A PNG of a layer, the selection, or the page.", json!({ "node_id": node, "scale": { "type": "number", "description": "Pixel density, default 1" } }), &[]),
        tool("get_code", "Code for a layer and its children: CSS (default), SwiftUI, or Jetpack Compose.", json!({ "node_id": node, "language": { "type": "string", "enum": ["css", "swiftui", "compose"] } }), &[]),
        tool("get_variable_defs", "Variable collections, modes, and values, plus color, text, and effect styles.", json!({}), &[]),
        tool("get_components", "Main components and component sets with their properties and variants.", json!({}), &[]),
        tool(
            "execute_code",
            "Run JavaScript against Figma's plugin API on the current page (async allowed). Changes are one undo step. `return` a JSON-serializable value to get it back; nodes come back as {id, type, name}. Supported: create* (Frame, Text, Rectangle, Ellipse, Line, Polygon, Star, Component, Section), createNodeFromSvg, createInstance, combineAsVariants, group, union/subtract/intersect/exclude; layer properties including auto layout (layoutMode, layoutWrap, itemSpacing, counterAxisSpacing, padding*, primary/counterAxisAlignItems, primary/counterAxisSizingMode, layoutSizingHorizontal/Vertical, layoutGrow, layoutAlign, layoutPositioning, min/max sizes), fills/strokes (solid, gradient, image), effects, corner radii, text (characters, fontName, fontSize, lineHeight, letterSpacing, alignment, textAutoResize, setRange*), constraints; styles (createPaintStyle/TextStyle/EffectStyle, *StyleId); variables (figma.variables.createVariableCollection, createVariable, setValueForMode, setBoundVariable, setBoundVariableForPaint, setExplicitVariableModeForCollection); component properties (addComponentProperty, componentPropertyReferences, setProperties). Text sizes settle after the call returns.",
            json!({ "code": { "type": "string" } }),
            &["code"],
        ),
        tool("list_pages", "Pages in the document, and which one is current.", json!({}), &[]),
        tool("set_page", "Switch to a page by index or name, creating it if `create` is true.", json!({ "page": { "type": ["string", "integer"] }, "create": { "type": "boolean" } }), &["page"]),
        tool("select", "Select layers (and optionally zoom to them).", json!({ "ids": { "type": "array", "items": { "type": "string" } }, "zoom": { "type": "boolean" } }), &["ids"]),
        tool("import_svg", "Add SVG markup as vector layers.", json!({ "svg": { "type": "string" }, "x": { "type": "number" }, "y": { "type": "number" }, "parent_id": { "type": "string" }, "name": { "type": "string" } }), &["svg"]),
        tool("place_image", "Add an image file as a rectangle with an image fill.", json!({ "path": { "type": "string" }, "x": { "type": "number" }, "y": { "type": "number" }, "width": { "type": "number" }, "height": { "type": "number" }, "parent_id": { "type": "string" } }), &["path"]),
        tool("save", "Save the document (to `path` if given, as .katachi).", json!({ "path": { "type": "string" } }), &[]),
        tool("open_file", "Open a .katachi or Figma .fig file (unsaved changes must be saved first).", json!({ "path": { "type": "string" } }), &["path"]),
        tool("undo", "Undo the last change.", json!({}), &[]),
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
    fn jsonrpc_lifecycle_and_tool_calls_reach_the_ui_thread() {
        let (tx, rx) = channel::<Call>();
        let ctx = egui::Context::default();
        // Stand-in for the UI thread.
        std::thread::spawn(move || {
            for c in rx {
                let _ = c.reply.send(text(format!("{} {}", c.name, c.args["code"].as_str().unwrap_or(""))));
            }
        });
        let init = handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-03-26" } }), &tx, &ctx).unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26", "echoes the client's version");
        assert!(handle(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }), &tx, &ctx).is_none(), "notifications get no reply");
        let list = handle(&json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }), &tx, &ctx).unwrap();
        assert!(list["result"]["tools"].as_array().unwrap().iter().any(|t| t["name"] == "execute_code"));
        let call = handle(&json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "execute_code", "arguments": { "code": "return 1" } } }), &tx, &ctx).unwrap();
        assert_eq!(call["result"]["content"][0]["text"], "execute_code return 1");
        assert_eq!(handle(&json!({ "jsonrpc": "2.0", "id": 4, "method": "nope" }), &tx, &ctx).unwrap()["error"]["code"], -32601);
    }
}
