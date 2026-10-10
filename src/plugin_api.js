// Figma's plugin API, implemented over a JSON copy of the page that Katachi loads before each
// event and collects afterwards (see plugins.rs). Layers are plain objects with a shared prototype.

const __nodes = new Map();
let __removed = [], __notes = [], __closed = false, __newId = 0, __errors = [], __outbox = [], __ui = null, __storage = {}, __result;
let __styles = [], __collections = [], __variables = [], __fonts = [];
let __removedStyles = [], __removedVariables = [], __removedCollections = [];
const __handlers = {}, __uiHandlers = [];
const __mixed = Symbol("mixed");

function __newid() { return "new:" + (++__newId); }
function __copy(x) { return x === undefined ? undefined : JSON.parse(JSON.stringify(x)); }
function __err(e) { __errors.push(String(e) + (e && e.stack ? "\n" + e.stack : "")); }
function __reset() {
  __removed = []; __notes = []; __errors = []; __outbox = []; __result = undefined;
  __removedStyles = []; __removedVariables = []; __removedCollections = [];
}
function __safe(f) { try { Promise.resolve(f()).catch(__err); } catch (e) { __err(e); } }
function __deliver(json) {
  const msg = JSON.parse(json);
  if (typeof figma.ui.onmessage === "function") __safe(() => figma.ui.onmessage(msg, { origin: "null" }));
  for (const h of __uiHandlers) __safe(() => h(msg, { origin: "null" }));
}
function __emit(type, arg) { for (const h of __handlers[type] || []) __safe(() => h(arg)); }
function __listens(type) { return (__handlers[type] || []).length > 0; }
// A returned value as plain JSON: layers, styles, and variables become references.
function __plain(v, depth) {
  depth = depth || 0;
  if (v === null || typeof v !== "object" || depth > 20) return typeof v === "function" || typeof v === "symbol" ? undefined : v;
  const proto = Object.getPrototypeOf(v);
  if (proto === __NodeProto) return { id: v.id, type: v.type, name: v.name };
  if (proto === __StyleProto || proto === __VariableProto || proto === __CollectionProto) return { id: v.id, type: v.type || "VARIABLE_COLLECTION", name: v.name };
  if (Array.isArray(v)) return v.map(x => __plain(x, depth + 1));
  const o = {};
  for (const k of Object.keys(v)) o[k] = __plain(v[k], depth + 1);
  return o;
}

function __link(n) {
  Object.setPrototypeOf(n, __NodeProto);
  __nodes.set(n.id, n);
  return n;
}

