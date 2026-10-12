// The promo film's design file: "Lumen", a made-up music app. Built with Katachi's plugin API.
const C = { violet: "#8E72FF", coral: "#FF5A3C", ink: "#16141F", sun: "#FFC94D", mint: "#3DDC97", sky: "#4DA8FF", pink: "#FF6FB5", paper: "#FFFFFF", mist: "#F2F0FA" };
const solid = (h, a = 1) => figma.util.solidPaint(h, { opacity: a });
const grad = (a, b, angle = 0) => ({ type: "GRADIENT_LINEAR", gradientStops: [{ position: 0, color: Object.assign(figma.util.rgb(a), { a: 1 }) }, { position: 1, color: Object.assign(figma.util.rgb(b), { a: 1 }) }], gradientTransform: angle ? [[0, 1, 0], [-1, 0, 1]] : [[1, 0, 0], [0, 1, 0]], opacity: 1, visible: true });
const shadow = (y = 18, r = 40, a = 0.18) => [{ type: "DROP_SHADOW", color: { r: 0.09, g: 0.08, b: 0.15, a }, offset: { x: 0, y }, radius: r, spread: 0, visible: true }];
function frame(name, x, y, w, h, fill, parent) {
  const f = figma.createFrame(); f.name = name; f.resize(w, h); f.x = x; f.y = y; f.fills = fill ? [fill] : []; f.clipsContent = true;
  if (parent) parent.appendChild(f);
  return f;
}
function rect(name, x, y, w, h, fill, r = 0, parent) { const n = figma.createRectangle(); n.name = name; n.resize(w, h); n.x = x; n.y = y; n.fills = [fill]; n.cornerRadius = r; if (parent) parent.appendChild(n); return n; }
function circle(name, x, y, d, fill, parent) { const n = figma.createEllipse(); n.name = name; n.resize(d, d); n.x = x; n.y = y; n.fills = [fill]; if (parent) parent.appendChild(n); return n; }
function text(s, x, y, size, color, style = "Regular", parent, w) {
  const t = figma.createText(); t.characters = s; t.fontSize = size; t.fontName = { family: "Inter", style }; t.fills = [solid(color)]; t.x = x; t.y = y;
  if (w) { t.textAutoResize = "HEIGHT"; t.resize(w, size); }
  if (parent) parent.appendChild(t);
  return t;
}
function pill(label, x, y, bg, fg, parent, w = 150) {
  const b = figma.createFrame(); b.name = label; parent.appendChild(b); b.x = x; b.y = y; b.fills = [bg]; b.cornerRadius = 999;
  b.layoutMode = "HORIZONTAL"; b.paddingLeft = b.paddingRight = 26; b.paddingTop = b.paddingBottom = 14; b.primaryAxisAlignItems = "CENTER"; b.counterAxisAlignItems = "CENTER";
  b.primaryAxisSizingMode = "AUTO"; b.counterAxisSizingMode = "AUTO";
  const t = figma.createText(); t.characters = label; t.fontSize = 18; t.fontName = { family: "Inter", style: "Semi Bold" }; t.fills = [solid(fg)]; b.appendChild(t);
  return b;
}

// Landing page.
const L = frame("Lumen · Landing", 0, 0, 1280, 800, grad(C.violet, C.coral));
circle("Glow", 760, -180, 620, solid("#FFFFFF", 0.14), L);
circle("Glow", -160, 520, 480, solid(C.sun, 0.22), L);
text("lumen", 64, 40, 28, C.paper, "Bold", L);
["Product", "Artists", "Pricing", "Blog"].forEach((s, i) => text(s, 760 + i * 110, 46, 17, C.paper, "Medium", L));
text("Make something\nwonderful.", 64, 200, 84, C.paper, "Bold", L);
text("Music made together, mixed live and shared anywhere.", 68, 410, 22, C.paper, "Regular", L, 520);
pill("Start free", 64, 500, solid(C.paper), C.ink, L);
const ghost = pill("Watch the film", 240, 500, solid("#FFFFFF", 0.16), C.paper, L); ghost.strokes = [solid("#FFFFFF", 0.6)]; ghost.strokeWeight = 1.5;
// Floating cards.
const card = frame("Now playing card", 760, 200, 400, 250, solid(C.paper), L); card.cornerRadius = 28; card.effects = shadow();
rect("Art", 24, 24, 120, 120, grad(C.sun, C.pink, 1), 20, card);
text("Golden Hour", 168, 40, 26, C.ink, "Bold", card); text("Mara Lin · Sunsets EP", 168, 78, 16, "#6B6880", "Regular", card);
rect("Track", 24, 180, 352, 8, solid(C.mist), 4, card); rect("Progress", 24, 180, 220, 8, grad(C.violet, C.coral), 4, card);
circle("Play", 300, 30, 76, solid(C.ink), card); rect("Play glyph", 330, 54, 20, 28, solid(C.paper), 3, card);
const stats = frame("Listeners card", 860, 480, 300, 200, solid(C.paper), L); stats.cornerRadius = 24; stats.effects = shadow(14, 32, 0.16);
text("Listeners", 24, 22, 16, "#6B6880", "Medium", stats); text("48.2k", 24, 44, 40, C.ink, "Bold", stats);
[40, 62, 50, 84, 70, 96, 78].forEach((h, i) => rect("Bar", 24 + i * 36, 180 - h, 22, h, i === 5 ? grad(C.violet, C.coral, 1) : solid(C.mist), 6, stats));

