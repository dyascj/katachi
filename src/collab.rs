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
use std::sync::atomic::{AtomicBool, Ordering};
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
        // Untouched layers share their Arc with the base: skip the deep compare.
        if !base.nodes.get(id).is_some_and(|b| Arc::ptr_eq(b, n) || same_but_expanded(b, n)) {
            ops.push(Op::Upsert(Box::new((**n).clone())));
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

/// Equal apart from `expanded`: whether a layer is open in the layers panel is each user's own
/// view, so it isn't sent (selecting deep inside a frame would overwrite peers' edits to it).
fn same_but_expanded(a: &Node, b: &Node) -> bool {
    if a.expanded == b.expanded {
        return a == b;
    }
    let mut a = a.clone();
    a.expanded = b.expanded;
    a == *b
}

/// Apply ops. Derived text layout (wrapped lines) and layers panel state stay local, so peers
/// don't echo them back.
pub fn apply(doc: &mut Document, ops: &[Op]) {
    for op in ops {
        match op {
            Op::Upsert(n) => {
                let mut n = (**n).clone();
                if let Some(local) = doc.nodes.get(&n.id) {
                    n.lines = local.lines.clone();
                    n.expanded = local.expanded;
                }
                doc.nodes.insert(n.id, std::sync::Arc::new(n));
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
/// Also makes a loaded file safe: at least one page, and no layer inside itself.
pub fn repair(doc: &mut Document) {
    if doc.pages.is_empty() {
        doc.pages = Document::default().pages;
    }
    let mut ordered: Vec<NodeId> = doc.nodes.keys().copied().collect();
    ordered.sort();
    // A parent chain that comes back to the layer is a cycle: the layer closing it moves to the top level.
    for &id in &ordered {
        let (mut cur, mut steps) = (doc.nodes[&id].parent, 0);
        while let Some(p) = cur
            && steps <= doc.nodes.len()
        {
            if p == id {
                doc.node_mut(id).parent = None;
                break;
            }
            (cur, steps) = (doc.nodes.get(&p).and_then(|n| n.parent), steps + 1);
        }
    }
    let ids: HashSet<NodeId> = doc.nodes.keys().copied().collect();
    for page in &mut doc.pages {
        page.children.retain(|c| ids.contains(c) && doc.nodes[c].parent.is_none());
        let mut seen = HashSet::new();
        page.children.retain(|c| seen.insert(*c));
    }
    let parents: HashMap<NodeId, Option<NodeId>> = doc.nodes.iter().map(|(k, n)| (*k, n.parent)).collect();
    for arc in doc.nodes.values_mut() {
        let me = arc.id;
        let mut seen = HashSet::new();
        let kids: Vec<NodeId> = arc.children.iter().copied().filter(|c| parents.get(c) == Some(&Some(me)) && seen.insert(*c)).collect();
        if kids != arc.children {
            std::sync::Arc::make_mut(arc).children = kids;
        }
    }
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

/// A running relay. Dropping it closes the port and disconnects everyone.
pub struct Relay {
    /// The shared document, as merged so far.
    pub doc: Arc<Mutex<Document>>,
    /// The session code: joiners connect to `host:port/token`; anything else is refused.
    pub token: String,
    port: u16,
    stop: Arc<AtomicBool>,
    state: Arc<Mutex<ServerState>>,
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Dropping the senders ends every connection's pump.
        if let Ok(mut s) = self.state.lock() {
            s.clients.clear();
        }
        // Wake the accept loop so it sees the flag.
        let _ = TcpStream::connect_timeout(&(std::net::Ipv4Addr::LOCALHOST, self.port).into(), Duration::from_secs(1));
    }
}

/// A hard-to-guess session code (64 bits from the std hasher's OS-seeded random keys).
pub fn session_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::hash::RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
    h.write_u32(std::process::id());
    format!("{:016x}", h.finish())
}

/// Run the relay on `listener` in background threads, admitting only clients that know `token`.
pub fn serve(listener: TcpListener, doc: Document, token: String) -> Relay {
    let state = Arc::new(Mutex::new(ServerState { doc, clients: HashMap::new(), next: 1 }));
    let snap = Arc::new(Mutex::new(state.lock().unwrap().doc.clone()));
    let stop = Arc::new(AtomicBool::new(false));
    let port = listener.local_addr().map_or(0, |a| a.port());
    let relay = Relay { doc: snap.clone(), token: token.clone(), port, stop: stop.clone(), state: state.clone() };
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            let Ok(stream) = stream else { continue };
            let (state, snap, token) = (state.clone(), snap.clone(), token.clone());
            std::thread::spawn(move || {
                // A connection that never finishes the handshake mustn't hold a thread forever.
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let check = |req: &tungstenite::handshake::server::Request, resp| {
                    if req.uri().path().trim_start_matches('/') == token {
                        Ok(resp)
                    } else {
                        Err(tungstenite::http::Response::builder().status(403).body(Some("Wrong session code".to_string())).unwrap_or_default())
                    }
                };
                let Ok(ws) = tungstenite::accept_hdr(stream, check) else { return };
                let (tx, rx) = channel::<String>();
                let me = {
                    let mut s = state.lock().unwrap();
                    let me = s.next;
                    s.next += 1;
                    let welcome = ToClient::Welcome { client: me, doc: Box::new(s.doc.clone()) };
                    let _ = tx.send(crate::doc::to_json(&welcome));
                    // Only the relay holds the sender, so dropping the relay ends this connection.
                    s.clients.insert(me, tx);
                    me
                };
                pump(ws, rx, |text| {
                    let Ok(msg) = crate::doc::from_json::<ToServer>(&text) else { return true };
                    let mut s = state.lock().unwrap();
                    let out = match msg {
                        ToServer::Ops(ops) => {
                            apply(&mut s.doc, &ops);
                            *snap.lock().unwrap() = s.doc.clone();
                            ToClient::Ops { from: me, ops }
                        }
                        ToServer::Presence(presence) => ToClient::Presence { from: me, presence },
                    };
                    let json = crate::doc::to_json(&out);
                    for (id, c) in &s.clients {
                        if *id != me {
                            let _ = c.send(json.clone());
                        }
                    }
                    true
                });
                let mut s = state.lock().unwrap();
                s.clients.remove(&me);
                let left = crate::doc::to_json(&ToClient::Left { from: me });
                for c in s.clients.values() {
                    let _ = c.send(left.clone());
                }
            });
        }
    });
    relay
}

/// A connection to a relay: messages in, JSON out.
pub struct Link {
    pub tx: Sender<String>,
    pub rx: Receiver<ToClient>,
}

/// Connect to `host:port/code`, the address the host shares.
pub fn connect(addr: &str, ctx: Option<egui::Context>) -> Result<Link, String> {
    let url = if addr.starts_with("ws://") { addr.to_string() } else { format!("ws://{addr}") };
    let host = url.trim_start_matches("ws://").split('/').next().unwrap_or_default().to_string();
    // Give up on an unreachable address in seconds, not the OS's minute and more.
    let mut err = "No such address".to_string();
    let stream = std::net::ToSocketAddrs::to_socket_addrs(&host)
        .map_err(|e| e.to_string())?
        .find_map(|a| TcpStream::connect_timeout(&a, Duration::from_secs(5)).map_err(|e| err = e.to_string()).ok())
        .ok_or(err)?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let (ws, _) = tungstenite::client(url.as_str(), stream).map_err(|e| match e {
        tungstenite::HandshakeError::Failure(tungstenite::Error::Http(r)) if r.status() == 403 => "Wrong session code. Ask the host for the full address.".to_string(),
        e => e.to_string(),
    })?;
    let (tx_out, rx_out) = channel::<String>();
    let (tx_in, rx_in) = channel::<ToClient>();
    std::thread::spawn(move || {
        pump(ws, rx_out, |text| {
            if let Ok(msg) = crate::doc::from_json::<ToClient>(&text) {
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
    pub client: u32,
    /// Document as last agreed with the server; local edits are diffs against it.
    pub base: Document,
    pub peers: HashMap<u32, Presence>,
    pub me: Presence,
    pub address: String,
    pub hosting: bool,
    /// The relay this app runs when hosting; leaving the session stops it.
    pub relay: Option<Relay>,
    last_send: f64,
    last_presence: Option<Presence>,
    pub connected: bool,
}

/// Move `next_id` past any of `ids` in this client's range, so it never reuses one (a relay that
/// restarted hands out client numbers again, and the file may hold that number's old layers).
fn claim_ids(doc: &mut Document, client: u32, ids: impl IntoIterator<Item = NodeId>) {
    let lo = (client as u64) << 32;
    for id in ids {
        if (lo..lo + (1 << 32)).contains(&id) {
            doc.next_id = doc.next_id.max(id + 1);
        }
    }
}

impl Collab {
    /// Wait for the welcome (up to a few seconds) and set up the session.
    pub fn join(link: Link, address: String, name: String, hosting: bool) -> Result<(Collab, Document), String> {
        let welcome = link.rx.recv_timeout(Duration::from_secs(5)).map_err(|_| "No response from the session".to_string())?;
        let ToClient::Welcome { client, doc } = welcome else { return Err("Unexpected reply".into()) };
        let mut doc = *doc;
        // Each client allocates ids from its own range so concurrent inserts never collide.
        doc.next_id = (client as u64) << 32;
        let ids: Vec<NodeId> = doc.nodes.keys().copied().collect();
        claim_ids(&mut doc, client, ids);
        let me = Presence { name, color: client as usize % PEER_COLORS.len(), page: 0, cursor: None, selection: vec![] };
        let c = Collab { link, client, base: doc.clone(), peers: HashMap::new(), me, address, hosting, relay: None, last_send: 0.0, last_presence: None, connected: true };
        Ok((c, doc))
    }

    /// Exchange changes: apply what arrived, send what changed locally (throttled).
    /// Returns ids of nodes this client changed in what it sent (for per-user undo).
    pub fn tick(&mut self, doc: &mut Document, now: f64) -> (Vec<NodeId>, Vec<Op>) {
        let mut remote = false;
        let mut received: Vec<Op> = vec![];
        loop {
            match self.link.rx.try_recv() {
                Ok(ToClient::Ops { ops, .. }) => {
                    // Local edits not yet sent must survive: apply to both, keep local diffs on top.
                    let pending = diff(&self.base, doc);
                    apply(&mut self.base, &ops);
                    apply(doc, &ops);
                    apply(doc, &pending);
                    claim_ids(doc, self.client, ops.iter().filter_map(|op| if let Op::Upsert(n) = op { Some(n.id) } else { None }));
                    remote = true;
                    received.extend(ops);
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
                let _ = self.link.tx.send(crate::doc::to_json(&ToServer::Ops(ops)));
                self.base = doc.clone();
                // A forced send (undo checkpoints pass f64::MAX) mustn't stall the throttle forever.
                if now < f64::MAX {
                    self.last_send = now;
                }
            }
            if self.last_presence.as_ref() != Some(&self.me) {
                let _ = self.link.tx.send(crate::doc::to_json(&ToServer::Presence(self.me.clone())));
                self.last_presence = Some(self.me.clone());
            }
        }
        (touched, received)
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
        let mut seed = Document::default();
        let frame = seed.insert(0, None, None, Node::new(Kind::Frame, r(0., 0.)));
        let relay = serve(listener, seed, session_token());
        let shared = relay.doc.clone();
        let addr = format!("127.0.0.1:{}/{}", relay.port, relay.token);
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
        // B edits A's rectangle; A sees it. Folding a frame in the layers panel stays B's own.
        db.node_mut(na).name = "From B".into();
        db.node_mut(frame).expanded = !da.node(frame).expanded;
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
        assert_ne!(da.node(frame).expanded, db.node(frame).expanded, "expanded isn't shared");
        assert_eq!(shared.lock().unwrap().nodes.len(), 3, "server holds the merged document");
        // Leaving (dropping the relay) disconnects everyone and closes the port.
        drop(relay);
        for _ in 0..200 {
            ca.tick(&mut da, t);
            if !ca.connected {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!ca.connected, "clients are dropped when the host leaves");
        std::thread::sleep(Duration::from_millis(50));
        assert!(connect(&addr, None).is_err(), "the port no longer accepts");
    }

    #[test]
    fn relay_wants_the_session_code_and_ids_skip_existing_layers() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut seed = Document::default();
        // Layers an earlier client 1 made, before the relay restarted.
        let mut old = Node::new(Kind::Rectangle, r(0., 0.));
        old.id = (1 << 32) + 7;
        seed.nodes.insert(old.id, Arc::new(old));
        repair(&mut seed);
        let relay = serve(listener, seed, "secret".into());
        let host = format!("127.0.0.1:{}", relay.port);
        assert!(connect(&host, None).is_err(), "no code");
        assert!(connect(&format!("{host}/guess"), None).is_err(), "wrong code");
        let (c, mut d) = Collab::join(connect(&format!("{host}/secret"), None).unwrap(), host.clone(), "A".into(), false).unwrap();
        assert_eq!(c.client, 1);
        let id = d.insert(0, None, None, Node::new(Kind::Ellipse, r(5., 5.)));
        assert_eq!(id, (1 << 32) + 8, "new ids go past the existing ones in this client's range");
    }
}
