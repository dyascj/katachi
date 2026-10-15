//! Text layout shared by the canvas, the in-place editor, and export; system font loading.

use crate::doc::{Decoration, LineHeight, ListKind, Node, TextAlign, TextCase, TextSizing, VAlign};
use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontFamily, FontId, Galley, Stroke, Vec2, vec2};
use resvg::usvg::fontdb;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};

/// Linked text, underlined in this color unless it has its own.
pub const LINK: Color32 = Color32::from_rgb(0x0D, 0x99, 0xFF);

/// Inter's line height in ems (hhea ascender - descender), egui's natural row height for it.
pub const NATURAL_LINE: f32 = 1.2109375;
pub const ASCENT: f32 = 0.96875;
/// Inter's cap height in ems.
pub const CAP: f32 = 0.727;

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

/// Whether `family` is installed (Inter always is: it's bundled).
pub fn installed(family: &str) -> bool {
    static SET: OnceLock<HashSet<String>> = OnceLock::new();
    SET.get_or_init(|| families().iter().cloned().collect()).contains(family)
}

/// Fonts the document uses that aren't installed, by family, with the styles used (Figma's
/// missing fonts; mostly from .fig imports).
pub fn missing_fonts(doc: &crate::doc::Document) -> Vec<(String, Vec<(u16, bool)>)> {
    let mut out: std::collections::BTreeMap<String, Vec<(u16, bool)>> = Default::default();
    for n in doc.nodes.values().filter(|n| n.kind == crate::doc::Kind::Text) {
        let runs = n.spans.iter().map(|sp| (sp.style.font_family.as_deref().unwrap_or(&n.font_family), sp.style.font_weight.unwrap_or(n.font_weight), sp.style.italic.unwrap_or(n.italic)));
        for (family, w, i) in std::iter::once((n.font_family.as_str(), n.font_weight, n.italic)).chain(runs) {
            if !installed(family) {
                out.entry(family.to_string()).or_default().push((w, i));
            }
        }
    }
    out.into_iter()
        .map(|(f, mut s)| {
            s.sort();
            s.dedup();
            (f, s)
        })
        .collect()
}

/// Inter's weights bundled with the app (upright only).
const INTER: [u16; 4] = [400, 500, 600, 700];

/// Installed styles of `family` as (weight, italic), lightest first, each upright before its italic.
pub fn styles_for(family: &str) -> Vec<(u16, bool)> {
    let mut s: Vec<(u16, bool)> = system_fonts()
        .faces()
        .filter(|f| f.families.iter().any(|(n, _)| n == family))
        .map(|f| (f.weight.0, f.style != fontdb::Style::Normal))
        .collect();
    if family == "Inter" {
        s.extend(INTER.map(|w| (w, false)));
    }
    s.sort();
    s.dedup();
    if s.is_empty() { vec![(400, false)] } else { s }
}

