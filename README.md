# Katachi

A Figma-style design tool written in Rust. Native, fast, and offline.

![Katachi](docs/screenshot.png)

Katachi (形, "form") aims for Figma's workflow: the same canvas behavior, selection rules, tools, and
keyboard shortcuts. It is a clean-room implementation. No Figma code or assets are used.

## Install

**macOS (from source)**

```sh
git clone https://github.com/dyascj/katachi && cd katachi
scripts/macos-app.sh --install   # builds Katachi.app into /Applications
```

**Any platform (from source)**

```sh
cargo install --path .
katachi [file.katachi]
```

Tagged releases (`v*`) build a universal macOS `.dmg`, a Windows `.zip`, and a Linux `.tar.gz`
through GitHub Actions.

Requires Rust 1.92 or newer.

## What works today

| Area | Status |
| --- | --- |
| Infinite canvas | Pan (scroll, Space-drag, middle mouse, H), zoom (pinch, Cmd-scroll, Cmd +/-, Shift 0/1/2) |
| Tools | Move, Frame, Rectangle, Ellipse, Line, Pen (bezier handles, close on first point), Text, Hand |
| Drawing | Shift for square or 45 degrees, Alt from center, click for a 100x100 default |
| Selection | Click, Shift-click, marquee, Cmd-click deep select, double-click into groups, Esc to parent, Enter to children |
| Editing | Move, resize from any corner or edge (Shift keeps ratio, Alt from center), arrow nudge, Alt-drag duplicate |
| Smart guides | Snap to sibling and parent edges and centers, pixel-grid rounding |
| Frames | Clip content, nest by drawing or dropping inside, new frames adopt enclosed layers, frame labels |
| Structure | Group, ungroup, frame selection, z-order shortcuts, multiple pages |
| Layers panel | Tree, rename, show/hide, lock, drag to reorder or nest |
| Design panel | Align, X/Y/W/H, radius, opacity, multiple fills and strokes, stroke weight and alignment, text weight and size |
| Text | Inter (Regular to Bold), auto-width, edit in place |
| Clipboard | Copy, cut, paste, duplicate. Pasting plain text makes a text layer |
| Files | `.katachi` (JSON) save and open, undo/redo, unsaved-changes prompt |
| Export | PNG (0.5x to 4x) and SVG, per layer or per page |

## Shortcuts

| Action | Keys |
| --- | --- |
| Move, Frame, Rectangle, Ellipse, Line, Pen, Text, Hand | V, F or A, R, O, L, P, T, H |
| Undo, redo | Cmd Z, Shift Cmd Z |
| Copy, paste, cut, duplicate | Cmd C, Cmd V, Cmd X, Cmd D |
| Group, frame selection, ungroup | Cmd G, Alt Cmd G, Shift Cmd G |
| Bring forward, send backward | Cmd ], Cmd [ (add Alt for front/back) |
| Show/hide, lock | Shift Cmd H, Shift Cmd L |
| Zoom to fit, 100%, selection | Shift 1, Shift 0, Shift 2 |
| Save, save as, open, export | Cmd S, Shift Cmd S, Cmd O, Shift Cmd E |

## Roadmap to Figma parity

Roughly in order:

1. Rotation, flip, and per-corner radius
2. Vector edit mode (double-click a path to move anchors and handles), boolean operations
3. Gradients, image fills, drop shadows and blurs, blend modes
4. Fixed-width and auto-height text, line height, letter spacing, alignment, system fonts
5. Constraints and auto layout
6. Components, instances, and variants
7. Styles and variables
8. Prototyping (connections, overlays, presentation view)
9. Comments and real-time multiplayer
10. Plugin API
11. Import of `.fig` files and SVG paste

## Development

```sh
cargo test                        # model, export, and headless UI interaction tests
cargo test shots -- --ignored     # render scripted screenshots to target/shots
KATACHI_SCREENSHOT=out.png cargo run -- examples/demo.katachi
```

Code map:

- `src/doc.rs`: document model (pages, node tree, hit testing, grouping, clipboard fragments)
- `src/render.rs`: lyon tessellation of nodes into egui meshes
- `src/canvas.rs`: canvas input, tools, selection rules, snapping, overlays
- `src/panels.rs`: layers, design panel, toolbar, icons
- `src/export.rs`: SVG writer and PNG export via resvg
- `src/app.rs`: app state, undo, files, shortcuts

Inter is bundled under the SIL Open Font License (`assets/fonts/Inter-LICENSE.txt`).