const __NodeProto = {
  resize(w, h) {
    this.width = Math.max(0.01, w); this.height = Math.max(0.01, h);
    if (this.type === "TEXT" && this.textAutoResize === "WIDTH_AND_HEIGHT") this.textAutoResize = "NONE";
  },
  resizeWithoutConstraints(w, h) { this.resize(w, h); },
  rescale(k) { this.resize(this.width * k, this.height * k); if (this.fontSize) this.fontSize *= k; },
  remove() {
    __removed.push(this.id);
    if (this.parent) this.parent.children = this.parent.children.filter(c => c !== this);
    const drop = n => { __nodes.delete(n.id); (n.children || []).forEach(drop); };
    drop(this);
    this.removed = true;
  },
  appendChild(c) { this.insertChild((this.children || []).length, c); },
  insertChild(i, c) {
    if (!this.children) throw new Error(`${this.type} can't have children`);
    if (c.parent && c.parent.children) c.parent.children = c.parent.children.filter(x => x !== c);
    this.children.splice(Math.min(i, this.children.length), 0, c);
    c.parent = this;
  },
  clone() {
    const copy = (n, parent) => {
      const { parent: _p, children, ...rest } = n;
      const c = __link(Object.assign(__copy(rest), { id: __newid(), parent }));
      if (children) c.children = children.map(k => copy(k, c));
      if (n.type === "INSTANCE") c.__cloneOf = n.id;
      return c;
    };
    const c = copy(this, this.parent);
    if (this.parent) { const s = this.parent.children; s.splice(s.indexOf(this) + 1, 0, c); }
    return c;
  },
  findAll(fn) {
    const out = [];
    const walk = n => { for (const c of n.children || []) { if (!fn || fn(c)) out.push(c); walk(c); } };
    walk(this);
    return out;
  },
  findOne(fn) { return this.findAll(fn)[0] || null; },
  findChildren(fn) { return (this.children || []).filter(fn || (() => true)); },
  findChild(fn) { return this.findChildren(fn)[0] || null; },
  findAllWithCriteria(c) { return this.findAll(n => !c.types || c.types.includes(n.type)); },
  setPluginData(k, v) { (this.__pluginData = this.__pluginData || {})[k] = String(v); },
  getPluginData(k) { return (this.__pluginData || {})[k] || ""; },
  getPluginDataKeys() { return Object.keys(this.__pluginData || {}); },
  setSharedPluginData() {}, getSharedPluginData() { return ""; },
  setRelaunchData() {},
  get absoluteBoundingBox() {
    let x = this.x, y = this.y;
    for (let p = this.parent; p && p.type !== "PAGE"; p = p.parent) { x += p.x; y += p.y; }
    return { x, y, width: this.width, height: this.height };
  },
  get absoluteRenderBounds() { return this.absoluteBoundingBox; },
  get absoluteTransform() { const b = this.absoluteBoundingBox; return [[1, 0, b.x], [0, 1, b.y]]; },
  get relativeTransform() { return [[1, 0, this.x], [0, 1, this.y]]; },
  set relativeTransform(t) { this.x = t[0][2]; this.y = t[1][2]; },
  // Styles and variables.
  setFillStyleIdAsync(id) { this.fillStyleId = id; return Promise.resolve(); },
  setStrokeStyleIdAsync(id) { this.strokeStyleId = id; return Promise.resolve(); },
  setTextStyleIdAsync(id) { this.textStyleId = id; return Promise.resolve(); },
  setEffectStyleIdAsync(id) { this.effectStyleId = id; return Promise.resolve(); },
  setBoundVariable(field, v) {
    this.boundVariables = this.boundVariables || {};
    if (v) this.boundVariables[field] = { type: "VARIABLE_ALIAS", id: typeof v === "string" ? v : v.id };
    else delete this.boundVariables[field];
  },
  setExplicitVariableModeForCollection(c, modeId) {
    this.explicitVariableModes = this.explicitVariableModes || {};
    this.explicitVariableModes[typeof c === "string" ? c : c.id] = modeId;
  },
  clearExplicitVariableModeForCollection(c) { if (this.explicitVariableModes) delete this.explicitVariableModes[typeof c === "string" ? c : c.id]; },
  // Text ranges: styles for some characters.
  __range(start, end, style) {
    this.__spans = this.__spans || [];
    this.__spans.push(Object.assign({ start, end }, style));
  },
  setRangeFontSize(s, e, size) { this.__range(s, e, { fontSize: size }); },
  setRangeFontName(s, e, font) { this.__range(s, e, { fontName: font }); },
  setRangeFills(s, e, fills) { this.__range(s, e, { fills: __copy(fills) }); },
  setRangeTextDecoration(s, e, d) { this.__range(s, e, { textDecoration: d }); },
  getRangeFontSize(s, e) { return this.fontSize; },
  getRangeFontName(s, e) { return this.fontName; },
  getRangeFills(s, e) { return this.fills; },
  insertCharacters(i, t) { this.characters = this.characters.slice(0, i) + t + this.characters.slice(i); },
  deleteCharacters(s, e) { this.characters = this.characters.slice(0, s) + this.characters.slice(e); },
  // Components and instances.
  createInstance() {
    if (this.type !== "COMPONENT") throw new Error("createInstance needs a component");
    const comp = this;
    // Provisional copies of the component's layers, so scripts can override them right away.
    const prov = (n, parent) => {
      const { parent: _p, children, ...rest } = n;
      const c = __link(Object.assign(__copy(rest), { id: __newid(), parent, __src: n.id }));
      if (children) c.children = children.map(k => prov(k, c));
      return c;
    };
    const set = comp.parent && comp.parent.type === "COMPONENT_SET" ? comp.parent : null;
    const inst = __create("INSTANCE", Object.assign(__nodeDefaults(comp), {
      name: set ? set.name : comp.name, mainComponentId: comp.id, componentProperties: {},
      x: comp.absoluteBoundingBox.x, y: comp.absoluteBoundingBox.y,
    }));
    inst.children = (comp.children || []).map(k => prov(k, inst));
    return inst;
  },
  get mainComponent() { return this.mainComponentId ? __nodes.get(this.mainComponentId) || null : null; },
  getMainComponentAsync() { return Promise.resolve(this.mainComponent); },
  swapComponent(c) { this.mainComponentId = c.id; this.children = []; },
  detachInstance() { this.type = "FRAME"; this.mainComponentId = undefined; return this; },
  resetOverrides() { this.__reset = true; },
  setProperties(props) {
    this.__setProps = Object.assign({}, this.__setProps || {}, props);
    const cp = this.componentProperties || {};
    for (const k of Object.keys(props)) cp[k] = Object.assign({}, cp[k] || {}, { value: props[k] });
    this.componentProperties = cp;
  },
  addComponentProperty(name, type, value) {
    this.componentPropertyDefinitions = this.componentPropertyDefinitions || {};
    this.componentPropertyDefinitions[name] = { type, defaultValue: value };
    return name;
  },
  editComponentProperty(name, def) {
    const d = this.componentPropertyDefinitions || {};
    const old = d[name];
    if (!old) throw new Error(`no property ${name}`);
    const next = def.name || name;
    delete d[name];
    d[next] = { type: old.type, defaultValue: def.defaultValue !== undefined ? def.defaultValue : old.defaultValue };
    return next;
  },
  deleteComponentProperty(name) { delete (this.componentPropertyDefinitions || {})[name]; },
};

