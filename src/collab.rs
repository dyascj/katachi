//! Real-time multiplayer: a WebSocket relay holding the shared document, and clients that
//! send throttled diffs of their changes plus presence (cursors and selections).

use crate::doc::{Collection, Comment, Document, Node, NodeId, Page, Style, Variable};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use egui::Pos2;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tungstenite::{Message, WebSocket};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Op {
    Upsert(Box<Node>),
    Remove(NodeId),
    Pages(Vec<Page>),
    Meta { styles: Vec<Style>, collections: Vec<Collection>, variables: Vec<Variable>, comments: Vec<Comment> },
    /// A newly added image (base64 of the encoded bytes).
    Image(String, String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Presence {
    pub name: String,
    pub color: usize,
    pub page: usize,
    pub cursor: Option<Pos2>,
    pub selection: Vec<NodeId>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ToServer {
    Ops(Vec<Op>),
    Presence(Presence),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ToClient {
    Welcome { client: u32, doc: Box<Document> },
    Ops { from: u32, ops: Vec<Op> },
    Presence { from: u32, presence: Presence },
    Left { from: u32 },
}

pub const PEER_COLORS: [egui::Color32; 6] = [
    egui::Color32::from_rgb(0xF2, 0x48, 0x22),
    egui::Color32::from_rgb(0x14, 0xAE, 0x5C),
    egui::Color32::from_rgb(0x97, 0x47, 0xFF),
    egui::Color32::from_rgb(0xFF, 0xA6, 0x29),
    egui::Color32::from_rgb(0x0D, 0x99, 0xFF),
    egui::Color32::from_rgb(0xE8, 0x3E, 0x8C),
];

/// Everything that changed from `base` to `doc`, as ops.
pub fn diff(base: &Document, doc: &Document) -> Vec<Op> {
    let mut ops = vec![];
    for (id, n) in &doc.nodes {
        if base.nodes.get(id) != Some(n) {
            ops.push(Op::Upsert(Box::new(n.clone())));
        }
    }
    for id in base.nodes.keys() {
        if !doc.nodes.contains_key(id) {
            ops.push(Op::Remove(*id));
        }
    }
    if base.pages != doc.pages {
        ops.push(Op::Pages(doc.pages.clone()));
    }
    if base.styles != doc.styles || base.collections != doc.collections || base.variables != doc.variables || base.comments != doc.comments {
        ops.push(Op::Meta { styles: doc.styles.clone(), collections: doc.collections.clone(), variables: doc.variables.clone(), comments: doc.comments.clone() });
    }
    for (k, v) in &doc.images {
        if !base.images.contains_key(k) {
            ops.push(Op::Image(k.clone(), B64.encode(v.as_slice())));
        }
    }
    ops
}

/// Apply ops. Derived text layout (wrapped lines) stays local so peers don't echo it back.
pub fn apply(doc: &mut Document, ops: &[Op]) {
    for op in ops {
        match op {
            Op::Upsert(n) => {
                let mut n = (**n).clone();
                if let Some(local) = doc.nodes.get(&n.id) {
                    n.lines = local.lines.clone();
                }
                doc.nodes.insert(n.id, n);
            }
            Op::Remove(id) => {
                doc.nodes.remove(id);
            }
            Op::Pages(p) => doc.pages = p.clone(),
            Op::Meta { styles, collections, variables, comments } => {
                doc.styles = styles.clone();
                doc.collections = collections.clone();
                doc.variables = variables.clone();
                doc.comments = comments.clone();
            }
            Op::Image(k, b) => {
                if let Ok(bytes) = B64.decode(b) {
                    doc.images.insert(k.clone(), Arc::new(bytes));
                }
            }
        }
    }
    repair(doc);
}

/// Make parent links and child lists agree after concurrent edits (last writer wins per node).
pub fn repair(doc: &mut Document) {
    let ids: HashSet<NodeId> = doc.nodes.keys().copied().collect();
    for page in &mut doc.pages {
        page.children.retain(|c| ids.contains(c) && doc.nodes[c].parent.is_none());
        let mut seen = HashSet::new();
        page.children.retain(|c| seen.insert(*c));
    }
    let parents: HashMap<NodeId, Option<NodeId>> = doc.nodes.iter().map(|(k, n)| (*k, n.parent)).collect();
    for n in doc.nodes.values_mut() {
        let me = n.id;
        n.children.retain(|c| parents.get(c) == Some(&Some(me)));
        let mut seen = HashSet::new();
        n.children.retain(|c| seen.insert(*c));
    }
    let mut ordered: Vec<NodeId> = doc.nodes.keys().copied().collect();
    ordered.sort();
    for id in ordered {
        match doc.nodes[&id].parent {
            Some(p) if !ids.contains(&p) => {
                doc.node_mut(id).parent = None;
                if !doc.pages.iter().any(|pg| pg.children.contains(&id)) {
                    doc.pages[0].children.push(id);
                }
            }
            Some(p) => {
                if !doc.nodes[&p].children.contains(&id) {
                    doc.node_mut(p).children.push(id);
                }
            }
            None => {
                if !doc.pages.iter().any(|pg| pg.children.contains(&id)) {
                    doc.pages[0].children.push(id);
                }
            }
        }
    }
}

// ---------- transport ----------

fn pump(mut ws: WebSocket<TcpStream>, outgoing: Receiver<String>, mut incoming: impl FnMut(String) -> bool) {
    let _ = ws.get_mut().set_read_timeout(Some(Duration::from_millis(10)));
    loop {
        match ws.read() {
            Ok(Message::Text(t)) => {
                if !incoming(t.as_str().to_string()) {
                    break;
                }
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(_) => break,
        }
        loop {
            match outgoing.try_recv() {
                Ok(m) => {
                    if ws.send(Message::text(m)).is_err() {
                        return;
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    let _ = ws.close(None);
                    return;
                }
            }
        }
    }
}

struct ServerState {
    doc: Document,
    clients: HashMap<u32, Sender<String>>,
    next: u32,
}

/// Run the relay on `listener` in background threads. Returns the shared document handle.
pub fn serve(listener: TcpListener, doc: Document) -> Arc<Mutex<Document>> {
    let state = Arc::new(Mutex::new(ServerState { doc, clients: HashMap::new(), next: 1 }));
    let snapshot = Arc::new(Mutex::new(state.lock().unwrap().doc.clone()));
    let snap = snapshot.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let state = state.clone();
            let snap = snap.clone();
            std::thread::spawn(move || {
                let Ok(ws) = tungstenite::accept(stream) else { return };
                let (tx, rx) = channel::<String>();
                let me = {
                    let mut s = state.lock().unwrap();
                    let me = s.next;
                    s.next += 1;
                    s.clients.insert(me, tx.clone());
                    let welcome = ToClient::Welcome { client: me, doc: Box::new(s.doc.clone()) };
                    let _ = tx.send(serde_json::to_string(&welcome).unwrap_or_default());
                    me
                };
                pump(ws, rx, |text| {
                    let Ok(msg) = serde_json::from_str::<ToServer>(&text) else { return true };
                    let mut s = state.lock().unwrap();
                    let out = match msg {
                        ToServer::Ops(ops) => {
                            apply(&mut s.doc, &ops);
                            *snap.lock().unwrap() = s.doc.clone();
                            ToClient::Ops { from: me, ops }
                        }
                        ToServer::Presence(presence) => ToClient::Presence { from: me, presence },
                    };
                    let json = serde_json::to_string(&out).unwrap_or_default();
                    for (id, c) in &s.clients {
                        if *id != me {
                            let _ = c.send(json.clone());
                        }
                    }
                    true
                });
                let mut s = state.lock().unwrap();
                s.clients.remove(&me);
                let left = serde_json::to_string(&ToClient::Left { from: me }).unwrap_or_default();
                for c in s.clients.values() {
                    let _ = c.send(left.clone());
                }
            });
        }
    });
    snapshot
}

/// A connection to a relay: messages in, JSON out.
pub struct Link {
    pub tx: Sender<String>,
    pub rx: Receiver<ToClient>,
}

pub fn connect(addr: &str, ctx: Option<egui::Context>) -> Result<Link, String> {
    let url = if addr.starts_with("ws://") { addr.to_string() } else { format!("ws://{addr}") };
    let host = url.trim_start_matches("ws://").split('/').next().unwrap_or_default().to_string();
    let stream = TcpStream::connect(&host).map_err(|e| e.to_string())?;
    let (ws, _) = tungstenite::client(url.as_str(), stream).map_err(|e| e.to_string())?;
    let (tx_out, rx_out) = channel::<String>();
    let (tx_in, rx_in) = channel::<ToClient>();
    std::thread::spawn(move || {
        pump(ws, rx_out, |text| {
            if let Ok(msg) = serde_json::from_str::<ToClient>(&text) {
                if tx_in.send(msg).is_err() {
                    return false;
                }
                if let Some(ctx) = &ctx {
                    ctx.request_repaint();
                }
            }
            true
        });
    });
    Ok(Link { tx: tx_out, rx: rx_in })
}

/// The app's side of a session.
pub struct Collab {
    pub link: Link,
    /// Server-assigned id (also the high bits of this client's node ids).
    #[allow(dead_code)]
    pub client: u32,
    /// Document as last agreed with the server; local edits are diffs against it.
    pub base: Document,
    pub peers: HashMap<u32, Presence>,
    pub me: Presence,
    pub address: String,
    pub hosting: bool,
    last_send: f64,
    last_presence: Option<Presence>,
    pub connected: bool,
}

impl Collab {
    /// Wait for the welcome (up to a few seconds) and set up the session.
    pub fn join(link: Link, address: String, name: String, hosting: bool) -> Result<(Collab, Document), String> {
        let welcome = link.rx.recv_timeout(Duration::from_secs(5)).map_err(|_| "No response from the session".to_string())?;
        let ToClient::Welcome { client, doc } = welcome else { return Err("Unexpected reply".into()) };
        let mut doc = *doc;
        // Each client allocates ids from its own range so concurrent inserts never collide.
        doc.next_id = doc.next_id.max((client as u64) << 32);
        let me = Presence { name, color: client as usize % PEER_COLORS.len(), page: 0, cursor: None, selection: vec![] };
        let c = Collab { link, client, base: doc.clone(), peers: HashMap::new(), me, address, hosting, last_send: 0.0, last_presence: None, connected: true };
        Ok((c, doc))
    }

    /// Exchange changes: apply what arrived, send what changed locally (throttled).
    /// Returns ids of nodes this client changed in what it sent (for per-user undo).
    pub fn tick(&mut self, doc: &mut Document, now: f64) -> Vec<NodeId> {
        let mut remote = false;
        loop {
            match self.link.rx.try_recv() {
                Ok(ToClient::Ops { ops, .. }) => {
                    // Local edits not yet sent must survive: apply to both, keep local diffs on top.
                    let pending = diff(&self.base, doc);
                    apply(&mut self.base, &ops);
                    apply(doc, &ops);
                    apply(doc, &pending);
                    remote = true;
                }
                Ok(ToClient::Presence { from, presence }) => {
                    self.peers.insert(from, presence);
                }
                Ok(ToClient::Left { from }) => {
                    self.peers.remove(&from);
                }
                Ok(ToClient::Welcome { .. }) => {}
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.connected = false;
                    break;
                }
            }
        }
        if remote {
            doc.reflow();
        }
        let mut touched = vec![];
        if now - self.last_send >= 1.0 / 30.0 {
            let ops = diff(&self.base, doc);
            if !ops.is_empty() {
                for op in &ops {
                    match op {
                        Op::Upsert(n) => touched.push(n.id),
                        Op::Remove(id) => touched.push(*id),
                        _ => {}
                    }
                }
                let _ = self.link.tx.send(serde_json::to_string(&ToServer::Ops(ops)).unwrap_or_default());
                self.base = doc.clone();
                self.last_send = now;
            }
            if self.last_presence.as_ref() != Some(&self.me) {
                let _ = self.link.tx.send(serde_json::to_string(&ToServer::Presence(self.me.clone())).unwrap_or_default());
                self.last_presence = Some(self.me.clone());
            }
        }
        touched
    }
}

/// This machine's LAN address, for sharing a hosted session.
pub fn local_ip() -> String {
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("8.8.8.8:80")?;
            s.local_addr()
        })
        .map_or("127.0.0.1".into(), |a| a.ip().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Kind;
    use egui::{Rect, pos2, vec2};

    fn r(x: f32, y: f32) -> Rect {
        Rect::from_min_size(pos2(x, y), vec2(10., 10.))
    }

    #[test]
    fn diff_apply_roundtrip_and_repair() {
        let mut a = Document::default();
        let f = a.insert(0, None, None, Node::new(Kind::Frame, r(0., 0.)));
        let base = a.clone();
        let mut b = a.clone();
        let kid = a.insert(0, Some(f), None, Node::new(Kind::Rectangle, r(1., 1.)));
        a.node_mut(f).name = "Renamed".into();
        let ops = diff(&base, &a);
        apply(&mut b, &ops);
        assert_eq!(b.node(kid).parent, Some(f));
        assert_eq!(b.node(f).children, vec![kid]);
        assert_eq!(b.node(f).name, "Renamed");
        // A parent list that lost a child (concurrent edit) gets repaired.
        b.node_mut(f).children.clear();
        repair(&mut b);
        assert_eq!(b.node(f).children, vec![kid]);
    }

    #[test]
    fn two_clients_converge_through_a_relay() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let mut seed = Document::default();
        seed.insert(0, None, None, Node::new(Kind::Frame, r(0., 0.)));
        let shared = serve(listener, seed);
        let (mut ca, mut da) = Collab::join(connect(&addr, None).unwrap(), addr.clone(), "A".into(), true).unwrap();
        let (mut cb, mut db) = Collab::join(connect(&addr, None).unwrap(), addr.clone(), "B".into(), false).unwrap();
        assert_ne!(ca.client, cb.client);
        // Both add a layer at the same time; ids come from separate ranges.
        let na = da.insert(0, None, None, Node::new(Kind::Rectangle, r(100., 0.)));
        let nb = db.insert(0, None, None, Node::new(Kind::Ellipse, r(200., 0.)));
        assert_ne!(na, nb);
        ca.me.cursor = Some(pos2(5., 5.));
        let mut t = 1.0;
        for _ in 0..200 {
            ca.tick(&mut da, t);
            cb.tick(&mut db, t);
            t += 0.05;
            if da.nodes.len() == 3 && db.nodes.len() == 3 && cb.peers.get(&ca.client).is_some_and(|p| p.cursor.is_some()) {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(da.nodes.len(), 3, "A sees B's ellipse");
        assert_eq!(db.nodes.len(), 3, "B sees A's rectangle");
        assert_eq!(da.pages[0].children.len(), 3);
        assert_eq!(db.pages[0].children.len(), 3);
        assert_eq!(cb.peers[&ca.client].cursor, Some(pos2(5., 5.)), "presence relayed");
        // B edits A's rectangle; A sees it.
        db.node_mut(na).name = "From B".into();
        for _ in 0..200 {
            cb.tick(&mut db, t);
            ca.tick(&mut da, t);
            t += 0.05;
            if da.node(na).name == "From B" {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(da.node(na).name, "From B");
        assert_eq!(shared.lock().unwrap().nodes.len(), 3, "server holds the merged document");
    }
}
