//! Libraries: Figma's team libraries, for local files. Any .katachi file can be a library; a
//! file adds libraries, then uses their components, styles and variables. Those are copied in
//! as hidden, read-only "remote" assets keyed by (library id, id in the library), so the file
//! renders even when a library goes missing. Updates copy them again in place, keeping this
//! file's ids, so instances re-sync and keep their overrides.

use crate::doc::{Action, Document, Kind, LibraryRef, Node, NodeId, PropValue, Remote, Style, StyleId, StyleValue, VarId, VarValue, Variable};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// FNV-1a: content hashes are saved in files, so they mustn't change between Rust releases.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x0100_0000_01b3))
}

fn hash_of<T: serde::Serialize>(v: &T) -> u64 {
    fnv(&serde_json::to_vec(v).unwrap_or_default())
}

/// A fresh random file id.
pub fn new_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
    format!("{:016x}", h.finish())
}

/// Names starting with "." or "_" aren't published, as in Figma.
pub fn hidden(name: &str) -> bool {
    name.starts_with('.') || name.starts_with('_')
}

/// A library component's content hash: its whole subtree (a component set with its variants).
pub fn component_hash(lib: &Document, root: NodeId) -> u64 {
    let nodes: Vec<&Node> = std::iter::once(root).chain(lib.descendants(root)).map(|id| &*lib.nodes[&id]).collect();
    hash_of(&nodes)
}

fn style_hash(s: &Style) -> u64 {
    hash_of(&(&s.name, &s.value))
}

fn var_hash(v: &Variable) -> u64 {
    hash_of(&(&v.name, &v.values, &v.scopes, &v.code_syntax))
}

/// A library's published components: (page, component to place, name). A component set shows
/// once, placing its first variant, as in the Assets tab.
pub fn components(lib: &Document) -> Vec<(usize, NodeId, String)> {
    let mut out: Vec<(usize, NodeId, String)> = lib
        .nodes
        .values()
        .filter(|n| matches!(n.kind, Kind::Component | Kind::ComponentSet) && n.remote.is_none() && !hidden(&n.name))
        .filter(|n| !n.parent.is_some_and(|p| lib.node(p).kind == Kind::ComponentSet))
        .filter_map(|n| match n.kind {
            Kind::ComponentSet => n.children.first().map(|&v| (lib.page_of(n.id), v, n.name.clone())),
            _ => Some((lib.page_of(n.id), n.id, n.name.clone())),
        })
        .collect();
    out.sort_by(|a, b| (a.0, &a.2).cmp(&(b.0, &b.2)));
    out
}

/// What kind of asset an update is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asset {
    Component,
    Style,
    Variable,
}

/// A library asset that changed since this file copied it.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub lib: String,
    pub asset: Asset,
    pub key: u64,
    pub name: String,
}