function __nodeDefaults(src) {
  const keep = ["width", "height", "fills", "strokes", "strokeWeight", "strokeAlign", "cornerRadius", "effects", "opacity",
    "layoutMode", "itemSpacing", "paddingLeft", "paddingRight", "paddingTop", "paddingBottom", "primaryAxisAlignItems",
    "counterAxisAlignItems", "primaryAxisSizingMode", "counterAxisSizingMode", "clipsContent", "layoutWrap", "counterAxisSpacing"];
  const o = {};
  for (const k of keep) if (src[k] !== undefined) o[k] = __copy(src[k]);
  return o;
}

const __solid = (r, g, b) => [{ type: "SOLID", color: { r, g, b }, opacity: 1, visible: true, blendMode: "NORMAL" }];
const __base = {
  x: 0, y: 0, width: 100, height: 100, rotation: 0, opacity: 1, visible: true, locked: false, blendMode: "PASS_THROUGH", isMask: false,
  fills: [], strokes: [], strokeWeight: 1, strokeAlign: "INSIDE", dashPattern: [], strokeCap: "NONE", strokeJoin: "MITER",
  effects: [], cornerRadius: 0, constraints: { horizontal: "MIN", vertical: "MIN" },
  layoutPositioning: "AUTO", layoutGrow: 0, layoutAlign: "INHERIT",
  minWidth: null, maxWidth: null, minHeight: null, maxHeight: null,
};
const __frameBase = {
  layoutMode: "NONE", layoutWrap: "NO_WRAP", itemSpacing: 0, counterAxisSpacing: 0,
  paddingLeft: 0, paddingRight: 0, paddingTop: 0, paddingBottom: 0,
  primaryAxisAlignItems: "MIN", counterAxisAlignItems: "MIN", primaryAxisSizingMode: "AUTO", counterAxisSizingMode: "FIXED",
  clipsContent: true, strokesIncludedInLayout: false,
};
function __create(type, props) {
  const frameish = ["FRAME", "COMPONENT", "COMPONENT_SET", "INSTANCE", "SECTION"].includes(type);
  const n = __link(Object.assign({ id: __newid(), type, name: type[0] + type.slice(1).toLowerCase().replace(/_/g, " ") },
    __copy(__base), frameish ? __copy(__frameBase) : {},
    { children: frameish || type === "GROUP" || type === "BOOLEAN_OPERATION" ? [] : undefined }, props || {}));
  figma.currentPage.appendChild(n);
  return n;
}
function __textDefaults() {
  return { characters: "", fontSize: 12, fontName: { family: "Inter", style: "Regular" }, textAlignHorizontal: "LEFT", textAlignVertical: "TOP",
    textAutoResize: "WIDTH_AND_HEIGHT", lineHeight: { unit: "AUTO" }, letterSpacing: { unit: "PERCENT", value: 0 }, textCase: "ORIGINAL",
    textDecoration: "NONE", paragraphSpacing: 0, textTruncation: "DISABLED", maxLines: null, fills: __solid(0, 0, 0), width: 0, height: 0 };
}
function __bool(op, nodes, parent, index) {
  const b = __create("BOOLEAN_OPERATION", { booleanOperation: op, fills: __copy(nodes[0].fills || []) });
  (parent || figma.currentPage).insertChild(index === undefined ? (parent || figma.currentPage).children.length : index, b);
  for (const n of nodes) b.appendChild(n);
  return b;
}