/// Upright weights installed for `family`.
pub fn weights_for(family: &str) -> Vec<u16> {
    let mut w: Vec<u16> = styles_for(family).into_iter().filter(|s| !s.1).map(|s| s.0).collect();
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

/// Figma's font style names: "Regular", "Italic", "Bold Italic".
pub fn style_name(w: u16, italic: bool) -> String {
    match (weight_name(w), italic) {
        ("Regular", true) => "Italic".into(),
        (name, true) => format!("{name} Italic"),
        (name, false) => name.into(),
    }
}

/// A face: family, weight, italic.
type FaceKey = (String, u16, bool);

#[derive(Default)]
struct Registry {
    /// Usable now.
    ready: HashMap<FaceKey, FontFamily>,
    /// Handed to egui on this pass; usable from a later one.
    pending: HashMap<FaceKey, u64>,
    missing: HashSet<FaceKey>,
}

/// Fonts loaded into one egui context (each context has its own font set).
fn registry(ctx: &egui::Context) -> Arc<Mutex<Registry>> {
    ctx.data_mut(|d| d.get_temp_mut_or_insert_with(egui::Id::new("katachi-font-registry"), || Arc::new(Mutex::new(Registry::default()))).clone())
}

/// Make a face of `family` available to egui. Call once per frame before drawing text.
pub fn ensure_font(ctx: &egui::Context, family: &str, weight: u16, italic: bool) {
    if family == "Inter" && !italic && INTER.contains(&weight) {
        return;
    }
    if italic {
        // Its upright face too: the stand-in until the italic loads, or for good if there's none.
        ensure_font(ctx, family, weight, false);
    }
    let key = (family.to_string(), weight, italic);
    let reg = registry(ctx);
    let mut r = reg.lock().unwrap();
    if r.ready.contains_key(&key) || r.missing.contains(&key) {
        return;
    }
    let name = format!("{family}#{weight}{}", if italic { "i" } else { "" });
    let pass = ctx.cumulative_pass_nr();
    if let Some(&added) = r.pending.get(&key) {
        // egui binds new fonts at the start of a pass, so only use it on a later one.
        if pass > added {
            r.pending.remove(&key);
            r.ready.insert(key, FontFamily::Name(name.into()));
            ctx.request_repaint();
        }
        return;
    }
    let db = system_fonts();
    let style = if italic { fontdb::Style::Italic } else { fontdb::Style::Normal };
    let q = fontdb::Query { families: &[fontdb::Family::Name(family)], weight: fontdb::Weight(weight), style, ..Default::default() };
    // fontdb falls back to an upright face when there's no italic; that one doesn't count.
    let face = db
        .query(&q)
        .filter(|&id| !italic || db.face(id).is_some_and(|f| f.style != fontdb::Style::Normal))
        .and_then(|id| db.with_face_data(id, |data, index| (data.to_vec(), index)));
    let Some((data, index)) = face else {
        r.missing.insert(key);
        return;
    };
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

/// egui family for a face, and whether to slant it: italics without an italic face (or before it
/// loads) are the upright face slanted, and fonts fall back to Inter until loaded.
pub fn font_family(ctx: &egui::Context, family: &str, weight: u16, italic: bool) -> (FontFamily, bool) {
    let ready = registry(ctx).lock().unwrap().ready.get(&(family.to_string(), weight, italic)).cloned();
    if let Some(f) = ready {
        return (f, false);
    }
    if italic {
        return (font_family(ctx, family, weight, false).0, true);
    }
    let f = match weight {
        0..=449 => FontFamily::Proportional,
        450..=549 => FontFamily::Name("Inter Medium".into()),
        550..=649 => FontFamily::Name("Inter SemiBold".into()),
        _ => FontFamily::Name("Inter Bold".into()),
    };
    (f, false)
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
        let c = st.color.map_or(if st.link.is_some() { LINK.gamma_multiply(alpha) } else { color }, |c| c.to32().gamma_multiply(alpha));
        let deco = Stroke::new((n.font_size / 14.0).max(1.0) * zoom, c);
        let decoration = st.decoration.unwrap_or(n.decoration);
        let family = st.font_family.as_deref().unwrap_or(&n.font_family);
        let (face, slant) = font_family(ctx, family, st.font_weight.unwrap_or(n.font_weight), st.italic.unwrap_or(n.italic));
        TextFormat {
            font_id: FontId::new(size, face),
            color: c,
            extra_letter_spacing: n.letter_spacing / 100.0 * size,
            line_height: (n.line_height != LineHeight::Auto).then(|| line_pitch(n) * zoom),
            underline: if decoration == Decoration::Underline || st.link.is_some() { deco } else { Stroke::NONE },
            strikethrough: if decoration == Decoration::Strikethrough { deco } else { Stroke::NONE },
            italics: slant,
            ..Default::default()
        }
    };
    let mut job = LayoutJob::default();
    let indent = n.paragraph_indent * zoom;
    if text.is_empty() || (n.spans.is_empty() && indent == 0.0) {
        job.append(&apply_case(text, n.text_case), indent, format(&Default::default()));
    } else {
        // One section per styled run, and per paragraph for the indent (egui's leading space
        // starts a section that far in). Char indices are clamped while the editor is a frame ahead.
        // ponytail: justified rows drop the indent (egui's justify realigns from the first glyph).
        let chars: Vec<char> = text.chars().collect();
        let mut cuts: Vec<usize> = vec![0, chars.len()];
        for sp in &n.spans {
            cuts.push(sp.start.min(chars.len()));
            cuts.push(sp.end.min(chars.len()));
        }
        if indent > 0.0 {
            cuts.extend(chars.iter().enumerate().filter(|(_, c)| **c == '\n').map(|(i, _)| i + 1));
        }
        cuts.sort();
        cuts.dedup();
        for w in cuts.windows(2) {
            let piece: String = chars[w[0]..w[1]].iter().collect();
            let st = n.spans.iter().find(|sp| sp.start <= w[0] && sp.end >= w[1]).map(|sp| sp.style.clone()).unwrap_or_default();
            let lead = if w[0] == 0 || chars[w[0] - 1] == '\n' { indent } else { 0.0 };
            job.append(&apply_case(&piece, n.text_case), lead, format(&st));
        }
    }
    // Justified rows stretch to the box, except each paragraph's last (Figma, and egui's justify).
    job.justify = n.text_align == TextAlign::Justify;
    // List rows shift right by their indent afterwards, so they wrap that much narrower.
    let indent = (0..n.lists.len()).map(|p| list_indent(n, p)).fold(0.0, f32::max);
    job.wrap.max_width = match n.text_sizing {
        TextSizing::AutoWidth => f32::INFINITY,
        _ => ((n.rect.width() - indent) * zoom).max(1.0),
    };
    if n.truncate {
        let fit = if n.text_sizing == TextSizing::Fixed { (n.rect.height() / line_pitch(n)).floor().max(1.0) as usize } else { usize::MAX };
        job.wrap.max_rows = if n.max_lines > 0 { fit.min(n.max_lines as usize) } else { fit };
        job.wrap.overflow_character = Some('…');
    }
    job
}

/// Hanging indent of paragraph `p` (world units): room for the marker, per nesting level.
pub fn list_indent(n: &Node, p: usize) -> f32 {
    match n.lists.get(p) {
        Some(l) if l.kind != ListKind::Plain => n.font_size * 1.5 * (l.indent as f32 + 1.0),
        _ => 0.0,
    }
}

/// Paragraph index of each laid-out row.
pub fn row_paragraphs(galley: &Galley) -> Vec<usize> {
    let mut p = 0;
    galley
        .rows
        .iter()
        .map(|r| {
            let me = p;
            if r.ends_with_newline {
                p += 1;
            }
            me
        })
        .collect()
}

/// Marker text per paragraph ("" for plain): bullets, or numbers counted per level.
pub fn list_markers(n: &Node) -> Vec<String> {
    let mut counts = [0usize; 8];
    n.lists
        .iter()
        .map(|l| match l.kind {
            ListKind::Plain => {
                counts = [0; 8];
                String::new()
            }
            ListKind::Bullet => "•".into(),
            ListKind::Number => {
                let lvl = (l.indent as usize).min(7);
                counts[lvl] += 1;
                counts[lvl + 1..].fill(0);
                let k = counts[lvl];
                if lvl % 2 == 1 { format!("{}.", (b'a' + ((k - 1) % 26) as u8) as char) } else { format!("{k}.") }
            }
        })
        .collect()
}

/// Shift each row for list indents and horizontal alignment within the box (egui lays out left-aligned).
pub fn align_rows(galley: Arc<Galley>, n: &Node, zoom: f32) -> Arc<Galley> {
    let lists = n.lists.iter().any(|l| l.kind != ListKind::Plain);
    if !lists && n.paragraph_spacing == 0.0 && (n.text_align == TextAlign::Left || n.text_align == TextAlign::Justify) {
        return galley;
    }
    let paras = row_paragraphs(&galley);
    let indents: Vec<f32> = paras.iter().map(|&p| list_indent(n, p) * zoom).collect();
    let width = match n.text_sizing {
        TextSizing::AutoWidth => galley.rows.iter().zip(&indents).map(|(r, i)| r.size.x + i).fold(0.0, f32::max),
        _ => n.rect.width() * zoom,
    };
    let f = match n.text_align {
        TextAlign::Center => 0.5,
        TextAlign::Right => 1.0,
        _ => 0.0,
    };
    let mut g = (*galley).clone();
    let gap = n.paragraph_spacing * zoom;
    for ((row, i), p) in g.rows.iter_mut().zip(&indents).zip(&paras) {
        row.pos.x = i + (width - i - row.size.x) * f;
        row.pos.y += gap * *p as f32;
    }
    let extra = gap * paras.last().copied().unwrap_or(0) as f32;
    g.rect = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(width, g.rect.height() + extra));
    Arc::new(g)
}

