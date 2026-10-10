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
    OnHover,
    WhilePressed,
    /// Fires this many milliseconds after the frame appears.
    AfterDelay(u32),
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
    /// Seconds.
    pub duration: f32,
}

impl Default for Transition {
    fn default() -> Self {
        Self { kind: TransitionKind::Instant, direction: Direction::Left, duration: 0.3 }
    }
}

/// A prototype interaction on a layer.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Reaction {
    pub trigger: Trigger,
    pub action: Action,
    #[serde(default)]
    pub transition: Transition,
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

/// Parse "Size=Large, State=Hover" into ordered pairs.
pub fn variant_pairs(name: &str) -> Vec<(String, String)> {
    name.split(',')
        .filter_map(|part| part.split_once('=').map(|(k, v)| (k.trim().to_string(), v.trim().to_string())))
        .collect()
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
}

impl ExportFormat {
    pub fn ext(self) -> &'static str {
        match self {
            ExportFormat::Png => "png",
            ExportFormat::Jpg => "jpg",
            ExportFormat::Svg => "svg",
            ExportFormat::Pdf => "pdf",
        }
    }
    pub fn raster(self) -> bool {
        matches!(self, ExportFormat::Png | ExportFormat::Jpg)
    }
}

/// One export preset on a layer, like Figma's Export rows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExportSetting {
    pub scale: f32,
    pub format: ExportFormat,
    pub suffix: String,
}

