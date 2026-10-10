mod app;
mod appearance;
mod boolean;
mod canvas;
mod codegen;
mod collab;
mod comments;
mod doc;
mod export;
mod fig_import;
mod files;
mod kiwi;
mod mcp;
mod mcp_tools;
mod paint_ui;
mod plugin_doc;
mod plugin_view;
mod plugins;
mod panels;
mod prototype;
mod render;
mod svg_import;
mod text;
mod variables_ui;
mod vector_edit;
#[cfg(test)]
mod tests;

use egui::{FontData, FontDefinitions, FontFamily};
use std::sync::Arc;

fn main() -> eframe::Result {
    // `katachi serve [file] [--port N]`: a headless multiplayer relay that saves to the file.
    let args: Vec<String> = std::env::args().collect();
    // `katachi mcp`: connect a stdio MCP client to the running app.
    if args.get(1).is_some_and(|a| a == "mcp") {
        mcp::stdio_bridge();
        return Ok(());
    }
    if args.get(1).is_some_and(|a| a == "serve") {
        serve_headless(&args[2..]);
        return Ok(());
    }
    let path = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Katachi")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([800.0, 500.0])
            // Always transparent so glass can switch on without a restart; opaque otherwise.
            .with_transparent(true)
            .with_icon(eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon/icon.png")).unwrap_or_default()),
        multisampling: 4,
        ..Default::default()
    };
    eframe::run_native(
        "Katachi",
        options,
        Box::new(|cc| {
            setup_style(&cc.egui_ctx);
            Ok(Box::new(app::App::new(path)))
        }),
    )
}

fn setup_style(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let names = ["Inter", "Inter Medium", "Inter SemiBold", "Inter Bold"];
    for (name, bytes) in names.iter().zip(export::FONTS) {
        fonts.font_data.insert(name.to_string(), Arc::new(FontData::from_static(bytes)));
    }
    // Inter first, egui's bundled fonts after it as glyph fallbacks.
    let fallbacks = fonts.families[&FontFamily::Proportional].clone();
    fonts.families.get_mut(&FontFamily::Proportional).unwrap().insert(0, "Inter".into());
    for name in &names[1..] {
        let mut list = vec![name.to_string()];
        list.extend(fallbacks.iter().cloned());
        fonts.families.insert(FontFamily::Name((*name).into()), list);
    }
    ctx.set_fonts(fonts);
    ctx.options_mut(|o| o.zoom_with_keyboard = false);
    appearance::apply(ctx, &appearance::load());
}

fn serve_headless(args: &[String]) {
    let port: u16 = args.iter().position(|a| a == "--port").and_then(|i| args.get(i + 1)).and_then(|p| p.parse().ok()).unwrap_or(7878);
    let file = args.iter().find(|a| !a.starts_with("--") && a.parse::<u16>().is_err()).map(std::path::PathBuf::from);
    let doc = file
        .as_ref()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| serde_json::from_value::<doc::Document>(v["document"].clone()).ok())
        .unwrap_or_default();
    let listener = std::net::TcpListener::bind(("0.0.0.0", port)).expect("port in use");
    let shared = collab::serve(listener, doc);
    println!("Katachi session on {}:{port}", collab::local_ip());
    let mut last = String::new();
    loop {
        std::thread::sleep(std::time::Duration::from_secs(5));
        if let Some(f) = &file {
            let json = serde_json::json!({ "katachi": 1, "document": *shared.lock().unwrap() }).to_string();
            if json != last {
                let _ = files::write_atomic(f, json.as_bytes());
                last = json;
            }
        }
    }
}
