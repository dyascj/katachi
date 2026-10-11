//! Side panels (pages, layers, design properties), floating toolbar, and painted icons.

use crate::app::{Align, App, Tool};
use crate::canvas::{BLUE, num};
use crate::doc::{AutoLayout, AxisAlign, Constraint, Kind, LayoutMode, NodeId, Page, Paint, Rgba, SHAPE_GRAY, Shadow, Sizing, StrokeAlign};
use crate::paint_ui::{PaintEvent, hex_field, paint_row, shadow_row};
use egui::{
    Color32, CornerRadius, DragValue, FontId, Painter, Pos2, Rect, Response, RichText, Sense, Shape, Stroke, StrokeKind,
    Ui, pos2, vec2,
};

#[derive(Clone, Copy, PartialEq)]
pub enum Icon {
    Move,
    Frame,
    Rect,
    Ellipse,
    Line,
    Pen,
    Text,
    Hand,
    Group,
    Align(Align),
    Eye(bool),
    Lock(bool),
    Plus,
    Minus,
    Chevron(bool),
    Menu,
    ArrowDown,
    ArrowRight,
    /// Auto layout wrap: across, then back on the next row.
    Wrap,
    Opacity,
    Radius,
    PadH,
    PadV,
    GapH,
    GapV,
    Rotate,
    Component,
    Instance,
    Boolean,
    Polygon,
    Star,
    Section,
    Slice,
    Arrow,
    Pencil,
    Scale,
    Play,
    Styles,
    Detach,
    Comment,
    Close,
}

use crate::appearance as theme;

/// Toolbar groups as in Figma UI3: (tool, icon, "Name  shortcut").
pub const TOOL_GROUPS: [&[(Tool, Icon, &str)]; 6] = [
    &[(Tool::Move, Icon::Move, "Move  V"), (Tool::Hand, Icon::Hand, "Hand tool  H"), (Tool::Scale, Icon::Scale, "Scale  K")],
    &[(Tool::Frame, Icon::Frame, "Frame  F"), (Tool::Section, Icon::Section, "Section  ⇧S"), (Tool::Slice, Icon::Slice, "Slice  S")],
    &[
        (Tool::Rectangle, Icon::Rect, "Rectangle  R"),
        (Tool::Line, Icon::Line, "Line  L"),
        (Tool::Arrow, Icon::Arrow, "Arrow  ⇧L"),
        (Tool::Ellipse, Icon::Ellipse, "Ellipse  O"),
        (Tool::Polygon, Icon::Polygon, "Polygon"),
        (Tool::Star, Icon::Star, "Star"),
    ],
    &[(Tool::Pen, Icon::Pen, "Pen  P"), (Tool::Pencil, Icon::Pencil, "Pencil  ⇧P")],
    &[(Tool::Text, Icon::Text, "Text  T")],
    &[(Tool::Comment, Icon::Comment, "Comment  C")],
];

/// Drag payload: a main component dragged from the Assets tab.
pub struct AssetDrag(pub NodeId);

pub fn paint_icon(p: &Painter, r: Rect, icon: Icon, color: Color32) {
    let s = Stroke::new(1.2, color);
    let c = r.center();
    let at = |x: f32, y: f32| c + vec2(x, y);
    match icon {
        Icon::Move => {
            let pts = [at(-4., -6.), at(-4., 5.), at(-1., 2.5), at(1.5, 7.), at(3., 6.2), at(0.6, 1.8), at(4.5, 1.8)];
            p.add(Shape::closed_line(pts.to_vec(), s));
        }
        Icon::Frame => {
            for d in [-2.5, 2.5] {
                p.line_segment([at(d, -6.), at(d, 6.)], s);
                p.line_segment([at(-6., d), at(6., d)], s);
            }
        }
        Icon::Rect => {
            p.rect_stroke(Rect::from_center_size(c, vec2(11., 11.)), 1.0, s, StrokeKind::Middle);
        }
        Icon::Ellipse => {
            p.circle_stroke(c, 5.5, s);
        }
        Icon::Line => {
            p.line_segment([at(-5., 5.), at(5., -5.)], s);
        }
        Icon::Pen => {
            let pts = [at(0., -6.), at(4.5, 1.), at(2., 6.), at(-2., 6.), at(-4.5, 1.)];
            p.add(Shape::closed_line(pts.to_vec(), s));
            p.line_segment([at(0., -6.), at(0., 0.5)], s);
            p.circle_filled(at(0., 1.5), 1.2, color);
        }
        Icon::Text => {
            p.line_segment([at(-5., -5.), at(5., -5.)], s);
            p.line_segment([at(0., -5.), at(0., 6.)], s);
        }
        Icon::Hand => {
            for (i, h) in [4.0, 6.0, 6.0, 5.0].iter().enumerate() {
                let x = -3.0 + i as f32 * 2.2;
                p.line_segment([at(x, 2.), at(x, 2. - h)], Stroke::new(1.6, color));
            }
            p.rect_stroke(Rect::from_min_max(at(-4., 1.), at(4.5, 6.5)), 2.0, s, StrokeKind::Middle);
            p.line_segment([at(-4., 3.), at(-6., 0.)], Stroke::new(1.6, color));
        }
        Icon::Group => {
            let b = Rect::from_center_size(c, vec2(10., 10.));
            for (a, d) in [(b.left_top(), vec2(3., 3.)), (b.right_top(), vec2(-3., 3.)), (b.left_bottom(), vec2(3., -3.)), (b.right_bottom(), vec2(-3., -3.))] {
                p.line_segment([a, a + vec2(d.x, 0.)], s);
                p.line_segment([a, a + vec2(0., d.y)], s);
            }
        }
        Icon::Align(a) => {
            let f = |r: Rect| p.rect_filled(r, 0.5, color);
            let bar = |x0: f32, y: f32, w: f32| Rect::from_min_size(at(x0, y), vec2(w, 3.));
            let col = |x: f32, y0: f32, h: f32| Rect::from_min_size(at(x, y0), vec2(3., h));
            match a {
                Align::Left => {
                    p.line_segment([at(-6., -6.), at(-6., 6.)], s);
                    f(bar(-4., -4., 9.));
                    f(bar(-4., 1., 5.));
                }
                Align::HCenter => {
                    p.line_segment([at(0., -6.), at(0., 6.)], s);
                    f(bar(-5., -4., 10.));
                    f(bar(-3., 1., 6.));
                }
                Align::Right => {
                    p.line_segment([at(6., -6.), at(6., 6.)], s);
                    f(bar(-5., -4., 9.));
                    f(bar(-1., 1., 5.));
                }
                Align::Top => {
                    p.line_segment([at(-6., -6.), at(6., -6.)], s);
                    f(col(-4., -4., 9.));
                    f(col(1., -4., 5.));
                }
                Align::VCenter => {
                    p.line_segment([at(-6., 0.), at(6., 0.)], s);
                    f(col(-4., -5., 10.));
                    f(col(1., -3., 6.));
                }
                Align::Bottom => {
                    p.line_segment([at(-6., 6.), at(6., 6.)], s);
                    f(col(-4., -5., 9.));
                    f(col(1., -1., 5.));
                }
            }
        }
        Icon::Eye(open) => {
            let pts: Vec<Pos2> = (0..=24)
                .map(|i| {
                    let t = i as f32 / 24.0 * std::f32::consts::TAU;
                    at(6.0 * t.cos(), 3.5 * t.sin())
                })
                .collect();
            p.add(Shape::line(pts, s));
            p.circle_filled(c, 1.8, color);
            if !open {
                p.line_segment([at(-6., 5.), at(6., -5.)], s);
            }
        }
        Icon::Lock(locked) => {
            p.rect_filled(Rect::from_min_max(at(-4., -0.5), at(4., 5.5)), 1.0, color);
            let shift = if locked { 0.0 } else { 3.0 };
            let pts: Vec<Pos2> = (0..=12)
                .map(|i| {
                    let t = std::f32::consts::PI * (1.0 + i as f32 / 12.0);
                    at(shift + 2.6 * t.cos(), -2.5 + 3.0 * t.sin())
                })
                .collect();
            p.add(Shape::line(pts, s));
            p.line_segment([at(shift - 2.6, -2.5), at(shift - 2.6, -0.5)], s);
            p.line_segment([at(shift + 2.6, -2.5), at(shift + 2.6, -0.5)], s);
        }
        Icon::Plus => {
            p.line_segment([at(-5., 0.), at(5., 0.)], s);
            p.line_segment([at(0., -5.), at(0., 5.)], s);
        }
        Icon::ArrowDown | Icon::ArrowRight => {
            let (a, b, l, r) = if icon == Icon::ArrowDown {
                (at(0., -6.), at(0., 6.), at(-4., 2.), at(4., 2.))
            } else {
                (at(-6., 0.), at(6., 0.), at(2., -4.), at(2., 4.))
            };
            p.line_segment([a, b], s);
            p.line_segment([l, b], s);
            p.line_segment([r, b], s);
        }
        Icon::Wrap => {
            let pts = [at(-6., -4.), at(4., -4.), at(6., -2.), at(6., 1.), at(4., 3.), at(-5., 3.)];
            for w in pts.windows(2) {
                p.line_segment([w[0], w[1]], s);
            }
            p.line_segment([at(-5., 3.), at(-2., 0.)], s);
            p.line_segment([at(-5., 3.), at(-2., 6.)], s);
        }
        Icon::Opacity => {
            p.circle_stroke(c, 5.0, s);
            let half: Vec<Pos2> = (0..=12).map(|i| {
                let t = std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / 12.0;
                at(5.0 * t.cos(), 5.0 * t.sin())
            }).collect();
            p.add(Shape::convex_polygon(half, color, Stroke::NONE));
        }
        Icon::Radius => {
            let pts: Vec<Pos2> = (0..=8).map(|i| {
                let t = std::f32::consts::PI + std::f32::consts::FRAC_PI_2 * i as f32 / 8.0;
                at(1.0 + 5.0 * t.cos(), 1.0 + 5.0 * t.sin())
            }).collect();
            p.line_segment([at(-4., 6.), at(-4., 1.)], s);
            p.line_segment([at(1., -4.), at(6., -4.)], s);
            p.add(Shape::line(pts, s));
        }
        Icon::PadH | Icon::PadV => {
            let h = icon == Icon::PadH;
            let f = |x: f32, y: f32| if h { at(x, y) } else { at(y, x) };
            p.line_segment([f(-6., -5.), f(-6., 5.)], s);
            p.line_segment([f(6., -5.), f(6., 5.)], s);
            p.rect_stroke(Rect::from_two_pos(f(-3., -3.), f(3., 3.)), 1.0, s, StrokeKind::Middle);
        }
        Icon::GapH | Icon::GapV => {
            let h = icon == Icon::GapH;
            let f = |x: f32, y: f32| if h { at(x, y) } else { at(y, x) };
            p.line_segment([f(-5., -5.), f(-5., 5.)], s);
            p.line_segment([f(5., -5.), f(5., 5.)], s);
            p.line_segment([f(-2., 0.), f(2., 0.)], s);
        }
        Icon::Rotate => {
            let pts: Vec<Pos2> = (0..=16).map(|i| {
                let t = -0.6 + 5.0 * i as f32 / 16.0;
                at(5.0 * t.cos(), 5.0 * t.sin())
            }).collect();
            let end = *pts.last().unwrap();
            p.add(Shape::line(pts, s));
            p.line_segment([end, end + vec2(3.0, 0.5)], s);
            p.line_segment([end, end + vec2(0.5, 3.0)], s);
        }
        Icon::Component => {
            // Four small diamonds.
            for (dx, dy) in [(0., -3.5), (3.5, 0.), (0., 3.5), (-3.5, 0.)] {
                let m = at(dx, dy);
                let d = 2.3;
                p.add(Shape::convex_polygon(vec![m + vec2(0., -d), m + vec2(d, 0.), m + vec2(0., d), m + vec2(-d, 0.)], color, Stroke::NONE));
            }
        }
        Icon::Instance => {
            let d = 5.5;
            p.add(Shape::closed_line(vec![at(0., -d), at(d, 0.), at(0., d), at(-d, 0.)], s));
        }
        Icon::Polygon | Icon::Star => {
            let star = icon == Icon::Star;
            let n = if star { 10 } else { 3 };
            let pts: Vec<Pos2> = (0..n)
                .map(|i| {
                    let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * i as f32 / n as f32;
                    let k = if star && i % 2 == 1 { 2.6 } else { 6.0 };
                    at(k * a.cos(), k * a.sin() + if star { 0.5 } else { 1.0 })
                })
                .collect();
            p.add(Shape::closed_line(pts, s));
        }
        Icon::Section => {
            p.rect_stroke(Rect::from_center_size(c + vec2(0., 1.), vec2(12., 10.)), 1.5, s, StrokeKind::Middle);
            p.rect_filled(Rect::from_min_size(at(-6., -6.), vec2(6., 3.)), 1.0, color);
        }
        Icon::Slice => {
            p.line_segment([at(-6., 6.), at(6., -6.)], s);
            p.rect_stroke(Rect::from_center_size(c, vec2(11., 11.)), 0.0, Stroke::new(1.0, color.gamma_multiply(0.6)), StrokeKind::Middle);
        }
        Icon::Arrow => {
            p.line_segment([at(-5., 5.), at(5., -5.)], s);
            p.line_segment([at(5., -5.), at(0., -5.)], s);
            p.line_segment([at(5., -5.), at(5., 0.)], s);
        }
        Icon::Pencil => {
            p.line_segment([at(-5., 5.), at(4., -4.)], Stroke::new(2.2, color));
            p.line_segment([at(-6., 6.), at(-5., 5.)], s);
        }
        Icon::Scale => {
            p.rect_stroke(Rect::from_min_size(at(-6., -1.), vec2(7., 7.)), 0.5, s, StrokeKind::Middle);
            p.line_segment([at(-1., 1.), at(6., -6.)], s);
            p.line_segment([at(6., -6.), at(2., -6.)], s);
            p.line_segment([at(6., -6.), at(6., -2.)], s);
        }
        Icon::Close => {
            p.line_segment([at(-4., -4.), at(4., 4.)], s);
            p.line_segment([at(4., -4.), at(-4., 4.)], s);
        }
        Icon::Comment => {
            let r = Rect::from_center_size(c, vec2(12., 11.));
            p.rect_stroke(r, egui::CornerRadius { nw: 6, ne: 6, se: 6, sw: 0 }, s, StrokeKind::Middle);
        }
        Icon::Styles => {
            for (dx, dy) in [(-3., -3.), (3., -3.), (-3., 3.), (3., 3.)] {
                p.circle_filled(at(dx, dy), 1.8, color);
            }
        }
        Icon::Detach => {
            p.line_segment([at(-6., 2.), at(-2., -2.)], s);
            p.line_segment([at(2., 2.), at(6., -2.)], s);
            p.line_segment([at(-1., -5.), at(1., 5.)], Stroke::new(1.0, color.gamma_multiply(0.7)));
        }
        Icon::Play => {
            p.add(Shape::convex_polygon(vec![at(-3.5, -5.), at(5., 0.), at(-3.5, 5.)], color, Stroke::NONE));
        }
        Icon::Boolean => {
            p.rect_stroke(Rect::from_min_size(at(-6., -6.), vec2(8., 8.)), 1.0, s, StrokeKind::Middle);
            p.rect_filled(Rect::from_min_size(at(-2., -2.), vec2(8., 8.)), 1.0, color);
        }
        Icon::Menu => {
            for y in [-4., 0., 4.] {
                p.line_segment([at(-6., y), at(6., y)], s);
            }
        }
        Icon::Minus => {
            p.line_segment([at(-5., 0.), at(5., 0.)], s);
        }
        Icon::Chevron(open) => {
            let pts = if open { vec![at(-3., -1.5), at(0., 1.5), at(3., -1.5)] } else { vec![at(-1.5, -3.), at(1.5, 0.), at(-1.5, 3.)] };
            p.add(Shape::line(pts, Stroke::new(1.0, color)));
        }
    }
}

pub fn icon_button(ui: &mut Ui, icon: Icon, selected: bool, size: f32, tip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, tip));
    let (bg, fg) = if selected {
        (BLUE, Color32::WHITE)
    } else if resp.hovered() {
        (theme::subtle(ui.ctx()), theme::ink(ui.ctx()))
    } else {
        (Color32::TRANSPARENT, theme::ink(ui.ctx()))
    };
    ui.painter().rect_filled(rect, 5.0, bg);
    paint_icon(ui.painter(), rect, icon, fg);
    if tip.is_empty() { resp } else { resp.on_hover_text(tip) }
}

