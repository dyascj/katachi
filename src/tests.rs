//! Headless interaction tests: real pointer/keyboard events through the whole app.

use crate::app::{App, Tool};
use crate::doc::{Kind, Node, NodeId};
use egui::{Event, Key, Modifiers, PointerButton, Pos2, Rect, pos2, vec2};
use egui_kittest::Harness;

/// Keep autosaves and recents out of the real user folder.
fn isolate_data() {
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
    let left = crate::files::orphaned_recoveries(std::path::Path::new("none"));
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
    let settle = |a: &mut Harness<App>, b: &mut Harness<App>| {
        for _ in 0..60 {
            a.step();
            b.step();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    };
    settle(&mut a, &mut b);
    assert!(b.state().doc.nodes.contains_key(&ra), "Bo sees Ada's rectangle");
    assert!(a.state().doc.nodes.contains_key(&rb), "Ada sees Bo's ellipse");
    assert!(a.state().collab.as_ref().unwrap().peers.values().any(|p| p.name == "Bo"), "presence");
    // Ada undoes: her rectangle goes away everywhere, Bo's ellipse stays.
    key(&mut a, Modifiers::COMMAND, Key::Z);
    settle(&mut a, &mut b);
    assert!(!a.state().doc.nodes.contains_key(&ra) && !b.state().doc.nodes.contains_key(&ra));
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
