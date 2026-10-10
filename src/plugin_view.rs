//! A plugin's UI: its HTML in a native webview inside the Katachi window (macOS and Windows).
//! A shim maps Figma's iframe messaging (`parent.postMessage({ pluginMessage })` and
//! `onmessage = e => e.data.pluginMessage`) onto the webview's IPC channel.

use std::sync::{Arc, Mutex};

/// Sent once the UI has loaded, so messages posted before then aren't lost.
pub const READY: &str = "__katachi_ready__";

/// Runs before the plugin's own scripts: `parent.postMessage` goes to Katachi.
#[cfg(any(target_os = "macos", target_os = "windows"))]
const SHIM: &str = r#"(() => {
  const send = (m) => window.ipc.postMessage(JSON.stringify(m && m.pluginMessage !== undefined ? m.pluginMessage : null));
  const host = { postMessage: send };
  try { Object.defineProperty(window, "parent", { get: () => host, configurable: true }); } catch (e) { try { window.parent = host; } catch (_) {} }
  window.addEventListener("load", () => window.ipc.postMessage("__katachi_ready__"));
})();"#;

pub struct PluginView {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    web: wry::WebView,
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
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub fn open(frame: &eframe::Frame, ctx: &egui::Context, html: &str, rect: egui::Rect, inbox: Arc<Mutex<Vec<String>>>) -> Result<Self, String> {
        let wake = ctx.clone();
        let web = wry::WebViewBuilder::new()
            .with_initialization_script(SHIM)
            .with_html(html)
            .with_bounds(bounds(rect, ctx.zoom_factor()))
            .with_ipc_handler(move |req: wry::http::Request<String>| {
                inbox.lock().unwrap().push(req.into_body());
                wake.request_repaint();
            })
            .build_as_child(frame)
            .map_err(|e| e.to_string())?;
        Ok(Self { web })
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    pub fn open(_frame: &eframe::Frame, _ctx: &egui::Context, _html: &str, _rect: egui::Rect, _inbox: Arc<Mutex<Vec<String>>>) -> Result<Self, String> {
        Err("plugin UIs need macOS or Windows for now".into())
    }

    /// Deliver a message from the plugin (`figma.ui.postMessage`) to the UI.
    pub fn post(&self, json: &str) {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let _ = self.web.evaluate_script(&format!("window.dispatchEvent(new MessageEvent('message', {{ data: {{ pluginMessage: {json}, pluginId: 'katachi' }} }}));"));
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let _ = json;
    }

    pub fn place(&self, rect: egui::Rect, zoom: f32) {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let _ = self.web.set_bounds(bounds(rect, zoom));
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let _ = (rect, zoom);
    }

    pub fn show(&self, visible: bool) {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let _ = self.web.set_visible(visible);
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let _ = visible;
    }
}