fn kind_icon(k: Kind) -> Icon {
    match k {
        Kind::Frame => Icon::Frame,
        Kind::Component => Icon::Component,
        Kind::Instance => Icon::Instance,
        Kind::ComponentSet => Icon::Component,
        Kind::Boolean => Icon::Boolean,
        Kind::Polygon => Icon::Polygon,
        Kind::Star => Icon::Star,
        Kind::Section => Icon::Section,
        Kind::Slice => Icon::Slice,
        Kind::Group => Icon::Group,
        Kind::Rectangle => Icon::Rect,
        Kind::Ellipse => Icon::Ellipse,
        Kind::Vector => Icon::Pen,
        Kind::Text => Icon::Text,
    }
}

/// Half the panel's content width, independent of what a row has already used.
fn half(ui: &Ui) -> f32 {
    (ui.max_rect().width() - 8.0) / 2.0
}

/// Figma-style number box with a label (or `@icon` name) inside on the left.
fn num_field(ui: &mut Ui, v: &mut f32, label: &str, min: f32, width: f32) -> bool {
    let icon = match label {
        "@opacity" => Some(Icon::Opacity),
        "@radius" => Some(Icon::Radius),
        "@padh" => Some(Icon::PadH),
        "@padv" => Some(Icon::PadV),
        "@gaph" => Some(Icon::GapH),
        "@gapv" => Some(Icon::GapV),
        "@rotate" => Some(Icon::Rotate),
        _ => None,
    };
    let prefix = match (icon, label.is_empty()) {
        (Some(_), _) => "     ".to_string(),
        (None, true) => String::new(),
        (None, false) => format!("{label}  "),
    };
    let unit = match label {
        "@opacity" => "%",
        "@rotate" => "°",
        _ => "",
    };
    let mut tmp = if v.is_nan() { 0.0 } else { *v };
    let mixed = v.is_nan();
    let resp = ui.add_sized(
        [width.max(30.0), 22.0],
        DragValue::new(&mut tmp).prefix(prefix).speed(1.0).range(min..=1e6).custom_formatter(move |x, _| {
            if mixed {
                "Mixed".into()
            } else {
                format!("{}{}", num(x as f32), unit)
            }
        }),
    );
    if let Some(i) = icon {
        let r = Rect::from_center_size(pos2(resp.rect.min.x + 13.0, resp.rect.center().y), vec2(14.0, 14.0));
        paint_icon(ui.painter(), r, i, theme::muted(ui.ctx()));
    }
    if resp.changed() {
        *v = tmp;
    }
    resp.changed()
}

/// Compact chevron button opening a Fixed / Hug / Fill menu.
#[allow(clippy::too_many_arguments)]
fn sizing_menu(ui: &mut Ui, id: egui::Id, mode: &mut Sizing, can_hug: bool, can_fill: bool, axis: usize, bounds: (&mut Option<f32>, &mut Option<f32>), cur: f32) -> bool {
    let resp = icon_button(ui, Icon::Chevron(true), false, 18.0, "Resizing");
    let mut changed = false;
    egui::Popup::menu(&resp).id(id).show(|ui| {
        for (m, ok, label) in [(Sizing::Fixed, true, "Fixed"), (Sizing::Hug, can_hug, "Hug contents"), (Sizing::Fill, can_fill, "Fill container")] {
            if ok && ui.selectable_label(*mode == m, label).clicked() && *mode != m {
                *mode = m;
                changed = true;
            }
        }
        ui.separator();
        let dim = if axis == 0 { "width" } else { "height" };
        for (b, which) in [(bounds.0, "min"), (bounds.1, "max")] {
            let label = if b.is_some() { format!("Remove {which} {dim}") } else { format!("Add {which} {dim}") };
            if ui.button(label).clicked() {
                *b = if b.is_some() { None } else { Some(cur) };
                changed = true;
                ui.close();
            }
        }
    });
    changed
}

/// 3x3 alignment picker for auto layout (rows = vertical, columns = horizontal position).
fn align_grid(ui: &mut Ui, l: &mut AutoLayout) {
    let (rect, resp) = ui.allocate_exact_size(vec2(64.0, 64.0), Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, 4.0, theme::subtle(ui.ctx()));
    let h = l.mode == LayoutMode::Horizontal;
    let idx = |a: AxisAlign| match a {
        AxisAlign::Min => 0,
        AxisAlign::Center => 1,
        AxisAlign::Max => 2,
    };
    let (col, row) = if h { (idx(l.main), idx(l.cross)) } else { (idx(l.cross), idx(l.main)) };
    let cell = |c: usize, r: usize| pos2(rect.min.x + 12.0 + c as f32 * 20.0, rect.min.y + 12.0 + r as f32 * 20.0);
    for r in 0..3 {
        for c in 0..3 {
            let at = cell(c, r);
            if (c, r) == (col, row) {
                // Bars show the children's arrangement.
                let (bw, bh) = if h { (3.0, 10.0) } else { (10.0, 3.0) };
                for k in [-1.0, 0.0, 1.0] {
                    let off = if h { vec2(k * 4.5, 0.0) } else { vec2(0.0, k * 4.5) };
                    let len = if k == 0.0 { 1.0 } else { 0.7 };
                    p.rect_filled(Rect::from_center_size(at + off, vec2(bw * if h { 1.0 } else { len }, bh * if h { len } else { 1.0 })), 1.0, BLUE);
                }
            } else {
                p.circle_filled(at, 1.5, theme::dim(ui.ctx()));
            }
        }
    }
    if let Some(pos) = resp.interact_pointer_pos().filter(|_| resp.clicked()) {
        let c = (((pos.x - rect.min.x) / 21.4) as usize).min(2);
        let r = (((pos.y - rect.min.y) / 21.4) as usize).min(2);
        let to = |i: usize| [AxisAlign::Min, AxisAlign::Center, AxisAlign::Max][i];
        if h {
            (l.main, l.cross) = (to(c), to(r));
        } else {
            (l.main, l.cross) = (to(r), to(c));
        }
    }
}

fn section_title(ui: &mut Ui, title: &str) -> bool {
    let mut add = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            add = icon_button(ui, Icon::Plus, false, 22.0, "").clicked();
        });
    });
    add
}

/// Figma's frame presets: group, then (name, width, height).
const FRAME_PRESETS: &[(&str, &[(&str, u32, u32)])] = &[
    ("Phone", &[("iPhone 16", 393, 852), ("iPhone 16 Pro", 402, 874), ("iPhone 16 Pro Max", 440, 956), ("iPhone 16 Plus", 430, 932), ("iPhone 14", 390, 844), ("iPhone SE", 320, 568), ("Android Compact", 412, 917), ("Android Medium", 700, 840)]),
    ("Tablet", &[("iPad mini 8.3", 744, 1133), ("iPad Pro 11\"", 834, 1194), ("iPad Pro 12.9\"", 1024, 1366), ("Surface Pro 8", 1440, 960)]),
    ("Desktop", &[("MacBook Air", 1280, 832), ("MacBook Pro 14\"", 1512, 982), ("MacBook Pro 16\"", 1728, 1117), ("Desktop", 1440, 1024), ("Wireframe", 1440, 1024), ("TV", 1280, 720)]),
    ("Presentation", &[("Slide 16:9", 1920, 1080), ("Slide 4:3", 1024, 768)]),
    ("Watch", &[("Apple Watch 45mm", 198, 242), ("Apple Watch 41mm", 176, 215), ("Apple Watch Ultra", 205, 251)]),
    ("Paper", &[("A4", 595, 842), ("A5", 420, 595), ("A6", 297, 420), ("Letter", 612, 792), ("Tabloid", 792, 1224)]),
    ("Social media", &[("Twitter post", 1200, 675), ("Facebook post", 1200, 630), ("Instagram post", 1080, 1350), ("Instagram story", 1080, 1920), ("LinkedIn post", 1200, 1200), ("YouTube thumbnail", 1280, 720), ("Dribbble shot", 400, 300)]),
];

/// A dropdown inside a popover. egui's ComboBox opens its own popup, and a click there counts as
/// outside the popover and closes it; a menu submenu belongs to the popover instead.
fn dropdown(ui: &mut Ui, text: impl std::fmt::Display, add: impl FnOnce(&mut Ui)) {
    ui.menu_button(text.to_string(), add);
}

fn heading(ui: &mut Ui, title: &str) {
    ui.add_space(4.0);
    ui.label(RichText::new(title).strong());
    ui.add_space(2.0);
}

impl App {
    // ---------- left: menu, pages, layers ----------