impl Default for ExportSetting {
    fn default() -> Self {
        Self { scale: 1.0, format: ExportFormat::Png, suffix: String::new() }
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

/// A fill or stroke paint. For gradients and images, `color`'s alpha is the paint opacity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Paint {
    pub color: Rgba,
    pub visible: bool,
    #[serde(default)]
    pub kind: PaintKind,
    /// Gradient stops: position in 0..=1 and color.
    #[serde(default)]
    pub stops: Vec<(f32, Rgba)>,
    /// Gradient handles in the layer's unit square: linear start/end, or radial center/edge.
    #[serde(default)]
    pub from: Pos2,
    #[serde(default)]
    pub to: Pos2,
    /// Key into `Document::images`.
    #[serde(default)]
    pub image: String,
}

impl Paint {
    pub fn solid(color: Rgba) -> Self {
        Self { color, visible: true, kind: PaintKind::Solid, stops: vec![], from: Pos2::ZERO, to: Pos2::ZERO, image: String::new() }
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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Node {
    pub id: NodeId,
    pub name: String,
    pub kind: Kind,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub rect: Rect,
    /// Degrees counter-clockwise around the rect center, as shown in Figma.
    pub rotation: f32,
    pub visible: bool,
    pub locked: bool,
    pub opacity: f32,
    pub fills: Vec<Paint>,
    pub strokes: Vec<Paint>,
    pub stroke_width: f32,
    pub stroke_align: StrokeAlign,
    pub radius: f32,
    /// Independent corners: top-left, top-right, bottom-right, bottom-left.
    pub corner_radii: Option<[f32; 4]>,
    /// Dash pattern (dash, gap, ...); empty = solid.
    pub dash: Vec<f32>,
    pub cap: Cap,
    pub join: Join,
    pub start_marker: Marker,
    pub end_marker: Marker,
    /// Polygon sides / star points.
    pub point_count: u32,
    /// Star inner radius as a fraction of the outer.
    pub inner_ratio: f32,
    pub clip: bool,
    pub shadows: Vec<Shadow>,
    pub layout: AutoLayout,
    /// Horizontal, vertical.
    pub sizing: [Sizing; 2],
    pub constraints: [Constraint; 2],
    /// Ignores the parent's auto layout.
    pub absolute: bool,
    pub bool_op: BoolOp,
    /// Clips the layers above it in the same parent.
    pub is_mask: bool,
    pub exports: Vec<ExportSetting>,
    pub blend: BlendMode,
    /// Prototype interactions.
    pub reactions: Vec<Reaction>,
    /// Linked shared styles.
    pub fill_style: Option<StyleId>,
    pub stroke_style: Option<StyleId>,
    pub text_style: Option<StyleId>,
    pub effect_style: Option<StyleId>,
    /// Components and sets: property definitions.
    pub comp_props: Vec<CompProp>,
    /// A layer inside a component bound to a property.
    pub prop_bind: Option<(String, PropTarget)>,
    /// Instances: chosen property values (missing = the default).
    pub prop_values: Vec<(String, PropValue)>,
    /// Instances: the main component they copy.
    pub component: Option<NodeId>,
    /// Inside an instance (and the instance itself): the component layer this mirrors.
    pub source: Option<NodeId>,
    /// The source's values as last copied; a property that differs from this is an override.
    pub synced: Option<Box<Node>>,
    pub paths: Vec<SubPath>,
    /// Even-odd fill rule (holes by parity) instead of non-zero winding.
    pub even_odd: bool,
    pub text: String,
    pub font_size: f32,
    pub font_weight: u16,
    pub font_family: String,
    pub text_sizing: TextSizing,
    pub text_align: TextAlign,
    pub text_valign: VAlign,
    pub line_height: LineHeight,
    /// Percent of the font size.
    pub letter_spacing: f32,
    pub decoration: Decoration,
    pub text_case: TextCase,
    /// Wrapped lines from the last layout, used by export.
    #[serde(skip)]
    pub lines: Vec<String>,
    pub expanded: bool,
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
            clip: false,
            shadows: vec![],
            layout: AutoLayout::default(),
            sizing: [Sizing::Fixed; 2],
            constraints: [Constraint::Min; 2],
            absolute: false,
            bool_op: BoolOp::Union,
            is_mask: false,
            exports: vec![],
            blend: BlendMode::Normal,
            reactions: vec![],
            fill_style: None,
            stroke_style: None,
            text_style: None,
            effect_style: None,
            comp_props: vec![],
            prop_bind: None,
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
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Page {
    pub name: String,
    pub children: Vec<NodeId>,
    pub background: Rgba,
    /// Prototype starting frames, in order.
    #[serde(default)]
    pub flows: Vec<NodeId>,
}

pub const PAGE_BG: Rgba = Rgba::rgb(0xF5, 0xF5, 0xF5);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Document {
    pub pages: Vec<Page>,
    pub nodes: HashMap<NodeId, Node>,
    pub next_id: NodeId,
    /// Embedded images: content hash -> encoded PNG/JPEG bytes (base64 in files).
    #[serde(default, with = "images_b64")]
    pub images: HashMap<String, std::sync::Arc<Vec<u8>>>,
    #[serde(default)]
    pub styles: Vec<Style>,
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
            pages: vec![Page { name: "Page 1".into(), children: vec![], background: PAGE_BG, flows: vec![] }],
            nodes: HashMap::new(),
            next_id: 1,
            images: HashMap::new(),
            styles: vec![],
        }
    }
}

impl Document {
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[&id]
    }
    pub fn node_mut(&mut self, id: NodeId) -> &mut Node {
        self.nodes.get_mut(&id).expect("node exists")
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
        self.nodes.insert(id, node);
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
        for n in self.nodes.values_mut() {
            for (link, stroke) in [(&mut n.fill_style, false), (&mut n.stroke_style, true)] {
                match link.and_then(|id| styles.get(&id)) {
                    Some(StyleValue::Paint(p)) => {
                        if stroke { n.strokes = p.clone() } else { n.fills = p.clone() }
                    }
                    Some(_) | None => *link = None,
                }
            }
            match n.text_style.and_then(|id| styles.get(&id)) {
                Some(StyleValue::Text(t)) => t.apply(n),
                _ => n.text_style = None,
            }
            match n.effect_style.and_then(|id| styles.get(&id)) {
                Some(StyleValue::Effect(e)) => n.shadows = e.clone(),
                _ => n.effect_style = None,
            }
        }
    }

