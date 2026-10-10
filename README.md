<p align="center"><img src="docs/banner.png" alt="Katachi" width="720"></p>

Katachi (形, "form") is a native design tool with Figma's workflow: the same canvas behavior, selection rules,
tools, panels, and keyboard shortcuts. It is written in Rust, runs offline, and stores files on your disk.

It is a clean-room implementation. No Figma code, assets, or file internals are used.

![Katachi](docs/screenshot.png)

## Install

**macOS**

```sh
git clone https://github.com/dyascj/katachi && cd katachi
scripts/macos-app.sh --install   # builds Katachi.app into /Applications
```

**Any platform**

```sh
cargo install --path .
katachi [file.katachi]
```

Tagged releases (`v*`) build a universal macOS `.dmg`, a Windows `.zip`, and a Linux `.tar.gz` through
GitHub Actions. Building from source needs Rust 1.92 or newer.

## Features

**Canvas and tools**
- Infinite canvas with pan, zoom, pixel grid at 800%+, rulers (Shift R), and smart guides
- Move, Scale (K), Frame, Section, Slice, Rectangle, Line, Arrow, Ellipse, Polygon, Star, Pen, Pencil, Text, Comment
- Figma's selection rules: click-through top-level frames, groups and instances select first, double-click to dive in, Cmd-click for deep select, pressing inside a selected parent drags the parent
- Resize from any edge or corner, rotate past the corners (Shift snaps to 15°), Alt-drag to duplicate
- Alt-hover distance measurements, flip (Shift H/V), distribute and tidy up, alignment

**Layout**
- Auto layout: direction, gap or auto gap, padding, 3x3 alignment, hug / fill / fixed sizing, absolute children, live drag reordering (Shift A)
- Constraints (left, right, left and right, center, scale), with Cmd to resize without them
- Layout guides: grids, columns, and rows, with snapping (Ctrl G to show or hide)

**Design systems**
- Components and instances that keep overrides through main-component edits
- Variants in component sets, with property dropdowns that keep overrides when switching
- Boolean, text, and instance-swap component properties
- Color, text, and effect styles
- Variables with collections and modes (for example Light and Dark), aliases, and bindings for colors, sizes, spacing, radius, text, and visibility
- Assets panel to drag components onto the canvas

**Vectors**
- Multi-contour paths with even-odd or non-zero fill, vector edit mode for anchors and handles
- Live boolean groups: union, subtract, intersect, exclude
- Flatten (Cmd E), SVG import by paste, drag-and-drop, or File > Place

**Paint and effects**
- Solid, linear, and radial gradient fills, and images (fill, fit, stretch, tile)
- Strokes with inside / center / outside alignment, dashes, caps, joins, and arrow markers
- Drop shadow, inner shadow, layer blur, background blur, blend modes, masks, independent corner radii

**Text**
- System fonts, auto width / auto height / fixed boxes, alignment, line height, letter spacing, decoration, and case

**Prototyping**
- Interactions (click, hover, press, after delay) with navigate, back, overlays, and links
- Transitions: instant, dissolve, smart animate, move in, push, slide in
- Present mode (Cmd Alt Enter) with flows, hotspot hints, and overlays

**Handoff and review**
- Dev Mode (Shift D) with CSS, SwiftUI, and Jetpack Compose code, colors, and type
- Comments pinned to frames, with replies and resolve

**Files**
- `.katachi` files (JSON), atomic saves, crash-recovery autosave, version history, open recent
- Export presets per layer to PNG, JPG, SVG, and PDF

| Auto layout | Components and variants |
| --- | --- |
| ![Auto layout](docs/auto-layout.png) | ![Components](docs/components.png) |
| **Prototyping** | **Dev Mode** |
| ![Prototype](docs/prototype.png) | ![Dev Mode](docs/dev-mode.png) |

## Shortcuts

| Action | Keys |
| --- | --- |
| Move, Scale, Hand | V, K, H |
| Frame, Section, Slice | F or A, Shift S, S |
| Rectangle, Line, Arrow, Ellipse | R, L, Shift L, O |
| Pen, Pencil, Text, Comment | P, Shift P, T, C |
| Undo, redo | Cmd Z, Shift Cmd Z |
| Copy, paste, cut, duplicate | Cmd C, Cmd V, Cmd X, Cmd D |
| Copy and paste properties | Alt Cmd C, Alt Cmd V |
| Group, frame selection, ungroup | Cmd G, Alt Cmd G, Shift Cmd G |
| Auto layout, remove auto layout | Shift A, Alt Shift A |
| Create component, detach instance | Alt Cmd K, Alt Cmd B |
| Union, subtract, intersect, exclude | Alt Shift U, S, I, E |
| Flatten, use as mask | Cmd E, Ctrl Cmd M |
| Bring forward, send backward | Cmd ], Cmd [ (add Alt for front and back) |
| Flip horizontal, vertical | Shift H, Shift V |
| Distribute, tidy up | Ctrl Alt H / V, Ctrl Alt T |
| Show or hide, lock | Shift Cmd H, Shift Cmd L |
| Next and previous layer, select parent | Tab, Shift Tab, Shift Enter |
| Zoom to fit, 100%, selection | Shift 1, Shift 0, Shift 2 |
| Rulers, layout guides, hide UI | Shift R, Ctrl G, Cmd \ |
| Dev Mode, present | Shift D, Alt Cmd Enter |
| Save, open, place image, export | Cmd S, Cmd O, Shift Cmd K, Shift Cmd E |

## Roadmap

- Real-time multiplayer
- Plugins
- Importing `.fig` files
- Rich text (mixed styles in one text box, lists)

## Development

```sh
cargo test                        # model, export, and headless UI interaction tests
cargo test shots -- --ignored     # render scripted screenshots to target/shots
cargo test --release perf -- --ignored --nocapture   # 5,000-layer frame timing
```

Code map:

| File | What it does |
| --- | --- |
| `src/doc.rs` | Document model: node tree, auto layout, constraints, components, styles, variables |
| `src/render.rs` | lyon tessellation into egui meshes, textures, raster fallback for blurs and blending |
| `src/canvas.rs` | Canvas input, tools, selection rules, snapping, overlays |
| `src/panels.rs` | Layers, Design panel, toolbar, icons |
| `src/export.rs` | SVG writer; PNG, JPG, and PDF via resvg and svg2pdf |
| `src/prototype.rs` | Present mode, interactions, transitions |
| `src/codegen.rs` | Dev Mode code generation |
| `src/text.rs` | Shared text layout and system fonts |
| `src/boolean.rs`, `src/vector_edit.rs`, `src/svg_import.rs` | Vector features |

Inter is bundled under the SIL Open Font License (`assets/fonts/Inter-LICENSE.txt`).