    pub fn left_panel(&mut self, ui: &mut Ui) {
        if self.sel != self.prev_sel {
            self.prev_sel = self.sel.clone();
            for id in self.sel.clone() {
                for a in self.doc.ancestors(id) {
                    self.doc.node_mut(a).expanded = true;
                }
            }
        }
        egui::Panel::left("left").resizable(true).default_size(240.0).min_size(180.0).show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let menu = icon_button(ui, Icon::Menu, false, 28.0, "Main menu");
                egui::Popup::menu(&menu).show(|ui| self.main_menu(ui));
                // Truncated, so a long file name can't widen the panel.
                ui.add(egui::Label::new(RichText::new(self.display_name()).strong()).truncate());
            });
            ui.horizontal(|ui| {
                for (assets, label) in [(false, "File"), (true, "Assets")] {
                    let text = RichText::new(label);
                    let text = if self.left_tab_assets == assets { text.strong() } else { text.color(theme::muted(ui.ctx())) };
                    if ui.add(egui::Button::new(text).frame(false)).clicked() {
                        self.left_tab_assets = assets;
                    }
                }
            });
            ui.separator();
            if self.left_tab_assets {
                self.assets(ui);
                return;
            }
            self.pages_list(ui);
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(RichText::new("Layers").strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button(if self.layer_search.is_some() { "Done" } else { "Search" }).clicked() {
                        self.layer_search = if self.layer_search.is_some() { None } else { Some(String::new()) };
                    }
                });
            });
            if let Some(q) = &mut self.layer_search {
                ui.add(egui::TextEdit::singleline(q).hint_text("Find layers").desired_width(f32::INFINITY));
            }
            // Only the rows in view are built, so huge files stay fast.
            let rows = self.layer_rows();
            let row_h = 28.0;
            egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, row_h, rows.len(), |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                self.layers(ui, &rows[range]);
            });
        });
    }

    /// Assets tab: every main component in the file. Drag one onto the canvas to place an instance.
    fn assets(&mut self, ui: &mut Ui) {
        let search_id = ui.id().with("asset-search");
        let mut q: String = ui.data(|d| d.get_temp(search_id)).unwrap_or_default();
        ui.add(egui::TextEdit::singleline(&mut q).hint_text("Search assets").desired_width(f32::INFINITY));
        ui.data_mut(|d| d.insert_temp(search_id, q.clone()));
        ui.add_space(4.0);
        ui.label(RichText::new("Local components").strong());
        let mut comps: Vec<(usize, NodeId, String)> = self
            .doc
            .nodes
            .values()
            .filter(|n| n.kind == Kind::Component && n.name.to_lowercase().contains(&q.to_lowercase()) || n.kind == Kind::ComponentSet)
            // A set appears once and places its first variant.
            .filter(|n| !n.parent.is_some_and(|p| self.doc.node(p).kind == Kind::ComponentSet))
            .filter_map(|n| match n.kind {
                Kind::ComponentSet => n.children.first().map(|&v| (self.doc.page_of(n.id), v, n.name.clone())),
                _ => Some((self.doc.page_of(n.id), n.id, n.name.clone())),
            })
            .collect();
        comps.sort_by(|a, b| (a.0, &a.2).cmp(&(b.0, &b.2)));
        if comps.is_empty() {
            ui.label(RichText::new("Create components with ⌥⌘K to reuse them here.").color(theme::muted(ui.ctx())));
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (page, id, name) in comps {
                let r = ui.dnd_drag_source(ui.id().with(("asset", id)), AssetDrag(id), |ui| {
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                        paint_icon(ui.painter(), rect, Icon::Component, crate::canvas::PURPLE);
                        ui.label(&name);
                        ui.label(RichText::new(&self.doc.pages[page].name).color(theme::faint(ui.ctx())).small());
                    });
                });
                if r.response.double_clicked() {
                    let at = self.view_center_world();
                    self.checkpoint();
                    let size = self.doc.node(id).rect.size();
                    let i = self.doc.instantiate(id, self.page, None, None, (at - size / 2.0).round());
                    self.sel = vec![i];
                }
            }
        });
    }

    fn main_menu(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let item = |ui: &mut Ui, label: &str, short: &str| ui.add(egui::Button::new(label).shortcut_text(short)).clicked();
        if item(ui, "Back to files", "") {
            self.want = Some(crate::app::Want::Home);
            ui.close();
        }
        ui.separator();
        if item(ui, "New design file", "⌘N") {
            self.request_new();
            ui.close();
        }
        if item(ui, "Open… (.katachi or Figma .fig)", "⌘O") {
            self.request_open();
            ui.close();
        }
        let recent = crate::files::recent();
        ui.add_enabled_ui(!recent.is_empty(), |ui| {
            ui.menu_button("Open recent", |ui| {
                for p in recent {
                    let label = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                    if ui.button(label).on_hover_text(p.display().to_string()).clicked() {
                        if self.dirty {
                            self.toast("Save or discard changes first (⌘S)".into());
                        } else {
                            self.open_path(p);
                        }
                        ui.close();
                    }
                }
            });
        });
        if item(ui, "Save", "⌘S") {
            self.save(false);
            ui.close();
        }
        if item(ui, "Save as…", "⇧⌘S") {
            self.save(true);
            ui.close();
        }
        if item(ui, "Collaborate…", "") {
            self.show_collab = true;
            ui.close();
        }
        if item(ui, "Local variables…", "") {
            self.show_variables = true;
            ui.close();
        }
        if item(ui, "Version history…", "") {
            self.show_history = true;
            ui.close();
        }
        ui.menu_button("Plugins", |ui| {
            for p in crate::plugins::installed() {
                if ui.button(&p.name).clicked() {
                    match crate::plugins::load(&p) {
                        Ok((code, html, id)) => self.run_plugin(&p.name, Some(&id), &code, html.as_deref()),
                        Err(e) => self.toast(format!("{}: {e}", p.name)),
                    }
                    ui.close();
                }
            }
            ui.menu_button("Development", |ui| {
                if ui.button("Import plugin from manifest…").clicked() {
                    if let Some(f) = rfd::FileDialog::new().add_filter("Plugin manifest", &["json"]).pick_file() {
                        match crate::plugins::install(&f) {
                            Ok(p) => self.toast(format!("Installed {}", p.name)),
                            Err(e) => self.toast(format!("Couldn't import plugin: {e}")),
                        }
                    }
                    ui.close();
                }
                if ui.button("Run script…").clicked() {
                    self.show_console = true;
                    ui.close();
                }
            });
        });
        ui.menu_button("MCP server", |ui| {
            let port = crate::mcp::port();
            if self.mcp_listening() {
                ui.label(format!("Listening on http://127.0.0.1:{port}/mcp"));
                if ui.button("Copy Claude Code setup command").clicked() {
                    ui.ctx().copy_text(format!("claude mcp add --transport http katachi http://127.0.0.1:{port}/mcp"));
                    self.toast("Copied; run it in a terminal".into());
                    ui.close();
                }
                if ui.button("Copy stdio config (Claude Desktop, Cursor)").clicked() {
                    let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "katachi".into());
                    let cfg = serde_json::json!({ "mcpServers": { "katachi": { "command": exe, "args": ["mcp"] } } });
                    ui.ctx().copy_text(serde_json::to_string_pretty(&cfg).unwrap_or_default());
                    self.toast("Copied MCP config".into());
                    ui.close();
                }
            } else {
                ui.label(format!("Not running: port {port} is in use (another Katachi window?)"));
            }
        });
        if item(ui, "Place image / SVG…", "⇧⌘K") {
            self.import_dialog();
            ui.close();
        }
        if item(ui, "Export…", "⇧⌘E") {
            self.export(&ui.ctx().clone());
            ui.close();
        }
        ui.separator();
        if item(ui, "Undo", "⌘Z") {
            self.undo();
            ui.close();
        }
        if item(ui, "Redo", "⇧⌘Z") {
            self.redo();
            ui.close();
        }
        ui.separator();
        if item(ui, "Appearance…", "") {
            self.show_appearance = true;
            ui.close();
        }
        if item(ui, "Zoom to fit", "⇧1") {
            self.zoom_to(&ctx, None, false);
            ui.close();
        }
        if item(ui, "Zoom to 100%", "⇧0") {
            self.zoom_to(&ctx, None, true);
            ui.close();
        }
        if item(ui, if self.outline_mode { "Show fills" } else { "Outline mode" }, "⌘Y") {
            self.outline_mode = !self.outline_mode;
            ui.close();
        }
    }

    fn pages_list(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Pages").strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_button(ui, Icon::Plus, false, 22.0, "Add page").clicked() {
                    self.checkpoint();
                    let n = self.doc.pages.len() + 1;
                    self.doc.pages.push(Page { name: format!("Page {n}"), children: vec![], background: crate::doc::PAGE_BG, flows: vec![] });
                    self.select_page(n - 1);
                }
            });
        });
        let mut delete = None;
        for i in 0..self.doc.pages.len() {
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
            let page_name = self.doc.pages[i].name.clone();
            resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, i == self.page, &page_name));
            if i == self.page {
                ui.painter().rect_filled(rect, 4.0, theme::subtle(ui.ctx()));
            } else if resp.hovered() {
                ui.painter().rect_filled(rect, 4.0, theme::subtle(ui.ctx()).gamma_multiply(0.6));
            }
            if let Some((pi, buf)) = &mut self.renaming_page
                && *pi == i {
                    let r = ui.put(rect.shrink2(vec2(6.0, 3.0)), egui::TextEdit::singleline(buf));
                    let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
                    if !r.has_focus() && !r.lost_focus() && !cancel {
                        r.request_focus();
                    }
                    if r.lost_focus() || cancel {
                        let name = if cancel { String::new() } else { buf.trim().to_string() };
                        self.renaming_page = None;
                        if !name.is_empty() && name != self.doc.pages[i].name {
                            self.checkpoint();
                            self.doc.pages[i].name = name;
                        }
                    }
                    continue;
                }
            ui.painter().text(
                rect.left_center() + vec2(10.0, 0.0),
                egui::Align2::LEFT_CENTER,
                &self.doc.pages[i].name,
                FontId::proportional(12.0),
                ui.visuals().text_color(),
            );
            if resp.clicked() {
                self.select_page(i);
            }
            if resp.double_clicked() {
                self.renaming_page = Some((i, self.doc.pages[i].name.clone()));
            }
            resp.context_menu(|ui| {
                if ui.button("Rename").clicked() {
                    self.renaming_page = Some((i, self.doc.pages[i].name.clone()));
                    ui.close();
                }
                if ui.add_enabled(self.doc.pages.len() > 1, egui::Button::new("Delete page")).clicked() {
                    delete = Some(i);
                    ui.close();
                }
            });
        }
        if let Some(i) = delete {
            self.checkpoint();
            for id in self.doc.pages[i].children.clone() {
                self.doc.remove(id);
            }
            self.doc.pages.remove(i);
            self.page = self.page.min(self.doc.pages.len() - 1);
            self.sel.clear();
            self.view_ready = false;
        }
    }

    fn layer_rows(&self) -> Vec<(NodeId, usize)> {
        // Searching shows every matching layer flat, at its depth.
        if let Some(q) = self.layer_search.as_ref().filter(|q| !q.is_empty()) {
            let q = q.to_lowercase();
            let mut out = vec![];
            fn walk(app: &App, list: &[NodeId], depth: usize, q: &str, out: &mut Vec<(NodeId, usize)>) {
                for &id in list.iter().rev() {
                    let n = app.doc.node(id);
                    if n.name.to_lowercase().contains(q) {
                        out.push((id, depth));
                    }
                    walk(app, &n.children, depth + 1, q, out);
                }
            }
            walk(self, &self.doc.pages[self.page].children, 0, &q, &mut out);
            return out.into_iter().map(|(id, _)| (id, 0)).collect();
        }
        fn walk(app: &App, list: &[NodeId], depth: usize, out: &mut Vec<(NodeId, usize)>) {
            for &id in list.iter().rev() {
                out.push((id, depth));
                let n = app.doc.node(id);
                if n.expanded && !n.children.is_empty() {
                    walk(app, &n.children, depth + 1, out);
                }
            }
        }
        let mut out = vec![];
        walk(self, &self.doc.pages[self.page].children, 0, &mut out);
        out
    }

    fn layers(&mut self, ui: &mut Ui, rows: &[(NodeId, usize)]) {
        let mut rects: Vec<(NodeId, Rect)> = vec![];
        let shift = ui.input(|i| i.modifiers.shift || i.modifiers.command);
        for &(id, depth) in rows {
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click_and_drag());
            rects.push((id, rect));
            let n = self.doc.node(id).clone();
            let selected = self.sel.contains(&id);
            // Screen readers and computer-use agents see the row as a named, selectable item.
            resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, &n.name));
            let in_selected = !selected && self.sel.iter().any(|&s| self.doc.is_ancestor(s, id));
            let p = &ui.painter().clone();
            if selected {
                p.rect_filled(rect, 0.0, theme::sel_bg(ui.ctx()));
            } else if in_selected {
                p.rect_filled(rect, 0.0, theme::child_bg(ui.ctx()));
            } else if resp.hovered() || self.hover == Some(id) {
                p.rect_stroke(rect.shrink(0.5), 0.0, Stroke::new(1.0, BLUE), StrokeKind::Inside);
            }
            let comp = self.accent(id) == crate::canvas::PURPLE;
            let fg = match (comp, n.visible) {
                (true, true) => crate::canvas::PURPLE,
                (true, false) => crate::canvas::PURPLE.gamma_multiply(0.45),
                (false, true) => theme::ink(ui.ctx()),
                (false, false) => theme::dim(ui.ctx()),
            };
            let x = rect.min.x + 6.0 + depth as f32 * 16.0;
            let chev = Rect::from_center_size(pos2(x + 6.0, rect.center().y), vec2(12.0, 12.0));
            if !n.children.is_empty() {
                paint_icon(p, chev, Icon::Chevron(n.expanded), theme::muted(ui.ctx()));
            }
            paint_icon(p, Rect::from_center_size(pos2(x + 22.0, rect.center().y), vec2(16.0, 16.0)), kind_icon(n.kind), fg);
            let name_pos = pos2(x + 36.0, rect.center().y);
            let renaming = matches!(&self.renaming, Some((r, _)) if *r == id);
            if renaming {
                let mut buf = self.renaming.as_ref().unwrap().1.clone();
                let edit_rect = Rect::from_min_max(pos2(name_pos.x - 2.0, rect.min.y + 3.0), rect.max - vec2(48.0, 3.0));
                let r = ui.put(edit_rect, egui::TextEdit::singleline(&mut buf));
                // Focus once; Enter or clicking away commits, Escape cancels, and either way the
                // keyboard goes back to the canvas.
                let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
                if !r.has_focus() && !r.lost_focus() && !cancel {
                    r.request_focus();
                }
                self.renaming = Some((id, buf.clone()));
                if r.lost_focus() || cancel {
                    self.renaming = None;
                    if r.has_focus() {
                        r.surrender_focus();
                    }
                    let name = if cancel { String::new() } else { buf.trim().to_string() };
                    if !name.is_empty() && name != self.doc.node(id).name {
                        self.checkpoint();
                        self.doc.node_mut(id).name = name;
                    }
                }
            } else {
                let strong = (n.parent.is_none() && n.kind.is_frame()) || n.kind == Kind::Component;
                let font = if strong { FontId::new(12.0, egui::FontFamily::Name("Inter SemiBold".into())) } else { FontId::proportional(12.0) };
                p.text(name_pos, egui::Align2::LEFT_CENTER, &n.name, font, fg);
            }
            // Eye / lock: always shown when active, otherwise on hover.
            let hovered = resp.hovered();
            let eye_r = Rect::from_center_size(pos2(rect.max.x - 16.0, rect.center().y), vec2(20.0, 20.0));
            let lock_r = Rect::from_center_size(pos2(rect.max.x - 38.0, rect.center().y), vec2(20.0, 20.0));
            let (visible, locked) = (n.visible, n.locked);
            if hovered || !visible {
                paint_icon(p, eye_r, Icon::Eye(visible), theme::icon(ui.ctx()));
            }
            if hovered || locked {
                paint_icon(p, lock_r, Icon::Lock(locked), theme::icon(ui.ctx()));
            }
            let click_pos = resp.interact_pointer_pos();
            if resp.clicked() {
                let at = |r: Rect| click_pos.is_some_and(|c| r.contains(c));
                if at(eye_r) {
                    self.checkpoint();
                    self.doc.node_mut(id).visible = !visible;
                } else if at(lock_r) {
                    self.checkpoint();
                    self.doc.node_mut(id).locked = !locked;
                    if !locked {
                        self.sel.retain(|s| *s != id);
                    }
                } else if at(chev.expand(4.0)) && !self.doc.node(id).children.is_empty() {
                    let n = self.doc.node_mut(id);
                    n.expanded = !n.expanded;
                } else if shift {
                    if selected { self.sel.retain(|s| *s != id) } else { self.sel.push(id) }
                } else {
                    self.end_text_edit();
                    self.sel = vec![id];
                }
            }
            if resp.double_clicked() && !click_pos.is_some_and(|c| eye_r.contains(c) || lock_r.contains(c)) {
                self.renaming = Some((id, self.doc.node(id).name.clone()));
            }
            if resp.drag_started() {
                self.layer_drag = Some(id);
            }
            // Right-click a row: select it (unless it's already in the selection) and show the canvas menu.
            if resp.secondary_clicked() && !self.sel.contains(&id) {
                self.sel = vec![id];
            }
            resp.context_menu(|ui| self.context_menu(ui));
        }
        self.layer_drop(ui, &rects);
    }

    /// Drag-and-drop reordering in the layers list.
    fn layer_drop(&mut self, ui: &mut Ui, rects: &[(NodeId, Rect)]) {
        let Some(dragged) = self.layer_drag else { return };
        let (ptr, released) = ui.input(|i| (i.pointer.hover_pos(), i.pointer.any_released()));
        let target = ptr.and_then(|p| {
            rects.iter().find(|(_, r)| p.y >= r.min.y && p.y < r.max.y).map(|&(id, r)| {
                let t = (p.y - r.min.y) / r.height();
                let container = self.doc.node(id).kind.is_container();
                let zone = if container && (0.3..0.7).contains(&t) { 1 } else if t < 0.5 { 0 } else { 2 };
                (id, r, zone)
            })
        });
        let valid = |id: NodeId| id != dragged && !self.doc.is_ancestor(dragged, id);
        if let Some((id, r, zone)) = target.filter(|t| valid(t.0)) {
            let p = ui.painter();
            match zone {
                1 => {
                    p.rect_stroke(r.shrink(1.0), 2.0, Stroke::new(2.0, BLUE), StrokeKind::Inside);
                }
                z => {
                    let y = if z == 0 { r.min.y } else { r.max.y };
                    p.line_segment([pos2(r.min.x + 8.0, y), pos2(r.max.x, y)], Stroke::new(2.0, BLUE));
                }
            }
            if released {
                self.checkpoint();
                let n = self.doc.node(id);
                let into_top = zone == 1 || (zone == 2 && n.expanded && !n.children.is_empty());
                if into_top {
                    self.doc.reparent(dragged, self.page, Some(id), None);
                } else {
                    let parent = n.parent;
                    let list: Vec<NodeId> = self.doc.children_of(self.page, parent).iter().copied().filter(|c| *c != dragged).collect();
                    let i = list.iter().position(|c| *c == id).unwrap_or(0);
                    // The list is drawn top-first, so "above" means a higher z index.
                    self.doc.reparent(dragged, self.page, parent, Some(if zone == 0 { i + 1 } else { i }));
                }
                self.doc.reflow();
                self.sel = vec![dragged];
            }
        }
        if released {
            self.layer_drag = None;
        }
    }

    // ---------- right: design properties ----------

    pub fn right_panel(&mut self, ui: &mut Ui) {
        egui::Panel::right("right").resizable(false).exact_size(248.0).show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if self.dev_mode {
                    let green = Color32::from_rgb(0x14, 0xAE, 0x5C);
                    let pill = egui::Button::new(RichText::new("Dev Mode").color(Color32::WHITE).strong()).fill(green);
                    if ui.add(pill).on_hover_text("Leave Dev Mode (⇧D)").clicked() {
                        self.dev_mode = false;
                    }
                    return;
                }
                for (proto, label) in [(false, "Design"), (true, "Prototype")] {
                    let text = RichText::new(label);
                    let text = if self.prototype_tab == proto { text.strong() } else { text.color(theme::muted(ui.ctx())) };
                    if ui.add(egui::Button::new(text).frame(false)).clicked() {
                        self.prototype_tab = proto;
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let ctx = ui.ctx().clone();
                    // Collaborators' avatars; clicking opens the session window.
                    if let Some(c) = &self.collab {
                        let mut open = false;
                        for p in c.peers.values().take(4) {
                            let (r, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
                            let col = crate::collab::PEER_COLORS[p.color % crate::collab::PEER_COLORS.len()];
                            ui.painter().circle_filled(r.center(), 10.0, col);
                            ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, p.name.chars().next().unwrap_or('?').to_string(), FontId::proportional(10.0), Color32::WHITE);
                            open |= resp.on_hover_text(&p.name).clicked();
                        }
                        if open {
                            self.show_collab = true;
                        }
                    }
                    if icon_button(ui, Icon::Play, false, 24.0, "Present (⌥⌘↵)").clicked() {
                        self.present(ctx.input(|i| i.time));
                    }
                    ui.menu_button(format!("{}%", (self.view.zoom * 100.0).round()), |ui| {
                        if ui.add(egui::Button::new("Zoom in").shortcut_text("⌘+")).clicked() {
                            self.zoom_step(&ctx, 2.0);
                        }
                        if ui.add(egui::Button::new("Zoom out").shortcut_text("⌘-")).clicked() {
                            self.zoom_step(&ctx, 0.5);
                        }
                        if ui.add(egui::Button::new("Zoom to fit").shortcut_text("⇧1")).clicked() {
                            self.zoom_to(&ctx, None, false);
                        }
                        if ui.add_enabled(!self.sel.is_empty(), egui::Button::new("Zoom to selection").shortcut_text("⇧2")).clicked() {
                            self.zoom_to(&ctx, Some(self.sel.clone()), false);
                        }
                        if ui.add(egui::Button::new("Zoom to 100%").shortcut_text("⇧0")).clicked() {
                            self.zoom_to(&ctx, None, true);
                        }
                    });
                });
            });
            ui.separator();
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.set_max_width(ui.available_width());
                // Never let a long label widen the panel.
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                ui.spacing_mut().item_spacing.y = 6.0;
                if self.dev_mode {
                    self.dev_panel(ui);
                    return;
                }
                if self.tool == Tool::Comment {
                    self.comments_panel(ui);
                    return;
                }
                // Frame tool with nothing being drawn: Figma's frame presets.
                if matches!(self.tool, Tool::Frame) && self.drag.is_none() && !self.prototype_tab {
                    self.frame_presets(ui);
                    return;
                }
                if self.prototype_tab {
                    self.prototype_props(ui);
                    return;
                }
                if self.sel.is_empty() {
                    self.page_props(ui);
                } else {
                    self.selection_props(ui);
                }
                ui.separator();
                self.export_props(ui);
            });
        });
    }

    /// The Frame tool's sidebar: click a preset to add a frame that size.
    fn frame_presets(&mut self, ui: &mut Ui) {
        let muted = theme::muted(ui.ctx());
        let mut pick: Option<(&str, f32, f32)> = None;
        for (group, items) in FRAME_PRESETS {
            egui::CollapsingHeader::new(RichText::new(*group).strong()).default_open(*group == "Phone" || *group == "Desktop").show(ui, |ui| {
                for &(name, w, h) in *items {
                    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(r, 4.0, theme::subtle(ui.ctx()));
                    }
                    ui.painter().text(r.left_center() + vec2(6.0, 0.0), egui::Align2::LEFT_CENTER, name, FontId::proportional(12.0), ui.visuals().text_color());
                    ui.painter().text(r.right_center() - vec2(6.0, 0.0), egui::Align2::RIGHT_CENTER, format!("{w}×{h}"), FontId::proportional(11.5), muted);
                    if resp.clicked() {
                        pick = Some((name, w as f32, h as f32));
                    }
                }
            });
        }
        if let Some((name, w, h)) = pick {
            self.add_preset_frame(ui.ctx(), name, w, h);
        }
    }

    /// A new frame to the right of what's on the page (or mid-screen on an empty page), selected.
    pub fn add_preset_frame(&mut self, ctx: &egui::Context, name: &str, w: f32, h: f32) {
        self.checkpoint();
        let kids = self.doc.pages[self.page].children.clone();
        let b = self.doc.bounds(&kids);
        let at = if kids.is_empty() || !b.is_finite() { (self.view_center_world() - vec2(w, h) / 2.0).round() } else { pos2(b.max.x + 100.0, b.min.y).round() };
        let mut n = crate::doc::Node::new(Kind::Frame, Rect::from_min_size(at, vec2(w, h)));
        n.name = name.to_string();
        let id = self.doc.insert(self.page, None, None, n);
        self.sel = vec![id];
        self.set_tool(Tool::Move);
        // Bring it into view without changing the zoom unless it doesn't fit.
        let screen = self.canvas_rect.unwrap_or(ctx.content_rect());
        let r = self.doc.node(id).rect;
        let on = Rect::from_min_max(self.view.to_screen(r.min), self.view.to_screen(r.max));
        if !screen.contains_rect(on) {
            if on.width() < screen.width() * 0.9 && on.height() < screen.height() * 0.9 {
                self.view.pan += screen.center() - on.center();
            } else {
                self.zoom_target = Some(id);
            }
        }
    }

    fn prototype_props(&mut self, ui: &mut Ui) {
        use crate::doc::{Action, Direction, Reaction, TransitionKind, Trigger};
        use crate::prototype::{describe, frames_on_page, top_frame};
        let frames = frames_on_page(&self.doc, self.page);
        let Some(&id) = self.sel.first() else {
            heading(ui, "Flow starting points");
            let flows = self.doc.pages[self.page].flows.clone();
            if flows.is_empty() {
                ui.label(RichText::new("Select a frame and mark it as a starting point.").color(theme::muted(ui.ctx())));
            }
            for f in flows {
                let name = self.doc.nodes.get(&f).map_or("Missing".into(), |n| n.name.clone());
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                    paint_icon(ui.painter(), r, Icon::Play, BLUE);
                    ui.label(name);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icon_button(ui, Icon::Minus, false, 22.0, "Remove").clicked() {
                            self.checkpoint();
                            self.doc.pages[self.page].flows.retain(|x| *x != f);
                        }
                    });
                });
            }
            return;
        };
        let top = top_frame(&self.doc, id);
        if top == id && self.doc.node(id).kind.is_frame() {
            let mut start = self.doc.pages[self.page].flows.contains(&id);
            if ui.checkbox(&mut start, "Flow starting point").changed() {
                self.checkpoint();
                let flows = &mut self.doc.pages[self.page].flows;
                if start { flows.push(id) } else { flows.retain(|x| *x != id) }
            }
            ui.separator();
        }
        let mut reactions = self.doc.node(id).reactions.clone();
        let mut changed = false;
        if section_title(ui, "Interactions") {
            // A new link points at the next frame to the right (then below), not whatever is first in the layer list.
            let here = self.doc.node(top).rect.min;
            let to = frames
                .iter()
                .copied()
                .filter(|f| *f != top)
                .min_by(|a, b| {
                    let key = |f: NodeId| {
                        let r = self.doc.node(f).rect.min;
                        let ahead = r.x > here.x + 1.0 || (r.y > here.y + 1.0 && (r.x - here.x).abs() <= 1.0);
                        (!ahead, (r - here).length())
                    };
                    let (ka, kb) = (key(*a), key(*b));
                    ka.0.cmp(&kb.0).then(ka.1.total_cmp(&kb.1))
                });
            reactions.push(Reaction { action: to.map_or(Action::Back, Action::Navigate), ..Default::default() });
            changed = true;
        }
        let name = |doc: &crate::doc::Document, f: NodeId| doc.nodes.get(&f).map_or("Missing".to_string(), |n| n.name.clone());
        let mut remove = None;
        for (i, r) in reactions.iter_mut().enumerate() {
            let pid = ui.id().with(("reaction", id, i));
            ui.horizontal(|ui| {
                let open = ui.add(egui::Button::new(describe(&self.doc, r)).frame(false).truncate());
                egui::Popup::menu(&open).id(pid).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                    ui.set_width(240.0);
                    let before = r.clone();
                    egui::Grid::new(pid.with("grid")).num_columns(2).show(ui, |ui| {
                        ui.label("Trigger");
                        let t_label = |t: &Trigger| match t {
                            Trigger::OnClick => "On click".into(),
                            Trigger::OnHover => "On hover".into(),
                            Trigger::WhilePressed => "While pressing".into(),
                            Trigger::AfterDelay(ms) => format!("After delay {ms}ms"),
                        };
                        dropdown(ui, t_label(&r.trigger), |ui| {
                            for t in [Trigger::OnClick, Trigger::OnHover, Trigger::WhilePressed, Trigger::AfterDelay(800)] {
                                let label = t_label(&t);
                                ui.selectable_value(&mut r.trigger, t, label);
                            }
                        });
                        ui.end_row();
                        if let Trigger::AfterDelay(ms) = &mut r.trigger {
                            ui.label("Delay");
                            ui.add(DragValue::new(ms).range(0..=60000).suffix("ms").speed(10.0));
                            ui.end_row();
                        }
                        ui.label("Action");
                        let a_label = match &r.action {
                            Action::None => "None",
                            Action::Navigate(_) => "Navigate to",
                            Action::Back => "Back",
                            Action::OpenOverlay(_) => "Open overlay",
                            Action::CloseOverlay => "Close overlay",
                            Action::OpenUrl(_) => "Open link",
                        };
                        let first = frames.iter().copied().find(|f| *f != top).unwrap_or(top);
                        dropdown(ui, a_label, |ui| {
                            ui.selectable_value(&mut r.action, Action::Navigate(first), "Navigate to");
                            ui.selectable_value(&mut r.action, Action::Back, "Back");
                            ui.selectable_value(&mut r.action, Action::OpenOverlay(first), "Open overlay");
                            ui.selectable_value(&mut r.action, Action::CloseOverlay, "Close overlay");
                            ui.selectable_value(&mut r.action, Action::OpenUrl("https://".into()), "Open link");
                            ui.selectable_value(&mut r.action, Action::None, "None");
                        });
                        ui.end_row();
                        match &mut r.action {
                            Action::Navigate(dest) | Action::OpenOverlay(dest) => {
                                ui.label("Destination");
                                dropdown(ui, name(&self.doc, *dest), |ui| {
                                    for &f in &frames {
                                        ui.selectable_value(dest, f, name(&self.doc, f));
                                    }
                                });
                                ui.end_row();
                            }
                            Action::OpenUrl(url) => {
                                ui.label("URL");
                                ui.text_edit_singleline(url);
                                ui.end_row();
                            }
                            _ => {}
                        }
                        if matches!(r.action, Action::Navigate(_) | Action::Back) {
                            ui.label("Animation");
                            let k_label = |k: TransitionKind| match k {
                                TransitionKind::Instant => "Instant",
                                TransitionKind::Dissolve => "Dissolve",
                                TransitionKind::SmartAnimate => "Smart animate",
                                TransitionKind::MoveIn => "Move in",
                                TransitionKind::Push => "Push",
                                TransitionKind::SlideIn => "Slide in",
                            };
                            dropdown(ui, k_label(r.transition.kind), |ui| {
                                for k in [TransitionKind::Instant, TransitionKind::Dissolve, TransitionKind::SmartAnimate, TransitionKind::MoveIn, TransitionKind::Push, TransitionKind::SlideIn] {
                                    ui.selectable_value(&mut r.transition.kind, k, k_label(k));
                                }
                            });
                            ui.end_row();
                            if matches!(r.transition.kind, TransitionKind::MoveIn | TransitionKind::Push | TransitionKind::SlideIn) {
                                ui.label("Direction");
                                ui.horizontal(|ui| {
                                    for (d, l) in [(Direction::Left, "←"), (Direction::Right, "→"), (Direction::Up, "↑"), (Direction::Down, "↓")] {
                                        ui.selectable_value(&mut r.transition.direction, d, l);
                                    }
                                });
                                ui.end_row();
                            }
                            if r.transition.kind != TransitionKind::Instant {
                                ui.label("Duration");
                                let mut ms = (r.transition.duration * 1000.0).round();
                                if ui.add(DragValue::new(&mut ms).range(0.0..=10000.0).suffix("ms").speed(10.0)).changed() {
                                    r.transition.duration = ms / 1000.0;
                                }
                                ui.end_row();
                            }
                        }
                    });
                    changed |= *r != before;
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon_button(ui, Icon::Minus, false, 22.0, "Remove").clicked() {
                        remove = Some(i);
                    }
                });
            });
        }
        if let Some(i) = remove {
            reactions.remove(i);
            changed = true;
        }
        if changed {
            self.begin_edit();
            self.doc.node_mut(id).reactions = reactions;
        }
    }

    /// Dev Mode inspector: size, generated code, colors, and type used by the selection.
    fn dev_panel(&mut self, ui: &mut Ui) {
        use crate::codegen::{Lang, generate};
        let Some(&id) = self.sel.first() else {
            ui.add_space(12.0);
            ui.label(RichText::new("Select a layer to inspect its code and properties.").color(theme::muted(ui.ctx())));
            return;
        };
        let n = self.doc.node(id).clone();
        ui.label(RichText::new(&n.name).strong().size(14.0));
        ui.label(RichText::new(format!("{}  ·  {} × {}", n.kind.label(), num(n.rect.width()), num(n.rect.height()))).color(theme::muted(ui.ctx())));
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Code").strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                for (lang, label) in [(Lang::Compose, "Compose"), (Lang::SwiftUi, "SwiftUI"), (Lang::Css, "CSS")] {
                    ui.selectable_value(&mut self.code_lang, lang, label);
                }
            });
        });
        let code = generate(&self.doc, id, self.code_lang);
        egui::Frame::new().fill(theme::subtle(ui.ctx())).corner_radius(6).inner_margin(8).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            ui.add(egui::Label::new(RichText::new(&code).monospace().size(11.0)).selectable(true));
        });
        if ui.add_sized([ui.available_width(), 24.0], egui::Button::new("Copy code")).clicked() {
            ui.ctx().copy_text(code);
            self.toast("Code copied".into());
        }
        // Every solid color and text style used in the selection, like Dev Mode's lists.
        let ids: Vec<NodeId> = std::iter::once(id).chain(self.doc.descendants(id)).collect();
        let mut colors: Vec<Rgba> = vec![];
        let mut type_styles: Vec<String> = vec![];
        for &d in &ids {
            let dn = self.doc.node(d);
            for p in dn.fills.iter().chain(dn.strokes.iter()).filter(|p| p.visible && p.kind == crate::doc::PaintKind::Solid) {
                if !colors.contains(&p.color) {
                    colors.push(p.color);
                }
            }
            if dn.kind == Kind::Text {
                let t = format!("{} {} · {}px", dn.font_family, crate::text::weight_name(dn.font_weight), num(dn.font_size));
                if !type_styles.contains(&t) {
                    type_styles.push(t);
                }
            }
        }
        if !colors.is_empty() {
            ui.separator();
            heading(ui, "Colors");
            for c in colors {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                    ui.painter().rect_filled(r, 3.0, c.to32());
                    ui.painter().rect_stroke(r, 3.0, Stroke::new(1.0, Color32::from_black_alpha(25)), StrokeKind::Inside);
                    let hex = format!("#{}", c.hex());
                    let alpha = if c.a() < 255 { format!("  {}%", (c.a() as f32 / 2.55).round()) } else { String::new() };
                    if ui.add(egui::Label::new(RichText::new(format!("{hex}{alpha}")).monospace()).sense(Sense::click())).on_hover_text("Click to copy").clicked() {
                        ui.ctx().copy_text(hex);
                    }
                });
            }
        }
        if !type_styles.is_empty() {
            ui.separator();
            heading(ui, "Typography");
            for t in type_styles {
                ui.label(t);
            }
        }
        ui.separator();
        self.export_props(ui);
    }

    fn page_props(&mut self, ui: &mut Ui) {
        heading(ui, "Page");
        let mut bg = self.doc.pages[self.page].background;
        ui.horizontal(|ui| {
            let mut c = [bg.0[0], bg.0[1], bg.0[2]];
            let mut changed = egui::color_picker::color_edit_button_srgb(ui, &mut c).changed();
            bg = Rgba::rgb(c[0], c[1], c[2]);
            changed |= hex_field(ui, ui.id().with("page-bg"), &mut bg, 56.0);
            if changed {
                self.begin_edit();
                self.doc.pages[self.page].background = bg;
            }
        });
        self.local_styles(ui);
        ui.separator();
        if ui.add_sized([ui.available_width(), 24.0], egui::Button::new("Local variables")).clicked() {
            self.show_variables = true;
        }
    }

    /// With nothing selected, list the file's styles like Figma's "Local styles".
    fn local_styles(&mut self, ui: &mut Ui) {
        use crate::doc::StyleValue;
        if self.doc.styles.is_empty() {
            return;
        }
        ui.separator();
        heading(ui, "Local styles");
        let mut delete = None;
        for (label, is) in [
            ("Color styles", (|v: &StyleValue| matches!(v, StyleValue::Paint(_))) as fn(&StyleValue) -> bool),
            ("Text styles", |v: &StyleValue| matches!(v, StyleValue::Text(_))),
            ("Effect styles", |v: &StyleValue| matches!(v, StyleValue::Effect(_))),
        ] {
            let list: Vec<crate::doc::Style> = self.doc.styles.iter().filter(|s| is(&s.value)).cloned().collect();
            if list.is_empty() {
                continue;
            }
            ui.label(RichText::new(label).color(theme::muted(ui.ctx())).small());
            for st in list {
                ui.horizontal(|ui| {
                    crate::paint_ui::style_preview(ui, &st.value);
                    let mut name = st.name.clone();
                    let r = ui.add(egui::TextEdit::singleline(&mut name).frame(egui::Frame::NONE).desired_width(130.0));
                    if r.changed() {
                        self.begin_edit();
                        if let Some(x) = self.doc.styles.iter_mut().find(|x| x.id == st.id) {
                            x.name = name;
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icon_button(ui, Icon::Minus, false, 22.0, "Delete style").clicked() {
                            delete = Some(st.id);
                        }
                        if let StyleValue::Paint(ps) = &st.value {
                            let edit = icon_button(ui, Icon::Styles, false, 22.0, "Edit style");
                            egui::Popup::menu(&edit).id(ui.id().with(("local-style", st.id))).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                                ui.set_width(240.0);
                                let mut ps = ps.clone();
                                let mut edited = false;
                                for (i, p) in ps.iter_mut().enumerate().rev() {
                                    if !matches!(paint_row(ui, ui.id().with(("ls-paint", st.id, i)), p), PaintEvent::None) {
                                        edited = true;
                                    }
                                }
                                if edited {
                                    self.begin_edit();
                                    if let Some(x) = self.doc.styles.iter_mut().find(|x| x.id == st.id) {
                                        x.value = StyleValue::Paint(ps);
                                    }
                                    self.doc.reflow();
                                }
                            });
                        }
                    });
                });
            }
        }
        if let Some(id) = delete {
            self.checkpoint();
            self.doc.styles.retain(|s| s.id != id);
            self.doc.reflow();
        }
    }

    fn selection_props(&mut self, ui: &mut Ui) {
        let roots = self.sel_roots();
        let first = self.sel[0];
        let n = self.doc.node(first).clone();
        self.component_section(ui, first);
        if roots.len() > 1 && roots.iter().all(|&r| self.doc.node(r).kind == Kind::Component) {
            if ui.add_sized([ui.available_width(), 24.0], egui::Button::new(RichText::new("Combine as variants").color(crate::canvas::PURPLE))).clicked() {
                self.combine_as_variants();
            }
            ui.separator();
        }
        let is_bool = roots.len() == 1 && n.kind == Kind::Boolean;
        if roots.len() > 1 || is_bool {
            ui.horizontal(|ui| {
                for op in [crate::doc::BoolOp::Union, crate::doc::BoolOp::Subtract, crate::doc::BoolOp::Intersect, crate::doc::BoolOp::Exclude] {
                    let on = is_bool && n.bool_op == op;
                    if ui.selectable_label(on, format!("{op:?}")).on_hover_text(format!("{op:?} selection")).clicked() {
                        self.boolean_selection(op);
                    }
                }
            });
            ui.separator();
        }
        self.position_section(ui, &roots);
        ui.separator();
        self.layout_section(ui, &roots);
        if let [one] = roots[..] {
            self.constraints_section(ui, one);
        }
        ui.separator();
        self.appearance_section(ui, &n);
        if self.sel.iter().all(|&id| self.doc.node(id).kind == Kind::Text) {
            ui.separator();
            self.typography_section(ui, &n);
        }
        if roots.len() == 1 && n.kind.is_frame() {
            ui.separator();
            self.layout_guides(ui, n.id);
        }
        if n.kind == Kind::Ellipse {
            // Figma's arc controls: start angle, sweep (as a percentage of a full turn), ratio.
            let [mut start, sweep, ratio] = n.arc.unwrap_or([0.0, 360.0, 0.0]);
            let (mut sweep_pct, mut ratio_pct) = (sweep / 3.6, ratio * 100.0);
            let mut changed = false;
            ui.horizontal(|ui| {
                ui.label("Arc");
                changed |= ui.add(DragValue::new(&mut start).speed(1.0).suffix("°")).on_hover_text("Start").changed();
                changed |= ui.add(DragValue::new(&mut sweep_pct).range(-100.0..=100.0).speed(0.5).suffix("%")).on_hover_text("Sweep").changed();
                changed |= ui.add(DragValue::new(&mut ratio_pct).range(0.0..=99.0).speed(0.5).suffix("%")).on_hover_text("Ratio").changed();
            });
            if changed {
                self.begin_edit();
                let arc = [start.rem_euclid(360.0), sweep_pct * 3.6, ratio_pct / 100.0];
                for &id in &self.sel.clone() {
                    if self.doc.node(id).kind == Kind::Ellipse {
                        self.doc.node_mut(id).arc = (arc != [0.0, 360.0, 0.0]).then_some(arc);
                    }
                }
            }
        }
        if matches!(n.kind, Kind::Polygon | Kind::Star) {
            let (mut count, mut ratio) = (n.point_count as f32, n.inner_ratio * 100.0);
            let mut changed = false;
            ui.horizontal(|ui| {
                ui.label("Count");
                changed |= ui.add(DragValue::new(&mut count).range(3.0..=60.0).speed(0.1)).changed();
                if n.kind == Kind::Star {
                    ui.label("Ratio");
                    changed |= ui.add(DragValue::new(&mut ratio).range(1.0..=100.0).speed(0.5).suffix("%")).changed();
                }
            });
            if changed {
                self.begin_edit();
                for &id in &self.sel.clone() {
                    let t = self.doc.node_mut(id);
                    t.point_count = count.round() as u32;
                    t.inner_ratio = ratio / 100.0;
                }
            }
        }
        if n.kind != Kind::Group {
            ui.separator();
            self.paints_section(ui, false);
            ui.separator();
            self.paints_section(ui, true);
        }
        ui.separator();
        self.effects_section(ui);
        self.selection_colors(ui, &roots);
        if roots.len() == 1 {
            self.variables_section(ui, roots[0]);
        }
    }

    /// Every solid color in the selection and its layers; editing one recolors all its uses.
    fn selection_colors(&mut self, ui: &mut Ui, roots: &[NodeId]) {
        if roots.len() < 2 && self.doc.node(roots[0]).children.is_empty() {
            return;
        }
        let ids: Vec<NodeId> = roots.iter().flat_map(|&r| std::iter::once(r).chain(self.doc.descendants(r))).collect();
        let mut colors: Vec<Rgba> = vec![];
        for &id in &ids {
            let n = self.doc.node(id);
            for p in n.fills.iter().chain(&n.strokes).filter(|p| p.visible && p.kind == crate::doc::PaintKind::Solid) {
                if !colors.contains(&p.color) {
                    colors.push(p.color);
                }
            }
        }
        if colors.is_empty() {
            return;
        }
        ui.separator();
        heading(ui, "Selection colors");
        let all_id = ui.id().with("selection-colors-all");
        let all = ui.data(|d| d.get_temp::<bool>(all_id)).unwrap_or(false);
        let mut edit = None;
        for (i, &c) in colors.iter().enumerate().take(if all { usize::MAX } else { 6 }) {
            let mut nc = c;
            if crate::paint_ui::color_row(ui, ui.id().with(("selection-color", i)), &mut nc) {
                edit = Some((c, nc));
            }
        }
        if colors.len() > 6 && ui.link(if all { "Show fewer".to_string() } else { format!("See all {} colors", colors.len()) }).clicked() {
            ui.data_mut(|d| d.insert_temp(all_id, !all));
        }
        if let Some((old, new)) = edit {
            self.begin_edit();
            for id in ids {
                let n = self.doc.node_mut(id);
                // A recolored paint no longer follows its style or variable.
                let mut hit = [false; 2];
                for (k, paints) in [&mut n.fills, &mut n.strokes].into_iter().enumerate() {
                    for p in paints.iter_mut().filter(|p| p.visible && p.kind == crate::doc::PaintKind::Solid && p.color == old) {
                        p.color = new;
                        hit[k] = true;
                    }
                }
                if hit[0] {
                    n.fill_style = None;
                    n.bindings.retain(|(b, _)| *b != crate::doc::Bindable::Fill);
                }
                if hit[1] {
                    n.stroke_style = None;
                    n.bindings.retain(|(b, _)| *b != crate::doc::Bindable::Stroke);
                }
            }
        }
    }

    /// Components to swap an instance to, by page, with a filter. True once swapped.
    pub fn swap_menu(&mut self, ui: &mut Ui, inst: NodeId) -> bool {
        let fid = ui.id().with(("swap-filter", inst));
        let mut q: String = ui.data(|d| d.get_temp(fid)).unwrap_or_default();
        ui.add(egui::TextEdit::singleline(&mut q).hint_text("Search components").desired_width(200.0));
        ui.data_mut(|d| d.insert_temp(fid, q.clone()));
        let q = q.to_lowercase();
        let current = self.doc.node(inst).component;
        let mut pick = None;
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            for (pi, page) in self.doc.pages.iter().enumerate() {
                // Sets swap to their first variant; components inside sets are reached through them.
                let mut items: Vec<(NodeId, String)> = vec![];
                let mut stack = page.children.clone();
                while let Some(id) = stack.pop() {
                    let n = self.doc.node(id);
                    match n.kind {
                        Kind::ComponentSet => {
                            if let Some(&first) = n.children.first() {
                                items.push((first, n.name.clone()));
                            }
                        }
                        Kind::Component => items.push((id, n.name.clone())),
                        _ => stack.extend(n.children.iter().copied()),
                    }
                }
                items.retain(|(_, name)| q.is_empty() || name.to_lowercase().contains(&q));
                if items.is_empty() {
                    continue;
                }
                items.sort_by(|a, b| a.1.cmp(&b.1));
                ui.label(RichText::new(&self.doc.pages[pi].name).small().color(theme::muted(ui.ctx())));
                for (cid, name) in items {
                    let on = current == Some(cid) || current.and_then(|c| self.doc.nodes.get(&c)).and_then(|c| c.parent) == self.doc.node(cid).parent.filter(|p| self.doc.node(*p).kind == Kind::ComponentSet);
                    if ui.add(egui::Button::selectable(on, name)).clicked() && !on {
                        pick = Some(cid);
                    }
                }
            }
        });
        if let Some(c) = pick {
            self.checkpoint();
            self.doc.swap_component(inst, c);
            self.doc.reflow();
            return true;
        }
        false
    }

    fn component_section(&mut self, ui: &mut Ui, id: NodeId) {
        use crate::doc::{CompProp, PropTarget, PropValue};
        let n = self.doc.node(id).clone();
        let inst = self.owning_instance(id);
        // The component (not instance) this layer lives in, if any.
        let owner = std::iter::once(id).chain(self.doc.ancestors(id)).find(|a| matches!(self.doc.node(*a).kind, Kind::Component | Kind::ComponentSet));
        if owner.is_none() && inst.is_none() {
            return;
        }
        let purple = crate::canvas::PURPLE;
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
            let icon = if inst.is_some() { Icon::Instance } else { Icon::Component };
            paint_icon(ui.painter(), rect, icon, purple);
            let title = match inst {
                Some(i) => {
                    let main = self.doc.node(i).component.and_then(|c| self.doc.nodes.get(&c)).map_or("Missing component".into(), |c| c.name.clone());
                    format!("Instance of {main}")
                }
                None => self.doc.node(owner.unwrap()).name.clone(),
            };
            // An instance's title is its swap picker, as in Figma.
            match inst.filter(|i| *i == id) {
                Some(i) => {
                    ui.menu_button(RichText::new(title).strong().color(purple), |ui| {
                        if self.swap_menu(ui, i) {
                            ui.close();
                        }
                    })
                    .response
                    .on_hover_text("Swap instance");
                }
                None => {
                    ui.label(RichText::new(title).strong().color(purple));
                }
            }
        });
        if let Some(i) = inst.filter(|i| *i == id) {
            let comp = self.doc.node(i).component.filter(|c| self.doc.nodes.contains_key(c));
            // Variant pickers when the main lives in a component set.
            if let Some(c) = comp {
                if let Some(set) = self.doc.node(c).parent.filter(|p| self.doc.node(*p).kind == Kind::ComponentSet) {
                    let current = crate::doc::variant_pairs(&self.doc.node(c).name);
                    for (prop, values) in self.doc.variant_props(set) {
                        let cur = current.iter().find(|(k, _)| *k == prop).map(|(_, v)| v.clone()).unwrap_or_default();
                        let mut pick = cur.clone();
                        ui.horizontal(|ui| {
                            ui.label(&prop);
                            egui::ComboBox::from_id_salt(("variant", i, &prop)).selected_text(&pick).width(110.0).show_ui(ui, |ui| {
                                for v in &values {
                                    ui.selectable_value(&mut pick, v.clone(), v);
                                }
                            });
                        });
                        if pick != cur {
                            let mut want = current.clone();
                            want.retain(|(k, _)| *k != prop);
                            want.push((prop.clone(), pick));
                            if let Some(v) = self.doc.find_variant(set, &want) {
                                self.checkpoint();
                                self.doc.swap_component(i, v);
                                self.doc.reflow();
                            }
                        }
                    }
                }
                // Component property values.
                let props = self.doc.props_of(c);
                let mut values = self.doc.node(i).prop_values.clone();
                let mut changed = false;
                for p in &props {
                    let cur = values.iter().find(|(k, _)| *k == p.name).map(|(_, v)| v.clone()).unwrap_or(p.default.clone());
                    let mut next = cur.clone();
                    ui.horizontal(|ui| {
                        ui.label(&p.name);
                        match &mut next {
                            PropValue::Bool(b) => {
                                ui.checkbox(b, "");
                            }
                            PropValue::Text(t) => {
                                ui.text_edit_singleline(t);
                            }
                            PropValue::Swap(target) => {
                                let comps: Vec<NodeId> = self.doc.nodes.values().filter(|n| n.kind == Kind::Component).map(|n| n.id).collect();
                                egui::ComboBox::from_id_salt(("swap", i, &p.name)).selected_text(self.doc.nodes.get(target).map_or("Missing".into(), |n| n.name.clone())).show_ui(ui, |ui| {
                                    for cid in comps {
                                        let name = self.doc.node(cid).name.clone();
                                        ui.selectable_value(target, cid, name);
                                    }
                                });
                            }
                        }
                    });
                    if next != cur {
                        values.retain(|(k, _)| *k != p.name);
                        values.push((p.name.clone(), next));
                        changed = true;
                    }
                }
                if changed {
                    self.begin_edit();
                    self.doc.node_mut(i).prop_values = values;
                    self.doc.reflow();
                }
            }
            ui.horizontal(|ui| {
                if ui.button("Go to main").clicked() {
                    self.go_to_main(i);
                }
                if ui.button("Reset").clicked() {
                    self.checkpoint();
                    self.doc.reset_instance(i);
                }
                if ui.button("Detach").clicked() {
                    self.checkpoint();
                    self.doc.detach_instance(i);
                }
            });
        } else if let Some(o) = owner.filter(|_| inst.is_none()) {
            let on = self.doc.node(o).clone();
            // Properties are defined on the set when there is one.
            let def = match on.parent.map(|p| self.doc.node(p).kind) {
                Some(Kind::ComponentSet) if on.kind == Kind::Component => on.parent.unwrap(),
                _ => o,
            };
            if id == o || id == def {
                let mut props = self.doc.node(def).comp_props.clone();
                let mut changed = false;
                let mut add_variant_prop = false;
                let mut bind_texts: Vec<(NodeId, String)> = vec![];
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Properties").strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let add = icon_button(ui, Icon::Plus, false, 22.0, "Add property");
                        egui::Popup::menu(&add).id(ui.id().with(("add-prop", def))).show(|ui| {
                            let fresh = |props: &[CompProp], base: &str| {
                                (1..).map(|k| format!("{base} {k}")).find(|nm| !props.iter().any(|p| &p.name == nm)).unwrap()
                            };
                            if ui.button("Variant").clicked() {
                                add_variant_prop = true;
                                ui.close();
                            }
                            if ui.button("Boolean").clicked() {
                                props.push(CompProp { name: fresh(&props, "Show"), default: PropValue::Bool(true) });
                                changed = true;
                                ui.close();
                            }
                            if ui.button("Text").clicked() {
                                let name = fresh(&props, "Text");
                                // Like Figma's "Create property" from a text layer: when the component
                                // has one unbound text layer per variant, the property drives it.
                                let variants: Vec<NodeId> = if self.doc.node(def).kind == Kind::ComponentSet { self.doc.node(def).children.clone() } else { vec![def] };
                                let texts: Vec<Vec<NodeId>> = variants
                                    .iter()
                                    .map(|v| self.doc.descendants(*v).into_iter().filter(|d| self.doc.node(*d).kind == Kind::Text && !self.doc.node(*d).prop_bind.iter().any(|(_, t)| *t == PropTarget::Text)).collect())
                                    .collect();
                                let default = if texts.iter().all(|t| t.len() == 1) {
                                    let first = self.doc.node(texts[0][0]).text.clone();
                                    bind_texts = texts.into_iter().map(|t| (t[0], name.clone())).collect();
                                    first
                                } else {
                                    "Text".into()
                                };
                                props.push(CompProp { name, default: PropValue::Text(default) });
                                changed = true;
                                ui.close();
                            }
                            if ui.button("Instance swap").clicked() {
                                props.push(CompProp { name: fresh(&props, "Icon"), default: PropValue::Swap(o) });
                                changed = true;
                                ui.close();
                            }
                        });
                    });
                });
                let mut remove = None;
                for (k, p) in props.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        changed |= ui.add(egui::TextEdit::singleline(&mut p.name).desired_width(90.0)).changed();
                        match &mut p.default {
                            PropValue::Bool(b) => changed |= ui.checkbox(b, "").changed(),
                            PropValue::Text(t) => changed |= ui.add(egui::TextEdit::singleline(t).desired_width(70.0)).changed(),
                            PropValue::Swap(_) => {
                                ui.label("Instance");
                            }
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if icon_button(ui, Icon::Minus, false, 20.0, "Remove").clicked() {
                                remove = Some(k);
                            }
                        });
                    });
                }
                if let Some(k) = remove {
                    props.remove(k);
                    changed = true;
                }
                if changed {
                    self.begin_edit();
                    self.doc.node_mut(def).comp_props = props;
                    for (t, name) in bind_texts {
                        self.doc.node_mut(t).prop_bind.push((name, PropTarget::Text));
                    }
                    self.doc.reflow();
                }
                if add_variant_prop {
                    if self.doc.node(def).kind == Kind::ComponentSet {
                        // A new variant property on every variant.
                        self.checkpoint();
                        let existing: Vec<String> = self.doc.variant_props(def).into_iter().map(|(k, _)| k).collect();
                        let name = (1..).map(|k| format!("Property {k}")).find(|nm| !existing.contains(nm)).unwrap();
                        for v in self.doc.node(def).children.clone() {
                            let n = self.doc.node_mut(v);
                            n.name = if crate::doc::variant_pairs(&n.name).is_empty() { format!("{name}=Default") } else { format!("{}, {name}=Default", n.name) };
                        }
                    } else {
                        self.add_variant_to(o);
                    }
                }
                if self.doc.node(def).kind == Kind::ComponentSet && ui.button("Add variant").clicked() {
                    self.add_variant(def);
                } else if on.kind == Kind::Component && def == o && ui.button("Add variant").clicked() {
                    self.add_variant_to(o);
                }
                if on.kind == Kind::Component && self.doc.node(def).kind != Kind::ComponentSet && ui.button("Create instance").clicked() {
                    self.checkpoint();
                    self.sel = self.duplicate_or_instance(&[o]);
                    if let Some(&i) = self.sel.first() {
                        self.doc.translate(i, vec2(on.rect.width() + 40.0, 0.0));
                    }
                }
            } else {
                // A layer inside a component: bind one of its properties.
                let props = self.doc.props_of(o);
                // One row per thing a property can drive here: visibility always, plus text or the swap.
                let mut targets = vec![PropTarget::Visible];
                match n.kind {
                    Kind::Text => targets.insert(0, PropTarget::Text),
                    Kind::Instance => targets.push(PropTarget::Swap),
                    _ => {}
                }
                let fits = |p: &CompProp, t: PropTarget| matches!((&p.default, t), (PropValue::Bool(_), PropTarget::Visible) | (PropValue::Text(_), PropTarget::Text) | (PropValue::Swap(_), PropTarget::Swap));
                let mut bind = n.prop_bind.clone();
                for t in targets {
                    let label = match t {
                        PropTarget::Text => "Text",
                        PropTarget::Visible => "Visible",
                        PropTarget::Swap => "Swap",
                    };
                    let mut cur: Option<String> = bind.iter().find(|(_, bt)| *bt == t).map(|(nm, _)| nm.clone());
                    let before = cur.clone();
                    ui.horizontal(|ui| {
                        ui.label(label);
                        egui::ComboBox::from_id_salt(("bind", id, label)).selected_text(cur.clone().unwrap_or_else(|| "None".into())).show_ui(ui, |ui| {
                            ui.selectable_value(&mut cur, None, "None");
                            for p in props.iter().filter(|p| fits(p, t)) {
                                ui.selectable_value(&mut cur, Some(p.name.clone()), &p.name);
                            }
                        });
                    });
                    if cur != before {
                        bind.retain(|(_, bt)| *bt != t);
                        if let Some(nm) = cur {
                            bind.push((nm, t));
                        }
                    }
                }
                if bind != n.prop_bind {
                    self.checkpoint();
                    self.doc.node_mut(id).prop_bind = bind;
                    self.doc.reflow();
                }
            }
        }
        ui.separator();
    }

    /// Origin that X/Y are reported against: the nearest frame, like Figma.
    fn frame_origin(&self, id: NodeId) -> egui::Vec2 {
        self.doc
            .ancestors(id)
            .into_iter()
            .find(|a| self.doc.node(*a).kind.is_frame())
            .map(|f| self.doc.node(f).rect.min.to_vec2())
            .unwrap_or_default()
    }

    fn position_section(&mut self, ui: &mut Ui, roots: &[NodeId]) {
        heading(ui, "Position");
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for a in [Align::Left, Align::HCenter, Align::Right, Align::Top, Align::VCenter, Align::Bottom] {
                if icon_button(ui, Icon::Align(a), false, 24.0, &format!("Align {a:?}")).clicked() {
                    self.align(a);
                }
            }
            if roots.len() >= 3 {
                if icon_button(ui, Icon::GapH, false, 24.0, "Distribute horizontal spacing (⌃⌥H)").clicked() {
                    self.distribute_selection(true);
                }
                if icon_button(ui, Icon::GapV, false, 24.0, "Distribute vertical spacing (⌃⌥V)").clicked() {
                    self.distribute_selection(false);
                }
            }
        });
        let first = roots[0];
        let b = self.doc.bounds(roots);
        let origin = self.frame_origin(first);
        // A lone rotated layer reports its rotated top-left corner, like Figma.
        let single = (roots.len() == 1).then(|| self.doc.node(first).clone()).filter(|n| n.rotation != 0.0);
        let anchor = single.as_ref().map_or(b.min, |n| n.corners()[0]);
        let (mut x, mut y) = (anchor.x - origin.x, anchor.y - origin.y);
        let mut moved = false;
        ui.horizontal(|ui| {
            moved |= num_field(ui, &mut x, "X", -1e6, half(ui));
            moved |= num_field(ui, &mut y, "Y", -1e6, half(ui));
        });
        if moved {
            self.begin_edit();
            let d = vec2(x + origin.x, y + origin.y) - anchor.to_vec2();
            for &id in roots {
                self.doc.translate(id, d);
            }
            self.doc.reflow();
        }
        // Rotation: a lone layer turns around its center, several turn around their shared center.
        let mut rot = if roots.len() == 1 { self.doc.node(first).rotation } else { 0.0 };
        let old = rot;
        ui.horizontal(|ui| {
            if num_field(ui, &mut rot, "@rotate", -1e6, half(ui)) {
                self.begin_edit();
                let c = self.doc.bounds(roots).center();
                for &id in roots {
                    let pivot = if roots.len() == 1 { self.doc.node(id).rect.center() } else { c };
                    self.doc.rotate(id, rot - old, pivot);
                }
            }
        });
    }

    fn layout_section(&mut self, ui: &mut Ui, roots: &[NodeId]) {
        let first = roots[0];
        let n = self.doc.node(first).clone();
        let is_frame = roots.len() == 1 && n.kind.is_frame();
        let al = is_frame && n.layout.mode != LayoutMode::None;
        ui.horizontal(|ui| {
            ui.label(RichText::new(if al { "Auto layout" } else { "Layout" }).strong());
            if is_frame {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (icon, tip) = if al { (Icon::Minus, "Remove auto layout (⌥⇧A)") } else { (Icon::Plus, "Add auto layout (⇧A)") };
                    if icon_button(ui, icon, false, 22.0, tip).clicked() {
                        if al { self.remove_auto_layout() } else { self.add_auto_layout() }
                    }
                });
            }
        });
        if al {
            self.auto_layout_controls(ui, first);
        }

        // Size, with Figma's Fixed / Hug / Fill modes where they apply.
        let b = self.doc.bounds(roots);
        let single = (roots.len() == 1).then(|| self.doc.node(first).clone()).filter(|n| n.rotation != 0.0);
        let size = single.as_ref().map_or(b.size(), |n| n.rect.size());
        let (mut w, mut h) = (size.x, size.y);
        let parent_al = n.parent.is_some_and(|p| self.doc.node(p).layout.mode != LayoutMode::None) && !n.absolute;
        let can_hug = n.layout.mode != LayoutMode::None;
        let mut sizing = n.sizing;
        let (mut mins, mut maxs) = (n.min_size, n.max_size);
        let mut sized = false;
        let mut sizing_changed = false;
        ui.horizontal(|ui| {
            let menus = roots.len() == 1 && (can_hug || parent_al);
            // Two fields and two 18-point menu buttons, with the gaps between them, fill the row exactly.
            let wide = if menus { (ui.max_rect().width() - 2.0 * 18.0 - 3.0 * ui.spacing().item_spacing.x) / 2.0 } else { half(ui) };
            for (axis, v, label) in [(0usize, &mut w, "W"), (1, &mut h, "H")] {
                let shown = match (menus, sizing[axis]) {
                    (true, Sizing::Hug) => "W Hug",
                    (true, Sizing::Fill) => "W Fill",
                    _ => label,
                };
                let shown = if axis == 1 { shown.replacen('W', "H", 1) } else { shown.to_string() };
                sized |= num_field(ui, v, &shown, 0.01, wide);
                if menus {
                    let cur = *v;
                    sizing_changed |= sizing_menu(ui, ui.id().with(("sizing", axis)), &mut sizing[axis], can_hug, parent_al, axis, (&mut mins[axis], &mut maxs[axis]), cur);
                }
            }
        });
        // Min / max rows, for the bounds that are set.
        if roots.len() == 1 && (mins.iter().chain(&maxs).any(|b| b.is_some())) {
            ui.horizontal_wrapped(|ui| {
                let [mw, mh] = &mut mins;
                let [xw, xh] = &mut maxs;
                for (b, label) in [(mw, "Min W"), (xw, "Max W"), (mh, "Min H"), (xh, "Max H")] {
                    if let Some(v) = b {
                        sizing_changed |= num_field(ui, v, label, 0.0, half(ui) - 4.0);
                    }
                }
            });
        }
        if sizing_changed {
            self.checkpoint();
            let n = self.doc.node_mut(first);
            n.sizing = sizing;
            // Filling text wraps at its width, like Figma.
            if n.kind == Kind::Text && sizing[0] == Sizing::Fill && n.text_sizing == crate::doc::TextSizing::AutoWidth {
                n.text_sizing = crate::doc::TextSizing::AutoHeight;
            }
            n.min_size = mins;
            n.max_size = maxs;
            self.doc.reflow();
        }
        if sized {
            self.begin_edit();
            for &id in roots {
                let s = &mut self.doc.node_mut(id).sizing;
                if (w - size.x).abs() > 1e-4 {
                    s[0] = Sizing::Fixed;
                }
                if (h - size.y).abs() > 1e-4 {
                    s[1] = Sizing::Fixed;
                }
            }
            if let Some(n) = &single {
                // Grow from the rotated top-left corner.
                let local = Rect::from_min_size(n.rect.min, vec2(w, h));
                let c = crate::doc::rotate(local.center(), n.rect.center(), n.rotation);
                self.doc.set_rect(n.id, Rect::from_center_size(c, local.size()));
            } else if b.width() > 0.0 || b.height() > 0.0 {
                let nb = Rect::from_min_size(b.min, vec2(w, h));
                let sx = if b.width() > 0.0 { w / b.width() } else { 1.0 };
                let sy = if b.height() > 0.0 { h / b.height() } else { 1.0 };
                let map = |p: Pos2| pos2(nb.min.x + (p.x - b.min.x) * sx, nb.min.y + (p.y - b.min.y) * sy);
                for &id in roots {
                    let r = self.doc.node(id).rect;
                    self.doc.set_rect(id, Rect::from_two_pos(map(r.min), map(r.max)));
                }
            }
            self.doc.reflow();
        }
        if parent_al || n.absolute {
            let mut abs = n.absolute;
            if ui.checkbox(&mut abs, "Ignore auto layout").changed() {
                self.checkpoint();
                self.doc.node_mut(first).absolute = abs;
                self.doc.reflow();
            }
        }
        if n.kind.is_frame() {
            let mut clip = n.clip;
            if ui.checkbox(&mut clip, "Clip content").changed() {
                self.checkpoint();
                for &id in &self.sel.clone() {
                    if self.doc.node(id).kind.is_frame() {
                        self.doc.node_mut(id).clip = clip;
                    }
                }
            }
        }
    }

    fn auto_layout_controls(&mut self, ui: &mut Ui, f: NodeId) {
        let mut l = self.doc.node(f).layout;
        let before = l;
        ui.horizontal(|ui| {
            for (m, wrap, icon, tip) in [
                (LayoutMode::Vertical, false, Icon::ArrowDown, "Vertical layout"),
                (LayoutMode::Horizontal, false, Icon::ArrowRight, "Horizontal layout"),
                (LayoutMode::Horizontal, true, Icon::Wrap, "Wrap"),
            ] {
                let on = l.mode == m && (m == LayoutMode::Vertical || l.wrap == wrap);
                if icon_button(ui, icon, on, 26.0, tip).clicked() {
                    l.mode = m;
                    l.wrap = wrap;
                }
            }
        });
        ui.horizontal(|ui| {
            align_grid(ui, &mut l);
            ui.vertical(|ui| {
                ui.add_enabled_ui(!l.space_between, |ui| {
                    num_field(ui, &mut l.gap, if l.mode == LayoutMode::Horizontal { "@gaph" } else { "@gapv" }, 0.0, (ui.available_width()).min(110.0));
                });
                if l.wrap && l.mode == LayoutMode::Horizontal {
                    num_field(ui, &mut l.cross_gap, "@gapv", 0.0, (ui.available_width()).min(110.0));
                }
                ui.checkbox(&mut l.space_between, "Auto gap");
                ui.checkbox(&mut l.strokes_included, "Include strokes").on_hover_text("Strokes take up space in the layout");
            });
        });
        let [t, r, b, lft] = &mut l.padding;
        let mut ph = if (*lft - *r).abs() < 1e-4 { *lft } else { f32::NAN };
        let mut pv = if (*t - *b).abs() < 1e-4 { *t } else { f32::NAN };
        let split_id = ui.id().with(("pad-split", f));
        let mut split = ui.data(|d| d.get_temp::<bool>(split_id)).unwrap_or(ph.is_nan() || pv.is_nan());
        ui.horizontal(|ui| {
            if split {
                for (v, label) in [(&mut *lft, "L"), (&mut *t, "T"), (&mut *r, "R"), (&mut *b, "B")] {
                    num_field(ui, v, label, 0.0, (ui.available_width() - 30.0) / 4.0 - 4.0);
                }
            } else {
                if num_field(ui, &mut ph, "@padh", 0.0, half(ui) - 16.0) {
                    (*lft, *r) = (ph, ph);
                }
                if num_field(ui, &mut pv, "@padv", 0.0, half(ui) - 16.0) {
                    (*t, *b) = (pv, pv);
                }
            }
            if icon_button(ui, Icon::Frame, split, 22.0, "Individual padding").clicked() {
                split = !split;
            }
        });
        ui.data_mut(|d| d.insert_temp(split_id, split));
        if l != before {
            self.begin_edit();
            let n = self.doc.node_mut(f);
            // Wrapping rows need a width to wrap at.
            if l.wrap && !before.wrap && n.sizing[0] == Sizing::Hug {
                n.sizing[0] = Sizing::Fixed;
            }
            n.layout = l;
            self.doc.reflow();
        }
    }

    fn constraints_section(&mut self, ui: &mut Ui, id: NodeId) {
        let n = self.doc.node(id).clone();
        let Some(p) = n.parent else { return };
        let pn = self.doc.node(p);
        if !pn.kind.is_frame() || (pn.layout.mode != LayoutMode::None && !n.absolute) {
            return;
        }
        ui.separator();
        heading(ui, "Constraints");
        let mut c = n.constraints;
        let names = |axis: usize, k: Constraint| match (axis, k) {
            (0, Constraint::Min) => "Left",
            (0, Constraint::Max) => "Right",
            (0, Constraint::Stretch) => "Left & right",
            (1, Constraint::Min) => "Top",
            (1, Constraint::Max) => "Bottom",
            (1, Constraint::Stretch) => "Top & bottom",
            (_, Constraint::Center) => "Center",
            (_, Constraint::Scale) => "Scale",
            _ => "",
        };
        let mut changed = false;
        ui.horizontal(|ui| {
            for (axis, cur) in c.iter_mut().enumerate() {
                egui::ComboBox::from_id_salt(("constraint", axis)).width(half(ui) - 12.0).selected_text(names(axis, *cur)).show_ui(ui, |ui| {
                    for k in [Constraint::Min, Constraint::Max, Constraint::Stretch, Constraint::Center, Constraint::Scale] {
                        changed |= ui.selectable_value(cur, k, names(axis, k)).changed();
                    }
                });
            }
        });
        if changed {
            self.checkpoint();
            self.doc.node_mut(id).constraints = c;
        }
    }

    fn appearance_section(&mut self, ui: &mut Ui, n: &crate::doc::Node) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Appearance").strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut blend = n.blend;
                egui::ComboBox::from_id_salt("blend").width(96.0).selected_text(format!("{blend:?}")).show_ui(ui, |ui| {
                    for b in crate::doc::BlendMode::ALL {
                        ui.selectable_value(&mut blend, b, format!("{b:?}"));
                    }
                });
                if blend != n.blend {
                    self.checkpoint();
                    for &id in &self.sel.clone() {
                        self.doc.node_mut(id).blend = blend;
                    }
                }
            });
        });
        let mut op = (n.opacity * 100.0).round();
        let mut r = n.radius;
        let has_radius = n.kind == Kind::Rectangle || n.kind.is_frame();
        let mut corners = n.corner_radii;
        ui.horizontal(|ui| {
            if num_field(ui, &mut op, "@opacity", 0.0, half(ui)) {
                self.begin_edit();
                for &id in &self.sel {
                    self.doc.node_mut(id).opacity = op.min(100.0) / 100.0;
                }
            }
            if has_radius {
                if corners.is_none() && num_field(ui, &mut r, "@radius", 0.0, half(ui) - 28.0) {
                    self.begin_edit();
                    for &id in &self.sel {
                        if self.doc.node(id).kind == Kind::Rectangle || self.doc.node(id).kind.is_frame() {
                            self.doc.node_mut(id).radius = r;
                        }
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon_button(ui, Icon::Radius, corners.is_some(), 22.0, "Independent corners").clicked() {
                        self.checkpoint();
                        corners = if corners.is_some() { None } else { Some([n.radius; 4]) };
                        for &id in &self.sel {
                            self.doc.node_mut(id).corner_radii = corners;
                        }
                    }
                });
            }
        });
        if let (true, Some(mut c)) = (has_radius, corners) {
            let mut changed = false;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let w = (ui.max_rect().width() - 12.0) / 4.0 - 1.0;
                for v in c.iter_mut() {
                    changed |= num_field(ui, v, "", 0.0, w);
                }
            });
            if changed {
                self.begin_edit();
                for &id in &self.sel {
                    self.doc.node_mut(id).corner_radii = Some(c);
                }
            }
        }
    }

    fn typography_section(&mut self, ui: &mut Ui, n: &crate::doc::Node) {
        use crate::doc::{Decoration, LineHeight, TextAlign, TextCase, TextSizing, VAlign};
        use crate::text::{families, weight_name, weights_for};
        let mut pick = None;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Typography").strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                pick = crate::paint_ui::style_menu(ui, ui.id().with("text-styles"), &self.doc.styles, |v| matches!(v, crate::doc::StyleValue::Text(_)));
            });
        });
        match pick {
            Some(crate::paint_ui::StylePick::Apply(sid)) => {
                self.checkpoint();
                for &id in &self.sel.clone() {
                    self.doc.node_mut(id).text_style = Some(sid);
                }
                self.doc.reflow();
                return;
            }
            Some(crate::paint_ui::StylePick::Create(name)) => {
                self.checkpoint();
                let sid = self.doc.add_style(name, crate::doc::StyleValue::Text(crate::doc::TextStyle::of(n)));
                for &id in &self.sel.clone() {
                    self.doc.node_mut(id).text_style = Some(sid);
                }
                self.doc.reflow();
                return;
            }
            None => {}
        }
        if let Some(style) = n.text_style.and_then(|sid| self.doc.style(sid)).cloned() {
            ui.horizontal(|ui| {
                crate::paint_ui::style_preview(ui, &style.value);
                ui.label(&style.name);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon_button(ui, Icon::Detach, false, 22.0, "Detach style").clicked() {
                        self.checkpoint();
                        for &id in &self.sel.clone() {
                            self.doc.node_mut(id).text_style = None;
                        }
                    }
                });
            });
        }
        let mut t = n.clone();
        // Font family picker with search.
        let fam_id = ui.id().with("font-search");
        let resp = ui.add_sized([ui.available_width(), 24.0], egui::Button::new(&t.font_family));
        egui::Popup::menu(&resp).id(fam_id.with("popup")).show(|ui| {
            ui.set_width(220.0);
            let mut q: String = ui.data(|d| d.get_temp(fam_id)).unwrap_or_default();
            ui.add(egui::TextEdit::singleline(&mut q).hint_text("Search fonts").desired_width(f32::INFINITY));
            ui.data_mut(|d| d.insert_temp(fam_id, q.clone()));
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                let ql = q.to_lowercase();
                for f in families().iter().filter(|f| f.to_lowercase().contains(&ql)).take(400) {
                    if ui.selectable_label(*f == t.font_family, f).clicked() {
                        t.font_family = f.clone();
                        let ws = weights_for(f);
                        if !ws.contains(&t.font_weight) {
                            t.font_weight = *ws.iter().min_by_key(|w| (**w as i32 - 400).abs()).unwrap_or(&400);
                        }
                        ui.close();
                    }
                }
            });
        });
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("weight").width(half(ui) - 12.0).selected_text(weight_name(t.font_weight)).show_ui(ui, |ui| {
                for w in weights_for(&t.font_family) {
                    ui.selectable_value(&mut t.font_weight, w, weight_name(w));
                }
            });
            num_field(ui, &mut t.font_size, "", 1.0, half(ui));
        });
        // Line height: "Auto", px, or "%" typed into the field.
        ui.horizontal(|ui| {
            let lh_id = ui.id().with(("lh", n.id));
            let shown = match t.line_height {
                LineHeight::Auto => "Auto".to_string(),
                LineHeight::Px(v) => num(v),
                LineHeight::Percent(p) => format!("{}%", num(p)),
            };
            let mut buf: String = ui.data(|d| d.get_temp(lh_id)).unwrap_or(shown);
            let r = ui.add_sized([half(ui), 22.0], egui::TextEdit::singleline(&mut buf).id(lh_id).hint_text("Line height"));
            if r.has_focus() {
                ui.data_mut(|d| d.insert_temp(lh_id, buf.clone()));
            } else {
                ui.data_mut(|d| d.remove::<String>(lh_id));
            }
            if r.lost_focus() {
                let v = buf.trim();
                t.line_height = if v.eq_ignore_ascii_case("auto") || v.is_empty() {
                    LineHeight::Auto
                } else if let Some(p) = v.strip_suffix('%').and_then(|p| p.trim().parse().ok()) {
                    LineHeight::Percent(p)
                } else if let Ok(px) = v.parse() {
                    LineHeight::Px(px)
                } else {
                    t.line_height
                };
            }
            r.on_hover_text("Line height: Auto, pixels, or percent");
            let mut ls = t.letter_spacing;
            if ui.add_sized([half(ui), 22.0], DragValue::new(&mut ls).speed(0.1).suffix("%").prefix("|A|  ").custom_formatter(|v, _| num(v as f32))).changed() {
                t.letter_spacing = ls;
            }
        });
        // Alignment and resizing.
        ui.horizontal(|ui| {
            for (a, icon) in [(TextAlign::Left, Icon::Align(Align::Left)), (TextAlign::Center, Icon::Align(Align::HCenter)), (TextAlign::Right, Icon::Align(Align::Right))] {
                if icon_button(ui, icon, t.text_align == a, 24.0, &format!("Align {a:?}")).clicked() {
                    t.text_align = a;
                }
            }
            ui.add_space(6.0);
            for (v, icon) in [(VAlign::Top, Icon::Align(Align::Top)), (VAlign::Middle, Icon::Align(Align::VCenter)), (VAlign::Bottom, Icon::Align(Align::Bottom))] {
                if icon_button(ui, icon, t.text_valign == v, 24.0, &format!("Align {v:?}")).clicked() {
                    t.text_valign = v;
                    if t.text_sizing != TextSizing::Fixed {
                        t.text_sizing = TextSizing::Fixed;
                    }
                }
            }
        });
        ui.horizontal(|ui| {
            let sizing = match t.text_sizing {
                TextSizing::AutoWidth => "Auto width",
                TextSizing::AutoHeight => "Auto height",
                TextSizing::Fixed => "Fixed size",
            };
            egui::ComboBox::from_id_salt("text-sizing").width(half(ui) - 12.0).selected_text(sizing).show_ui(ui, |ui| {
                ui.selectable_value(&mut t.text_sizing, TextSizing::AutoWidth, "Auto width");
                ui.selectable_value(&mut t.text_sizing, TextSizing::AutoHeight, "Auto height");
                ui.selectable_value(&mut t.text_sizing, TextSizing::Fixed, "Fixed size");
            });
            let case = match t.text_case {
                TextCase::Original => "Aa  As typed",
                TextCase::Upper => "AA  Upper",
                TextCase::Lower => "aa  Lower",
                TextCase::Title => "Aa  Title",
            };
            egui::ComboBox::from_id_salt("text-case").width(half(ui) - 12.0).selected_text(case).show_ui(ui, |ui| {
                ui.selectable_value(&mut t.text_case, TextCase::Original, "As typed");
                ui.selectable_value(&mut t.text_case, TextCase::Upper, "UPPER CASE");
                ui.selectable_value(&mut t.text_case, TextCase::Lower, "lower case");
                ui.selectable_value(&mut t.text_case, TextCase::Title, "Title Case");
            });
        });
        ui.horizontal(|ui| {
            if ui.selectable_label(false, RichText::new(" I ").italics()).on_hover_text("Italic (selected characters)").clicked() {
                let on = self
                    .text_selection(ui.ctx())
                    .and_then(|(a, _)| n.spans.iter().find(|sp| sp.start <= a && sp.end > a))
                    .and_then(|sp| sp.style.italic)
                    .unwrap_or(false);
                self.style_selection(ui.ctx(), crate::doc::SpanStyle { italic: Some(!on), ..Default::default() });
            }
            for (d, label) in [(Decoration::Underline, RichText::new(" U ").underline()), (Decoration::Strikethrough, RichText::new(" S ").strikethrough())] {
                let on = t.decoration == d;
                if ui.selectable_label(on, label).on_hover_text(format!("{d:?}")).clicked() {
                    t.decoration = if on { Decoration::None } else { d };
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                num_field(ui, &mut t.paragraph_spacing, "¶", 0.0, 64.0);
            });
        });
        ui.horizontal(|ui| {
            for (kind, label, tip) in [(crate::doc::ListKind::Bullet, " • ", "Bulleted list (⇧⌘8)"), (crate::doc::ListKind::Number, " 1. ", "Numbered list (⇧⌘7)")] {
                let on = !n.lists.is_empty() && n.lists.iter().all(|l| l.kind == kind);
                if ui.selectable_label(on, label).on_hover_text(tip).clicked() {
                    self.toggle_list(ui.ctx(), kind);
                    return;
                }
            }
            ui.add_space(6.0);
            ui.checkbox(&mut t.truncate, "Truncate");
            if t.truncate {
                ui.add(egui::DragValue::new(&mut t.max_lines).range(0..=999).custom_formatter(|v, _| if v == 0.0 { "Auto".into() } else { format!("{v} lines") }))
                    .on_hover_text("Max lines (Auto fits the box)");
            }
        });
        let changed = t.font_family != n.font_family
            || t.font_weight != n.font_weight
            || t.font_size != n.font_size
            || t.line_height != n.line_height
            || t.letter_spacing != n.letter_spacing
            || t.text_align != n.text_align
            || t.text_valign != n.text_valign
            || t.text_sizing != n.text_sizing
            || t.decoration != n.decoration
            || t.text_case != n.text_case
            || t.truncate != n.truncate
            || t.paragraph_spacing != n.paragraph_spacing
            || t.max_lines != n.max_lines;
        // With characters selected in the editor, changes style just that range (rich text).
        if changed && self.text_selection(ui.ctx()).is_some() {
            let patch = crate::doc::SpanStyle {
                font_family: (t.font_family != n.font_family).then(|| t.font_family.clone()),
                font_weight: (t.font_weight != n.font_weight).then_some(t.font_weight),
                font_size: (t.font_size != n.font_size).then_some(t.font_size),
                decoration: (t.decoration != n.decoration).then_some(t.decoration),
                ..Default::default()
            };
            if !patch.is_empty() {
                self.style_selection(ui.ctx(), patch);
                return;
            }
        }
        if changed {
            self.begin_edit();
            for &id in &self.sel.clone() {
                let d = self.doc.node_mut(id);
                // Changing a styled value by hand detaches the style, like Figma.
                d.text_style = None;
                d.font_family = t.font_family.clone();
                d.font_weight = t.font_weight;
                d.font_size = t.font_size;
                d.line_height = t.line_height;
                d.letter_spacing = t.letter_spacing;
                d.text_align = t.text_align;
                d.text_valign = t.text_valign;
                d.text_sizing = t.text_sizing;
                d.decoration = t.decoration;
                d.text_case = t.text_case;
                d.truncate = t.truncate;
                d.paragraph_spacing = t.paragraph_spacing.max(0.0);
                d.max_lines = t.max_lines;
            }
        }
    }

    fn paints_section(&mut self, ui: &mut Ui, stroke: bool) {
        use crate::doc::StyleValue;
        use crate::paint_ui::{StylePick, style_menu, style_preview};
        let first = self.sel[0];
        let n = self.doc.node(first).clone();
        let mut paints = if stroke { n.strokes.clone() } else { n.fills.clone() };
        let mut changed = false;
        let link = if stroke { n.stroke_style } else { n.fill_style };
        let mut add = false;
        let mut pick = None;
        ui.horizontal(|ui| {
            ui.label(RichText::new(if stroke { "Stroke" } else { "Fill" }).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                add = icon_button(ui, Icon::Plus, false, 22.0, "").clicked();
                pick = style_menu(ui, ui.id().with(("paint-styles", stroke)), &self.doc.styles, |v| matches!(v, StyleValue::Paint(_)));
            });
        });
        let set_link = |app: &mut App, v: Option<crate::doc::StyleId>| {
            for &id in &app.sel.clone() {
                let t = app.doc.node_mut(id);
                if stroke { t.stroke_style = v } else { t.fill_style = v }
            }
            app.doc.reflow();
        };
        match pick {
            Some(StylePick::Apply(sid)) => {
                self.checkpoint();
                set_link(self, Some(sid));
            }
            Some(StylePick::Create(name)) => {
                self.checkpoint();
                let sid = self.doc.add_style(name, StyleValue::Paint(paints.clone()));
                set_link(self, Some(sid));
            }
            None => {}
        }
        // A linked style shows as one row; editing it edits the style for everyone.
        if let Some(style) = link.and_then(|sid| self.doc.style(sid)).cloned() {
            ui.horizontal(|ui| {
                style_preview(ui, &style.value);
                let open = ui.add(egui::Button::new(&style.name).frame(false));
                egui::Popup::menu(&open).id(ui.id().with(("edit-style", style.id))).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                    ui.set_width(240.0);
                    ui.label(RichText::new(format!("Edit {}", style.name)).strong());
                    if let StyleValue::Paint(mut ps) = style.value.clone() {
                        let mut edited = false;
                        for (i, p) in ps.iter_mut().enumerate().rev() {
                            if !matches!(paint_row(ui, ui.id().with(("style-paint", style.id, i)), p), PaintEvent::None) {
                                edited = true;
                            }
                        }
                        if edited {
                            self.begin_edit();
                            if let Some(st) = self.doc.styles.iter_mut().find(|s| s.id == style.id) {
                                st.value = StyleValue::Paint(ps);
                            }
                            self.doc.reflow();
                        }
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon_button(ui, Icon::Detach, false, 22.0, "Detach style").clicked() {
                        self.checkpoint();
                        set_link(self, None);
                    }
                });
            });
            return;
        }
        if add {
            let c = match (stroke, n.kind) {
                (true, _) | (false, Kind::Text) => Rgba::BLACK,
                (false, Kind::Frame) => Rgba::WHITE,
                _ => SHAPE_GRAY,
            };
            paints.push(Paint::solid(c));
            changed = true;
        }
        let mut remove = None;
        // Topmost paint first, as in Figma.
        for i in (0..paints.len()).rev() {
            let id = ui.id().with((stroke, i, first));
            match paint_row(ui, id, &mut paints[i]) {
                PaintEvent::None => {}
                PaintEvent::Changed => changed = true,
                PaintEvent::Remove => remove = Some(i),
                PaintEvent::Image(bytes) => {
                    paints[i].image = self.add_image(bytes);
                    changed = true;
                }
            }
        }
        if let Some(i) = remove {
            paints.remove(i);
            changed = true;
        }
        let (mut width, mut align) = (n.stroke_width, n.stroke_align);
        let mut stroke_changed = false;
        if stroke && !paints.is_empty() {
            ui.horizontal(|ui| {
                stroke_changed |= ui
                    .add(DragValue::new(&mut width).range(0.0..=1000.0).speed(0.1).prefix("Weight  ").custom_formatter(|v, _| num(v as f32)))
                    .changed();
                if n.kind != Kind::Vector {
                    egui::ComboBox::from_id_salt("stroke-align").width(70.0).selected_text(format!("{align:?}")).show_ui(ui, |ui| {
                        for a in [StrokeAlign::Inside, StrokeAlign::Center, StrokeAlign::Outside] {
                            stroke_changed |= ui.selectable_value(&mut align, a, format!("{a:?}")).changed();
                        }
                    });
                }
                if n.kind == Kind::Vector && n.paths.iter().any(|p| !p.closed) {
                    use crate::doc::Marker;
                    let mut ends = (n.start_marker, n.end_marker);
                    let label = |m: Marker| match m {
                        Marker::None => "—",
                        Marker::Arrow => "→",
                        Marker::Triangle => "▶",
                        Marker::Circle => "●",
                    };
                    for (i, m) in [&mut ends.0, &mut ends.1].into_iter().enumerate() {
                        egui::ComboBox::from_id_salt(("marker", i)).width(28.0).selected_text(label(*m)).show_ui(ui, |ui| {
                            for k in [Marker::None, Marker::Arrow, Marker::Triangle, Marker::Circle] {
                                ui.selectable_value(m, k, format!("{} {k:?}", label(k)));
                            }
                        });
                    }
                    if ends != (n.start_marker, n.end_marker) {
                        self.begin_edit();
                        for &id in &self.sel.clone() {
                            let t = self.doc.node_mut(id);
                            (t.start_marker, t.end_marker) = ends;
                        }
                    }
                }
                // Dashes, caps, and joins live in a settings popover, like Figma.
                let more = icon_button(ui, Icon::Menu, false, 22.0, "Advanced stroke");
                egui::Popup::menu(&more).id(ui.id().with("stroke-more")).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                    ui.set_width(200.0);
                    let mut style = (n.dash.clone(), n.cap, n.join);
                    let dash_id = ui.id().with(("dash", n.id));
                    let shown = style.0.iter().map(|d| num(*d)).collect::<Vec<_>>().join(", ");
                    let mut buf: String = ui.data(|d| d.get_temp(dash_id)).unwrap_or(shown);
                    ui.label("Dashes");
                    let r = ui.add(egui::TextEdit::singleline(&mut buf).id(dash_id).hint_text("e.g. 4, 2"));
                    if r.has_focus() {
                        ui.data_mut(|d| d.insert_temp(dash_id, buf.clone()));
                    } else {
                        ui.data_mut(|d| d.remove::<String>(dash_id));
                    }
                    if r.lost_focus() {
                        style.0 = buf.split([',', ' ']).filter_map(|t| t.trim().parse::<f32>().ok()).filter(|v| *v >= 0.0).collect();
                    }
                    ui.horizontal(|ui| {
                        ui.label("Caps");
                        for c in [crate::doc::Cap::None, crate::doc::Cap::Round, crate::doc::Cap::Square] {
                            ui.selectable_value(&mut style.1, c, format!("{c:?}"));
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("Join");
                        for j in [crate::doc::Join::Miter, crate::doc::Join::Round, crate::doc::Join::Bevel] {
                            ui.selectable_value(&mut style.2, j, format!("{j:?}"));
                        }
                    });
                    if style != (n.dash.clone(), n.cap, n.join) {
                        self.begin_edit();
                        for &id in &self.sel.clone() {
                            let t = self.doc.node_mut(id);
                            (t.dash, t.cap, t.join) = style.clone();
                        }
                    }
                    if n.kind == Kind::Rectangle || n.kind.is_frame() {
                        ui.separator();
                        ui.label("Sides");
                        let w = n.stroke_width;
                        let mut sides = n.stroke_sides;
                        ui.horizontal_wrapped(|ui| {
                            for (label, v) in [("All", None), ("Top", Some([w, 0., 0., 0.])), ("Right", Some([0., w, 0., 0.])), ("Bottom", Some([0., 0., w, 0.])), ("Left", Some([0., 0., 0., w])), ("Custom", Some(sides.unwrap_or([w; 4])))] {
                                let on = match (label, sides) {
                                    ("Custom", Some(s)) => !s.iter().filter(|x| **x > 0.0).count().eq(&1),
                                    _ => sides == v,
                                };
                                if ui.selectable_label(on, label).clicked() {
                                    sides = v;
                                }
                            }
                        });
                        if let Some(s) = &mut sides {
                            ui.horizontal(|ui| {
                                for (x, l) in s.iter_mut().zip(["T", "R", "B", "L"]) {
                                    ui.add(DragValue::new(x).range(0.0..=1000.0).speed(0.1).prefix(l).custom_formatter(|v, _| num(v as f32)));
                                }
                            });
                        }
                        if sides != n.stroke_sides {
                            self.begin_edit();
                            for &id in &self.sel.clone() {
                                let t = self.doc.node_mut(id);
                                if t.kind == Kind::Rectangle || t.kind.is_frame() {
                                    t.stroke_sides = sides;
                                }
                            }
                        }
                    }
                });
            });
        }
        // Recoloring selected characters while editing text.
        if changed && !stroke && n.kind == Kind::Text && self.text_selection(ui.ctx()).is_some()
            && let Some(c) = paints.iter().rev().find(|p| p.visible && p.kind == crate::doc::PaintKind::Solid).map(|p| p.color) {
                self.style_selection(ui.ctx(), crate::doc::SpanStyle { color: Some(c), ..Default::default() });
                return;
            }
        if changed || stroke_changed {
            self.begin_edit();
            for &id in &self.sel.clone() {
                let t = self.doc.node_mut(id);
                if t.kind == Kind::Group {
                    continue;
                }
                if stroke {
                    t.strokes = paints.clone();
                    if stroke_changed {
                        if width != t.stroke_width && t.stroke_sides.is_some() {
                            t.stroke_sides = None;
                        }
                        t.stroke_width = width;
                        if t.kind != Kind::Vector {
                            t.stroke_align = align;
                        }
                    }
                } else {
                    t.fills = paints.clone();
                }
            }
        }
    }

    fn layout_guides(&mut self, ui: &mut Ui, id: NodeId) {
        use crate::doc::{GridAlign, GridKind, LayoutGrid};
        let mut grids = self.doc.node(id).grids.clone();
        let before = grids.clone();
        if section_title(ui, "Layout guide") {
            grids.push(LayoutGrid::default());
        }
        let mut remove = None;
        for (i, g) in grids.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                let label = match g.kind {
                    GridKind::Grid => format!("Grid {}px", num(g.size)),
                    GridKind::Columns => format!("{} columns", g.count),
                    GridKind::Rows => format!("{} rows", g.count),
                };
                let open = ui.add(egui::Button::new(label).frame(false));
                egui::Popup::menu(&open).id(ui.id().with(("grid", id, i))).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                    ui.set_width(220.0);
                    ui.horizontal(|ui| {
                        for k in [GridKind::Grid, GridKind::Columns, GridKind::Rows] {
                            ui.selectable_value(&mut g.kind, k, format!("{k:?}"));
                        }
                    });
                    egui::Grid::new(ui.id().with(("grid-fields", id, i))).num_columns(2).show(ui, |ui| {
                        if g.kind == GridKind::Grid {
                            ui.label("Size");
                            ui.add(DragValue::new(&mut g.size).range(1.0..=1000.0));
                            ui.end_row();
                        } else {
                            ui.label("Count");
                            ui.add(DragValue::new(&mut g.count).range(1..=100));
                            ui.end_row();
                            ui.label("Type");
                            dropdown(ui, format!("{:?}", g.align), |ui| {
                                for a in [GridAlign::Stretch, GridAlign::Min, GridAlign::Center, GridAlign::Max] {
                                    ui.selectable_value(&mut g.align, a, format!("{a:?}"));
                                }
                            });
                            ui.end_row();
                            if g.align != GridAlign::Stretch {
                                ui.label("Width");
                                ui.add(DragValue::new(&mut g.size).range(1.0..=10000.0));
                                ui.end_row();
                            }
                            ui.label("Gutter");
                            ui.add(DragValue::new(&mut g.gutter).range(0.0..=1000.0));
                            ui.end_row();
                            ui.label(if g.align == GridAlign::Stretch { "Margin" } else { "Offset" });
                            ui.add(DragValue::new(&mut g.margin).range(-1000.0..=1000.0));
                            ui.end_row();
                        }
                    });
                    ui.horizontal(|ui| {
                        hex_field(ui, ui.id().with(("grid-color", id, i)), &mut g.color, 56.0);
                        crate::paint_ui::alpha_field(ui, &mut g.color);
                    });
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon_button(ui, Icon::Minus, false, 22.0, "Remove").clicked() {
                        remove = Some(i);
                    }
                    if icon_button(ui, Icon::Eye(g.visible), false, 22.0, "Show/hide").clicked() {
                        g.visible = !g.visible;
                    }
                });
            });
        }
        if let Some(i) = remove {
            grids.remove(i);
        }
        if grids != before {
            self.begin_edit();
            self.doc.node_mut(id).grids = grids;
        }
    }

    fn effects_section(&mut self, ui: &mut Ui) {
        use crate::doc::StyleValue;
        use crate::paint_ui::{StylePick, style_menu, style_preview};
        let first = self.sel[0];
        let mut shadows = self.doc.node(first).shadows.clone();
        let mut changed = false;
        let link = self.doc.node(first).effect_style;
        let mut add = false;
        let mut pick = None;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Effects").strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                add = icon_button(ui, Icon::Plus, false, 22.0, "").clicked();
                pick = style_menu(ui, ui.id().with("effect-styles"), &self.doc.styles, |v| matches!(v, StyleValue::Effect(_)));
            });
        });
        let set_link = |app: &mut App, v: Option<crate::doc::StyleId>| {
            for &id in &app.sel.clone() {
                app.doc.node_mut(id).effect_style = v;
            }
            app.doc.reflow();
        };
        match pick {
            Some(StylePick::Apply(sid)) => {
                self.checkpoint();
                set_link(self, Some(sid));
            }
            Some(StylePick::Create(name)) => {
                self.checkpoint();
                let sid = self.doc.add_style(name, StyleValue::Effect(shadows.clone()));
                set_link(self, Some(sid));
            }
            None => {}
        }
        if let Some(style) = link.and_then(|sid| self.doc.style(sid)).cloned() {
            ui.horizontal(|ui| {
                style_preview(ui, &style.value);
                ui.label(&style.name);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon_button(ui, Icon::Detach, false, 22.0, "Detach style").clicked() {
                        self.checkpoint();
                        set_link(self, None);
                    }
                });
            });
            return;
        }
        if add {
            shadows.push(Shadow::default());
            changed = true;
        }
        let mut remove = None;
        for i in (0..shadows.len()).rev() {
            let (c, r) = shadow_row(ui, ui.id().with(("shadow", i, first)), &mut shadows[i]);
            changed |= c;
            if r {
                remove = Some(i);
            }
        }
        if let Some(i) = remove {
            shadows.remove(i);
            changed = true;
        }
        if changed {
            self.begin_edit();
            for &id in &self.sel.clone() {
                self.doc.node_mut(id).shadows = shadows.clone();
            }
        }
    }

    fn export_props(&mut self, ui: &mut Ui) {
        use crate::doc::{ExportFormat, ExportSetting};
        let roots = self.sel_roots();
        let single = (roots.len() == 1).then(|| roots[0]);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Export").strong());
            if let Some(id) = single {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon_button(ui, Icon::Plus, false, 22.0, "Add export setting").clicked() {
                        self.checkpoint();
                        let n = self.doc.node_mut(id);
                        // Figma adds the next common scale.
                        let next = match n.exports.last().map(|e| e.scale) {
                            None => 1.0,
                            Some(s) if s < 2.0 => 2.0,
                            Some(s) => s + 1.0,
                        };
                        n.exports.push(ExportSetting { scale: next, ..Default::default() });
                    }
                });
            }
        });
        let fmt_combo = |ui: &mut Ui, id: egui::Id, f: &mut ExportFormat| {
            let mut changed = false;
            egui::ComboBox::from_id_salt(id).width(56.0).selected_text(f.ext().to_uppercase()).show_ui(ui, |ui| {
                for k in [ExportFormat::Png, ExportFormat::Jpg, ExportFormat::Svg, ExportFormat::Pdf] {
                    changed |= ui.selectable_value(f, k, k.ext().to_uppercase()).changed();
                }
            });
            changed
        };
        let scale_combo = |ui: &mut Ui, id: egui::Id, s: &mut f32| {
            let mut changed = false;
            egui::ComboBox::from_id_salt(id).width(48.0).selected_text(format!("{}x", num(*s))).show_ui(ui, |ui| {
                for v in [0.5, 0.75, 1.0, 1.5, 2.0, 3.0, 4.0] {
                    changed |= ui.selectable_value(s, v, format!("{}x", num(v))).changed();
                }
            });
            changed
        };
        let presets = single.map(|id| self.doc.node(id).exports.clone()).unwrap_or_default();
        if presets.is_empty() {
            ui.horizontal(|ui| {
                scale_combo(ui, ui.id().with("export-scale"), &mut self.export_scale);
                let mut f = if self.export_svg { ExportFormat::Svg } else { ExportFormat::Png };
                if fmt_combo(ui, ui.id().with("export-format"), &mut f) {
                    self.export_svg = f == ExportFormat::Svg;
                }
            });
        } else if let Some(id) = single {
            let mut list = presets.clone();
            let mut remove = None;
            for (i, p) in list.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    scale_combo(ui, ui.id().with(("ex-scale", id, i)), &mut p.scale);
                    ui.add(egui::TextEdit::singleline(&mut p.suffix).hint_text("Suffix").desired_width(60.0));
                    fmt_combo(ui, ui.id().with(("ex-fmt", id, i)), &mut p.format);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icon_button(ui, Icon::Minus, false, 20.0, "Remove").clicked() {
                            remove = Some(i);
                        }
                    });
                });
            }
            if let Some(i) = remove {
                list.remove(i);
            }
            if list != presets {
                self.begin_edit();
                self.doc.node_mut(id).exports = list;
            }
        }
        let label = match roots.as_slice() {
            [] => format!("Export {}", self.doc.pages[self.page].name),
            [one] => format!("Export {}", self.doc.node(*one).name),
            many => format!("Export {} layers", many.len()),
        };
        if ui.add_sized([ui.available_width(), 26.0], egui::Button::new(label)).clicked() {
            self.export(&ui.ctx().clone());
        }
    }

    // ---------- bottom toolbar ----------

    pub fn toolbar(&mut self, ctx: &egui::Context, canvas: Rect) {
        egui::Area::new(egui::Id::new("toolbar"))
            .fixed_pos(pos2(canvas.center().x, canvas.max.y - 16.0))
            .pivot(egui::Align2::CENTER_BOTTOM)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(theme::card(ui.ctx()))
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(6)
                    .stroke(Stroke::new(1.0, theme::border(ui.ctx())))
                    .shadow(egui::Shadow { offset: [0, 2], blur: 10, spread: 0, color: Color32::from_black_alpha(30) })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            for (g, tools) in TOOL_GROUPS.iter().enumerate() {
                                // The button shows the active tool if it's in this group, else the group's last one.
                                let current = tools.iter().find(|(t, _, _)| *t == self.tool).unwrap_or_else(|| {
                                    tools.iter().find(|(t, _, _)| *t == self.tool_groups[g]).unwrap_or(&tools[0])
                                });
                                let active = tools.iter().any(|(t, _, _)| *t == self.tool);
                                if icon_button(ui, current.1, active, 32.0, current.2).clicked() {
                                    self.set_tool(current.0);
                                }
                                if tools.len() > 1 {
                                    let more = icon_button(ui, Icon::Chevron(true), false, 14.0, "");
                                    egui::Popup::menu(&more).id(ui.id().with(("tools", g))).show(|ui| {
                                        for &(t, icon, label) in tools.iter() {
                                            ui.horizontal(|ui| {
                                                let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                                                paint_icon(ui.painter(), r, icon, theme::ink(ui.ctx()));
                                                let (name, key) = label.split_once("  ").unwrap_or((label, ""));
                                                if ui.add(egui::Button::new(name).shortcut_text(key).frame(false).min_size(vec2(140.0, 0.0))).clicked() {
                                                    self.set_tool(t);
                                                    ui.close();
                                                }
                                            });
                                        }
                                    });
                                }
                                ui.add_space(4.0);
                            }
                        });
                    });
            });
    }
}

