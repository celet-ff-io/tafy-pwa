// The UI-side i2c client: the PWA speaks the frame protocol to the
// wasm core through a MessageChannel bridged inside the worker.
//
// Wire discipline (protocol.md § The wire): frame #1 = the handshake
// (both directions; the core's ack = a raw Handshake, not an
// OutgoingFrame — positional typing); frames #2..n = bodies
// (IncomingFrame UI->core, OutgoingFrame core->UI). The client is
// SINGLE-FLIGHT (the stdio grammar is one op at a time): answers pair
// with the pending request; statements route to the broadcast state.
// A commit returns TWO frames — msg_package_push + the resting draft —
// returned by the API as one pair (B7: the package carries the
// complete roster; MsgAdded is retired).
//
// Day-one content kind = "text/plain"; the ts claim
// (application/vnd.zconn.ts) is stamped by the core at commit.

use std::collections::HashMap;

use dioxus::prelude::*;
use prost::Message as _;
use zconned_stdio_types::{IncomingFrame, OutgoingFrame, PROTOCOL_VERSION};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::spawn_local;
// native check face: the client never RUNS natively (the core is a
// browser worker, A17) — the stub only lets a plain `cargo check`
// typecheck the ungated frame/state code.
#[cfg(not(target_arch = "wasm32"))]
fn spawn_local<F: std::future::Future<Output = ()> + 'static>(f: F) {
    let _ = f;
}

// TEMP boot breadcrumbs (removed after the device verification)
#[cfg(not(target_arch = "wasm32"))]
fn trace(m: &str) {
    eprintln!("[client] {m}");
}
#[cfg(target_arch = "wasm32")]
fn trace(m: &str) {
    web_sys::console::log_1(&format!("[client] {m}").into());
}

// ---- the interface-facing state (one GlobalSignal; the client task
// writes it, the pages read it) ----

#[derive(Clone, Debug, PartialEq, Default)]
pub enum Status {
    #[default]
    /// boot in progress
    Booting,
    /// handshake ok, identity + threads seeded
    Ready,
    /// loud failure (handshake mismatch, link mis-decode, worker gone)
    Failed(String),
}

