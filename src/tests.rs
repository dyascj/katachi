//! Headless interaction tests: real pointer/keyboard events through the whole app.

use crate::app::{App, Tool};
use crate::doc::{Kind, Node, NodeId};
use egui::{Event, Key, Modifiers, PointerButton, Pos2, Rect, pos2, vec2};
use egui_kittest::Harness;

fn harness() -> Harness<'static, App> {
    let mut h = Harness::builder().with_size(vec2(1440.0, 900.0)).with_step_dt(1.0 / 60.0).build_eframe(|cc| {
        crate::setup_style(&cc.egui_ctx);
        App::new(None)
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
