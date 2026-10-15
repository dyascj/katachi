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

// ---------- the app ----------

use crate::app::{App, Want};
use crate::paint_ui::StylePick;
use egui::RichText;

impl App {
    fn doc_dir(&self) -> Option<PathBuf> {
        self.path.as_ref().and_then(|p| p.parent()).map(Path::to_path_buf)
    }

    /// How a library path is stored: relative to this file once it has a folder.
    fn stored_path(&self, p: &Path) -> String {
        match self.doc_dir() {
            Some(d) => relative(&d, p),
            None => p.to_string_lossy().into_owned(),
        }
    }

    /// Store library paths relative to `dir`, where the file is being saved.
    pub fn rebase_libraries(&mut self, dir: Option<&Path>) {
        let (old, Some(dir)) = (self.doc_dir(), dir) else { return };
        for r in &mut self.doc.libraries {
            let abs = self.libs.get(&r.id).map_or_else(|| resolve(old.as_deref(), &r.path), |l| l.path.clone());
            r.path = relative(dir, &abs);
        }
    }

    /// Re-read libraries that changed on disk and list what they changed. Runs when the file
    /// opens and when the window regains focus.
    pub fn refresh_libraries(&mut self) {
        let dir = self.doc_dir();
        let mut updates = vec![];
        for i in 0..self.doc.libraries.len() {
            let r = self.doc.libraries[i].clone();
            let l = load(dir.as_deref(), &r, self.libs.get(&r.id));
            if l.doc.is_some() && l.path != resolve(dir.as_deref(), &r.path) {
                // Found again after a move or rename: remember where.
                self.doc.libraries[i].path = self.stored_path(&l.path);
                self.dirty = true;
            }
            if let Some(d) = &l.doc {
                updates.extend(changes(&self.doc, &r.id, d));
            }
            self.libs.insert(r.id, l);
        }
        self.lib_updates = updates;
    }

    fn lib_doc(&self, id: &str) -> Option<Arc<Document>> {
        self.libs.get(id)?.doc.clone()
    }

    /// Enabled libraries that could be read.
    pub fn enabled_libs(&self) -> Vec<(LibraryRef, Arc<Document>)> {
        self.doc.libraries.iter().filter(|r| r.enabled).filter_map(|r| Some((r.clone(), self.lib_doc(&r.id)?))).collect()
    }

    /// Libraries for plugins: (id, document).
    pub fn plugin_libs(&self) -> Vec<(String, Arc<Document>)> {
        self.enabled_libs().into_iter().map(|(r, d)| (r.id, d)).collect()
    }

    /// "Add library file": use another .katachi file's assets here.
    pub fn add_library(&mut self, path: PathBuf) {
        let Some(lib) = read(&path) else { return self.toast("Couldn't open that file as a library".into()) };
        let id = id_of(&lib, &path);
        if self.path.as_ref() == Some(&path) || id == self.doc.id {
            return self.toast("A file can't be its own library".into());
        }
        self.checkpoint();
        match self.doc.libraries.iter_mut().find(|l| l.id == id) {
            Some(l) => l.enabled = true,
            None => {
                let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                let stored = self.stored_path(&path);
                self.doc.libraries.push(LibraryRef { id, name, path: stored, enabled: true });
            }
        }
        self.refresh_libraries();
    }

    /// Point library `i` at another file. If that's a different library, the assets this file
    /// copied move over to it, matched by their ids there (a copy of the original, say).
    pub fn relink_library(&mut self, i: usize, path: PathBuf) {
        let Some(lib) = read(&path) else { return self.toast("Couldn't open that file as a library".into()) };
        let (old, new) = (self.doc.libraries[i].id.clone(), id_of(&lib, &path));
        self.checkpoint();
        if new != old {
            let retarget = |r: &mut Option<Remote>| {
                if let Some(r) = r.as_mut().filter(|r| r.lib == old) {
                    r.lib = new.clone();
                }
            };
            for n in self.doc.nodes.values_mut() {
                if n.remote.as_ref().is_some_and(|r| r.lib == old) {
                    retarget(&mut Arc::make_mut(n).remote);
                }
            }
            self.doc.styles.iter_mut().for_each(|s| retarget(&mut s.remote));
            self.doc.variables.iter_mut().for_each(|v| retarget(&mut v.remote));
            self.doc.collections.iter_mut().for_each(|c| retarget(&mut c.remote));
        }
        let stored = self.stored_path(&path);
        let r = &mut self.doc.libraries[i];
        (r.id, r.path) = (new, stored);
        r.name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        self.libs.remove(&old);
        self.refresh_libraries();
    }

