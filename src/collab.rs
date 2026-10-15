//! Real-time multiplayer: a WebSocket relay holding the shared document, and clients that
//! send throttled diffs of their changes plus presence (cursors and selections).

use crate::doc::{Collection, Comment, Document, Node, NodeId, Page, Style, Variable};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use egui::Pos2;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
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

/// What a user is doing right now. Not part of the document: never saved, never undone.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Presence {
    pub name: String,
    pub color: usize,
    pub page: usize,
    pub cursor: Option<Pos2>,
    pub selection: Vec<NodeId>,
    /// Cursor chat (Figma's "/"): the message in the bubble by this user's pointer.
    #[serde(default)]
    pub chat: Option<String>,
    /// The part of the page on this user's screen (world space), for followers.
    #[serde(default)]
    pub view: Option<egui::Rect>,
    /// The text layer this user is editing and the caret's character index.
    #[serde(default)]
    pub caret: Option<(NodeId, usize)>,
    /// Spotlight: asks everyone else to follow this user.
    #[serde(default)]
    pub spotlight: bool,
}

/// The longest cursor chat message, in characters.
pub const CHAT_MAX: usize = 100;

impl Presence {
    /// Presence comes from peers, so bound its sizes and drop values that can't be drawn.
    pub fn sanitize(&mut self) {
        clip(&mut self.name, 40);
        if let Some(c) = &mut self.chat {
            clip(c, CHAT_MAX);
        }
        self.color %= PEER_COLORS.len();
        self.selection.truncate(2000);
        self.cursor = self.cursor.filter(|p| p.x.is_finite() && p.y.is_finite());
        self.view = self.view.filter(|r| r.is_finite() && r.is_positive());
    }
}

/// Cut `s` to at most `n` characters.
fn clip(s: &mut String, n: usize) {
    if let Some((i, _)) = s.char_indices().nth(n) {
        s.truncate(i);
    }
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
    /// The host removed this client; it shouldn't reconnect.
    Kicked,
}

pub const PEER_COLORS: [egui::Color32; 6] = [
    egui::Color32::from_rgb(0xF2, 0x48, 0x22),
    egui::Color32::from_rgb(0x14, 0xAE, 0x5C),
    egui::Color32::from_rgb(0x97, 0x47, 0xFF),
    egui::Color32::from_rgb(0xFF, 0xA6, 0x29),
    egui::Color32::from_rgb(0x0D, 0x99, 0xFF),
    egui::Color32::from_rgb(0xE8, 0x3E, 0x8C),
];

/// A participant's color (presence carries an index into `PEER_COLORS`).
pub fn peer_color(i: usize) -> egui::Color32 {
    PEER_COLORS[i % PEER_COLORS.len()]
}

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
    /// Everyone's latest presence, so a joiner sees people who aren't moving.
    presence: HashMap<u32, Presence>,
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

impl Relay {
    /// Remove a participant (the host's "Remove from session"): they're told, then disconnected.
    pub fn kick(&self, client: u32) {
        self.drop_client(client, true);
    }

    /// Close a client's connection; dropping its sender ends its pump after what's queued.
    fn drop_client(&self, client: u32, kicked: bool) {
        let mut s = self.state.lock().unwrap();
        if let Some(tx) = s.clients.remove(&client)
            && kicked
        {
            let _ = tx.send(crate::doc::to_json(&ToClient::Kicked));
        }
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
    let state = Arc::new(Mutex::new(ServerState { doc, clients: HashMap::new(), presence: HashMap::new(), next: 1 }));
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
                    for (&from, p) in &s.presence {
                        let _ = tx.send(crate::doc::to_json(&ToClient::Presence { from, presence: p.clone() }));
                    }
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
                        ToServer::Presence(mut presence) => {
                            presence.sanitize();
                            s.presence.insert(me, presence.clone());
                            ToClient::Presence { from: me, presence }
                        }
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
                s.presence.remove(&me);
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
    /// Woken when a message arrives (and kept for reconnecting).
    ctx: Option<egui::Context>,
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
    let wake = ctx.clone();
    std::thread::spawn(move || {
        pump(ws, rx_out, |text| {
            if let Ok(msg) = crate::doc::from_json::<ToClient>(&text) {
                if tx_in.send(msg).is_err() {
                    return false;
                }
                if let Some(ctx) = &wake {
                    ctx.request_repaint();
                }
            }
            true
        });
        // Wake the app so it notices the drop and starts reconnecting.
        if let Some(ctx) = &wake {
            ctx.request_repaint();
        }
    });
    Ok(Link { tx: tx_out, rx: rx_in, ctx })
}

/// The relay's first message: this client's id and the shared document.
fn welcome(link: &Link) -> Result<(u32, Document), String> {
    match link.rx.recv_timeout(Duration::from_secs(5)) {
        Ok(ToClient::Welcome { client, doc }) => Ok((client, *doc)),
        Ok(_) => Err("Unexpected reply".into()),
        Err(_) => Err("No response from the session".into()),
    }
}

/// How the link to the session is doing.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Connected,
    /// Lost the connection; retrying (the number of failed attempts so far).
    Reconnecting(u32),
    /// Gave up, or the host removed this user: why.
    Disconnected(String),
}