// ---------- styles and variables ----------

const __StyleProto = {
  remove() { __styles = __styles.filter(s => s !== this); __removedStyles.push(this.id); },
  getStyleConsumersAsync() { return Promise.resolve([]); },
};
const __CollectionProto = {
  addMode(name) { const id = this.id.replace("VariableCollectionId:", "") + ":" + this.modes.length; this.modes.push({ modeId: id, name }); return id; },
  renameMode(id, name) { const m = this.modes.find(m => m.modeId === id); if (m) m.name = name; },
  removeMode(id) { this.modes = this.modes.filter(m => m.modeId !== id); },
  remove() { __collections = __collections.filter(c => c !== this); __removedCollections.push(this.id); },
  get defaultModeId() { return this.modes[0].modeId; },
  get variableIds() { return __variables.filter(v => v.variableCollectionId === this.id).map(v => v.id); },
};
const __VariableProto = {
  setValueForMode(modeId, value) { this.valuesByMode[modeId] = __copy(value); },
  remove() { __variables = __variables.filter(v => v !== this); __removedVariables.push(this.id); },
  resolveForConsumer() { const c = __collections.find(c => c.id === this.variableCollectionId); return { value: this.valuesByMode[c.modes[0].modeId], resolvedType: this.resolvedType }; },
};
function __style(type, props) {
  const s = Object.setPrototypeOf(Object.assign({ id: "S:" + __newid(), type, name: type[0] + type.slice(1).toLowerCase() + " style", description: "" }, props), __StyleProto);
  __styles.push(s);
  return s;
}
const __byId = (list, id) => list.find(x => x.id === id) || null;