    /// Copy a library component in (callers make the undo step first).
    pub fn import_component(&mut self, lib: &str, key: NodeId) -> Option<NodeId> {
        let d = self.lib_doc(lib)?;
        Import::new(&self.doc, lib, &d).component(&mut self.doc, key)
    }

    /// A style picked from a menu, with a library one copied in: Apply or Create as before.
    pub fn local_style_pick(&mut self, pick: Option<StylePick>) -> Option<StylePick> {
        match pick {
            Some(StylePick::Library(lib, key)) => {
                let d = self.lib_doc(&lib)?;
                Import::new(&self.doc, &lib, &d).style(&mut self.doc, key).map(StylePick::Apply)
            }
            p => p,
        }
    }

    /// A variable picked from a menu (library, id there), copied in if it's a library's.
    pub fn local_var(&mut self, pick: Option<(Option<String>, VarId)>) -> Option<VarId> {
        match pick? {
            (None, v) => Some(v),
            (Some(lib), v) => {
                let d = self.lib_doc(&lib)?;
                Import::new(&self.doc, &lib, &d).variable(&mut self.doc, v)
            }
        }
    }

    /// The Styles menu: this file's styles, then each enabled library's.
    pub fn style_picker(&self, ui: &mut egui::Ui, id: egui::Id, kind: impl Fn(&StyleValue) -> bool) -> Option<StylePick> {
        let local: Vec<Style> = self.doc.styles.iter().filter(|s| s.remote.is_none()).cloned().collect();
        let libs: Vec<(String, String, Vec<Style>)> =
            self.enabled_libs().into_iter().map(|(r, d)| (r.name, r.id, d.styles.iter().filter(|s| s.remote.is_none() && !hidden(&s.name)).cloned().collect())).collect();
        crate::paint_ui::style_menu(ui, id, &local, &libs, kind)
    }

    /// Figma's "Publish library": save with a new publish version, which files using this one
    /// compare against.
    pub fn publish_library(&mut self) {
        let before = self.doc.published;
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        self.doc.published = (before.0 + 1, now);
        if self.save(false) {
            self.toast(format!("Published version {}", self.doc.published.0));
        } else {
            self.doc.published = before;
        }
    }

    /// Take library updates (all, or the `only`th), as one undo step.
    pub fn apply_library_updates(&mut self, only: Option<usize>) {
        let picked: Vec<Change> = match only {
            Some(i) => self.lib_updates.get(i).cloned().into_iter().collect(),
            None => self.lib_updates.clone(),
        };
        if picked.is_empty() {
            return;
        }
        self.checkpoint();
        for r in self.doc.libraries.clone() {
            if let Some(d) = self.lib_doc(&r.id) {
                update(&mut self.doc, &r.id, &d, &picked);
            }
        }
        self.refresh_libraries();
    }

    /// Open a library's file in a new tab ("Go to main component" on a library instance).
    pub fn open_library(&mut self, lib: &str) {
        match self.libs.get(lib).filter(|l| l.doc.is_some()) {
            Some(l) => self.want = Some(Want::Open(l.path.clone())),
            None => self.toast("The library file is missing".into()),
        }
    }

    /// Whether an instance's main component comes from a library.
    pub fn main_is_remote(&self, inst: NodeId) -> bool {
        self.doc.node(inst).component.is_some_and(|c| self.doc.nodes.contains_key(&c) && remote_root(&self.doc, c).is_some())
    }

    /// "Move to this file": the instance's library component becomes a local one, mid-screen.
    pub fn localize_main(&mut self, inst: NodeId) {
        let Some(c) = self.doc.node(inst).component else { return };
        self.checkpoint();
        let at = self.view_center_world();
        if let Some(root) = localize_component(&mut self.doc, c, self.page, at) {
            self.sel = vec![root];
        }
    }

