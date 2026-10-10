//! Text layout shared by the canvas, the in-place editor, and export; system font loading.

use crate::doc::{Decoration, LineHeight, Node, TextAlign, TextCase, TextSizing, VAlign};
use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontFamily, FontId, Galley, Stroke, Vec2, vec2};
use resvg::usvg::fontdb;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};

/// Inter's line height in ems (hhea ascender - descender), egui's natural row height for it.
pub const NATURAL_LINE: f32 = 1.2109375;
pub const ASCENT: f32 = 0.96875;

/// Installed fonts, loaded on first use (takes a moment on machines with many fonts).
pub fn system_fonts() -> &'static fontdb::Database {
    static DB: OnceLock<fontdb::Database> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        db
    })
}

/// Every family name available, Inter first.
pub fn families() -> &'static [String] {
    static LIST: OnceLock<Vec<String>> = OnceLock::new();
    LIST.get_or_init(|| {
        let mut set: Vec<String> = system_fonts()
            .faces()
            .filter_map(|f| f.families.first().map(|(n, _)| n.clone()))
            .filter(|n| !n.starts_with('.'))
            .collect();
        set.sort_by_key(|s| s.to_lowercase());
        set.dedup();
        set.retain(|f| f != "Inter");
        set.insert(0, "Inter".into());
        set
    })
}

/// Upright weights installed for `family`.
pub fn weights_for(family: &str) -> Vec<u16> {
    if family == "Inter" {
        return vec![400, 500, 600, 700];
    }
    let mut w: Vec<u16> = system_fonts()
        .faces()
        .filter(|f| f.style == fontdb::Style::Normal && f.families.iter().any(|(n, _)| n == family))
        .map(|f| f.weight.0)
        .collect();
    w.sort();
    w.dedup();
    if w.is_empty() { vec![400] } else { w }
}

pub fn weight_name(w: u16) -> &'static str {
    match w {
        0..=149 => "Thin",
        150..=249 => "Extra Light",
        250..=349 => "Light",
        350..=449 => "Regular",
        450..=549 => "Medium",
        550..=649 => "Semi Bold",
        650..=749 => "Bold",
        750..=849 => "Extra Bold",
        _ => "Black",
    }
}

#[derive(Default)]
struct Registry {
    /// Usable now.
    ready: HashMap<(String, u16), FontFamily>,
    /// Handed to egui on this pass; usable from a later one.
    pending: HashMap<(String, u16), u64>,
    missing: HashSet<(String, u16)>,
}

/// Fonts loaded into one egui context (each context has its own font set).
fn registry(ctx: &egui::Context) -> Arc<Mutex<Registry>> {
    ctx.data_mut(|d| d.get_temp_mut_or_insert_with(egui::Id::new("katachi-font-registry"), || Arc::new(Mutex::new(Registry::default()))).clone())
}

/// Make `family`/`weight` available to egui. Call once per frame before drawing text.
pub fn ensure_font(ctx: &egui::Context, family: &str, weight: u16) {
    if family == "Inter" {
        return;
    }
    let key = (family.to_string(), weight);
    let reg = registry(ctx);
    let mut r = reg.lock().unwrap();
    if r.ready.contains_key(&key) || r.missing.contains(&key) {
        return;
    }
    let pass = ctx.cumulative_pass_nr();
    if let Some(&added) = r.pending.get(&key) {
        // egui binds new fonts at the start of a pass, so only use it on a later one.
        if pass > added {
            r.pending.remove(&key);
            let name = format!("{family}#{weight}");
            r.ready.insert(key, FontFamily::Name(name.into()));
            ctx.request_repaint();
        }
        return;
    }
    let db = system_fonts();
    let q = fontdb::Query { families: &[fontdb::Family::Name(family)], weight: fontdb::Weight(weight), ..Default::default() };
    let face = db.query(&q).and_then(|id| db.with_face_data(id, |data, index| (data.to_vec(), index)));
    let Some((data, index)) = face else {
        r.missing.insert(key);
        return;
    };
    let name = format!("{family}#{weight}");
    let mut fd = egui::FontData::from_owned(data);
    fd.index = index;
    ctx.add_font(egui::epaint::text::FontInsert {
        name: name.clone(),
        data: fd,
        families: vec![egui::epaint::text::InsertFontFamily {
            family: FontFamily::Name(name.into()),
            priority: egui::epaint::text::FontPriority::Highest,
        }],
    });
    r.pending.insert(key, pass);
    ctx.request_repaint();
}

/// egui family for a node's font, falling back to Inter until a system font is loaded.
pub fn font_family(ctx: &egui::Context, family: &str, weight: u16) -> FontFamily {
    if family != "Inter"
        && let Some(f) = registry(ctx).lock().unwrap().ready.get(&(family.to_string(), weight)) {
            return f.clone();
        }
    match weight {
        0..=449 => FontFamily::Proportional,
        450..=549 => FontFamily::Name("Inter Medium".into()),
        550..=649 => FontFamily::Name("Inter SemiBold".into()),
        _ => FontFamily::Name("Inter Bold".into()),
    }
}

pub fn apply_case(text: &str, case: TextCase) -> String {
    match case {
        TextCase::Original => text.to_string(),
        TextCase::Upper => text.to_uppercase(),
        TextCase::Lower => text.to_lowercase(),
        TextCase::Title => {
            let mut out = String::with_capacity(text.len());
            let mut start = true;
            for c in text.chars() {
                if start && c.is_alphabetic() {
                    out.extend(c.to_uppercase());
                } else {
                    out.push(c);
                }
                start = c.is_whitespace();
            }
            out
        }
    }
}