/// List markers to paint: each paragraph's first row gets one, left of its indent.
/// Returns (top-left relative to the galley origin at `zoom`, marker).
pub fn markers(n: &Node, galley: &Galley, zoom: f32) -> Vec<(Vec2, String)> {
    if n.lists.is_empty() {
        return vec![];
    }
    let names = list_markers(n);
    let mut out = vec![];
    let mut prev = usize::MAX;
    for (row, p) in galley.rows.iter().zip(row_paragraphs(galley)) {
        if p != prev && let Some(m) = names.get(p).filter(|m| !m.is_empty()) {
            out.push((vec2(row.pos.x - n.font_size * 1.2 * zoom, row.pos.y), m.clone()));
        }
        prev = p;
    }
    out
}

/// A galley for a list marker in the paragraph's base style.
pub fn marker_galley(painter: &egui::Painter, n: &Node, marker: &str, zoom: f32, color: Color32) -> Arc<Galley> {
    let mut m = n.clone();
    m.spans.clear();
    m.lists.clear();
    m.text_sizing = TextSizing::AutoWidth;
    lay(painter, &m, marker, zoom, color)
}

/// Biggest glyph egui rasterizes, in device pixels: its font atlas panics on glyphs wider
/// than the atlas, which huge text at high zoom reaches.
const MAX_GLYPH_PX: f32 = 1000.0;

