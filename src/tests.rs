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
    assert_eq!((n.kind, n.points.len(), n.closed), (Kind::Vector, 3, false));
    assert_eq!(n.rect, r(0., 0., 100., 100.));
    // Clicking the first anchor closes the shape.
    key(&mut h, Modifiers::NONE, Key::P);
    for (x, y) in [(300., 0.), (400., 0.), (350., 80.), (300., 0.)] {
        let p = s(&h, x, y);
        click(&mut h, p, Modifiers::NONE);
    }
    let id = h.state().sel[0];
    assert!(h.state().doc.node(id).closed);
    assert_eq!(h.state().doc.node(id).points.len(), 3);
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
    use crate::doc::{Paint, PaintKind, Rgba, Shadow};
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