    /// Create an instance of `comp` with its top-left at `at`.
    pub fn instantiate(&mut self, comp: NodeId, page: usize, parent: Option<NodeId>, index: Option<usize>, at: Pos2) -> NodeId {
        let mut c = self.nodes[&comp].clone();
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
                *n = Node { id: n.id, parent: n.parent, children: n.children.clone(), kind: n.kind, component: n.component, source: n.source, synced: None, ..src };
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
        let page = self.page_of(inst);
        let mut c = self.nodes[&comp].clone();
        // Instances of a variant are named after the component set, as in Figma.
        if let Some(set) = c.parent.map(|p| &self.nodes[&p]).filter(|p| p.kind == Kind::ComponentSet) {
            c.name = set.name.clone();
        }
        // Instance layers by the component layer they mirror.
        let mut by_source: HashMap<NodeId, NodeId> = HashMap::new();
        for d in self.descendants(inst) {
            if let Some(s) = self.nodes[&d].source {
                by_source.entry(s).or_insert(d);
            }
        }
        // Root: merge appearance, keep placement; size follows the main unless resized.
        let target_size = {
            let n = &self.nodes[&inst];
            let snap = n.synced.as_deref();
            if snap.is_none_or(|s| s.rect.size() == n.rect.size()) { c.rect.size() } else { n.rect.size() }
        };
        let actual = Rect::from_min_size(self.nodes[&inst].rect.min, target_size);
        merge_from(self.node_mut(inst), &c);
        self.node_mut(inst).rect = Rect::from_min_size(actual.min, c.rect.size());
        let offset = actual.min - c.rect.min;
        // Children, in component order.
        let mut keep = std::collections::HashSet::new();
        let mut order: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        for cid in self.descendants(comp) {
            let src = self.nodes[&cid].clone();
            let parent = if src.parent == Some(comp) { inst } else { by_source[&src.parent.unwrap()] };
            let id = match by_source.get(&cid) {
                Some(&e) => {
                    merge_from(self.node_mut(e), &src);
                    if self.nodes[&e].parent != Some(parent) {
                        self.reparent(e, page, Some(parent), None);
                    }
                    e
                }
                None => {
                    let mut n = Node { children: vec![], ..src.clone() };
                    n.source = Some(cid);
                    n.synced = Some(Box::new(Node { children: vec![], ..src.clone() }));
                    // Nested instances keep pointing at their own component.
                    let id = self.insert(page, Some(parent), None, n);
                    by_source.insert(cid, id);
                    id
                }
            };
            let text = self.nodes[&id].kind == Kind::Text;
            let n = self.node_mut(id);
            n.rect = if text {
                Rect::from_min_size(src.rect.min + offset, n.rect.size())
            } else {
                Rect::from_min_size(src.rect.min + offset, src.rect.size())
            };
            keep.insert(id);
            order.entry(parent).or_default().push(id);
        }
        for d in self.descendants(inst) {
            if !keep.contains(&d) && self.nodes.get(&d).is_some_and(|n| n.source.is_some()) {
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
        for d in self.descendants(inst) {
            let Some((name, target)) = self.nodes[&d].prop_bind.clone() else { continue };
            let n = self.node_mut(d);
            match (target, value(&name)) {
                (PropTarget::Visible, Some(PropValue::Bool(b))) => n.visible = b,
                (PropTarget::Text, Some(PropValue::Text(t))) => n.text = t,
                (PropTarget::Swap, Some(PropValue::Swap(c))) => n.component = Some(c),
                _ => {}
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
                    overrides.insert(path(self, d), n.clone());
                }
        }
        for c in self.nodes[&inst].children.clone() {
            self.remove(c);
        }
        // Keep the root's last snapshot: anything not overridden then takes the new variant's value.
        let n = self.node_mut(inst);
        n.component = Some(comp);
        n.source = Some(comp);
        let size = self.nodes[&comp].rect.size();
        let n = self.node_mut(inst);
        n.rect = Rect::from_min_size(n.rect.min, size);
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
        self.apply_styles();
        let instances: Vec<NodeId> = self.nodes.values().filter(|n| n.kind == Kind::Instance).map(|n| n.id).collect();
        // ponytail: syncs every instance on every edit; track dirty components when files get large
        for _ in 0..2 {
            for &i in &instances {
                if self.nodes.contains_key(&i) {
                    self.sync_instance(i);
                }
            }
        }
        self.reflow_layout();
    }

    fn reflow_layout(&mut self) {
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
        let [pt, pr, pb, pl] = l.padding;
        let (p0, p1, q0, q1) = if h { (pl, pr, pt, pb) } else { (pt, pb, pl, pr) };
        let kids: Vec<NodeId> = n
            .children
            .iter()
            .copied()
            .filter(|c| self.nodes[c].visible && !self.nodes[c].absolute)
            .collect();
        // Text is always auto-width, so it can only hug.
        let fills = |k: &Node, axis: usize| k.sizing[axis] == Sizing::Fill && k.kind != Kind::Text;
        let count = kids.len() as f32;
        let gaps = l.gap * (count - 1.0).max(0.0);
        let fixed_main: f32 = kids.iter().map(|c| &self.nodes[c]).filter(|k| !fills(k, ma)).map(|k| main(k.aabb().size())).sum();
        let n_fill = kids.iter().filter(|c| fills(&self.nodes[c], ma)).count() as f32;

        let mut size = n.rect.size();
        if n.sizing[ma] == Sizing::Hug {
            let fill_now: f32 = kids.iter().map(|c| &self.nodes[c]).filter(|k| fills(k, ma)).map(|k| main(k.aabb().size())).sum();
            let m = p0 + p1 + fixed_main + fill_now + if l.space_between { 0.0 } else { gaps };
            size = along(m, cross(size));
        }
        if n.sizing[ca] == Sizing::Hug {
            let c = kids.iter().map(|c| &self.nodes[c]).filter(|k| !fills(k, ca)).map(|k| cross(k.aabb().size())).fold(0.0, f32::max);
            size = along(main(size), q0 + q1 + c);
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
                AxisAlign::Min => 0.0,
                AxisAlign::Center => free / 2.0,
                AxisAlign::Max => free,
            }
        };
        let origin = n.rect.min;
        let mut cursor = main(origin.to_vec2()) + p0 + lead;
        for c in kids {
            let k = self.nodes[&c].clone();
            let mut ks = k.aabb().size();
            if fills(&k, ma) {
                ks = along(fill_main, cross(ks));
            }
            if fills(&k, ca) {
                ks = along(main(ks), inner_cross);
            }
            if (ks - k.aabb().size()).length() > 1e-4 && k.rotation == 0.0 {
                self.set_rect(c, Rect::from_min_size(k.rect.min, ks));
            }
            let off = match l.cross {
                AxisAlign::Min => 0.0,
                AxisAlign::Center => (inner_cross - cross(ks)) / 2.0,
                AxisAlign::Max => inner_cross - cross(ks),
            };
            let target = along(cursor, cross(origin.to_vec2()) + q0 + off).to_pos2();
            let cur = self.nodes[&c].aabb().min;
            if cur != target {
                self.translate(c, target - cur);
            }
            cursor += main(ks) + gap;
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
        let b = self.nodes[&bottom].clone();
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

    /// Copy subtrees into a standalone fragment (roots first, in z order).
    pub fn extract(&self, ids: &[NodeId]) -> Vec<Node> {
        let mut out = vec![];
        for id in self.sort_by_z(ids) {
            out.push(self.nodes[&id].clone());
            out.extend(self.descendants(id).into_iter().map(|d| self.nodes[&d].clone()));
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
            if let Some(h) = self.hit_in(&n.children, p, tol) {
                return Some(h);
            }
            if self.hit_self(n, p, tol) {
                return Some(id);
            }
        }
        None
    }

    fn hit_self(&self, n: &Node, p: Pos2, tol: f32) -> bool {
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
        name, visible, opacity, fills, strokes, stroke_width, stroke_align, radius, corner_radii, dash, cap, join, clip, shadows, layout, sizing, constraints, absolute,
        paths, even_odd, text, font_size, font_weight, font_family, text_sizing, text_align, text_valign, line_height, letter_spacing,
        decoration, text_case, rotation, component, start_marker, end_marker, point_count, inner_ratio, reactions, fill_style,
        stroke_style, text_style, effect_style, prop_bind, is_mask, blend
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
            d.node_mut(lbl).prop_bind = Some(("Label".into(), PropTarget::Text));
            let bg = d.node(c).children[0];
            d.node_mut(bg).prop_bind = Some(("Show label".into(), PropTarget::Visible));
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
    fn reparent_rejects_cycles() {
        let mut d = Document::default();
        let f = d.insert(0, None, None, Node::new(Kind::Frame, r(0., 0., 100., 100.)));
        let g = d.insert(0, Some(f), None, Node::new(Kind::Frame, r(0., 0., 50., 50.)));
        d.reparent(f, 0, Some(g), None);
        assert_eq!(d.node(f).parent, None);
    }
}