/// `job` laid out, with glyphs over `MAX_GLYPH_PX` laid out smaller and the result scaled up.
pub fn lay(painter: &egui::Painter, n: &Node, text: &str, zoom: f32, color: Color32) -> Arc<Galley> {
    let biggest = n.spans.iter().filter_map(|s| s.style.font_size).fold(n.font_size, f32::max) * painter.pixels_per_point();
    let z = zoom.min(MAX_GLYPH_PX / biggest.max(1e-6));
    let galley = painter.layout_job(job(painter.ctx(), n, text, z, color));
    if z >= zoom {
        return galley;
    }
    let s = zoom / z;
    let mut shape = egui::epaint::TextShape::new(egui::Pos2::ZERO, galley, color);
    shape.transform(egui::emath::TSTransform::from_scaling(s));
    let mut galley = shape.galley;
    // The mesh is scaled; glyph metrics too, so the editor's cursor and selection line up.
    for row in &mut Arc::make_mut(&mut galley).rows {
        for g in &mut Arc::make_mut(&mut row.row).glyphs {
            g.pos = (g.pos.to_vec2() * s).to_pos2();
            g.advance_width *= s;
            g.line_height *= s;
            g.font_ascent *= s;
            g.font_height *= s;
            g.font_face_ascent *= s;
            g.font_face_height *= s;
        }
    }
    galley
}

/// Vertical trim: the space cut above the first line's capitals and below the last line's
/// baseline (world units), or nothing for Standard.
/// ponytail: Inter's metrics for every font; read each face's cap height if trims look off.
pub fn trim(n: &Node) -> (f32, f32) {
    if !n.vertical_trim {
        return (0.0, 0.0);
    }
    let pitch = line_pitch(n);
    let half_leading = (pitch - n.font_size * NATURAL_LINE) / 2.0;
    (half_leading + (ASCENT - CAP) * n.font_size, pitch - half_leading - ASCENT * n.font_size)
}

/// Offset (world units) from the box's top-left to where the galley is drawn:
/// Figma centers glyphs in each line box, and fixed boxes align vertically.
pub fn draw_offset(n: &Node, rows: usize) -> Vec2 {
    let pitch = line_pitch(n);
    let half_leading = if n.line_height == LineHeight::Auto { 0.0 } else { (pitch - n.font_size * NATURAL_LINE) / 2.0 };
    let (top, bottom) = trim(n);
    let text_h = pitch * rows.max(1) as f32 + n.paragraph_spacing * n.text.matches('\n').count() as f32 - top - bottom;
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
    vec2(0.0, v + half_leading - top)
}

/// The link under world point `p` on text layer `n`, for clicking it (presentation).
pub fn link_at(painter: &egui::Painter, n: &Node, p: egui::Pos2) -> Option<String> {
    if !n.spans.iter().any(|sp| sp.style.link.is_some()) {
        return None;
    }
    let (galley, _, _) = layout(painter, n, 1.0, Color32::BLACK);
    let local = crate::doc::rotate(p, n.rect.center(), -n.rotation) - (n.rect.min + draw_offset(n, galley.rows.len()));
    if !galley.rect.contains(local.to_pos2()) {
        return None;
    }
    crate::doc::link_at(&n.spans, galley.cursor_from_pos(local).index.0).map(|l| l.2)
}

/// Laid-out text for drawing, plus the content size at 100% and the wrapped lines.
pub fn layout(painter: &egui::Painter, n: &Node, zoom: f32, color: Color32) -> (Arc<Galley>, Vec2, Vec<String>) {
    let text = if n.text.is_empty() { " " } else { n.text.as_str() };
    let galley = lay(painter, n, text, zoom, color);
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
    let width = galley.rows.iter().zip(row_paragraphs(&galley)).map(|(r, p)| r.size.x + list_indent(n, p) * zoom).fold(0.0, f32::max) / zoom;
    // Mixed sizes make rows taller than the base pitch; take whichever is larger.
    let laid_out = if n.spans.is_empty() { 0.0 } else { galley.size().y / zoom };
    let (top, bottom) = trim(n);
    let size = vec2(width, (line_pitch(n) * rows.max(1) as f32).max(laid_out) + n.paragraph_spacing * n.text.matches('\n').count() as f32 - top - bottom);
    (align_rows(galley, n, zoom), size, lines)
}
