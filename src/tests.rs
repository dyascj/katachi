//! Headless interaction tests: real pointer/keyboard events through the whole app.

use crate::app::{App, Tool};
use crate::doc::{Kind, Node, NodeId};
use egui::{Event, Key, Modifiers, PointerButton, Pos2, Rect, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

/// Keep autosaves and recents out of the real user folder.
pub fn isolate_data() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let dir = std::env::temp_dir().join(format!("katachi-test-data-{}", std::process::id()));
        // SAFETY: set once before any test reads it; every test uses the same value.
        unsafe { std::env::set_var("KATACHI_DATA_DIR", dir) };
    });
}

fn harness() -> Harness<'static, App> {
    isolate_data();
    let mut h = Harness::builder().with_size(vec2(1440.0, 900.0)).with_step_dt(1.0 / 60.0).build_eframe(|cc| {
        crate::setup_style(&cc.egui_ctx);
        let mut app = App::new(None);
        // The recovery test runs in parallel against the same data dir.
        app.recoverable.clear();
        app
    });
    h.run_steps(2);
    h
}

/// World point -> screen point under the current view.
fn s(h: &Harness<App>, x: f32, y: f32) -> Pos2 {
    h.state().view.to_screen(pos2(x, y))
}

fn press(h: &mut Harness<App>, p: Pos2, down: bool, m: Modifiers) {
    h.input_mut().modifiers = m;
    h.event_modifiers(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: down, modifiers: m }, m);
    h.step();
}

fn move_to(h: &mut Harness<App>, p: Pos2, m: Modifiers) {
    h.input_mut().modifiers = m;
    h.event_modifiers(Event::PointerMoved(p), m);
    h.step();
}

fn drag(h: &mut Harness<App>, a: Pos2, b: Pos2, m: Modifiers) {
    move_to(h, a, m);
    press(h, a, true, m);
    for i in 1..=6 {
        move_to(h, a.lerp(b, i as f32 / 6.0), m);
    }
    press(h, b, false, m);
    h.input_mut().modifiers = Modifiers::NONE;
    h.run_steps(2);
}

fn click(h: &mut Harness<App>, p: Pos2, m: Modifiers) {
    h.run_steps(30); // let the double-click window lapse
    move_to(h, p, m);
    press(h, p, true, m);
    press(h, p, false, m);
    h.input_mut().modifiers = Modifiers::NONE;
    h.run_steps(2);
}

fn key(h: &mut Harness<App>, m: Modifiers, k: Key) {
    h.key_press_modifiers(m, k);
    h.run_steps(2);
}

fn draw(h: &mut Harness<App>, tool: Key, a: (f32, f32), b: (f32, f32)) -> NodeId {
    key(h, Modifiers::NONE, tool);
    let (pa, pb) = (s(h, a.0, a.1), s(h, b.0, b.1));
    drag(h, pa, pb, Modifiers::NONE);
    *h.state().sel.last().expect("drawn node selected")
}

fn rect(h: &Harness<App>, id: NodeId) -> Rect {
    h.state().doc.node(id).rect
}

fn r(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::from_min_size(pos2(x, y), vec2(w, h))
}

