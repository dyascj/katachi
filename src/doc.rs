//! Document model: pages hold a tree of nodes. Coordinates are absolute world units.

use egui::{Color32, Pos2, Rect, Vec2, pos2, vec2};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub type NodeId = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Kind {
    Frame,
    /// A main component: a frame other layers can instantiate.
    Component,
    /// A live copy of a component.
    Instance,
    /// Variants of one component, combined.
    ComponentSet,
    /// A live boolean of its children.
    Boolean,
    /// Regular polygon inscribed in the box.
    Polygon,
    Star,
    /// A top-level organizer for frames.
    Section,
    /// An export region; draws nothing.
    Slice,
    Group,
    Rectangle,
    Ellipse,
    Vector,
    Text,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Frame => "Frame",
            Kind::Component => "Component",
            Kind::Instance => "Instance",
            Kind::ComponentSet => "Component set",
            Kind::Boolean => "Boolean",
            Kind::Polygon => "Polygon",
            Kind::Star => "Star",
            Kind::Section => "Section",
            Kind::Slice => "Slice",
            Kind::Group => "Group",
            Kind::Rectangle => "Rectangle",
            Kind::Ellipse => "Ellipse",
            Kind::Vector => "Vector",
            Kind::Text => "Text",
        }
    }
    pub fn is_container(self) -> bool {
        self.is_frame() || matches!(self, Kind::Group | Kind::Boolean)
    }
    /// Frames, components, instances, and sections all behave as frames.
    pub fn is_frame(self) -> bool {
        matches!(self, Kind::Frame | Kind::Component | Kind::Instance | Kind::Section | Kind::ComponentSet)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BoolOp {
    #[default]
    Union,
    Subtract,
    Intersect,
    Exclude,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LayoutMode {
    #[default]
    None,
    Horizontal,
    Vertical,
    /// Figma's grid auto layout: children flow into rows and columns of cells.
    Grid,
}

/// How a layer sizes itself along one axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Sizing {
    #[default]
    Fixed,
    /// Shrink-wrap content (auto-layout frames).
    Hug,
    /// Stretch to the auto-layout parent.
    Fill,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum AxisAlign {
    #[default]
    Min,
    Center,
    Max,
    /// Horizontal layouts, cross axis: line up the first text baselines (Figma's "Text baseline").
    Baseline,
}

/// Figma constraint for one axis: Left/Top, Right/Bottom, Left & right, Center, Scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Constraint {
    #[default]
    Min,
    Max,
    Stretch,
    Center,
    Scale,
}

#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoLayout {
    pub mode: LayoutMode,
    pub gap: f32,
    /// "Auto" spacing: children spread to fill the main axis.
    pub space_between: bool,
    /// Top, right, bottom, left.
    pub padding: [f32; 4],
    pub main: AxisAlign,
    pub cross: AxisAlign,
    /// Horizontal layouts: flow children onto new rows when they run out of width.
    pub wrap: bool,
    /// Gap between wrapped rows.
    pub cross_gap: f32,
    /// Strokes take up space: the frame's inner stroke pads it, children's outer strokes add to their size.
    pub strokes_included: bool,
    /// Canvas stacking "First on top": earlier children paint over later ones (Figma's itemReverseZIndex).
    pub reverse_z: bool,
    /// Grid: column and row counts (0 = 1 column; rows grow to fit the children).
    pub grid: [u32; 2],
    /// Grid: fixed column width and row height; None shares the space out (Fill), or hugs the
    /// largest cell when the frame hugs that axis. Column gap is `gap`, row gap `cross_gap`.
    pub tracks: [Option<f32>; 2],
}

/// How a text box sizes itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TextSizing {
    #[default]
    AutoWidth,
    AutoHeight,
    Fixed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum VAlign {
    #[default]
    Top,
    Middle,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
pub enum LineHeight {
    #[default]
    Auto,
    Px(f32),
    Percent(f32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Decoration {
    #[default]
    None,
    Underline,
    Strikethrough,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TextCase {
    #[default]
    Original,
    Upper,
    Lower,
    Title,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Trigger {
    #[default]
    OnClick,
    /// Figma's "While hovering": the action holds while the pointer stays on the layer.
    OnHover,
    WhilePressed,
    /// Fires this many milliseconds after the frame appears.
    AfterDelay(u32),
    MouseEnter,
    MouseLeave,
    OnDrag,
    /// Key/Gamepad: a key with modifiers (only Cmd/Ctrl, Shift and Alt are kept).
    Key(egui::Key, egui::Modifiers),
}

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub enum Action {
    #[default]
    None,
    Navigate(NodeId),
    Back,
    OpenOverlay(NodeId),
    CloseOverlay,
    OpenUrl(String),
    /// Interactive components: switch the instance the trigger is in to this variant.
    ChangeTo(NodeId),
    /// Scroll the layer's scrolling frame until the layer reaches its top-left.
    ScrollTo(NodeId),
    /// Replace the topmost overlay with this frame.
    SwapOverlay(NodeId),
    /// Give a variable a new value for the rest of the presentation.
    SetVariable(VarId, VarValue),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TransitionKind {
    #[default]
    Instant,
    Dissolve,
    SmartAnimate,
    MoveIn,
    Push,
    SlideIn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Direction {
    #[default]
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transition {
    pub kind: TransitionKind,
    pub direction: Direction,
    /// Seconds (springs settle in their own time).
    pub duration: f32,
    #[serde(default)]
    pub easing: Easing,
}

impl Default for Transition {
    fn default() -> Self {
        Self { kind: TransitionKind::Instant, direction: Direction::Left, duration: 0.3, easing: Easing::EaseOut }
    }
}

/// Figma's animation curves.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
pub enum Easing {
    Linear,
    EaseIn,
    #[default]
    EaseOut,
    EaseInOut,
    EaseInBack,
    EaseOutBack,
    EaseInOutBack,
    /// Cubic bezier control points: x1, y1, x2, y2.
    Custom([f32; 4]),
    Gentle,
    Quick,
    Bouncy,
    Slow,
}

/// Where an overlay opens, relative to the frame (or the trigger, for Manual).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum OverlayPos {
    #[default]
    Center,
    TopLeft,
    TopCenter,
    TopRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
    Manual,
}

/// Open and Swap overlay settings.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OverlaySettings {
    pub position: OverlayPos,
    /// Manual position: from the trigger layer's top-left.
    pub offset: Vec2,
    pub close_outside: bool,
    /// Color laid behind the overlay; None leaves the frame as is.
    pub background: Option<Rgba>,
}

impl Default for OverlaySettings {
    /// Centered, closing on an outside click, over a dimmed frame (how overlays always opened before).
    fn default() -> Self {
        Self { position: OverlayPos::Center, offset: Vec2::ZERO, close_outside: true, background: Some(Rgba([0, 0, 0, 100])) }
    }
}

/// Prototype overflow: which way a frame scrolls when its content runs past its edges.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Overflow {
    #[default]
    None,
    Vertical,
    Horizontal,
    Both,
}

/// How a layer moves when its parent frame scrolls.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ScrollPos {
    #[default]
    Scroll,
    Fixed,
    /// Scrolls until its top edge reaches the frame's top, then stays.
    Sticky,
}

/// Devices a prototype can be presented in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Device {
    #[default]
    None,
    IPhone16,
    IPhone16ProMax,
    Pixel9,
    IPad,
    MacBook,
    Desktop,
}

/// A page's prototype settings.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProtoSettings {
    pub device: Device,
    pub background: Rgba,
}

impl Default for ProtoSettings {
    fn default() -> Self {
        Self { device: Device::None, background: Rgba::rgb(0x1E, 0x1E, 0x1E) }
    }
}

/// A prototype interaction on a layer.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Reaction {
    pub trigger: Trigger,
    pub action: Action,
    #[serde(default)]
    pub transition: Transition,
    /// Open and Swap overlay only.
    #[serde(default, skip_serializing_if = "is_default")]
    pub overlay: OverlaySettings,
}

/// A component property definition (on a component or component set).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PropValue {
    Bool(bool),
    Text(String),
    /// Instance swap: the component to show.
    Swap(NodeId),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CompProp {
    pub name: String,
    pub default: PropValue,
}

/// Which property of a layer a component property drives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PropTarget {
    Visible,
    Text,
    Swap,
}

/// Property bindings were one optional pair in older files; now a layer can have several.
fn one_or_many_binds<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<(String, PropTarget)>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Binds {
        One(Option<(String, PropTarget)>),
        Many(Vec<(String, PropTarget)>),
    }
    Ok(match Binds::deserialize(d)? {
        Binds::One(o) => o.into_iter().collect(),
        Binds::Many(v) => v,
    })
}

/// Parse "Size=Large, State=Hover" into ordered pairs.
pub fn variant_pairs(name: &str) -> Vec<(String, String)> {
    name.split(',')
        .filter_map(|part| part.split_once('=').map(|(k, v)| (k.trim().to_string(), v.trim().to_string())))
        .collect()
}

pub type VarId = u64;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum VarValue {
    Color(Rgba),
    Number(f32),
    Text(String),
    Bool(bool),
    /// Points at another variable.
    Alias(VarId),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Variable {
    pub id: VarId,
    pub name: String,
    pub collection: u64,
    /// One value per mode of the collection.
    pub values: Vec<VarValue>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Collection {
    pub id: u64,
    pub name: String,
    pub modes: Vec<String>,
}

/// Layer properties a variable can drive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Bindable {
    Fill,
    Stroke,
    Opacity,
    Radius,
    Width,
    Height,
    Gap,
    Padding,
    StrokeWidth,
    FontSize,
    Text,
    Visible,
}

impl Bindable {
    /// How the property reads in the UI.
    pub fn label(self) -> &'static str {
        match self {
            Bindable::Fill => "Fill",
            Bindable::Stroke => "Stroke",
            Bindable::Opacity => "Opacity",
            Bindable::Radius => "Corner radius",
            Bindable::Width => "Width",
            Bindable::Height => "Height",
            Bindable::Gap => "Gap",
            Bindable::Padding => "Padding",
            Bindable::StrokeWidth => "Stroke width",
            Bindable::FontSize => "Font size",
            Bindable::Text => "Text",
            Bindable::Visible => "Visibility",
        }
    }

    pub const ALL: [Bindable; 12] = [
        Bindable::Fill,
        Bindable::Stroke,
        Bindable::Opacity,
        Bindable::Radius,
        Bindable::Width,
        Bindable::Height,
        Bindable::Gap,
        Bindable::Padding,
        Bindable::StrokeWidth,
        Bindable::FontSize,
        Bindable::Text,
        Bindable::Visible,
    ];
    /// Whether a value of this kind fits the property.
    pub fn accepts(self, v: &VarValue) -> bool {
        match self {
            Bindable::Fill | Bindable::Stroke => matches!(v, VarValue::Color(_)),
            Bindable::Text => matches!(v, VarValue::Text(_)),
            Bindable::Visible => matches!(v, VarValue::Bool(_)),
            _ => matches!(v, VarValue::Number(_)),
        }
    }
}

/// A comment thread pinned on the canvas, optionally anchored to a frame so it moves with it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Comment {
    pub id: u64,
    pub page: usize,
    /// World position, or offset from the anchor's top-left when anchored.
    pub at: Pos2,
    pub anchor: Option<NodeId>,
    /// (author, text, unix seconds); the first entry opens the thread.
    pub messages: Vec<(String, String, u64)>,
    pub resolved: bool,
}

/// Style overrides for a range of characters in a text layer (unset = the layer's own value).
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SpanStyle {
    pub font_family: Option<String>,
    pub font_weight: Option<u16>,
    pub font_size: Option<f32>,
    pub color: Option<Rgba>,
    pub decoration: Option<Decoration>,
    pub italic: Option<bool>,
}

impl SpanStyle {
    /// Overlay `patch` on top of self.
    pub fn merge(&self, patch: &SpanStyle) -> SpanStyle {
        SpanStyle {
            font_family: patch.font_family.clone().or(self.font_family.clone()),
            font_weight: patch.font_weight.or(self.font_weight),
            font_size: patch.font_size.or(self.font_size),
            color: patch.color.or(self.color),
            decoration: patch.decoration.or(self.decoration),
            italic: patch.italic.or(self.italic),
        }
    }
    pub fn is_empty(&self) -> bool {
        *self == SpanStyle::default()
    }
}

/// A styled character range [start, end) in a text layer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextSpan {
    pub start: usize,
    pub end: usize,
    pub style: SpanStyle,
}

/// Apply a style patch to chars [start, end), splitting and merging spans as needed.
pub fn style_range(spans: &[TextSpan], len: usize, start: usize, end: usize, patch: &SpanStyle) -> Vec<TextSpan> {
    let (start, end) = (start.min(len), end.min(len));
    if start >= end {
        return spans.to_vec();
    }
    // Break the text into pieces at every boundary, then restyle the pieces inside the range.
    let mut cuts: Vec<usize> = vec![0, len, start, end];
    for sp in spans {
        cuts.push(sp.start.min(len));
        cuts.push(sp.end.min(len));
    }
    cuts.sort();
    cuts.dedup();
    let mut out: Vec<TextSpan> = vec![];
    for w in cuts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let base = spans.iter().find(|sp| sp.start <= a && sp.end >= b).map(|sp| sp.style.clone()).unwrap_or_default();
        let style = if a >= start && b <= end { base.merge(patch) } else { base };
        if style.is_empty() {
            continue;
        }
        match out.last_mut() {
            Some(last) if last.end == a && last.style == style => last.end = b,
            _ => out.push(TextSpan { start: a, end: b, style }),
        }
    }
    out
}

/// Keep spans attached to their characters across an edit from `old` to `new`.
/// Typing at the end of a span extends it, as in Figma.
pub fn shift_spans(spans: &[TextSpan], old: &str, new: &str) -> Vec<TextSpan> {
    if spans.is_empty() || old == new {
        return spans.to_vec();
    }
    let (o, n): (Vec<char>, Vec<char>) = (old.chars().collect(), new.chars().collect());
    let prefix = o.iter().zip(&n).take_while(|(a, b)| a == b).count();
    let max_suffix = o.len().min(n.len()) - prefix;
    let suffix = o.iter().rev().zip(n.iter().rev()).take(max_suffix).take_while(|(a, b)| a == b).count();
    let (old_end, new_end) = (o.len() - suffix, n.len() - suffix);
    let map = |x: usize, is_end: bool| -> usize {
        if x < prefix || (x == prefix && !is_end) {
            x
        } else if x >= old_end && !(x == prefix && is_end) {
            x + new_end - old_end
        } else if is_end {
            new_end
        } else {
            prefix
        }
    };
    spans
        .iter()
        .map(|sp| TextSpan { start: map(sp.start, false), end: map(sp.end, true), style: sp.style.clone() })
        .filter(|sp| sp.end > sp.start)
        .collect()
}

/// Replace every case-insensitive match of `find` (lowercase) in a text layer, keeping spans and lists
/// on the characters they styled. Returns how many were replaced.
pub fn replace_text(n: &mut Node, find: &str, with: &str) -> usize {
    let f: Vec<char> = find.chars().collect();
    if f.is_empty() {
        return 0;
    }
    let chars: Vec<char> = n.text.chars().collect();
    let same = |a: char, b: char| a == b || a.to_lowercase().eq(b.to_lowercase());
    let mut starts = vec![];
    let mut i = 0;
    while i + f.len() <= chars.len() {
        if chars[i..i + f.len()].iter().zip(&f).all(|(a, b)| same(*a, *b)) {
            starts.push(i);
            i += f.len();
        } else {
            i += 1;
        }
    }
    // Last match first, so earlier indices stay put; each step is one contiguous edit.
    for &s in starts.iter().rev() {
        let cur: Vec<char> = n.text.chars().collect();
        let next: String = cur[..s].iter().collect::<String>() + with + &cur[s + f.len()..].iter().collect::<String>();
        n.spans = shift_spans(&n.spans, &n.text, &next);
        n.lists = shift_lists(&n.lists, &n.text, &next);
        n.text = next;
    }
    starts.len()
}

/// Paragraphs after an edit: list formatting follows its paragraph, and new
/// paragraphs typed inside a list item continue the list.
pub fn shift_lists(lists: &[ListLine], old: &str, new: &str) -> Vec<ListLine> {
    if lists.is_empty() || old == new {
        return lists.to_vec();
    }
    let (o, n): (Vec<char>, Vec<char>) = (old.chars().collect(), new.chars().collect());
    let prefix = o.iter().zip(&n).take_while(|(a, b)| a == b).count();
    let max_suffix = o.len().min(n.len()) - prefix;
    let suffix = o.iter().rev().zip(n.iter().rev()).take(max_suffix).take_while(|(a, b)| a == b).count();
    let nl = |s: &[char]| s.iter().filter(|c| **c == '\n').count();
    let p = nl(&o[..prefix]);
    let removed = nl(&o[prefix..o.len() - suffix]);
    let added = nl(&n[prefix..n.len() - suffix]);
    let mut all = lists.to_vec();
    all.resize(nl(&o) + 1, ListLine::default());
    let mut out: Vec<ListLine> = all[..=p].to_vec();
    out.extend(std::iter::repeat_n(all[p], added));
    out.extend_from_slice(&all[(p + 1 + removed).min(all.len())..]);
    while out.last().is_some_and(|l| l.kind == ListKind::Plain) {
        out.pop();
    }
    out
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ListKind {
    #[default]
    Plain,
    Bullet,
    Number,
}

/// One paragraph's list formatting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListLine {
    pub kind: ListKind,
    pub indent: u8,
}

/// How far a layer's visible stroke reaches (inside, outside) its box.
pub fn stroke_reach(n: &Node) -> (f32, f32) {
    if n.stroke_width <= 0.0 || !n.strokes.iter().any(|s| s.visible) {
        return (0.0, 0.0);
    }
    let w = n.stroke_width;
    match n.stroke_align {
        StrokeAlign::Inside => (w, 0.0),
        StrokeAlign::Center => (w / 2.0, w / 2.0),
        StrokeAlign::Outside => (0.0, w),
    }
}

/// Individual stroke sides as filled bands (unrotated world rects), honoring stroke alignment.
/// ponytail: bands are square-cornered; rounded corners with per-side weights are approximate.
pub fn side_bands(n: &Node, [t, rt, b, l]: [f32; 4]) -> Vec<Rect> {
    let r = n.rect;
    let (fi, fo) = match n.stroke_align {
        StrokeAlign::Inside => (1.0, 0.0),
        StrokeAlign::Center => (0.5, 0.5),
        StrokeAlign::Outside => (0.0, 1.0),
    };
    let outer = Rect::from_min_max(pos2(r.min.x - l * fo, r.min.y - t * fo), pos2(r.max.x + rt * fo, r.max.y + b * fo));
    let inner = Rect::from_min_max(pos2(r.min.x + l * fi, r.min.y + t * fi), pos2(r.max.x - rt * fi, r.max.y - b * fi));
    [
        Rect::from_min_max(outer.min, pos2(outer.max.x, inner.min.y)),
        Rect::from_min_max(pos2(inner.max.x, inner.min.y), pos2(outer.max.x, inner.max.y)),
        Rect::from_min_max(pos2(outer.min.x, inner.max.y), outer.max),
        Rect::from_min_max(pos2(outer.min.x, inner.min.y), pos2(inner.min.x, inner.max.y)),
    ]
    .into_iter()
    .filter(|b| b.width() > 0.0 && b.height() > 0.0)
    .collect()
}

/// A size along `axis` kept within the layer's min and max.
pub fn clamp_size(n: &Node, axis: usize, v: f32) -> f32 {
    let v = n.min_size[axis].map_or(v, |m| v.max(m));
    n.max_size[axis].map_or(v, |m| v.min(m))
}

pub type StyleId = u64;

/// Typography captured by a text style.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextStyle {
    pub font_family: String,
    pub font_weight: u16,
    pub font_size: f32,
    pub line_height: LineHeight,
    pub letter_spacing: f32,
    pub decoration: Decoration,
    pub text_case: TextCase,
}