/// Line pitch in world units.
pub fn line_pitch(n: &Node) -> f32 {
    match n.line_height {
        LineHeight::Auto => n.font_size * NATURAL_LINE,
        LineHeight::Px(px) => px,
        LineHeight::Percent(p) => n.font_size * p / 100.0,
    }
}

/// Layout job for a text node at `zoom`. `text` is the raw (un-cased) string.
pub fn job(ctx: &egui::Context, n: &Node, text: &str, zoom: f32, color: Color32) -> LayoutJob {
    let alpha = color.a() as f32 / 255.0;
    let format = |st: &crate::doc::SpanStyle| {
        let size = st.font_size.unwrap_or(n.font_size) * zoom;
        let c = st.color.map_or(color, |c| c.to32().gamma_multiply(alpha));
        let deco = Stroke::new((n.font_size / 14.0).max(1.0) * zoom, c);
        let decoration = st.decoration.unwrap_or(n.decoration);
        let family = st.font_family.as_deref().unwrap_or(&n.font_family);
        TextFormat {
            font_id: FontId::new(size, font_family(ctx, family, st.font_weight.unwrap_or(n.font_weight))),
            color: c,
            extra_letter_spacing: n.letter_spacing / 100.0 * size,
            line_height: (n.line_height != LineHeight::Auto).then(|| line_pitch(n) * zoom),
            underline: if decoration == Decoration::Underline { deco } else { Stroke::NONE },
            strikethrough: if decoration == Decoration::Strikethrough { deco } else { Stroke::NONE },
            italics: st.italic.unwrap_or(false),
            ..Default::default()
        }
    };
    let mut job = LayoutJob::default();
    if n.spans.is_empty() {
        job.append(&apply_case(text, n.text_case), 0.0, format(&Default::default()));
    } else {
        // One section per styled run; char indices are clamped while the editor is a frame ahead.
        let chars: Vec<char> = text.chars().collect();
        let mut cuts: Vec<usize> = vec![0, chars.len()];
        for sp in &n.spans {
            cuts.push(sp.start.min(chars.len()));
            cuts.push(sp.end.min(chars.len()));
        }
        cuts.sort();
        cuts.dedup();
        for w in cuts.windows(2) {
            let piece: String = chars[w[0]..w[1]].iter().collect();
            let st = n.spans.iter().find(|sp| sp.start <= w[0] && sp.end >= w[1]).map(|sp| sp.style.clone()).unwrap_or_default();
            job.append(&apply_case(&piece, n.text_case), 0.0, format(&st));
        }
    }
    job.wrap.max_width = match n.text_sizing {
        TextSizing::AutoWidth => f32::INFINITY,
        _ => (n.rect.width() * zoom).max(1.0),
    };
    job
}

/// Shift each row for horizontal alignment within the box (egui lays out left-aligned).
pub fn align_rows(galley: Arc<Galley>, n: &Node, zoom: f32) -> Arc<Galley> {
    if n.text_align == TextAlign::Left || n.text_align == TextAlign::Justify {
        return galley;
    }
    let width = match n.text_sizing {
        TextSizing::AutoWidth => galley.size().x,
        _ => n.rect.width() * zoom,
    };
    let f = if n.text_align == TextAlign::Center { 0.5 } else { 1.0 };
    let mut g = (*galley).clone();
    for row in &mut g.rows {
        row.pos.x = (width - row.size.x) * f;
    }
    g.rect = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(width, g.rect.height()));
    Arc::new(g)
}

/// Offset (world units) from the box's top-left to where the galley is drawn:
/// Figma centers glyphs in each line box, and fixed boxes align vertically.
pub fn draw_offset(n: &Node, rows: usize) -> Vec2 {
    let pitch = line_pitch(n);
    let half_leading = if n.line_height == LineHeight::Auto { 0.0 } else { (pitch - n.font_size * NATURAL_LINE) / 2.0 };
    let text_h = pitch * rows.max(1) as f32;
    let v = if n.text_sizing == TextSizing::Fixed {
        let f = match n.text_valign {
            VAlign::Top => 0.0,
            VAlign::Middle => 0.5,
            VAlign::Bottom => 1.0,
        };
        (n.rect.height() - text_h) * f
    } else {
        0.0
    };
    vec2(0.0, v + half_leading)
}

/// Laid-out text for drawing, plus the content size at 100% and the wrapped lines.
pub fn layout(painter: &egui::Painter, n: &Node, zoom: f32, color: Color32) -> (Arc<Galley>, Vec2, Vec<String>) {
    let text = if n.text.is_empty() { " " } else { n.text.as_str() };
    let galley = painter.layout_job(job(painter.ctx(), n, text, zoom, color));
    let rows = galley.rows.len();
    // Lines keep a trailing newline where the paragraph ended, so export can track char offsets.
    let lines = galley
        .rows
        .iter()
        .map(|r| {
            let mut l: String = r.glyphs.iter().map(|g| g.chr).collect();
            if r.ends_with_newline {
                l.push('\n');
            }
            l
        })
        .collect();
    let width = galley.rows.iter().map(|r| r.size.x).fold(0.0, f32::max) / zoom;
    // Mixed sizes make rows taller than the base pitch; take whichever is larger.
    let laid_out = if n.spans.is_empty() { 0.0 } else { galley.size().y / zoom };
    let size = vec2(width, (line_pitch(n) * rows.max(1) as f32).max(laid_out));
    (align_rows(galley, n, zoom), size, lines)
}