#[test]
fn draw_rectangle_then_undo_redo() {
    let mut h = harness();
    let id = draw(&mut h, Key::R, (0.0, 0.0), (120.0, 80.0));
    assert_eq!(h.state().doc.node(id).kind, Kind::Rectangle);
    assert_eq!(rect(&h, id), r(0., 0., 120., 80.));
    assert_eq!(h.state().tool, Tool::Move, "tool resets after drawing");
    key(&mut h, Modifiers::COMMAND, Key::Z);
    assert!(h.state().doc.nodes.is_empty());
    key(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    assert_eq!(h.state().doc.nodes.len(), 1);
}

#[test]
fn click_with_shape_tool_makes_default_size() {
    let mut h = harness();
    key(&mut h, Modifiers::NONE, Key::O);
    let p = s(&h, 10.0, 10.0);
    click(&mut h, p, Modifiers::NONE);
    let id = h.state().sel[0];
    assert_eq!(h.state().doc.node(id).kind, Kind::Ellipse);
    assert_eq!(rect(&h, id), r(10., 10., 100., 100.));
}

#[test]
fn shift_draw_is_square_and_alt_draws_from_center() {
    let mut h = harness();
    key(&mut h, Modifiers::NONE, Key::R);
    let (a, b) = (s(&h, 0., 0.), s(&h, 50., 120.));
    drag(&mut h, a, b, Modifiers::SHIFT);
    let id = h.state().sel[0];
    assert_eq!(rect(&h, id), r(0., 0., 120., 120.));
    key(&mut h, Modifiers::NONE, Key::R);
    let (a, b) = (s(&h, 400., 400.), s(&h, 420., 410.));
    drag(&mut h, a, b, Modifiers::ALT);
    let id = h.state().sel[0];
    assert_eq!(rect(&h, id), r(380., 390., 40., 20.));
}

#[test]
fn move_resize_and_delete() {
    let mut h = harness();
    let id = draw(&mut h, Key::R, (0.0, 0.0), (100.0, 100.0));
    let (a, b) = (s(&h, 50., 50.), s(&h, 80., 90.));
    drag(&mut h, a, b, Modifiers::NONE);
    assert_eq!(rect(&h, id), r(30., 40., 100., 100.));
    // Bottom-right corner handle.
    let (a, b) = (s(&h, 130., 140.), s(&h, 180., 160.));
    drag(&mut h, a, b, Modifiers::NONE);
    assert_eq!(rect(&h, id), r(30., 40., 150., 120.));
    key(&mut h, Modifiers::NONE, Key::ArrowRight);
    key(&mut h, Modifiers::SHIFT, Key::ArrowDown);
    assert_eq!(rect(&h, id), r(31., 50., 150., 120.));
    key(&mut h, Modifiers::NONE, Key::Backspace);
    assert!(h.state().doc.nodes.is_empty());
}

#[test]
fn alt_drag_duplicates() {
    let mut h = harness();
    let id = draw(&mut h, Key::R, (0.0, 0.0), (100.0, 100.0));
    let (a, b) = (s(&h, 50., 50.), s(&h, 250., 50.));
    drag(&mut h, a, b, Modifiers::ALT);
    assert_eq!(h.state().doc.nodes.len(), 2);
    assert_eq!(rect(&h, id), r(0., 0., 100., 100.), "original stays");
    let dup = h.state().sel[0];
    assert_ne!(dup, id);
    assert_eq!(rect(&h, dup), r(200., 0., 100., 100.));
}

#[test]
fn snaps_to_sibling_edges() {
    let mut h = harness();
    draw(&mut h, Key::R, (0.0, 0.0), (100.0, 100.0));
    let b = draw(&mut h, Key::R, (300.0, 0.0), (400.0, 100.0));
    // Drop b's left edge 3px right of a's right edge: it should snap flush.
    let (p, q) = (s(&h, 350., 50.), s(&h, 153., 50.));
    drag(&mut h, p, q, Modifiers::NONE);
    assert_eq!(rect(&h, b).min.x, 100.0);
}

#[test]
fn frames_nest_children_and_release_them() {
    let mut h = harness();
    let f = draw(&mut h, Key::F, (0.0, 0.0), (300.0, 300.0));
    let c = draw(&mut h, Key::R, (20.0, 20.0), (60.0, 60.0));
    assert_eq!(h.state().doc.node(c).parent, Some(f), "drawn inside frame");
    // Children of a top-level frame are directly clickable.
    let p = s(&h, -300., 300.);
    click(&mut h, p, Modifiers::NONE);
    let p = s(&h, 40., 40.);
    click(&mut h, p, Modifiers::NONE);
    assert_eq!(h.state().sel, vec![c]);
    let (a, b) = (s(&h, 40., 40.), s(&h, 400., 40.));
    drag(&mut h, a, b, Modifiers::NONE);
    assert_eq!(h.state().doc.node(c).parent, None, "dragged out of frame");
    let (a, b) = (s(&h, 400., 40.), s(&h, 100., 100.));
    drag(&mut h, a, b, Modifiers::NONE);
    assert_eq!(h.state().doc.node(c).parent, Some(f), "dropped back in");
    // Moving the frame carries the child.
    let before = rect(&h, c);
    let (a, b) = (s(&h, 250., 250.), s(&h, 260., 250.));
    let p = s(&h, 250., 250.);
    click(&mut h, p, Modifiers::NONE);
    assert_eq!(h.state().sel, vec![f]);
    drag(&mut h, a, b, Modifiers::NONE);
    assert_eq!(rect(&h, c), before.translate(vec2(10., 0.)));
}

#[test]
fn new_frame_adopts_enclosed_layers() {
    let mut h = harness();
    let a = draw(&mut h, Key::R, (10.0, 10.0), (50.0, 50.0));
    let b = draw(&mut h, Key::R, (300.0, 10.0), (350.0, 50.0));
    let f = draw(&mut h, Key::F, (0.0, 0.0), (200.0, 200.0));
    assert_eq!(h.state().doc.node(a).parent, Some(f));
    assert_eq!(h.state().doc.node(b).parent, None);
}

#[test]
fn marquee_group_and_click_through() {
    let mut h = harness();
    let a = draw(&mut h, Key::R, (0.0, 0.0), (50.0, 50.0));
    let b = draw(&mut h, Key::O, (100.0, 0.0), (150.0, 50.0));
    let (p, q) = (s(&h, -20., -20.), s(&h, 120., 20.));
    drag(&mut h, p, q, Modifiers::NONE);
    let mut sel = h.state().sel.clone();
    sel.sort();
    assert_eq!(sel, vec![a, b]);
    key(&mut h, Modifiers::COMMAND, Key::G);
    let g = h.state().sel[0];
    assert_eq!(h.state().doc.node(g).kind, Kind::Group);
    // Clicking a grouped layer selects the group; double-click dives in.
    let p = s(&h, -300., 300.);
    click(&mut h, p, Modifiers::NONE);
    let p = s(&h, 25., 25.);
    click(&mut h, p, Modifiers::NONE);
    assert_eq!(h.state().sel, vec![g]);
    move_to(&mut h, p, Modifiers::NONE);
    h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.step();
    h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
    assert_eq!(h.state().sel, vec![a]);
    // Cmd-click selects deep directly.
    let p = s(&h, -300., 300.);
    click(&mut h, p, Modifiers::NONE);
    let p = s(&h, 125., 25.);
    click(&mut h, p, Modifiers::COMMAND);
    assert_eq!(h.state().sel, vec![b]);
    // Ungroup restores both to the page.
    let p = s(&h, -300., 300.);
    click(&mut h, p, Modifiers::NONE);
    let p = s(&h, 25., 25.);
    click(&mut h, p, Modifiers::NONE);
    key(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::G);
    assert_eq!(h.state().doc.pages[0].children, vec![a, b]);
}

#[test]
fn text_tool_types_and_empty_text_disappears() {
    let mut h = harness();
    key(&mut h, Modifiers::NONE, Key::T);
    let p = s(&h, 10., 10.);
    click(&mut h, p, Modifiers::NONE);
    h.event(Event::Text("Hello".into()));
    h.run_steps(2);
    key(&mut h, Modifiers::NONE, Key::Escape);
    let id = h.state().sel[0];
    let n = h.state().doc.node(id);
    assert_eq!(n.text, "Hello");
    assert!(n.rect.width() > 20.0, "auto width: {:?}", n.rect);
    assert!(h.state().editing_text.is_none());
    key(&mut h, Modifiers::NONE, Key::T);
    let p = s(&h, 300., 300.);
    click(&mut h, p, Modifiers::NONE);
    key(&mut h, Modifiers::NONE, Key::Escape);
    assert_eq!(h.state().doc.nodes.len(), 1, "empty text removed");
}

#[test]
fn pen_draws_paths() {
    let mut h = harness();
    key(&mut h, Modifiers::NONE, Key::P);
    for (x, y) in [(0., 0.), (100., 0.), (100., 100.)] {
        let p = s(&h, x, y);
        click(&mut h, p, Modifiers::NONE);
    }
    key(&mut h, Modifiers::NONE, Key::Enter);
    let id = h.state().sel[0];
    let n = h.state().doc.node(id);
    assert_eq!((n.kind, n.paths[0].points.len(), n.paths[0].closed), (Kind::Vector, 3, false));
    assert_eq!(n.rect, r(0., 0., 100., 100.));
    // Clicking the first anchor closes the shape.
    key(&mut h, Modifiers::NONE, Key::P);
    for (x, y) in [(300., 0.), (400., 0.), (350., 80.), (300., 0.)] {
        let p = s(&h, x, y);
        click(&mut h, p, Modifiers::NONE);
    }
    let id = h.state().sel[0];
    assert!(h.state().doc.node(id).paths[0].closed);
    assert_eq!(h.state().doc.node(id).paths[0].points.len(), 3);
}

#[test]
fn copy_paste_roundtrip_through_clipboard_text() {
    let mut h = harness();
    let f = draw(&mut h, Key::F, (0.0, 0.0), (200.0, 200.0));
    draw(&mut h, Key::R, (10.0, 10.0), (50.0, 50.0));
    h.state_mut().sel = vec![f];
    h.event(Event::Copy);
    h.step();
    let copied = h
        .output()
        .platform_output
        .commands
        .iter()
        .find_map(|c| match c {
            egui::OutputCommand::CopyText(t) => Some(t.clone()),
            _ => None,
        })
        .expect("copied to clipboard");
    h.state_mut().sel.clear();
    h.event(Event::Paste(copied));
    h.run_steps(2);
    assert_eq!(h.state().doc.nodes.len(), 4);
    let pasted = h.state().sel[0];
    assert_ne!(pasted, f);
    assert_eq!(h.state().doc.node(pasted).children.len(), 1);
    // Plain text pastes as a text layer.
    h.event(Event::Paste("hi there".into()));
    h.run_steps(2);
    let t = h.state().sel[0];
    assert_eq!(h.state().doc.node(t).text, "hi there");
}

#[test]
fn file_roundtrip() {
    let mut h = harness();
    draw(&mut h, Key::F, (0.0, 0.0), (200.0, 200.0));
    draw(&mut h, Key::O, (10.0, 10.0), (50.0, 50.0));
    let dir = std::env::temp_dir().join(format!("katachi-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("t.katachi");
    let json = serde_json::json!({ "katachi": 1, "document": h.state().doc });
    std::fs::write(&path, json.to_string()).unwrap();
    let mut app = App::new(Some(path));
    assert_eq!(app.doc.nodes.len(), 2);
    assert!(!app.dirty);
    let ids: Vec<NodeId> = app.doc.nodes.keys().copied().collect();
    let n: &Node = app.doc.node(ids[0]);
    assert!(matches!(n.kind, Kind::Frame | Kind::Ellipse));
    app.sel = ids;
    app.delete_selection();
    assert!(app.dirty);
}

/// Visual check: `cargo test shots -- --ignored` writes PNGs to target/shots.
#[test]
#[ignore]
fn shots() {
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/shots");
    std::fs::create_dir_all(&out).unwrap();
    let mut h = Harness::builder().with_size(vec2(1440.0, 900.0)).with_step_dt(1.0 / 60.0).with_theme(egui::Theme::Light).wgpu().build_eframe(|cc| {
        crate::setup_style(&cc.egui_ctx);
        App::new(Some(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/demo.katachi")))
    });
    h.run_steps(3);
    let save = |h: &mut Harness<App>, name: &str| {
        h.render().unwrap().save(out.join(format!("{name}.png"))).unwrap();
    };
    // Select the hero rectangle inside the Landing frame, and hover the avatar.
    let p = s(&h, 200., 90.);
    click(&mut h, p, Modifiers::NONE);
    let p = s(&h, 70., 215.);
    move_to(&mut h, p, Modifiers::NONE);
    save(&mut h, "1-select-hover");
    // Mid-drag: move the avatar so its center lines up with the title.
    let (a, b) = (s(&h, 72., 216.), s(&h, 72., 260.));
    move_to(&mut h, a, Modifiers::NONE);
    press(&mut h, a, true, Modifiers::NONE);
    for i in 1..=6 {
        move_to(&mut h, a.lerp(b, i as f32 / 6.0), Modifiers::NONE);
    }
    save(&mut h, "2-dragging-snap");
    press(&mut h, b, false, Modifiers::NONE);
    // Pen in progress.
    key(&mut h, Modifiers::NONE, Key::P);
    for (x, y) in [(0., 360.), (100., 440.), (200., 360.)] {
        let p = s(&h, x, y);
        click(&mut h, p, Modifiers::NONE);
    }
    let p = s(&h, 300., 450.);
    move_to(&mut h, p, Modifiers::NONE);
    save(&mut h, "3-pen");
    // A rotated, selected card.
    key(&mut h, Modifiers::NONE, Key::Escape);
    key(&mut h, Modifiers::NONE, Key::Escape);
    let card = *h.state().doc.pages[0].children.last().unwrap();
    let c = h.state().doc.node(card).rect.center();
    h.state_mut().doc.rotate(card, -12.0, c);
    h.state_mut().sel = vec![card];
    h.run_steps(3);
    save(&mut h, "4-rotated");
    // Paints and effects: gradient hero, shadowed card, radial avatar, image tile.
    use crate::doc::{Paint, PaintKind, Rgba};
    let st = h.state_mut();
    st.undo();
    let ids: Vec<NodeId> = st.doc.nodes.keys().copied().collect();
    let find = |st: &App, name: &str| *ids.iter().find(|i| st.doc.node(**i).name == name).unwrap();
    let (hero, card, avatar) = (find(st, "Hero"), find(st, "Card"), find(st, "Avatar"));
    let mut g = Paint::solid(Rgba::rgb(13, 153, 255));
    g.convert(PaintKind::Linear);
    g.stops = vec![(0.0, Rgba::rgb(13, 153, 255)), (1.0, Rgba::rgb(151, 71, 255))];
    g.from = pos2(0.0, 0.5);
    g.to = pos2(1.0, 0.5);
    st.doc.node_mut(hero).fills = vec![g];
    st.doc.node_mut(card).shadows = vec![Shadow { blur: 24.0, offset: vec2(0., 12.), color: Rgba([0, 0, 0, 90]), ..Default::default() }];
    let mut rg = Paint::solid(Rgba::rgb(255, 199, 0));
    rg.convert(PaintKind::Radial);
    st.doc.node_mut(avatar).fills = vec![rg];
    let mut buf = std::io::Cursor::new(vec![]);
    image::RgbaImage::from_fn(64, 64, |x, y| image::Rgba([(x * 4) as u8, (y * 4) as u8, 180, 255])).write_to(&mut buf, image::ImageFormat::Png).unwrap();
    let key = st.add_image(buf.into_inner());
    let mut ip = Paint::solid(Rgba::WHITE);
    ip.convert(PaintKind::Image);
    ip.image = key;
    let mut tile = Node::new(Kind::Rectangle, r(0., 360., 160., 100.));
    tile.fills = vec![ip];
    tile.radius = 12.0;
    let t = st.doc.insert(0, None, None, tile);
    st.sel = vec![card];
    h.run_steps(3);
    save(&mut h, "5-paints");
    // Auto layout panel: wrap the tile and two copies in a horizontal stack.
    let st = h.state_mut();
    st.sel = vec![t];
    st.duplicate_selection();
    st.nudge(vec2(180., 0.));
    st.sel = vec![t, st.sel[0]];
    st.add_auto_layout();
    h.run_steps(3);
    save(&mut h, "6-auto-layout");
    // Components: the card becomes a main component with an instance beside it.
    let st = h.state_mut();
    st.sel = vec![card];
    st.create_component();
    let insts = st.duplicate_or_instance(&[card]);
    st.doc.translate(insts[0], vec2(280., 0.));
    st.doc.reflow();
    st.sel = insts;
    st.zoom_to(&egui::Context::default(), None, false);
    h.run_steps(3);
    save(&mut h, "7-component");
    h.state_mut().left_tab_assets = true;
    h.run_steps(3);
    save(&mut h, "8-assets");
    // Typography: the title in a system font, selected.
    let st = h.state_mut();
    st.left_tab_assets = false;
    let title = *st.doc.nodes.keys().find(|i| st.doc.node(**i).name == "Title").unwrap();
    st.doc.node_mut(title).font_family = "Georgia".into();
    st.doc.node_mut(title).letter_spacing = -2.0;
    st.sel = vec![title];
    h.run_steps(5);
    save(&mut h, "9-typography");
    // Booleans and vector edit mode.
    let st = h.state_mut();
    let sq = st.doc.insert(0, None, None, Node::new(Kind::Rectangle, r(0., 380., 120., 120.)));
    let ci = st.doc.insert(0, None, None, Node::new(Kind::Ellipse, r(60., 320., 120., 120.)));
    let star = st.doc.insert(0, None, None, Node::new(Kind::Ellipse, r(220., 360., 140., 140.)));
    st.sel = vec![sq, ci];
    st.boolean_selection(crate::doc::BoolOp::Subtract);
    let b = st.sel[0];
    st.doc.node_mut(b).fills = vec![crate::doc::Paint::solid(crate::doc::Rgba::rgb(13, 153, 255))];
    st.sel = vec![star];
    st.enter_vector_edit(star);
    if let Some(ve) = st.vector_edit.as_mut() {
        ve.selected = vec![(0, 1)];
    }
    st.zoom_to(&egui::Context::default(), None, false);
    h.run_steps(4);
    save(&mut h, "10-boolean-vector-edit");
    // Alt measurements against the hovered layer, with rulers.
    let st = h.state_mut();
    st.vector_edit = None;
    st.rulers = true;
    let find = |st: &App, name: &str| *st.doc.nodes.keys().find(|i| st.doc.node(**i).name == name).unwrap();
    let (avatar, hero) = (find(st, "Avatar"), find(st, "Hero"));
    st.sel = vec![avatar];
    let target = h.state().view.to_screen(h.state().doc.node(hero).rect.center());
    h.input_mut().modifiers = Modifiers::ALT;
    h.event(Event::PointerMoved(target));
    h.input_mut().modifiers = Modifiers::ALT;
    h.run_steps(2);
    save(&mut h, "11-measure-rulers");
    // Effects: inner shadow, layer blur, frosted glass, dashes, independent corners.
    use crate::doc::{EffectKind, Shadow};
    h.input_mut().modifiers = Modifiers::NONE;
    let st = h.state_mut();
    st.rulers = false;
    let find = |st: &App, name: &str| *st.doc.nodes.keys().find(|i| st.doc.node(**i).name == name).unwrap();
    let (hero, avatar, landing) = (find(st, "Hero"), find(st, "Avatar"), find(st, "Landing"));
    st.doc.node_mut(hero).shadows = vec![Shadow { kind: EffectKind::InnerShadow, offset: vec2(0., 6.), blur: 12., color: Rgba([0, 0, 0, 120]), ..Default::default() }];
    st.doc.node_mut(avatar).shadows = vec![Shadow { kind: EffectKind::LayerBlur, blur: 6., ..Default::default() }];
    let lr = st.doc.node(landing).rect;
    let mut glass = Node::new(Kind::Rectangle, r(lr.min.x + 220., lr.min.y + 40., 200., 110.));
    glass.fills = vec![crate::doc::Paint::solid(Rgba([255, 255, 255, 70]))];
    glass.shadows = vec![Shadow { kind: EffectKind::BackgroundBlur, blur: 16., ..Default::default() }];
    glass.corner_radii = Some([24., 4., 24., 4.]);
    glass.strokes = vec![crate::doc::Paint::solid(Rgba::WHITE)];
    glass.stroke_width = 2.;
    glass.dash = vec![8., 4.];
    glass.cap = crate::doc::Cap::Round;
    let g = st.doc.insert(0, Some(landing), None, glass);
    st.sel = vec![g];
    h.run_steps(4);
    save(&mut h, "12-effects");
    // New shapes in a section, with an arrow and a slice.
    let st = h.state_mut();
    let mut sec = Node::new(Kind::Section, r(-40., 560., 520., 220.));
    sec.name = "Shapes".into();
    let sec = st.doc.insert(0, None, None, sec);
    let mut pg = Node::new(Kind::Polygon, r(0., 620., 120., 120.));
    pg.fills = vec![crate::doc::Paint::solid(Rgba::rgb(255, 199, 0))];
    st.doc.insert(0, Some(sec), None, pg);
    let mut sr = Node::new(Kind::Star, r(150., 620., 120., 120.));
    sr.fills = vec![crate::doc::Paint::solid(Rgba::rgb(242, 72, 34))];
    st.doc.insert(0, Some(sec), None, sr);
    let mut ar = Node::vector(&[crate::doc::PathPoint { p: pos2(300., 700.), ..Default::default() }, crate::doc::PathPoint { p: pos2(440., 640.), h_in: vec2(-60., 0.), ..Default::default() }], false);
    ar.end_marker = crate::doc::Marker::Arrow;
    ar.start_marker = crate::doc::Marker::Circle;
    ar.stroke_width = 3.;
    st.doc.insert(0, Some(sec), None, ar);
    st.doc.insert(0, None, None, Node::new(Kind::Slice, r(520., 600., 200., 150.)));
    st.sel = vec![];
    st.set_tool(Tool::Star);
    st.zoom_to(&egui::Context::default(), None, false);
    h.run_steps(4);
    save(&mut h, "13-shapes");
    // Prototype tab: noodles from the hero to the cards, a flow start, then present.
    let st = h.state_mut();
    st.set_tool(Tool::Move);
    let find = |st: &App, name: &str| *st.doc.nodes.keys().find(|i| st.doc.node(**i).name == name).unwrap();
    let (hero, landing) = (find(st, "Hero"), find(st, "Landing"));
    let cards: Vec<NodeId> = st.doc.pages[0].children.iter().copied().filter(|c| st.doc.node(*c).name == "Card").collect();
    st.doc.node_mut(hero).reactions = vec![crate::doc::Reaction {
        action: crate::doc::Action::Navigate(cards[0]),
        transition: crate::doc::Transition { kind: crate::doc::TransitionKind::Dissolve, ..Default::default() },
        ..Default::default()
    }];
    st.doc.pages[0].flows = vec![landing];
    st.prototype_tab = true;
    st.sel = vec![hero];
    h.run_steps(3);
    save(&mut h, "14-prototype");
    let st = h.state_mut();
    st.sel = vec![landing];
    st.present(0.0);
    h.run_steps(3);
    save(&mut h, "15-present");
    // Styles: a linked color style on the hero, and the local styles list.
    let st = h.state_mut();
    st.presenting = None;
    st.prototype_tab = false;
    let find = |st: &App, name: &str| *st.doc.nodes.keys().find(|i| st.doc.node(**i).name == name).unwrap();
    let (hero, title, card) = (find(st, "Hero"), find(st, "Title"), find(st, "Card"));
    let brand = st.doc.add_style("Brand/Blue".into(), crate::doc::StyleValue::Paint(vec![crate::doc::Paint::solid(Rgba::rgb(13, 153, 255))]));
    st.doc.add_style("Brand/Ink".into(), crate::doc::StyleValue::Paint(vec![crate::doc::Paint::solid(Rgba::rgb(30, 30, 30))]));
    let h1 = st.doc.add_style("Heading/H1".into(), crate::doc::StyleValue::Text(crate::doc::TextStyle::of(st.doc.node(title))));
    st.doc.add_style("Elevation/Card".into(), crate::doc::StyleValue::Effect(st.doc.node(card).shadows.clone()));
    st.doc.node_mut(hero).fill_style = Some(brand);
    st.doc.node_mut(title).text_style = Some(h1);
    st.doc.reflow();
    st.sel = vec![hero];
    h.run_steps(3);
    save(&mut h, "16-style-linked");
    h.state_mut().sel.clear();
    h.run_steps(3);
    save(&mut h, "17-local-styles");
    // Variants: two button states combined into a set, an instance switched to Hover.
    let st = h.state_mut();
    let mk = |st: &mut App, name: &str, color: Rgba, x: f32| {
        let mut c = Node::new(Kind::Component, r(x, 820., 140., 44.));
        c.name = name.into();
        c.radius = 22.;
        c.fills = vec![crate::doc::Paint::solid(color)];
        let c = st.doc.insert(0, None, None, c);
        let mut t = Node::new(Kind::Text, r(x + 40., 832., 60., 20.));
        t.name = "label".into();
        t.text = "Continue".into();
        t.font_size = 15.;
        t.font_weight = 600;
        t.fills = vec![crate::doc::Paint::solid(Rgba::WHITE)];
        st.doc.insert(0, Some(c), None, t);
        c
    };
    let a = mk(st, "State=Default", Rgba::rgb(13, 153, 255), 0.);
    let b = mk(st, "State=Hover", Rgba::rgb(0, 110, 210), 180.);
    st.doc.combine_as_variants(&[a, b]);
    let i = st.doc.instantiate(a, 0, None, None, pos2(420., 840.));
    st.doc.swap_component(i, b);
    st.doc.reflow();
    st.sel = vec![i];
    st.zoom_to(&egui::Context::default(), None, false);
    h.run_steps(4);
    save(&mut h, "18-variants");
    // Layout guides on the landing frame.
    let st = h.state_mut();
    let landing = *st.doc.nodes.keys().find(|i| st.doc.node(**i).name == "Landing").unwrap();
    st.doc.node_mut(landing).grids = vec![crate::doc::LayoutGrid { kind: crate::doc::GridKind::Columns, count: 4, gutter: 16., margin: 24., ..Default::default() }];
    st.sel = vec![landing];
    h.run_steps(3);
    save(&mut h, "19-layout-grid");
    // Variables: a Light/Dark theme bound to the landing frame's fill and the hero radius.
    use crate::doc::{Bindable, VarValue};
    let st = h.state_mut();
    st.doc.node_mut(landing).grids.clear();
    let c = st.doc.add_collection("Theme".into());
    st.doc.collections.last_mut().unwrap().modes = vec!["Light".into(), "Dark".into()];
    let surface = st.doc.add_variable(c, "color/surface".into(), VarValue::Color(Rgba::WHITE));
    st.doc.variables.last_mut().unwrap().values[1] = VarValue::Color(Rgba::rgb(24, 24, 27));
    let radius = st.doc.add_variable(c, "radius/lg".into(), VarValue::Number(12.));
    st.doc.variables.last_mut().unwrap().values[1] = VarValue::Number(24.);
    st.doc.node_mut(landing).bindings = vec![(Bindable::Fill, surface)];
    let hero = *st.doc.nodes.keys().find(|i| st.doc.node(**i).name == "Hero").unwrap();
    st.doc.node_mut(hero).bindings = vec![(Bindable::Radius, radius)];
    st.doc.node_mut(landing).modes = vec![(c, 1)];
    st.doc.reflow();
    st.sel = vec![landing];
    st.show_variables = true;
    h.run_steps(4);
    save(&mut h, "20-variables");
    // Dev Mode on the card.
    let st = h.state_mut();
    st.show_variables = false;
    st.dev_mode = true;
    let card = *st.doc.nodes.keys().find(|i| st.doc.node(**i).name == "Card" && st.doc.node(**i).kind == Kind::Component).unwrap();
    st.sel = vec![card];
    h.run_steps(3);
    save(&mut h, "21-dev-mode");
    // Comments: two threads, one open.
    let st = h.state_mut();
    st.dev_mode = false;
    let landing = *st.doc.nodes.keys().find(|i| st.doc.node(**i).name == "Landing").unwrap();
    for (i, (at, text)) in [(pos2(400., 20.), "Can we try a warmer gradient here?"), (pos2(120., 260.), "Love the type pairing")].into_iter().enumerate() {
        st.doc.comments.push(crate::doc::Comment {
            id: 9000 + i as u64,
            page: 0,
            at,
            anchor: Some(landing),
            messages: vec![("Charles".into(), text.into(), 0), ("Luna".into(), "Agreed, trying #FF7A59 → #9747FF".into(), 0)][..if i == 0 { 2 } else { 1 }].to_vec(),
            resolved: false,
        });
    }
    st.set_tool(Tool::Comment);
    st.open_comment = Some(9000);
    st.sel.clear();
    h.run_steps(4);
    save(&mut h, "22-comments");
    // Rich text: one text layer with mixed weight, color, italic, and size.
    let st = h.state_mut();
    st.set_tool(Tool::Move);
    st.open_comment = None;
    let sub = *st.doc.nodes.keys().find(|i| st.doc.node(**i).name == "Subtitle").unwrap();
    let n = st.doc.node_mut(sub);
    n.text = "Design in Rust, fast and native".into();
    n.font_size = 16.;
    n.spans = vec![
        crate::doc::TextSpan { start: 10, end: 14, style: crate::doc::SpanStyle { font_weight: Some(700), color: Some(Rgba::rgb(242, 72, 34)), ..Default::default() } },
        crate::doc::TextSpan { start: 16, end: 20, style: crate::doc::SpanStyle { italic: Some(true), ..Default::default() } },
        crate::doc::TextSpan { start: 25, end: 31, style: crate::doc::SpanStyle { font_size: Some(22.), font_weight: Some(600), color: Some(Rgba::rgb(13, 153, 255)), ..Default::default() } },
    ];
    st.sel = vec![sub];
    h.run_steps(4);
    save(&mut h, "23-rich-text");
    // Multiplayer: host, then show two collaborators' cursors and selections.
    let ctx = h.ctx.clone();
    let st = h.state_mut();
    st.collab_port = 0;
    st.host_session(&ctx);
    let find = |st: &App, name: &str| *st.doc.nodes.keys().find(|i| st.doc.node(**i).name == name).unwrap();
    let (hero, star) = (find(st, "Hero"), find(st, "Star 1"));
    if let Some(c) = st.collab.as_mut() {
        c.peers.insert(91, crate::collab::Presence { name: "Luna".into(), color: 2, page: 0, cursor: Some(pos2(300., 120.)), selection: vec![hero] });
        c.peers.insert(92, crate::collab::Presence { name: "Sam".into(), color: 1, page: 0, cursor: Some(pos2(260., 700.)), selection: vec![star] });
    }
    st.sel.clear();
    h.run_steps(3);
    save(&mut h, "24-multiplayer");
}

#[test]
fn rotate_by_corner_drag_and_resize_rotated() {
    let mut h = harness();
    let id = draw(&mut h, Key::R, (0.0, 0.0), (100.0, 100.0));
    // Grab just outside the top-right corner and swing it up and around: shift snaps to 15°.
    let c = s(&h, 50., 50.);
    let a = s(&h, 110., -10.);
    let b = c + vec2(-80.0, -80.0); // 90° counter-clockwise from the start direction
    drag(&mut h, a, b, Modifiers::SHIFT);
    assert_eq!(h.state().doc.node(id).rotation, 90.0);
    assert_eq!(rect(&h, id), r(0., 0., 100., 100.), "rotates in place");
    // Right edge of the rotated box (its local max-x) now faces up: drag it up 50.
    let (p, q) = (s(&h, 50., 0.), s(&h, 50., -50.));
    drag(&mut h, p, q, Modifiers::NONE);
    let n = h.state().doc.node(id);
    assert_eq!(n.rect.size(), vec2(150., 100.), "{:?}", n.rect);
    assert_eq!(n.aabb(), r(0., -50., 100., 150.), "bottom edge stays planted");
}

#[test]
fn dropping_an_image_file_creates_an_image_layer() {
    let mut h = harness();
    let mut buf = std::io::Cursor::new(vec![]);
    image::RgbaImage::from_pixel(40, 30, image::Rgba([0, 200, 0, 255])).write_to(&mut buf, image::ImageFormat::Png).unwrap();
    let p = s(&h, 0., 0.);
    move_to(&mut h, p, Modifiers::NONE);
    h.input_mut().dropped_files.push(egui::DroppedFile {
        name: "leaf.png".into(),
        bytes: Some(buf.into_inner().into()),
        ..Default::default()
    });
    h.run_steps(2);
    let id = h.state().sel[0];
    let n = h.state().doc.node(id);
    assert_eq!(n.rect, r(-20., -15., 40., 30.));
    assert_eq!(n.fills[0].kind, crate::doc::PaintKind::Image);
    assert_eq!(h.state().doc.images.len(), 1);
    assert_eq!(n.name, "leaf.png");
}

#[test]
fn shift_a_wraps_in_auto_layout_and_drag_reorders() {
    use crate::doc::LayoutMode;
    let mut h = harness();
    let a = draw(&mut h, Key::R, (0.0, 0.0), (40.0, 40.0));
    let b = draw(&mut h, Key::R, (60.0, 0.0), (100.0, 40.0));
    let c = draw(&mut h, Key::R, (120.0, 0.0), (160.0, 40.0));
    h.state_mut().sel = vec![a, b, c];
    key(&mut h, Modifiers::SHIFT, Key::A);
    let f = h.state().sel[0];
    let fr = h.state().doc.node(f).clone();
    assert_eq!(fr.layout.mode, LayoutMode::Horizontal);
    assert_eq!(fr.children, vec![a, b, c]);
    assert_eq!(fr.rect, r(0., 0., 140., 40.), "hugs: 3x40 + 2x10 gap");
    // Drag `a` past `c`: it moves to the end and the others slide left.
    let (from, to) = (s(&h, 20., 20.), s(&h, 135., 20.));
    let p = s(&h, -300., 300.);
    click(&mut h, p, Modifiers::NONE);
    click(&mut h, from, Modifiers::NONE);
    assert_eq!(h.state().sel, vec![a], "children of a top-level frame are directly selectable");
    drag(&mut h, from, to, Modifiers::NONE);
    let fr = h.state().doc.node(f).clone();
    assert_eq!(fr.children, vec![b, c, a]);
    assert_eq!(rect(&h, b).min.x, 0.0);
    assert_eq!(rect(&h, a).min.x, 100.0);
    // Remove auto layout keeps positions.
    h.state_mut().sel = vec![f];
    key(&mut h, Modifiers::SHIFT | Modifiers::ALT, Key::A);
    assert_eq!(h.state().doc.node(f).layout.mode, LayoutMode::None);
    assert_eq!(rect(&h, a).min.x, 100.0);
}

#[test]
fn components_instances_via_shortcuts() {
    let mut h = harness();
    let rect_id = draw(&mut h, Key::R, (0.0, 0.0), (80.0, 40.0));
    key(&mut h, Modifiers::COMMAND | Modifiers::ALT, Key::K);
    let comp = h.state().sel[0];
    assert_eq!(h.state().doc.node(comp).kind, Kind::Component);
    assert_eq!(h.state().doc.node(comp).children, vec![rect_id]);
    // Alt-drag the main component: makes an instance.
    let (a, b) = (s(&h, 40., 20.), s(&h, 240., 20.));
    drag(&mut h, a, b, Modifiers::ALT);
    let inst = h.state().sel[0];
    assert_eq!(h.state().doc.node(inst).kind, Kind::Instance);
    assert_eq!(rect(&h, inst), r(200., 0., 80., 40.));
    // Edit the main's child fill through the model; the instance follows.
    h.state_mut().doc.node_mut(rect_id).fills[0].color = crate::doc::Rgba::rgb(1, 2, 3);
    h.state_mut().doc.reflow();
    let kid = h.state().doc.node(inst).children[0];
    assert_eq!(h.state().doc.node(kid).fills[0].color, crate::doc::Rgba::rgb(1, 2, 3));
    // Clicking inside an instance selects the instance; Cmd+Alt+B detaches it.
    let p = s(&h, -300., 300.);
    click(&mut h, p, Modifiers::NONE);
    let p = s(&h, 240., 20.);
    click(&mut h, p, Modifiers::NONE);
    assert_eq!(h.state().sel, vec![inst]);
    key(&mut h, Modifiers::COMMAND | Modifiers::ALT, Key::B);
    assert_eq!(h.state().doc.node(inst).kind, Kind::Frame);
}

#[test]
fn text_wraps_aligns_and_spaces_lines() {
    use crate::doc::{LineHeight, TextAlign, TextSizing};
    let mut h = harness();
    let mut n = Node::new(Kind::Text, r(0., 0., 60., 10.));
    n.text = "alpha beta gamma delta".into();
    n.font_size = 12.0;
    n.text_sizing = TextSizing::AutoHeight;
    n.line_height = LineHeight::Px(20.0);
    n.text_align = TextAlign::Center;
    let id = h.state_mut().doc.insert(0, None, None, n);
    h.run_steps(3);
    let t = h.state().doc.node(id).clone();
    assert_eq!(t.rect.width(), 60.0, "auto height keeps width");
    assert!(t.lines.len() >= 3, "wrapped into lines: {:?}", t.lines);
    assert_eq!(t.rect.height(), 20.0 * t.lines.len() as f32, "line height drives box height");
    let svg = crate::export::to_svg(&h.state().doc, &[id]);
    assert!(svg.contains(r#"text-anchor="middle""#) && svg.contains(r#"x="30""#), "{svg}");
    // Dragging the text tool makes a fixed box and starts editing.
    key(&mut h, Modifiers::NONE, Key::T);
    let (a, b) = (s(&h, 200., 0.), s(&h, 300., 80.));
    drag(&mut h, a, b, Modifiers::NONE);
    let t2 = h.state().sel[0];
    assert_eq!(h.state().doc.node(t2).text_sizing, TextSizing::Fixed);
    assert_eq!(rect(&h, t2), r(200., 0., 100., 80.));
    assert_eq!(h.state().editing_text, Some(t2));
    h.event(Event::Text("Hi".into()));
    h.run_steps(2);
    assert_eq!(rect(&h, t2), r(200., 0., 100., 80.), "fixed boxes keep their size");
}

#[test]
fn lists_indent_number_and_export() {
    use crate::doc::{ListKind, ListLine};
    let mut h = harness();
    let mut n = Node::new(Kind::Text, r(0., 0., 10., 10.));
    n.text = "Steps\nOne\nTwo".into();
    n.font_size = 10.0;
    let plain = Node { lists: vec![], ..n.clone() };
    let id = h.state_mut().doc.insert(0, None, None, n);
    let p = h.state_mut().doc.insert(0, None, None, plain);
    h.state_mut().sel = vec![id];
    h.run_steps(2);
    // Shift Cmd 7 numbers every paragraph of the selected layer.
    key(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::Num7);
    h.run_steps(2);
    let t = h.state().doc.node(id).clone();
    assert_eq!(t.lists, vec![ListLine { kind: ListKind::Number, indent: 0 }; 3]);
    assert_eq!(crate::text::list_markers(&t), vec!["1.", "2.", "3."]);
    assert!((t.rect.width() - h.state().doc.node(p).rect.width() - 15.0).abs() < 0.5, "auto width grows by the indent");
    let svg = crate::export::to_svg(&h.state().doc, &[id]);
    assert!(svg.contains(">2.</tspan>") && svg.contains(r#"x="15""#), "{svg}");
    // Undo restores plain text; a mixed list numbers only its own items.
    key(&mut h, Modifiers::COMMAND, Key::Z);
    assert!(h.state().doc.node(id).lists.is_empty());
    let n = h.state_mut().doc.node_mut(id);
    n.lists = vec![ListLine::default(), ListLine { kind: ListKind::Number, indent: 0 }, ListLine { kind: ListKind::Number, indent: 0 }];
    assert_eq!(crate::text::list_markers(n), vec!["", "1.", "2."]);
}

#[test]
fn paragraph_spacing_separates_paragraphs_only() {
    use crate::doc::{LineHeight, TextSizing};
    let mut h = harness();
    let mut n = Node::new(Kind::Text, r(0., 0., 400., 10.));
    n.text = "First paragraph\nSecond".into();
    n.font_size = 10.0;
    n.text_sizing = TextSizing::AutoHeight;
    n.line_height = LineHeight::Px(20.0);
    n.paragraph_spacing = 8.0;
    let id = h.state_mut().doc.insert(0, None, None, n);
    h.run_steps(3);
    assert_eq!(h.state().doc.node(id).rect.height(), 48.0, "two lines plus one paragraph gap");
    let svg = crate::export::to_svg(&h.state().doc, &[id]);
    let ys: Vec<f32> = svg.match_indices(" y=\"").map(|(i, _)| svg[i + 4..].split('"').next().unwrap().parse().unwrap()).collect();
    assert_eq!(ys.len(), 2, "{svg}");
    assert_eq!(ys[1] - ys[0], 28.0);
}

#[test]
fn truncated_text_stops_at_max_lines_with_ellipsis() {
    use crate::doc::{LineHeight, TextSizing};
    let mut h = harness();
    let mut n = Node::new(Kind::Text, r(0., 0., 60., 10.));
    n.text = "alpha beta gamma delta epsilon zeta".into();
    n.font_size = 12.0;
    n.text_sizing = TextSizing::AutoHeight;
    n.line_height = LineHeight::Px(20.0);
    n.truncate = true;
    n.max_lines = 2;
    let id = h.state_mut().doc.insert(0, None, None, n);
    h.run_steps(3);
    let t = h.state().doc.node(id).clone();
    assert_eq!(t.lines.len(), 2, "{:?}", t.lines);
    assert!(t.lines[1].ends_with('…'), "{:?}", t.lines);
    assert_eq!(t.rect.height(), 40.0);
    let css = crate::codegen::css(&h.state().doc, id);
    assert!(css.contains("-webkit-line-clamp: 2"), "{css}");
}

fn double_click(h: &mut Harness<App>, p: Pos2) {
    h.run_steps(30);
    move_to(h, p, Modifiers::NONE);
    for _ in 0..2 {
        press(h, p, true, Modifiers::NONE);
        press(h, p, false, Modifiers::NONE);
    }
    h.run_steps(2);
}

#[test]
fn vector_edit_moves_adds_and_deletes_points() {
    let mut h = harness();
    let id = draw(&mut h, Key::R, (0.0, 0.0), (100.0, 100.0));
    let p = s(&h, 50., 50.);
    double_click(&mut h, p);
    let v = h.state().sel[0];
    assert!(h.state().vector_edit.is_some(), "double-click enters vector edit");
    assert_ne!(v, id, "rectangle was converted to a path");
    assert_eq!(h.state().doc.node(v).paths[0].points.len(), 4);
    // Drag the bottom-right anchor out by (20, 10).
    let (a, b) = (s(&h, 100., 100.), s(&h, 120., 110.));
    drag(&mut h, a, b, Modifiers::NONE);
    assert_eq!(rect(&h, v), r(0., 0., 120., 110.));
    // Click the top edge to add an anchor, then delete it.
    let p = s(&h, 50., 0.);
    click(&mut h, p, Modifiers::NONE);
    assert_eq!(h.state().doc.node(v).paths[0].points.len(), 5);
    key(&mut h, Modifiers::NONE, Key::Backspace);
    assert_eq!(h.state().doc.node(v).paths[0].points.len(), 4);
    key(&mut h, Modifiers::NONE, Key::Escape);
    assert!(h.state().vector_edit.is_none());
    assert_eq!(h.state().sel, vec![v], "still selected after leaving edit mode");
}

#[test]
fn paste_svg_and_flatten() {
    let mut h = harness();
    h.event(Event::Paste(r##"<svg width="24" height="24" xmlns="http://www.w3.org/2000/svg"><circle cx="12" cy="12" r="10" fill="#f00"/></svg>"##.into()));
    h.run_steps(2);
    let f = h.state().sel[0];
    assert_eq!(h.state().doc.node(f).kind, Kind::Frame);
    assert_eq!(h.state().doc.node(f).rect.size(), vec2(24., 24.));
    // Flatten a rotated ellipse: one vector, same visual bounds.
    let e = draw(&mut h, Key::O, (100.0, 0.0), (160.0, 40.0));
    let c = h.state().doc.node(e).rect.center();
    h.state_mut().doc.rotate(e, 30.0, c);
    key(&mut h, Modifiers::COMMAND, Key::E);
    let v = h.state().sel[0];
    let n = h.state().doc.node(v);
    assert_eq!(n.kind, Kind::Vector);
    assert_eq!(n.rotation, 0.0, "rotation baked into the outline");
    // Analytic bounds of a 60x40 ellipse turned 30°, centered at (130, 20).
    let (hx, hy) = ((30f32 * 0.866).hypot(20. * 0.5), (30f32 * 0.5).hypot(20. * 0.866));
    let want = Rect::from_center_size(pos2(130., 20.), vec2(2. * hx, 2. * hy));
    assert!((n.rect.min - want.min).length() < 1.0 && (n.rect.max - want.max).length() < 1.0, "{:?} vs {want:?}", n.rect);
    assert!(!h.state().doc.nodes.contains_key(&e));
}

#[test]
fn boolean_shortcuts_and_live_updates() {
    let mut h = harness();
    let a = draw(&mut h, Key::R, (0.0, 0.0), (100.0, 100.0));
    let b = draw(&mut h, Key::O, (50.0, 50.0), (150.0, 150.0));
    h.state_mut().sel = vec![a, b];
    key(&mut h, Modifiers::SHIFT | Modifiers::ALT, Key::S);
    let g = h.state().sel[0];
    assert_eq!(h.state().doc.node(g).kind, Kind::Boolean);
    assert_eq!(h.state().doc.node(g).bool_op, crate::doc::BoolOp::Subtract);
    assert_eq!(rect(&h, g), r(0., 0., 100., 100.), "subtract result keeps the bottom square's bounds");
    // The bitten corner is empty: clicks pass through.
    let p = s(&h, -300., 300.);
    click(&mut h, p, Modifiers::NONE);
    let p = s(&h, 90., 90.);
    click(&mut h, p, Modifiers::NONE);
    assert!(h.state().sel.is_empty());
    let p = s(&h, 20., 20.);
    click(&mut h, p, Modifiers::NONE);
    assert_eq!(h.state().sel, vec![g], "solid area selects the boolean");
    // Moving an operand updates the result live.
    h.state_mut().doc.translate(b, vec2(100., 100.));
    h.state_mut().doc.reflow();
    let p = s(&h, 90., 90.);
    click(&mut h, p, Modifiers::NONE);
    assert_eq!(h.state().sel, vec![g]);
}

#[test]
fn arrangement_and_navigation_shortcuts() {
    let mut h = harness();
    let a = draw(&mut h, Key::R, (0.0, 0.0), (10.0, 10.0));
    let b = draw(&mut h, Key::O, (20.0, 0.0), (30.0, 10.0));
    let c = draw(&mut h, Key::R, (100.0, 0.0), (110.0, 10.0));
    h.state_mut().sel = vec![a, b, c];
    key(&mut h, Modifiers { alt: true, ctrl: true, ..Default::default() }, Key::H);
    assert_eq!(rect(&h, b).min.x, 50.0, "distributed");
    // Tab walks siblings; Shift+Enter selects the parent.
    h.state_mut().sel = vec![c];
    key(&mut h, Modifiers::NONE, Key::Tab);
    assert_eq!(h.state().sel, vec![b]);
    // Copy/paste properties.
    h.state_mut().doc.node_mut(a).fills[0].color = crate::doc::Rgba::rgb(9, 9, 9);
    h.state_mut().doc.node_mut(a).opacity = 0.5;
    h.state_mut().sel = vec![a];
    key(&mut h, Modifiers::COMMAND | Modifiers::ALT, Key::C);
    h.state_mut().sel = vec![c];
    key(&mut h, Modifiers::COMMAND | Modifiers::ALT, Key::V);
    assert_eq!(h.state().doc.node(c).fills[0].color, crate::doc::Rgba::rgb(9, 9, 9));
    assert_eq!(h.state().doc.node(c).opacity, 0.5);
    // Flip a group mirrors its children within the selection bounds.
    h.state_mut().sel = vec![a, c];
    key(&mut h, Modifiers::COMMAND, Key::G);
    key(&mut h, Modifiers::SHIFT, Key::H);
    assert_eq!(rect(&h, a).min.x, 100.0);
    assert_eq!(rect(&h, c).min.x, 0.0);
    h.state_mut().sel = vec![a];
    key(&mut h, Modifiers::SHIFT, Key::Enter);
    assert_eq!(h.state().doc.node(h.state().sel[0]).kind, Kind::Group);
    // Cmd+\ hides the panels; the canvas grows to the window.
    key(&mut h, Modifiers::COMMAND, Key::Backslash);
    assert!(h.state().ui_hidden);
    assert_eq!(h.state().canvas_rect.unwrap().width(), 1440.0);
}

#[test]
fn blur_spreads_and_preserves_energy() {
    let (w, h) = (21, 1);
    let mut px = vec![0u8; w * h * 4];
    for c in 0..4 {
        px[10 * 4 + c] = 210;
    }
    crate::render::blur_rgba(&mut px, w, h, 2.0);
    let alpha: Vec<u8> = px.chunks(4).map(|p| p[3]).collect();
    assert!(alpha[10] < 210 && alpha[10] > 0 && alpha[8] > 0, "{alpha:?}");
    assert!(alpha[10] >= alpha[8] && alpha[8] >= alpha[5], "peaked: {alpha:?}");
}

#[test]
fn autosave_and_recovery_roundtrip() {
    isolate_data();
    let mut a = App::new(None);
    a.doc.insert(0, None, None, Node::new(Kind::Rectangle, r(0., 0., 10., 10.)));
    a.dirty = true;
    a.autosave(100.0);
    // Written in the background.
    let mut left = vec![];
    for _ in 0..200 {
        left = crate::files::orphaned_recoveries(std::path::Path::new("none"));
        if !left.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(!left.is_empty(), "autosave written");
    // A new session sees the orphan and can recover it as unsaved work.
    let mut b = App::new(None);
    assert!(!b.recoverable.is_empty());
    let p = b.recoverable.iter().find(|p| left.contains(p)).unwrap().clone();
    b.recover(&p);
    assert_eq!(b.doc.nodes.len(), 1);
    assert!(b.dirty);
    assert!(!p.exists(), "old autosave re-homed");
    b.new_file();
    a.clear_recovery();
}

#[test]
fn new_tools_shapes_pencil_sections_slices_scale() {
    use crate::doc::Marker;
    let mut h = harness();
    // Arrow: Shift+L, drag.
    let a = draw(&mut h, Key::L, (0.0, 0.0), (100.0, 0.0));
    let _ = a;
    key(&mut h, Modifiers::SHIFT, Key::L);
    let (p, q) = (s(&h, 0., 50.), s(&h, 120., 50.));
    drag(&mut h, p, q, Modifiers::NONE);
    let arrow = h.state().sel[0];
    assert_eq!(h.state().doc.node(arrow).end_marker, Marker::Arrow);
    assert!(crate::export::to_svg(&h.state().doc, &[arrow]).matches("<path").count() >= 2, "line + arrowhead");
    // Polygon and star from the toolbar groups.
    h.state_mut().set_tool(Tool::Star);
    let (p, q) = (s(&h, 200., 0.), s(&h, 300., 100.));
    drag(&mut h, p, q, Modifiers::NONE);
    let star = h.state().sel[0];
    assert_eq!(h.state().doc.node(star).star_points().len(), 10);
    assert_eq!(h.state().doc.hit(0, pos2(250., 50.), 0.), Some(star));
    assert_eq!(h.state().doc.hit(0, pos2(205., 5.), 0.), None, "corners of the box are outside the star");
    // Pencil: freehand squiggle becomes a smooth vector with fewer anchors.
    key(&mut h, Modifiers::SHIFT, Key::P);
    let pts: Vec<Pos2> = (0..=40).map(|i| s(&h, i as f32 * 5., 200. + (i as f32 * 0.3).sin() * 20.)).collect();
    move_to(&mut h, pts[0], Modifiers::NONE);
    press(&mut h, pts[0], true, Modifiers::NONE);
    for p in &pts[1..] {
        move_to(&mut h, *p, Modifiers::NONE);
    }
    press(&mut h, pts[40], false, Modifiers::NONE);
    h.run_steps(2);
    let pen = h.state().sel[0];
    let pn = h.state().doc.node(pen);
    assert_eq!(pn.kind, Kind::Vector);
    assert!(pn.paths[0].points.len() < 30 && pn.paths[0].points.len() > 3, "{}", pn.paths[0].points.len());
    // Section around the star adopts it; slice exports what's under it.
    key(&mut h, Modifiers::SHIFT, Key::S);
    let (p, q) = (s(&h, 180., -20.), s(&h, 320., 120.));
    drag(&mut h, p, q, Modifiers::NONE);
    let sec = h.state().sel[0];
    assert_eq!(h.state().doc.node(sec).kind, Kind::Section);
    assert_eq!(h.state().doc.node(star).parent, Some(sec));
    key(&mut h, Modifiers::NONE, Key::S);
    let (p, q) = (s(&h, 190., -10.), s(&h, 310., 110.));
    drag(&mut h, p, q, Modifiers::NONE);
    let slice = h.state().sel[0];
    let svg = crate::export::region_svg(&h.state().doc, 0, h.state().doc.node(slice).aabb(), slice);
    assert!(svg.contains("<polygon"), "star inside the slice");
    // Scale tool doubles stroke weight with size.
    h.state_mut().doc.node_mut(arrow).stroke_width = 2.0;
    h.state_mut().sel = vec![arrow];
    key(&mut h, Modifiers::NONE, Key::K);
    let (p, q) = (s(&h, 120., 50.), s(&h, 240., 50.));
    drag(&mut h, p, q, Modifiers::SHIFT);
    let n = h.state().doc.node(arrow);
    assert!((n.stroke_width - 4.0).abs() < 0.1, "{}", n.stroke_width);
}

#[test]
fn prototype_connect_present_and_navigate() {
    use crate::doc::{Action, Trigger};
    let mut h = harness();
    let a = draw(&mut h, Key::F, (0.0, 0.0), (200.0, 150.0));
    let b = draw(&mut h, Key::F, (300.0, 0.0), (500.0, 150.0));
    let btn = draw(&mut h, Key::R, (20.0, 20.0), (80.0, 50.0));
    assert_eq!(h.state().doc.node(btn).parent, Some(a));
    // Prototype tab: drag the connect handle from the button onto frame B.
    h.state_mut().prototype_tab = true;
    h.state_mut().sel = vec![btn];
    h.run_steps(1);
    let (p, q) = (s(&h, 80., 35.), s(&h, 400., 75.));
    drag(&mut h, p, q, Modifiers::NONE);
    assert_eq!(h.state().doc.node(btn).reactions[0].action, Action::Navigate(b));
    // A label on top of the button, with no interaction of its own.
    let mut label = Node::new(Kind::Text, r(40., 25., 30., 20.));
    label.text = "Go".into();
    h.state_mut().doc.insert(0, Some(a), None, label);
    // B goes back after a short delay.
    h.state_mut().doc.node_mut(b).reactions.push(crate::doc::Reaction { trigger: Trigger::AfterDelay(500), action: Action::Back, ..Default::default() });
    // Present from A and click the button: lands on B.
    h.state_mut().sel = vec![a];
    key(&mut h, Modifiers::COMMAND | Modifiers::ALT, Key::Enter);
    assert_eq!(h.state().presenting.as_ref().map(|p| p.frame), Some(a));
    h.run_steps(2);
    let screen = h.state().presenting.as_ref().map(|_| ()).and(Some(egui::Rect::from_min_size(Pos2::ZERO, vec2(1440., 900.)))).unwrap();
    let view = crate::render::View::fit(h.state().doc.node(a).rect, screen, 1.0);
    let at = view.to_screen(pos2(50., 35.));
    click(&mut h, at, Modifiers::NONE);
    assert_eq!(h.state().presenting.as_ref().map(|p| p.frame), Some(b));
    // After the delay, Back returns to A; Esc leaves present mode.
    h.run_steps(40);
    assert_eq!(h.state().presenting.as_ref().map(|p| p.frame), Some(a));
    key(&mut h, Modifiers::NONE, Key::Escape);
    assert!(h.state().presenting.is_none());
}

/// Screen point of (x, y) in the frame being presented.
fn ps(h: &Harness<App>, x: f32, y: f32) -> Pos2 {
    let st = h.state();
    let frame = st.presenting.as_ref().expect("presenting").frame;
    st.presented_view(&st.presented(), frame, Rect::from_min_size(Pos2::ZERO, vec2(1440., 900.))).to_screen(pos2(x, y))
}

#[test]
fn prototype_change_to_swaps_variants_while_presenting() {
    use crate::doc::{Action, Reaction, Trigger};
    let mut h = harness();
    let st = h.state_mut();
    let comp = |st: &mut App, name: &str, x: f32| {
        let mut c = Node::new(Kind::Component, r(x, 400., 80., 40.));
        c.name = name.into();
        st.doc.insert(0, None, None, c)
    };
    let (a, b, c) = (comp(st, "State=Default", 0.), comp(st, "State=Hover", 100.), comp(st, "State=Pressed", 200.));
    st.doc.combine_as_variants(&[a, b, c]);
    // Hovering shows Hover; clicking Hover switches to Pressed.
    st.doc.node_mut(a).reactions.push(Reaction { trigger: Trigger::OnHover, action: Action::ChangeTo(b), ..Default::default() });
    st.doc.node_mut(b).reactions.push(Reaction { trigger: Trigger::OnClick, action: Action::ChangeTo(c), ..Default::default() });
    let frame = st.doc.insert(0, None, None, Node::new(Kind::Frame, r(0., 0., 300., 200.)));
    let inst = st.doc.instantiate(a, 0, Some(frame), None, pos2(20., 20.));
    assert_eq!(crate::prototype::sibling_variants(&st.doc, a), vec![a, b, c], "Change to is offered inside a variant");
    assert!(crate::prototype::sibling_variants(&st.doc, frame).is_empty());
    st.sel = vec![frame];
    st.present(0.0);
    h.run_steps(2);
    let variant = |h: &Harness<App>| h.state().presented().node(inst).component;
    let on = ps(&h, 60., 40.);
    move_to(&mut h, on, Modifiers::NONE);
    h.run_steps(1);
    assert_eq!(variant(&h), Some(b), "while hovering");
    let away = ps(&h, 250., 150.);
    move_to(&mut h, away, Modifiers::NONE);
    h.run_steps(1);
    assert_eq!(variant(&h), Some(a), "back when the pointer leaves");
    click(&mut h, on, Modifiers::NONE);
    assert_eq!(variant(&h), Some(c));
    assert_eq!(h.state().doc.node(inst).component, Some(a), "the file is untouched");
}

#[test]
fn prototype_enter_leave_key_and_drag_triggers() {
    use crate::doc::{Action, Easing, OverlayPos, OverlaySettings, Reaction, Transition, TransitionKind, Trigger};
    let mut h = harness();
    let st = h.state_mut();
    let a = st.doc.insert(0, None, None, Node::new(Kind::Frame, r(0., 0., 300., 200.)));
    let b = st.doc.insert(0, None, None, Node::new(Kind::Frame, r(400., 0., 300., 200.)));
    let tip = st.doc.insert(0, None, None, Node::new(Kind::Frame, r(800., 0., 100., 50.)));
    let btn = st.doc.insert(0, Some(a), None, Node::new(Kind::Rectangle, r(20., 20., 80., 40.)));
    let pad = st.doc.insert(0, Some(a), None, Node::new(Kind::Rectangle, r(150., 100., 100., 80.)));
    // A tooltip in the bottom-right corner while the pointer is on the button.
    let corner = OverlaySettings { position: OverlayPos::BottomRight, background: None, ..Default::default() };
    st.doc.node_mut(btn).reactions = vec![
        Reaction { trigger: Trigger::MouseEnter, action: Action::OpenOverlay(tip), overlay: corner, ..Default::default() },
        Reaction { trigger: Trigger::MouseLeave, action: Action::CloseOverlay, ..Default::default() },
    ];
    st.doc.node_mut(pad).reactions.push(Reaction { trigger: Trigger::OnDrag, action: Action::Navigate(b), ..Default::default() });
    let spring = Transition { kind: TransitionKind::SmartAnimate, easing: Easing::Bouncy, ..Default::default() };
    st.doc.node_mut(a).reactions.push(Reaction { trigger: Trigger::Key(Key::K, Modifiers::COMMAND), action: Action::Navigate(b), transition: spring, ..Default::default() });
    st.sel = vec![a];
    st.present(0.0);
    h.run_steps(2);
    let on = ps(&h, 60., 40.);
    move_to(&mut h, on, Modifiers::NONE);
    h.run_steps(1);
    let overlays = |h: &Harness<App>| h.state().presenting.as_ref().unwrap().overlays.clone();
    assert_eq!(overlays(&h).len(), 1, "mouse enter");
    assert_eq!(overlays(&h)[0].at, vec2(200., 150.), "bottom right of the frame");
    let off = ps(&h, 120., 150.);
    move_to(&mut h, off, Modifiers::NONE);
    h.run_steps(1);
    assert!(overlays(&h).is_empty(), "mouse leave");
    // Cmd+K navigates with a spring, which settles in its own time.
    key(&mut h, Modifiers::COMMAND, Key::K);
    assert_eq!(h.state().presenting.as_ref().map(|p| p.frame), Some(b));
    assert!(h.state().presenting.as_ref().unwrap().anim.is_some());
    h.run_steps(70);
    assert!(h.state().presenting.as_ref().unwrap().anim.is_none());
    // Restart, then drag the pad.
    key(&mut h, Modifiers::NONE, Key::R);
    assert_eq!(h.state().presenting.as_ref().map(|p| p.frame), Some(a));
    let (p, q) = (ps(&h, 160., 120.), ps(&h, 240., 120.));
    drag(&mut h, p, q, Modifiers::NONE);
    assert_eq!(h.state().presenting.as_ref().map(|p| p.frame), Some(b));
}

#[test]
fn prototype_scroll_to_fixed_layers_and_wheel() {
    use crate::doc::{Action, Overflow, Reaction, ScrollPos};
    let mut h = harness();
    let st = h.state_mut();
    let a = st.doc.insert(0, None, None, Node::new(Kind::Frame, r(0., 0., 200., 200.)));
    st.doc.node_mut(a).overflow = Overflow::Vertical;
    st.doc.insert(0, Some(a), None, Node::new(Kind::Rectangle, r(0., 0., 200., 600.)));
    let target = st.doc.insert(0, Some(a), None, Node::new(Kind::Rectangle, r(0., 400., 200., 50.)));
    let bar = st.doc.insert(0, Some(a), None, Node::new(Kind::Rectangle, r(0., 170., 200., 30.)));
    st.doc.node_mut(bar).scroll_pos = ScrollPos::Fixed;
    let go = st.doc.insert(0, Some(a), None, Node::new(Kind::Rectangle, r(10., 10., 50., 30.)));
    st.doc.node_mut(go).reactions.push(Reaction { action: Action::ScrollTo(target), ..Default::default() });
    st.sel = vec![a];
    st.present(0.0);
    h.run_steps(2);
    let at = ps(&h, 30., 20.);
    click(&mut h, at, Modifiers::NONE);
    let offset = |h: &Harness<App>| h.state().presenting.as_ref().unwrap().scroll.get(&a).copied().unwrap_or_default();
    assert_eq!(offset(&h), vec2(0., 400.), "scrolled to the target, as far as the content goes");
    let shown = h.state().presented();
    assert_eq!(shown.node(target).rect.min.y, 0.);
    assert_eq!(shown.node(bar).rect.min.y, 170., "fixed layers stay put");
    // The wheel scrolls back up.
    for _ in 0..4 {
        h.event(Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: vec2(0.0, 50.0), modifiers: Modifiers::NONE, phase: egui::TouchPhase::Move });
        h.run_steps(3);
    }
    h.run_steps(20);
    let y = offset(&h).y;
    assert!(y < 400. && y >= 0., "{y}");
}

#[test]
fn prototype_overlay_settings_swap_overlay_and_set_variable() {
    use crate::doc::{Action, Bindable, OverlayPos, OverlaySettings, Reaction, Rgba, VarValue};
    let mut h = harness();
    let st = h.state_mut();
    let m = st.doc.insert(0, None, None, Node::new(Kind::Frame, r(0., 0., 300., 200.)));
    let menu = st.doc.insert(0, None, None, Node::new(Kind::Frame, r(800., 300., 100., 60.)));
    let next = st.doc.insert(0, Some(menu), None, Node::new(Kind::Rectangle, r(810., 310., 30., 30.)));
    let sheet = st.doc.insert(0, None, None, Node::new(Kind::Frame, r(800., 500., 80., 80.)));
    let swatch = st.doc.insert(0, Some(m), None, Node::new(Kind::Rectangle, r(10., 10., 40., 40.)));
    let red = st.doc.insert(0, Some(m), None, Node::new(Kind::Rectangle, r(60., 10., 40., 40.)));
    let open = st.doc.insert(0, Some(m), None, Node::new(Kind::Rectangle, r(110., 10., 40., 40.)));
    let c = st.doc.add_collection("Theme".into());
    let var = st.doc.add_variable(c, "accent".into(), VarValue::Color(Rgba::WHITE));
    st.doc.node_mut(swatch).bindings.push((Bindable::Fill, var));
    st.doc.reflow();
    st.doc.node_mut(red).reactions.push(Reaction { action: Action::SetVariable(var, VarValue::Color(Rgba::rgb(255, 0, 0))), ..Default::default() });
    // A menu just under the button that stays open on outside clicks.
    let below = OverlaySettings { position: OverlayPos::Manual, offset: vec2(0., 45.), close_outside: false, background: None };
    st.doc.node_mut(open).reactions.push(Reaction { action: Action::OpenOverlay(menu), overlay: below, ..Default::default() });
    st.doc.node_mut(next).reactions.push(Reaction { action: Action::SwapOverlay(sheet), ..Default::default() });
    st.sel = vec![m];
    st.present(0.0);
    h.run_steps(2);
    let at = ps(&h, 80., 30.);
    click(&mut h, at, Modifiers::NONE);
    assert_eq!(h.state().presented().node(swatch).fills[0].color, Rgba::rgb(255, 0, 0), "bound fill follows the variable");
    assert_eq!(h.state().doc.node(swatch).fills[0].color, Rgba::WHITE, "the file is untouched");
    let at = ps(&h, 130., 30.);
    click(&mut h, at, Modifiers::NONE);
    let overlays = |h: &Harness<App>| h.state().presenting.as_ref().unwrap().overlays.clone();
    assert_eq!(overlays(&h)[0].at, vec2(110., 55.), "offset from the trigger");
    let outside = ps(&h, 20., 180.);
    click(&mut h, outside, Modifiers::NONE);
    assert_eq!(overlays(&h).len(), 1, "stays open on outside clicks");
    let at = ps(&h, 110. + 25., 55. + 25.);
    click(&mut h, at, Modifiers::NONE);
    let shown = overlays(&h);
    assert_eq!((shown.len(), shown[0].id, shown[0].at), (1, sheet, vec2(110., 60.)), "swapped for a centered sheet");
}

#[test]
fn prototype_presents_inside_a_device() {
    use crate::doc::{Action, Device, Reaction};
    let mut h = harness();
    let st = h.state_mut();
    st.doc.pages[0].prototype.device = Device::IPhone16;
    let a = st.doc.insert(0, None, None, Node::new(Kind::Frame, r(0., 0., 393., 852.)));
    let b = st.doc.insert(0, None, None, Node::new(Kind::Frame, r(500., 0., 393., 852.)));
    let btn = st.doc.insert(0, Some(a), None, Node::new(Kind::Rectangle, r(20., 700., 100., 40.)));
    st.doc.node_mut(btn).reactions.push(Reaction { action: Action::Navigate(b), ..Default::default() });
    st.sel = vec![a];
    st.present(0.0);
    h.run_steps(2);
    let window = Rect::from_min_size(Pos2::ZERO, vec2(1440., 900.));
    let v = h.state().presented_view(&h.state().doc, a, window);
    assert!(v.zoom < 1.0, "the phone scales down to fit");
    assert!(window.contains_rect(v.rect_to_screen(rect(&h, a))));
    let at = ps(&h, 70., 720.);
    click(&mut h, at, Modifiers::NONE);
    assert_eq!(h.state().presenting.as_ref().map(|p| p.frame), Some(b));
}

#[test]
fn prototype_key_trigger_is_set_by_pressing_it() {
    use crate::doc::{Action, Reaction, Trigger};
    let mut h = harness();
    let a = draw(&mut h, Key::F, (0.0, 0.0), (200.0, 150.0));
    let r0 = Reaction { trigger: Trigger::Key(Key::Space, Modifiers::NONE), action: Action::Back, ..Default::default() };
    h.state_mut().doc.node_mut(a).reactions.push(r0.clone());
    h.state_mut().prototype_tab = true;
    h.state_mut().sel = vec![a];
    h.run_steps(2);
    let row = crate::prototype::describe(&h.state().doc, &r0);
    h.get_by_label(&row).click();
    h.run_steps(2);
    h.get_by_label(&crate::prototype::key_label(Key::Space, Modifiers::NONE)).click();
    h.run_steps(2);
    let tool = h.state().tool;
    key(&mut h, Modifiers::SHIFT, Key::K);
    assert_eq!(h.state().doc.node(a).reactions[0].trigger, Trigger::Key(Key::K, Modifiers::SHIFT));
    assert!(h.state().tool == tool, "the key went to the trigger, not a shortcut");
}

#[test]
fn saves_keep_versions() {
    isolate_data();
    let file = std::env::temp_dir().join(format!("katachi-versions-{}.katachi", std::process::id()));
    crate::files::save_version(&file, b"{\"katachi\":1}");
    std::thread::sleep(std::time::Duration::from_millis(5));
    crate::files::save_version(&file, b"{\"katachi\":2}");
    let v = crate::files::versions(&file);
    assert_eq!(v.len(), 2);
    assert_eq!(std::fs::read_to_string(&v[0].0).unwrap(), "{\"katachi\":2}", "newest first");
}

#[test]
fn comments_pin_post_and_follow_their_frame() {
    let mut h = harness();
    let f = draw(&mut h, Key::F, (0.0, 0.0), (200.0, 200.0));
    key(&mut h, Modifiers::NONE, Key::C);
    let p = s(&h, 50., 50.);
    click(&mut h, p, Modifiers::NONE);
    assert!(h.state().draft_comment.is_some());
    h.event(Event::Text("Looks great".into()));
    h.run_steps(2);
    key(&mut h, Modifiers::COMMAND, Key::Enter);
    let c = h.state().doc.comments.first().cloned().expect("posted");
    assert_eq!(c.anchor, Some(f));
    assert_eq!(c.messages[0].1, "Looks great");
    assert_eq!(h.state().doc.comment_pos(&c), pos2(50., 50.));
    h.state_mut().doc.translate(f, vec2(100., 0.));
    assert_eq!(h.state().doc.comment_pos(&c), pos2(150., 50.), "pin moves with its frame");
}

/// `cargo test --release perf -- --ignored --nocapture`
#[test]
#[ignore]
fn perf_large_document() {
    let mut h = Harness::builder().with_size(vec2(1440.0, 900.0)).with_step_dt(1.0 / 60.0).wgpu().build_eframe(|cc| {
        crate::setup_style(&cc.egui_ctx);
        App::new(None)
    });
    let st = h.state_mut();
    for f in 0..50 {
        let (fx, fy) = ((f % 10) as f32 * 600.0, (f / 10) as f32 * 600.0);
        let frame = st.doc.insert(0, None, None, Node::new(Kind::Frame, r(fx, fy, 560., 560.)));
        for i in 0..100 {
            let (x, y) = (fx + (i % 10) as f32 * 55.0, fy + (i / 10) as f32 * 55.0);
            st.doc.insert(0, Some(frame), None, Node::new(Kind::Rectangle, r(x, y, 50., 50.)));
        }
        for i in 0..10 {
            let mut t = Node::new(Kind::Text, r(fx + 5., fy + i as f32 * 50. + 5., 10., 10.));
            t.text = format!("Label {f}-{i}");
            st.doc.insert(0, Some(frame), None, t);
        }
    }
    st.doc.reflow();
    h.run_steps(3);
    let t = std::time::Instant::now();
    h.run_steps(30);
    let idle = t.elapsed() / 30;
    // Drag a frame around: every frame restores the base doc, moves, and reflows.
    let p = h.state().view.to_screen(pos2(10., 10.));
    let t = std::time::Instant::now();
    drag(&mut h, p, p + vec2(200., 100.), Modifiers::NONE);
    let drag_time = t.elapsed() / 10;
    let t = std::time::Instant::now();
    h.state_mut().doc.reflow();
    let reflow = t.elapsed();
    eprintln!("nodes={} idle frame={idle:?} drag frame≈{drag_time:?} reflow={reflow:?}", h.state().doc.nodes.len());
}

/// Render SVGs in $KATACHI_RENDER_DIR to PNGs with the app's own resvg pipeline.
#[test]
#[ignore]
fn render_svgs() {
    let Some(dir) = std::env::var_os("KATACHI_RENDER_DIR") else { return };
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "svg") {
            let svg = std::fs::read_to_string(&p).unwrap();
            let scale: f32 = std::env::var("KATACHI_RENDER_SCALE").ok().and_then(|s| s.parse().ok()).unwrap_or(0.35);
            let (px, size) = crate::export::rasterize(&svg, scale).unwrap();
            let img = image::RgbaImage::from_fn(size[0] as u32, size[1] as u32, |x, y| {
                let i = (y as usize * size[0] + x as usize) * 4;
                let a = px[i + 3] as u32;
                let un = |c: u8| if a == 0 { 0 } else { ((c as u32 * 255) / a).min(255) as u8 };
                image::Rgba([un(px[i]), un(px[i + 1]), un(px[i + 2]), px[i + 3]])
            });
            img.save(p.with_extension("png")).unwrap();
        }
    }
}

#[test]
fn rich_text_editing_styles_selection() {
    let mut h = harness();
    let mut t = Node::new(Kind::Text, r(0., 0., 10., 10.));
    t.text = "Hello world".into();
    t.font_size = 20.;
    let id = h.state_mut().doc.insert(0, None, None, t);
    h.state_mut().sel = vec![id];
    h.state_mut().editing_text = Some(id);
    h.state_mut().focus_text = true;
    h.run_steps(3);
    // Select "world" in the live editor.
    let edit_id = egui::Id::new("katachi-text-edit");
    let mut state = egui::TextEdit::load_state(&h.ctx, edit_id).expect("editor state");
    state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(6), egui::text::CCursor::new(11))));
    state.store(&h.ctx, edit_id);
    let ctx = h.ctx.clone();
    assert_eq!(h.state().text_selection(&ctx), Some((6, 11)));
    let bold = crate::doc::SpanStyle { font_weight: Some(700), color: Some(crate::doc::Rgba::rgb(242, 72, 34)), ..Default::default() };
    assert!(h.state_mut().style_selection(&ctx, bold.clone()));
    assert_eq!(h.state().doc.node(id).spans, vec![crate::doc::TextSpan { start: 6, end: 11, style: bold.clone() }]);
    // Keep typing at the end: the bold run grows with it.
    let mut state = egui::TextEdit::load_state(&h.ctx, edit_id).unwrap();
    state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(11))));
    state.store(&h.ctx, edit_id);
    h.event(Event::Text("!".into()));
    h.run_steps(2);
    let n = h.state().doc.node(id);
    assert_eq!(n.text, "Hello world!");
    assert_eq!((n.spans[0].start, n.spans[0].end), (6, 12));
}

#[test]
fn multiplayer_host_join_edit_and_undo_only_mine() {
    let mut a = harness();
    let mut b = harness();
    a.state_mut().collab_port = 0;
    a.state_mut().collab_name = "Ada".into();
    let ctx = a.ctx.clone();
    a.state_mut().host_session(&ctx);
    let port = a.state().collab.as_ref().expect("hosting").address.rsplit(':').next().unwrap().to_string();
    b.state_mut().collab_name = "Bo".into();
    let ctx = b.ctx.clone();
    b.state_mut().join_session(&ctx, &format!("127.0.0.1:{port}"), false);
    assert!(b.state().collab.is_some(), "joined");
    // Ada draws; Bo sees it. Bo draws; Ada sees it.
    let ra = draw(&mut a, Key::R, (0.0, 0.0), (100.0, 100.0));
    let rb = draw(&mut b, Key::O, (200.0, 0.0), (260.0, 60.0));
    // Up to 5 s for the sockets (tests run in parallel and the machine may be busy).
    let settle = |a: &mut Harness<App>, b: &mut Harness<App>, done: &dyn Fn(&App, &App) -> bool| {
        for _ in 0..1000 {
            a.step();
            b.step();
            if done(a.state(), b.state()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    };
    settle(&mut a, &mut b, &|a, b| b.doc.nodes.contains_key(&ra) && a.doc.nodes.contains_key(&rb) && a.collab.as_ref().is_some_and(|c| c.peers.values().any(|p| p.name == "Bo")));
    let dump = |h: &Harness<App>| {
        let c = h.state().collab.as_ref();
        format!("nodes {:?} connected {:?} client {:?}", h.state().doc.nodes.keys().collect::<Vec<_>>(), c.map(|c| c.connected), c.map(|c| c.client))
    };
    assert!(b.state().doc.nodes.contains_key(&ra), "Bo sees Ada's rectangle {ra}: Ada {} / Bo {}", dump(&a), dump(&b));
    assert!(a.state().doc.nodes.contains_key(&rb), "Ada sees Bo's ellipse");
    assert!(a.state().collab.as_ref().unwrap().peers.values().any(|p| p.name == "Bo"), "presence");
    // Bo is mid-drag (moving his ellipse) when Ada draws: her new layer survives the drag on both sides.
    let (p0, p1) = (s(&b, 230., 30.), s(&b, 330., 60.));
    move_to(&mut b, p0, Modifiers::NONE);
    press(&mut b, p0, true, Modifiers::NONE);
    move_to(&mut b, p0.lerp(p1, 0.5), Modifiers::NONE);
    let rc = draw(&mut a, Key::R, (0.0, 300.0), (80.0, 380.0));
    settle(&mut a, &mut b, &|_, b| b.doc.nodes.contains_key(&rc));
    move_to(&mut b, p1, Modifiers::NONE);
    press(&mut b, p1, false, Modifiers::NONE);
    settle(&mut a, &mut b, &|a, b| a.doc.node(rb).rect == b.doc.node(rb).rect);
    assert!(a.state().doc.nodes.contains_key(&rc) && b.state().doc.nodes.contains_key(&rc), "drawn during Bo's drag");
    assert_eq!(a.state().doc.node(rb).rect, b.state().doc.node(rb).rect, "Bo's move reached Ada");
    // Ada undoes: her latest rectangle goes away everywhere; her first one and Bo's ellipse stay.
    key(&mut a, Modifiers::COMMAND, Key::Z);
    settle(&mut a, &mut b, &|a, b| !a.doc.nodes.contains_key(&rc) && !b.doc.nodes.contains_key(&rc));
    assert!(!a.state().doc.nodes.contains_key(&rc) && !b.state().doc.nodes.contains_key(&rc));
    assert!(a.state().doc.nodes.contains_key(&ra) && b.state().doc.nodes.contains_key(&ra));
    assert!(a.state().doc.nodes.contains_key(&rb) && b.state().doc.nodes.contains_key(&rb), "undo leaves others' work");
}

/// `KATACHI_PERF_FILE=x.katachi cargo test --release perf_file -- --ignored --nocapture`
#[test]
#[ignore]
fn perf_file() {
    let Some(f) = std::env::var_os("KATACHI_PERF_FILE") else { return };
    let t = std::time::Instant::now();
    let mut h = Harness::builder().with_size(vec2(1440.0, 900.0)).with_step_dt(1.0 / 60.0).build_eframe(move |cc| {
        crate::setup_style(&cc.egui_ctx);
        App::new(Some(std::path::PathBuf::from(&f)))
    });
    eprintln!("open: {:?}, nodes {}", t.elapsed(), h.state().doc.nodes.len());
    let pages: Vec<usize> = std::env::var("KATACHI_PERF_PAGES").ok().map(|p| p.split(',').filter_map(|x| x.parse().ok()).collect()).unwrap_or_else(|| (0..h.state().doc.pages.len().min(12)).collect());
    for page in pages {
        h.state_mut().select_page(page);
        h.run_steps(2);
        let t = std::time::Instant::now();
        h.run_steps(10);
        eprintln!("page {page} '{}': frame {:?}", h.state().doc.pages[page].name, t.elapsed() / 10);
    }
    let t = std::time::Instant::now();
    h.state_mut().doc.reflow();
    eprintln!("reflow {:?}", t.elapsed());
}



#[test]
fn selection_colors_recolor_every_use() {
    use crate::doc::{Paint, Rgba};
    let mut h = harness();
    let mut f = Node::new(Kind::Frame, r(0., 0., 200., 100.));
    f.fills = vec![Paint::solid(Rgba::rgb(255, 255, 255))];
    let f = h.state_mut().doc.insert(0, None, None, f);
    let mut ids = vec![];
    for (x, c) in [(10., Rgba::rgb(255, 0, 0)), (60., Rgba::rgb(0, 0, 255)), (110., Rgba::rgb(255, 0, 0))] {
        let mut k = Node::new(Kind::Rectangle, r(x, 10., 40., 40.));
        k.fills = vec![Paint::solid(c)];
        ids.push(h.state_mut().doc.insert(0, Some(f), None, k));
    }
    h.state_mut().sel = vec![f];
    h.run_steps(3);
    h.get_all_by_value("FF0000").next().unwrap().focus();
    h.run_steps(1);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.get_all_by_value("FF0000").next().unwrap().type_text("00FF00");
    h.run_steps(1);
    h.key_press(Key::Enter);
    h.run_steps(2);
    let d = &h.state().doc;
    assert_eq!(d.node(ids[0]).fills[0].color, Rgba::rgb(0, 255, 0));
    assert_eq!(d.node(ids[2]).fills[0].color, Rgba::rgb(0, 255, 0));
    assert_eq!(d.node(ids[1]).fills[0].color, Rgba::rgb(0, 0, 255), "other colors untouched");
}

#[test]
fn select_matching_and_paste_to_replace() {
    use crate::doc::{Paint, Rgba};
    let mut h = harness();
    let add = |h: &mut Harness<App>, kind, rect, c| {
        let mut n = Node::new(kind, rect);
        n.fills = vec![Paint::solid(c)];
        h.state_mut().doc.insert(0, None, None, n)
    };
    let red = Rgba::rgb(255, 0, 0);
    let a = add(&mut h, Kind::Rectangle, r(0., 0., 20., 20.), red);
    let b = add(&mut h, Kind::Rectangle, r(40., 0., 30., 30.), red);
    let c = add(&mut h, Kind::Rectangle, r(80., 0., 20., 20.), Rgba::rgb(0, 0, 255));
    let e = add(&mut h, Kind::Ellipse, r(0., 100., 10., 10.), red);
    h.state_mut().sel = vec![a];
    key(&mut h, Modifiers::COMMAND | Modifiers::ALT, Key::A);
    let mut sel = h.state().sel.clone();
    sel.sort();
    assert_eq!(sel, vec![a, b], "same type and look, any size");
    // Copy the ellipse, replace the blue rectangle with it.
    h.state_mut().sel = vec![e];
    h.event(Event::Copy);
    h.step();
    h.state_mut().sel = vec![c];
    key(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::R);
    let d = &h.state().doc;
    assert!(!d.nodes.contains_key(&c));
    let new = h.state().sel[0];
    assert_eq!(d.node(new).kind, Kind::Ellipse);
    assert_eq!(d.node(new).rect.center(), pos2(90., 10.), "centered where the replaced layer was");
}


/// `KATACHI_BENCH_FILE=big.katachi cargo test --release perf_bench -- --ignored --nocapture`:
/// frame times on a real file (each page fitted, idle, panning, hovering), plus open, undo
/// checkpoint and save costs.
#[test]
#[ignore]
fn perf_bench() {
    use std::time::Instant;
    let Some(file) = std::env::var_os("KATACHI_BENCH_FILE") else { return };
    let mut h = harness();
    let t = Instant::now();
    h.state_mut().open_path(file.into());
    eprintln!("open {:?} ({} nodes, {} pages)", t.elapsed(), h.state().doc.nodes.len(), h.state().doc.pages.len());
    let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.0;
    let frame = |h: &mut Harness<'static, App>| {
        let t = Instant::now();
        h.step();
        ms(t.elapsed())
    };
    for p in 0..h.state().doc.pages.len() {
        h.state_mut().page = p;
        h.state_mut().view_ready = false;
        let first = frame(&mut h);
        let idle: f64 = (0..10).map(|_| frame(&mut h)).sum::<f64>() / 10.0;
        let pan: f64 = (0..10)
            .map(|i| {
                h.event(Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: vec2(0.0, if i % 2 == 0 { 40.0 } else { -40.0 }), modifiers: Modifiers::NONE, phase: egui::TouchPhase::Move });
                frame(&mut h)
            })
            .sum::<f64>()
            / 10.0;
        let hover: f64 = (0..10)
            .map(|i| {
                h.event(Event::PointerMoved(pos2(400.0 + i as f32 * 50.0, 300.0 + i as f32 * 30.0)));
                frame(&mut h)
            })
            .sum::<f64>()
            / 10.0;
        let name = h.state().doc.pages[p].name.clone();
        let n = h.state().doc.pages[p].children.len();
        eprintln!("page {p} {name:>32} ({n:>3} top): first {first:7.1} ms  idle {idle:6.1}  pan {pan:6.1}  hover {hover:6.1}");
    }
    let rss = || {
        let out = std::process::Command::new("ps").args(["-o", "rss=", "-p", &std::process::id().to_string()]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().parse::<u64>().unwrap_or(0) / 1024
    };
    let before = rss();
    let t = Instant::now();
    for _ in 0..50 {
        h.state_mut().checkpoint();
    }
    eprintln!("undo checkpoint {:?} each, 50 steps: +{} MB", t.elapsed() / 50, rss().saturating_sub(before));
    // A real editing step: checkpoint, move a layer, reflow, draw.
    h.state_mut().page = 2;
    let id = h.state().doc.pages[2].children[0];
    let before = rss();
    let t = Instant::now();
    for i in 0..50 {
        let a = h.state_mut();
        a.checkpoint();
        let d = if i % 2 == 0 { 10.0 } else { -10.0 };
        a.doc.node_mut(id).rect = a.doc.node(id).rect.translate(vec2(d, 0.0));
        a.doc.reflow();
        h.step();
    }
    eprintln!("edit + reflow + frame + checkpoint {:?} each, 50 steps: +{} MB", t.elapsed() / 50, rss().saturating_sub(before));
    // Which part copies layers: count layers no longer shared with the last undo snapshot.
    let shared = |a: &App| a.doc.nodes.iter().filter(|(_, n)| std::sync::Arc::strong_count(n) > 1).count();
    h.state_mut().checkpoint();
    let n0 = shared(h.state());
    let t = Instant::now();
    h.state_mut().doc.reflow();
    eprintln!("reflow {:?}", t.elapsed());
    let n1 = shared(h.state());
    h.step();
    let n2 = shared(h.state());
    eprintln!("shared layers: after checkpoint {n0}, after reflow {n1}, after a frame {n2}");
    // Reflow should be idempotent: name the layers a second pass still changes.
    let before = h.state().doc.nodes.clone();
    h.state_mut().doc.reflow();
    eprintln!("a second reflow still changes {} layers", h.state().doc.nodes.iter().filter(|(id, n)| !std::sync::Arc::ptr_eq(n, &before[id])).count());
    let t = Instant::now();
    let json = crate::doc::file_json(&h.state().doc);
    eprintln!("serialize {:?} ({} MB)", t.elapsed(), json.len() >> 20);
    let t = Instant::now();
    let path = std::env::temp_dir().join("katachi-bench-save.katachi");
    h.state_mut().save_to(path);
    eprintln!("save {:?}", t.elapsed());
}

#[test]
fn compact_files_load_back_identical() {
    let mut doc = crate::doc::Document::default();
    crate::plugins::run(&mut doc, 0, &[], include_str!("../assets/promo/showcase.js")).unwrap();
    // Instances carry a snapshot of their main component, which is compacted too.
    crate::plugins::run(&mut doc, 0, &[], r#"const set = figma.currentPage.findOne(n => n.name === "Button"); const i = set.children[1].createInstance(); i.x = 40; i.y = 2000;"#).unwrap();
    let compact = crate::doc::file_json(&doc);
    assert!(!compact.contains("inner_ratio") && !compact.contains("\"letter_spacing\":0"), "defaults are left out");
    let back = crate::doc::parse_file(&compact).unwrap();
    assert_eq!(serde_json::to_value(&back).unwrap(), serde_json::to_value(&doc).unwrap());
    // Older files spelled out every field; missing ones load as defaults either way.
    let old = r#"{"katachi":1,"document":{"pages":[{"name":"Page 1","children":[1],"background":[245,245,245,255]}],"nodes":{"1":{"id":1,"name":"R","kind":"Rectangle","opacity":1.0,"inner_ratio":0.382,"rect":{"min":{"x":0,"y":0},"max":{"x":10,"y":10}}}},"next_id":2}}"#;
    let d = crate::doc::parse_file(old).unwrap();
    assert_eq!(d.nodes[&1].opacity, 1.0);
    assert!(d.nodes[&1].visible);
}

#[test]
fn renaming_a_layer_hands_the_keyboard_back() {
    let mut h = harness();
    let id = draw(&mut h, Key::R, (0.0, 0.0), (100.0, 100.0));
    for (finish, expect) in [(Key::Enter, "Card"), (Key::Escape, "Card")] {
        h.state_mut().renaming = Some((id, h.state().doc.node(id).name.clone()));
        h.run_steps(2);
        h.key_press_modifiers(Modifiers::COMMAND, Key::A);
        h.event(Event::Text(if finish == Key::Enter { "Card".into() } else { "Nope".into() }));
        h.run_steps(2);
        key(&mut h, Modifiers::NONE, finish);
        h.run_steps(2);
        assert_eq!(h.state().doc.node(id).name, expect, "Enter commits, Escape cancels");
        assert!(h.state().renaming.is_none());
        // Shortcuts reach the canvas again.
        key(&mut h, Modifiers::NONE, Key::O);
        assert_eq!(h.state().tool, Tool::Ellipse, "after {finish:?}");
        key(&mut h, Modifiers::NONE, Key::V);
    }
}

#[test]
fn figma_details_from_the_audit() {
    let mut h = harness();
    // Text layers are named after their text until renamed.
    key(&mut h, Modifiers::NONE, Key::T);
    let at = s(&h, -300.0, -200.0);
    click(&mut h, at, Modifiers::NONE);
    h.event(Event::Text("ATLANTA".into()));
    h.run_steps(2);
    let t = h.state().editing_text.expect("editing");
    assert_eq!(h.state().doc.node(t).name, "ATLANTA");
    key(&mut h, Modifiers::NONE, Key::Escape);
    h.state_mut().doc.node_mut(t).name = "City".into();
    h.state_mut().editing_text = Some(t);
    h.run_steps(2);
    h.event(Event::Text("!".into()));
    h.run_steps(2);
    assert_eq!(h.state().doc.node(t).name, "City", "a hand-made name sticks");
    key(&mut h, Modifiers::NONE, Key::Escape);

    // Closing a pen path inside a frame leaves the path selected.
    let f = draw(&mut h, Key::F, (0.0, 0.0), (300.0, 300.0));
    key(&mut h, Modifiers::NONE, Key::P);
    for (x, y) in [(50.0, 50.0), (200.0, 60.0), (120.0, 200.0)] {
        let p = s(&h, x, y);
        move_to(&mut h, p, Modifiers::NONE);
        press(&mut h, p, true, Modifiers::NONE);
        press(&mut h, p, false, Modifiers::NONE);
    }
    let first = s(&h, 50.0, 50.0);
    move_to(&mut h, first, Modifiers::NONE);
    press(&mut h, first, true, Modifiers::NONE);
    press(&mut h, first, false, Modifiers::NONE);
    h.run_steps(2);
    let sel = h.state().sel.clone();
    assert_eq!(sel.len(), 1);
    assert_eq!(h.state().doc.node(sel[0]).kind, Kind::Vector, "the new path, not its frame");
    assert_eq!(h.state().doc.node(sel[0]).parent, Some(f));
    key(&mut h, Modifiers::NONE, Key::Escape);
    assert_eq!((h.state().tool, h.state().sel.clone()), (Tool::Move, sel), "Escape leaves the pen with the path still selected");

    // Frame presets.
    let ctx = h.ctx.clone();
    h.state_mut().add_preset_frame(&ctx, "iPhone 16", 393.0, 852.0);
    let id = h.state().sel[0];
    let n = h.state().doc.node(id);
    assert_eq!((n.name.as_str(), n.rect.width(), n.rect.height(), n.kind), ("iPhone 16", 393.0, 852.0, Kind::Frame));
    assert!(n.rect.min.x >= 400.0, "placed beside what's already there");

    // Wrapping layers in auto layout makes a transparent frame.
    h.run_steps(2);
    let c = h.state().canvas_rect.unwrap().center();
    h.state_mut().view = crate::render::View { zoom: 1.0, pan: c.to_vec2() - vec2(60.0, 1225.0) };
    let a = draw(&mut h, Key::R, (0.0, 1200.0), (50.0, 1250.0));
    let b = draw(&mut h, Key::R, (80.0, 1200.0), (130.0, 1250.0));
    h.state_mut().sel = vec![a, b];
    key(&mut h, Modifiers::SHIFT, Key::A);
    let w = h.state().sel[0];
    assert_eq!(h.state().doc.node(w).layout.mode, crate::doc::LayoutMode::Horizontal);
    assert!(h.state().doc.node(w).fills.is_empty());
}

#[test]
fn instance_overrides_survive_styles_and_variables_on_the_main_component() {
    for via_style in [false, true] {
        let mut doc = crate::doc::Document::default();
        let step = |doc: &mut crate::doc::Document, js: &str| {
            crate::plugins::run(doc, 0, &[], js).unwrap();
            doc.reflow();
        };
        step(&mut doc, r##"
            const c = figma.createComponent(); c.name = "Toggle"; c.resize(80, 40);
            const track = figma.createRectangle(); track.name = "Track"; track.resize(80, 40); c.appendChild(track);
            track.fills = [figma.util.solidPaint("#FF0000")];
            c.createInstance().x = 200;
        "##);
        step(&mut doc, r##"
            const i = figma.currentPage.findOne(n => n.type === "INSTANCE");
            i.children[0].fills = [figma.util.solidPaint("#0000FF")];
        "##);
        step(&mut doc, if via_style { r##"
            const track = figma.currentPage.findOne(n => n.name === "Track" && n.parent.type === "COMPONENT");
            const st = figma.createPaintStyle(); st.name = "HUD / Accent"; st.paints = [figma.util.solidPaint("#00FF00")];
            track.fillStyleId = st.id;
        "## } else { r##"
            const track = figma.currentPage.findOne(n => n.name === "Track" && n.parent.type === "COMPONENT");
            const col = figma.variables.createVariableCollection("Theme");
            const accent = figma.variables.createVariable("Accent", col, "COLOR");
            accent.setValueForMode(col.modes[0].modeId, { r: 0, g: 1, b: 0, a: 1 });
            track.fills = [figma.variables.setBoundVariableForPaint(figma.util.solidPaint("#000000"), "color", accent)];
        "## });
        let color = |inst: bool| doc.nodes.values().find(|n| n.name == "Track" && n.source.is_some() == inst).unwrap().fills[0].color;
        assert_eq!(color(false), crate::doc::Rgba::rgb(0, 255, 0), "main follows (style: {via_style})");
        assert_eq!(color(true), crate::doc::Rgba::rgb(0, 0, 255), "the instance keeps its override (style: {via_style})");
    }
}

#[test]
fn add_variant_turns_a_component_into_a_set() {
    let mut h = harness();
    let f = draw(&mut h, Key::F, (0.0, 0.0), (120.0, 40.0));
    key(&mut h, Modifiers::COMMAND | Modifiers::ALT, Key::K);
    let c = h.state().sel[0];
    assert_eq!(h.state().doc.node(c).kind, Kind::Component);
    let _ = f;
    h.state_mut().add_variant_to(c);
    let v2 = h.state().sel[0];
    let set = h.state().doc.node(c).parent.expect("in a set");
    assert_eq!(h.state().doc.node(set).kind, Kind::ComponentSet);
    assert_eq!(h.state().doc.node(set).children.len(), 2);
    assert_eq!(h.state().doc.node(c).name, "Property 1=Default");
    assert_eq!(h.state().doc.node(v2).name, "Property 1=Variant2");
    // Again on a variant: a third one in the same set.
    h.state_mut().add_variant_to(v2);
    assert_eq!(h.state().doc.node(set).children.len(), 3);
}

/// Renders panels Codex flagged, for a visual check: `cargo test --release ui_look -- --ignored`.
#[test]
#[ignore]
fn ui_look() {
    isolate_data();
    let mut h = Harness::builder().with_size(vec2(1440.0, 900.0)).with_pixels_per_point(2.0).wgpu().build_eframe(|cc| {
        crate::setup_style(&cc.egui_ctx);
        let mut app = App::new(None);
        app.recoverable.clear();
        let c = app.doc.add_collection("Theme".into());
        app.doc.collections.iter_mut().find(|x| x.id == c).unwrap().modes = vec!["Dark".into(), "Light".into()];
        let v = app.doc.add_variable(c, "Accent".into(), crate::doc::VarValue::Color(crate::doc::Rgba::rgb(0x8E, 0x72, 0xFF)));
        app.doc.variables.iter_mut().find(|x| x.id == v).unwrap().values = vec![crate::doc::VarValue::Color(crate::doc::Rgba::rgb(0x8E, 0x72, 0xFF)), crate::doc::VarValue::Color(crate::doc::Rgba::rgb(0x12, 0x34, 0x56))];
        app.show_variables = true;
        app
    });
    h.run_steps(4);
    h.render().unwrap().save("target/look-variables.png").unwrap();
    // An auto layout frame with a child, selected: the Design panel.
    h.state_mut().show_variables = false;
    let mut f = Node::new(Kind::Frame, r(0., 0., 300., 100.));
    f.name = "Row".into();
    let f = h.state_mut().doc.insert(0, None, None, f);
    h.state_mut().doc.insert(0, Some(f), None, Node::new(Kind::Rectangle, r(10., 10., 40., 40.)));
    h.state_mut().sel = vec![f];
    h.state_mut().add_auto_layout();
    h.state_mut().doc.node_mut(f).shadows.push(Default::default());
    h.run_steps(4);
    h.render().unwrap().save("target/look-autolayout.png").unwrap();
    // Arcs: a dial ring, a donut slice and a pie, selected.
    let mut ids = vec![];
    for (i, arc) in [[0.0, 360.0, 0.8], [135.0, 270.0, 0.7], [-90.0, 120.0, 0.0]].into_iter().enumerate() {
        let mut e = Node::new(Kind::Ellipse, r(400. + i as f32 * 160., 0., 140., 140.));
        e.arc = Some(arc);
        e.fills = vec![crate::doc::Paint::solid(crate::doc::Rgba::rgb(0x8E, 0x72, 0xFF))];
        ids.push(h.state_mut().doc.insert(0, None, None, e));
    }
    h.state_mut().sel = vec![ids[1]];
    h.run_steps(4);
    h.render().unwrap().save("target/look-arcs.png").unwrap();
    // Assets: the showcase's components as a grid.
    {
        let a = h.state_mut();
        crate::plugins::run(&mut a.doc, 0, &[], include_str!("../assets/promo/showcase.js")).unwrap();
        crate::plugins::run(&mut a.doc, 0, &[], "for (const c of ['Chip', 'Avatar', 'Card']) { const k = figma.createComponent(); k.name = c; k.resize(80, 40); k.y = 3000; k.cornerRadius = 12; k.fills = [figma.util.solidPaint('#4DA8FF')]; }").unwrap();
        a.left_tab_assets = true;
    }
    h.run_steps(6);
    h.render().unwrap().save("target/look-assets.png").unwrap();
    eprintln!("asset thumbnails: {} ; tiles labeled: Button {} Chip {}", h.state().asset_thumbs.len(), h.query_all_by_label("Button").count(), h.query_all_by_label("Chip").count());
}

#[test]
fn layer_rows_and_pages_are_named_for_assistive_tech() {
    let mut h = harness();
    draw(&mut h, Key::R, (0.0, 0.0), (50.0, 50.0));
    h.run_steps(2);
    assert!(h.query_by_label("Rectangle 1").is_some(), "layer row");
    assert!(h.query_by_label("Page 1").is_some(), "page row");
    assert!(h.query_by_label("Main menu").is_some(), "icon buttons");
}

#[test]
fn ellipse_arcs_draw_rings_and_slices() {
    let mut doc = crate::doc::Document::default();
    crate::plugins::run(&mut doc, 0, &[], r##"
        const ring = figma.createEllipse(); ring.name = "Ring"; ring.resize(100, 100);
        ring.arcData = { startingAngle: 0, endingAngle: 2 * Math.PI, innerRadius: 0.8 };
        const pie = figma.createEllipse(); pie.name = "Pie"; pie.resize(100, 100); pie.x = 200;
        pie.arcData = { startingAngle: 0, endingAngle: Math.PI / 2, innerRadius: 0 };
    "##).unwrap();
    let get = |name: &str| doc.nodes.values().find(|n| n.name == name).unwrap().clone();
    let (ring, pie) = (get("Ring"), get("Pie"));
    assert_eq!(ring.arc, Some([0.0, 360.0, 0.8]));
    assert!(pie.arc.is_some_and(|a| (a[1] - 90.0).abs() < 0.01));
    // The ring's hole and the slice's empty quarters don't hit; their solid parts do.
    let hit = |n: &crate::doc::Node, x: f32, y: f32| doc.hit_self(n, pos2(x, y), 0.0);
    assert!(hit(&ring, 5.0, 50.0) && !hit(&ring, 50.0, 50.0));
    assert!(hit(&pie, 270.0, 70.0), "lower-right quarter (clockwise from 3 o'clock)");
    assert!(!hit(&pie, 230.0, 30.0), "upper-left is empty");
    let svg = crate::export::to_svg(&doc, &[ring.id]);
    assert!(svg.contains("fill-rule=\"evenodd\"") && svg.contains("<path"), "{svg}");
}

#[test]
fn layer_search_finds_text_and_groups_become_auto_layout_frames() {
    let mut h = harness();
    let mut t = Node::new(Kind::Text, r(0., 0., 80., 20.));
    t.text = "Move more".into();
    t.name = "Text 17".into();
    let t = h.state_mut().doc.insert(0, None, None, t);
    h.state_mut().layer_search = Some("move more".into());
    h.run_steps(2);
    assert!(h.query_by_label("Text 17").is_some(), "found by what it says");
    h.state_mut().layer_search = None;
    // Shift+A on a group: the group itself becomes the auto layout frame.
    let a = draw(&mut h, Key::O, (0.0, 100.0), (40.0, 140.0));
    let b = draw(&mut h, Key::O, (60.0, 100.0), (100.0, 140.0));
    h.state_mut().sel = vec![a, b];
    key(&mut h, Modifiers::COMMAND, Key::G);
    let g = h.state().sel[0];
    key(&mut h, Modifiers::SHIFT, Key::A);
    assert_eq!(h.state().sel, vec![g]);
    let n = h.state().doc.node(g);
    assert_eq!((n.kind, n.layout.mode), (Kind::Frame, crate::doc::LayoutMode::Horizontal));
    assert_eq!(n.children.len(), 2, "the ellipses lay out directly");
    let _ = t;
}

#[test]
fn assets_show_component_previews_and_search_filters_sets() {
    let mut h = harness();
    crate::plugins::run(&mut h.state_mut().doc, 0, &[], include_str!("../assets/promo/showcase.js")).unwrap();
    crate::plugins::run(&mut h.state_mut().doc, 0, &[], "const k = figma.createComponent(); k.name = 'Chip'; k.resize(80, 40);").unwrap();
    h.state_mut().left_tab_assets = true;
    h.run_steps(4);
    assert_eq!(h.state().asset_thumbs.len(), 2, "the Button set and Chip");
    // Editing a component re-renders just its preview.
    let chip = h.state().doc.nodes.values().find(|n| n.name == "Chip").unwrap().id;
    let before = h.state().asset_thumbs[&chip].0;
    h.state_mut().doc.node_mut(chip).radius = 12.0;
    h.run_steps(31);
    assert_ne!(h.state().asset_thumbs[&chip].0, before);
}

#[test]
fn exported_png_keeps_image_fills() {
    let mut doc = crate::doc::Document::default();
    let mut f = Node::new(Kind::Frame, r(0., 0., 300., 200.));
    f.fills = vec![crate::doc::Paint::solid(crate::doc::Rgba::rgb(20, 20, 30))];
    let f = doc.insert(0, None, None, f);
    // A red photo, filling a wide tile (cropped to cover).
    let img = image::RgbaImage::from_pixel(40, 60, image::Rgba([255, 0, 0, 255]));
    let mut png = vec![];
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    doc.images.insert("photo".into(), std::sync::Arc::new(png));
    let mut tile = Node::new(Kind::Rectangle, r(50., 50., 200., 100.));
    let mut paint = crate::doc::Paint::solid(crate::doc::Rgba::WHITE);
    paint.kind = crate::doc::PaintKind::Image;
    paint.image = "photo".into();
    tile.fills = vec![paint.clone()];
    tile.radius = 12.0;
    doc.insert(0, Some(f), None, tile.clone());
    // The same with a (tiny) adjustment, as Codex's HUD had.
    let mut adjusted = tile;
    adjusted.rect = r(50., 160., 200., 30.);
    paint.adjust.saturation = -0.01;
    adjusted.fills = vec![paint];
    doc.insert(0, Some(f), None, adjusted);
    let png = crate::export::to_png(&doc, &[f], 1.0).unwrap();
    let out = image::load_from_memory(&png).unwrap().to_rgba8();
    assert_eq!(out.get_pixel(150, 100).0[..3], [255, 0, 0], "plain image fill");
    assert_eq!(out.get_pixel(60, 60).0[..3], [255, 0, 0], "covers the tile from its corner");
    assert!(out.get_pixel(150, 175).0[0] > 200 && out.get_pixel(150, 175).0[1] < 60, "adjusted image fill: {:?}", out.get_pixel(150, 175));
}

/// `KATACHI_EXPORT_FILE=x.katachi cargo test --release export_file -- --ignored`: first page's frames to target/export-N.png.
#[test]
#[ignore]
fn export_file() {
    let Some(f) = std::env::var_os("KATACHI_EXPORT_FILE") else { return };
    let doc = crate::doc::parse_file(&std::fs::read_to_string(f).unwrap()).unwrap();
    for (i, &id) in doc.pages[0].children.iter().enumerate() {
        std::fs::write(format!("target/export-{i}.png"), crate::export::to_png(&doc, &[id], 1.0).unwrap()).unwrap();
    }
}

#[test]
fn dev_mode_lists_a_multi_selection() {
    let mut h = harness();
    let a = draw(&mut h, Key::R, (0.0, 0.0), (50.0, 50.0));
    let b = draw(&mut h, Key::O, (100.0, 0.0), (150.0, 50.0));
    h.state_mut().sel = vec![a, b];
    h.state_mut().dev_mode = true;
    h.run_steps(2);
    assert!(h.query_by_label("2 layers selected").is_some());
    // Each layer appears in the layers panel and in the Dev Mode list.
    assert_eq!(h.query_all_by_label("Ellipse 1").count(), 2);
    let _ = b;
}

#[test]
fn slash_named_styles_group_into_folders() {
    let mut h = harness();
    crate::plugins::run(&mut h.state_mut().doc, 0, &[], "for (const n of ['HUD / Title', 'HUD / Body', 'Accent']) { const s = figma.createPaintStyle(); s.name = n; s.paints = [figma.util.solidPaint('#8E72FF')]; }").unwrap();
    h.run_steps(2);
    assert!(h.query_by_label("HUD").is_some(), "a HUD folder");
    assert!(h.query_by_label("HUD / Title").is_none(), "names show without the folder");
}

#[test]
fn files_and_pages_reopen_where_they_were_viewed() {
    let mut h = harness();
    draw(&mut h, Key::R, (0.0, 0.0), (100.0, 100.0));
    h.state_mut().view.zoom = 2.5;
    h.state_mut().view.pan += vec2(-300.0, 120.0);
    h.run_steps(1);
    let r = h.state().canvas_rect.unwrap();
    let center = h.state().view.to_world(r.center());
    let path = std::env::temp_dir().join(format!("katachi-view-{}.katachi", std::process::id()));
    assert!(h.state_mut().save_to(path.clone()));
    let mut g = harness();
    g.state_mut().open_path(path);
    g.run_steps(2);
    let back = g.state().view.to_world(g.state().canvas_rect.unwrap().center());
    assert_eq!(g.state().view.zoom, 2.5);
    assert!((back - center).length() < 0.5, "{back:?} vs {center:?}");
    // Switching pages and back keeps each page's view.
    g.state_mut().doc.pages.push(crate::doc::Page { name: "Two".into(), children: vec![], background: crate::doc::PAGE_BG, flows: vec![], view: None, guides: vec![], prototype: Default::default() });
    g.state_mut().select_page(1);
    g.run_steps(2);
    g.state_mut().select_page(0);
    g.run_steps(2);
    assert_eq!(g.state().view.zoom, 2.5);
}

#[test]
fn vector_edit_panel_bends_and_moves_points() {
    let mut h = harness();
    let mut v = Node::vector(&[crate::doc::PathPoint { p: pos2(0., 0.), ..Default::default() }, crate::doc::PathPoint { p: pos2(100., 0.), ..Default::default() }, crate::doc::PathPoint { p: pos2(50., 80.), ..Default::default() }], true);
    v.name = "Tri".into();
    let id = h.state_mut().doc.insert(0, None, None, v);
    h.state_mut().enter_vector_edit(id);
    h.state_mut().vector_edit.as_mut().unwrap().selected = vec![(0, 2)];
    h.run_steps(2);
    assert!(h.query_by_label("Edit vector").is_some());
    h.get_by_label("Bend").click();
    h.run_steps(2);
    let p = h.state().doc.node(id).paths[0].points[2];
    assert!(p.h_in != vec2(0., 0.) && p.h_out != vec2(0., 0.), "smooth now");
    h.get_by_label("Done").click();
    h.run_steps(2);
    assert!(h.state().vector_edit.is_none());
}

#[test]
fn crop_mode_zooms_and_pans_the_image() {
    let mut doc = crate::doc::Document::default();
    // Left half red, right half blue.
    let img = image::RgbaImage::from_fn(100, 50, |x, _| if x < 50 { image::Rgba([255, 0, 0, 255]) } else { image::Rgba([0, 0, 255, 255]) });
    let mut png = vec![];
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    doc.images.insert("two".into(), std::sync::Arc::new(png));
    let mut n = Node::new(Kind::Rectangle, r(0., 0., 100., 50.));
    let mut p = crate::doc::Paint::solid(crate::doc::Rgba::WHITE);
    p.kind = crate::doc::PaintKind::Image;
    p.image = "two".into();
    p.scale_mode = crate::doc::ScaleMode::Crop;
    p.tile_scale = 2.0;
    p.from = pos2(1.0, 0.0);
    n.fills = vec![p];
    let id = doc.insert(0, None, None, n);
    let out = image::load_from_memory(&crate::export::to_png(&doc, &[id], 1.0).unwrap()).unwrap().to_rgba8();
    assert_eq!(out.get_pixel(5, 25).0[..3], [0, 0, 255], "zoomed 2x to the right half: blue at the left edge");
    assert_eq!(out.get_pixel(95, 25).0[..3], [0, 0, 255]);
    // Through the plugin API: Figma's imageTransform round-trips.
    let v = crate::plugin_doc::node_json(&doc, id);
    let t = &v["fills"][0]["imageTransform"];
    assert!((t[0][0].as_f64().unwrap() - 0.5).abs() < 1e-4 && (t[0][2].as_f64().unwrap() - 0.5).abs() < 1e-4, "{t}");
    let back = crate::plugin_doc::paint_from(&v["fills"][0], &doc, Some(egui::vec2(100.0, 50.0))).unwrap();
    assert_eq!(back.scale_mode, crate::doc::ScaleMode::Crop);
    assert!((back.tile_scale - 2.0).abs() < 1e-4 && (back.from.x - 1.0).abs() < 1e-4);
}

#[test]
fn quick_actions_run_commands_and_find_layers() {
    let mut h = harness();
    let a = draw(&mut h, Key::R, (0.0, 0.0), (40.0, 40.0));
    let b = draw(&mut h, Key::R, (60.0, 0.0), (100.0, 40.0));
    h.state_mut().sel = vec![a, b];
    key(&mut h, Modifiers::COMMAND, Key::Slash);
    assert!(h.state().quick.is_some(), "Cmd / opens it");
    h.event(Event::Text("add auto".into()));
    h.run_steps(2);
    key(&mut h, Modifiers::NONE, Key::Enter);
    assert!(h.state().quick.is_none());
    let w = h.state().sel[0];
    assert_eq!(h.state().doc.node(w).layout.mode, crate::doc::LayoutMode::Horizontal, "ran Add auto layout");
    // Layers by name.
    h.state_mut().doc.node_mut(b).name = "Checkout button".into();
    key(&mut h, Modifiers::COMMAND, Key::K);
    h.event(Event::Text("checkout".into()));
    h.run_steps(2);
    key(&mut h, Modifiers::NONE, Key::Enter);
    assert_eq!(h.state().sel, vec![b]);
}

#[test]
fn settings_modal_tabs_and_preferences() {
    let mut h = harness();
    key(&mut h, Modifiers::COMMAND, Key::Comma);
    assert_eq!(h.state().settings_tab, Some(crate::settings::Tab::Account), "Cmd , opens Settings");
    h.run_steps(2);
    h.get_by_label("Canvas").click();
    h.run_steps(2);
    assert_eq!(h.state().settings_tab, Some(crate::settings::Tab::Canvas));
    // Keep tool selected: two rectangles in a row without pressing R again.
    h.state_mut().prefs.keep_tool_selected = true;
    h.state_mut().settings_tab = None;
    h.run_steps(2);
    draw(&mut h, Key::R, (0.0, 0.0), (40.0, 40.0));
    assert_eq!(h.state().tool, Tool::Rectangle, "still drawing rectangles");
    h.state_mut().prefs.keep_tool_selected = false;
    draw(&mut h, Key::R, (60.0, 0.0), (100.0, 40.0));
    assert_eq!(h.state().tool, Tool::Move);
    // Nudge amounts come from Settings.
    h.state_mut().prefs.big_nudge = 8.0;
    let id = h.state().sel[0];
    let x = h.state().doc.node(id).rect.min.x;
    key(&mut h, Modifiers::SHIFT, Key::ArrowRight);
    assert_eq!(h.state().doc.node(id).rect.min.x, x + 8.0);
}

#[test]
fn floating_surfaces_stay_solid_with_glass() {
    let ctx = egui::Context::default();
    let a = crate::appearance::Appearance { glass: true, panel_opacity: 0.4, ..Default::default() };
    crate::appearance::apply(&ctx, &a);
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        let v = ctx.style_of(theme).visuals.clone();
        assert!(v.panel_fill.a() < 255, "panels are glass");
        assert_eq!(v.window_fill.a(), 255, "popovers, menus and windows are solid");
    }
    let mut s = egui::Style::default();
    crate::chrome::menu_style(&mut s);
    assert_eq!(s.visuals.window_fill, crate::chrome::MENU_BG, "menus are near-black in either theme");
}

#[test]
fn main_menu_follows_figma() {
    let mut h = harness();
    h.get_by_label("Main menu").click();
    h.run_steps(2);
    for label in ["Back to files", "Actions…", "File", "Edit", "View", "Object", "Arrange", "Plugins", "Preferences", "Help and account"] {
        assert!(h.query_all_by_label_contains(label).next().is_some(), "{label}");
    }
}

#[test]
fn find_and_version_history_take_over_their_panels() {
    let mut h = harness();
    let mut t = Node::new(Kind::Text, r(0., 0., 80., 20.));
    t.text = "Checkout".into();
    t.name = "Label".into();
    let t = h.state_mut().doc.insert(0, None, None, t);
    key(&mut h, Modifiers::COMMAND, Key::F);
    h.run_steps(2);
    assert!(h.query_by_label("Layers").is_none(), "Find replaces Pages and Layers");
    h.event(Event::Text("check".into()));
    h.run_steps(2);
    h.get_by_label("Label  \u{201C}Checkout\u{201D}").click();
    h.run_steps(2);
    assert_eq!(h.state().sel, vec![t]);
    // Escape in the field closes Find.
    h.state_mut().find.3 = true;
    h.run_steps(2);
    key(&mut h, Modifiers::NONE, Key::Escape);
    assert!(!h.state().find.0, "Escape closes Find");
    h.state_mut().show_history = true;
    h.run_steps(2);
    assert!(h.query_by_label("Version history").is_some());
    assert!(h.query_by_label("Design").is_none(), "replaces the Design panel");
    h.get_by_label("Close").click();
    h.run_steps(2);
    assert!(!h.state().show_history);
}

#[test]
fn color_picker_drags_and_escape_from_find_keeps_selection() {
    let mut h = harness();
    let id = draw(&mut h, Key::R, (0.0, 0.0), (80.0, 80.0));
    // Open the fill popover from its swatch, then drag across the color square.
    h.get_all_by_label("Color swatch").next().unwrap().click();
    h.run_steps(3);
    let before = h.state().doc.node(id).fills[0].color;
    let popover = egui::Popup::is_any_open(&h.ctx);
    assert!(popover, "fill editor open");
    let sq = h.get_by_label("Saturation and brightness").rect();
    let (a, b) = (sq.min + vec2(10.0, 10.0), sq.min + vec2(sq.width() * 0.8, sq.height() * 0.6));
    move_to(&mut h, a, Modifiers::NONE);
    press(&mut h, a, true, Modifiers::NONE);
    for i in 1..=5 {
        move_to(&mut h, a.lerp(b, i as f32 / 5.0), Modifiers::NONE);
    }
    let mid = h.state().doc.node(id).fills[0].color;
    press(&mut h, b, false, Modifiers::NONE);
    let found = (mid != before).then_some(());
    assert!(found.is_some(), "dragging in the picker changes the fill while the button is held");
    assert!(egui::Popup::is_any_open(&h.ctx), "and the editor stays open");
    // Find, then Escape: Find closes and the selection stays.
    key(&mut h, Modifiers::NONE, Key::Escape);
    h.state_mut().sel = vec![id];
    key(&mut h, Modifiers::COMMAND, Key::F);
    h.run_steps(2);
    key(&mut h, Modifiers::NONE, Key::Escape);
    assert!(!h.state().find.0);
    assert_eq!(h.state().sel, vec![id], "selection kept");
}

#[test]
fn snaps_to_equal_spacing_and_marks_gaps() {
    let mut h = harness();
    draw(&mut h, Key::R, (0.0, 0.0), (100.0, 100.0));
    draw(&mut h, Key::R, (150.0, 0.0), (250.0, 100.0));
    let c = draw(&mut h, Key::R, (400.0, 0.0), (500.0, 100.0));
    // Drop c 3px past the 50px gap that a and b already have: it lands one gap after b.
    let (a, b) = (s(&h, 450., 50.), s(&h, 353., 50.));
    move_to(&mut h, a, Modifiers::NONE);
    press(&mut h, a, true, Modifiers::NONE);
    for i in 1..=6 {
        move_to(&mut h, a.lerp(b, i as f32 / 6.0), Modifiers::NONE);
    }
    assert_eq!(h.state().gap_marks.len(), 2, "both 50px gaps marked");
    press(&mut h, b, false, Modifiers::NONE);
    h.run_steps(2);
    assert_eq!(rect(&h, c).min.x, 300.0);
    assert!(h.state().gap_marks.is_empty());
    // Centered between two neighbors: equal gaps on both sides.
    let mut h = harness();
    draw(&mut h, Key::R, (0.0, 0.0), (100.0, 100.0));
    draw(&mut h, Key::R, (300.0, 0.0), (400.0, 100.0));
    let c = draw(&mut h, Key::R, (0.0, 300.0), (100.0, 400.0));
    let (a, b) = (s(&h, 50., 350.), s(&h, 198., 50.));
    drag(&mut h, a, b, Modifiers::NONE);
    assert_eq!(rect(&h, c), r(150., 0., 100., 100.));
}

#[test]
fn smart_selection_spacing_handle_and_swap() {
    let mut h = harness();
    let a = draw(&mut h, Key::R, (0.0, 0.0), (100.0, 100.0));
    let b = draw(&mut h, Key::R, (150.0, 0.0), (250.0, 100.0));
    let c = draw(&mut h, Key::R, (300.0, 0.0), (400.0, 100.0));
    key(&mut h, Modifiers::COMMAND, Key::A);
    assert!(h.state().smart_selection().is_some_and(|(axis, _)| axis == 0));
    // The handle mid-gap widens every gap together.
    let (p, q) = (s(&h, 125., 50.), s(&h, 145., 50.));
    drag(&mut h, p, q, Modifiers::NONE);
    assert_eq!((rect(&h, a).min.x, rect(&h, b).min.x, rect(&h, c).min.x), (0., 170., 340.));
    key(&mut h, Modifiers::COMMAND, Key::Z);
    assert_eq!((rect(&h, b).min.x, rect(&h, c).min.x), (150., 300.), "one undo step");
    // Dragging a's center knob onto c's slot swaps them.
    let (p, q) = (s(&h, 50., 50.), s(&h, 350., 50.));
    drag(&mut h, p, q, Modifiers::NONE);
    assert_eq!((rect(&h, a).min.x, rect(&h, b).min.x, rect(&h, c).min.x), (300., 150., 0.));
    assert_eq!(h.state().sel.len(), 3, "selection kept");
}

#[test]
fn ruler_guides_create_snap_move_and_delete() {
    use crate::doc::{Axis, Guide};
    let mut h = harness();
    let id = draw(&mut h, Key::R, (0.0, 0.0), (100.0, 100.0));
    key(&mut h, Modifiers::SHIFT, Key::R);
    let top = h.state().canvas_rect.unwrap().min + vec2(400.0, 10.0);
    // Pull a horizontal guide out of the top ruler down to y = 200.
    let to = pos2(top.x, s(&h, 0., 200.).y);
    drag(&mut h, top, to, Modifiers::NONE);
    assert_eq!(h.state().doc.pages[0].guides, vec![Guide { axis: Axis::Y, offset: 200.0 }]);
    // Layers snap to it: the bottom edge dropped 3px short lands on the guide.
    let (a, b) = (s(&h, 50., 50.), s(&h, 50., 147.));
    click(&mut h, a, Modifiers::NONE);
    drag(&mut h, a, b, Modifiers::NONE);
    assert_eq!(rect(&h, id).min.y, 100.0);
    // Drag the guide itself, then select it and delete it with Backspace.
    let (a, b) = (s(&h, 300., 200.), s(&h, 300., 250.));
    drag(&mut h, a, b, Modifiers::NONE);
    assert_eq!(h.state().doc.pages[0].guides[0].offset, 250.0);
    let p = s(&h, 300., 250.);
    click(&mut h, p, Modifiers::NONE);
    assert_eq!(h.state().guide_sel, Some(0));
    assert!(h.state().sel.is_empty(), "selecting a guide drops the layer selection");
    key(&mut h, Modifiers::NONE, Key::Backspace);
    assert!(h.state().doc.pages[0].guides.is_empty());
    key(&mut h, Modifiers::COMMAND, Key::Z);
    assert_eq!(h.state().doc.pages[0].guides.len(), 1, "undoable");
    // Dragging it back onto the ruler removes it; guides round-trip through the file.
    let json = crate::doc::file_json(&h.state().doc);
    assert_eq!(crate::doc::parse_file(&json).unwrap().pages[0].guides.len(), 1);
    let a = s(&h, 300., 250.);
    drag(&mut h, a, top, Modifiers::NONE);
    assert!(h.state().doc.pages[0].guides.is_empty());
    assert!(!crate::doc::file_json(&h.state().doc).contains("guides"), "no guides, no field");
}

#[test]
fn zoom_tool_and_next_frame() {
    let mut h = harness();
    let center = |h: &Harness<App>| h.state().view.to_world(h.state().canvas_rect.unwrap().center());
    // Z: click zooms in at the point, Alt-click zooms back out.
    key(&mut h, Modifiers::NONE, Key::Z);
    assert_eq!(h.state().tool, Tool::Zoom, "a tap keeps the zoom tool");
    let p = s(&h, 100., 100.);
    click(&mut h, p, Modifiers::NONE);
    assert_eq!(h.state().view.zoom, 2.0);
    assert!((s(&h, 100., 100.) - p).length() < 0.5, "the clicked point stays put");
    click(&mut h, p, Modifiers::ALT);
    assert_eq!(h.state().view.zoom, 1.0);
    // Dragging a box zooms to fit it.
    let (a, b) = (s(&h, 0., 0.), s(&h, 40., 40.));
    drag(&mut h, a, b, Modifiers::NONE);
    assert!(h.state().view.zoom > 10.0);
    assert!((center(&h) - pos2(20., 20.)).length() < 0.5);
    key(&mut h, Modifiers::NONE, Key::Escape);
    // N and Shift N walk the top-level frames in layers-panel order (top first).
    let st = h.state_mut();
    let f1 = st.doc.insert(0, None, None, Node::new(Kind::Frame, r(0., 0., 100., 100.)));
    let f2 = st.doc.insert(0, None, None, Node::new(Kind::Frame, r(1000., 0., 100., 100.)));
    st.zoom_target = Some(f1);
    h.run_steps(2);
    let c = |h: &Harness<App>, f: NodeId| (center(h) - h.state().doc.node(f).rect.center()).length() < 0.5;
    assert!(c(&h, f1));
    key(&mut h, Modifiers::NONE, Key::N);
    assert!(c(&h, f2), "wraps from the bottom frame to the top one");
    key(&mut h, Modifiers::NONE, Key::N);
    assert!(c(&h, f1));
    key(&mut h, Modifiers::SHIFT, Key::N);
    assert!(c(&h, f2));
    // Holding Z makes the zoom tool temporary: releasing it brings back the previous tool.
    let z = |pressed| Event::Key { key: Key::Z, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
    h.event(z(true));
    h.run_steps(40);
    assert_eq!(h.state().tool, Tool::Zoom);
    h.event(z(false));
    h.run_steps(2);
    assert_eq!(h.state().tool, Tool::Move);
}

#[test]
fn pixel_grid_and_snap_to_pixel_toggles() {
    let mut h = harness();
    let id = draw(&mut h, Key::R, (0.0, 0.0), (100.0, 100.0));
    // Snapping to the pixel grid (on by default) keeps a fractional drag on whole pixels.
    let (a, b) = (s(&h, 50., 50.), s(&h, 80.25, 50.));
    drag(&mut h, a, b, Modifiers::NONE);
    assert_eq!(rect(&h, id).min.x, 30.0);
    key(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::Quote);
    assert!(!h.state().prefs.snap_to_pixels);
    let (a, b) = (s(&h, 50., 50.), s(&h, 50.5, 80.25));
    drag(&mut h, a, b, Modifiers::NONE);
    assert_eq!(rect(&h, id).min, pos2(30.5, 30.25), "free positioning with snap off");
    key(&mut h, Modifiers::SHIFT, Key::Quote);
    assert!(!h.state().prefs.pixel_grid);
    key(&mut h, Modifiers::SHIFT, Key::Quote);
    assert!(h.state().prefs.pixel_grid);
}

#[test]
fn layers_panel_shift_ranges_multi_drag_and_reveal() {
    let mut h = harness();
    let ids: Vec<NodeId> = (0..60).map(|i| h.state_mut().doc.insert(0, None, None, Node::new(Kind::Rectangle, r(i as f32 * 30., 0., 20., 20.)))).collect();
    h.run_steps(2);
    let row = |h: &Harness<App>, name: &str| h.get_by_label(name).rect();
    let click_row = |h: &mut Harness<App>, name: &str, m| {
        let p = row(h, name).center();
        click(h, p, m);
    };
    // The list is top-first: Rectangle 60 is the top row.
    click_row(&mut h, "Rectangle 60", Modifiers::NONE);
    click_row(&mut h, "Rectangle 57", Modifiers::SHIFT);
    let mut sel = h.state().sel.clone();
    sel.sort();
    assert_eq!(sel, ids[56..60].to_vec(), "Shift-click selects the range");
    click_row(&mut h, "Rectangle 58", Modifiers::COMMAND);
    assert_eq!(h.state().sel.len(), 3, "Cmd-click toggles one row");
    // Drag two selected rows below Rectangle 55: both move, in their order.
    h.state_mut().sel = vec![ids[59], ids[58]];
    h.run_steps(2);
    let (from, to) = (row(&h, "Rectangle 60").center(), row(&h, "Rectangle 55").center() + vec2(0.0, 8.0));
    drag(&mut h, from, to, Modifiers::NONE);
    let kids = &h.state().doc.pages[0].children;
    assert_eq!(kids[54..57].to_vec(), vec![ids[58], ids[59], ids[54]], "moved together, order kept");
    // Rows are virtualized: the bottom one isn't built until the selection scrolls it into view.
    assert!(h.query_by_label("Rectangle 1").is_none());
    h.state_mut().sel = vec![ids[0]];
    h.run_steps(3);
    let r1 = row(&h, "Rectangle 1");
    assert!(r1.max.y <= 900.0 && r1.min.y > 0.0, "revealed at {r1:?}");
}

#[test]
fn undo_brings_back_the_selection_and_page() {
    let mut h = harness();
    let a = draw(&mut h, Key::R, (0.0, 0.0), (50.0, 50.0));
    key(&mut h, Modifiers::NONE, Key::ArrowRight);
    key(&mut h, Modifiers::NONE, Key::Escape);
    assert!(h.state().sel.is_empty());
    key(&mut h, Modifiers::COMMAND, Key::Z);
    assert_eq!(h.state().sel, vec![a], "what was selected comes back");
    assert_eq!(rect(&h, a).min.x, 0.0);
    key(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    assert_eq!((h.state().sel.clone(), rect(&h, a).min.x), (vec![], 1.0), "redo returns to the state undo left");
    // A change on another page: undo goes back there.
    let mut pg = h.state().doc.pages[0].clone();
    (pg.name, pg.children) = ("Page 2".into(), vec![]);
    h.state_mut().doc.pages.push(pg);
    h.state_mut().select_page(1);
    h.run_steps(2);
    let b = draw(&mut h, Key::O, (0.0, 0.0), (40.0, 40.0));
    key(&mut h, Modifiers::NONE, Key::ArrowDown);
    h.state_mut().select_page(0);
    h.run_steps(2);
    key(&mut h, Modifiers::COMMAND, Key::Z);
    assert_eq!((h.state().page, h.state().sel.clone()), (1, vec![b]));
    assert_eq!(rect(&h, b).min.y, 0.0);
}

#[test]
fn select_all_inverse_and_same_properties() {
    use crate::app::Same;
    use crate::doc::{Paint, Rgba};
    let mut h = harness();
    let add = |h: &mut Harness<App>, kind, c| {
        let mut n = Node::new(kind, r(0., 0., 20., 20.));
        n.fills = vec![Paint::solid(c)];
        h.state_mut().doc.insert(0, None, None, n)
    };
    let (red, blue) = (Rgba::rgb(255, 0, 0), Rgba::rgb(0, 0, 255));
    let a = add(&mut h, Kind::Rectangle, red);
    let b = add(&mut h, Kind::Ellipse, red);
    let c = add(&mut h, Kind::Rectangle, blue);
    let locked = add(&mut h, Kind::Rectangle, red);
    h.state_mut().doc.node_mut(locked).locked = true;
    key(&mut h, Modifiers::COMMAND, Key::A);
    assert_eq!(h.state().sel, vec![a, b, c], "Cmd A skips locked layers");
    h.state_mut().sel = vec![a];
    key(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::A);
    assert_eq!(h.state().sel, vec![b, c], "select inverse");
    h.state_mut().sel = vec![a];
    h.state_mut().select_same(Same::Fill);
    assert_eq!(h.state().sel, vec![a, b], "same fill, any kind");
    // No stroke on the layer: the selection stays.
    h.state_mut().select_same(Same::Stroke);
    assert_eq!(h.state().sel, vec![a, b]);
    let text = |h: &mut Harness<App>, size| {
        let mut t = Node::new(Kind::Text, r(0., 50., 80., 20.));
        (t.text, t.font_size) = ("Hi".into(), size);
        h.state_mut().doc.insert(0, None, None, t)
    };
    let (t1, _, t3) = (text(&mut h, 16.0), text(&mut h, 24.0), text(&mut h, 16.0));
    h.state_mut().sel = vec![t1];
    h.state_mut().select_same(Same::Font);
    assert_eq!(h.state().sel, vec![t1, t3]);
}

#[test]
fn batch_rename_numbers_and_replaces_as_one_undo_step() {
    let mut h = harness();
    let ids: Vec<NodeId> = (0..3).map(|i| draw(&mut h, Key::R, (i as f32 * 60.0, 0.0), (i as f32 * 60.0 + 40.0, 40.0))).collect();
    // One layer: inline rename in its row.
    key(&mut h, Modifiers::COMMAND, Key::R);
    assert!(h.state().renaming.is_some() && h.state().batch_rename.is_none());
    key(&mut h, Modifiers::NONE, Key::Escape);
    h.state_mut().sel = ids.clone();
    h.run_steps(2);
    key(&mut h, Modifiers::COMMAND, Key::R);
    let b = h.state_mut().batch_rename.as_mut().expect("batch rename open");
    b.to = "Card $n of $N ($&)".into();
    (b.find, b.with) = ("Rectangle".into(), "Box".into());
    h.run_steps(2);
    assert!(h.query_by_label_contains("Card 1 of 3 (Box 1)").is_some(), "live preview");    h.get_by_label("Rename").click();
    h.run_steps(2);
    let names: Vec<String> = ids.iter().map(|&id| h.state().doc.node(id).name.clone()).collect();
    assert_eq!(names, ["Card 1 of 3 (Box 1)", "Card 2 of 2 (Box 2)", "Card 3 of 1 (Box 3)"]);
    assert!(h.state().batch_rename.is_none());
    key(&mut h, Modifiers::COMMAND, Key::Z);
    assert_eq!(h.state().doc.node(ids[2]).name, "Rectangle 3", "one undo step");
    assert_eq!(h.state().doc.node(ids[0]).name, "Rectangle 1");
}

fn copied_text(h: &mut Harness<App>) -> String {
    h.event(Event::Copy);
    h.step();
    h.output().platform_output.commands.iter().find_map(|c| match c {
        egui::OutputCommand::CopyText(t) => Some(t.clone()),
        _ => None,
    }).expect("copied to clipboard")
}

#[test]
fn resize_to_fit_remove_fill_lock_all_and_paste_over_selection() {
    let mut h = harness();
    let f = draw(&mut h, Key::F, (0.0, 0.0), (200.0, 200.0));
    let a = draw(&mut h, Key::R, (10.0, 20.0), (50.0, 60.0));
    assert_eq!(h.state().doc.node(a).parent, Some(f));
    h.state_mut().sel = vec![f];
    key(&mut h, Modifiers::COMMAND | Modifiers::SHIFT | Modifiers::ALT, Key::R);
    assert_eq!((rect(&h, f), rect(&h, a)), (r(10., 20., 40., 40.), r(10., 20., 40., 40.)), "frame hugs its child, which stays put");
    h.state_mut().sel = vec![a];
    h.state_mut().remove_fills();
    assert!(h.state().doc.node(a).fills.is_empty());
    // Lock all locks the page's top level; Unlock all frees nested layers too.
    h.state_mut().doc.node_mut(a).locked = true;
    h.state_mut().lock_all(true);
    assert!(h.state().doc.node(f).locked && h.state().sel.is_empty());
    h.state_mut().lock_all(false);
    assert!(!h.state().doc.node(f).locked && !h.state().doc.node(a).locked);
    // Copy the child; Cmd V with the frame selected pastes into it, Shift Cmd V over it.
    h.state_mut().sel = vec![a];
    let copied = copied_text(&mut h);
    h.state_mut().sel = vec![f];
    h.event(Event::Paste(copied.clone()));
    h.run_steps(2);
    let inside = h.state().sel[0];
    assert_eq!(h.state().doc.node(inside).parent, Some(f));
    h.state_mut().sel = vec![f];
    h.input_mut().modifiers = Modifiers::COMMAND | Modifiers::SHIFT;
    h.event(Event::Paste(copied));
    h.run_steps(2);
    h.input_mut().modifiers = Modifiers::NONE;
    let over = h.state().sel[0];
    assert_eq!(h.state().doc.node(over).parent, None, "over the frame, not in it");
    assert_eq!(h.state().doc.pages[0].children, vec![f, over]);
    assert_eq!(rect(&h, over), rect(&h, a), "same position as copied");
}

#[test]
fn clipboard_images_paste_and_dropped_images_replace_fills() {
    use crate::app::TEST_CLIPBOARD;
    let mut h = harness();
    let img = image::RgbaImage::from_pixel(40, 30, image::Rgba([0, 200, 0, 255]));
    TEST_CLIPBOARD.with(|c| c.borrow_mut().1 = Some(img.clone()));
    // Cmd V with only an image on the clipboard reaches egui as just the V key going up.
    let v_up = Event::Key { key: Key::V, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::COMMAND };
    h.event(v_up.clone());
    h.run_steps(2);
    let id = h.state().sel[0];
    let center = h.state().view_center_world();
    let n = h.state().doc.node(id);
    assert_eq!((n.rect.size(), n.rect.center()), (vec2(40., 30.), center));
    assert_eq!(n.fills[0].kind, crate::doc::PaintKind::Image);
    // With a frame selected, the image goes in the middle of it.
    let f = draw(&mut h, Key::F, (150.0, 150.0), (350.0, 350.0));
    h.event(v_up.clone());
    h.run_steps(2);
    let inner = h.state().sel[0];
    assert_eq!((h.state().doc.node(inner).parent, rect(&h, inner).center()), (Some(f), pos2(250., 250.)));
    // A text paste doesn't also paste the image.
    let count = h.state().doc.nodes.len();
    h.event(Event::Paste("hello".into()));
    h.event(v_up);
    h.run_steps(2);
    assert_eq!(h.state().doc.nodes.len(), count + 1, "just the text layer");
    // Dropping an image on a layer with an image fill swaps the image; Alt-drop does it on any layer.
    let drop = |h: &mut Harness<App>, at: Pos2, m: Modifiers, color: [u8; 4]| {
        let mut buf = std::io::Cursor::new(vec![]);
        image::RgbaImage::from_pixel(8, 8, image::Rgba(color)).write_to(&mut buf, image::ImageFormat::Png).unwrap();
        let p = s(h, at.x, at.y);
        move_to(h, p, m);
        h.input_mut().dropped_files.push(egui::DroppedFile { name: "new.png".into(), bytes: Some(buf.into_inner().into()), ..Default::default() });
        h.input_mut().modifiers = m;
        h.run_steps(2);
        h.input_mut().modifiers = Modifiers::NONE;
    };
    let count = h.state().doc.nodes.len();
    let old_key = h.state().doc.node(id).fills[0].image.clone();
    drop(&mut h, center - vec2(10., 10.), Modifiers::NONE, [255, 0, 0, 255]);
    assert_eq!(h.state().doc.nodes.len(), count, "no new layer");
    assert_ne!(h.state().doc.node(id).fills[0].image, old_key, "image replaced");
    let plain = draw(&mut h, Key::R, (-400.0, 0.0), (-300.0, 100.0));
    drop(&mut h, pos2(-350., 50.), Modifiers::ALT, [0, 0, 255, 255]);
    assert_eq!(h.state().doc.node(plain).fills.iter().map(|p| p.kind).collect::<Vec<_>>(), vec![crate::doc::PaintKind::Image]);
    TEST_CLIPBOARD.with(|c| c.borrow_mut().1 = None);
}

#[test]
fn pages_reorder_by_dragging_duplicate_and_dividers() {
    let mut h = harness();
    let a = draw(&mut h, Key::R, (0.0, 0.0), (50.0, 50.0));
    for name in ["Page 2", "---", "Page 3"] {
        let mut pg = h.state().doc.pages[0].clone();
        (pg.name, pg.children) = (name.into(), vec![]);
        h.state_mut().doc.pages.push(pg);
    }
    h.run_steps(2);
    let names = |h: &Harness<App>| h.state().doc.pages.iter().map(|p| p.name.clone()).collect::<Vec<_>>();
    // A divider is a line in the list: clicking it opens nothing.
    let divider = h.get_by_label("---").rect();
    assert!(divider.height() < 20.0);
    click(&mut h, divider.center(), Modifiers::NONE);
    assert_eq!(h.state().page, 0);
    // Drag Page 3 above Page 1; the open page stays open.
    let (from, to) = (h.get_by_label("Page 3").rect().center(), h.get_by_label("Page 1").rect().min + vec2(20.0, 3.0));
    drag(&mut h, from, to, Modifiers::NONE);
    assert_eq!(names(&h), ["Page 3", "Page 1", "Page 2", "---"]);
    assert_eq!(h.state().page, 1, "still on Page 1");
    key(&mut h, Modifiers::COMMAND, Key::Z);
    assert_eq!(names(&h)[0], "Page 1", "undoable");
    h.state_mut().duplicate_page(0);
    h.run_steps(2);
    assert_eq!(names(&h)[..2], ["Page 1", "Page 1 copy"]);
    assert_eq!(h.state().page, 1, "the copy opens");
    let copy = h.state().doc.pages[1].children.clone();
    assert_eq!(copy.len(), 1);
    assert_ne!(copy[0], a);
    assert_eq!(rect(&h, copy[0]), rect(&h, a));
}

#[test]
fn paste_to_replace_takes_layers_copied_in_another_tab() {
    use crate::app::TEST_CLIPBOARD;
    let mut a = harness();
    let e = draw(&mut a, Key::O, (0.0, 0.0), (30.0, 30.0));
    a.state_mut().sel = vec![e];
    let copied = copied_text(&mut a);
    let mut b = harness();
    let t = draw(&mut b, Key::R, (100.0, 100.0), (140.0, 140.0));
    TEST_CLIPBOARD.with(|c| c.borrow_mut().0 = Some(copied));
    key(&mut b, Modifiers::COMMAND | Modifiers::SHIFT, Key::R);
    TEST_CLIPBOARD.with(|c| c.borrow_mut().0 = None);
    let new = b.state().sel[0];
    assert!(!b.state().doc.nodes.contains_key(&t));
    assert_eq!((b.state().doc.node(new).kind, rect(&b, new).center()), (Kind::Ellipse, pos2(120., 120.)));
}

/// Put characters [a, b) of the text being edited under the editor's selection.
fn select_chars(h: &mut Harness<App>, a: usize, b: usize) {
    let edit_id = egui::Id::new("katachi-text-edit");
    let mut state = egui::TextEdit::load_state(&h.ctx, edit_id).expect("editor state");
    state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(a), egui::text::CCursor::new(b))));
    state.store(&h.ctx, edit_id);
}

/// A clipboard event as egui-winit sends it: the chord's modifiers are only in the input state.
fn clip_event(h: &mut Harness<App>, m: Modifiers, e: Event) {
    h.input_mut().modifiers = m;
    h.event_modifiers(e, m);
    h.run_steps(2);
    h.input_mut().modifiers = Modifiers::NONE;
}

#[test]
fn text_formatting_shortcuts_style_layers_and_ranges() {
    use crate::doc::{Decoration, LineHeight, TextAlign, TextSizing};
    let mut h = harness();
    let mut t = Node::new(Kind::Text, r(0., 0., 120., 10.));
    t.text = "Hello world".into();
    t.font_size = 20.;
    t.text_sizing = TextSizing::AutoHeight;
    let id = h.state_mut().doc.insert(0, None, None, t);
    h.state_mut().sel = vec![id];
    h.run_steps(2);
    let node = |h: &Harness<App>| h.state().doc.node(id).clone();
    // With the layer selected, shortcuts restyle all of it.
    key(&mut h, Modifiers::COMMAND, Key::B);
    assert_eq!(node(&h).font_weight, 700);
    key(&mut h, Modifiers::COMMAND, Key::B);
    assert_eq!(node(&h).font_weight, 400, "Cmd B toggles back");
    key(&mut h, Modifiers::COMMAND, Key::I);
    assert!(node(&h).italic);
    key(&mut h, Modifiers::COMMAND, Key::U);
    assert_eq!(node(&h).decoration, Decoration::Underline);
    key(&mut h, Modifiers::COMMAND | Modifiers::ALT, Key::J);
    assert_eq!(node(&h).text_align, TextAlign::Justify);
    key(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::Period);
    assert_eq!(node(&h).font_size, 21.0);
    key(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::Comma);
    key(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::Comma);
    assert_eq!(node(&h).font_size, 19.0);
    key(&mut h, Modifiers::ALT, Key::Period);
    assert_eq!(node(&h).letter_spacing, 1.0);
    key(&mut h, Modifiers::ALT | Modifiers::SHIFT, Key::Period);
    assert_eq!(node(&h).line_height, LineHeight::Px((19.0 * crate::text::NATURAL_LINE).round() + 1.0), "Auto becomes px");
    key(&mut h, Modifiers::COMMAND | Modifiers::ALT, Key::Period);
    assert_eq!(node(&h).font_weight, 500, "next installed weight");
    key(&mut h, Modifiers::COMMAND, Key::Z);
    assert_eq!(node(&h).font_weight, 400, "one undo step each");
    // Shift Cmd X reaches the app as a cut: it strikes through instead of cutting the layer.
    clip_event(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Event::Cut);
    assert!(h.state().doc.nodes.contains_key(&id));
    assert_eq!(node(&h).decoration, Decoration::Strikethrough);
    // While editing with characters selected, they apply to the range only.
    h.state_mut().editing_text = Some(id);
    h.state_mut().focus_text = true;
    h.run_steps(3);
    select_chars(&mut h, 6, 11);
    key(&mut h, Modifiers::COMMAND, Key::B);
    let n = node(&h);
    assert_eq!((n.font_weight, n.spans.len()), (400, 1));
    assert_eq!((n.spans[0].start, n.spans[0].end, n.spans[0].style.font_weight), (6, 11, Some(700)));
    clip_event(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Event::Cut);
    let n = node(&h);
    assert_eq!(n.text, "Hello world", "the editor didn't cut");
    assert_eq!(n.spans[0].style.decoration, Some(Decoration::None), "strikethrough toggles off on the range");
    // Justified rows fill the box but a paragraph's last row doesn't.
    let mut j = n.clone();
    j.text = "alpha beta gamma delta epsilon a".into();
    j.spans.clear();
    j.italic = false;
    let painter = egui::Painter::new(h.ctx.clone(), egui::LayerId::background(), Rect::EVERYTHING);
    let (g, _, _) = crate::text::layout(&painter, &j, 1.0, egui::Color32::BLACK);
    assert!(g.rows.len() >= 2);
    assert!((g.rows[0].size.x - 120.0).abs() < 0.5, "{}", g.rows[0].size.x);
    assert!(g.rows.last().unwrap().size.x < 100.0, "{}", g.rows.last().unwrap().size.x);
    let jid = h.state_mut().doc.insert(0, None, None, j);
    h.run_steps(2);
    let svg = crate::export::to_svg(&h.state().doc, &[jid]);
    assert_eq!(svg.matches(r#"textLength="120""#).count(), h.state().doc.node(jid).lines.len() - 1, "{svg}");
}

#[test]
fn italics_use_real_faces_and_slant_only_without_one() {
    use crate::text::{style_name, styles_for};
    let mut h = harness();
    assert_eq!(style_name(400, true), "Italic");
    assert_eq!(style_name(700, true), "Bold Italic");
    assert_eq!(style_name(600, false), "Semi Bold");
    assert!([400, 500, 600, 700].iter().all(|w| styles_for("Inter").contains(&(*w, false))));
    let slanted = |h: &Harness<App>, family: &str, italic: bool| {
        let mut n = Node::new(Kind::Text, r(0., 0., 10., 10.));
        (n.font_family, n.italic) = (family.into(), italic);
        crate::text::job(&h.ctx, &n, "x", 1.0, egui::Color32::BLACK).sections[0].format.italics
    };
    // No such family: Inter stands in, slanted.
    let mut t = Node::new(Kind::Text, r(0., 0., 10., 10.));
    (t.text, t.font_family, t.italic) = ("Hi".into(), "No Such Font".into(), true);
    let missing = h.state_mut().doc.insert(0, None, None, t.clone());
    // A family with a real italic (when this machine has one) draws it unslanted.
    let real = crate::text::families().iter().find(|f| styles_for(f).contains(&(400, true)) && styles_for(f).contains(&(400, false))).cloned();
    if let Some(f) = &real {
        t.font_family = f.clone();
        h.state_mut().doc.insert(0, None, None, t);
    }
    h.run_steps(4);
    assert!(slanted(&h, "No Such Font", true));
    assert!(!slanted(&h, "No Such Font", false));
    assert!(crate::export::to_svg(&h.state().doc, &[missing]).contains(r#"font-style="italic""#));
    if let Some(f) = real {
        assert!(!slanted(&h, &f, true), "{f} has an italic face");
        assert_eq!(crate::text::font_family(&h.ctx, &f, 400, true).0, egui::FontFamily::Name(format!("{f}#400i").into()));
    }
}

#[test]
fn missing_fonts_show_and_replace_in_one_step() {
    use crate::doc::{SpanStyle, TextSpan};
    let mut h = harness();
    let mut t = Node::new(Kind::Text, r(0., 0., 10., 10.));
    (t.text, t.font_family, t.font_weight) = ("Imported".into(), "Fig Only Sans".into(), 700);
    t.spans = vec![TextSpan { start: 0, end: 3, style: SpanStyle { font_family: Some("Fig Only Serif".into()), italic: Some(true), ..Default::default() } }];
    let id = h.state_mut().doc.insert(0, None, None, t);
    h.state_mut().sel = vec![id];
    h.run_steps(2);
    let missing = crate::text::missing_fonts(&h.state().doc);
    assert_eq!(missing, vec![("Fig Only Sans".to_string(), vec![(700, false)]), ("Fig Only Serif".to_string(), vec![(700, true)])]);
    // The Typography section flags it and opens Replace fonts.
    h.get_by_label("Missing fonts");
    h.get_by_label("Replace fonts…").click();
    h.run_steps(2);
    assert_eq!(h.state().replace_fonts.as_ref().map(|p| p.len()), Some(2));
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Replace fonts").click();
    h.run_steps(2);
    let n = h.state().doc.node(id).clone();
    assert_eq!((n.font_family.as_str(), n.font_weight, n.spans[0].style.font_family.as_deref()), ("Inter", 700, Some("Inter")));
    assert!(crate::text::missing_fonts(&h.state().doc).is_empty() && h.state().replace_fonts.is_none());
    assert!(h.query_by_label("Missing fonts").is_none());
    key(&mut h, Modifiers::COMMAND, Key::Z);
    assert_eq!(crate::text::missing_fonts(&h.state().doc).len(), 2, "one undo step");
}

#[test]
fn text_links_add_edit_remove_export_and_click_through() {
    let mut h = harness();
    let f = draw(&mut h, Key::F, (0.0, 0.0), (300.0, 100.0));
    let mut t = Node::new(Kind::Text, r(10., 10., 10., 10.));
    (t.text, t.font_size) = ("Visit site".into(), 20.0);
    let id = h.state_mut().doc.insert(0, Some(f), None, t);
    h.state_mut().sel = vec![id];
    h.state_mut().editing_text = Some(id);
    h.state_mut().focus_text = true;
    h.run_steps(3);
    // Cmd K on selected characters opens the link field instead of quick actions.
    select_chars(&mut h, 6, 10);
    key(&mut h, Modifiers::COMMAND, Key::K);
    assert!(h.state().quick.is_none());
    assert_eq!(h.state().link_edit, Some((6, 10, String::new())));
    h.event(Event::Text("example.com".into()));
    key(&mut h, Modifiers::NONE, Key::Enter);
    let n = h.state().doc.node(id).clone();
    assert_eq!(n.text, "Visit site");
    assert_eq!(crate::doc::link_at(&n.spans, 7), Some((6, 10, "https://example.com".into())));
    assert!(crate::doc::link_at(&n.spans, 2).is_none());
    // Exported as an <a> around the same run.
    let svg = crate::export::to_svg(&h.state().doc, &[id]);
    assert!(svg.contains(r##"<a href="https://example.com"><tspan fill="#0D99FF" text-decoration="underline">site</tspan></a>"##), "{svg}");
    assert!(svg.contains(">Visit </tspan>"), "{svg}");
    // The caret in the link shows it, with Remove.
    select_chars(&mut h, 8, 8);
    h.run_steps(2);
    assert!(h.query_by_label("https://example.com").is_some());
    // Clicking it in presentation opens it.
    h.state_mut().editing_text = None;
    h.state_mut().sel = vec![f];
    key(&mut h, Modifiers::COMMAND | Modifiers::ALT, Key::Enter);
    h.run_steps(2);
    let painter = egui::Painter::new(h.ctx.clone(), egui::LayerId::background(), Rect::EVERYTHING);
    let n = h.state().doc.node(id).clone();
    let (g, _, _) = crate::text::layout(&painter, &n, 1.0, egui::Color32::BLACK);
    let w = n.rect.min + crate::text::draw_offset(&n, g.rows.len()) + g.pos_from_cursor(egui::text::CCursor::new(8)).center().to_vec2();
    assert_eq!(crate::text::link_at(&painter, &n, w).as_deref(), Some("https://example.com"));
    let at = ps(&h, w.x, w.y);
    move_to(&mut h, at, Modifiers::NONE);
    press(&mut h, at, true, Modifiers::NONE);
    // Released in a single frame, so its output is the one to read.
    h.input_mut().events.push(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.step();
    let opened = h.output().platform_output.commands.iter().any(|c| matches!(c, egui::OutputCommand::OpenUrl(u) if u.url == "https://example.com"));
    assert!(opened, "{:?}", h.output().platform_output.commands);
    key(&mut h, Modifiers::NONE, Key::Escape);
    // Remove takes it off again, in one undo step.
    h.state_mut().editing_text = Some(id);
    h.state_mut().focus_text = true;
    h.run_steps(3);
    select_chars(&mut h, 8, 8);
    h.run_steps(2);
    h.get_by_label("Remove link").click();
    h.run_steps(2);
    assert!(crate::doc::link_at(&h.state().doc.node(id).spans, 8).is_none());
    assert!(h.state().doc.node(id).spans.is_empty());
}

#[test]
fn paragraph_indent_moves_first_lines_in() {
    use crate::doc::TextSizing;
    let mut h = harness();
    let mut n = Node::new(Kind::Text, r(0., 0., 10., 10.));
    (n.text, n.font_size) = ("alpha beta gamma\nsecond".into(), 10.0);
    let plain = h.state_mut().doc.insert(0, None, None, n.clone());
    n.paragraph_indent = 20.0;
    let id = h.state_mut().doc.insert(0, None, None, n.clone());
    // Wrapped: only each paragraph's first row starts in.
    n.text_sizing = TextSizing::AutoHeight;
    n.rect = r(0., 100., 70., 10.);
    let wrapped = h.state_mut().doc.insert(0, None, None, n);
    h.run_steps(3);
    let w = |id| h.state().doc.node(id).rect.width();
    assert!((w(id) - w(plain) - 20.0).abs() < 0.5, "auto width grows by the indent: {} vs {}", w(id), w(plain));
    let painter = egui::Painter::new(h.ctx.clone(), egui::LayerId::background(), Rect::EVERYTHING);
    let t = h.state().doc.node(wrapped).clone();
    let (g, _, _) = crate::text::layout(&painter, &t, 1.0, egui::Color32::BLACK);
    let starts: Vec<f32> = g.rows.iter().map(|r| r.pos.x + r.glyphs[0].pos.x).collect();
    assert!(starts.len() >= 3, "{:?}", t.lines);
    assert!((starts[0] - 20.0).abs() < 0.5 && starts[1] < 1.0 && (starts.last().unwrap() - 20.0).abs() < 0.5, "{starts:?}");
    let svg = crate::export::to_svg(&h.state().doc, &[id]);
    assert_eq!(svg.matches(r#"x="20""#).count(), 2, "{svg}");
}

#[test]
fn copy_and_paste_properties_survive_egui_clipboard_events() {
    use crate::app::TEST_CLIPBOARD;
    let mut h = harness();
    let a = draw(&mut h, Key::R, (0.0, 0.0), (40.0, 40.0));
    let b = draw(&mut h, Key::R, (100.0, 0.0), (140.0, 40.0));
    h.state_mut().doc.node_mut(a).opacity = 0.5;
    h.state_mut().sel = vec![a];
    // The windowing layer sends Alt Cmd C and Alt Cmd V as Copy and Paste, never as C and V keys.
    let am = Modifiers::COMMAND | Modifiers::ALT;
    clip_event(&mut h, am, Event::Copy);
    assert!(h.state().style_clip.is_some(), "Alt Cmd C copies properties");
    h.state_mut().sel = vec![b];
    clip_event(&mut h, am, Event::Paste("some text".into()));
    assert_eq!(h.state().doc.node(b).opacity, 0.5, "Alt Cmd V pastes properties");
    assert_eq!(h.state().doc.nodes.len(), 2, "and doesn't paste the clipboard text");
    let release = Event::Key { key: Key::V, physical_key: None, pressed: false, repeat: false, modifiers: am };
    clip_event(&mut h, am, release.clone());
    // With no text on the clipboard only the V key's release arrives.
    h.state_mut().doc.node_mut(b).opacity = 1.0;
    TEST_CLIPBOARD.with(|c| c.borrow_mut().1 = Some(image::RgbaImage::new(2, 2)));
    clip_event(&mut h, am, release);
    TEST_CLIPBOARD.with(|c| c.borrow_mut().1 = None);
    assert_eq!(h.state().doc.node(b).opacity, 0.5);
    assert_eq!(h.state().doc.nodes.len(), 2, "no image pasted");
}