const figma = {
  apiVersion: "1.0.0", editorType: "figma", mode: "default", mixed: __mixed, fileKey: "katachi",
  notify(m) { __notes.push(String(m)); return { cancel() {} }; },
  closePlugin(m) { if (m) __notes.push(String(m)); __closed = true; },
  showUI(html, opts) {
    opts = opts || {};
    __ui = { html: String(html), width: opts.width || 300, height: opts.height || 200, visible: opts.visible !== false, title: opts.title || null };
  },
  ui: {
    postMessage(m) { __outbox.push(JSON.stringify(m === undefined ? null : m)); },
    onmessage: undefined,
    on(type, cb) { if (type === "message") __uiHandlers.push(cb); },
    once(type, cb) { this.on(type, cb); },
    off(type, cb) { const i = __uiHandlers.indexOf(cb); if (i >= 0) __uiHandlers.splice(i, 1); },
    resize(w, h) { if (__ui) { __ui.width = w; __ui.height = h; } },
    show() { if (__ui) __ui.visible = true; },
    hide() { if (__ui) __ui.visible = false; },
    close() { __ui = null; },
  },
  on(type, cb) { (__handlers[type] = __handlers[type] || []).push(cb); },
  once(type, cb) { this.on(type, cb); },
  off(type, cb) { const l = __handlers[type] || []; const i = l.indexOf(cb); if (i >= 0) l.splice(i, 1); },
  commitUndo() {}, triggerUndo() {},
  loadFontAsync() { return Promise.resolve(); },
  listAvailableFontsAsync() { return Promise.resolve(__fonts.flatMap(f => ["Regular", "Medium", "Semi Bold", "Bold"].map(style => ({ fontName: { family: f, style } })))); },
  getNodeById(id) { return __nodes.get(id) || null; },
  getNodeByIdAsync(id) { return Promise.resolve(__nodes.get(id) || null); },
  setCurrentPageAsync() { return Promise.resolve(); },
  createRectangle() { return __create("RECTANGLE", { fills: __solid(0.851, 0.851, 0.851) }); },
  createEllipse() { return __create("ELLIPSE", { fills: __solid(0.851, 0.851, 0.851) }); },
  createPolygon() { return __create("POLYGON", { fills: __solid(0.851, 0.851, 0.851), pointCount: 3 }); },
  createStar() { return __create("STAR", { fills: __solid(0.851, 0.851, 0.851), pointCount: 5, innerRadius: 0.38 }); },
  createLine() { return __create("LINE", { height: 0, strokes: __solid(0, 0, 0), strokeAlign: "CENTER" }); },
  createFrame() { return __create("FRAME", { fills: __solid(1, 1, 1) }); },
  createSection() { return __create("SECTION", { fills: __solid(1, 1, 1) }); },
  createComponent() { return __create("COMPONENT", { fills: __solid(1, 1, 1), componentPropertyDefinitions: {} }); },
  createText() { return __create("TEXT", __textDefaults()); },
  createNodeFromSvg(svg) {
    const num = re => { const m = svg.match(re); return m ? parseFloat(m[1]) : null; };
    const vb = svg.match(/viewBox="[\d.\-]+[ ,]+[\d.\-]+[ ,]+([\d.]+)[ ,]+([\d.]+)"/);
    const w = num(/<svg[^>]*\swidth="([\d.]+)/) || (vb ? parseFloat(vb[1]) : 24);
    const h = num(/<svg[^>]*\sheight="([\d.]+)/) || (vb ? parseFloat(vb[2]) : 24);
    return __create("FRAME", { __svg: String(svg), width: w, height: h, fills: [], name: "Svg" });
  },
  createComponentFromNode(n) {
    n.type = "COMPONENT"; n.componentPropertyDefinitions = n.componentPropertyDefinitions || {};
    Object.keys(__frameBase).forEach(k => { if (n[k] === undefined) n[k] = __copy(__frameBase[k]); });
    if (!n.children) n.children = [];
    return n;
  },
  combineAsVariants(nodes, parent, index) {
    const p = parent || figma.currentPage;
    const bs = nodes.map(n => n.absoluteBoundingBox), pad = 20;
    const x = Math.min(...bs.map(b => b.x)) - pad, y = Math.min(...bs.map(b => b.y)) - pad;
    const set = __create("COMPONENT_SET", { fills: [], componentPropertyDefinitions: {}, clipsContent: false,
      width: Math.max(...bs.map(b => b.x + b.width)) - x + pad, height: Math.max(...bs.map(b => b.y + b.height)) - y + pad });
    p.insertChild(index === undefined ? p.children.length : index, set);
    const origin = p.type === "PAGE" ? { x: 0, y: 0 } : p.absoluteBoundingBox;
    set.x = x - origin.x; set.y = y - origin.y;
    set.name = nodes[0].name.includes("=") ? "Component" : nodes[0].name;
    nodes.forEach((n, i) => {
      const b = n.absoluteBoundingBox;
      if (!n.name.includes("=")) n.name = `Property 1=${i === 0 ? "Default" : "Variant" + (i + 1)}`;
      set.appendChild(n); n.x = b.x - x; n.y = b.y - y;
    });
    return set;
  },
  group(nodes, parent, index) {
    const p = parent || figma.currentPage;
    const xs = nodes.map(n => n.absoluteBoundingBox), x = Math.min(...xs.map(b => b.x)), y = Math.min(...xs.map(b => b.y));
    const g = __create("GROUP", { children: [], fills: [] });
    p.insertChild(index === undefined ? p.children.length : index, g);
    const origin = g.parent.type === "PAGE" ? { x: 0, y: 0 } : g.parent.absoluteBoundingBox;
    g.x = x - origin.x; g.y = y - origin.y;
    g.width = Math.max(...xs.map(b => b.x + b.width)) - x; g.height = Math.max(...xs.map(b => b.y + b.height)) - y;
    for (const n of nodes) { const b = n.absoluteBoundingBox; g.appendChild(n); n.x = b.x - x; n.y = b.y - y; }
    return g;
  },
  union(nodes, parent, index) { return __bool("UNION", nodes, parent, index); },
  subtract(nodes, parent, index) { return __bool("SUBTRACT", nodes, parent, index); },
  intersect(nodes, parent, index) { return __bool("INTERSECT", nodes, parent, index); },
  exclude(nodes, parent, index) { return __bool("EXCLUDE", nodes, parent, index); },
  ungroup(g) { const p = g.parent, i = p.children.indexOf(g), kids = [...g.children]; kids.forEach((k, j) => { k.x += g.x; k.y += g.y; p.insertChild(i + j, k); }); g.remove(); return kids; },
  // Styles.
  createPaintStyle() { return __style("PAINT", { paints: [] }); },
  createTextStyle() { return __style("TEXT", { fontName: { family: "Inter", style: "Regular" }, fontSize: 12, lineHeight: { unit: "AUTO" }, letterSpacing: { unit: "PERCENT", value: 0 }, textCase: "ORIGINAL", textDecoration: "NONE" }); },
  createEffectStyle() { return __style("EFFECT", { effects: [] }); },
  getLocalPaintStyles() { return __styles.filter(s => s.type === "PAINT"); },
  getLocalTextStyles() { return __styles.filter(s => s.type === "TEXT"); },
  getLocalEffectStyles() { return __styles.filter(s => s.type === "EFFECT"); },
  getLocalPaintStylesAsync() { return Promise.resolve(this.getLocalPaintStyles()); },
  getLocalTextStylesAsync() { return Promise.resolve(this.getLocalTextStyles()); },
  getLocalEffectStylesAsync() { return Promise.resolve(this.getLocalEffectStyles()); },
  getStyleById(id) { return __byId(__styles, id); },
  getStyleByIdAsync(id) { return Promise.resolve(__byId(__styles, id)); },
  variables: {
    createVariableCollection(name) {
      const c = Object.setPrototypeOf({ id: "VariableCollectionId:" + __newid(), name, modes: [], hiddenFromPublishing: false }, __CollectionProto);
      c.modes.push({ modeId: c.id.replace("VariableCollectionId:", "") + ":0", name: "Mode 1" });
      __collections.push(c);
      return c;
    },
    createVariable(name, collection, type) {
      const c = typeof collection === "string" ? __byId(__collections, collection) : collection;
      if (!c) throw new Error("createVariable needs a collection");
      const zero = { COLOR: { r: 0, g: 0, b: 0, a: 1 }, FLOAT: 0, STRING: "", BOOLEAN: false }[type];
      const v = Object.setPrototypeOf({ id: "VariableID:" + __newid(), name, variableCollectionId: c.id, resolvedType: type, valuesByMode: {}, description: "", scopes: ["ALL_SCOPES"] }, __VariableProto);
      for (const m of c.modes) v.valuesByMode[m.modeId] = __copy(zero);
      __variables.push(v);
      return v;
    },
    getLocalVariableCollections() { return __collections; },
    getLocalVariables(type) { return __variables.filter(v => !type || v.resolvedType === type); },
    getLocalVariableCollectionsAsync() { return Promise.resolve(__collections); },
    getLocalVariablesAsync(type) { return Promise.resolve(this.getLocalVariables(type)); },
    getVariableById(id) { return __byId(__variables, id); },
    getVariableByIdAsync(id) { return Promise.resolve(__byId(__variables, id)); },
    getVariableCollectionById(id) { return __byId(__collections, id); },
    getVariableCollectionByIdAsync(id) { return Promise.resolve(__byId(__collections, id)); },
    createVariableAlias(v) { return { type: "VARIABLE_ALIAS", id: v.id }; },
    createVariableAliasByIdAsync(id) { return Promise.resolve({ type: "VARIABLE_ALIAS", id }); },
    setBoundVariableForPaint(paint, field, v) {
      const p = __copy(paint);
      p.boundVariables = Object.assign({}, p.boundVariables || {});
      if (v) p.boundVariables[field] = { type: "VARIABLE_ALIAS", id: v.id }; else delete p.boundVariables[field];
      return p;
    },
    setBoundVariableForEffect(effect, field, v) { return __copy(effect); },
  },
  viewport: { center: { x: 0, y: 0 }, zoom: 1, bounds: { x: 0, y: 0, width: 1440, height: 900 }, scrollAndZoomIntoView() {} },
  clientStorage: {
    getAsync(k) { return Promise.resolve(__storage[k]); },
    setAsync(k, v) { __storage[k] = v; return Promise.resolve(); },
    deleteAsync(k) { delete __storage[k]; return Promise.resolve(); },
    keysAsync() { return Promise.resolve(Object.keys(__storage)); },
  },
  util: {
    rgb(c) { return typeof c === "string" ? figma.util.__hex(c, false) : { r: c.r, g: c.g, b: c.b }; },
    rgba(c) { return typeof c === "string" ? figma.util.__hex(c, true) : { r: c.r, g: c.g, b: c.b, a: c.a === undefined ? 1 : c.a }; },
    solidPaint(c, over) { const { r, g, b, a } = figma.util.rgba(c); return Object.assign({ type: "SOLID", color: { r, g, b }, opacity: a, visible: true, blendMode: "NORMAL" }, over || {}); },
    __hex(s, alpha) {
      let h = s.replace("#", "");
      if (h.length <= 4) h = h.split("").map(c => c + c).join("");
      const v = i => parseInt(h.slice(i, i + 2), 16) / 255;
      const o = { r: v(0), g: v(2), b: v(4) };
      if (alpha) o.a = h.length >= 8 ? v(6) : 1;
      return o;
    },
  },
  base64Encode() { throw new Error("base64Encode isn't supported"); },
};

function __page(name, id, loaded) {
  const p = Object.setPrototypeOf({ type: "PAGE", id, name, children: [], __loaded: loaded }, __NodeProto);
  let sel = [];
  Object.defineProperty(p, "selection", { get() { return sel; }, set(v) { sel = [...v]; }, configurable: true, enumerable: false });
  return p;
}
let __currentPage = null;
Object.defineProperty(figma, "currentPage", { get() { return __currentPage; }, set(p) { __currentPage = p; }, configurable: true });
figma.setCurrentPageAsync = (p) => { __currentPage = p; return Promise.resolve(); };
figma.loadAllPagesAsync = () => Promise.resolve();
figma.createPage = () => { const p = __page("Page " + (figma.root.children.length + 1), "page:" + __newid(), true); figma.root.children.push(p); p.parent = figma.root; return p; };

function __load(snap) {
  __nodes.clear();
  const link = (raw, parent) => {
    const n = __link(raw);
    n.parent = parent;
    if (raw.children) n.children = raw.children.map(c => link(c, n));
    return n;
  };
  const root = Object.setPrototypeOf({ type: "DOCUMENT", id: "0:0", name: "Document", children: [] }, __NodeProto);
  root.children = snap.pages.map((pg, i) => {
    const p = __page(pg.name, "page:" + i, pg.loaded);
    p.parent = root;
    p.children = pg.children.map(c => link(c, p));
    return p;
  });
  figma.root = root;
  __currentPage = root.children[snap.current];
  __currentPage.selection = snap.selection.map(id => __nodes.get(id)).filter(Boolean);
  __styles = snap.styles.map(s => Object.setPrototypeOf(s, __StyleProto));
  __collections = snap.collections.map(c => Object.setPrototypeOf(c, __CollectionProto));
  __variables = snap.variables.map(v => Object.setPrototypeOf(v, __VariableProto));
  __fonts = snap.fonts || __fonts;
}

function __collect() {
  const out = [];
  const walk = (n, parentId, page) => {
    for (const c of n.children || []) {
      const { parent, children, ...rest } = c;
      out.push(Object.assign(rest, { parentId, page, childIds: children ? children.map(k => k.id) : null }));
      walk(c, c.id, page);
    }
  };
  figma.root.children.forEach((p, i) => walk(p, null, i));
  return JSON.stringify({
    nodes: out, removed: __removed, notes: __notes,
    pages: figma.root.children.map(p => ({ id: p.id, name: p.name, loaded: p.__loaded !== false, top: p.children.map(c => c.id) })),
    current: Math.max(0, figma.root.children.indexOf(__currentPage)),
    selection: __currentPage.selection.map(n => n.id), closed: __closed, errors: __errors, outbox: __outbox, ui: __ui,
    storage: JSON.stringify(__storage), result: __result === undefined ? null : JSON.stringify(__plain(__result)),
    styles: __styles, collections: __collections.map(c => ({ id: c.id, name: c.name, modes: c.modes })), variables: __variables,
    removedStyles: __removedStyles, removedVariables: __removedVariables, removedCollections: __removedCollections,
  });
}