    /// The Libraries dialog: libraries added to this file, on or off, with missing ones to
    /// relink; add one from disk or from recent files.
    pub fn libraries_window(&mut self, ctx: &egui::Context) {
        if !self.show_libraries {
            return;
        }
        let mut open = true;
        let (mut toggle, mut relink, mut add) = (None, None, None);
        crate::chrome::window(ctx, "Libraries", &mut open, 420.0, |ui| {
            let muted = crate::appearance::muted(ui.ctx());
            if self.doc.libraries.is_empty() {
                ui.label(RichText::new("Add a .katachi file to use its components, styles and variables here.").color(muted));
            }
            for (i, r) in self.doc.libraries.iter().enumerate() {
                let lib = self.libs.get(&r.id).and_then(|l| l.doc.clone());
                ui.horizontal(|ui| {
                    let mut on = r.enabled;
                    if ui.checkbox(&mut on, RichText::new(&r.name).strong()).changed() {
                        toggle = Some(i);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| match &lib {
                        None => {
                            if ui.button("Relink…").clicked() {
                                relink = Some(i);
                            }
                            ui.label(RichText::new("Missing").color(egui::Color32::from_rgb(0xF2, 0x48, 0x22)));
                        }
                        Some(d) => {
                            let styles = d.styles.iter().filter(|s| s.remote.is_none() && !hidden(&s.name)).count();
                            ui.label(RichText::new(format!("{} components, {styles} styles", components(d).len())).small().color(muted));
                        }
                    });
                });
            }
            ui.separator();
            if ui.button("Add library file…").clicked()
                && let Some(p) = rfd::FileDialog::new().add_filter("Katachi", &["katachi"]).pick_file()
            {
                add = Some(p);
            }
            // Recent files are the likely libraries, as Figma lists a team's files.
            let dir = self.doc_dir();
            let used: Vec<PathBuf> = self.doc.libraries.iter().map(|r| self.libs.get(&r.id).map_or_else(|| resolve(dir.as_deref(), &r.path), |l| l.path.clone())).collect();
            let recent: Vec<PathBuf> = crate::files::recent().into_iter().filter(|p| Some(p) != self.path.as_ref() && !used.contains(p)).take(8).collect();
            if !recent.is_empty() {
                ui.label(RichText::new("Recent files").small().color(muted));
            }
            for p in recent {
                ui.horizontal(|ui| {
                    ui.label(p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()).on_hover_text(p.to_string_lossy());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Add").clicked() {
                            add = Some(p.clone());
                        }
                    });
                });
            }
        });
        self.show_libraries = open;
        if let Some(i) = toggle {
            self.checkpoint();
            self.doc.libraries[i].enabled = !self.doc.libraries[i].enabled;
        }
        if let Some(i) = relink
            && let Some(p) = rfd::FileDialog::new().add_filter("Katachi", &["katachi"]).pick_file()
        {
            self.relink_library(i, p);
        }
        if let Some(p) = add {
            self.add_library(p);
        }
    }

    /// Figma's "Library updates available" notice over the canvas, and its review dialog.
    pub fn library_updates(&mut self, ctx: &egui::Context) {
        if self.lib_updates.is_empty() {
            self.review_updates = false;
            return;
        }
        if !self.review_updates {
            let at = self.canvas_rect.unwrap_or(ctx.content_rect()).right_bottom() - egui::vec2(16.0, 16.0);
            egui::Area::new(egui::Id::new("lib-updates")).pivot(egui::Align2::RIGHT_BOTTOM).fixed_pos(at).order(egui::Order::Foreground).show(ctx, |ui| {
                crate::chrome::popover_frame(ui).inner_margin(10).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Library updates available").strong());
                        if ui.button("Review").clicked() {
                            self.review_updates = true;
                        }
                    });
                });
            });
            return;
        }
        let mut one = None;
        let names: HashMap<String, String> = self.doc.libraries.iter().map(|r| (r.id.clone(), r.name.clone())).collect();
        let body = |ui: &mut egui::Ui| {
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                for (i, c) in self.lib_updates.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let kind = match c.asset {
                            Asset::Component => "Component",
                            Asset::Style => "Style",
                            Asset::Variable => "Variable",
                        };
                        ui.label(&c.name);
                        ui.label(RichText::new(format!("{kind} · {}", names.get(&c.lib).map_or("", |n| n.as_str()))).small().color(crate::appearance::muted(ui.ctx())));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("Update").clicked() {
                                one = Some(i);
                            }
                        });
                    });
                }
            });
        };
        match crate::chrome::dialog(ctx, "lib-updates-review", "Library updates", 380.0, body, &[("Close", false), ("Update all", true)]) {
            Some(1) => self.apply_library_updates(None),
            Some(_) => self.review_updates = false,
            None => {}
        }
        if one.is_some() {
            self.apply_library_updates(one);
        }
    }
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