impl TextStyle {
    pub fn of(n: &Node) -> Self {
        Self {
            font_family: n.font_family.clone(),
            font_weight: n.font_weight,
            font_size: n.font_size,
            line_height: n.line_height,
            letter_spacing: n.letter_spacing,
            decoration: n.decoration,
            text_case: n.text_case,
        }
    }
    pub fn apply(&self, n: &mut Node) {
        n.font_family = self.font_family.clone();
        n.font_weight = self.font_weight;
        n.font_size = self.font_size;
        n.line_height = self.line_height;
        n.letter_spacing = self.letter_spacing;
        n.decoration = self.decoration;
        n.text_case = self.text_case;
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum StyleValue {
    Paint(Vec<Paint>),
    Text(TextStyle),
    Effect(Vec<Shadow>),
}

/// A shared style that layers link to; editing it updates every user.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Style {
    pub id: StyleId,
    pub name: String,
    pub value: StyleValue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BlendMode {
    #[default]
    Normal,
    Darken,
    Multiply,
    ColorBurn,
    Lighten,
    Screen,
    ColorDodge,
    Overlay,
    SoftLight,
    HardLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

impl BlendMode {
    pub const ALL: [BlendMode; 16] = [
        BlendMode::Normal,
        BlendMode::Darken,
        BlendMode::Multiply,
        BlendMode::ColorBurn,
        BlendMode::Lighten,
        BlendMode::Screen,
        BlendMode::ColorDodge,
        BlendMode::Overlay,
        BlendMode::SoftLight,
        BlendMode::HardLight,
        BlendMode::Difference,
        BlendMode::Exclusion,
        BlendMode::Hue,
        BlendMode::Saturation,
        BlendMode::Color,
        BlendMode::Luminosity,
    ];
    /// CSS / SVG `mix-blend-mode` keyword.
    pub fn css(self) -> &'static str {
        match self {
            BlendMode::Normal => "normal",
            BlendMode::Darken => "darken",
            BlendMode::Multiply => "multiply",
            BlendMode::ColorBurn => "color-burn",
            BlendMode::Lighten => "lighten",
            BlendMode::Screen => "screen",
            BlendMode::ColorDodge => "color-dodge",
            BlendMode::Overlay => "overlay",
            BlendMode::SoftLight => "soft-light",
            BlendMode::HardLight => "hard-light",
            BlendMode::Difference => "difference",
            BlendMode::Exclusion => "exclusion",
            BlendMode::Hue => "hue",
            BlendMode::Saturation => "saturation",
            BlendMode::Color => "color",
            BlendMode::Luminosity => "luminosity",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ExportFormat {
    #[default]
    Png,
    Jpg,
    Svg,
    Pdf,
    Webp,
}

impl ExportFormat {
    pub const ALL: [ExportFormat; 5] = [ExportFormat::Png, ExportFormat::Jpg, ExportFormat::Webp, ExportFormat::Svg, ExportFormat::Pdf];
    pub fn ext(self) -> &'static str {
        match self {
            ExportFormat::Png => "png",
            ExportFormat::Jpg => "jpg",
            ExportFormat::Svg => "svg",
            ExportFormat::Pdf => "pdf",
            ExportFormat::Webp => "webp",
        }
    }
    pub fn raster(self) -> bool {
        matches!(self, ExportFormat::Png | ExportFormat::Jpg | ExportFormat::Webp)
    }
}

/// How big a raster export is, typed into one field as in Figma: "2x" scales (by `scale`),
/// "512w" and "512h" fix the width or height in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
pub enum ExportSize {
    #[default]
    Scale,
    Width(f32),
    Height(f32),
}

/// One export preset on a layer, like Figma's Export rows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExportSetting {
    pub scale: f32,
    pub format: ExportFormat,
    pub suffix: String,
    #[serde(default, skip_serializing_if = "is_default")]
    pub size: ExportSize,
    /// JPG quality, 1 to 100.
    #[serde(default = "jpg_quality", skip_serializing_if = "is_jpg_quality")]
    pub quality: u8,
}

fn jpg_quality() -> u8 {
    92
}
fn is_jpg_quality(q: &u8) -> bool {
    *q == jpg_quality()
}

impl Default for ExportSetting {
    fn default() -> Self {
        Self { scale: 1.0, format: ExportFormat::Png, suffix: String::new(), size: ExportSize::Scale, quality: jpg_quality() }
    }
}

impl ExportSetting {
    /// The size field's text: "2x", "512w", "512h".
    pub fn size_label(&self) -> String {
        let n = |v: f32| ((v * 100.0).round() / 100.0).to_string();
        match self.size {
            ExportSize::Scale => format!("{}x", n(self.scale)),
            ExportSize::Width(w) => format!("{}w", n(w)),
            ExportSize::Height(h) => format!("{}h", n(h)),
        }
    }

    /// Read what was typed into the size field ("2.5x", "2.5", "512w", "512 h"); false if it
    /// isn't a size, leaving the preset as it was.
    pub fn set_size(&mut self, text: &str) -> bool {
        let t = text.trim().to_lowercase().replace(' ', "");
        let (num, unit) = match t.chars().last() {
            Some(c @ ('x' | 'w' | 'h')) => (&t[..t.len() - 1], c),
            _ => (t.as_str(), 'x'),
        };
        let Some(v) = num.parse::<f32>().ok().filter(|v| v.is_finite() && *v > 0.0) else { return false };
        match unit {
            'w' => self.size = ExportSize::Width(v),
            'h' => self.size = ExportSize::Height(v),
            _ => (self.size, self.scale) = (ExportSize::Scale, v.min(64.0)),
        }
        true
    }

    /// The raster scale that gives this preset's size for content `size` big.
    pub fn scale_for(&self, size: Vec2) -> f32 {
        match self.size {
            ExportSize::Scale => self.scale,
            ExportSize::Width(w) => w / size.x.max(0.01),
            ExportSize::Height(h) => h / size.y.max(0.01),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum GridKind {
    /// Uniform square grid.
    #[default]
    Grid,
    Columns,
    Rows,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum GridAlign {
    #[default]
    Stretch,
    Min,
    Center,
    Max,
}

/// A frame's layout guide (Figma's layout grids).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayoutGrid {
    pub kind: GridKind,
    pub count: u32,
    /// Grid cell size, or track width when not stretching.
    pub size: f32,
    pub gutter: f32,
    /// Margin (stretch) or offset (aligned).
    pub margin: f32,
    pub align: GridAlign,
    pub color: Rgba,
    pub visible: bool,
}

impl Default for LayoutGrid {
    fn default() -> Self {
        Self { kind: GridKind::Grid, count: 5, size: 10.0, gutter: 20.0, margin: 0.0, align: GridAlign::Stretch, color: Rgba([255, 0, 0, 26]), visible: true }
    }
}

impl LayoutGrid {
    /// Column/row tracks between `lo` and `hi` as (start, end) pairs.
    pub fn bands(&self, lo: f32, hi: f32) -> Vec<(f32, f32)> {
        let n = self.count.max(1) as usize;
        let gutters = self.gutter * (n as f32 - 1.0);
        let (track, start) = match self.align {
            GridAlign::Stretch => (((hi - lo) - 2.0 * self.margin - gutters) / n as f32, lo + self.margin),
            GridAlign::Min => (self.size, lo + self.margin),
            GridAlign::Max => (self.size, hi - self.margin - (self.size * n as f32 + gutters)),
            GridAlign::Center => (self.size, (lo + hi) / 2.0 - (self.size * n as f32 + gutters) / 2.0 + self.margin),
        };
        (0..n).map(|i| {
            let a = start + i as f32 * (track + self.gutter);
            (a, a + track.max(0.0))
        }).collect()
    }
}

/// What sits at the ends of an open path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Marker {
    #[default]
    None,
    Arrow,
    Triangle,
    Circle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Cap {
    #[default]
    None,
    Round,
    Square,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Join {
    #[default]
    Miter,
    Round,
    Bevel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StrokeAlign {
    Inside,
    Center,
    Outside,
}

/// A vector anchor. `p` is relative to the node's `rect.min`; handles are offsets from `p`.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct PathPoint {
    pub p: Pos2,
    pub h_in: Vec2,
    pub h_out: Vec2,
}

/// One contour of a vector. Anchors are relative to the node's `rect.min`.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct SubPath {
    pub points: Vec<PathPoint>,
    pub closed: bool,
}

/// Unmultiplied sRGBA. Unlike egui's premultiplied `Color32`, a 0% opacity color keeps its hue.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Rgba(pub [u8; 4]);

impl Rgba {
    pub const BLACK: Rgba = Rgba([0, 0, 0, 255]);
    pub const WHITE: Rgba = Rgba([255, 255, 255, 255]);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Rgba([r, g, b, 255])
    }
    pub fn to32(self) -> Color32 {
        let [r, g, b, a] = self.0;
        Color32::from_rgba_unmultiplied(r, g, b, a)
    }
    pub fn a(self) -> u8 {
        self.0[3]
    }
    pub fn with_alpha(self, a: u8) -> Self {
        Rgba([self.0[0], self.0[1], self.0[2], a])
    }
    pub fn hex(self) -> String {
        format!("{:02X}{:02X}{:02X}", self.0[0], self.0[1], self.0[2])
    }
    pub fn lerp(self, o: Rgba, t: f32) -> Rgba {
        Rgba(std::array::from_fn(|i| (self.0[i] as f32 + (o.0[i] as f32 - self.0[i] as f32) * t).round() as u8))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PaintKind {
    #[default]
    Solid,
    Linear,
    Radial,
    Image,
}

/// How an image fills its layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ScaleMode {
    /// Cover, cropping overflow.
    #[default]
    Fill,
    /// Contain, letterboxed.
    Fit,
    Stretch,
    Tile,
    /// Figma's Crop: like Fill, zoomed by `tile_scale` (1 = cover) and shifted by `from`
    /// (-1..1 of the room to move on each axis, 0 = centered).
    Crop,
}

/// An embedded image's pixel size (read from its header).
pub fn image_size(doc: &Document, key: &str) -> Option<Vec2> {
    let bytes = doc.images.get(key)?;
    let (w, h) = image::ImageReader::new(std::io::Cursor::new(bytes.as_slice())).with_guessed_format().ok()?.into_dimensions().ok()?;
    Some(vec2(w as f32, h as f32))
}

/// Crop settings from Figma's imageTransform (layer unit square -> image unit square).
pub fn crop_from_transform(p: &mut Paint, t: [[f32; 3]; 2], layer: Vec2, image: Vec2) {
    let (a, d, tx, ty) = (t[0][0].max(1e-6), t[1][1].max(1e-6), t[0][2], t[1][2]);
    let (la, ia) = (layer.x / layer.y.max(1e-6), image.x / image.y.max(1e-6));
    let cover = if la > ia { vec2(1.0, ia / la) } else { vec2(la / ia, 1.0) };
    p.scale_mode = ScaleMode::Crop;
    p.tile_scale = (cover.x / a).max(cover.y / d).max(1.0);
    let span = cover / p.tile_scale;
    let c = pos2(tx + a / 2.0, ty + d / 2.0);
    let off = |c: f32, s: f32| if 1.0 - s > 1e-6 { ((c - 0.5) / ((1.0 - s) / 2.0)).clamp(-1.0, 1.0) } else { 0.0 };
    p.from = pos2(off(c.x, span.x), off(c.y, span.y));
}

/// The part of an image a Fill or Crop paint shows, in the image's unit square: center and size.
pub fn crop_window(p: &Paint, layer: Vec2, image: Vec2) -> (Pos2, Vec2) {
    let (la, ia) = (layer.x / layer.y.max(1e-6), image.x / image.y.max(1e-6));
    let cover = if la > ia { vec2(1.0, ia / la) } else { vec2(la / ia, 1.0) };
    if p.scale_mode != ScaleMode::Crop {
        return (pos2(0.5, 0.5), cover);
    }
    let span = cover / p.tile_scale.max(1.0);
    let c = pos2(0.5 + p.from.x.clamp(-1.0, 1.0) * (1.0 - span.x) / 2.0, 0.5 + p.from.y.clamp(-1.0, 1.0) * (1.0 - span.y) / 2.0);
    (c, span)
}

/// A fill or stroke paint. For gradients and images, `color`'s alpha is the paint opacity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Paint {
    pub color: Rgba,
    pub visible: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub kind: PaintKind,
    /// Gradient stops: position in 0..=1 and color.
    #[serde(default, skip_serializing_if = "is_default")]
    pub stops: Vec<(f32, Rgba)>,
    /// Gradient handles in the layer's unit square: linear start/end, or radial center/edge.
    #[serde(default, skip_serializing_if = "is_default")]
    pub from: Pos2,
    #[serde(default, skip_serializing_if = "is_default")]
    pub to: Pos2,
    /// Key into `Document::images`.
    #[serde(default, skip_serializing_if = "is_default")]
    pub image: String,
    #[serde(default, skip_serializing_if = "is_default")]
    pub scale_mode: ScaleMode,
    /// Tile size relative to the image's pixels.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub tile_scale: f32,
    /// Image paints: color adjustments.
    #[serde(default, skip_serializing_if = "is_default")]
    pub adjust: ImageAdjust,
}

/// Figma's image adjustments, each -1..=1 (0 = unchanged).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImageAdjust {
    pub exposure: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub temperature: f32,
    pub tint: f32,
    pub highlights: f32,
    pub shadows: f32,
}

impl ImageAdjust {
    pub fn is_identity(&self) -> bool {
        *self == Self::default()
    }

    /// Adjust unpremultiplied RGBA pixels in place.
    /// ponytail: simple per-pixel curves, close to Figma's look but not its exact math.
    pub fn apply(&self, px: &mut [u8]) {
        if self.is_identity() {
            return;
        }
        let gain = 2f32.powf(self.exposure);
        for p in px.chunks_exact_mut(4) {
            let mut c = [p[0], p[1], p[2]].map(|v| v as f32 / 255.0);
            for v in &mut c {
                *v = (*v * gain - 0.5) * (1.0 + self.contrast) + 0.5;
            }
            let luma = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
            let tone = if luma > 0.5 { self.highlights * (luma - 0.5) } else { self.shadows * (0.5 - luma) };
            for v in &mut c {
                *v = luma + (*v - luma) * (1.0 + self.saturation) + tone * 0.5;
            }
            c[0] += self.temperature * 0.12;
            c[2] -= self.temperature * 0.12;
            c[1] -= self.tint * 0.1;
            for (o, v) in p.iter_mut().zip(c) {
                *o = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
    }
}

fn one() -> f32 {
    1.0
}

impl Paint {
    pub fn solid(color: Rgba) -> Self {
        Self {
            color,
            visible: true,
            kind: PaintKind::Solid,
            stops: vec![],
            from: Pos2::ZERO,
            to: Pos2::ZERO,
            image: String::new(),
            scale_mode: ScaleMode::Fill,
            tile_scale: 1.0,
            adjust: ImageAdjust::default(),
        }
    }

    /// An image fill showing the document image stored under `key`.
    pub fn image(key: String) -> Self {
        let mut p = Self::solid(Rgba::WHITE);
        p.convert(PaintKind::Image);
        p.image = key;
        p
    }

    /// Switch kind, seeding Figma's defaults from the current color.
    pub fn convert(&mut self, kind: PaintKind) {
        if kind == self.kind {
            return;
        }
        let a = self.color.a();
        let base = if self.kind == PaintKind::Solid { self.color.with_alpha(255) } else { self.stops.first().map_or(Rgba::BLACK, |s| s.1) };
        match kind {
            PaintKind::Solid => self.color = base.with_alpha(a),
            PaintKind::Linear | PaintKind::Radial => {
                if self.stops.len() < 2 {
                    self.stops = vec![(0.0, base), (1.0, base.with_alpha(0))];
                }
                (self.from, self.to) = if kind == PaintKind::Linear {
                    (pos2(0.5, 0.0), pos2(0.5, 1.0))
                } else {
                    (pos2(0.5, 0.5), pos2(1.0, 0.5))
                };
                self.color = Rgba::WHITE.with_alpha(a);
            }
            PaintKind::Image => self.color = Rgba::WHITE.with_alpha(a),
        }
        self.kind = kind;
    }

    /// Paint opacity in 0..=1.
    pub fn alpha(&self) -> f32 {
        self.color.a() as f32 / 255.0
    }

    /// Gradient color at `t`, interpolated in unmultiplied sRGB like Figma.
    pub fn sample(&self, t: f32) -> Rgba {
        let stops = &self.stops;
        let Some(first) = stops.first() else { return Rgba([0, 0, 0, 0]) };
        if t <= first.0 {
            return first.1;
        }
        for w in stops.windows(2) {
            if t <= w[1].0 {
                let span = (w[1].0 - w[0].0).max(1e-6);
                return w[0].1.lerp(w[1].1, (t - w[0].0) / span);
            }
        }
        stops.last().unwrap().1
    }

    /// Gradient parameter for a point in the layer's unit square.
    pub fn gradient_t(&self, p: Pos2) -> f32 {
        let d = self.to - self.from;
        match self.kind {
            PaintKind::Radial => (p - self.from).length() / d.length().max(1e-6),
            _ => (p - self.from).dot(d) / d.length_sq().max(1e-6),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum EffectKind {
    #[default]
    DropShadow,
    InnerShadow,
    LayerBlur,
    BackgroundBlur,
}

impl EffectKind {
    pub fn label(self) -> &'static str {
        match self {
            EffectKind::DropShadow => "Drop shadow",
            EffectKind::InnerShadow => "Inner shadow",
            EffectKind::LayerBlur => "Layer blur",
            EffectKind::BackgroundBlur => "Background blur",
        }
    }
    pub fn is_shadow(self) -> bool {
        matches!(self, EffectKind::DropShadow | EffectKind::InnerShadow)
    }
}

/// A layer effect. Blurs use only `blur`; shadows use everything.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Shadow {
    #[serde(default)]
    pub kind: EffectKind,
    pub color: Rgba,
    pub offset: Vec2,
    pub blur: f32,
    pub spread: f32,
    pub visible: bool,
}

impl Default for Shadow {
    /// Figma's default drop shadow.
    fn default() -> Self {
        Self { kind: EffectKind::DropShadow, color: Rgba([0, 0, 0, 64]), offset: vec2(0.0, 4.0), blur: 4.0, spread: 0.0, visible: true }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Node {
    #[serde(skip_serializing_if = "dflt::id")]
    pub id: NodeId,
    #[serde(skip_serializing_if = "dflt::name")]
    pub name: String,
    #[serde(skip_serializing_if = "dflt::kind")]
    pub kind: Kind,
    #[serde(skip_serializing_if = "dflt::parent")]
    pub parent: Option<NodeId>,
    #[serde(skip_serializing_if = "dflt::children")]
    pub children: Vec<NodeId>,
    #[serde(skip_serializing_if = "dflt::rect")]
    pub rect: Rect,
    /// Degrees counter-clockwise around the rect center, as shown in Figma.
    #[serde(skip_serializing_if = "dflt::rotation")]
    pub rotation: f32,
    #[serde(skip_serializing_if = "dflt::visible")]
    pub visible: bool,
    #[serde(skip_serializing_if = "dflt::locked")]
    pub locked: bool,
    #[serde(skip_serializing_if = "dflt::opacity")]
    pub opacity: f32,
    #[serde(skip_serializing_if = "dflt::fills")]
    pub fills: Vec<Paint>,
    #[serde(skip_serializing_if = "dflt::strokes")]
    pub strokes: Vec<Paint>,
    #[serde(skip_serializing_if = "dflt::stroke_width")]
    pub stroke_width: f32,
    #[serde(skip_serializing_if = "dflt::stroke_align")]
    pub stroke_align: StrokeAlign,
    #[serde(skip_serializing_if = "dflt::radius")]
    pub radius: f32,
    /// Independent corners: top-left, top-right, bottom-right, bottom-left.
    #[serde(skip_serializing_if = "dflt::corner_radii")]
    pub corner_radii: Option<[f32; 4]>,
    /// Dash pattern (dash, gap, ...); empty = solid.
    #[serde(skip_serializing_if = "dflt::dash")]
    pub dash: Vec<f32>,
    #[serde(skip_serializing_if = "dflt::cap")]
    pub cap: Cap,
    #[serde(skip_serializing_if = "dflt::join")]
    pub join: Join,
    #[serde(skip_serializing_if = "dflt::start_marker")]
    pub start_marker: Marker,
    #[serde(skip_serializing_if = "dflt::end_marker")]
    pub end_marker: Marker,
    /// Polygon sides / star points.
    #[serde(skip_serializing_if = "dflt::point_count")]
    pub point_count: u32,
    /// Star inner radius as a fraction of the outer.
    #[serde(skip_serializing_if = "dflt::inner_ratio")]
    pub inner_ratio: f32,
    /// Ellipse arc, as in Figma: start angle and sweep in degrees (clockwise from 3 o'clock) and
    /// the inner radius as a fraction of the outer. None draws the whole ellipse.
    #[serde(skip_serializing_if = "dflt::arc")]
    pub arc: Option<[f32; 3]>,
    #[serde(skip_serializing_if = "dflt::clip")]
    pub clip: bool,
    #[serde(skip_serializing_if = "dflt::shadows")]
    pub shadows: Vec<Shadow>,
    #[serde(skip_serializing_if = "dflt::layout")]
    pub layout: AutoLayout,
    /// Horizontal, vertical.
    #[serde(skip_serializing_if = "dflt::sizing")]
    pub sizing: [Sizing; 2],
    /// Rectangles and frames: individual stroke weights (top, right, bottom, left).
    #[serde(skip_serializing_if = "dflt::stroke_sides")]
    pub stroke_sides: Option<[f32; 4]>,
    /// Min and max width/height for hugging and filling layers.
    #[serde(skip_serializing_if = "dflt::min_size")]
    pub min_size: [Option<f32>; 2],
    #[serde(skip_serializing_if = "dflt::max_size")]
    pub max_size: [Option<f32>; 2],
    #[serde(skip_serializing_if = "dflt::constraints")]
    pub constraints: [Constraint; 2],
    /// Ignores the parent's auto layout.
    #[serde(skip_serializing_if = "dflt::absolute")]
    pub absolute: bool,
    /// In a grid auto layout: how many columns and rows this layer's cell spans.
    #[serde(skip_serializing_if = "dflt::grid_span")]
    pub grid_span: [u32; 2],
    #[serde(skip_serializing_if = "dflt::bool_op")]
    pub bool_op: BoolOp,
    /// Clips the layers above it in the same parent.
    #[serde(skip_serializing_if = "dflt::is_mask")]
    pub is_mask: bool,
    #[serde(skip_serializing_if = "dflt::grids")]
    pub grids: Vec<LayoutGrid>,
    #[serde(skip_serializing_if = "dflt::exports")]
    pub exports: Vec<ExportSetting>,
    #[serde(skip_serializing_if = "dflt::blend")]
    pub blend: BlendMode,
    /// Prototype interactions.
    #[serde(skip_serializing_if = "dflt::reactions")]
    pub reactions: Vec<Reaction>,
    /// Frames: prototype scrolling.
    #[serde(skip_serializing_if = "dflt::overflow")]
    pub overflow: Overflow,
    /// Children of scrolling frames: scroll along, stay fixed, or stick.
    #[serde(skip_serializing_if = "dflt::scroll_pos")]
    pub scroll_pos: ScrollPos,
    /// Properties driven by variables.
    #[serde(skip_serializing_if = "dflt::bindings")]
    pub bindings: Vec<(Bindable, VarId)>,
    /// Explicit variable modes set on this layer: (collection, mode index); children inherit.
    #[serde(skip_serializing_if = "dflt::modes")]
    pub modes: Vec<(u64, usize)>,
    /// Linked shared styles.
    #[serde(skip_serializing_if = "dflt::fill_style")]
    pub fill_style: Option<StyleId>,
    #[serde(skip_serializing_if = "dflt::stroke_style")]
    pub stroke_style: Option<StyleId>,
    #[serde(skip_serializing_if = "dflt::text_style")]
    pub text_style: Option<StyleId>,
    #[serde(skip_serializing_if = "dflt::effect_style")]
    pub effect_style: Option<StyleId>,
    /// Components and sets: property definitions.
    #[serde(skip_serializing_if = "dflt::comp_props")]
    pub comp_props: Vec<CompProp>,
    /// A layer inside a component bound to a property.
    #[serde(deserialize_with = "one_or_many_binds")]
    #[serde(skip_serializing_if = "dflt::prop_bind")]
    pub prop_bind: Vec<(String, PropTarget)>,
    /// Instances: chosen property values (missing = the default).
    #[serde(skip_serializing_if = "dflt::prop_values")]
    pub prop_values: Vec<(String, PropValue)>,
    /// Instances: the main component they copy.
    #[serde(skip_serializing_if = "dflt::component")]
    pub component: Option<NodeId>,
    /// Inside an instance (and the instance itself): the component layer this mirrors.
    #[serde(skip_serializing_if = "dflt::source")]
    pub source: Option<NodeId>,
    /// The source's values as last copied; a property that differs from this is an override.
    #[serde(skip_serializing_if = "dflt::synced")]
    pub synced: Option<Box<Node>>,
    #[serde(skip_serializing_if = "dflt::paths")]
    pub paths: Vec<SubPath>,
    /// Even-odd fill rule (holes by parity) instead of non-zero winding.
    #[serde(skip_serializing_if = "dflt::even_odd")]
    pub even_odd: bool,
    #[serde(skip_serializing_if = "dflt::text")]
    pub text: String,
    #[serde(skip_serializing_if = "dflt::font_size")]
    pub font_size: f32,
    #[serde(skip_serializing_if = "dflt::font_weight")]
    pub font_weight: u16,
    #[serde(skip_serializing_if = "dflt::font_family")]
    pub font_family: String,
    #[serde(skip_serializing_if = "dflt::text_sizing")]
    pub text_sizing: TextSizing,
    #[serde(skip_serializing_if = "dflt::text_align")]
    pub text_align: TextAlign,
    #[serde(skip_serializing_if = "dflt::text_valign")]
    pub text_valign: VAlign,
    #[serde(skip_serializing_if = "dflt::line_height")]
    pub line_height: LineHeight,
    /// Percent of the font size.
    #[serde(skip_serializing_if = "dflt::letter_spacing")]
    pub letter_spacing: f32,
    #[serde(skip_serializing_if = "dflt::decoration")]
    pub decoration: Decoration,
    #[serde(skip_serializing_if = "dflt::text_case")]
    pub text_case: TextCase,
    /// Extra space after each paragraph.
    #[serde(skip_serializing_if = "dflt::paragraph_spacing")]
    pub paragraph_spacing: f32,
    /// End overflowing text with an ellipsis.
    #[serde(skip_serializing_if = "dflt::truncate")]
    pub truncate: bool,
    /// With truncation on: lines before the ellipsis (0 = as many as fit).
    #[serde(skip_serializing_if = "dflt::max_lines")]
    pub max_lines: u32,
    /// Styled character ranges (rich text).
    #[serde(skip_serializing_if = "dflt::spans")]
    pub spans: Vec<TextSpan>,
    /// List formatting per paragraph (missing paragraphs are plain).
    #[serde(skip_serializing_if = "dflt::lists")]
    pub lists: Vec<ListLine>,
    /// Wrapped lines from the last layout, used by export.
    #[serde(skip)]
    pub lines: Vec<String>,
    #[serde(skip_serializing_if = "dflt::expanded")]
    pub expanded: bool,
}

/// Serialization leaves out fields at `Node::default()`'s values; `#[serde(default)]` fills
/// them back in on load. Files get several times smaller and faster to write and read.
mod dflt {
    use super::*;
    thread_local!(static D: Node = Node::default());
    macro_rules! same {
        ($($f:ident: $t:ty),* $(,)?) => { $(pub fn $f(v: &$t) -> bool { D.with(|d| d.$f == *v) })* };
    }
    same!(
        id: NodeId,
        name: String,
        kind: Kind,
        parent: Option<NodeId>,
        children: Vec<NodeId>,
        rect: Rect,
        rotation: f32,
        visible: bool,
        locked: bool,
        opacity: f32,
        fills: Vec<Paint>,
        strokes: Vec<Paint>,
        stroke_width: f32,
        stroke_align: StrokeAlign,
        radius: f32,
        corner_radii: Option<[f32; 4]>,
        dash: Vec<f32>,
        cap: Cap,
        join: Join,
        start_marker: Marker,
        end_marker: Marker,
        point_count: u32,
        inner_ratio: f32,
        arc: Option<[f32; 3]>,
        clip: bool,
        shadows: Vec<Shadow>,
        layout: AutoLayout,
        sizing: [Sizing; 2],
        stroke_sides: Option<[f32; 4]>,
        min_size: [Option<f32>; 2],
        max_size: [Option<f32>; 2],
        constraints: [Constraint; 2],
        absolute: bool,
        grid_span: [u32; 2],
        bool_op: BoolOp,
        is_mask: bool,
        grids: Vec<LayoutGrid>,
        exports: Vec<ExportSetting>,
        blend: BlendMode,
        reactions: Vec<Reaction>,
        overflow: Overflow,
        scroll_pos: ScrollPos,
        bindings: Vec<(Bindable, VarId)>,
        modes: Vec<(u64, usize)>,
        fill_style: Option<StyleId>,
        stroke_style: Option<StyleId>,
        text_style: Option<StyleId>,
        effect_style: Option<StyleId>,
        comp_props: Vec<CompProp>,
        prop_bind: Vec<(String, PropTarget)>,
        prop_values: Vec<(String, PropValue)>,
        component: Option<NodeId>,
        source: Option<NodeId>,
        synced: Option<Box<Node>>,
        paths: Vec<SubPath>,
        even_odd: bool,
        text: String,
        font_size: f32,
        font_weight: u16,
        font_family: String,
        text_sizing: TextSizing,
        text_align: TextAlign,
        text_valign: VAlign,
        line_height: LineHeight,
        letter_spacing: f32,
        decoration: Decoration,
        text_case: TextCase,
        paragraph_spacing: f32,
        truncate: bool,
        max_lines: u32,
        spans: Vec<TextSpan>,
        lists: Vec<ListLine>,
        expanded: bool,
    );
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}
fn is_one(v: &f32) -> bool {
    *v == 1.0
}

impl Default for Node {
    fn default() -> Self {
        Self {
            id: 0,
            name: String::new(),
            kind: Kind::Rectangle,
            parent: None,
            children: vec![],
            rect: Rect::from_min_size(Pos2::ZERO, vec2(100.0, 100.0)),
            rotation: 0.0,
            visible: true,
            locked: false,
            opacity: 1.0,
            fills: vec![],
            strokes: vec![],
            stroke_width: 1.0,
            stroke_align: StrokeAlign::Inside,
            radius: 0.0,
            corner_radii: None,
            dash: vec![],
            cap: Cap::None,
            join: Join::Miter,
            start_marker: Marker::None,
            end_marker: Marker::None,
            point_count: 5,
            inner_ratio: 0.382,
            arc: None,
            clip: false,
            shadows: vec![],
            layout: AutoLayout::default(),
            sizing: [Sizing::Fixed; 2],
            stroke_sides: None,
            min_size: [None; 2],
            max_size: [None; 2],
            constraints: [Constraint::Min; 2],
            absolute: false,
            grid_span: [1, 1],
            bool_op: BoolOp::Union,
            is_mask: false,
            grids: vec![],
            exports: vec![],
            blend: BlendMode::Normal,
            reactions: vec![],
            overflow: Overflow::None,
            scroll_pos: ScrollPos::Scroll,
            bindings: vec![],
            modes: vec![],
            fill_style: None,
            stroke_style: None,
            text_style: None,
            effect_style: None,
            comp_props: vec![],
            prop_bind: vec![],
            prop_values: vec![],
            component: None,
            source: None,
            synced: None,
            paths: vec![],
            even_odd: false,
            text: String::new(),
            font_size: 12.0,
            font_weight: 400,
            font_family: "Inter".into(),
            text_sizing: TextSizing::AutoWidth,
            text_align: TextAlign::Left,
            text_valign: VAlign::Top,
            line_height: LineHeight::Auto,
            letter_spacing: 0.0,
            decoration: Decoration::None,
            text_case: TextCase::Original,
            paragraph_spacing: 0.0,
            truncate: false,
            max_lines: 0,
            spans: vec![],
            lists: vec![],
            lines: vec![],
            expanded: true,
        }
    }
}

pub const SHAPE_GRAY: Rgba = Rgba::rgb(0xD9, 0xD9, 0xD9);

impl Node {
    /// A node with Figma's defaults for its kind.
    pub fn new(kind: Kind, rect: Rect) -> Self {
        let mut n = Node { kind, rect, ..Default::default() };
        match kind {
            Kind::Frame | Kind::Component | Kind::Instance => {
                n.fills = vec![Paint::solid(Rgba::WHITE)];
                n.clip = true;
            }
            Kind::ComponentSet => {}
            Kind::Rectangle | Kind::Ellipse | Kind::Star => n.fills = vec![Paint::solid(SHAPE_GRAY)],
            Kind::Polygon => {
                n.fills = vec![Paint::solid(SHAPE_GRAY)];
                n.point_count = 3;
            }
            Kind::Section => {
                n.fills = vec![Paint::solid(Rgba::WHITE)];
                n.strokes = vec![Paint::solid(Rgba::rgb(0xE6, 0xE6, 0xE6))];
                n.radius = 2.0;
            }
            Kind::Slice => {}
            Kind::Vector => {
                n.strokes = vec![Paint::solid(Rgba::BLACK)];
                n.stroke_align = StrokeAlign::Center;
            }
            Kind::Text => n.fills = vec![Paint::solid(Rgba::BLACK)],
            Kind::Boolean => n.fills = vec![Paint::solid(SHAPE_GRAY)],
            Kind::Group => {}
        }
        n
    }

    /// Build a vector node from absolute anchors; bounds come from the anchors and handles.
    pub fn vector(abs: &[PathPoint], closed: bool) -> Self {
        Node::vector_paths(vec![SubPath { points: abs.to_vec(), closed }])
    }

    /// Build a vector node from absolute contours.
    pub fn vector_paths(abs: Vec<SubPath>) -> Self {
        let mut n = Node::new(Kind::Vector, Rect::NOTHING);
        n.paths = abs;
        n.refit_vector();
        n
    }

    /// Whether the shape encloses area that fills can paint.
    pub fn has_area(&self) -> bool {
        self.kind != Kind::Vector || self.paths.iter().any(|p| p.closed)
    }

    /// Re-derive `rect` so it tightly wraps every contour (anchors relative to it).
    pub fn refit_vector(&mut self) {
        let origin = if self.rect.is_finite() { self.rect.min.to_vec2() } else { Vec2::ZERO };
        let mut r = Rect::NOTHING;
        for sp in &mut self.paths {
            for pt in sp.points.iter_mut() {
                pt.p += origin;
            }
            let k = sp.points.len();
            if k == 1 {
                r.extend_with(sp.points[0].p);
            }
            let segs = if sp.closed { k } else { k.saturating_sub(1) };
            for i in 0..segs {
                let (a, b) = (sp.points[i], sp.points[(i + 1) % k]);
                r = r.union(cubic_bounds(a.p, a.p + a.h_out, b.p + b.h_in, b.p));
            }
        }
        if !r.is_finite() {
            r = Rect::from_min_size(Pos2::ZERO, Vec2::ZERO);
        }
        for sp in &mut self.paths {
            for pt in &mut sp.points {
                pt.p -= r.min.to_vec2();
            }
        }
        self.rect = r;
    }

    /// Unrotated outline of polygons and stars, in world space.
    /// An ellipse with a partial sweep or a hole.
    pub fn is_arc(&self) -> bool {
        self.kind == Kind::Ellipse && self.arc.is_some_and(|a| a != [0.0, 360.0, 0.0])
    }

    /// An arc ellipse's outline, as closed polygons: the outer arc, then the inner arc back (a
    /// ring or donut slice) or the center (a pie slice). A full ring is two loops, the inner one
    /// reversed so it's a hole under either fill rule. `grow` widens the radii (stroke alignment).
    pub fn arc_outline(&self, grow: f32) -> Vec<Vec<Pos2>> {
        let [start, sweep, ratio] = self.arc.unwrap_or([0.0, 360.0, 0.0]);
        let sweep = sweep.clamp(-360.0, 360.0);
        let c = self.rect.center();
        let (rx, ry) = (self.rect.width() / 2.0 + grow, self.rect.height() / 2.0 + grow);
        let (irx, iry) = ((self.rect.width() / 2.0 * ratio - grow).max(0.0), (self.rect.height() / 2.0 * ratio - grow).max(0.0));
        let n = ((sweep.abs() / 360.0) * 96.0).ceil().max(8.0) as usize;
        let at = |i: usize, a: f32, b: f32| {
            let t = (start + sweep * i as f32 / n as f32).to_radians();
            c + vec2(a * t.cos(), b * t.sin())
        };
        let hole = ratio > 0.0 && irx > 0.0 && iry > 0.0;
        if sweep.abs() >= 359.99 {
            let mut loops = vec![(0..n).map(|i| at(i, rx, ry)).collect::<Vec<_>>()];
            if hole {
                loops.push((0..n).rev().map(|i| at(i, irx, iry)).collect());
            }
            return loops;
        }
        let mut p: Vec<Pos2> = (0..=n).map(|i| at(i, rx, ry)).collect();
        if hole {
            p.extend((0..=n).rev().map(|i| at(i, irx, iry)));
        } else {
            p.push(c);
        }
        vec![p]
    }

    pub fn star_points(&self) -> Vec<Pos2> {
        let r = self.rect;
        let (c, rx, ry) = (r.center(), r.width() / 2.0, r.height() / 2.0);
        let n = self.point_count.max(3) as usize;
        let star = self.kind == Kind::Star;
        let steps = if star { n * 2 } else { n };
        (0..steps)
            .map(|i| {
                // Point straight up first, like Figma.
                let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * i as f32 / steps as f32;
                let k = if star && i % 2 == 1 { self.inner_ratio } else { 1.0 };
                pos2(c.x + rx * k * a.cos(), c.y + ry * k * a.sin())
            })
            .collect()
    }

    /// Corner radii (tl, tr, br, bl), uniform unless set independently.
    pub fn radii(&self) -> [f32; 4] {
        self.corner_radii.unwrap_or([self.radius; 4])
    }

    /// World corners (top-left, top-right, bottom-right, bottom-left), rotation applied.
    pub fn corners(&self) -> [Pos2; 4] {
        let r = self.rect;
        [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()].map(|p| rotate(p, r.center(), self.rotation))
    }

    /// Axis-aligned bounds in world space.
    pub fn aabb(&self) -> Rect {
        if self.rotation == 0.0 {
            return self.rect;
        }
        Rect::from_points(&self.corners())
    }

    /// World point mapped into the node's unrotated frame.
    pub fn to_local(&self, p: Pos2) -> Pos2 {
        rotate(p, self.rect.center(), -self.rotation)
    }

    pub fn first_fill(&self) -> Option<Rgba> {
        self.fills.iter().rev().find(|p| p.visible && p.kind == PaintKind::Solid).map(|p| p.color)
    }

    /// Children bottom to top as painted: auto layout's "First on top" stacking reverses them.
    pub fn paint_order(&self) -> Vec<NodeId> {
        let mut v = self.children.clone();
        if self.layout.reverse_z && self.layout.mode != LayoutMode::None {
            v.reverse();
        }
        v
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub name: String,
    pub children: Vec<NodeId>,
    pub background: Rgba,
    /// Prototype starting frames, in order.
    #[serde(default)]
    pub flows: Vec<NodeId>,
    /// Where the page was last viewed: world center x, y and zoom (saved in the file, as Figma does).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<[f32; 3]>,
    /// Ruler guides dragged onto the page.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guides: Vec<Guide>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub prototype: ProtoSettings,
}

/// Which way a ruler guide runs: an X guide is a vertical line at an x offset, Y horizontal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Axis {
    X,
    Y,
}

/// A page guide from the rulers (Figma's `Guide`: an axis and a world offset).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Guide {
    pub axis: Axis,
    pub offset: f32,
}

pub const PAGE_BG: Rgba = Rgba::rgb(0xF5, 0xF5, 0xF5);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Document {
    pub pages: Vec<Page>,
    /// Shared with undo snapshots: editing a layer copies just that layer (`node_mut`).
    pub nodes: HashMap<NodeId, std::sync::Arc<Node>>,
    pub next_id: NodeId,
    /// Embedded images: content hash -> encoded PNG/JPEG bytes (base64 in files).
    #[serde(default, with = "images_b64")]
    pub images: HashMap<String, std::sync::Arc<Vec<u8>>>,
    #[serde(default)]
    pub styles: Vec<Style>,
    #[serde(default)]
    pub comments: Vec<Comment>,
    #[serde(default)]
    pub collections: Vec<Collection>,
    #[serde(default)]
    pub variables: Vec<Variable>,
    #[serde(skip)]
    pub cache: SyncCache,
}

/// Remembers which instances are already in sync, so editing a big file doesn't re-sync
/// thousands of instances. Shared between clones (undo snapshots, drag bases); entries are
/// validated by comparing component content, so a stale entry just causes a re-sync.
#[derive(Clone, Default)]
pub struct SyncCache(std::sync::Arc<std::sync::Mutex<SyncState>>);

#[derive(Default)]
pub struct SyncState {
    /// Component -> (revision, subtree as last seen).
    comps: HashMap<NodeId, (u64, Vec<Node>)>,
    /// Instance -> what it was last synced against.
    insts: HashMap<NodeId, InstKey>,
    next_rev: u64,
}

#[derive(Clone, PartialEq)]
struct InstKey {
    comp: NodeId,
    rev: u64,
    size: (u32, u32),
    props: Vec<(String, PropValue)>,
}

impl PartialEq for SyncCache {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl std::fmt::Debug for SyncCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SyncCache")
    }
}

/// Whether a text layer still has the name Katachi gave it (its text, or "Text" / "Text 3").
pub fn auto_text_name(name: &str, text: &str) -> bool {
    let generic = name == "Text" || name.strip_prefix("Text ").is_some_and(|n| n.parse::<u32>().is_ok());
    generic || name == text_layer_name(text)
}

/// A text layer's automatic name: its text on one line, cut at 60 characters.
pub fn text_layer_name(text: &str) -> String {
    let one: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = one.chars().take(60).collect();
    if cut.is_empty() { "Text".into() } else { cut }
}

/// A .katachi file: `{"katachi": 1, "document": {...}}`.
#[derive(Deserialize)]
struct FileJson {
    document: Document,
}

/// Read a .katachi file's text.
pub fn parse_file(text: &str) -> Result<Document, String> {
    serde_json::from_str::<FileJson>(text).map(|f| f.document).map_err(|e| e.to_string())
}

/// A .katachi file's text.
pub fn file_json(doc: &Document) -> String {
    // Straight to text: going through a serde_json::Value doubles the time on big files.
    format!(r#"{{"katachi":1,"document":{}}}"#, serde_json::to_string(doc).unwrap_or_default())
}

mod images_b64 {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::HashMap;
    use std::sync::Arc;

    pub fn serialize<S: Serializer>(m: &HashMap<String, Arc<Vec<u8>>>, s: S) -> Result<S::Ok, S::Error> {
        let enc: HashMap<&String, String> = m.iter().map(|(k, v)| (k, STANDARD.encode(v.as_slice()))).collect();
        enc.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<HashMap<String, Arc<Vec<u8>>>, D::Error> {
        let enc = HashMap::<String, String>::deserialize(d)?;
        enc.into_iter()
            .map(|(k, v)| STANDARD.decode(v).map(|b| (k, Arc::new(b))).map_err(serde::de::Error::custom))
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    Forward,
    Backward,
    Front,
    Back,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            pages: vec![Page { name: "Page 1".into(), children: vec![], background: PAGE_BG, flows: vec![], view: None, guides: vec![], prototype: Default::default() }],
            nodes: HashMap::new(),
            next_id: 1,
            images: HashMap::new(),
            styles: vec![],
            comments: vec![],
            collections: vec![],
            variables: vec![],
            cache: SyncCache::default(),
        }
    }
}

impl Document {
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[&id]
    }
    pub fn node_mut(&mut self, id: NodeId) -> &mut Node {
        std::sync::Arc::make_mut(self.nodes.get_mut(&id).expect("node exists"))
    }

    pub fn page_of(&self, id: NodeId) -> usize {
        let root = *self.ancestors(id).last().unwrap_or(&id);
        self.pages.iter().position(|p| p.children.contains(&root)).unwrap_or(0)
    }

    /// Parent chain, nearest first (excludes `id`).
    pub fn ancestors(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = vec![];
        let mut cur = self.nodes.get(&id).and_then(|n| n.parent);
        while let Some(p) = cur {
            out.push(p);
            cur = self.nodes[&p].parent;
        }
        out
    }

    pub fn is_ancestor(&self, anc: NodeId, id: NodeId) -> bool {
        self.ancestors(id).contains(&anc)
    }

    /// All descendants, pre-order.
    pub fn descendants(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = vec![];
        let mut stack: Vec<NodeId> = self.nodes[&id].children.iter().rev().copied().collect();
        while let Some(c) = stack.pop() {
            out.push(c);
            stack.extend(self.nodes[&c].children.iter().rev());
        }
        out
    }

    pub fn children_of(&self, page: usize, parent: Option<NodeId>) -> &Vec<NodeId> {
        match parent {
            Some(p) => &self.nodes[&p].children,
            None => &self.pages[page].children,
        }
    }

    fn children_of_mut(&mut self, page: usize, parent: Option<NodeId>) -> &mut Vec<NodeId> {
        match parent {
            Some(p) => &mut self.node_mut(p).children,
            None => &mut self.pages[page].children,
        }
    }

    /// Next default name, e.g. "Rectangle 3".
    pub fn auto_name(&self, label: &str) -> String {
        let n = self
            .nodes
            .values()
            .filter(|n| n.name.strip_prefix(label).is_some_and(|r| r.starts_with(' ')))
            .count();
        format!("{label} {}", n + 1)
    }

    /// Insert `node` under `parent` (None = page root). `index` None = on top.
    pub fn insert(&mut self, page: usize, parent: Option<NodeId>, index: Option<usize>, mut node: Node) -> NodeId {
        let id = self.next_id;
        self.next_id += 1;
        node.id = id;
        node.parent = parent;
        if node.name.is_empty() {
            node.name = self.auto_name(node.kind.label());
        }
        self.nodes.insert(id, std::sync::Arc::new(node));
        let list = self.children_of_mut(page, parent);
        let i = index.unwrap_or(list.len()).min(list.len());
        list.insert(i, id);
        id
    }

    /// Unlink `id` from its parent list. Returns (page, parent, index).
    fn detach(&mut self, id: NodeId) -> (usize, Option<NodeId>, usize) {
        let page = self.page_of(id);
        let parent = self.nodes[&id].parent;
        let list = self.children_of_mut(page, parent);
        let i = list.iter().position(|c| *c == id).unwrap_or(0);
        list.retain(|c| *c != id);
        (page, parent, i)
    }

    pub fn index_in_parent(&self, id: NodeId) -> usize {
        let page = self.page_of(id);
        let parent = self.nodes[&id].parent;
        self.children_of(page, parent).iter().position(|c| *c == id).unwrap_or(0)
    }

    pub fn remove(&mut self, id: NodeId) {
        if !self.nodes.contains_key(&id) {
            return;
        }
        self.detach(id);
        for d in self.descendants(id) {
            self.nodes.remove(&d);
        }
        self.nodes.remove(&id);
    }

    /// Move `id` under `parent` at `index` (index is computed after detaching).
    pub fn reparent(&mut self, id: NodeId, page: usize, parent: Option<NodeId>, index: Option<usize>) {
        if parent == Some(id) || parent.is_some_and(|p| self.is_ancestor(id, p)) {
            return;
        }
        self.detach(id);
        self.node_mut(id).parent = parent;
        let list = self.children_of_mut(page, parent);
        let i = index.unwrap_or(list.len()).min(list.len());
        list.insert(i, id);
    }

    pub fn translate(&mut self, id: NodeId, d: Vec2) {
        for n in std::iter::once(id).chain(self.descendants(id)) {
            let node = self.node_mut(n);
            node.rect = node.rect.translate(d);
        }
    }

    /// Rotate a subtree by `deg` (counter-clockwise) around `pivot`.
    /// Groups stay axis-aligned and re-hug their rotated children.
    pub fn rotate(&mut self, id: NodeId, deg: f32, pivot: Pos2) {
        for n in std::iter::once(id).chain(self.descendants(id)) {
            let node = self.node_mut(n);
            let c = rotate(node.rect.center(), pivot, deg);
            node.rect = Rect::from_center_size(c, node.rect.size());
            if node.kind != Kind::Group {
                node.rotation = normalize_deg(node.rotation + deg);
            }
        }
        self.reflow();
    }

    /// Resize a node. Groups and vectors scale their contents; frame children follow their constraints.
    pub fn set_rect(&mut self, id: NodeId, r: Rect) {
        self.set_rect_with(id, r, true);
    }

    /// `constraints: false` resizes a frame without moving its children (Cmd-drag in Figma).
    pub fn set_rect_with(&mut self, id: NodeId, r: Rect, constraints: bool) {
        let old = self.nodes[&id].rect;
        let sx = if old.width() > 0.0 { r.width() / old.width() } else { 1.0 };
        let sy = if old.height() > 0.0 { r.height() / old.height() } else { 1.0 };
        let map = |p: Pos2| pos2(r.min.x + (p.x - old.min.x) * sx, r.min.y + (p.y - old.min.y) * sy);
        let node = self.node_mut(id);
        node.rect = r;
        for pt in node.paths.iter_mut().flat_map(|sp| sp.points.iter_mut()) {
            pt.p = pos2(pt.p.x * sx, pt.p.y * sy);
            pt.h_in = vec2(pt.h_in.x * sx, pt.h_in.y * sy);
            pt.h_out = vec2(pt.h_out.x * sx, pt.h_out.y * sy);
        }
        let (kind, mode) = (node.kind, node.layout.mode);
        let kids = node.children.clone();
        if matches!(kind, Kind::Group | Kind::Boolean) {
            for c in kids {
                let cr = self.nodes[&c].rect;
                self.set_rect(c, Rect::from_two_pos(map(cr.min), map(cr.max)));
            }
        } else if kind.is_frame() && mode == LayoutMode::None && constraints {
            for c in kids {
                let (cr, [ch, cv]) = (self.nodes[&c].rect, self.nodes[&c].constraints);
                let axis = |k: Constraint, lo: f32, hi: f32, olo: f32, ohi: f32, nlo: f32, nhi: f32| -> (f32, f32) {
                    match k {
                        Constraint::Min => (lo + nlo - olo, hi + nlo - olo),
                        Constraint::Max => (lo + nhi - ohi, hi + nhi - ohi),
                        Constraint::Stretch => (lo + nlo - olo, hi + nhi - ohi),
                        Constraint::Center => {
                            let d = (nlo + nhi - olo - ohi) / 2.0;
                            (lo + d, hi + d)
                        }
                        Constraint::Scale => {
                            let s = if ohi > olo { (nhi - nlo) / (ohi - olo) } else { 1.0 };
                            (nlo + (lo - olo) * s, nlo + (hi - olo) * s)
                        }
                    }
                };
                let (x0, x1) = axis(ch, cr.min.x, cr.max.x, old.min.x, old.max.x, r.min.x, r.max.x);
                let (y0, y1) = axis(cv, cr.min.y, cr.max.y, old.min.y, old.max.y, r.min.y, r.max.y);
                let nr = Rect::from_min_max(pos2(x0, y0), pos2(x1.max(x0), y1.max(y0)));
                if (nr.size() - cr.size()).length() < 1e-4 {
                    self.translate(c, nr.min - cr.min);
                } else {
                    self.set_rect(c, nr);
                }
            }
        }
    }

    /// Where a comment pin sits in world space.
    pub fn comment_pos(&self, c: &Comment) -> Pos2 {
        match c.anchor.and_then(|a| self.nodes.get(&a)) {
            Some(n) => n.rect.min + c.at.to_vec2(),
            None => c.at,
        }
    }

    pub fn variable(&self, id: VarId) -> Option<&Variable> {
        self.variables.iter().find(|v| v.id == id)
    }

    pub fn add_collection(&mut self, name: String) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.collections.push(Collection { id, name, modes: vec!["Mode 1".into()] });
        id
    }

    pub fn add_variable(&mut self, collection: u64, name: String, value: VarValue) -> VarId {
        let id = self.next_id;
        self.next_id += 1;
        let modes = self.collections.iter().find(|c| c.id == collection).map_or(1, |c| c.modes.len());
        self.variables.push(Variable { id, name, collection, values: vec![value; modes] });
        id
    }

    /// The mode index a layer sees for a collection: nearest explicit choice up the tree, else the first.
    pub fn mode_for(&self, id: NodeId, collection: u64) -> usize {
        std::iter::once(id)
            .chain(self.ancestors(id))
            .find_map(|a| self.nodes[&a].modes.iter().find(|(c, _)| *c == collection).map(|(_, m)| *m))
            .unwrap_or(0)
    }

    /// Push variable values into bound properties.
    pub fn apply_variables(&mut self) {
        if self.variables.is_empty() {
            return;
        }
        // Resolve everything first, against an index (this runs on every edit, over every bound layer).
        let index: HashMap<VarId, &Variable> = self.variables.iter().map(|v| (v.id, v)).collect();
        let any_modes = self.nodes.values().any(|n| !n.modes.is_empty());
        let mode_for = |node: NodeId, collection: u64| -> usize {
            let mut cur = any_modes.then_some(node);
            while let Some(id) = cur {
                let Some(n) = self.nodes.get(&id) else { break };
                if let Some((_, m)) = n.modes.iter().find(|(c, _)| *c == collection) {
                    return *m;
                }
                cur = n.parent;
            }
            0
        };
        let resolve = |var: VarId, node: NodeId| -> Option<VarValue> {
            let mut cur = var;
            for _ in 0..8 {
                let v = index.get(&cur)?;
                let mode = mode_for(node, v.collection).min(v.values.len().saturating_sub(1));
                match v.values.get(mode)? {
                    VarValue::Alias(next) => cur = *next,
                    other => return Some(other.clone()),
                }
            }
            None
        };
        let mut writes = vec![];
        for n in self.nodes.values() {
            for &(prop, var) in &n.bindings {
                // Inside an instance, a property overridden by hand keeps the override; the
                // binding it copied from the main component no longer drives it.
                if let Some(sync) = &n.synced
                    && Self::bound_value_differs(n, sync, prop)
                {
                    continue;
                }
                if let Some(val) = resolve(var, n.id)
                    && !Self::binding_holds(n, prop, &val)
                {
                    writes.push((n.id, prop, val));
                }
            }
        }
        // Only layers whose values change are written (and so copied away from undo snapshots).
        for (id, prop, val) in writes {
            let n = self.node_mut(id);
            match (prop, val) {
                (Bindable::Fill, VarValue::Color(c)) => match n.fills.last_mut() {
                    Some(p) if p.kind == PaintKind::Solid => p.color = c,
                    _ => n.fills = vec![Paint::solid(c)],
                },
                (Bindable::Stroke, VarValue::Color(c)) => match n.strokes.last_mut() {
                    Some(p) if p.kind == PaintKind::Solid => p.color = c,
                    _ => n.strokes = vec![Paint::solid(c)],
                },
                (Bindable::Opacity, VarValue::Number(v)) => n.opacity = (v / 100.0).clamp(0.0, 1.0),
                (Bindable::Radius, VarValue::Number(v)) => n.radius = v.max(0.0),
                (Bindable::Width, VarValue::Number(v)) => n.rect = Rect::from_min_size(n.rect.min, vec2(v.max(0.0), n.rect.height())),
                (Bindable::Height, VarValue::Number(v)) => n.rect = Rect::from_min_size(n.rect.min, vec2(n.rect.width(), v.max(0.0))),
                (Bindable::Gap, VarValue::Number(v)) => n.layout.gap = v,
                (Bindable::Padding, VarValue::Number(v)) => n.layout.padding = [v; 4],
                (Bindable::StrokeWidth, VarValue::Number(v)) => n.stroke_width = v.max(0.0),
                (Bindable::FontSize, VarValue::Number(v)) => n.font_size = v.max(1.0),
                (Bindable::Text, VarValue::Text(t)) => n.text = t,
                (Bindable::Visible, VarValue::Bool(b)) => n.visible = b,
                _ => {}
            }
        }
    }

    /// Whether `prop` differs between two versions of a layer.
    fn bound_value_differs(a: &Node, b: &Node, prop: Bindable) -> bool {
        match prop {
            Bindable::Fill => a.fills != b.fills,
            Bindable::Stroke => a.strokes != b.strokes,
            Bindable::Opacity => a.opacity != b.opacity,
            Bindable::Radius => a.radius != b.radius,
            Bindable::Width => a.rect.width() != b.rect.width(),
            Bindable::Height => a.rect.height() != b.rect.height(),
            Bindable::Gap => a.layout.gap != b.layout.gap,
            Bindable::Padding => a.layout.padding != b.layout.padding,
            Bindable::StrokeWidth => a.stroke_width != b.stroke_width,
            Bindable::FontSize => a.font_size != b.font_size,
            Bindable::Text => a.text != b.text,
            Bindable::Visible => a.visible != b.visible,
        }
    }

    /// Whether a bound property already shows the variable's value.
    fn binding_holds(n: &Node, prop: Bindable, val: &VarValue) -> bool {
        match (prop, val) {
            (Bindable::Fill, VarValue::Color(c)) => n.fills.last().is_some_and(|p| p.kind == PaintKind::Solid && p.color == *c),
            (Bindable::Stroke, VarValue::Color(c)) => n.strokes.last().is_some_and(|p| p.kind == PaintKind::Solid && p.color == *c),
            (Bindable::Opacity, VarValue::Number(v)) => n.opacity == (v / 100.0).clamp(0.0, 1.0),
            (Bindable::Radius, VarValue::Number(v)) => n.radius == v.max(0.0),
            (Bindable::Width, VarValue::Number(v)) => n.rect.width() == v.max(0.0),
            (Bindable::Height, VarValue::Number(v)) => n.rect.height() == v.max(0.0),
            (Bindable::Gap, VarValue::Number(v)) => n.layout.gap == *v,
            (Bindable::Padding, VarValue::Number(v)) => n.layout.padding == [*v; 4],
            (Bindable::StrokeWidth, VarValue::Number(v)) => n.stroke_width == v.max(0.0),
            (Bindable::FontSize, VarValue::Number(v)) => n.font_size == v.max(1.0),
            (Bindable::Text, VarValue::Text(t)) => n.text == *t,
            (Bindable::Visible, VarValue::Bool(b)) => n.visible == *b,
            _ => true,
        }
    }

    pub fn style(&self, id: StyleId) -> Option<&Style> {
        self.styles.iter().find(|s| s.id == id)
    }

    /// Create a style from a value and return its id.
    pub fn add_style(&mut self, name: String, value: StyleValue) -> StyleId {
        let id = self.next_id;
        self.next_id += 1;
        self.styles.push(Style { id, name, value });
        id
    }

    /// Push style values into every linked layer; links to deleted styles fall away.
    pub fn apply_styles(&mut self) {
        if self.styles.is_empty() && self.nodes.values().all(|n| n.fill_style.is_none() && n.stroke_style.is_none() && n.text_style.is_none() && n.effect_style.is_none()) {
            return;
        }
        let styles: HashMap<StyleId, StyleValue> = self.styles.iter().map(|s| (s.id, s.value.clone())).collect();
        for arc in self.nodes.values_mut() {
            if arc.fill_style.is_none() && arc.stroke_style.is_none() && arc.text_style.is_none() && arc.effect_style.is_none() {
                continue;
            }
            // Work on a copy and only write back real changes, so layers stay shared with undo.
            let mut n = (**arc).clone();
            Self::apply_styles_to(&mut n, &styles);
            if n != **arc {
                *arc = std::sync::Arc::new(n);
            }
        }
    }

    fn apply_styles_to(n: &mut Node, styles: &HashMap<StyleId, StyleValue>) {
        // Inside an instance, a property overridden by hand keeps the override; a style the
        // main component's layer uses doesn't overwrite it.
        let (fill_over, stroke_over, text_over, effect_over) = match &n.synced {
            Some(s) => (
                s.fills != n.fills,
                s.strokes != n.strokes,
                (s.font_family != n.font_family || s.font_size != n.font_size || s.font_weight != n.font_weight || s.line_height != n.line_height || s.letter_spacing != n.letter_spacing),
                s.shadows != n.shadows,
            ),
            None => (false, false, false, false),
        };
        for (link, stroke) in [(&mut n.fill_style, false), (&mut n.stroke_style, true)] {
            match link.and_then(|id| styles.get(&id)) {
                Some(StyleValue::Paint(p)) => {
                    if stroke && !stroke_over {
                        n.strokes = p.clone()
                    } else if !stroke && !fill_over {
                        n.fills = p.clone()
                    }
                }
                Some(_) | None => *link = None,
            }
        }
        match n.text_style.and_then(|id| styles.get(&id)) {
            Some(StyleValue::Text(t)) => {
                if !text_over {
                    t.apply(n)
                }
            }
            _ => n.text_style = None,
        }
        match n.effect_style.and_then(|id| styles.get(&id)) {
            Some(StyleValue::Effect(e)) => {
                if !effect_over {
                    n.shadows = e.clone()
                }
            }
            _ => n.effect_style = None,
        }
    }

    /// Create an instance of `comp` with its top-left at `at`.
    pub fn instantiate(&mut self, comp: NodeId, page: usize, parent: Option<NodeId>, index: Option<usize>, at: Pos2) -> NodeId {
        let mut c = (*self.nodes[&comp]).clone();
        if let Some(set) = c.parent.map(|p| &self.nodes[&p]).filter(|p| p.kind == Kind::ComponentSet) {
            c.name = set.name.clone();
        }
        let mut n = Node { children: vec![], name: c.name.clone(), ..c.clone() };
        n.kind = Kind::Instance;
        n.component = Some(comp);
        n.source = Some(comp);
        n.synced = Some(Box::new(Node { children: vec![], ..c }));
        n.rect = Rect::from_min_size(at, n.rect.size());
        let id = self.insert(page, parent, index, n);
        self.sync_instance(id);
        self.reflow();
        id
    }

    /// Turn an instance back into a plain frame; nested instances stay instances.
    pub fn detach_instance(&mut self, inst: NodeId) {
        let n = self.node_mut(inst);
        n.kind = Kind::Frame;
        n.component = None;
        n.source = None;
        n.synced = None;
        for d in self.descendants(inst) {
            if self.ancestors(d).iter().take_while(|a| **a != inst).any(|a| self.nodes[a].kind == Kind::Instance) {
                continue;
            }
            let n = self.node_mut(d);
            n.source = None;
            n.synced = None;
        }
    }

    /// Drop all overrides on an instance.
    pub fn reset_instance(&mut self, inst: NodeId) {
        for id in std::iter::once(inst).chain(self.descendants(inst)) {
            if let Some(src) = self.nodes[&id].source.and_then(|s| self.nodes.get(&s)).cloned() {
                let n = self.node_mut(id);
                let keep = n.rect;
                *n = Node { id: n.id, parent: n.parent, children: n.children.clone(), kind: n.kind, component: n.component, source: n.source, synced: None, ..(*src).clone() };
                n.rect = keep;
                n.synced = Some(Box::new(Node { children: vec![], ..n.clone() }));
            }
        }
        if let Some(c) = self.nodes[&inst].component.and_then(|c| self.nodes.get(&c)) {
            let size = c.rect.size();
            let n = self.node_mut(inst);
            n.rect = Rect::from_min_size(n.rect.min, size);
        }
        self.sync_instance(inst);
        self.reflow();
    }

    /// Bring an instance in line with its main component, keeping overrides.
    pub fn sync_instance(&mut self, inst: NodeId) {
        let Some(comp) = self.nodes[&inst].component.filter(|c| self.nodes.contains_key(c)) else { return };
        if comp == inst || self.is_ancestor(inst, comp) {
            return;
        }
        // An instance nested inside another instance is a copy its outer instance keeps in step;
        // it only syncs from its own component once swapped to a different one.
        let outer = self.nodes[&inst].source.filter(|s| *s != comp).and_then(|s| self.nodes.get(&s)).filter(|s| s.kind == Kind::Instance).map(|s| s.component);
        if outer == Some(Some(comp)) {
            self.apply_instance_props(inst);
            return;
        }
        let nested = outer.is_some();
        let page = self.page_of(inst);
        let mut c = (*self.nodes[&comp]).clone();
        // Instances of a variant are named after the component set, as in Figma.
        if let Some(set) = c.parent.map(|p| &self.nodes[&p]).filter(|p| p.kind == Kind::ComponentSet) {
            c.name = set.name.clone();
        }
        // Nested instances that were swapped own their subtree; leave it to their own sync.
        let swapped: std::collections::HashSet<NodeId> = self
            .descendants(inst)
            .into_iter()
            .filter(|&d| {
                let n = &self.nodes[&d];
                n.kind == Kind::Instance && n.source.and_then(|s| self.nodes.get(&s)).is_some_and(|s| s.kind == Kind::Instance && s.component != n.component)
            })
            .collect();
        let under_swapped = |doc: &Document, d: NodeId| doc.ancestors(d).iter().take_while(|a| **a != inst).any(|a| swapped.contains(a));
        // Instance layers by the component layer they mirror.
        let mut by_source: HashMap<NodeId, NodeId> = HashMap::new();
        for d in self.descendants(inst) {
            if let Some(s) = self.nodes[&d].source
                && !under_swapped(self, d)
            {
                by_source.entry(s).or_insert(d);
            }
        }
        // Root: merge appearance, keep placement; size follows the main unless resized.
        // A swapped nested instance keeps its own place, size, and visibility (its outer instance sets them).
        let target_size = {
            let n = &self.nodes[&inst];
            let snap = n.synced.as_deref();
            if !nested && snap.is_none_or(|s| s.rect.size() == n.rect.size()) { c.rect.size() } else { n.rect.size() }
        };
        let actual = Rect::from_min_size(self.nodes[&inst].rect.min, target_size);
        if !nested {
            merge_from(self.node_mut(inst), &c);
        }
        self.node_mut(inst).rect = Rect::from_min_size(actual.min, c.rect.size());
        let offset = actual.min - c.rect.min;
        // Children, in component order.
        let mut keep = std::collections::HashSet::new();
        let mut order: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        for cid in self.descendants(comp) {
            let src = self.nodes[&cid].clone();
            let parent = if src.parent == Some(comp) {
                inst
            } else {
                // Layers inside a swapped nested instance belong to it.
                match src.parent.and_then(|p| by_source.get(&p)).copied() {
                    Some(p) if !swapped.contains(&p) => p,
                    _ => continue,
                }
            };
            let id = match by_source.get(&cid) {
                Some(&e) => {
                    merge_from(self.node_mut(e), &src);
                    // Geometry isn't an override inside instances (as in Figma): the resize below
                    // rescales it from the component's every time, so it mustn't compound.
                    if src.kind == Kind::Vector {
                        self.node_mut(e).paths = src.paths.clone();
                    }
                    if self.nodes[&e].parent != Some(parent) {
                        self.reparent(e, page, Some(parent), None);
                    }
                    e
                }
                None => {
                    let mut n = Node { children: vec![], ..(*src).clone() };
                    n.source = Some(cid);
                    n.synced = Some(Box::new(Node { children: vec![], ..(*src).clone() }));
                    // Nested instances keep pointing at their own component.
                    let id = self.insert(page, Some(parent), None, n);
                    by_source.insert(cid, id);
                    id
                }
            };
            let text = self.nodes[&id].kind == Kind::Text;
            if swapped.contains(&id) {
                // A swapped nested instance owns its layers: move and size it as a whole.
                let (cur, target) = (self.nodes[&id].rect, Rect::from_min_size(src.rect.min + offset, src.rect.size()));
                self.translate(id, target.min - cur.min);
                if target.size() != cur.size() {
                    self.set_rect(id, target);
                }
            } else {
                let n = self.node_mut(id);
                n.rect = if text {
                    Rect::from_min_size(src.rect.min + offset, n.rect.size())
                } else {
                    Rect::from_min_size(src.rect.min + offset, src.rect.size())
                };
            }
            keep.insert(id);
            order.entry(parent).or_default().push(id);
        }
        for d in self.descendants(inst) {
            if !keep.contains(&d) && self.nodes.get(&d).is_some_and(|n| n.source.is_some()) && !under_swapped(self, d) {
                self.remove(d);
            }
        }
        for (parent, kids) in order {
            let extra: Vec<NodeId> = self.nodes[&parent].children.iter().copied().filter(|k| !kids.contains(k)).collect();
            self.node_mut(parent).children = kids.into_iter().chain(extra).collect();
        }
        // Resized instances re-apply constraints from the component's size.
        if actual.size() != c.rect.size() {
            self.set_rect(inst, actual);
        }
        self.apply_instance_props(inst);
    }

    /// A component's revision: bumps whenever its subtree's content changes.
    fn comp_rev(&self, comp: NodeId, revs: &mut HashMap<NodeId, u64>) -> u64 {
        if let Some(r) = revs.get(&comp) {
            return *r;
        }
        let ids: Vec<NodeId> = std::iter::once(comp).chain(self.descendants(comp)).collect();
        let cache = self.cache.0.clone();
        let mut st = cache.lock().unwrap();
        let same = st.comps.get(&comp).is_some_and(|(_, snap)| snap.len() == ids.len() && snap.iter().zip(&ids).all(|(a, id)| *a == *self.nodes[id]));
        let rev = if same {
            st.comps[&comp].0
        } else {
            st.next_rev += 1;
            let rev = st.next_rev;
            st.comps.insert(comp, (rev, ids.iter().map(|id| (*self.nodes[id]).clone()).collect()));
            rev
        };
        revs.insert(comp, rev);
        rev
    }

    fn inst_key(&self, inst: NodeId, revs: &mut HashMap<NodeId, u64>) -> Option<InstKey> {
        let n = &self.nodes[&inst];
        let comp = n.component.filter(|c| self.nodes.contains_key(c))?;
        let (size, props) = ((n.rect.width().to_bits(), n.rect.height().to_bits()), n.prop_values.clone());
        Some(InstKey { comp, rev: self.comp_rev(comp, revs), size, props })
    }

    fn instance_stale(&self, inst: NodeId, revs: &mut HashMap<NodeId, u64>) -> bool {
        let Some(key) = self.inst_key(inst, revs) else { return false };
        self.cache.0.lock().unwrap().insts.get(&inst) != Some(&key)
    }

    fn mark_synced(&self, inst: NodeId, revs: &mut HashMap<NodeId, u64>) {
        if let Some(key) = self.inst_key(inst, revs) {
            self.cache.0.lock().unwrap().insts.insert(inst, key);
        }
    }

    /// Property definitions for a component: its own, or its component set's.
    pub fn props_of(&self, comp: NodeId) -> Vec<CompProp> {
        let c = &self.nodes[&comp];
        match c.parent.map(|p| &self.nodes[&p]) {
            Some(set) if set.kind == Kind::ComponentSet => set.comp_props.clone(),
            _ => c.comp_props.clone(),
        }
    }

    /// Drive bound layers inside an instance from its property values.
    fn apply_instance_props(&mut self, inst: NodeId) {
        let Some(comp) = self.nodes[&inst].component.filter(|c| self.nodes.contains_key(c)) else { return };
        let props = self.props_of(comp);
        if props.is_empty() {
            return;
        }
        let values = self.nodes[&inst].prop_values.clone();
        let value = |name: &str| values.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone()).or_else(|| props.iter().find(|p| p.name == name).map(|p| p.default.clone()));
        // Only this instance's own layers: ones inside a nested instance follow that instance's properties.
        let own: Vec<NodeId> = self.descendants(inst).into_iter().filter(|&d| !self.ancestors(d).iter().take_while(|a| **a != inst).any(|a| self.nodes[a].kind == Kind::Instance)).collect();
        for d in own {
            for (name, target) in self.nodes[&d].prop_bind.clone() {
                let n = self.node_mut(d);
                match (target, value(&name)) {
                    (PropTarget::Visible, Some(PropValue::Bool(b))) => n.visible = b,
                    (PropTarget::Text, Some(PropValue::Text(t))) => n.text = t,
                    (PropTarget::Swap, Some(PropValue::Swap(c))) => n.component = Some(c),
                    _ => {}
                }
            }
        }
    }

    /// Switch an instance to another main component, carrying overrides over by layer name.
    pub fn swap_component(&mut self, inst: NodeId, comp: NodeId) {
        if self.nodes[&inst].component == Some(comp) || !self.nodes.contains_key(&comp) {
            return;
        }
        let path = |doc: &Document, id: NodeId| {
            let mut parts = vec![doc.nodes[&id].name.clone()];
            for a in doc.ancestors(id) {
                if a == inst {
                    break;
                }
                parts.push(doc.nodes[&a].name.clone());
            }
            parts.reverse();
            parts.join("/")
        };
        // Remember each overridden layer (differs from what it last copied).
        let mut overrides: HashMap<String, Node> = HashMap::new();
        for d in self.descendants(inst) {
            let n = &self.nodes[&d];
            if let Some(snap) = &n.synced
                && (n.fills != snap.fills || n.text != snap.text || n.visible != snap.visible || n.strokes != snap.strokes || n.opacity != snap.opacity) {
                    overrides.insert(path(self, d), (**n).clone());
                }
        }
        for c in self.nodes[&inst].children.clone() {
            self.remove(c);
        }
        // Keep the root's last snapshot: anything not overridden then takes the new variant's value.
        // An instance nested in another keeps its link (and its place and size) in the outer one.
        let nested = self.nodes[&inst].source.and_then(|s| self.nodes.get(&s)).is_some_and(|s| s.kind == Kind::Instance);
        let n = self.node_mut(inst);
        n.component = Some(comp);
        if !nested {
            n.source = Some(comp);
            let size = self.nodes[&comp].rect.size();
            let n = self.node_mut(inst);
            n.rect = Rect::from_min_size(n.rect.min, size);
        }
        self.sync_instance(inst);
        for d in self.descendants(inst) {
            if let Some(o) = overrides.get(&path(self, d)) {
                let n = self.node_mut(d);
                let snap = n.synced.as_deref().cloned();
                if let Some(snap) = snap {
                    if o.fills != snap.fills { n.fills = o.fills.clone() }
                    if o.text != snap.text { n.text = o.text.clone() }
                    if o.visible != snap.visible { n.visible = o.visible }
                    if o.strokes != snap.strokes { n.strokes = o.strokes.clone() }
                    if o.opacity != snap.opacity { n.opacity = o.opacity }
                }
            }
        }
    }

    /// Variant properties of a component set: each property with its values in order.
    pub fn variant_props(&self, set: NodeId) -> Vec<(String, Vec<String>)> {
        let mut out: Vec<(String, Vec<String>)> = vec![];
        for &c in &self.nodes[&set].children {
            for (k, v) in variant_pairs(&self.nodes[&c].name) {
                match out.iter_mut().find(|(n, _)| *n == k) {
                    Some((_, vals)) if !vals.contains(&v) => vals.push(v),
                    Some(_) => {}
                    None => out.push((k, vec![v])),
                }
            }
        }
        out
    }

    /// The variant in `set` matching `want` (property -> value), if any.
    pub fn find_variant(&self, set: NodeId, want: &[(String, String)]) -> Option<NodeId> {
        self.nodes[&set].children.iter().copied().find(|&c| {
            let pairs = variant_pairs(&self.nodes[&c].name);
            want.iter().all(|(k, v)| pairs.iter().any(|(pk, pv)| pk == k && pv == v))
        })
    }

    /// Put components into a new component set, laid out in a row, naming plain ones as variants.
    pub fn combine_as_variants(&mut self, comps: &[NodeId]) -> Option<NodeId> {
        let comps = self.sort_by_z(comps);
        if comps.len() < 2 || comps.iter().any(|c| self.nodes[c].kind != Kind::Component) {
            return None;
        }
        let page = self.page_of(comps[0]);
        let base = self.nodes[&comps[0]].name.split(',').next().unwrap_or("Component").split('=').next().unwrap_or("Component").trim().to_string();
        let b = self.bounds(&comps);
        let mut set = Node::new(Kind::ComponentSet, b.expand(20.0));
        set.fills.clear();
        set.clip = false;
        set.name = base.clone();
        let set_id = self.insert(page, None, None, set);
        let mut x = b.min.x;
        for (i, &c) in comps.iter().enumerate() {
            self.reparent(c, page, Some(set_id), None);
            let r = self.nodes[&c].rect;
            self.translate(c, pos2(x, b.min.y) - r.min);
            x += r.width() + 20.0;
            let n = self.node_mut(c);
            if variant_pairs(&n.name).is_empty() {
                n.name = format!("Property 1={}", if i == 0 { "Default".to_string() } else { format!("Variant{}", i + 1) });
            }
        }
        let r = self.bounds(&comps).expand(20.0);
        self.node_mut(set_id).rect = r;
        Some(set_id)
    }

    /// Re-derive everything computed from children: group bounds and auto layout.
    pub fn reflow(&mut self) {
        // Layout passes write every layer they visit; layers that end up unchanged go back to
        // sharing their old copy, so undo snapshots stay small.
        let before = self.nodes.clone();
        self.reflow_passes();
        for (id, n) in self.nodes.iter_mut() {
            if let Some(old) = before.get(id)
                && !std::sync::Arc::ptr_eq(n, old)
                && **n == **old
            {
                *n = old.clone();
            }
        }
    }

    fn reflow_passes(&mut self) {
        let mut t = std::time::Instant::now();
        self.apply_styles();
        crate::plugins::lap("reflow: styles", &mut t);
        self.apply_variables();
        crate::plugins::lap("reflow: variables", &mut t);
        let instances: Vec<NodeId> = self.nodes.values().filter(|n| n.kind == Kind::Instance).map(|n| n.id).collect();
        for _ in 0..2 {
            let mut revs: HashMap<NodeId, u64> = HashMap::new();
            for &i in &instances {
                if self.nodes.contains_key(&i) && self.instance_stale(i, &mut revs) {
                    self.sync_instance(i);
                    self.mark_synced(i, &mut revs);
                }
            }
        }
        crate::plugins::lap("reflow: instances", &mut t);
        // Instances copied their bindings; resolve them too, before layout uses sizes and gaps.
        self.apply_variables();
        crate::plugins::lap("reflow: variables again", &mut t);
        self.reflow_layout();
        crate::plugins::lap("reflow: layout", &mut t);
    }

    fn reflow_layout(&mut self) {
        self.fit_component_sets();
        self.fit_groups_only();
        let mut frames: Vec<(usize, NodeId)> = self
            .nodes
            .values()
            .filter(|n| n.kind.is_frame() && n.layout.mode != LayoutMode::None)
            .map(|n| (self.ancestors(n.id).len(), n.id))
            .collect();
        if frames.is_empty() {
            return;
        }
        // Deepest first so hugging frames know their content, then top-down so Fill sizes flow into children.
        frames.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        for &(_, f) in &frames {
            self.layout_frame(f);
        }
        self.fit_groups_only();
        for &(_, f) in frames.iter().rev() {
            self.layout_frame(f);
        }
        self.fit_groups_only();
    }

    fn layout_frame(&mut self, f: NodeId) {
        let n = self.nodes[&f].clone();
        let l = n.layout;
        let h = l.mode == LayoutMode::Horizontal;
        let (ma, ca) = if h { (0, 1) } else { (1, 0) };
        let main = |v: Vec2| if h { v.x } else { v.y };
        let cross = |v: Vec2| if h { v.y } else { v.x };
        let along = |m: f32, c: f32| if h { vec2(m, c) } else { vec2(c, m) };
        let inset = if l.strokes_included { stroke_reach(&n).0 } else { 0.0 };
        let [pt, pr, pb, pl] = l.padding.map(|p| p + inset);
        let (p0, p1, q0, q1) = if h { (pl, pr, pt, pb) } else { (pt, pb, pl, pr) };
        // A child's layout box: its bounds, plus its outer stroke when strokes are included.
        let out = |k: &Node| if l.strokes_included { stroke_reach(k).1 } else { 0.0 };
        let lsize = |k: &Node| k.aabb().size() + Vec2::splat(2.0 * out(k));
        let kids: Vec<NodeId> = n
            .children
            .iter()
            .copied()
            .filter(|c| self.nodes[c].visible && !self.nodes[c].absolute)
            .collect();
        if l.mode == LayoutMode::Grid {
            return self.layout_grid(f, &n, &kids, [pt, pr, pb, pl], &out);
        }
        if l.wrap && h {
            return self.layout_wrap(f, &n, &kids, [pt, pr, pb, pl], &out);
        }
        // Auto-width text can only hug; fixed or auto-height text can fill (and then wraps).
        let fills = |k: &Node, axis: usize| k.sizing[axis] == Sizing::Fill && (k.kind != Kind::Text || k.text_sizing != TextSizing::AutoWidth);
        let count = kids.len() as f32;
        let gaps = l.gap * (count - 1.0).max(0.0);
        let fixed_main: f32 = kids.iter().map(|c| &self.nodes[c]).filter(|k| !fills(k, ma)).map(|k| main(lsize(k))).sum();
        let n_fill = kids.iter().filter(|c| fills(&self.nodes[c], ma)).count() as f32;

        let mut size = n.rect.size();
        if n.sizing[ma] == Sizing::Hug {
            let fill_now: f32 = kids.iter().map(|c| &self.nodes[c]).filter(|k| fills(k, ma)).map(|k| main(lsize(k))).sum();
            let m = p0 + p1 + fixed_main + fill_now + if l.space_between { 0.0 } else { gaps };
            size = along(m, cross(size));
        }
        // Baseline alignment: each layer drops by how far its baseline sits above the lowest one.
        let bases: Vec<f32> = if h && l.cross == AxisAlign::Baseline {
            let b: Vec<f32> = kids.iter().map(|c| &self.nodes[c]).map(|k| if fills(k, ca) { f32::NAN } else { self.baseline_of(k) + out(k) }).collect();
            let top = b.iter().copied().filter(|x| !x.is_nan()).fold(0.0, f32::max);
            b.into_iter().map(|x| if x.is_nan() { 0.0 } else { top - x }).collect()
        } else {
            vec![0.0; kids.len()]
        };
        if n.sizing[ca] == Sizing::Hug {
            let c = kids.iter().zip(&bases).map(|(c, o)| (&self.nodes[c], o)).filter(|(k, _)| !fills(k, ca)).map(|(k, o)| o + cross(lsize(k))).fold(0.0, f32::max);
            size = along(main(size), q0 + q1 + c);
        }
        for axis in 0..2 {
            if n.sizing[axis] == Sizing::Hug {
                size[axis] = clamp_size(&n, axis, size[axis]);
            }
        }
        if size != n.rect.size() {
            self.node_mut(f).rect = Rect::from_min_size(n.rect.min, size);
        }
        let inner_main = (main(size) - p0 - p1).max(0.0);
        let inner_cross = (cross(size) - q0 - q1).max(0.0);
        let free = inner_main - fixed_main - gaps;
        let fill_main = if n_fill > 0.0 { (free / n_fill).max(0.0) } else { 0.0 };
        let gap = if l.space_between && n_fill == 0.0 && count > 1.0 { (inner_main - fixed_main) / (count - 1.0) } else { l.gap };
        let lead = if n_fill > 0.0 || l.space_between {
            0.0
        } else {
            match l.main {
                AxisAlign::Min | AxisAlign::Baseline => 0.0,
                AxisAlign::Center => free / 2.0,
                AxisAlign::Max => free,
            }
        };
        let origin = n.rect.min;
        let mut cursor = main(origin.to_vec2()) + p0 + lead;
        for (i, c) in kids.into_iter().enumerate() {
            let k = self.nodes[&c].clone();
            let o = out(&k);
            let mut ks = lsize(&k);
            // ponytail: a fill layer stopped by min/max doesn't hand its slack to the others.
            if fills(&k, ma) {
                ks = along(clamp_size(&k, ma, fill_main), cross(ks));
            }
            if fills(&k, ca) {
                ks = along(main(ks), clamp_size(&k, ca, inner_cross));
            }
            if (ks - lsize(&k)).length() > 1e-4 && k.rotation == 0.0 {
                self.set_rect(c, Rect::from_min_size(k.rect.min, ks - Vec2::splat(2.0 * o)));
            }
            let off = match l.cross {
                AxisAlign::Min => 0.0,
                AxisAlign::Baseline => bases[i],
                AxisAlign::Center => (inner_cross - cross(ks)) / 2.0,
                AxisAlign::Max => inner_cross - cross(ks),
            };
            let target = along(cursor, cross(origin.to_vec2()) + q0 + off).to_pos2() + Vec2::splat(o);
            let cur = self.nodes[&c].aabb().min;
            if cur != target {
                self.translate(c, target - cur);
            }
            cursor += main(ks) + gap;
        }
    }

    /// Horizontal wrap: rows break at the frame's width; each row aligns on its own.
    fn layout_wrap(&mut self, f: NodeId, n: &Node, kids: &[NodeId], padding: [f32; 4], out: &dyn Fn(&Node) -> f32) {
        let l = n.layout;
        let [pt, pr, pb, pl] = padding;
        let inner_w = (n.rect.width() - pl - pr).max(0.0);
        let mut rows: Vec<Vec<(NodeId, Vec2)>> = vec![vec![]];
        let mut used = 0.0;
        for &c in kids {
            let k = &self.nodes[&c];
            let s = k.aabb().size() + Vec2::splat(2.0 * out(k));
            if !rows.last().unwrap().is_empty() && used + l.gap + s.x > inner_w + 1e-3 {
                rows.push(vec![]);
                used = 0.0;
            }
            let row = rows.last_mut().unwrap();
            used += if row.is_empty() { s.x } else { l.gap + s.x };
            row.push((c, s));
        }
        // Baseline rows: each layer's drop below the row top; the row is as tall as its lowest bottom.
        let baseline = l.cross == AxisAlign::Baseline;
        let drops: Vec<Vec<f32>> = rows
            .iter()
            .map(|r| {
                let b: Vec<f32> = r.iter().map(|(c, _)| if baseline { self.baseline_of(&self.nodes[c]) + out(&self.nodes[c]) } else { 0.0 }).collect();
                let top = b.iter().copied().fold(0.0, f32::max);
                b.into_iter().map(|x| top - x).collect()
            })
            .collect();
        let heights: Vec<f32> = rows.iter().zip(&drops).map(|(r, d)| r.iter().zip(d).map(|((_, s), o)| o + s.y).fold(0.0, f32::max)).collect();
        if n.sizing[1] == Sizing::Hug {
            let content = heights.iter().sum::<f32>() + l.cross_gap * (rows.len() as f32 - 1.0).max(0.0);
            let h = clamp_size(n, 1, pt + pb + content);
            self.node_mut(f).rect = Rect::from_min_size(n.rect.min, vec2(n.rect.width(), h));
        }
        let mut y = n.rect.min.y + pt;
        for ((row, &rh), drop) in rows.iter().zip(&heights).zip(&drops) {
            let widths: f32 = row.iter().map(|(_, s)| s.x).sum();
            let count = row.len() as f32;
            let free = inner_w - widths - l.gap * (count - 1.0).max(0.0);
            let (lead, gap) = if l.space_between && count > 1.0 {
                (0.0, (inner_w - widths) / (count - 1.0))
            } else {
                let lead = match l.main {
                    AxisAlign::Min | AxisAlign::Baseline => 0.0,
                    AxisAlign::Center => free / 2.0,
                    AxisAlign::Max => free,
                };
                (lead, l.gap)
            };
            let mut x = n.rect.min.x + pl + lead;
            for (&(c, s), d) in row.iter().zip(drop) {
                let off = match l.cross {
                    AxisAlign::Min => 0.0,
                    AxisAlign::Baseline => *d,
                    AxisAlign::Center => (rh - s.y) / 2.0,
                    AxisAlign::Max => rh - s.y,
                };
                let cur = self.nodes[&c].aabb().min;
                let target = pos2(x, y + off) + Vec2::splat(out(&self.nodes[&c]));
                if cur != target {
                    self.translate(c, target - cur);
                }
                x += s.x + gap;
            }
            y += rh + l.cross_gap;
        }
    }

    /// Distance from a layer's top to its first text baseline: text uses its first line, auto
    /// layout frames their first child's, anything else its bottom edge (as Figma does).
    fn baseline_of(&self, k: &Node) -> f32 {
        if k.kind == Kind::Text && k.rotation == 0.0 {
            let rows = if k.lines.is_empty() { k.text.matches('\n').count() + 1 } else { k.lines.len() };
            return crate::text::draw_offset(k, rows).y + k.font_size * crate::text::ASCENT;
        }
        if k.layout.mode != LayoutMode::None
            && k.rotation == 0.0
            && let Some(c) = k.children.iter().map(|c| &self.nodes[c]).find(|c| c.visible && !c.absolute)
        {
            return c.aabb().min.y - k.rect.min.y + self.baseline_of(c);
        }
        k.aabb().height()
    }

    /// Grid auto layout: children fill cells row by row, each spanning `grid_span` cells (CSS grid's
    /// sparse auto-placement); rows are added when the children outnumber the cells.
    fn layout_grid(&mut self, f: NodeId, n: &Node, kids: &[NodeId], [pt, pr, pb, pl]: [f32; 4], out: &dyn Fn(&Node) -> f32) {
        let l = n.layout;
        let cols = l.grid[0].max(1) as usize;
        let gaps = [l.gap, l.cross_gap];
        let mut taken: Vec<Vec<bool>> = vec![];
        let mut placed: Vec<(NodeId, [usize; 2], [usize; 2])> = vec![];
        let mut cursor = 0;
        for &c in kids {
            let span = self.nodes[&c].grid_span;
            let (cs, rs) = ((span[0].max(1) as usize).min(cols), span[1].max(1) as usize);
            let mut i = cursor;
            loop {
                let (r, col) = (i / cols, i % cols);
                if taken.len() < r + rs {
                    taken.resize(r + rs, vec![false; cols]);
                }
                if col + cs <= cols && (r..r + rs).all(|y| (col..col + cs).all(|x| !taken[y][x])) {
                    for row in &mut taken[r..r + rs] {
                        row[col..col + cs].fill(true);
                    }
                    placed.push((c, [col, r], [cs, rs]));
                    cursor = i + cs;
                    break;
                }
                i += 1;
            }
        }
        let counts = [cols, (l.grid[1] as usize).max(taken.len()).max(1)];
        let pad = [pl + pr, pt + pb];
        let fills = |k: &Node, axis: usize| k.sizing[axis] == Sizing::Fill && (k.kind != Kind::Text || k.text_sizing != TextSizing::AutoWidth);
        let lsize = |k: &Node| k.aabb().size() + Vec2::splat(2.0 * out(k));
        let mut size = n.rect.size();
        let mut track = [0.0; 2];
        for a in 0..2 {
            let gap_sum = gaps[a] * (counts[a] as f32 - 1.0);
            track[a] = match l.tracks[a] {
                Some(t) => t,
                // Hugging Fill tracks are as big as the biggest single-cell layer.
                None if n.sizing[a] == Sizing::Hug => placed
                    .iter()
                    .filter(|(c, _, s)| s[a] == 1 && !fills(&self.nodes[c], a))
                    .map(|(c, _, _)| lsize(&self.nodes[c])[a])
                    .fold(0.0, f32::max),
                None => ((size[a] - pad[a] - gap_sum) / counts[a] as f32).max(0.0),
            };
            if n.sizing[a] == Sizing::Hug {
                size[a] = clamp_size(n, a, pad[a] + track[a] * counts[a] as f32 + gap_sum);
            }
        }
        if size != n.rect.size() {
            self.node_mut(f).rect = Rect::from_min_size(n.rect.min, size);
        }
        let origin = n.rect.min + vec2(pl, pt);
        let place = |a: AxisAlign, free: f32| match a {
            AxisAlign::Min | AxisAlign::Baseline => 0.0,
            AxisAlign::Center => free / 2.0,
            AxisAlign::Max => free,
        };
        for (c, at, span) in placed {
            let cell_min = origin + vec2(at[0] as f32 * (track[0] + gaps[0]), at[1] as f32 * (track[1] + gaps[1]));
            let cell = vec2(span[0] as f32 * track[0] + (span[0] - 1) as f32 * gaps[0], span[1] as f32 * track[1] + (span[1] - 1) as f32 * gaps[1]);
            let k = self.nodes[&c].clone();
            let o = out(&k);
            let mut ks = lsize(&k);
            for a in 0..2 {
                if fills(&k, a) {
                    ks[a] = clamp_size(&k, a, cell[a]);
                }
            }
            if (ks - lsize(&k)).length() > 1e-4 && k.rotation == 0.0 {
                self.set_rect(c, Rect::from_min_size(k.rect.min, ks - Vec2::splat(2.0 * o)));
            }
            let target = cell_min + vec2(place(l.main, cell.x - ks.x), place(l.cross, cell.y - ks.y)) + Vec2::splat(o);
            let cur = self.nodes[&c].aabb().min;
            if cur != target {
                self.translate(c, target - cur);
            }
        }
    }

    /// Component sets grow to keep their variants inside (with padding), like Figma; they never
    /// shrink on their own, so a set made roomier by hand stays that way.
    fn fit_component_sets(&mut self) {
        let sets: Vec<NodeId> = self.nodes.values().filter(|n| n.kind == Kind::ComponentSet && n.layout.mode == LayoutMode::None && !n.children.is_empty()).map(|n| n.id).collect();
        for s in sets {
            let kids = self.nodes[&s].children.clone();
            let b = kids.iter().fold(Rect::NOTHING, |r, c| r.union(self.nodes[c].aabb())).expand(20.0);
            let r = self.nodes[&s].rect;
            if !r.contains_rect(b) {
                self.node_mut(s).rect = r.union(b);
            }
        }
    }

    /// Groups always hug their children; empty groups disappear.
    fn fit_groups_only(&mut self) {
        let mut groups: Vec<(usize, NodeId)> = self
            .nodes
            .values()
            .filter(|n| matches!(n.kind, Kind::Group | Kind::Boolean))
            .map(|n| (self.ancestors(n.id).len(), n.id))
            .collect();
        groups.sort_by(|a, b| b.0.cmp(&a.0)); // deepest first
        for (_, g) in groups {
            let kids = self.nodes[&g].children.clone();
            if kids.is_empty() {
                self.remove(g);
                continue;
            }
            let r = if self.nodes[&g].kind == Kind::Boolean {
                let paths = crate::boolean::result(self, g);
                let b = paths.iter().flat_map(|sp| sp.points.iter()).fold(Rect::NOTHING, |r, pt| r.union(Rect::from_min_size(pt.p, Vec2::ZERO)));
                if b.is_finite() { b } else { kids.iter().fold(Rect::NOTHING, |r, c| r.union(self.nodes[c].aabb())) }
            } else {
                kids.iter().fold(Rect::NOTHING, |r, c| r.union(self.nodes[c].aabb()))
            };
            self.node_mut(g).rect = r;
        }
    }

    /// Siblings sorted bottom-to-top, as they appear in their parent list.
    pub fn sort_by_z(&self, ids: &[NodeId]) -> Vec<NodeId> {
        let mut v: Vec<(Vec<usize>, NodeId)> = ids.iter().map(|&id| (self.z_path(id), id)).collect();
        v.sort();
        v.into_iter().map(|(_, id)| id).collect()
    }

    /// Path of indices from the page root to `id`; sorts in paint order.
    pub fn z_path(&self, id: NodeId) -> Vec<usize> {
        let mut path: Vec<usize> = std::iter::once(id)
            .chain(self.ancestors(id))
            .map(|n| self.index_in_parent(n))
            .collect();
        path.reverse();
        path
    }

    pub fn group(&mut self, ids: &[NodeId]) -> Option<NodeId> {
        let ids = self.sort_by_z(ids);
        let top = *ids.last()?;
        let page = self.page_of(top);
        let parent = self.nodes[&top].parent;
        let index = self.index_in_parent(top);
        let g = self.insert(page, parent, Some(index + 1), Node::new(Kind::Group, Rect::NOTHING));
        for id in ids {
            self.reparent(id, page, Some(g), None);
        }
        self.reflow();
        Some(g)
    }

    /// Mirror a subtree across a vertical (`horizontal = true`) or horizontal line at `axis`.
    pub fn flip(&mut self, id: NodeId, horizontal: bool, axis: f32) {
        for n in std::iter::once(id).chain(self.descendants(id)) {
            let node = self.node_mut(n);
            let r = node.rect;
            node.rect = if horizontal {
                Rect::from_min_size(pos2(2.0 * axis - r.max.x, r.min.y), r.size())
            } else {
                Rect::from_min_size(pos2(r.min.x, 2.0 * axis - r.max.y), r.size())
            };
            node.rotation = normalize_deg(-node.rotation);
            let (w, h) = (r.width(), r.height());
            for pt in node.paths.iter_mut().flat_map(|sp| sp.points.iter_mut()) {
                if horizontal {
                    pt.p.x = w - pt.p.x;
                    pt.h_in.x = -pt.h_in.x;
                    pt.h_out.x = -pt.h_out.x;
                } else {
                    pt.p.y = h - pt.p.y;
                    pt.h_in.y = -pt.h_in.y;
                    pt.h_out.y = -pt.h_out.y;
                }
            }
            for p in node.fills.iter_mut().chain(node.strokes.iter_mut()) {
                if horizontal {
                    (p.from.x, p.to.x) = (1.0 - p.from.x, 1.0 - p.to.x);
                } else {
                    (p.from.y, p.to.y) = (1.0 - p.from.y, 1.0 - p.to.y);
                }
            }
        }
    }

    /// Even out the gaps between layers along one axis, keeping the outermost two in place.
    pub fn distribute(&mut self, ids: &[NodeId], horizontal: bool) {
        if ids.len() < 3 {
            return;
        }
        let key = |r: Rect| if horizontal { r.min.x } else { r.min.y };
        let size = |r: Rect| if horizontal { r.width() } else { r.height() };
        let mut items: Vec<(NodeId, Rect)> = ids.iter().map(|&i| (i, self.nodes[&i].aabb())).collect();
        items.sort_by(|a, b| key(a.1).total_cmp(&key(b.1)));
        let start = key(items[0].1);
        let end = items.iter().map(|(_, r)| key(*r) + size(*r)).fold(f32::MIN, f32::max);
        let total: f32 = items.iter().map(|(_, r)| size(*r)).sum();
        let gap = (end - start - total) / (items.len() - 1) as f32;
        let mut cursor = start;
        for (id, r) in items {
            let d = (cursor - key(r)).round();
            self.translate(id, if horizontal { vec2(d, 0.0) } else { vec2(0.0, d) });
            cursor += size(r) + gap;
        }
    }

    /// Wrap layers in a live boolean group; it takes the bottom layer's paint, like Figma.
    pub fn boolean(&mut self, ids: &[NodeId], op: BoolOp) -> Option<NodeId> {
        let g = self.group(ids)?;
        let bottom = self.nodes[&g].children[0];
        let b = (*self.nodes[&bottom]).clone();
        let n = self.node_mut(g);
        n.kind = Kind::Boolean;
        n.bool_op = op;
        n.name = format!("{op:?}");
        n.fills = if b.fills.is_empty() { vec![Paint::solid(SHAPE_GRAY)] } else { b.fills };
        n.strokes = b.strokes;
        n.stroke_width = b.stroke_width;
        n.expanded = false;
        self.reflow();
        Some(g)
    }

    /// Dissolve a group or frame, keeping its children in place. Returns the children.
    pub fn ungroup(&mut self, id: NodeId) -> Vec<NodeId> {
        let page = self.page_of(id);
        let parent = self.nodes[&id].parent;
        let index = self.index_in_parent(id);
        let kids = self.nodes[&id].children.clone();
        for (i, c) in kids.iter().enumerate() {
            self.reparent(*c, page, parent, Some(index + 1 + i));
        }
        self.remove(id);
        self.reflow();
        kids
    }

    pub fn reorder(&mut self, id: NodeId, order: Order) {
        let page = self.page_of(id);
        let parent = self.nodes[&id].parent;
        let list = self.children_of_mut(page, parent);
        let Some(i) = list.iter().position(|c| *c == id) else { return };
        let last = list.len() - 1;
        let j = match order {
            Order::Forward => (i + 1).min(last),
            Order::Backward => i.saturating_sub(1),
            Order::Front => last,
            Order::Back => 0,
        };
        let n = list.remove(i);
        list.insert(j, n);
    }

    /// Move an auto layout child `steps` places along the flow, skipping hidden and absolute siblings.
    pub fn step_in_flow(&mut self, id: NodeId, steps: isize) {
        let (page, parent) = (self.page_of(id), self.nodes[&id].parent);
        let list = self.children_of(page, parent).clone();
        let flow: Vec<usize> = (0..list.len()).filter(|&i| self.nodes[&list[i]].visible && !self.nodes[&list[i]].absolute).collect();
        let Some(k) = flow.iter().position(|&i| list[i] == id) else { return };
        let to = flow[(k as isize + steps).clamp(0, flow.len() as isize - 1) as usize];
        let list = self.children_of_mut(page, parent);
        let n = list.remove(flow[k]);
        list.insert(to, n);
    }

    /// Copy subtrees into a standalone fragment (roots first, in z order).
    pub fn extract(&self, ids: &[NodeId]) -> Vec<Node> {
        let mut out = vec![];
        for id in self.sort_by_z(ids) {
            out.push((*self.nodes[&id]).clone());
            out.extend(self.descendants(id).into_iter().map(|d| (*self.nodes[&d]).clone()));
        }
        out
    }

    /// Insert a fragment from `extract` under `parent` with fresh ids. Returns new root ids.
    pub fn paste(&mut self, frag: &[Node], page: usize, parent: Option<NodeId>, index: Option<usize>) -> Vec<NodeId> {
        let in_frag: std::collections::HashSet<NodeId> = frag.iter().map(|n| n.id).collect();
        let mut map: HashMap<NodeId, NodeId> = HashMap::new();
        let mut roots = vec![];
        let mut at = index;
        for n in frag {
            let mut copy = n.clone();
            copy.children.clear();
            let is_root = !n.parent.is_some_and(|p| in_frag.contains(&p));
            let (par, idx) = if is_root { (parent, at) } else { (Some(map[&n.parent.unwrap()]), None) };
            let new = self.insert(page, par, idx, copy);
            map.insert(n.id, new);
            if is_root {
                roots.push(new);
                at = at.map(|i| i + 1);
            }
        }
        roots
    }

    pub fn duplicate(&mut self, ids: &[NodeId]) -> Vec<NodeId> {
        let mut out = vec![];
        for id in self.sort_by_z(ids) {
            let page = self.page_of(id);
            let parent = self.nodes[&id].parent;
            let index = self.index_in_parent(id);
            let frag = self.extract(&[id]);
            out.extend(self.paste(&frag, page, parent, Some(index + 1)));
        }
        out
    }

    /// Deepest visible, unlocked node under `p` (topmost in paint order).
    pub fn hit(&self, page: usize, p: Pos2, tol: f32) -> Option<NodeId> {
        self.hit_in(&self.pages[page].children, p, tol)
    }

    fn hit_in(&self, list: &[NodeId], p: Pos2, tol: f32) -> Option<NodeId> {
        for &id in list.iter().rev() {
            let n = &self.nodes[&id];
            if !n.visible || n.locked {
                continue;
            }
            if n.kind.is_frame() && n.clip && !n.rect.expand(tol).contains(n.to_local(p)) {
                continue;
            }
            if n.kind == Kind::Boolean {
                // Only the result's area is clickable; it resolves to the operand underneath.
                if !crate::boolean::contains(self, id, p) {
                    continue;
                }
                return self.hit_in(&n.children, p, tol).or(Some(id));
            }
            if let Some(h) = self.hit_in(&n.paint_order(), p, tol) {
                return Some(h);
            }
            if self.hit_self(n, p, tol) {
                return Some(id);
            }
        }
        None
    }

    pub fn hit_self(&self, n: &Node, p: Pos2, tol: f32) -> bool {
        let p = n.to_local(p);
        match n.kind {
            Kind::Group | Kind::Boolean => false,
            Kind::Frame | Kind::Component | Kind::Instance | Kind::Section | Kind::ComponentSet | Kind::Rectangle | Kind::Text => {
                n.rect.expand(tol).contains(p)
            }
            Kind::Slice => false,
            Kind::Polygon | Kind::Star => {
                let pts = n.star_points();
                contains(&[(pts, true)], p, false)
            }
            Kind::Ellipse if n.is_arc() => {
                let polys: Vec<(Vec<Pos2>, bool)> = n.arc_outline(tol).into_iter().map(|p| (p, true)).collect();
                contains(&polys, p, true)
            }
            Kind::Ellipse => {
                let c = n.rect.center();
                let r = n.rect.size() / 2.0 + Vec2::splat(tol);
                let d = p - c;
                (d.x / r.x).powi(2) + (d.y / r.y).powi(2) <= 1.0
            }
            Kind::Vector => {
                let polys = flatten(n);
                let filled = n.fills.iter().any(|f| f.visible) && n.has_area();
                if filled && contains(&polys, p, n.even_odd) {
                    return true;
                }
                let reach = tol + n.stroke_width / 2.0;
                polys.iter().any(|(poly, closed)| {
                    let segs = poly.len().saturating_sub(if *closed { 0 } else { 1 });
                    (0..segs).any(|i| seg_dist(p, poly[i], poly[(i + 1) % poly.len()]) <= reach)
                })
            }
        }
    }

    pub fn bounds(&self, ids: &[NodeId]) -> Rect {
        ids.iter().fold(Rect::NOTHING, |r, id| r.union(self.nodes[id].aabb()))
    }
}

/// Copy each property from `src` unless the instance layer overrode it
/// (its value no longer matches what was last copied).
fn merge_from(e: &mut Node, src: &Node) {
    let snap = e.synced.take().map(|b| *b).unwrap_or_else(|| src.clone());
    macro_rules! take {
        ($($f:ident),*) => {$(
            if e.$f == snap.$f {
                e.$f = src.$f.clone();
            }
        )*};
    }
    take!(
        name, visible, opacity, fills, strokes, stroke_width, stroke_sides, stroke_align, radius, corner_radii, dash, cap, join, clip, shadows, layout, sizing, min_size, max_size, constraints, absolute,
        paths, even_odd, text, font_size, font_weight, font_family, text_sizing, text_align, text_valign, line_height, letter_spacing,
        decoration, text_case, paragraph_spacing, truncate, max_lines, rotation, component, start_marker, end_marker, point_count, inner_ratio, reactions, overflow, scroll_pos, fill_style,
        stroke_style, text_style, effect_style, prop_bind, is_mask, blend, bindings, modes, spans, lists
    );
    e.synced = Some(Box::new(Node { children: vec![], synced: None, ..src.clone() }));
}

/// Tight bounds of a cubic Bezier: endpoints plus the curve's extrema on each axis.
pub fn cubic_bounds(p0: Pos2, p1: Pos2, p2: Pos2, p3: Pos2) -> Rect {
    let mut r = Rect::from_two_pos(p0, p3);
    let at = |t: f32| {
        let u = 1.0 - t;
        (p0.to_vec2() * u * u * u + p1.to_vec2() * 3.0 * u * u * t + p2.to_vec2() * 3.0 * u * t * t + p3.to_vec2() * t * t * t).to_pos2()
    };
    for axis in 0..2 {
        let (a0, a1, a2, a3) = (p0[axis], p1[axis], p2[axis], p3[axis]);
        // B'(t)/3 = A t² + B t + C
        let (a, b, c) = (-a0 + 3.0 * a1 - 3.0 * a2 + a3, 2.0 * (a0 - 2.0 * a1 + a2), a1 - a0);
        let mut roots = vec![];
        if a.abs() < 1e-9 {
            if b.abs() > 1e-9 {
                roots.push(-c / b);
            }
        } else {
            let disc = b * b - 4.0 * a * c;
            if disc >= 0.0 {
                let sq = disc.sqrt();
                roots.push((-b + sq) / (2.0 * a));
                roots.push((-b - sq) / (2.0 * a));
            }
        }
        for t in roots.into_iter().filter(|t| *t > 0.0 && *t < 1.0) {
            r.extend_with(at(t));
        }
    }
    r
}

/// Rotate `p` around `c` by `deg` counter-clockwise on screen (y points down).
pub fn rotate(p: Pos2, c: Pos2, deg: f32) -> Pos2 {
    if deg == 0.0 {
        return p;
    }
    // Exact quarter turns keep coordinates on whole pixels.
    let (s, co) = match normalize_deg(deg) {
        90.0 => (-1.0, 0.0),
        180.0 => (0.0, -1.0),
        -90.0 => (1.0, 0.0),
        _ => (-deg.to_radians()).sin_cos(),
    };
    let d = p - c;
    c + vec2(d.x * co - d.y * s, d.x * s + d.y * co)
}

/// Wrap to (-180, 180].
pub fn normalize_deg(d: f32) -> f32 {
    let r = d.rem_euclid(360.0);
    if r > 180.0 { r - 360.0 } else { r }
}

/// Absolute polylines approximating each contour (16 steps per curve), with closed flags.
pub fn flatten(n: &Node) -> Vec<(Vec<Pos2>, bool)> {
    let o = n.rect.min.to_vec2();
    n.paths
        .iter()
        .filter(|sp| !sp.points.is_empty())
        .map(|sp| {
            let pts = &sp.points;
            let mut out = vec![pts[0].p + o];
            let segs = if sp.closed { pts.len() } else { pts.len() - 1 };
            for i in 0..segs {
                let a = pts[i];
                let b = pts[(i + 1) % pts.len()];
                let (p0, p1, p2, p3) = (a.p + o, a.p + a.h_out + o, b.p + b.h_in + o, b.p + o);
                if a.h_out == Vec2::ZERO && b.h_in == Vec2::ZERO {
                    out.push(p3);
                    continue;
                }
                for s in 1..=16 {
                    let t = s as f32 / 16.0;
                    let u = 1.0 - t;
                    let v = p0.to_vec2() * u * u * u + p1.to_vec2() * 3.0 * u * u * t + p2.to_vec2() * 3.0 * u * t * t + p3.to_vec2() * t * t * t;
                    out.push(v.to_pos2());
                }
            }
            if sp.closed && out.len() > 1 && out.first() == out.last() {
                out.pop();
            }
            (out, sp.closed)
        })
        .collect()
}

/// Point-in-shape across closed contours with the given fill rule.
pub fn contains(polys: &[(Vec<Pos2>, bool)], p: Pos2, even_odd: bool) -> bool {
    let mut winding = 0i32;
    let mut crossings = 0;
    for (poly, closed) in polys {
        if !closed || poly.len() < 3 {
            continue;
        }
        let n = poly.len();
        for i in 0..n {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            if (a.y <= p.y) != (b.y <= p.y) {
                let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
                if p.x < x {
                    crossings += 1;
                    winding += if b.y > a.y { 1 } else { -1 };
                }
            }
        }
    }
    if even_odd { crossings % 2 == 1 } else { winding != 0 }
}

fn seg_dist(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = if ab.length_sq() > 0.0 { ((p - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
    (a + ab * t).distance(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_min_size(pos2(x, y), vec2(w, h))
    }

    #[test]
    fn insert_names_and_order() {
        let mut d = Document::default();
        let a = d.insert(0, None, None, Node::new(Kind::Rectangle, r(0., 0., 10., 10.)));
        let b = d.insert(0, None, None, Node::new(Kind::Rectangle, r(5., 5., 10., 10.)));
        assert_eq!(d.node(a).name, "Rectangle 1");
        assert_eq!(d.node(b).name, "Rectangle 2");
        assert_eq!(d.hit(0, pos2(7., 7.), 0.), Some(b), "topmost wins");
        d.reorder(b, Order::Back);
        assert_eq!(d.hit(0, pos2(7., 7.), 0.), Some(a));
    }

    #[test]
    fn group_ungroup_and_scale() {
        let mut d = Document::default();
        let a = d.insert(0, None, None, Node::new(Kind::Rectangle, r(0., 0., 10., 10.)));
        let b = d.insert(0, None, None, Node::new(Kind::Ellipse, r(20., 20., 10., 10.)));
        let g = d.group(&[b, a]).unwrap();
        assert_eq!(d.node(g).rect, r(0., 0., 30., 30.));
        assert_eq!(d.node(g).children, vec![a, b], "z order kept");
        assert_eq!(d.pages[0].children, vec![g]);
        d.set_rect(g, r(0., 0., 60., 60.));
        assert_eq!(d.node(b).rect, r(40., 40., 20., 20.));
        d.translate(g, vec2(1., 1.));
        assert_eq!(d.node(a).rect, r(1., 1., 20., 20.));
        assert_eq!(d.ungroup(g), vec![a, b]);
        assert_eq!(d.pages[0].children, vec![a, b]);
        assert!(!d.nodes.contains_key(&g));
    }

    #[test]
    fn baseline_alignment_lines_up_text() {
        let mut d = Document::default();
        let mut f = Node::new(Kind::Frame, r(0., 0., 300., 10.));
        f.layout = AutoLayout { mode: LayoutMode::Horizontal, cross: AxisAlign::Baseline, ..Default::default() };
        f.sizing = [Sizing::Hug; 2];
        let f = d.insert(0, None, None, f);
        let text = |size: f32| Node { text: "Hi".into(), font_size: size, ..Node::new(Kind::Text, r(0., 0., 20., size * 1.21)) };
        let big = d.insert(0, Some(f), None, text(32.));
        let small = d.insert(0, Some(f), None, text(12.));
        let rect = d.insert(0, Some(f), None, Node::new(Kind::Rectangle, r(0., 0., 10., 10.)));
        d.reflow();
        let base = |id: NodeId| d.node(id).rect.min.y + d.node(id).font_size * crate::text::ASCENT;
        assert!((base(big) - base(small)).abs() < 1e-3, "baselines meet");
        assert_eq!(d.node(big).rect.min.y, 0.0, "the tallest text sits at the top");
        assert!((d.node(rect).rect.max.y - base(big)).abs() < 1e-3, "a shape sits on the baseline");
        assert_eq!(d.node(f).rect.height(), d.node(small).rect.max.y.max(d.node(big).rect.max.y), "hugs the dropped text");
    }

    #[test]
    fn grid_layout_places_spans_and_sizes_tracks() {
        let mut d = Document::default();
        let mut f = Node::new(Kind::Frame, r(0., 0., 220., 10.));
        f.layout = AutoLayout { mode: LayoutMode::Grid, grid: [3, 1], gap: 10., cross_gap: 5., padding: [0., 10., 0., 10.], ..Default::default() };
        f.sizing = [Sizing::Fixed, Sizing::Hug];
        let f = d.insert(0, None, None, f);
        let kid = |d: &mut Document| d.insert(0, Some(f), None, Node::new(Kind::Rectangle, r(0., 0., 20., 30.)));
        let a = kid(&mut d);
        let wide = kid(&mut d);
        d.node_mut(wide).grid_span = [2, 1];
        d.node_mut(wide).sizing[0] = Sizing::Fill;
        let c = kid(&mut d);
        let e = kid(&mut d);
        d.reflow();
        // Columns: (220 - 20 padding - 20 gaps) / 3 = 60 wide. Rows hug the 30-tall layers.
        assert_eq!(d.node(a).rect, r(10., 0., 20., 30.));
        assert_eq!(d.node(wide).rect, r(80., 0., 130., 30.), "spans two columns and fills them");
        assert_eq!(d.node(c).rect.min, pos2(10., 35.), "next row");
        assert_eq!(d.node(e).rect.min, pos2(80., 35.));
        assert_eq!(d.node(f).rect.height(), 65.0, "rows grew to fit: 2 x 30 + 5");
        // Fixed tracks.
        d.node_mut(f).layout.tracks = [Some(40.), None];
        d.reflow();
        assert_eq!(d.node(e).rect.min.x, 60.0);
    }

    #[test]
    fn first_on_top_reverses_paint_and_hits() {
        let mut d = Document::default();
        let mut f = Node::new(Kind::Frame, r(0., 0., 100., 100.));
        f.layout = AutoLayout { mode: LayoutMode::Horizontal, gap: -50., ..Default::default() };
        let f = d.insert(0, None, None, f);
        let a = d.insert(0, Some(f), None, Node::new(Kind::Rectangle, r(0., 0., 60., 60.)));
        let b = d.insert(0, Some(f), None, Node::new(Kind::Rectangle, r(0., 0., 60., 60.)));
        d.reflow();
        assert_eq!(d.hit(0, pos2(30., 30.), 0.), Some(b), "last on top");
        d.node_mut(f).layout.reverse_z = true;
        assert_eq!(d.node(f).paint_order(), vec![b, a]);
        assert_eq!(d.hit(0, pos2(30., 30.), 0.), Some(a), "first on top");
    }

    #[test]
    fn step_in_flow_skips_hidden_siblings() {
        let mut d = Document::default();
        let f = d.insert(0, None, None, Node::new(Kind::Frame, r(0., 0., 100., 100.)));
        let ids: Vec<NodeId> = (0..4).map(|_| d.insert(0, Some(f), None, Node::new(Kind::Rectangle, r(0., 0., 10., 10.)))).collect();
        d.node_mut(ids[1]).visible = false;
        d.step_in_flow(ids[0], 1);
        assert_eq!(d.node(f).children, vec![ids[1], ids[2], ids[0], ids[3]]);
        d.step_in_flow(ids[0], 5);
        assert_eq!(d.node(f).children, vec![ids[1], ids[2], ids[3], ids[0]]);
        d.step_in_flow(ids[0], -1);
        assert_eq!(d.node(f).children, vec![ids[1], ids[2], ids[0], ids[3]]);
    }

    #[test]
    fn empty_group_is_removed() {
        let mut d = Document::default();
        let a = d.insert(0, None, None, Node::new(Kind::Rectangle, r(0., 0., 10., 10.)));
        let g = d.group(&[a]).unwrap();
        d.remove(a);
        d.reflow();
        assert!(!d.nodes.contains_key(&g));
        assert!(d.pages[0].children.is_empty());
    }

    #[test]
    fn frame_clips_hits_and_keeps_children_on_resize() {
        let mut d = Document::default();
        let f = d.insert(0, None, None, Node::new(Kind::Frame, r(0., 0., 100., 100.)));
        let c = d.insert(0, Some(f), None, Node::new(Kind::Rectangle, r(90., 90., 50., 50.)));
        assert_eq!(d.hit(0, pos2(95., 95.), 0.), Some(c));
        assert_eq!(d.hit(0, pos2(120., 120.), 0.), None, "clipped");
        d.set_rect(f, r(0., 0., 200., 200.));
        assert_eq!(d.node(c).rect, r(90., 90., 50., 50.));
    }

    #[test]
    fn duplicate_and_paste_deep() {
        let mut d = Document::default();
        let f = d.insert(0, None, None, Node::new(Kind::Frame, r(0., 0., 100., 100.)));
        d.insert(0, Some(f), None, Node::new(Kind::Rectangle, r(1., 1., 5., 5.)));
        let dup = d.duplicate(&[f]);
        assert_eq!(d.pages[0].children, vec![f, dup[0]]);
        assert_eq!(d.node(dup[0]).children.len(), 1);
        let kid = d.node(dup[0]).children[0];
        assert_eq!(d.node(kid).parent, Some(dup[0]));
        assert_eq!(d.nodes.len(), 4);
    }

    #[test]
    fn vector_hit_and_bounds() {
        let pts = [
            PathPoint { p: pos2(10., 10.), ..Default::default() },
            PathPoint { p: pos2(110., 10.), ..Default::default() },
        ];
        let mut d = Document::default();
        let v = d.insert(0, None, None, Node::vector(&pts, false));
        assert_eq!(d.node(v).rect, r(10., 10., 100., 0.));
        assert_eq!(d.hit(0, pos2(50., 11.), 1.), Some(v));
        assert_eq!(d.hit(0, pos2(50., 20.), 1.), None);
        d.set_rect(v, r(10., 10., 50., 0.));
        assert_eq!(d.node(v).paths[0].points[1].p, pos2(50., 0.));
    }

    #[test]
    fn rotation_hits_and_bounds() {
        let mut d = Document::default();
        let a = d.insert(0, None, None, Node::new(Kind::Rectangle, r(0., 0., 100., 10.)));
        d.rotate(a, 90.0, pos2(50., 5.));
        let n = d.node(a);
        assert_eq!(n.rotation, 90.0);
        let b = n.aabb();
        assert!((b.width() - 10.0).abs() < 0.01 && (b.height() - 100.0).abs() < 0.01, "{b:?}");
        assert_eq!(d.hit(0, pos2(50., 40.), 0.), Some(a), "inside rotated box");
        assert_eq!(d.hit(0, pos2(90., 5.), 0.), None, "outside rotated box");
        // Counter-clockwise: the right end swings up.
        let tr = n.corners()[1];
        assert!(tr.y < -40.0, "{tr:?}");
        // Groups re-hug rotated children and carry them around a pivot.
        let g = d.group(&[a]).unwrap();
        d.rotate(g, 90.0, pos2(50., 5.));
        assert_eq!(d.node(a).rotation, 180.0);
        assert_eq!(d.node(g).rotation, 0.0);
        assert!((d.node(g).rect.width() - 100.0).abs() < 0.01);
    }

    #[test]
    fn gradients_sample_and_convert() {
        let mut p = Paint::solid(Rgba::rgb(255, 0, 0));
        p.convert(PaintKind::Linear);
        assert_eq!(p.stops[0].1, Rgba::rgb(255, 0, 0));
        assert_eq!(p.sample(0.5), Rgba([255, 0, 0, 128]), "fades without darkening");
        assert_eq!(p.sample(1.0).a(), 0);
        assert_eq!(p.gradient_t(pos2(0.3, 0.5)), 0.5, "top-to-bottom by default");
        p.convert(PaintKind::Radial);
        assert_eq!(p.gradient_t(pos2(0.5, 0.0)), 1.0);
        p.convert(PaintKind::Solid);
        assert_eq!(p.color, Rgba::rgb(255, 0, 0));
    }

    #[test]
    fn images_roundtrip_through_json() {
        let mut d = Document::default();
        d.images.insert("abc".into(), std::sync::Arc::new(vec![1, 2, 3, 250]));
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains("AQID+g=="), "{json}");
        let back: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(back.images["abc"].as_slice(), &[1, 2, 3, 250]);
    }

    fn al_frame(d: &mut Document, mode: LayoutMode) -> NodeId {
        let mut f = Node::new(Kind::Frame, r(0., 0., 300., 100.));
        f.layout = AutoLayout { mode, gap: 10., padding: [5., 5., 5., 5.], ..Default::default() };
        d.insert(0, None, None, f)
    }

    #[test]
    fn older_files_with_one_property_binding_still_load() {
        let old: Node = serde_json::from_str(r#"{"prop_bind":["Label","Text"]}"#).unwrap();
        assert_eq!(old.prop_bind, vec![("Label".to_string(), PropTarget::Text)]);
        let none: Node = serde_json::from_str(r#"{"prop_bind":null}"#).unwrap();
        assert!(none.prop_bind.is_empty());
        let many: Node = serde_json::from_str(r#"{"prop_bind":[["Show","Visible"],["Icon","Swap"]]}"#).unwrap();
        assert_eq!(many.prop_bind.len(), 2);
    }

    #[test]
    fn component_sets_grow_to_hold_their_variants() {
        let mut d = Document::default();
        let set = d.insert(0, None, None, Node::new(Kind::ComponentSet, r(0., 0., 100., 100.)));
        let c = d.insert(0, Some(set), None, Node::new(Kind::Component, r(20., 20., 200., 40.)));
        d.reflow();
        assert_eq!(d.node(set).rect, r(0., 0., 240., 100.));
        d.node_mut(c).rect = r(20., 20., 20., 20.);
        d.reflow();
        assert_eq!(d.node(set).rect.width(), 240., "doesn't shrink on its own");
    }

    #[test]
    fn image_adjustments_change_pixels() {
        let px = |adj: ImageAdjust| {
            let mut p = vec![100, 50, 25, 255];
            adj.apply(&mut p);
            p
        };
        assert_eq!(px(ImageAdjust::default()), vec![100, 50, 25, 255]);
        assert_eq!(px(ImageAdjust { exposure: 1.0, ..Default::default() }), vec![200, 100, 50, 255], "one stop brighter");
        let gray = px(ImageAdjust { saturation: -1.0, ..Default::default() });
        assert!(gray[0] == gray[1] && gray[1] == gray[2], "{gray:?}");
        let warm = px(ImageAdjust { temperature: 1.0, ..Default::default() });
        assert!(warm[0] > 100 && warm[2] < 25);
    }

    #[test]
    fn individual_stroke_sides_draw_as_bands() {
        let mut d = Document::default();
        let mut n = Node::new(Kind::Rectangle, r(0., 0., 100., 50.));
        n.strokes = vec![Paint::solid(Rgba::rgb(0, 0, 0))];
        n.stroke_align = StrokeAlign::Inside;
        n.stroke_sides = Some([0., 0., 4., 0.]);
        assert_eq!(side_bands(&n, n.stroke_sides.unwrap()), vec![r(0., 46., 100., 4.)], "a bottom border only");
        n.stroke_align = StrokeAlign::Outside;
        n.stroke_sides = Some([2., 0., 0., 2.]);
        assert_eq!(side_bands(&n, n.stroke_sides.unwrap()), vec![r(-2., -2., 102., 2.), r(-2., 0., 2., 50.)]);
        let id = d.insert(0, None, None, n);
        let svg = crate::export::to_svg(&d, &[id]);
        assert_eq!(svg.matches("<rect").count(), 3, "fill plus two bands: {svg}");
    }

    #[test]
    fn auto_layout_wraps_rows_and_respects_min_max() {
        let mut d = Document::default();
        let f = al_frame(&mut d, LayoutMode::Horizontal);
        d.node_mut(f).rect = r(0., 0., 100., 10.);
        d.node_mut(f).sizing = [Sizing::Fixed, Sizing::Hug];
        d.node_mut(f).layout.wrap = true;
        d.node_mut(f).layout.cross_gap = 4.;
        // Inner width 90: 40 + 10 + 40 fits, the third wraps.
        let kids: Vec<NodeId> = [20., 30., 10.].iter().map(|&h| d.insert(0, Some(f), None, Node::new(Kind::Rectangle, r(0., 0., 40., h)))).collect();
        d.reflow();
        assert_eq!(d.node(kids[0]).rect, r(5., 5., 40., 20.));
        assert_eq!(d.node(kids[1]).rect, r(55., 5., 40., 30.));
        assert_eq!(d.node(kids[2]).rect, r(5., 39., 40., 10.), "second row below the tallest plus the row gap");
        assert_eq!(d.node(f).rect.height(), 5. + 30. + 4. + 10. + 5.);
        // Hug height stops at the max; a filling child stops at its min.
        d.node_mut(f).max_size[1] = Some(40.);
        d.reflow();
        assert_eq!(d.node(f).rect.height(), 40.);
        d.node_mut(f).layout.wrap = false;
        d.node_mut(f).rect = r(0., 0., 300., 40.);
        d.node_mut(kids[2]).sizing[0] = Sizing::Fill;
        d.node_mut(kids[2]).min_size[0] = Some(250.);
        d.reflow();
        assert_eq!(d.node(kids[2]).rect.width(), 250.);
        let css = crate::codegen::css(&d, kids[2]);
        assert!(css.contains("min-width: 250px"), "{css}");
        // Included strokes: the frame's inside stroke pads it, a child's outside stroke adds to its box.
        d.node_mut(kids[2]).sizing[0] = Sizing::Fixed;
        d.node_mut(kids[2]).min_size[0] = None;
        let stroke = |n: &mut Node, align| {
            n.strokes = vec![Paint::solid(Rgba::rgb(0, 0, 0))];
            n.stroke_width = 2.;
            n.stroke_align = align;
        };
        stroke(d.node_mut(f), StrokeAlign::Inside);
        stroke(d.node_mut(kids[0]), StrokeAlign::Outside);
        d.node_mut(f).layout.strokes_included = true;
        d.reflow();
        assert_eq!(d.node(kids[0]).rect.min, pos2(9., 9.), "5 padding + 2 frame stroke + 2 own stroke");
        assert_eq!(d.node(kids[1]).rect.min.x, 9. + 40. + 2. + 10.);
    }

    #[test]
    fn auto_layout_stacks_hugs_and_fills() {
        let mut d = Document::default();
        let f = al_frame(&mut d, LayoutMode::Horizontal);
        let a = d.insert(0, Some(f), None, Node::new(Kind::Rectangle, r(100., 100., 20., 30.)));
        let b = d.insert(0, Some(f), None, Node::new(Kind::Rectangle, r(0., 0., 40., 10.)));
        d.reflow();
        assert_eq!(d.node(a).rect, r(5., 5., 20., 30.));
        assert_eq!(d.node(b).rect, r(35., 5., 40., 10.));
        // Hug both axes.
        d.node_mut(f).sizing = [Sizing::Hug, Sizing::Hug];
        d.reflow();
        assert_eq!(d.node(f).rect, r(0., 0., 80., 40.));
        // Fill: b takes the rest of a fixed 300 wide frame; cross fill stretches.
        d.node_mut(f).sizing = [Sizing::Fixed, Sizing::Fixed];
        d.node_mut(f).rect = r(0., 0., 300., 100.);
        d.node_mut(b).sizing = [Sizing::Fill, Sizing::Fill];
        d.reflow();
        assert_eq!(d.node(b).rect, r(35., 5., 260., 90.));
        // Centered, vertical, space-between.
        d.node_mut(b).sizing = [Sizing::Fixed, Sizing::Fixed];
        d.node_mut(b).rect = r(0., 0., 40., 10.);
        d.node_mut(f).layout.mode = LayoutMode::Vertical;
        d.node_mut(f).layout.space_between = true;
        d.node_mut(f).layout.cross = AxisAlign::Center;
        d.reflow();
        assert_eq!(d.node(a).rect.min, pos2(140., 5.));
        assert_eq!(d.node(b).rect.max.y, 95.);
        // Absolute children are left alone; hidden ones don't take space.
        d.node_mut(a).absolute = true;
        d.node_mut(a).rect = r(200., 50., 20., 30.);
        d.reflow();
        assert_eq!(d.node(a).rect, r(200., 50., 20., 30.));
    }

    #[test]
    fn nested_auto_layout_hug_propagates() {
        let mut d = Document::default();
        let outer = al_frame(&mut d, LayoutMode::Vertical);
        d.node_mut(outer).sizing = [Sizing::Hug, Sizing::Hug];
        let inner = d.insert(0, Some(outer), None, {
            let mut f = Node::new(Kind::Frame, r(0., 0., 10., 10.));
            f.layout = AutoLayout { mode: LayoutMode::Horizontal, gap: 4., ..Default::default() };
            f.sizing = [Sizing::Hug, Sizing::Hug];
            f
        });
        d.insert(0, Some(inner), None, Node::new(Kind::Rectangle, r(0., 0., 30., 30.)));
        d.insert(0, Some(inner), None, Node::new(Kind::Rectangle, r(0., 0., 30., 20.)));
        d.reflow();
        assert_eq!(d.node(inner).rect, r(5., 5., 64., 30.));
        assert_eq!(d.node(outer).rect, r(0., 0., 74., 40.));
    }

    #[test]
    fn constraints_follow_frame_resize() {
        let mut d = Document::default();
        let f = d.insert(0, None, None, Node::new(Kind::Frame, r(0., 0., 100., 100.)));
        let mk = |d: &mut Document, h: Constraint, v: Constraint| {
            let mut n = Node::new(Kind::Rectangle, r(10., 10., 20., 20.));
            n.constraints = [h, v];
            d.insert(0, Some(f), None, n)
        };
        let left = mk(&mut d, Constraint::Min, Constraint::Min);
        let right = mk(&mut d, Constraint::Max, Constraint::Max);
        let both = mk(&mut d, Constraint::Stretch, Constraint::Center);
        let scale = mk(&mut d, Constraint::Scale, Constraint::Scale);
        d.set_rect(f, r(-50., 0., 200., 200.));
        assert_eq!(d.node(left).rect, r(-40., 10., 20., 20.), "left edge moved, child follows it");
        assert_eq!(d.node(right).rect, r(60., 110., 20., 20.));
        assert_eq!(d.node(both).rect, r(-40., 60., 120., 20.));
        assert_eq!(d.node(scale).rect, r(-30., 20., 40., 40.));
        d.set_rect_with(f, r(0., 0., 100., 100.), false);
        assert_eq!(d.node(left).rect, r(-40., 10., 20., 20.), "Cmd-resize ignores constraints");
    }

    #[test]
    fn instances_follow_main_and_keep_overrides() {
        let mut d = Document::default();
        let mut comp = Node::new(Kind::Component, r(0., 0., 100., 50.));
        comp.name = "Button".into();
        let c = d.insert(0, None, None, comp);
        let mut label = Node::new(Kind::Text, r(10., 10., 30., 15.));
        label.text = "OK".into();
        let t = d.insert(0, Some(c), None, label);
        let bg = d.insert(0, Some(c), Some(0), Node::new(Kind::Rectangle, r(0., 0., 100., 50.)));
        let i = d.instantiate(c, 0, None, None, pos2(200., 0.));
        let inst = d.node(i).clone();
        assert_eq!((inst.kind, inst.name.as_str(), inst.rect), (Kind::Instance, "Button", r(200., 0., 100., 50.)));
        assert_eq!(inst.children.len(), 2);
        let (ibg, it) = (inst.children[0], inst.children[1]);
        assert_eq!(d.node(it).text, "OK");
        assert_eq!(d.node(it).rect.min, pos2(210., 10.));
        assert_eq!(d.node(ibg).source, Some(bg));

        // Override the label text, then change the main: fill propagates, text override sticks.
        d.node_mut(it).text = "Cancel".into();
        d.node_mut(bg).fills = vec![Paint::solid(Rgba::rgb(255, 0, 0))];
        d.node_mut(t).text = "Okay".into();
        d.node_mut(t).font_size = 20.0;
        d.reflow();
        assert_eq!(d.node(ibg).fills[0].color, Rgba::rgb(255, 0, 0));
        assert_eq!(d.node(it).text, "Cancel", "override kept");
        assert_eq!(d.node(it).font_size, 20.0, "non-overridden prop updates");

        // Structure changes flow through; reset drops overrides.
        let dot = d.insert(0, Some(c), None, Node::new(Kind::Ellipse, r(80., 5., 10., 10.)));
        d.reflow();
        assert_eq!(d.node(i).children.len(), 3);
        assert_eq!(d.node(*d.node(i).children.last().unwrap()).rect.min, pos2(280., 5.));
        d.remove(dot);
        d.reflow();
        assert_eq!(d.node(i).children.len(), 2);
        d.reset_instance(i);
        assert_eq!(d.node(it).text, "Okay");

        // Moving the main doesn't move instances; detaching cuts the link.
        d.translate(c, vec2(-50., 0.));
        d.reflow();
        assert_eq!(d.node(i).rect.min, pos2(200., 0.));
        d.detach_instance(i);
        d.node_mut(t).text = "Later".into();
        d.reflow();
        assert_eq!(d.node(i).kind, Kind::Frame);
        assert_eq!(d.node(it).text, "Okay");
    }

    #[test]
    fn resized_instance_applies_constraints() {
        let mut d = Document::default();
        let c = d.insert(0, None, None, Node::new(Kind::Component, r(0., 0., 100., 50.)));
        let mut pin = Node::new(Kind::Rectangle, r(80., 0., 20., 20.));
        pin.constraints = [Constraint::Max, Constraint::Min];
        d.insert(0, Some(c), None, pin);
        let i = d.instantiate(c, 0, None, None, pos2(0., 100.));
        d.set_rect(i, r(0., 100., 200., 50.));
        d.reflow();
        let kid = d.node(i).children[0];
        assert_eq!(d.node(kid).rect, r(180., 100., 20., 20.), "pinned right");
        assert_eq!(d.node(i).rect.width(), 200.0, "size override kept");
    }

    #[test]
    fn holes_follow_fill_rule() {
        let sq = |x: f32, w: f32| SubPath {
            points: [(x, x), (x + w, x), (x + w, x + w), (x, x + w)].map(|(a, b)| PathPoint { p: pos2(a, b), ..Default::default() }).to_vec(),
            closed: true,
        };
        let mut d = Document::default();
        let mut n = Node::vector_paths(vec![sq(0., 100.), sq(25., 50.)]);
        n.fills = vec![Paint::solid(Rgba::BLACK)];
        n.even_odd = true;
        let v = d.insert(0, None, None, n);
        assert_eq!(d.node(v).rect, r(0., 0., 100., 100.));
        assert_eq!(d.hit(0, pos2(10., 10.), 0.), Some(v));
        assert_eq!(d.hit(0, pos2(50., 50.), 0.), None, "even-odd hole");
        d.node_mut(v).even_odd = false;
        assert_eq!(d.hit(0, pos2(50., 50.), 0.), Some(v), "same winding fills");
    }

    #[test]
    fn flip_and_distribute() {
        let mut d = Document::default();
        let pts = [PathPoint { p: pos2(0., 0.), ..Default::default() }, PathPoint { p: pos2(10., 5.), h_in: vec2(-2., 0.), ..Default::default() }];
        let v = d.insert(0, None, None, Node::vector(&pts, false));
        d.flip(v, true, 5.0);
        let n = d.node(v);
        assert_eq!(n.rect, r(0., 0., 10., 5.));
        assert_eq!(n.paths[0].points[0].p, pos2(10., 0.));
        assert_eq!(n.paths[0].points[1].h_in, vec2(2., 0.));
        let a = d.insert(0, None, None, Node::new(Kind::Rectangle, r(0., 0., 10., 10.)));
        let b = d.insert(0, None, None, Node::new(Kind::Rectangle, r(15., 0., 10., 10.)));
        let c = d.insert(0, None, None, Node::new(Kind::Rectangle, r(90., 0., 10., 10.)));
        d.distribute(&[c, a, b], true);
        assert_eq!(d.node(b).rect.min.x, 45.0);
        assert_eq!(d.node(c).rect.min.x, 90.0);
    }

    #[test]
    fn styles_propagate_and_detach() {
        let mut d = Document::default();
        let a = d.insert(0, None, None, Node::new(Kind::Rectangle, r(0., 0., 10., 10.)));
        let b = d.insert(0, None, None, Node::new(Kind::Ellipse, r(20., 0., 10., 10.)));
        let red = d.add_style("Brand/Red".into(), StyleValue::Paint(vec![Paint::solid(Rgba::rgb(255, 0, 0))]));
        d.node_mut(a).fill_style = Some(red);
        d.node_mut(b).fill_style = Some(red);
        d.reflow();
        assert_eq!(d.node(b).fills[0].color, Rgba::rgb(255, 0, 0));
        // Editing the style updates every user.
        d.styles[0].value = StyleValue::Paint(vec![Paint::solid(Rgba::rgb(0, 0, 255))]);
        d.reflow();
        assert_eq!(d.node(a).fills[0].color, Rgba::rgb(0, 0, 255));
        // Deleting it leaves the last value and drops the link.
        d.styles.clear();
        d.reflow();
        assert_eq!(d.node(a).fill_style, None);
        assert_eq!(d.node(a).fills[0].color, Rgba::rgb(0, 0, 255));
    }

    #[test]
    fn variants_swap_and_props_drive_layers() {
        let mut d = Document::default();
        let mk = |d: &mut Document, name: &str, color: Rgba, x: f32| {
            let mut c = Node::new(Kind::Component, r(x, 0., 100., 40.));
            c.name = name.into();
            let c = d.insert(0, None, None, c);
            let mut bg = Node::new(Kind::Rectangle, r(x, 0., 100., 40.));
            bg.name = "bg".into();
            bg.fills = vec![Paint::solid(color)];
            d.insert(0, Some(c), None, bg);
            let mut t = Node::new(Kind::Text, r(x + 10., 10., 40., 15.));
            t.name = "label".into();
            t.text = "Button".into();
            d.insert(0, Some(c), None, t);
            c
        };
        let a = mk(&mut d, "State=Default", Rgba::rgb(0, 0, 255), 0.);
        let b = mk(&mut d, "State=Hover", Rgba::rgb(0, 255, 0), 200.);
        d.node_mut(a).fills = vec![Paint::solid(Rgba::rgb(1, 1, 1))];
        d.node_mut(b).fills = vec![Paint::solid(Rgba::rgb(2, 2, 2))];
        let set = d.combine_as_variants(&[a, b]).unwrap();
        assert_eq!(d.node(set).kind, Kind::ComponentSet);
        assert_eq!(d.variant_props(set), vec![("State".to_string(), vec!["Default".to_string(), "Hover".to_string()])]);
        assert_eq!(d.find_variant(set, &[("State".into(), "Hover".into())]), Some(b));
        // An instance with a text override keeps it when swapped to another variant.
        let i = d.instantiate(a, 0, None, None, pos2(0., 200.));
        let label = d.node(i).children[1];
        d.node_mut(label).text = "Buy".into();
        d.swap_component(i, b);
        let kids = d.node(i).children.clone();
        assert_eq!(d.node(kids[0]).fills[0].color, Rgba::rgb(0, 255, 0), "takes the new variant's look");
        assert_eq!(d.node(i).fills[0].color, Rgba::rgb(2, 2, 2), "root paint follows the swap too");
        assert_eq!(d.node(kids[1]).text, "Buy", "override carried over");
        assert_eq!(d.node(i).name, d.node(set).name, "named after the set");
        // Boolean and text properties drive bound layers.
        d.node_mut(set).comp_props = vec![
            CompProp { name: "Show label".into(), default: PropValue::Bool(true) },
            CompProp { name: "Label".into(), default: PropValue::Text("Go".into()) },
        ];
        for c in [a, b] {
            let lbl = d.node(c).children[1];
            d.node_mut(lbl).prop_bind = vec![("Label".into(), PropTarget::Text)];
            let bg = d.node(c).children[0];
            d.node_mut(bg).prop_bind = vec![("Show label".into(), PropTarget::Visible)];
        }
        let j = d.instantiate(a, 0, None, None, pos2(0., 300.));
        let kids = d.node(j).children.clone();
        assert_eq!(d.node(kids[1]).text, "Go");
        d.node_mut(j).prop_values = vec![("Label".into(), PropValue::Text("Pay".into())), ("Show label".into(), PropValue::Bool(false))];
        d.reflow();
        assert_eq!(d.node(kids[1]).text, "Pay");
        assert!(!d.node(kids[0]).visible);
    }

    #[test]
    fn grid_bands() {
        let g = LayoutGrid { kind: GridKind::Columns, count: 3, gutter: 10., margin: 20., ..Default::default() };
        let near = |a: &[(f32, f32)], b: &[(f32, f32)]| a.iter().zip(b).all(|(x, y)| (x.0 - y.0).abs() < 1e-3 && (x.1 - y.1).abs() < 1e-3);
        assert!(near(&g.bands(0., 340.), &[(20., 113.3333), (123.3333, 216.6667), (226.6667, 320.)]));
        let c = LayoutGrid { align: GridAlign::Center, size: 50., count: 2, gutter: 10., margin: 0., ..g };
        assert_eq!(c.bands(0., 200.), vec![(45., 95.), (105., 155.)]);
    }

    #[test]
    fn variables_resolve_by_mode_and_alias() {
        let mut d = Document::default();
        let c = d.add_collection("Theme".into());
        d.collections[0].modes = vec!["Light".into(), "Dark".into()];
        let base = d.add_variable(c, "gray/900".into(), VarValue::Color(Rgba::rgb(20, 20, 20)));
        let bg = d.add_variable(c, "bg".into(), VarValue::Color(Rgba::WHITE));
        d.variables[1].values[1] = VarValue::Alias(base);
        let space = d.add_variable(c, "space/md".into(), VarValue::Number(12.0));
        let f = d.insert(0, None, None, Node::new(Kind::Frame, r(0., 0., 100., 100.)));
        let child = d.insert(0, Some(f), None, Node::new(Kind::Rectangle, r(10., 10., 20., 20.)));
        d.node_mut(child).bindings = vec![(Bindable::Fill, bg), (Bindable::Radius, space)];
        d.reflow();
        assert_eq!(d.node(child).fills[0].color, Rgba::WHITE, "light by default");
        assert_eq!(d.node(child).radius, 12.0);
        // Switching the frame to Dark flows down to the child, through the alias.
        d.node_mut(f).modes = vec![(c, 1)];
        d.reflow();
        assert_eq!(d.node(child).fills[0].color, Rgba::rgb(20, 20, 20));
        // Editing the variable (Dark value, since the frame is Dark) updates bound layers.
        d.variables[2].values[1] = VarValue::Number(4.0);
        d.reflow();
        assert_eq!(d.node(child).radius, 4.0);
        assert!(Bindable::Radius.accepts(&VarValue::Number(1.0)) && !Bindable::Fill.accepts(&VarValue::Number(1.0)));
    }

    #[test]
    fn rich_text_spans_style_and_shift() {
        let bold = SpanStyle { font_weight: Some(700), ..Default::default() };
        let red = SpanStyle { color: Some(Rgba::rgb(255, 0, 0)), ..Default::default() };
        // "Hello world": bold "world", then red "lo wo" (overlaps both).
        let s1 = style_range(&[], 11, 6, 11, &bold);
        assert_eq!(s1, vec![TextSpan { start: 6, end: 11, style: bold.clone() }]);
        let s2 = style_range(&s1, 11, 3, 8, &red);
        assert_eq!(s2.len(), 3);
        assert_eq!((s2[0].start, s2[0].end), (3, 6));
        assert_eq!(s2[1].style, bold.merge(&red));
        assert_eq!((s2[2].start, s2[2].end, s2[2].style.clone()), (8, 11, bold.clone()));
        // Typing before a span shifts it; typing at its end extends it; deleting through it trims it.
        let typed = shift_spans(&s1, "Hello world", "Oh Hello world");
        assert_eq!((typed[0].start, typed[0].end), (9, 14));
        let extended = shift_spans(&s1, "Hello world", "Hello worlds");
        assert_eq!((extended[0].start, extended[0].end), (6, 12));
        let trimmed = shift_spans(&s1, "Hello world", "Hello wd");
        assert_eq!((trimmed[0].start, trimmed[0].end), (6, 8));
        assert!(shift_spans(&s1, "Hello world", "Hello ").is_empty(), "span deleted with its text");
        let b = ListLine { kind: ListKind::Bullet, indent: 0 };
        let l = vec![ListLine::default(), b];
        assert_eq!(shift_lists(&l, "Title\nOne", "Title\nOne\nTwo"), vec![ListLine::default(), b, b], "enter continues the list");
        assert_eq!(shift_lists(&l, "Title\nOne", "Intro\nTitle\nOne"), vec![ListLine::default(), ListLine::default(), b], "earlier paragraphs shift it");
        assert!(shift_lists(&l, "Title\nOne", "TitleOne").is_empty(), "joined into the plain paragraph");
        let mut t = Node::new(Kind::Text, Rect::ZERO);
        t.text = "Cat and cat".into();
        t.spans = vec![TextSpan { start: 8, end: 11, style: SpanStyle { font_weight: Some(700), ..Default::default() } }];
        assert_eq!(replace_text(&mut t, "cat", "Dog"), 2);
        assert_eq!(t.text, "Dog and Dog");
        assert_eq!((t.spans[0].start, t.spans[0].end), (8, 11), "the bold word stays bold");
    }

    #[test]
    fn reparent_rejects_cycles() {
        let mut d = Document::default();
        let f = d.insert(0, None, None, Node::new(Kind::Frame, r(0., 0., 100., 100.)));
        let g = d.insert(0, Some(f), None, Node::new(Kind::Frame, r(0., 0., 50., 50.)));
        d.reparent(f, 0, Some(g), None);
        assert_eq!(d.node(f).parent, None);
    }
}
