//! A plugin's UI. On macOS and Windows its HTML runs in a native webview inside the Katachi
//! window; elsewhere (Linux) it opens in the default browser, served from 127.0.0.1. Either way a
//! shim maps Figma's iframe messaging (`parent.postMessage({ pluginMessage })` and
//! `onmessage = e => e.data.pluginMessage`) onto Katachi's channel.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// Sent once the UI has loaded, so messages posted before then aren't lost.
pub const READY: &str = "__katachi_ready__";

/// Runs before the plugin's own scripts in the native webview: `parent.postMessage` goes to Katachi.
#[cfg(any(target_os = "macos", target_os = "windows"))]
const SHIM: &str = r#"(() => {
  const send = (m) => window.ipc.postMessage(JSON.stringify(m && m.pluginMessage !== undefined ? m.pluginMessage : null));
  const host = { postMessage: send };
  try { Object.defineProperty(window, "parent", { get: () => host, configurable: true }); } catch (e) { try { window.parent = host; } catch (_) {} }
  window.addEventListener("load", () => window.ipc.postMessage("__katachi_ready__"));
})();"#;

/// The same, for a browser tab: sends are POSTs, and plugin messages arrive by long polling.
const BROWSER_SHIM: &str = r#"<script>(() => {
  const send = (m) => fetch("/send", { method: "POST", body: JSON.stringify(m && m.pluginMessage !== undefined ? m.pluginMessage : null) });
  const host = { postMessage: send };
  try { Object.defineProperty(window, "parent", { get: () => host, configurable: true }); } catch (e) { try { window.parent = host; } catch (_) {} }
  window.addEventListener("load", () => fetch("/send", { method: "POST", body: "__katachi_ready__" }));
  (async () => {
    for (;;) {
      try {
        const r = await fetch("/poll");
        if (r.status === 410) { document.title = "Plugin closed"; return; }
        for (const m of await r.json()) window.dispatchEvent(new MessageEvent("message", { data: { pluginMessage: m, pluginId: "katachi" } }));
      } catch (e) { await new Promise(r => setTimeout(r, 1000)); }
    }
  })();
})();</script>
"#;

pub struct PluginView {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    web: Option<wry::WebView>,
    browser: Option<Browser>,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn bounds(r: egui::Rect, zoom: f32) -> wry::Rect {
    wry::Rect {
        position: wry::dpi::LogicalPosition::new((r.min.x * zoom) as f64, (r.min.y * zoom) as f64).into(),
        size: wry::dpi::LogicalSize::new((r.width() * zoom) as f64, (r.height() * zoom) as f64).into(),
    }
}

impl PluginView {
    /// Open the UI at `rect` (egui points). Messages from it land in `inbox` and wake egui.
    pub fn open(frame: &eframe::Frame, ctx: &egui::Context, html: &str, rect: egui::Rect, inbox: Arc<Mutex<Vec<String>>>) -> Result<Self, String> {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            let (wake, sink) = (ctx.clone(), inbox.clone());
            let web = wry::WebViewBuilder::new()
                .with_initialization_script(SHIM)
                .with_html(html)
                .with_bounds(bounds(rect, ctx.zoom_factor()))
                .with_ipc_handler(move |req: wry::http::Request<String>| {
                    sink.lock().unwrap().push(req.into_body());
                    wake.request_repaint();
                })
                .build_as_child(frame);
            match web {
                Ok(web) => Ok(Self { web: Some(web), browser: None }),
                // No webview available (say, WebView2 missing on Windows): use the browser instead.
                Err(_) => {
                    let b = Browser::start(html, inbox, ctx.clone())?;
                    open_url(&b.url);
                    Ok(Self { web: None, browser: Some(b) })
                }
            }
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = (frame, rect);
            let b = Browser::start(html, inbox, ctx.clone())?;
            open_url(&b.url);
            Ok(Self { browser: Some(b) })
        }
    }

    /// Whether the UI sits inside the Katachi window (otherwise it's in a browser tab).
    pub fn in_window(&self) -> bool {
        self.browser.is_none()
    }

    /// Deliver a message from the plugin (`figma.ui.postMessage`) to the UI.
    pub fn post(&self, json: &str) {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some(web) = &self.web {
            let _ = web.evaluate_script(&format!("window.dispatchEvent(new MessageEvent('message', {{ data: {{ pluginMessage: {json}, pluginId: 'katachi' }} }}));"));
        }
        if let Some(b) = &self.browser {
            b.post(json);
        }
    }

    pub fn place(&self, rect: egui::Rect, zoom: f32) {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some(web) = &self.web {
            let _ = web.set_bounds(bounds(rect, zoom));
        }
        let _ = (rect, zoom);
    }