/// Assets this file copied from library `lib_id` that the library has changed since. A library
/// that has been published only offers what a newer publish changed.
pub fn changes(doc: &Document, lib_id: &str, lib: &Document) -> Vec<Change> {
    let mine = |r: &Option<Remote>| r.clone().filter(|r| r.lib == lib_id && (lib.published.0 == 0 || lib.published.0 > r.version));
    let change = |asset, key, name: &str| Change { lib: lib_id.into(), asset, key, name: name.into() };
    let mut out = vec![];
    for n in doc.nodes.values().filter(|n| n.parent.is_none()) {
        if let Some(r) = mine(&n.remote)
            && let Some(c) = lib.nodes.get(&r.key)
            && component_hash(lib, r.key) != r.hash
        {
            out.push(change(Asset::Component, r.key, &c.name));
        }
    }
    for s in &doc.styles {
        if let Some(r) = mine(&s.remote)
            && let Some(l) = lib.style(r.key).filter(|l| style_hash(l) != r.hash)
        {
            out.push(change(Asset::Style, r.key, &l.name));
        }
    }
    for v in &doc.variables {
        if let Some(r) = mine(&v.remote)
            && let Some(l) = lib.variable(r.key).filter(|l| var_hash(l) != r.hash)
        {
            out.push(change(Asset::Variable, r.key, &l.name));
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Copy changed assets again; instances re-sync on the reflow and keep their overrides.
pub fn update(doc: &mut Document, lib_id: &str, lib: &Document, changes: &[Change]) {
    let mine: Vec<&Change> = changes.iter().filter(|c| c.lib == lib_id).collect();
    let mut imp = Import::new(doc, lib_id, lib);
    imp.refresh = mine.iter().map(|c| c.key).collect();
    for c in mine {
        match c.asset {
            Asset::Component => imp.component(doc, c.key),
            Asset::Style => imp.style(doc, c.key),
            Asset::Variable => imp.variable(doc, c.key),
        };
    }
    doc.reflow();
}

/// Copies library assets into a document, reusing copies it already has unless refreshing them.
pub struct Import<'a> {
    lib: &'a Document,
    id: String,
    /// Library ids of assets to copy again even if present (an update).
    pub refresh: HashSet<u64>,
    /// Library id -> this document's id, for everything copied or found so far.
    nodes: HashMap<NodeId, NodeId>,
    styles: HashMap<StyleId, StyleId>,
    vars: HashMap<VarId, VarId>,
    colls: HashMap<u64, u64>,
    /// This document's copies of the library's layers, by library id.
    have: HashMap<NodeId, NodeId>,
}

impl<'a> Import<'a> {
    pub fn new(doc: &Document, lib_id: &str, lib: &'a Document) -> Self {
        let have = doc.nodes.values().filter_map(|n| n.remote.as_ref().filter(|r| r.lib == lib_id).map(|r| (r.key, n.id))).collect();
        Import { lib, id: lib_id.into(), refresh: HashSet::new(), nodes: HashMap::new(), styles: HashMap::new(), vars: HashMap::new(), colls: HashMap::new(), have }
    }

    fn remote(&self, key: u64, hash: u64) -> Option<Remote> {
        Some(Remote { lib: self.id.clone(), key, hash, version: if hash == 0 { 0 } else { self.lib.published.0 } })
    }

    /// This document's existing copy of library layer `i`.
    fn has(&self, doc: &Document, i: NodeId) -> Option<NodeId> {
        self.have.get(&i).copied().filter(|l| doc.nodes.contains_key(l))
    }

    fn fresh_id(doc: &mut Document) -> u64 {
        doc.next_id += 1;
        doc.next_id - 1
    }

    /// Copy a library component (its whole set, for a variant) and everything it uses: nested
    /// components, styles, variables and images. Returns this document's id for it.
    pub fn component(&mut self, doc: &mut Document, key: NodeId) -> Option<NodeId> {
        if let Some(&l) = self.nodes.get(&key) {
            return Some(l);
        }
        let lib = self.lib;
        let n = lib.nodes.get(&key)?;
        let root = n.parent.filter(|p| lib.node(*p).kind == Kind::ComponentSet).unwrap_or(key);
        let ids: Vec<NodeId> = std::iter::once(root).chain(lib.descendants(root)).collect();
        if !self.refresh.contains(&root) && self.has(doc, root).is_some() && self.has(doc, key).is_some() {
            for &i in &ids {
                if let Some(l) = self.has(doc, i) {
                    self.nodes.insert(i, l);
                }
            }
            return self.nodes.get(&key).copied();
        }
        // Ids first: layers copied before keep theirs (so instances keep their overrides), and
        // components that use each other don't loop.
        let old: Vec<NodeId> = self.has(doc, root).map(|r| std::iter::once(r).chain(doc.descendants(r)).collect()).unwrap_or_default();
        for &i in &ids {
            let l = self.has(doc, i).unwrap_or_else(|| Self::fresh_id(doc));
            self.nodes.insert(i, l);
        }
        for &i in &ids {
            let n = lib.node(i);
            let swaps = n.prop_values.iter().map(|(_, v)| v).chain(n.comp_props.iter().map(|p| &p.default)).filter_map(|v| if let PropValue::Swap(c) = v { Some(*c) } else { None });
            for c in n.component.into_iter().chain(swaps).collect::<Vec<_>>() {
                self.component(doc, c);
            }
            for s in [n.fill_style, n.stroke_style, n.text_style, n.effect_style, n.grid_style].into_iter().flatten() {
                self.style(doc, s);
            }
            let vars = n.bindings.iter().map(|b| b.1).chain(n.reactions.iter().filter_map(|r| if let Action::SetVariable(v, _) = r.action { Some(v) } else { None }));
            for v in vars.collect::<Vec<_>>() {
                self.variable(doc, v);
            }
            for &(c, _) in &n.modes {
                self.collection(doc, c);
            }
            for p in n.fills.iter().chain(&n.strokes).chain(n.synced.iter().flat_map(|s| s.fills.iter().chain(&s.strokes))) {
                self.image(doc, &p.image);
            }
        }
        let keep: HashSet<NodeId> = ids.iter().map(|i| self.nodes[i]).collect();
        for o in old.into_iter().filter(|o| !keep.contains(o)) {
            doc.nodes.remove(&o);
        }
        let hash = component_hash(lib, root);
        for &i in &ids {
            let mut n = (*lib.nodes[&i]).clone();
            self.remap(&mut n);
            n.id = self.nodes[&i];
            n.parent = if i == root { None } else { n.parent.and_then(|p| self.nodes.get(&p).copied()) };
            n.children = n.children.iter().filter_map(|c| self.nodes.get(c).copied()).collect();
            n.ready = false;
            n.remote = self.remote(i, if i == root { hash } else { 0 });
            self.have.insert(i, n.id);
            doc.nodes.insert(n.id, Arc::new(n));
        }
        self.nodes.get(&key).copied()
    }

    /// Point a copied layer's references at this document's copies; ones that weren't copied go.
    fn remap(&self, n: &mut Node) {
        let node = |id: NodeId| self.nodes.get(&id).copied();
        n.component = n.component.and_then(node);
        n.source = n.source.and_then(node);
        for s in [&mut n.fill_style, &mut n.stroke_style, &mut n.text_style, &mut n.effect_style, &mut n.grid_style] {
            *s = s.and_then(|x| self.styles.get(&x).copied());
        }
        n.bindings = n.bindings.iter().filter_map(|&(b, v)| Some((b, *self.vars.get(&v)?))).collect();
        n.modes = n.modes.iter().filter_map(|&(c, m)| Some((*self.colls.get(&c)?, m))).collect();
        let swap = |v: &mut PropValue| match v {
            PropValue::Swap(c) => node(*c).map(|l| *c = l).is_some(),
            _ => true,
        };
        n.prop_values.retain_mut(|(_, v)| swap(v));
        n.comp_props.retain_mut(|p| swap(&mut p.default));
        for p in &mut n.comp_props {
            p.preferred = p.preferred.iter().filter_map(|&c| node(c)).collect();
        }
        n.reactions.retain_mut(|r| match &mut r.action {
            Action::Navigate(id) | Action::OpenOverlay(id) | Action::ChangeTo(id) | Action::ScrollTo(id) | Action::SwapOverlay(id) => node(*id).map(|l| *id = l).is_some(),
            Action::SetVariable(v, _) => self.vars.get(v).map(|&l| *v = l).is_some(),
            _ => true,
        });
        if let Some(s) = &mut n.synced {
            let s: &mut Node = s;
            s.parent = None;
            self.remap(s);
        }
    }

    fn image(&self, doc: &mut Document, key: &str) {
        if let Some(b) = self.lib.images.get(key) {
            doc.images.entry(key.to_string()).or_insert_with(|| b.clone());
        }
    }

    /// Copy a library style; returns this document's id for it.
    pub fn style(&mut self, doc: &mut Document, key: StyleId) -> Option<StyleId> {
        if let Some(&l) = self.styles.get(&key) {
            return Some(l);
        }
        let s = self.lib.style(key)?;
        let at = doc.styles.iter().position(|x| x.remote.as_ref().is_some_and(|r| r.lib == self.id && r.key == key));
        let fresh = Style { id: 0, name: s.name.clone(), value: s.value.clone(), remote: self.remote(key, style_hash(s)) };
        let id = match at {
            Some(i) if !self.refresh.contains(&key) => doc.styles[i].id,
            Some(i) => {
                doc.styles[i] = Style { id: doc.styles[i].id, ..fresh };
                doc.styles[i].id
            }
            None => {
                let id = Self::fresh_id(doc);
                doc.styles.push(Style { id, ..fresh });
                id
            }
        };
        if let StyleValue::Paint(ps) = &s.value {
            for p in ps {
                self.image(doc, &p.image);
            }
        }
        self.styles.insert(key, id);
        Some(id)
    }

    /// Copy a library variable, with its collection and the variables it aliases.
    pub fn variable(&mut self, doc: &mut Document, key: VarId) -> Option<VarId> {
        if let Some(&l) = self.vars.get(&key) {
            return Some(l);
        }
        let v = self.lib.variable(key)?;
        let at = doc.variables.iter().find(|x| x.remote.as_ref().is_some_and(|r| r.lib == self.id && r.key == key)).map(|x| x.id);
        let id = at.unwrap_or_else(|| Self::fresh_id(doc));
        self.vars.insert(key, id);
        if at.is_some() && !self.refresh.contains(&key) {
            return Some(id);
        }
        let collection = self.collection(doc, v.collection)?;
        let values = v
            .values
            .iter()
            .map(|x| match x {
                VarValue::Alias(a) => self.variable(doc, *a).map(VarValue::Alias).or_else(|| self.lib.resolve_first(*a)).unwrap_or(VarValue::Number(0.0)),
                other => other.clone(),
            })
            .collect();
        let new = Variable { id, name: v.name.clone(), collection, values, scopes: v.scopes.clone(), code_syntax: v.code_syntax.clone(), remote: self.remote(key, var_hash(v)) };
        match doc.variables.iter().position(|x| x.id == id) {
            Some(i) => doc.variables[i] = new,
            None => doc.variables.push(new),
        }
        Some(id)
    }

    /// Copy (or refresh) a library variable collection's name and modes.
    fn collection(&mut self, doc: &mut Document, key: u64) -> Option<u64> {
        if let Some(&l) = self.colls.get(&key) {
            return Some(l);
        }
        let c = self.lib.collections.iter().find(|c| c.id == key)?;
        let id = match doc.collections.iter_mut().find(|x| x.remote.as_ref().is_some_and(|r| r.lib == self.id && r.key == key)) {
            Some(x) => {
                (x.name, x.modes) = (c.name.clone(), c.modes.clone());
                x.id
            }
            None => {
                let id = Self::fresh_id(doc);
                doc.collections.push(crate::doc::Collection { id, name: c.name.clone(), modes: c.modes.clone(), remote: self.remote(key, 0) });
                id
            }
        };
        self.colls.insert(key, id);
        Some(id)
    }
}

/// figma.importComponentByKeyAsync: copy library component "libraryId:nodeId" in. Returns the
/// copy's root layer (the component, or its set).
pub fn import_key(doc: &mut Document, libs: &[(String, Arc<Document>)], key: &str) -> Option<NodeId> {
    let (lib, id) = key.rsplit_once(':')?;
    let d = &libs.iter().find(|(l, _)| l == lib)?.1;
    let local = Import::new(doc, lib, d).component(doc, id.parse().ok()?)?;
    remote_root(doc, local).map(|r| r.1)
}

/// "Move to this file": a library component (with its set, for a variant) becomes a local one
/// on `page` with its top-left at `at`; instances keep using it. Returns the moved layer.
pub fn localize_component(doc: &mut Document, id: NodeId, page: usize, at: egui::Pos2) -> Option<NodeId> {
    let root = doc.ancestors(id).last().copied().unwrap_or(id);
    doc.node(root).remote.as_ref()?;
    for d in std::iter::once(root).chain(doc.descendants(root)) {
        doc.node_mut(d).remote = None;
    }
    doc.pages[page].children.push(root);
    let min = doc.node(root).rect.min;
    doc.translate(root, at - min);
    Some(root)
}

/// The library component a layer belongs to, if it's a remote copy: (library id, root layer).
pub fn remote_root(doc: &Document, id: NodeId) -> Option<(String, NodeId)> {
    let root = doc.ancestors(id).last().copied().unwrap_or(id);
    Some((doc.nodes.get(&root)?.remote.as_ref()?.lib.clone(), root))
}

// ---------- files ----------

/// `to` relative to the folder `dir` ("../ui/kit.katachi"), or absolute when they share nothing
/// but the filesystem root.
pub fn relative(dir: &Path, to: &Path) -> String {
    let (a, b): (Vec<_>, Vec<_>) = (dir.components().collect(), to.components().collect());
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    if common <= 1 {
        return to.to_string_lossy().into_owned();
    }
    let rel: PathBuf = a[common..].iter().map(|_| std::path::Component::ParentDir).chain(b[common..].iter().copied()).collect();
    rel.to_string_lossy().into_owned()
}

/// A stored library path, resolved against this file's folder.
pub fn resolve(dir: Option<&Path>, path: &str) -> PathBuf {
    match dir {
        Some(d) if Path::new(path).is_relative() => d.join(path),
        _ => PathBuf::from(path),
    }
}

/// Read a .katachi file.
pub fn read(p: &Path) -> Option<Document> {
    crate::doc::parse_file(&std::fs::read_to_string(p).ok()?).ok()
}

/// A library's id: its file id, or (for files saved before ids) one made from its path.
pub fn id_of(lib: &Document, path: &Path) -> String {
    if lib.id.is_empty() { format!("path-{:016x}", fnv(path.as_os_str().as_encoded_bytes())) } else { lib.id.clone() }
}

/// A library file as last read from disk.
#[derive(Clone)]
pub struct Loaded {
    pub path: PathBuf,
    /// None while the file is missing or unreadable.
    pub doc: Option<Arc<Document>>,
    modified: Option<std::time::SystemTime>,
}

/// Read library `r` (`dir` is this file's folder), reusing `cached` while the file's
/// modification time is unchanged. A library that moved or was renamed is found again by its
/// id, among recent files and this file's folder.
pub fn load(dir: Option<&Path>, r: &LibraryRef, cached: Option<&Loaded>) -> Loaded {
    let path = resolve(dir, &r.path);
    let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let now = modified(&path);
    if let Some(c) = cached.filter(|c| c.path == path && c.modified == now) {
        return c.clone();
    }
    if let Some(doc) = read(&path) {
        return Loaded { path, doc: Some(Arc::new(doc)), modified: now };
    }
    // ponytail: parses every candidate; fine for a missing library, an index if it gets slow.
    let mut candidates = crate::files::recent();
    if let Some(rd) = dir.and_then(|d| std::fs::read_dir(d).ok()) {
        candidates.extend(rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "katachi")));
    }
    for p in candidates.into_iter().filter(|p| *p != path) {
        if let Some(doc) = read(&p).filter(|d| d.id == r.id) {
            return Loaded { modified: modified(&p), path: p, doc: Some(Arc::new(doc)) };
        }
    }
    Loaded { path, doc: None, modified: now }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::{Paint, Rgba, VarValue};
    use egui::{Rect, pos2, vec2};

    /// A library with a button component using a color style and a variable, plus a hidden one.
    fn library() -> (Document, NodeId, StyleId, VarId) {
        let mut lib = Document { id: "lib1".into(), ..Default::default() };
        let blue = lib.add_style("Brand/Blue".into(), StyleValue::Paint(vec![Paint::solid(Rgba::rgb(0, 0, 255))]));
        let coll = lib.add_collection("Tokens".into());
        let gap = lib.add_variable(coll, "space/m".into(), VarValue::Number(8.0));
        let mut b = Node::new(Kind::Component, Rect::from_min_size(pos2(0., 0.), vec2(80., 30.)));
        b.name = "Button".into();
        b.fill_style = Some(blue);
        let b = lib.insert(0, None, None, b);
        let mut label = Node::new(Kind::Text, Rect::from_min_size(pos2(10., 5.), vec2(40., 20.)));
        label.text = "OK".into();
        label.bindings = vec![(crate::doc::Bindable::FontSize, gap)];
        lib.insert(0, Some(b), None, label);
        let mut h = Node::new(Kind::Component, Rect::from_min_size(pos2(200., 0.), vec2(10., 10.)));
        h.name = "_Private".into();
        lib.insert(0, None, None, h);
        lib.reflow();
        (lib, b, blue, gap)
    }

    #[test]
    fn import_update_and_localize() {
        let (mut lib, button, blue, _) = library();
        assert_eq!(components(&lib).iter().map(|c| c.2.as_str()).collect::<Vec<_>>(), vec!["Button"], "hidden components aren't offered");
        let mut doc = Document::default();
        let comp = Import::new(&doc, "lib1", &lib).component(&mut doc, button).unwrap();
        assert!(!doc.pages[0].children.contains(&comp), "the copy stays off the pages");
        assert_eq!(doc.node(comp).remote.as_ref().unwrap().key, button);
        let style = doc.node(comp).fill_style.unwrap();
        assert_eq!(doc.style(style).unwrap().remote.as_ref().unwrap().key, blue, "styles come along");
        let label = doc.node(comp).children[0];
        let var = doc.node(label).bindings[0].1;
        assert!(doc.variable(var).unwrap().remote.is_some() && doc.collections.len() == 1, "and variables with their collection");
        // Importing again reuses the copy.
        assert_eq!(Import::new(&doc, "lib1", &lib).component(&mut doc, button), Some(comp));
        let inst = doc.instantiate(comp, 0, None, None, pos2(0., 0.));
        let text = doc.node(inst).children[0];
        doc.node_mut(text).text = "Go".into();
        doc.reflow();
        // Survives a save, and stays off the pages.
        let back = crate::doc::parse_file(&crate::doc::file_json(&doc)).unwrap();
        assert_eq!(back.pages[0].children, vec![inst]);
        assert!(back.node(comp).remote.is_some());
        // The library changes: the button gets wider and its label red.
        assert!(changes(&doc, "lib1", &lib).is_empty());
        lib.node_mut(button).rect.max.x = 120.0;
        let lib_label = lib.node(button).children[0];
        lib.node_mut(lib_label).fills = vec![Paint::solid(Rgba::rgb(255, 0, 0))];
        let ch = changes(&doc, "lib1", &lib);
        assert_eq!(ch.iter().map(|c| (c.asset, c.key)).collect::<Vec<_>>(), vec![(Asset::Component, button)]);
        // Once the library publishes, only a newer publish shows changes.
        lib.published = (1, 0);
        update(&mut doc, "lib1", &lib, &ch);
        assert!(changes(&doc, "lib1", &lib).is_empty());
        assert_eq!(doc.node(comp).rect.width(), 120.0, "updated in place");
        assert_eq!(doc.node(inst).rect.width(), 120.0, "the instance follows");
        assert_eq!(doc.node(text).text, "Go", "and keeps its override");
        assert_eq!(doc.node(text).fills[0].color, Rgba::rgb(255, 0, 0));
        lib.node_mut(button).name = "Primary".into();
        assert!(changes(&doc, "lib1", &lib).is_empty(), "unpublished edits wait");
        lib.published = (2, 0);
        assert_eq!(changes(&doc, "lib1", &lib).len(), 1);
        // Move to this file: a normal local component.
        localize_component(&mut doc, comp, 0, pos2(300., 0.));
        assert!(doc.node(comp).remote.is_none() && doc.pages[0].children.contains(&comp));
        assert_eq!(doc.node(comp).rect.min, pos2(300., 0.));
        assert!(changes(&doc, "lib1", &lib).is_empty());
    }

    #[test]
    fn paths_are_relative_to_the_file() {
        let p = |s: &str| PathBuf::from(s);
        assert_eq!(relative(&p("/work/app"), &p("/work/app/ui.katachi")), "ui.katachi");
        assert_eq!(relative(&p("/work/app"), &p("/work/kit/ui.katachi")), p("../kit/ui.katachi").to_string_lossy());
        assert_eq!(resolve(Some(&p("/work/app")), "../kit/ui.katachi"), p("/work/app/../kit/ui.katachi"));
        assert_eq!(resolve(Some(&p("/work/app")), "/abs/ui.katachi"), p("/abs/ui.katachi"));
    }
}
