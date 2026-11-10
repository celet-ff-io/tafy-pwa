// The UI-side i2c client: the PWA speaks the frame protocol to the
// wasm core through a MessageChannel bridged inside the worker.
//
// Wire discipline (protocol.md § The wire): frame #1 = the handshake
// (both directions; the core's ack = a raw Handshake, not an
// OutgoingFrame — positional typing); frames #2..n = bodies
// (IncomingFrame UI->core, OutgoingFrame core->UI). The client is
// SINGLE-FLIGHT (the stdio grammar is one op at a time): answers pair
// with the pending request; statements route to the broadcast state.
// A commit returns TWO frames — msg_added + the resting draft —
// returned by the API as one pair.
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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CoreState {
    pub status: Status,
    pub self_card: Option<String>,
    pub threads: Vec<ThreadRow>,
    /// thread id -> fetched log (oldest first as the core states it)
    pub logs: HashMap<String, Vec<LogRow>>,
    /// the last Error frame, surfaced verbatim in the UI
    pub error: Option<String>,
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

fn entry_row(entry: &zconned_stdio_types::Entry) -> LogRow {
    // content = the resolved slot's value (kind text/plain day one)
    let text = entry
        .content
        .as_ref()
        .and_then(|c| c.value.as_ref())
        .map(|v| String::from_utf8_lossy(&v.data).into_owned())
        .unwrap_or_default();
    let ts_ms = entry
        .meta
        .as_ref()
        .and_then(|m| m.ts.as_ref())
        .map(|t| (t.seconds.max(0) as u64) * 1000 + (t.nanos / 1_000_000) as u64)
        .unwrap_or(0);
    LogRow {
        ref_id: entry_ref_id(entry),
        // day one the only committer is the self core (peers arrive
        // with the carrier leg, batch 3)
        author: "Me".to_string(),
        text,
        ts_ms,
    }
}

// ---- the client ----

#[derive(Debug)]
enum Expect {
    /// exactly one answer frame of this shape
    One(AnswerKind),
    /// the commit pair: msg_added + the resting draft (MsgDraftPush)
    Commit,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum AnswerKind {
    MsgAdded,
    MsgCursorPush,
    MsgDraftPush,
    MsgThreadsPush,
    MsgThreadsLogPush,
    SelfPush,
}

fn answer_kind(f: &OutgoingFrame) -> Option<AnswerKind> {
    use zconned_stdio_types::outgoing_frame::Frame as F;
    match f.frame.as_ref()? {
        F::MsgAdded(_) => Some(AnswerKind::MsgAdded),
        F::MsgCursorPush(_) => Some(AnswerKind::MsgCursorPush),
        F::MsgDraftPush(_) => Some(AnswerKind::MsgDraftPush),
        F::MsgThreadsPush(_) => Some(AnswerKind::MsgThreadsPush),
        F::MsgThreadsLogPush(_) => Some(AnswerKind::MsgThreadsLogPush),
        F::SelfPush(_) => Some(AnswerKind::SelfPush),
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
                        None => route_statement(f),
                        Some((expect, reply, mut acc)) => {
                            let matches = match (&expect, answer_kind(&f)) {
                                (Expect::One(want), Some(got)) => got == *want,
                                (Expect::Commit, Some(AnswerKind::MsgAdded)) => acc.is_empty(),
                                (Expect::Commit, Some(AnswerKind::MsgDraftPush)) => !acc.is_empty(),
                                _ => false,
                            };
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

/// Unsolicited frames: the statement bus. Day one = commit receipts
/// for ops whose reply consumer is gone + batch-3 arrivals later.
fn route_statement(f: OutgoingFrame) {
    use zconned_stdio_types::outgoing_frame::Frame as F;
    if let F::MsgAdded(a) = f.frame.as_ref().unwrap()
        && let Some(entry) = a.entry.as_ref()
    {
        let id = entry_ref_id(entry);
        let row = entry_row(entry);
        let mut st = CORE.write();
        if !st.threads.iter().any(|t| t.id == id) {
            st.threads.push(ThreadRow { id: id.clone() });
        }
        let rows = st.logs.entry(id).or_default();
        let dup = rows.last() == Some(&row);
        if !dup {
            rows.push(row);
        }
    }
}

// ---- boot ----

static BOOT: std::sync::OnceLock<()> = std::sync::OnceLock::new();
static OPS: std::sync::OnceLock<tokio::sync::mpsc::UnboundedSender<Req>> =
    std::sync::OnceLock::new();

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
    // the worker URL resolves against the DOCUMENT base (the page
    // lives under /tafy/ on gh-pages — a router path must not bend
    // the relative resolution)
    let doc = web_sys::window()
        .ok_or("no window")?
        .document()
        .ok_or("no document")?;
    let base = doc
        .base_uri()
        .ok()
        .flatten()
        .ok_or("no base uri")?
        .trim_end_matches('/')
        .to_string();
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

    // the bridge: port2 travels INTO the worker; port1 stays here
    let channel = web_sys::MessageChannel::new().map_err(|_| "no MessageChannel".to_string())?;
    let port = channel.port1();
    let transfer = js_sys::Array::new();
    transfer.push(&channel.port2());
    worker
        .post_message_with_transfer(&JsValue::NULL, &transfer)
        .map_err(|e| format!("port transfer failed: {e:?}"))?;

    // inbound: onmessage -> the bridge channel (the spike-proven
    // JS-event wake path: tokio channels wake fine on the wasm
    // executor)
    trace("channel up, wiring onmessage");
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
                CORE.write().self_card = Some(p.card);
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
    trace(&format!("seeded; threads={}", CORE.read().threads.len()));
    if CORE.read().status == Status::Booting {
        CORE.write().status = Status::Ready;
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
    r_rx.await.map_err(|_| "core io dropped".to_string())?
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
            rows.extend(p.entries.iter().map(entry_row));
        }
    }
    Ok(rows)
}

/// Load a thread's log into the cache (the ThreadPage's mount ask).
pub fn load_thread(id: String) {
    spawn_local(async move {
        if let Ok(rows) = thread_log(&id).await {
            CORE.write().logs.insert(id, rows);
        }
    });
}

/// The compose-commit flow, ONE single-flight chain: cursor ->
/// draft content -> commit; returns the pair (the committed entry's
/// thread id, the resting draft). thread_hex = None starts a fresh
/// chain (the core generates the genesis parent); Some(id) commits
/// onto the thread's leaf (day-one linear frontier).
pub async fn commit(thread_hex: Option<&str>, text: &str) -> Result<String, String> {
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
    // answers the pair (msg_added + the resting draft)
    let frames = request(
        incoming(M::MsgDraftCommit(zconned_stdio_types::MsgDraftCommit {})),
        Expect::Commit,
    )
    .await?;
    // the pair: entry (the new tip) + the resting draft
    let mut new_id = None;
    for f in frames {
        if let Some(zconned_stdio_types::outgoing_frame::Frame::MsgAdded(a)) = f.frame
            && let Some(entry) = a.entry.as_ref()
        {
            let id = entry_ref_id(entry);
            let row = entry_row(entry);
            let thread = thread_hex
                .map(|s| s.to_string())
                .unwrap_or_else(|| id.clone());
            let mut st = CORE.write();
            if !st.threads.iter().any(|t| t.id == thread) {
                st.threads.push(ThreadRow { id: thread.clone() });
            }
            let rows = st.logs.entry(thread).or_default();
            if rows.last() != Some(&row) {
                rows.push(row);
            }
            new_id = Some(id);
        }
    }
    new_id.ok_or_else(|| "commit answered without an entry".to_string())
}