    pub fn show(&self, visible: bool) {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some(web) = &self.web {
            let _ = web.set_visible(visible);
        }
        let _ = visible;
    }

    /// The browser tab's address, when the UI isn't in the window.
    pub fn url(&self) -> Option<&str> {
        self.browser.as_ref().map(|b| b.url.as_str())
    }
}

pub fn open_url(url: &str) {
    let cmd = if cfg!(target_os = "macos") { "open" } else if cfg!(target_os = "windows") { "explorer" } else { "xdg-open" };
    let _ = std::process::Command::new(cmd).arg(url).spawn();
}

/// A plugin UI served to a browser tab from 127.0.0.1.
pub struct Browser {
    pub url: String,
    outbox: Arc<(Mutex<Vec<String>>, Condvar)>,
    stop: Arc<AtomicBool>,
}

impl Browser {
    pub fn start(html: &str, inbox: Arc<Mutex<Vec<String>>>, ctx: egui::Context) -> Result<Self, String> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        let url = format!("http://127.0.0.1:{}/", listener.local_addr().map_err(|e| e.to_string())?.port());
        let page = Arc::new(format!("{BROWSER_SHIM}{html}"));
        let outbox: Arc<(Mutex<Vec<String>>, Condvar)> = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let (o, s) = (outbox.clone(), stop.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (page, inbox, o, s, ctx) = (page.clone(), inbox.clone(), o.clone(), s.clone(), ctx.clone());
                std::thread::spawn(move || {
                    let _ = serve(stream, &page, &inbox, &o, &s, &ctx);
                });
            }
        });
        Ok(Self { url, outbox, stop })
    }

    pub fn post(&self, json: &str) {
        let (lock, cv) = &*self.outbox;
        lock.lock().unwrap().push(json.to_string());
        cv.notify_all();
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.outbox.1.notify_all();
    }
}

fn respond(mut s: TcpStream, status: &str, ctype: &str, body: &str) -> std::io::Result<()> {
    write!(s, "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
}

fn serve(stream: TcpStream, page: &str, inbox: &Mutex<Vec<String>>, outbox: &(Mutex<Vec<String>>, Condvar), stop: &AtomicBool, ctx: &egui::Context) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
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
    let mut body = vec![0; len.min(16 << 20)];
    reader.read_exact(&mut body)?;
    match (method, path) {
        ("GET", "/") => respond(stream, "200 OK", "text/html; charset=utf-8", page),
        ("POST", "/send") => {
            inbox.lock().unwrap().push(String::from_utf8_lossy(&body).into_owned());
            ctx.request_repaint();
            respond(stream, "204 No Content", "text/plain", "")
        }
        ("GET", "/poll") => {
            let (lock, cv) = outbox;
            let mut q = lock.lock().unwrap();
            if q.is_empty() && !stop.load(Ordering::Relaxed) {
                q = cv.wait_timeout(q, Duration::from_secs(20)).unwrap().0;
            }
            if stop.load(Ordering::Relaxed) {
                return respond(stream, "410 Gone", "text/plain", "");
            }
            let msgs: Vec<String> = q.drain(..).collect();
            drop(q);
            respond(stream, "200 OK", "application/json", &format!("[{}]", msgs.join(",")))
        }
        _ => respond(stream, "404 Not Found", "text/plain", ""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn http(url: &str, method: &str, path: &str, body: &str) -> String {
        let addr = url.trim_start_matches("http://").trim_end_matches('/');
        let mut s = TcpStream::connect(addr).unwrap();
        write!(s, "{method} {path} HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    }

    #[test]
    fn browser_bridge_carries_messages_both_ways() {
        let inbox: Arc<Mutex<Vec<String>>> = Arc::default();
        let b = Browser::start("<p>Hello</p>", inbox.clone(), egui::Context::default()).unwrap();
        let page = http(&b.url, "GET", "/", "");
        assert!(page.contains("parent") && page.ends_with("<p>Hello</p>"), "shim before the plugin's HTML");
        http(&b.url, "POST", "/send", r#"{"type":"create"}"#);
        assert_eq!(*inbox.lock().unwrap(), vec![r#"{"type":"create"}"#.to_string()]);
        b.post(r#"{"type":"made"}"#);
        b.post("2");
        assert!(http(&b.url, "GET", "/poll", "").ends_with(r#"[{"type":"made"},2]"#));
        let url = b.url.clone();
        drop(b);
        assert!(http(&url, "GET", "/poll", "").starts_with("HTTP/1.1 410"), "closing the plugin ends the tab's polling");
    }
}