/// How long a dropped client keeps trying to get back in.
const RETRY_FOR: Duration = Duration::from_secs(30);

/// A reconnect thread's reports: a failed attempt's number, or the new link and its welcome.
type Retry = Receiver<Result<(Link, u32, Document), u32>>;

/// The app's side of a session.
pub struct Collab {
    pub link: Link,
    /// Server-assigned id (also the high bits of this client's node ids).
    pub client: u32,
    /// Document as last agreed with the server; local edits are diffs against it.
    pub base: Document,
    /// Others in the session by client id (ordered, so avatars keep their places).
    pub peers: BTreeMap<u32, Presence>,
    pub me: Presence,
    /// The address shown to share (the host's LAN address).
    pub address: String,
    /// The address this client connects to, kept for reconnecting.
    dial: String,
    pub hosting: bool,
    /// The relay this app runs when hosting; leaving the session stops it.
    pub relay: Option<Relay>,
    last_send: f64,
    last_presence: Option<Presence>,
    pub status: Status,
    retry: Option<Retry>,
    /// Peers whose spotlight turned on (true) or off (false) since the app last looked.
    pub spotlights: Vec<(u32, bool)>,
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

/// Each client allocates ids from its own range so concurrent inserts never collide.
fn own_ids(doc: &mut Document, client: u32) {
    doc.next_id = (client as u64) << 32;
    let ids: Vec<NodeId> = doc.nodes.keys().copied().collect();
    claim_ids(doc, client, ids);
}

impl Collab {
    /// Wait for the welcome (up to a few seconds) and set up the session.
    pub fn join(link: Link, address: String, name: String, hosting: bool) -> Result<(Collab, Document), String> {
        let (client, mut doc) = welcome(&link)?;
        own_ids(&mut doc, client);
        let me = Presence { name, color: client as usize % PEER_COLORS.len(), ..Default::default() };
        let c = Collab {
            link,
            client,
            base: doc.clone(),
            peers: BTreeMap::new(),
            me,
            dial: address.clone(),
            address,
            hosting,
            relay: None,
            last_send: 0.0,
            last_presence: None,
            status: Status::Connected,
            retry: None,
            spotlights: vec![],
        };
        Ok((c, doc))
    }

    /// Keep trying to get back into the session in the background, backing off, for up to
    /// `RETRY_FOR`. Local edits wait in the document meanwhile.
    pub fn reconnect(&mut self) {
        let (tx, rx) = channel();
        let (addr, ctx) = (self.dial.clone(), self.link.ctx.clone());
        std::thread::spawn(move || {
            let start = Instant::now();
            let mut wait = Duration::from_millis(250);
            for attempt in 1.. {
                std::thread::sleep(wait);
                let got = connect(&addr, ctx.clone()).and_then(|link| welcome(&link).map(|(client, doc)| (link, client, doc)));
                let give_up = got.is_err() && start.elapsed() + wait > RETRY_FOR;
                // Giving up drops the sender; leaving the session drops the receiver: stop then too.
                if give_up || tx.send(got.map_err(|_| attempt)).is_err() {
                    break;
                }
                if let Some(ctx) = &ctx {
                    ctx.request_repaint();
                }
                wait = (wait * 2).min(Duration::from_secs(5));
            }
            if let Some(ctx) = &ctx {
                ctx.request_repaint();
            }
        });
        self.retry = Some(rx);
        self.status = Status::Reconnecting(0);
        self.peers.clear();
    }