// Phone screens.
const phone = (name, x) => { const p = frame(name, x, 0, 390, 844, solid(C.paper)); p.cornerRadius = 44; p.effects = shadow(30, 60, 0.12); return p; };
const p1 = phone("Lumen · Now playing", 1400);
rect("Art", 32, 96, 326, 326, grad(C.violet, C.pink, 1), 32, p1);
circle("Sun", 180, 150, 120, solid(C.sun, 0.9), p1);
text("Golden Hour", 32, 452, 32, C.ink, "Bold", p1); text("Mara Lin", 32, 496, 18, "#6B6880", "Regular", p1);
rect("Track", 32, 556, 326, 6, solid(C.mist), 3, p1); rect("Progress", 32, 556, 190, 6, grad(C.violet, C.coral), 3, p1);
circle("Prev", 70, 620, 64, solid(C.mist), p1); circle("Play", 155, 606, 92, grad(C.violet, C.coral, 1), p1); circle("Next", 266, 620, 64, solid(C.mist), p1);
const p2 = phone("Lumen · Library", 1850);
text("Library", 32, 80, 40, C.ink, "Bold", p2);
[["Late Night Drive", C.sky, C.violet], ["Sunsets EP", C.sun, C.coral], ["Focus Flow", C.mint, C.sky], ["Rainy Mornings", C.pink, C.violet], ["Weekend Mix", C.coral, C.sun]].forEach(([n, a, b], i) => {
  rect("Cover", 32, 160 + i * 100, 76, 76, grad(a, b, 1), 18, p2);
  text(n, 128, 172 + i * 100, 20, C.ink, "Semi Bold", p2); text(`${12 + i * 7} songs`, 128, 202 + i * 100, 15, "#6B6880", "Regular", p2);
});
const p3 = phone("Lumen · Profile", 2300);
rect("Header", 0, 0, 390, 300, grad(C.coral, C.sun, 1), 0, p3);
circle("Avatar", 135, 210, 120, solid(C.ink), p3); text("CJ", 168, 248, 40, C.paper, "Bold", p3);
text("CJ Dyas", 128, 350, 30, C.ink, "Bold", p3);
[["1.2k", "Followers"], ["348", "Following"], ["27", "Mixes"]].forEach(([v, l], i) => { text(v, 40 + i * 120, 430, 28, C.ink, "Bold", p3); text(l, 40 + i * 120, 466, 14, "#6B6880", "Medium", p3); });

// Design system with a button component set.
const ds = frame("Lumen · Design system", 0, 960, 2690, 620, solid(C.paper)); ds.cornerRadius = 32;
text("Design system", 64, 56, 44, C.ink, "Bold", ds);
Object.entries(C).forEach(([n, h], i) => { circle(n, 64 + i * 110, 160, 84, solid(h), ds); text(n, 64 + i * 110, 256, 15, "#6B6880", "Medium", ds); });
text("Aa", 1120, 140, 120, C.ink, "Bold", ds); text("Inter · Bold, Semi Bold, Regular", 1124, 290, 18, "#6B6880", "Regular", ds);
const variants = [];
[["Primary", grad(C.violet, C.coral), C.paper], ["Dark", solid(C.ink), C.paper], ["Soft", solid(C.mist), C.ink]].forEach(([v, bg, fg], i) => {
  const c = figma.createComponent(); c.name = `Style=${v}`; c.x = 1600 + i * 240; c.y = 1120; c.fills = [bg]; c.cornerRadius = 999;
  c.layoutMode = "HORIZONTAL"; c.paddingLeft = c.paddingRight = 28; c.paddingTop = c.paddingBottom = 16; c.primaryAxisSizingMode = "AUTO"; c.counterAxisSizingMode = "AUTO";
  const t = figma.createText(); t.characters = "Get started"; t.fontSize = 18; t.fontName = { family: "Inter", style: "Semi Bold" }; t.fills = [solid(fg)]; c.appendChild(t);
  variants.push(c);
});
const set = figma.combineAsVariants(variants, ds); set.name = "Button"; set.x = 1580; set.y = 140;
[["Chill", C.sky], ["Indie", C.pink], ["Lo-fi", C.mint], ["Jazz", C.sun]].forEach(([n, h], i) => { const ch = pill(n, 64 + i * 140, 400, solid(h, 0.18), C.ink, ds); });
// Sticky notes.
const note = (s, x, y, color, rot) => { const n = frame("Sticky", x, y, 220, 160, solid(color)); n.cornerRadius = 6; n.rotation = rot; n.effects = shadow(8, 18, 0.14); text(s, 20, 20, 20, C.ink, "Semi Bold", n, 180); return n; };
note("Love this gradient! (Ava)", 2780, 60, "#FFE58A", -4);
note("Ship it Friday?", 2800, 300, "#B8F2D4", 3);
return "ok";