/// The relay connection's live face (the worker's control-port
/// statements: the visit's real state — no probe theater).
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Conn {
    #[default]
    Disconnected,
    Connecting,
    Connected,
    /// the visit failed (the detail = the transport's error)
    Error(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ThreadRow {
    /// the thread id: "<major>:<minor>:<algo>:<digest-hex>"
    pub id: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LogRow {
    /// the entry's own MsgRef in id form (the cursor's `at` leaf)
    pub ref_id: String,
    pub author: String,
    pub text: String,
    /// unix ms (the row's local record time)
    pub ts_ms: u64,
}

/// One contact (a Peer record's display face; 107's W2, web-adapted).
#[derive(Clone, Debug, PartialEq)]
pub struct PeerRow {
    /// the display fingerprint: sign_pub's first 8 bytes, hex pairs
    pub fp: String,
    /// the identity anchor (SHA3-256 of sign_pub, full hex) — the
    /// join key against a row's meta.from (B6 §4)
    pub anchor_hex: String,
    pub sign_algo: String,
    pub dh_algo: String,
    /// the peer's endpoint hints, verbatim
    pub endpoints: Vec<String>,
    /// the canonical single-line base64 (the shareable copy source)
    pub card: String,
    /// the card carries the record-signing key (B6 field 8)? absent
    /// = an old card — the peer's record signatures are unverifiable
    /// until re-exchange (107's pitfall 3: surface it, never hide it)
    pub has_record_key: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CoreState {
    pub status: Status,
    pub self_card: Option<String>,
    pub peers: Vec<PeerRow>,
    pub threads: Vec<ThreadRow>,
    /// thread id -> fetched log (oldest first as the core states it)
    pub logs: HashMap<String, Vec<LogRow>>,
    /// the last Error frame, surfaced verbatim in the UI
    pub error: Option<String>,
    /// the relay connection's live face
    pub conn: Conn,
    /// the self core's identity anchor (full hex; None = no card yet)
    pub self_anchor: Option<String>,
    /// the client boot gate (the io task up + the seed done): mount-
    /// time fetches key off this so they re-fire after a boot race
    pub booted: bool,
}

pub static CORE: GlobalSignal<CoreState> = Signal::global(CoreState::default);

// ---- wire helpers ----

fn ref_id_of(ver_major: u32, ver_minor: u32, algo: &str, digest: &[u8]) -> String {
    format!(
        "{}:{}:{}:{}",
        ver_major,
        ver_minor,
        algo,
        digest
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    )
}

fn entry_ref_id(entry: &zconned_stdio_types::Entry) -> String {
    let meta = entry.meta.as_ref();
    match meta {
        Some(m) => ref_id_of(entry.ver_major, entry.ver_minor, &m.algo, &m.digest),
        None => format!("{}:{}::", entry.ver_major, entry.ver_minor),
    }
}

fn thread_ref_id(te: &zconned_stdio_types::ThreadEntry) -> String {
    match te.thread.as_ref() {
        Some(r) => ref_id_of(r.ver_major, r.ver_minor, &r.algo, &r.digest),
        None => String::new(),
    }
}

fn parse_ref_id(id: &str) -> Option<zconned_material::MsgRef> {
    let mut parts = id.splitn(4, ':');
    let ver_major = parts.next()?.parse().ok()?;
    let ver_minor = parts.next()?.parse().ok()?;
    let algo = parts.next()?.to_string();
    let hexs = parts.next()?;
    if hexs.len() % 2 != 0 {
        return None;
    }
    let digest = (0..hexs.len() / 2)
        .map(|i| u8::from_str_radix(&hexs[i * 2..i * 2 + 2], 16))
        .collect::<Result<Vec<u8>, _>>()
        .ok()?;
    Some(zconned_material::MsgRef {
        ver_major,
        ver_minor,
        algo,
        digest,
    })
}

// RFC3339 UTC ("2026-11-11T08:47:00.123Z") -> epoch millis. The ts
// claim is the sender-declared compose time; a non-UTC offset or a
// malformed string = None (the caller falls back to the observation).
// Days-from-civil: Howard Hinnant's algorithm (valid for the whole
// representable range, no Y2038-class hazard).
fn chrono_millis(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':'
    {
        return None;
    }
    let num = |r: std::ops::Range<usize>| -> Option<u32> {
        s.get(r)?.bytes().try_fold(0u32, |a, c| {
            c.is_ascii_digit().then(|| a * 10 + u32::from(c - b'0'))
        })
    };
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi, se) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) {
        return None;
    }
    // leap-year-correct day count
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let mdays = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if d > mdays[(mo - 1) as usize] {
        return None;
    }
    // optional fraction + zone suffix (Z or +hh:mm — only Z accepted)
    let rest = &s[19..];
    let mut millis = 0u64;
    let zone_ok = if let Some(frac) = rest.strip_prefix('.') {
        let digits: String = frac.chars().take_while(|c| c.is_ascii_digit()).collect();
        let zone = &frac[digits.len()..];
        // millis = the first three fraction digits, zero-padded
        let mut m = 0u64;
        for (i, c) in digits.bytes().take(3).enumerate() {
            m += u64::from(c - b'0') * [100, 10, 1][i];
        }
        millis = m;
        zone == "Z"
    } else {
        rest == "Z"
    };
    if !zone_ok {
        return None;
    }
    let yy = y as i64;
    let mp = if mo > 2 { mo } else { mo + 12 } as i64;
    let (yy2, mp2) = if mo > 2 { (yy, mp) } else { (yy - 1, mp) };
    let era = if yy2 >= 0 { yy2 } else { yy2 - 399 } / 400;
    let yoe = yy2 - era * 400;
    let doy = (153 * (mp2 - 3) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let secs = days * 86_400 + h as i64 * 3600 + mi as i64 * 60 + se as i64;
    (secs >= 0)
        .then(|| (secs as u64) * 1000 + millis)
}

fn entry_row(entry: &zconned_stdio_types::Entry) -> LogRow {
    // content = the resolved slot's value (kind text/plain day one)
    let text = entry
        .content
        .as_ref()
        .and_then(|c| c.value.as_ref())
        .map(|v| String::from_utf8_lossy(&v.data).into_owned())
        .unwrap_or_default();
    // THE SEND TIME (the 106 verdict on the battery report): meta.ts
    // is the OBSERVER's ingest time by design (RowMeta = the
    // observer-facts half); the sender's time rides the entry's ts
    // claim (application/vnd.zconn.ts, stamped at compose) — prefer
    // the claim when present, else the observation.
    let ts_ms = entry
        .claims
        .iter()
        .find(|c| c.kind == "application/vnd.zconn.ts")
        .and_then(|c| c.value.as_ref())
        .map(|v| String::from_utf8_lossy(&v.data).into_owned())
        .and_then(|s| chrono_millis(&s))
        .or_else(|| {
            entry
                .meta
                .as_ref()
                .and_then(|m| m.ts.as_ref())
                .map(|t| (t.seconds.max(0) as u64) * 1000 + (t.nanos / 1_000_000) as u64)
        })
        .unwrap_or(0);
    LogRow {
        ref_id: entry_ref_id(entry),
        // B6 §4: the author = the store's observation of meta.from,
        // joined against the PeerBook (the self anchor renders Me;
        // the display name lives in the PeerBook's own face)
        author: author_face(
            entry
                .meta
                .as_ref()
                .map(|m| m.from.as_slice())
                .unwrap_or(&[]),
        ),
        text,
        ts_ms,
    }
}

// ---- the client ----

#[derive(Debug)]
enum Expect {
    /// exactly one answer frame of this shape
    One(AnswerKind),
    /// the commit pair: msg_package_push + the resting draft
    /// (MsgDraftPush) — B7 E1: the self-signature lands with the
    /// msg, so the package carries the COMPLETE roster
    Commit,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum AnswerKind {
    MsgPackagePush,
    MsgCursorPush,
    MsgDraftPush,
    MsgThreadsPush,
    MsgThreadsLogPush,
    SelfPush,
    PeersPush,
}

fn answer_kind(f: &OutgoingFrame) -> Option<AnswerKind> {
    use zconned_stdio_types::outgoing_frame::Frame as F;
    match f.frame.as_ref()? {
        F::MsgPackagePush(_) => Some(AnswerKind::MsgPackagePush),
        F::MsgCursorPush(_) => Some(AnswerKind::MsgCursorPush),
        F::MsgDraftPush(_) => Some(AnswerKind::MsgDraftPush),
        F::MsgThreadsPush(_) => Some(AnswerKind::MsgThreadsPush),
        F::MsgThreadsLogPush(_) => Some(AnswerKind::MsgThreadsLogPush),
        F::SelfPush(_) => Some(AnswerKind::SelfPush),
        F::PeersPush(_) => Some(AnswerKind::PeersPush),
        _ => None,
    }
}

struct Req {
    frame: IncomingFrame,
    expect: Expect,
    reply: tokio::sync::oneshot::Sender<Result<Vec<OutgoingFrame>, String>>,
}

struct CoreIo {
    /// inbound frames from the worker (one postMessage = one frame)
    inbound: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
    ops: tokio::sync::mpsc::UnboundedReceiver<Req>,
    /// outbound bytes (the worker pump posts them to the port)
    tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
}

impl CoreIo {
    fn send_frame(&self, frame: &impl prost::Message) {
        let _ = self.tx.send(frame.encode_length_delimited_to_vec());
    }

    /// The single-flight IO loop. Handshake first (positional frame
    /// #1); then ops serialize; statements route to the state.
    async fn run(mut self) {
        // ---- frame #1: the handshake, both directions ----
        self.send_frame(&zconned_stdio_types::Handshake {
            version: PROTOCOL_VERSION,
        });
        let mut ack = None;
        while ack.is_none() {
            match self.inbound.recv().await {
                Some(bytes) => {
                    match zconned_stdio_types::Handshake::decode(&mut bytes.as_slice()) {
                        Ok(h) => ack = Some(h.version),
                        // a body frame before the ack = a mis-decoded
                        // link (protocol.md: loud, not silent)
                        Err(_) => {
                            CORE.write().status = Status::Failed(
                                "link mis-decoded: a body frame arrived before the handshake ack"
                                    .to_string(),
                            );
                            return;
                        }
                    }
                }
                None => {
                    CORE.write().status = Status::Failed("link died before the ack".to_string());
                    return;
                }
            }
        }
        trace(&format!("handshake ack = {:?}", ack));
        if ack != Some(PROTOCOL_VERSION) {
            CORE.write().status = Status::Failed(format!(
                "handshake version mismatch: core {:?}, interface {PROTOCOL_VERSION}",
                ack
            ));
            return;
        }

        // ---- the single-flight loop ----
        let mut pending: Option<(
            Expect,
            tokio::sync::oneshot::Sender<Result<Vec<OutgoingFrame>, String>>,
            Vec<OutgoingFrame>,
        )> = None;
        loop {
            tokio::select! {
                op = self.ops.recv(), if pending.is_none() => match op {
                    Some(req) => {
                        self.send_frame(&req.frame);
                        pending = Some((req.expect, req.reply, Vec::new()));
                    }
                    None => break, // the client is gone
                },
                frame = self.inbound.recv() => {
                    let Some(bytes) = frame else { break };
                    // the worker's outbound pump posts DEFAMED payloads
                    // (one frame per postMessage) — plain decode
                    let f = match OutgoingFrame::decode(&mut bytes.as_slice()) {
                        Ok(f) => f,
                        Err(e) => {
                            CORE.write().status =
                                Status::Failed(format!("link mis-decoded: {e}"));
                            break;
                        }
                    };
                    if let Some((code, message)) = error_of(&f) {
                        // an Error frame resolves the pending op (or
                        // floats alone as a note)
                        if let Some((_, reply, _)) = pending.take() {
                            let _ = reply.send(Err(format!("{code}: {message}")));
                        }
                        CORE.write().error = Some(format!("{code}: {message}"));
                        continue;
                    }
                    match pending.take() {
                        None => {
                            // TEMP io probe (retire list pending)
                            trace(&format!("[io] statement routed: {}B", bytes.len()));
                            route_statement(f);
                        }
                        Some((expect, reply, mut acc)) => {
                            let matches = match (&expect, answer_kind(&f)) {
                                (Expect::One(want), Some(got)) => got == *want,
                                (Expect::Commit, Some(AnswerKind::MsgPackagePush)) => acc.is_empty(),
                                (Expect::Commit, Some(AnswerKind::MsgDraftPush)) => !acc.is_empty(),
                                _ => false,
                            };
                            // TEMP io probe (retire list pending)
                            trace(&format!(
                                "[io] frame {}B kind={:?} expect={:?} matches={matches}",
                                bytes.len(),
                                answer_kind(&f),
                                std::mem::discriminant(&expect)
                            ));
                            if matches {
                                acc.push(f);
                                let done = match &expect {
                                    Expect::One(_) => true,
                                    Expect::Commit => acc.len() == 2,
                                };
                                if done {
                                    let _ = reply.send(Ok(acc));
                                } else {
                                    pending = Some((expect, reply, acc));
                                }
                            } else {
                                // not our answer: a statement sneaking
                                // in mid-op
                                route_statement(f);
                                pending = Some((expect, reply, acc));
                            }
                        }
                    }
                }
            }
        }
    }
}

fn error_of(f: &OutgoingFrame) -> Option<(String, String)> {
    use zconned_stdio_types::outgoing_frame::Frame as F;
    match f.frame.as_ref()? {
        F::Error(e) => Some((e.code.clone(), e.message.clone())),
        _ => None,
    }
}

fn peer_row_of(peer: &zconned_stdio_types::Peer) -> Option<PeerRow> {
    use base64::Engine as _;
    let fp: String = peer
        .sign_pub
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect();
    let fp = fp
        .as_bytes()
        .chunks(4)
        .map(|c| String::from_utf8_lossy(c).to_string())
        .collect::<Vec<_>>()
        .join(" ");
    let card = base64::engine::general_purpose::STANDARD.encode(peer.encode_to_vec());
    Some(PeerRow {
        fp,
        anchor_hex: anchor_hex_of_peer(peer).unwrap_or_default(),
        sign_algo: peer.sign_algo.clone(),
        dh_algo: peer.dh_algo.clone(),
        endpoints: peer.endpoints.clone(),
        card,
        has_record_key: peer.record_sign_pub.is_some(),
    })
}

/// The identity anchor (SHA3-256 of sign_pub) as full lowercase hex.
fn anchor_hex_of_peer(peer: &zconned_stdio_types::Peer) -> Option<String> {
    let mut sign = [0u8; 32];
    if peer.sign_pub.len() != 32 {
        return None;
    }
    sign.copy_from_slice(&peer.sign_pub);
    Some(
        zconned_crypto::Fingerprint::of(&sign)
            .digest()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    )
}

/// The row author's display face (B6 §4): meta.from = the author's
/// mailbox anchor as the store observed it. The join is against the
/// UI's PeerBook; the self anchor renders "Me"; a stranger shows the
/// short anchor; an absent anchor renders the bare dash (missing =
/// fact, the committing core had no mail identity).
fn author_face(from: &[u8]) -> String {
    if from.is_empty() {
        return "—".to_string();
    }
    let hex: String = from.iter().map(|b| format!("{b:02x}")).collect();
    let st = CORE.read();
    if st.self_anchor.as_deref() == Some(hex.as_str()) {
        return "Me".to_string();
    }
    if let Some(p) = st.peers.iter().find(|p| p.anchor_hex == hex) {
        return p.fp.clone();
    }
    // stranger: the short anchor form (the first six bytes)
    format!("{}…", &hex[..12])
}

fn set_peers(st: &mut CoreState, peers: &[zconned_stdio_types::Peer]) {
    st.peers = peers.iter().filter_map(peer_row_of).collect();
}

/// Unsolicited frames: the statement bus. B7: the msg domain speaks
/// ONE frame — every MsgPackagePush (arrival, commit receipt whose
/// reply consumer is gone, roster-driven re-push) is the same
/// upsert-by-digest ingest; the roster rides the package itself
/// (local.signer), so the old roster channel is gone entirely.
fn route_statement(f: OutgoingFrame) {
    use zconned_stdio_types::outgoing_frame::Frame as F;
    if let F::PeersPush(p) = f.frame.as_ref().unwrap() {
        set_peers(&mut CORE.write(), &p.peers);
        return;
    }
    // a resource landed in the core: fill the matching row slot (the
    // row's ref tail = ":<algo>:<digest-hex>" — the push carries the
    // same pair) and re-render
    if let F::ResourcePush(p) = f.frame.as_ref().unwrap()
        && let (Some(r), Some(v)) = (p.r#ref.as_ref(), p.value.as_ref())
    {
        let tail = format!(
            ":{}:{}",
            r.algo,
            r.digest
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
        let text = String::from_utf8_lossy(&v.data).into_owned();
        let mut st = CORE.write();
        for rows in st.logs.values_mut() {
            for row in rows.iter_mut() {
                if row.ref_id.ends_with(&tail) {
                    row.text = text.clone();
                }
            }
        }
        return;
    }
    // B7 §4.1: ONE ingest path — upsert by the entry's digest. No
    // branching on frame provenance (arrival / own-commit receipt /
    // roster re-push are the same frame; newness and own-ness are
    // derivable locally). The roster (local.signer) is the ✍ join's
    // second input when the confirmed-from display gets ruled — the
    // verdict stays ours (the core reports facts, never the
    // conjunction).
    if let F::MsgPackagePush(p) = f.frame.as_ref().unwrap()
        && let Some(pkg) = p.package.as_ref()
        && let Some(entry) = pkg.entry.as_ref()
    {
        let id = entry_ref_id(entry);
        let row = entry_row(entry);
        let mut st = CORE.write();
        if !st.threads.iter().any(|t| t.id == id) {
            st.threads.push(ThreadRow { id: id.clone() });
        }
        let rows = st.logs.entry(id).or_default();
        // upsert by digest: a re-push (roster change) REPLACES the
        // row; a first sight appends
        match rows.iter().position(|r| r.ref_id == row.ref_id) {
            Some(i) => rows[i] = row,
            None => rows.push(row),
        }
    }
}

// ---- boot ----

static BOOT: std::sync::OnceLock<()> = std::sync::OnceLock::new();
static OPS: std::sync::OnceLock<tokio::sync::mpsc::UnboundedSender<Req>> =
    std::sync::OnceLock::new();

/// The deploy root: the first `script[src]` that carries dx's
/// `/assets/` prefix, cut just before it. The dx entry script is the
/// one stable anchor of the build's base_path (Dioxus.toml) — relative
/// srcs (`./pwa.js`) and scripts without the prefix are skipped.
#[cfg(target_arch = "wasm32")]
fn deploy_root(doc: &web_sys::Document) -> Option<String> {
    let scripts = doc.query_selector_all("script[src]").ok()?;
    for i in 0..scripts.length() {
        let Some(el) = scripts
            .get(i)
            .and_then(|n| n.dyn_ref::<web_sys::Element>().cloned())
        else {
            continue;
        };
        let Some(src) = el.get_attribute("src") else {
            continue;
        };
        if let Some(cut) = src.find("/assets/") {
            return Some(src[..cut].to_string());
        }
    }
    None
}

/// Boot the client once (the App calls this in an effect).
pub fn boot() {
    if BOOT.set(()).is_err() {
        return;
    }
    spawn_local(async move {
        if let Err(e) = run_client().await {
            CORE.write().status = Status::Failed(e);
        }
    });
}

#[cfg(target_arch = "wasm32")]
async fn run_client() -> Result<(), String> {
    // the UI thread's panic hook: a release-wasm panic = a bare trap
    // with an anonymous JS exception — surface the panic message
    std::panic::set_hook(Box::new(|info| {
        web_sys::console::error_1(&format!("[panic] {info}").into());
    }));
    // THE DEPLOY ROOT, from a stable anchor: the dx entry script's
    // src carries the build's base_path prefix (Dioxus.toml
    // `base_path`, baked into <script src="{base}/assets/..."> at
    // build time). The DOCUMENT base cannot serve: without a <base>
    // tag, base_uri() is the document URL itself, and on a router
    // deep-link (/tafy/thread/new) that IS the deep path — the
    // worker URL resolved to /tafy/thread/new/tafy-core/... = 404
    // and the boot died silently at 'not connected' (the deep-link
    // boot bug, found in the 2026-11-11 live-relay round).
    let doc = web_sys::window()
        .ok_or("no window")?
        .document()
        .ok_or("no document")?;
    let base = deploy_root(&doc).unwrap_or_else(|| {
        // the old derivation as the fallback (works at the root page)
        doc.base_uri()
            .ok()
            .flatten()
            .unwrap_or_default()
            .trim_end_matches('/')
            .to_string()
    });
    let url = format!("{base}/tafy-core/tafy-worker.js");

    let opts = web_sys::WorkerOptions::new();
    opts.set_type(web_sys::WorkerType::Module);
    let worker = web_sys::Worker::new_with_options(&url, &opts)
        .map_err(|e| format!("worker spawn failed: {e:?}"))?;

    // THE READY GATE: a message posted into a module worker before
    // its script's evaluation completes can be DROPPED (observed on
    // chromium: an early port transfer never arrives). The worker
    // posts "worker:ready" once its scope handler is live — wait for
    // it BEFORE transferring the port.
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<()>();
    let ready_tx = std::cell::RefCell::new(Some(ready_tx));
    let on_worker_msg = Closure::wrap(Box::new(move |e: web_sys::MessageEvent| {
        let text = e.data().as_string().unwrap_or_default();
        if text.contains("worker:ready") {
            if let Some(tx) = ready_tx.borrow_mut().take() {
                let _ = tx.send(());
            }
        } else if !text.is_empty() {
            // the worker's boot breadcrumbs surface on the page console
            web_sys::console::log_1(&format!("[worker] {text}").into());
        }
    }) as Box<dyn FnMut(web_sys::MessageEvent)>);
    worker.set_onmessage(Some(on_worker_msg.as_ref().unchecked_ref()));
    on_worker_msg.forget();
    // NO tokio::time here: the wasm executor has no tokio timer
    // driver — a time::timeout panics the task silently. The worker
    // always posts ready; if it died, the transfer never happens and
    // the boot stays visibly "Booting" (the honest failure face).
    match ready_rx.await {
        Ok(()) => trace("worker ready; transferring the port"),
        Err(_) => return Err("worker dropped its ready signal".to_string()),
    }

    // the bridges: TWO channels transfer in ONE message — [i2c, ctrl]
    // (the worker's onmessage collects ports in order). The i2c port
    // carries the framed protocol; the control port carries the
    // boot/connect JSON and the conn-state faces back.
    let channel = web_sys::MessageChannel::new().map_err(|_| "no MessageChannel".to_string())?;
    let ctrl_channel =
        web_sys::MessageChannel::new().map_err(|_| "no MessageChannel".to_string())?;
    let port = channel.port1();
    let ctrl_port = ctrl_channel.port1();
    park_ctrl_port(ctrl_port.clone());
    let transfer = js_sys::Array::new();
    transfer.push(&channel.port2());
    transfer.push(&ctrl_channel.port2());
    worker
        .post_message_with_transfer(&JsValue::NULL, &transfer)
        .map_err(|e| format!("port transfer failed: {e:?}"))?;

    // the boot face: the relay URL rides FIRST on the control port
    // (the worker's core-sender shape depends on it)
    let relay = relay_saved();
    let boot_face = serde_json::json!({ "t": "boot", "relay": relay });
    ctrl_port
        .post_message(&JsValue::from_str(&boot_face.to_string()))
        .map_err(|e| format!("boot face failed: {e:?}"))?;

    // control faces back: the worker's conn statements -> the state
    let (conn_tx, mut conn_rx) = tokio::sync::mpsc::unbounded_channel::<Conn>();
    let ctrl_onmessage = Closure::wrap(Box::new(move |e: web_sys::MessageEvent| {
        let raw = e.data().as_string().unwrap_or_default();
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
            // not a conn face (e.g. the storage diag reply) -> console
            trace(&format!("[ctrl] {raw}"));
            return;
        };
        let state = v.get("state").and_then(|s| s.as_str()).unwrap_or("");
        let detail = v.get("detail").and_then(|s| s.as_str()).unwrap_or("");
        let conn = match state {
            "connecting" => Conn::Connecting,
            "connected" => Conn::Connected,
            "disconnected" => Conn::Disconnected,
            "error" => Conn::Error(detail.to_string()),
            _ => return,
        };
        let _ = conn_tx.send(conn);
    }) as Box<dyn FnMut(web_sys::MessageEvent)>);
    ctrl_port.set_onmessage(Some(ctrl_onmessage.as_ref().unchecked_ref()));
    ctrl_onmessage.forget();
    ctrl_port.start();
    spawn_local(async move {
        while let Some(conn) = conn_rx.recv().await {
            CORE.write().conn = conn;
        }
    });

    // inbound: onmessage -> the bridge channel (the spike-proven
    // JS-event wake path: tokio channels wake fine on the wasm
    // executor)
    trace("channel up, wiring onmessage");
    let _ = &ctrl_port; // the control port lives for the page's life
    let (in_tx, in_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    let onmessage = Closure::wrap(Box::new(move |e: web_sys::MessageEvent| {
        let bytes = js_sys::Uint8Array::new(&e.data()).to_vec();
        let head: Vec<String> = bytes.iter().take(12).map(|b| format!("{b:02x}")).collect();
        web_sys::console::log_1(
            &format!(
                "[client] port1 frame in ({}B): {}",
                bytes.len(),
                head.join(" ")
            )
            .into(),
        );
        let _ = in_tx.send(bytes);
    }) as Box<dyn FnMut(web_sys::MessageEvent)>);
    port.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    onmessage.forget();

    // outbound: bytes -> the port (the worker deframes its side)
    let (out_tx, mut out_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    spawn_local(async move {
        while let Some(bytes) = out_rx.recv().await {
            web_sys::console::log_1(&format!("[client] posting {}B to port1", bytes.len()).into());
            let _ = port.post_message(&js_sys::Uint8Array::from(bytes.as_slice()));
        }
    });

    trace("io task up, sending handshake");
    let (op_tx, op_rx) = tokio::sync::mpsc::unbounded_channel::<Req>();
    let _ = OPS.set(op_tx);
    spawn_local(
        CoreIo {
            inbound: in_rx,
            ops: op_rx,
            tx: out_tx,
        }
        .run(),
    );

    // ---- seed the state through the same single-flight queue ----
    if let Ok(frames) = request(
        incoming(M::SelfGet(zconned_stdio_types::SelfGet {})),
        Expect::One(AnswerKind::SelfPush),
    )
    .await
    {
        for f in frames {
            if let Some(zconned_stdio_types::outgoing_frame::Frame::SelfPush(p)) = f.frame {
                use base64::Engine as _;
                // the self anchor joins against meta.from in the
                // author face (B6 §4) — decode the card back to Peer
                let self_anchor = base64::engine::general_purpose::STANDARD
                    .decode(p.card.as_bytes())
                    .ok()
                    .and_then(|b| zconned_stdio_types::Peer::decode(&b[..]).ok())
                    .and_then(|peer| anchor_hex_of_peer(&peer));
                let mut st = CORE.write();
                st.self_card = Some(p.card);
                st.self_anchor = self_anchor;
            }
        }
    }
    if let Ok(frames) = request(
        incoming(M::MsgThreadsGet(zconned_stdio_types::MsgThreadsGet {})),
        Expect::One(AnswerKind::MsgThreadsPush),
    )
    .await
    {
        for f in frames {
            if let Some(zconned_stdio_types::outgoing_frame::Frame::MsgThreadsPush(p)) = f.frame {
                CORE.write().threads = p
                    .threads
                    .iter()
                    .map(thread_ref_id)
                    .filter(|s| !s.is_empty())
                    .map(|id| ThreadRow { id })
                    .collect();
            }
        }
    }
    if let Ok(frames) = request(
        incoming(M::PeersGet(zconned_stdio_types::PeersGet {})),
        Expect::One(AnswerKind::PeersPush),
    )
    .await
    {
        for f in frames {
            if let Some(zconned_stdio_types::outgoing_frame::Frame::PeersPush(p)) = f.frame {
                set_peers(&mut CORE.write(), &p.peers);
            }
        }
    }
    trace(&format!("seeded; threads={}", CORE.read().threads.len()));
    if CORE.read().status == Status::Booting {
        CORE.write().status = Status::Ready;
    }
    // the reactive boot gate: mount-time fetches (the thread page's
    // log ask) subscribe to this flag and re-fire when it flips —
    // the deep-link mount races the boot otherwise (the 'core not
    // booted' Err used to swallow the fetch silently)
    CORE.write().booted = true;
    // the relay visit: auto-connect when the setting exists (107's
    // posture — the connection IS the visit; no visit = queued mail)
    // TEMP diag: the post-reload walk's disk truth (removed after the hunt)
    storage_diag();
    let relay = relay_saved();
    if !relay.trim().is_empty() {
        relay_connect(relay);
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
async fn run_client() -> Result<(), String> {
    Err("the client runs on wasm only".to_string())
}

// ---- op frame builders (the oneof is a nested enum in prost) ----

fn incoming(m: zconned_stdio_types::incoming_frame::Method) -> IncomingFrame {
    IncomingFrame { method: Some(m) }
}

use zconned_stdio_types::incoming_frame::Method as M;

async fn request(frame: IncomingFrame, expect: Expect) -> Result<Vec<OutgoingFrame>, String> {
    let Some(tx) = OPS.get() else {
        return Err("core not booted".to_string());
    };
    let (r_tx, r_rx) = tokio::sync::oneshot::channel();
    tx.send(Req {
        frame,
        expect,
        reply: r_tx,
    })
    .map_err(|_| "core io gone".to_string())?;
    // TEMP io probe (retire list pending): the request left; if the
    // io task never resolves it we need to see WHICH side died
    let r = r_rx.await;
    match r.as_ref() {
        Err(_) => trace("[io] request DIED: the io task dropped the reply"),
        Ok(inner) => match inner {
            Ok(frames) => trace(&format!("[io] request resolved: {} frame(s)", frames.len())),
            Err(e) => trace(&format!("[io] request resolved Err: {e}")),
        },
    }
    r.map_err(|_| "core io dropped".to_string())?
}

/// self_get -> the identity card, verbatim (the core returns
/// Error{no_identity} only when the host passed no card — the worker
/// always creates the keyring, so this answers). Seeded at boot; a
/// re-ask belongs to a future refresh affordance.
#[allow(dead_code)]
pub async fn self_card() -> Result<String, String> {
    let frames = request(
        incoming(M::SelfGet(zconned_stdio_types::SelfGet {})),
        Expect::One(AnswerKind::SelfPush),
    )
    .await?;
    for f in frames {
        if let Some(zconned_stdio_types::outgoing_frame::Frame::SelfPush(p)) = f.frame {
            return Ok(p.card);
        }
    }
    Err("self_get answered without a card".to_string())
}

/// peers_get -> the resting send-target set (seeded at boot; also
/// kept as the op API alongside its statement route).
#[allow(dead_code)]
pub async fn peers() -> Result<Vec<PeerRow>, String> {
    let frames = request(
        incoming(M::PeersGet(zconned_stdio_types::PeersGet {})),
        Expect::One(AnswerKind::PeersPush),
    )
    .await?;
    for f in frames {
        if let Some(zconned_stdio_types::outgoing_frame::Frame::PeersPush(p)) = f.frame {
            let mut st = CORE.write();
            set_peers(&mut st, &p.peers);
            return Ok(st.peers.clone());
        }
    }
    Err("peers_get answered without a set".to_string())
}

/// peer_set{card} from a pasted card: the canonical single-line
/// base64 goes to the core VERBATIM — the core decodes it, verifies
/// the signature and derives the anchor (the interface never parses
/// key material; 107's ruled UX, now the proto's card field).
/// The receipt = the whole resting set (PeersPush, dual-role).
pub async fn add_peer(card_b64: &str) -> Result<Vec<PeerRow>, String> {
    let frames = request(
        incoming(M::PeerSet(zconned_stdio_types::PeerSet {
            peer: None,
            card: Some(card_b64.trim().to_string()),
        })),
        Expect::One(AnswerKind::PeersPush),
    )
    .await?;
    for f in frames {
        if let Some(zconned_stdio_types::outgoing_frame::Frame::PeersPush(p)) = f.frame {
            let mut st = CORE.write();
            set_peers(&mut st, &p.peers);
            return Ok(st.peers.clone());
        }
    }
    Err("peer_set answered without a set".to_string())
}

/// msg_threads_get -> the thread list (seeded at boot; kept as the
/// op API alongside its statement route).
#[allow(dead_code)]
pub async fn threads() -> Result<Vec<ThreadRow>, String> {
    let frames = request(
        incoming(M::MsgThreadsGet(zconned_stdio_types::MsgThreadsGet {})),
        Expect::One(AnswerKind::MsgThreadsPush),
    )
    .await?;
    let mut rows = Vec::new();
    for f in frames {
        if let Some(zconned_stdio_types::outgoing_frame::Frame::MsgThreadsPush(p)) = f.frame {
            rows.extend(
                p.threads
                    .iter()
                    .map(thread_ref_id)
                    .filter(|s| !s.is_empty())
                    .map(|id| ThreadRow { id }),
            );
        }
    }
    Ok(rows)
}

/// msg_threads_log_get{thread} -> the log (merged into the cache).
pub async fn thread_log(thread_hex: &str) -> Result<Vec<LogRow>, String> {
    let thread = parse_ref_id(thread_hex).ok_or_else(|| format!("bad thread ref {thread_hex}"))?;
    let frames = request(
        incoming(M::MsgThreadsLogGet(zconned_stdio_types::MsgThreadsLogGet {
            thread: Some(thread),
        })),
        Expect::One(AnswerKind::MsgThreadsLogPush),
    )
    .await?;
    let mut rows = Vec::new();
    for f in frames {
        if let Some(zconned_stdio_types::outgoing_frame::Frame::MsgThreadsLogPush(p)) = f.frame {
            // B7 E6: the log answer = one package per message; the
            // roster rides each package (local.signer)
            rows.extend(
                p.packages
                    .iter()
                    .filter_map(|pkg| pkg.entry.as_ref())
                    .map(entry_row),
            );
        }
    }
    Ok(rows)
}

/// Load a thread's log into the cache (the ThreadPage's mount ask).
pub fn load_thread(id: String) {
    spawn_local(async move {
        match thread_log(&id).await {
            Ok(rows) => {
                CORE.write().logs.insert(id, rows);
            }
            // TEMP diagnostic (retire list pending): the silent-swallow
            // shape left the thread page at 'Stream pending load…'
            // forever with no face — surface the Err verbatim.
            Err(e) => {
                // TEMP diagnostic (retire list pending): the silent-swallow
                // shape left the thread page at 'Stream pending load…'
                // forever with no face — surface the Err verbatim.
                #[cfg(target_arch = "wasm32")]
                web_sys::console::error_1(&format!("[thread_log] {id}: {e}").into());
                #[cfg(not(target_arch = "wasm32"))]
                eprintln!("[thread_log] {id}: {e}");
            }
        }
    });
}

/// The compose-commit flow, ONE single-flight chain: cursor ->
/// draft content -> commit; returns the pair (the committed entry's
/// thread id, the resting draft). thread_hex = None starts a fresh
/// chain (the core generates the genesis parent); Some(id) commits
/// onto the thread's leaf (day-one linear frontier).
pub async fn commit(thread_hex: Option<&str>, text: &str) -> Result<String, String> {
    trace(&format!(
        "[commit] enter: thread={thread_hex:?} len={}",
        text.len()
    ));
    // 1. the cursor
    let cursor = match thread_hex {
        None => zconned_stdio_types::MsgCursorSet {
            at: vec![],
            fresh: Some(zconned_stdio_types::Fresh {}),
        },
        Some(id) => {
            let leaf = CORE
                .read()
                .logs
                .get(id)
                .and_then(|rows| rows.last().map(|r| r.ref_id.clone()))
                .ok_or_else(|| "thread log not loaded — cannot position the cursor".to_string())?;
            let at = parse_ref_id(&leaf).ok_or_else(|| format!("bad leaf ref {leaf}"))?;
            zconned_stdio_types::MsgCursorSet {
                at: vec![at],
                fresh: None,
            }
        }
    };
    request(
        incoming(M::MsgCursorSet(cursor)),
        Expect::One(AnswerKind::MsgCursorPush),
    )
    .await?;
    // 2. the draft content (day-one kind text/plain; no vendor claims)
    request(
        incoming(M::MsgDraftContentSet(
            zconned_stdio_types::MsgDraftContentSet {
                kind: "text/plain".to_string(),
                data: text.as_bytes().to_vec(),
            },
        )),
        Expect::One(AnswerKind::MsgDraftPush),
    )
    .await?;
    // 3. the commit: the core stamps the registry claims (ts) and
    // answers the B7 pair (msg_package_push with the COMPLETE
    // roster + the resting draft)
    let frames = request(
        incoming(M::MsgDraftCommit(zconned_stdio_types::MsgDraftCommit {})),
        Expect::Commit,
    )
    .await?;
    // the pair: package (the new tip + its roster) + the resting draft
    let mut new_id = None;
    for f in frames {
        if let Some(zconned_stdio_types::outgoing_frame::Frame::MsgPackagePush(p)) = f.frame
            && let Some(entry) = p.package.as_ref().and_then(|pkg| pkg.entry.as_ref())
        {
            let id = entry_ref_id(entry);
            let row = entry_row(entry);
            // THE THREAD id: for a FRESH commit the core generates the
            // genesis parent (the cursor's fresh) — the thread is the
            // entry's prev[0], NOT the entry's own ref (the message's
            // digest is its own anchor; pulling the log by it pulls a
            // thread that does not exist). Existing threads keep the
            // caller's id.
            let thread = thread_hex.map(|s| s.to_string()).unwrap_or_else(|| {
                entry
                    .prev
                    .first()
                    .map(|r| ref_id_of(entry.ver_major, entry.ver_minor, &r.algo, &r.digest))
                    .unwrap_or_else(|| id.clone())
            });
            let mut st = CORE.write();
            if !st.threads.iter().any(|t| t.id == thread) {
                st.threads.push(ThreadRow { id: thread.clone() });
            }
            let rows = st.logs.entry(thread.clone()).or_default();
            if rows.last() != Some(&row) {
                rows.push(row);
            }
            // the RETURNED id = the thread (the caller navigates to it;
            // the message's own digest is NOT addressable as a thread)
            new_id = Some(thread);
        }
    }
    new_id.ok_or_else(|| "commit answered without an entry".to_string())
}

// ---- the relay control face (batch 3) ----

/// The relay URL reader (the UI's own setting; localStorage).
pub fn relay_saved() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()
            .and_then(|w| w.local_storage().ok().flatten())
            .and_then(|s| s.get_item("tafy.relay_url").ok().flatten())
            .unwrap_or_default()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        String::new()
    }
}

/// The control port, parked at boot (the worker's connect /
/// disconnect faces ride it; !Send — single-threaded page, fine).
#[cfg(target_arch = "wasm32")]
static CTRL_PORT: std::sync::OnceLock<web_sys::MessagePort> = std::sync::OnceLock::new();

/// Park the control port (run_client calls this once, at boot).
#[cfg(target_arch = "wasm32")]
fn park_ctrl_port(p: web_sys::MessagePort) {
    let _ = CTRL_PORT.set(p);
}

/// Drive a relay visit: the worker repoints + runs the library's
/// visit routine (claim, flip, pool, plane pumps, cursors, re-push).
pub fn relay_connect(url: String) {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(port) = CTRL_PORT.get() {
            let face = serde_json::json!({ "t": "connect", "relay": url });
            let _ = port.post_message(&JsValue::from_str(&face.to_string()));
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = url;
    }
}

/// TEMP diag: the worker's storage mirror + the last persist failure.
pub fn storage_diag() {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(port) = CTRL_PORT.get() {
            trace("storage_diag: posting the diag face");
            let _ = port.post_message(&JsValue::from_str(r#"{"t":"diag"}"#));
        } else {
            trace("storage_diag: NO CTRL PORT");
        }
    }
}

/// End the current visit (the pumps end; queued mail stays).
pub fn relay_disconnect() {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(port) = CTRL_PORT.get() {
            let _ = port.post_message(&JsValue::from_str(r#"{"t":"disconnect"}"#));
        }
    }
}

#[cfg(test)]
mod ts_claim_tests {
    use super::chrono_millis;
    #[test]
    fn rfc3339_utc_parses() {
        // cross-checked against an independent epoch calculator
        assert_eq!(chrono_millis("2026-11-11T08:47:00.123Z"), Some(1_794_386_820_123));
        assert_eq!(chrono_millis("2026-11-11T08:47:00Z"), Some(1_794_386_820_000));
        assert_eq!(chrono_millis("2024-02-29T00:00:00Z"), Some(1_709_164_800_000));
        assert_eq!(chrono_millis("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(chrono_millis("2026-11-11T08:47:00+01:00"), None);
        assert_eq!(chrono_millis("2026-02-29T00:00:00Z"), None);
        assert_eq!(chrono_millis("garbage"), None);
    }
}