    /// Ops from the session: local edits not yet sent must survive, so apply them to both the
    /// agreed base and the document, then put the local diffs back on top.
    fn receive(&mut self, doc: &mut Document, ops: Vec<Op>, received: &mut Vec<Op>) {
        let pending = diff(&self.base, doc);
        apply(&mut self.base, &ops);
        apply(doc, &ops);
        apply(doc, &pending);
        claim_ids(doc, self.client, ops.iter().filter_map(|op| if let Op::Upsert(n) = op { Some(n.id) } else { None }));
        received.extend(ops);
    }

    /// Exchange changes: apply what arrived, send what changed locally (throttled).
    /// Returns ids of nodes this client changed in what it sent (for per-user undo).
    pub fn tick(&mut self, doc: &mut Document, now: f64) -> (Vec<NodeId>, Vec<Op>) {
        let mut received: Vec<Op> = vec![];
        while let Some(r) = self.retry.as_ref().map(|rx| rx.try_recv()) {
            match r {
                Ok(Err(attempt)) => self.status = Status::Reconnecting(attempt),
                Ok(Ok((link, client, fresh))) => {
                    // Back in: catch up on what others did meanwhile; unsent edits go out next.
                    self.retry = None;
                    (self.link, self.client, self.status, self.last_presence) = (link, client, Status::Connected, None);
                    let ops = diff(&self.base, &fresh);
                    self.receive(doc, ops, &mut received);
                    own_ids(doc, client);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.retry = None;
                    self.status = Status::Disconnected("Couldn't reach the session".into());
                }
            }
        }
        while self.status == Status::Connected {
            match self.link.rx.try_recv() {
                Ok(ToClient::Ops { ops, .. }) => self.receive(doc, ops, &mut received),
                Ok(ToClient::Presence { from, mut presence }) => {
                    presence.sanitize();
                    let was = self.peers.get(&from).is_some_and(|p| p.spotlight);
                    if presence.spotlight != was {
                        self.spotlights.push((from, presence.spotlight));
                    }
                    self.peers.insert(from, presence);
                }
                Ok(ToClient::Left { from }) => {
                    if self.peers.remove(&from).is_some_and(|p| p.spotlight) {
                        self.spotlights.push((from, false));
                    }
                }
                Ok(ToClient::Kicked) => {
                    self.status = Status::Disconnected("The host removed you from the session".into());
                    self.peers.clear();
                }
                Ok(ToClient::Welcome { .. }) => {}
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.reconnect(),
            }
        }
        if !received.is_empty() {
            doc.reflow();
        }
        let mut touched = vec![];
        if self.status == Status::Connected && now - self.last_send >= 1.0 / 30.0 {
            let ops = diff(&self.base, doc);
            // ponytail: ops queued just as the link drops are lost (no acks); add acks if that bites.
            let ids: Vec<NodeId> = ops.iter().filter_map(|op| match op {
                Op::Upsert(n) => Some(n.id),
                Op::Remove(id) => Some(*id),
                _ => None,
            }).collect();
            if !ops.is_empty() && self.link.tx.send(crate::doc::to_json(&ToServer::Ops(ops))).is_ok() {
                touched = ids;
                self.base = doc.clone();
                // A forced send (undo checkpoints pass f64::MAX) mustn't stall the throttle forever.
                if now < f64::MAX {
                    self.last_send = now;
                }
            }
            if self.last_presence.as_ref() != Some(&self.me) && self.link.tx.send(crate::doc::to_json(&ToServer::Presence(self.me.clone()))).is_ok() {
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
            if ca.status != Status::Connected {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_ne!(ca.status, Status::Connected, "clients are dropped when the host leaves");
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

    /// A relay with clients named `names`, all joined.
    fn session(names: &[&str]) -> (Relay, Vec<(Collab, Document)>) {
        let relay = serve(TcpListener::bind("127.0.0.1:0").unwrap(), Document::default(), session_token());
        let addr = format!("127.0.0.1:{}/{}", relay.port, relay.token);
        let cs = names.iter().map(|n| Collab::join(connect(&addr, None).unwrap(), addr.clone(), n.to_string(), false).unwrap()).collect();
        (relay, cs)
    }

    /// Tick every client until `done` (up to about 10 s; tests share the machine).
    fn settle(cs: &mut [(Collab, Document)], done: impl Fn(&[(Collab, Document)]) -> bool) {
        let mut t = 1.0;
        for _ in 0..2000 {
            for (c, d) in cs.iter_mut() {
                c.tick(d, t);
            }
            t += 0.05;
            if done(cs) {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn presence_extras_are_relayed_bounded_and_replayed_to_joiners() {
        let (relay, mut cs) = session(&["A", "B"]);
        let a = cs[0].0.client;
        let me = &mut cs[0].0.me;
        me.chat = Some("x".repeat(500));
        me.name = "N".repeat(500);
        me.view = Some(Rect::from_min_max(pos2(0., 0.), pos2(-5., 10.)));
        me.caret = Some((7, 3));
        me.spotlight = true;
        me.color = 1_000_003;
        settle(&mut cs, |cs| cs[1].0.peers.contains_key(&a));
        let p = &cs[1].0.peers[&a];
        assert_eq!(p.chat.as_ref().map(|c| c.chars().count()), Some(CHAT_MAX), "chat clipped");
        assert_eq!(p.name.chars().count(), 40, "name clipped");
        assert_eq!(p.view, None, "a view that can't be followed is dropped");
        assert_eq!(p.caret, Some((7, 3)));
        assert!(p.color < PEER_COLORS.len());
        assert_eq!(cs[1].0.spotlights, vec![(a, true)], "spotlight start reported");
        // Someone joining later sees A at once, though A hasn't moved since.
        let addr = format!("127.0.0.1:{}/{}", relay.port, relay.token);
        cs.push(Collab::join(connect(&addr, None).unwrap(), addr, "C".into(), false).unwrap());
        settle(&mut cs, |cs| cs[2].0.peers.contains_key(&a));
        assert!(cs[2].0.peers[&a].spotlight, "presence replayed to a joiner");
        // Turning spotlight off is reported too.
        cs[0].0.me.spotlight = false;
        settle(&mut cs, |cs| cs[1].0.spotlights.len() == 2);
        assert_eq!(cs[1].0.spotlights[1], (a, false));
    }

    #[test]
    fn a_kicked_client_is_told_and_stays_out() {
        let (relay, mut cs) = session(&["Host", "Guest"]);
        let (host, guest) = (cs[0].0.client, cs[1].0.client);
        settle(&mut cs, |cs| cs[0].0.peers.contains_key(&guest) && cs[1].0.peers.contains_key(&host));
        relay.kick(guest);
        settle(&mut cs, |cs| matches!(cs[1].0.status, Status::Disconnected(_)) && cs[0].0.peers.is_empty());
        assert!(matches!(&cs[1].0.status, Status::Disconnected(why) if why.contains("removed")), "{:?}", cs[1].0.status);
        assert!(cs[0].0.peers.is_empty(), "the host sees them leave");
        // No reconnecting after a kick.
        std::thread::sleep(Duration::from_millis(400));
        let (c, d) = &mut cs[1];
        c.tick(d, 100.0);
        assert!(matches!(cs[1].0.status, Status::Disconnected(_)));
        assert_eq!(relay.state.lock().unwrap().clients.len(), 1);
    }

    #[test]
    fn a_dropped_client_reconnects_and_keeps_its_unsent_edits() {
        let (relay, mut cs) = session(&["A", "B"]);
        let b = cs[1].0.client;
        settle(&mut cs, |cs| cs[0].0.peers.contains_key(&b));
        relay.drop_client(b, false);
        settle(&mut cs, |cs| cs[1].0.status != Status::Connected);
        assert!(matches!(cs[1].0.status, Status::Reconnecting(_)));
        // Both edit while B is away: B's edit waits locally, A's reaches B once back.
        let nb = cs[1].1.insert(0, None, None, Node::new(Kind::Ellipse, r(50., 50.)));
        let na = cs[0].1.insert(0, None, None, Node::new(Kind::Rectangle, r(0., 0.)));
        settle(&mut cs, |cs| cs[1].0.status == Status::Connected && cs[0].1.nodes.contains_key(&nb) && cs[1].1.nodes.contains_key(&na));
        assert_eq!(cs[1].0.status, Status::Connected);
        assert_ne!(cs[1].0.client, b, "a new connection");
        assert!(cs[0].1.nodes.contains_key(&nb), "B's offline edit reached A");
        assert!(cs[1].1.nodes.contains_key(&na), "A's edit reached B");
        // New layers from B use its new id range, never clashing with the old ones.
        let again = cs[1].1.insert(0, None, None, Node::new(Kind::Ellipse, r(90., 90.)));
        assert_eq!(again >> 32, cs[1].0.client as u64);
        assert_eq!(relay.doc.lock().unwrap().nodes.len(), 2);
    }
}
