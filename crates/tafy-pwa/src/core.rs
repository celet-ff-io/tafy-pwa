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

/// The ts claim kind (the registry kind the core ATTESTS at commit:
/// the interface supplies an empty claim, the core overwrites the
/// value with its own RFC3339 commit time — never interface-supplied).
const CLAIM_KIND_TS: &str = "application/vnd.zconn.ts";

#[derive(Clone, Debug, PartialEq)]
pub struct LogRow {
    /// the entry's own MsgRef in id form (the cursor's `at` leaf)
    pub ref_id: String,
    pub author: String,
    pub text: String,
    /// unix ms (the row's local record time)
    pub ts_ms: u64,
    /// THE TIMESTAMP MESSAGE (107's timestamp discipline): an empty
    /// text/plain entry carrying the core-attested ts claim — the row
    /// renders as the standalone dim ts line (no author, no bubble:
    /// the row IS the timestamp)
    pub stamp: bool,
    /// the author's mailbox anchor (meta.from), full lowercase hex;
    /// "" = the committing core had no mail identity (the dash face)
    pub from_hex: String,
    /// THE VERIFIED SIGNER ROSTER (the package's local.signer): the
    /// anchors whose record signatures the core verified and landed.
    /// THE REMARK/UNTRUSTED JOINS READ THIS: the author-display face
    /// and the untrusted verdict (the principal, 2026-11-12: a remark
    /// names EVERY identity in a message — the author and every
    /// signer — and a received message whose author signed nothing
    /// shows the untrusted face).
    pub signers: Vec<SignerRow>,
}

/// One verified signer (display facts only; the name face is looked
/// up live from the registry so a remark edit re-faces every row).
#[derive(Clone, Debug, PartialEq)]
pub struct SignerRow {
    pub anchor_hex: String,
}

/// One identity-registry row (B7 c2): an observed anchor + its local
/// remark. The remark is THIS DEVICE's annotation (the author slot of
/// outgoing packages carries only the SELF remark); none = unset —
/// the anchor face shows instead.
#[derive(Clone, Debug, PartialEq)]
pub struct IdRow {
    pub anchor_hex: String,
    pub remark: Option<String>,
}

/// One contact (a Peer record's display face; 107's W2, web-adapted).
#[derive(Clone, Debug, PartialEq)]
pub struct PeerRow {
    /// the display fingerprint: the SHA3-256 digest's first 8 bytes,
    /// grouped (F1, via zconned_crypto::Fingerprint::short)
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
    /// the identity registry (B7 c2): observed anchors + the local
    /// remarks (the wholesale IdentitiesPush view; boot-seeded +
    /// re-asked by the Me page, re-merged on every set/reset receipt)
    pub identities: Vec<IdRow>,
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
    if b.len() < 20
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
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
    (secs >= 0).then(|| (secs as u64) * 1000 + millis)
}

fn entry_row(
    entry: &zconned_stdio_types::Entry,
    local: Option<&zconned_stdio_types::Local>,
) -> LogRow {
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
        .find(|c| c.kind == CLAIM_KIND_TS)
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
    let from_hex = entry
        .meta
        .as_ref()
        .map(|m| m.from.iter().map(|b| format!("{b:02x}")).collect::<String>())
        .unwrap_or_default();
    LogRow {
        ref_id: entry_ref_id(entry),
        // B6 §4: the author = the store's observation of meta.from,
        // joined against the PeerBook (the self anchor renders Me;
        // the REMARK-PREFERRED face is applied at render —
        // author_display — so a registry edit re-faces every row).
        author: author_face(
            entry
                .meta
                .as_ref()
                .map(|m| m.from.as_slice())
                .unwrap_or(&[]),
        ),
        text: text.clone(),
        ts_ms,
        stamp: text.is_empty()
            && entry.claims.iter().any(|c| {
                c.kind == CLAIM_KIND_TS && c.value.as_ref().is_some_and(|v| !v.data.is_empty())
            }),
        from_hex,
        // the package's verified signer roster (B7: it rides EVERY
        // package — the commit receipt carries the complete set incl.
        // the self-signature)
        signers: local
            .map(|l| {
                l.signer
                    .iter()
                    .map(|s| SignerRow {
                        anchor_hex: s.anchor.iter().map(|b| format!("{b:02x}")).collect(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
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
    MsgThreadLogPush,
    SelfPush,
    PeersPush,
    IdentitiesPush,
}

fn answer_kind(f: &OutgoingFrame) -> Option<AnswerKind> {
    use zconned_stdio_types::outgoing_frame::Frame as F;
    match f.frame.as_ref()? {
        F::MsgPackagePush(_) => Some(AnswerKind::MsgPackagePush),
        F::MsgCursorPush(_) => Some(AnswerKind::MsgCursorPush),
        F::MsgDraftPush(_) => Some(AnswerKind::MsgDraftPush),
        F::MsgThreadsPush(_) => Some(AnswerKind::MsgThreadsPush),
        F::MsgThreadLogPush(_) => Some(AnswerKind::MsgThreadLogPush),
        F::SelfPush(_) => Some(AnswerKind::SelfPush),
        F::PeersPush(_) => Some(AnswerKind::PeersPush),
        F::IdentitiesPush(_) => Some(AnswerKind::IdentitiesPush),
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
    // F1 (106's fingerprint-notation ruling, dev 91d0aad): the display
    // fingerprint = the SHA3-256 DIGEST's first 8 bytes, grouped via
    // the canon helper Fingerprint::short(). The raw sign_pub prefix
    // (the old take(8) derivation) is retired — a raw-key prefix and
    // a digest prefix are never equal, so the two UIs could never be
    // cross-checked. anchor_hex (the full digest, the machine join
    // key) stays as-is.
    let fp = fp_of_peer(peer).unwrap_or_default();
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

/// The F1 short display fingerprint: the digest's first 8 bytes via
/// the canon helper (the grouping shape is 106's standard verbatim).
pub(crate) fn fp_of_peer(peer: &zconned_stdio_types::Peer) -> Option<String> {
    let mut sign = [0u8; 32];
    if peer.sign_pub.len() != 32 {
        return None;
    }
    sign.copy_from_slice(&peer.sign_pub);
    Some(zconned_crypto::Fingerprint::of(&sign).short())
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

/// The identity registry's wholesale merge (B7 c2): the anchor set +
/// the remarks, verbatim from the core (it sorts; we keep order).
fn set_identities(st: &mut CoreState, ids: &[zconned_stdio_types::Identity]) {
    st.identities = ids
        .iter()
        .map(|i| IdRow {
            anchor_hex: i.anchor.iter().map(|b| format!("{b:02x}")).collect(),
            remark: i.remark.clone().filter(|r| !r.is_empty()),
        })
        .collect();
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
    // the identity registry's wholesale view (B7 c2): the set/reset
    // receipt can surface here when the reply consumer is gone —
    // same merge either way
    if let F::IdentitiesPush(p) = f.frame.as_ref().unwrap() {
        set_identities(&mut CORE.write(), &p.identities);
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
    //
    // THE THREAD: the arrival notice carries NO thread ref (the i2c
    // arrival shape; the dumb-interface rule — the core reports
    // facts, the interface re-pulls on demand). Derive what the
    // chain gives: a prev[0] matching a known row places the
    // arrival in that row's thread; anything else (a new thread or
    // a chain gap) lands via a log re-pull — the store already has
    // the landed facts, the pull is a request per loaded thread.
    if let F::MsgPackagePush(p) = f.frame.as_ref().unwrap()
        && let Some(pkg) = p.package.as_ref()
        && let Some(entry) = pkg.entry.as_ref()
    {
        let row = entry_row(entry, pkg.local.as_ref());
        let prev_ref = entry
            .prev
            .first()
            .map(|r| ref_id_of(entry.ver_major, entry.ver_minor, &r.algo, &r.digest));
        let mut placed = false;
        if let Some(prev) = &prev_ref {
            let mut st = CORE.write();
            for rows in st.logs.values_mut() {
                if !rows.iter().any(|r| &r.ref_id == prev) {
                    continue;
                }
                match rows.iter().position(|r| r.ref_id == row.ref_id) {
                    Some(i) => rows[i] = row.clone(),
                    None => rows.push(row.clone()),
                }
                placed = true;
                break;
            }
        }
        if !placed {
            let ids: Vec<String> = CORE.read().logs.keys().cloned().collect();
            spawn_local(async move {
                for id in ids {
                    if let Ok(rows) = thread_log(&id).await {
                        CORE.write().logs.insert(id, rows);
                    }
                }
                if let Ok(ts) = threads().await {
                    CORE.write().threads = ts;
                }
            });
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
        } else if text.contains("worker:alive") {
            // THE HEARTBEAT'S ECHO: the worker's 5s broadcast stamps
            // the clock — the heartbeat task judges liveness off this
            // (106's ruling: under panic=abort a dead pump traps the
            // module; two missed answers = a dead worker).
            HEARTBEAT_PONG.store(
                js_sys::Date::now() as u64,
                std::sync::atomic::Ordering::SeqCst,
            );
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
    // the identity registry: boot-seeded so the remark-preferred
    // faces are ready before the first thread render
    if let Ok(frames) = request(
        incoming(M::IdentitiesGet(zconned_stdio_types::IdentitiesGet {})),
        Expect::One(AnswerKind::IdentitiesPush),
    )
    .await
    {
        for f in frames {
            if let Some(zconned_stdio_types::outgoing_frame::Frame::IdentitiesPush(p)) = f.frame {
                set_identities(&mut CORE.write(), &p.identities);
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
    // the worker-death heartbeat: live from now on (the ports are up,
    // the boot face answered — a dead module gets surfaced + reloaded)
    spawn_worker_heartbeat();
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

/// msg_thread_log_get{thread} -> the log (merged into the cache).
/// B8 RENAMED SINGULAR (106 dev `cb271cf`, B28: the verb inventory
/// IS the cardinality documentation): the one-thread log verbs claim
/// a singular object now; the threads LIST verbs stay plural.
pub async fn thread_log(thread_hex: &str) -> Result<Vec<LogRow>, String> {
    let thread = parse_ref_id(thread_hex).ok_or_else(|| format!("bad thread ref {thread_hex}"))?;
    let frames = request(
        incoming(M::MsgThreadLogGet(zconned_stdio_types::MsgThreadLogGet {
            thread: Some(thread),
        })),
        Expect::One(AnswerKind::MsgThreadLogPush),
    )
    .await?;
    let mut rows = Vec::new();
    for f in frames {
        if let Some(zconned_stdio_types::outgoing_frame::Frame::MsgThreadLogPush(p)) = f.frame {
            // B7 E6: the log answer = one package per message; the
            // roster rides each package (local.signer)
            rows.extend(
                p.packages
                    .iter()
                    .filter_map(|pkg| {
                        pkg.entry
                            .as_ref()
                            .map(|e| entry_row(e, pkg.local.as_ref()))
                    }),
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
///
/// THE TIMESTAMP DISCIPLINE (107 parity, 2026-11-12): the interface
/// keeps the stamp state (a stamp within the window). When the state
/// is down — never stamped, or the last stamp aged out — the STAMP
/// MESSAGE commits FIRST (empty text/plain + the ts claim the core
/// attests at commit) and the text then chains FROM the stamp row
/// (the recomputed cursor leaf = the stamp's own ref). 107 queues
/// the text for the async receipt; ours is an awaited single-flight
/// chain, so the text simply follows the stamp commit, and the stamp
/// state moves on SUCCESS only (a failed stamp leaves the state
/// down — the next send retries).
pub async fn commit(thread_hex: Option<&str>, text: &str) -> Result<String, String> {
    let mut hex = thread_hex.map(|s| s.to_string());
    if stamp_needed() {
        hex = Some(commit_one(hex.as_deref(), "", true).await?);
        STAMPED_AT_MS.store(page_now_ms(), std::sync::atomic::Ordering::Relaxed);
    }
    commit_one(hex.as_deref(), text, false).await
}

/// The stamp state: the page clock (ms) of the last stamp commit;
/// 0 = never stamped. The interface AGES the state out by comparison
/// (107's insight: a timer and this aging are observably identical).
/// The page clock, not the unix clock: the state never persists
/// (a reload restarts it at 0 = the first send stamps — same as
/// 107, whose state starts None every app start).
static STAMPED_AT_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
const STAMP_WINDOW_MS: u64 = 5 * 60 * 1000;

/// Whether the stamp state is down (107's stamp_needed): never
/// stamped, or the last stamp is older than the window.
fn stamp_needed() -> bool {
    stamp_needed_at(
        STAMPED_AT_MS.load(std::sync::atomic::Ordering::Relaxed),
        page_now_ms(),
    )
}

/// The window test, time-injected (the unit-testable core of the
/// discipline).
fn stamp_needed_at(stamped_ms: u64, now_ms: u64) -> bool {
    stamped_ms == 0 || now_ms.saturating_sub(stamped_ms) >= STAMP_WINDOW_MS
}

/// The page clock in ms. wasm: performance.now() (the time origin's
/// scale — fine for the aging math, every term shares the origin).
/// native: the unix clock (the test face).
fn page_now_ms() -> u64 {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()
            .and_then(|w| w.performance())
            .map(|p| p.now() as u64)
            .unwrap_or(0)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
}

/// ONE commit chain (the discipline's atomic unit): cursor ->
/// draft content -> [stamp claim] -> commit. stamp = the timestamp
/// message: empty text/plain + the ts claim (empty data — the core
/// attests the value at commit).
async fn commit_one(thread_hex: Option<&str>, text: &str, stamp: bool) -> Result<String, String> {
    trace(&format!(
        "[commit] enter: thread={thread_hex:?} len={} stamp={stamp}",
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
    // 2. the draft content (day-one kind text/plain; the stamp's
    // data is legitimately empty)
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
    // 2b. the stamp's claim: the registry kind stages PENDING (empty
    // data — the core overwrites it with its own clock at commit;
    // an interface-supplied value would be discarded anyway)
    if stamp {
        request(
            incoming(M::MsgDraftClaimsAdd(
                zconned_stdio_types::MsgDraftClaimsAdd {
                    kind: CLAIM_KIND_TS.to_string(),
                    data: Vec::new(),
                },
            )),
            Expect::One(AnswerKind::MsgDraftPush),
        )
        .await?;
    }
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
            let row = entry_row(entry, p.package.as_ref().and_then(|pkg| pkg.local.as_ref()));
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

// ---- the identity-registry face (B7 c2) ----
// The registry = THIS DEVICE's annotations over observed anchors.
// The remark names EVERY identity a message carries: the author slot
// AND every verified signer (the principal, 2026-11-12). The display
// faces look up the registry LIVE (a set/reset re-faces every row),
// falling back to the parse-time anchor faces.

/// The F1 short form OF THE ANCHOR itself (the anchor = the
/// Fingerprint digest, so the short form groups its first 8 bytes —
/// the same notation Fingerprint::short() prints for a peer).
pub(crate) fn short_anchor(hex: &str) -> String {
    let head: Vec<char> = hex.chars().take(16).collect();
    head.chunks(4)
        .map(|c| c.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join(" ")
}

/// One identity's live remark (the registry's own view).
pub fn identity_remark(anchor_hex: &str) -> Option<String> {
    CORE.read()
        .identities
        .iter()
        .find(|i| i.anchor_hex == anchor_hex)
        .and_then(|i| i.remark.clone())
}

/// The author's display face, remark-preferred (the principal,
/// 2026-11-12): the self anchor renders "Me"; else the registry
/// remark; else the parse-time anchor face (peer fp / short anchor /
/// dash).
pub fn author_display(row: &LogRow) -> String {
    author_display_in(&CORE.read(), row)
}

fn author_display_in(st: &CoreState, row: &LogRow) -> String {
    if !row.from_hex.is_empty() && st.self_anchor.as_deref() == Some(row.from_hex.as_str()) {
        return "Me".to_string();
    }
    if let Some(r) = st
        .identities
        .iter()
        .find(|i| i.anchor_hex == row.from_hex)
        .and_then(|i| i.remark.clone())
    {
        return r;
    }
    row.author.clone()
}

/// One signer's display face, remark-preferred: self = "Me"; else
/// the registry remark; else the PeerBook fingerprint; else the
/// short anchor form.
pub fn signer_display(anchor_hex: &str) -> String {
    signer_display_in(&CORE.read(), anchor_hex)
}

fn signer_display_in(st: &CoreState, anchor_hex: &str) -> String {
    if st.self_anchor.as_deref() == Some(anchor_hex) {
        return "Me".to_string();
    }
    if let Some(r) = st
        .identities
        .iter()
        .find(|i| i.anchor_hex == anchor_hex)
        .and_then(|i| i.remark.clone())
    {
        return r;
    }
    if let Some(p) = st.peers.iter().find(|p| p.anchor_hex == anchor_hex) {
        return p.fp.clone();
    }
    if anchor_hex.len() >= 12 {
        format!("{}…", &anchor_hex[..12])
    } else {
        anchor_hex.to_string()
    }
}

/// THE UNTRUSTED VERDICT (the interface's conjunction — the core
/// reports facts, never the conjunction): a RECEIVED message that
/// HAS an author but NO verified signature FROM that author renders
/// the untrusted face (yellow bg + the honest line). Own rows are
/// exempt (the self-signature lands with the commit, B7 E1).
pub fn row_untrusted(row: &LogRow) -> bool {
    row_untrusted_in(&CORE.read(), row)
}

fn row_untrusted_in(st: &CoreState, row: &LogRow) -> bool {
    if row.from_hex.is_empty() {
        return false;
    }
    let own = st.self_anchor.as_deref() == Some(row.from_hex.as_str());
    !own && !row.signers.iter().any(|s| s.anchor_hex == row.from_hex)
}

/// identities_get -> the wholesale registry view (merged; the Me
/// page's Authors section re-asks on mount — the roster grows with
/// observed anchors and the core only pushes on the verbs).
pub async fn identities() -> Result<Vec<IdRow>, String> {
    let frames = request(
        incoming(M::IdentitiesGet(zconned_stdio_types::IdentitiesGet {})),
        Expect::One(AnswerKind::IdentitiesPush),
    )
    .await?;
    let mut rows = None;
    for f in frames {
        if let Some(zconned_stdio_types::outgoing_frame::Frame::IdentitiesPush(p)) = f.frame {
            let mut st = CORE.write();
            set_identities(&mut st, &p.identities);
            rows = Some(st.identities.clone());
        }
    }
    rows.ok_or_else(|| "identities_get answered without a registry".to_string())
}

/// Fire-and-forget registry refresh (the Me page's mount ask).
pub fn identities_refresh() {
    spawn_local(async move {
        let _ = identities().await;
    });
}

fn hex_to_bytes(hex: &str) -> Result<Vec<u8>, String> {
    if hex.len() % 2 != 0 {
        return Err("bad anchor hex".to_string());
    }
    (0..hex.len() / 2)
        .map(|i| {
            u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
                .map_err(|_| "bad anchor hex".to_string())
        })
        .collect()
}

/// identity_remark_set{anchor, remark} (case 17): the remark is
/// required non-empty (the core errors remark_required; the UI
/// pre-checks). The receipt = the wholesale registry view.
pub async fn set_remark(anchor_hex: &str, remark: &str) -> Result<(), String> {
    let anchor = hex_to_bytes(anchor_hex)?;
    let frames = request(
        incoming(M::IdentityRemarkSet(zconned_stdio_types::IdentityRemarkSet {
            anchor,
            remark: remark.to_string(),
        })),
        Expect::One(AnswerKind::IdentitiesPush),
    )
    .await?;
    // THE RECEIPT IS THE NEW WHOLESALE VIEW — merge it (the display
    // faces read the live registry signal)
    for f in frames {
        if let Some(zconned_stdio_types::outgoing_frame::Frame::IdentitiesPush(p)) = f.frame {
            set_identities(&mut CORE.write(), &p.identities);
        }
    }
    Ok(())
}

/// identity_remark_reset{anchor} (case 18): clear one remark — the
/// anchor face shows instead. Receipt = the wholesale view.
pub async fn reset_remark(anchor_hex: &str) -> Result<(), String> {
    let anchor = hex_to_bytes(anchor_hex)?;
    let frames = request(
        incoming(M::IdentityRemarkReset(
            zconned_stdio_types::IdentityRemarkReset { anchor },
        )),
        Expect::One(AnswerKind::IdentitiesPush),
    )
    .await?;
    // same merge as set (the wholesale view is the receipt)
    for f in frames {
        if let Some(zconned_stdio_types::outgoing_frame::Frame::IdentitiesPush(p)) = f.frame {
            set_identities(&mut CORE.write(), &p.identities);
        }
    }
    Ok(())
}

// ---- the thread-delete face (B8) ----

/// msg_thread_delete{thread} (case 19) -> the receipt = msgThreadsPush
/// (the wholesale SURVIVING threads list — the dual-role precedent;
/// re-render from it, no per-package cleanup echoes). Semantics (B8,
/// principal-confirmed): LOCAL store curation — one node removes its
/// own copy of one thread; counterparts untouched; the unreferenced
/// sweep rides the delete (resources/signatures no surviving record
/// references go with it); identity storage untouched; mails already
/// sent keep draining. RESURRECTION: an arriving mail for the deleted
/// thread re-grows it, and a commit onto the deleted anchor resolves
/// to the SAME id — expected UX, not a bug (mute/block = a separate
/// future feature). Unknown/absent id = an idempotent no-op (the
/// same receipt, no error).
pub async fn delete_thread(thread_hex: &str) -> Result<(), String> {
    let thread = parse_ref_id(thread_hex).ok_or_else(|| format!("bad thread ref {thread_hex}"))?;
    let frames = request(
        incoming(M::MsgThreadDelete(zconned_stdio_types::MsgThreadDelete {
            thread: Some(thread),
        })),
        Expect::One(AnswerKind::MsgThreadsPush),
    )
    .await?;
    let mut surviving = None;
    for f in frames {
        if let Some(zconned_stdio_types::outgoing_frame::Frame::MsgThreadsPush(p)) = f.frame {
            surviving = Some(
                p.threads
                    .iter()
                    .map(thread_ref_id)
                    .filter(|s| !s.is_empty())
                    .map(|id| ThreadRow { id })
                    .collect::<Vec<ThreadRow>>(),
            );
        }
    }
    let rows = surviving.ok_or_else(|| "delete answered without a threads receipt".to_string())?;
    let mut st = CORE.write();
    st.threads = rows;
    st.logs.remove(thread_hex);
    Ok(())
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

/// The heartbeat's pong clock (ms since epoch, stamped by the ctrl
/// onmessage; the heartbeat task reads it). 0 = never answered.
#[cfg(target_arch = "wasm32")]
static HEARTBEAT_PONG: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// THE WORKER-DEATH HEARTBEAT (106's ruling on the panic=abort
/// posture: a dead pump traps the module and the worker dies
/// silently — the page must notice and act). Cadence 5s; a cycle
/// posts `{"t":"ping"}` on the control port; a cycle with no fresh
/// pong is a MISS; two consecutive misses = a dead worker → surface
/// + respawn. The respawn is a full page RELOAD (the airtight shape:
/// every static/port rewiring problem dies with the page; the OPFS
/// store persists) guarded against a death-at-boot loop — a reload
/// no more often than every 20s, else stay surfaced. The reload
/// also re-runs the visit (the relay setting persists), so the
/// client comes back connected.
#[cfg(target_arch = "wasm32")]
const HEARTBEAT_PERIOD_MS: i32 = 5_000;
#[cfg(target_arch = "wasm32")]
const HEARTBEAT_MAX_MISSES: u32 = 2;
#[cfg(target_arch = "wasm32")]
const HEARTBEAT_RELOAD_MIN_GAP_MS: f64 = 20_000.0;

#[cfg(target_arch = "wasm32")]
async fn page_sleep(ms: i32) {
    // NO tokio::time on the page (no timer driver — core.rs's
    // standing note); a window setTimeout promise, awaited.
    let p = js_sys::Promise::new(&mut |resolve: js_sys::Function, _| {
        if let Some(w) = web_sys::window() {
            let _ = w
                .set_timeout_with_callback_and_timeout_and_arguments_0(resolve.unchecked_ref(), ms);
        }
    });
    let _ = wasm_bindgen_futures::JsFuture::from(p).await;
}

#[cfg(target_arch = "wasm32")]
fn spawn_worker_heartbeat() {
    spawn_local(async move {
        // the first cycle: arm the clock (a pong before any ping = a
        // stale stamp from a previous worker — ignore it)
        HEARTBEAT_PONG.store(0, std::sync::atomic::Ordering::SeqCst);
        let mut misses: u32 = 0;
        loop {
            page_sleep(HEARTBEAT_PERIOD_MS).await;
            let last = HEARTBEAT_PONG.load(std::sync::atomic::Ordering::SeqCst);
            let now = js_sys::Date::now() as u64;
            // watch-only: the worker broadcasts its own 5s pulse on the
            // scope; two stale cycles = a dead worker
            let answered = last > 0 && now.saturating_sub(last) <= HEARTBEAT_PERIOD_MS as u64 * 3;
            if answered {
                misses = 0;
                continue;
            }
            misses += 1;
            web_sys::console::error_1(
                &format!("[heartbeat] the worker missed ping {misses}/{HEARTBEAT_MAX_MISSES}")
                    .into(),
            );
            if misses < HEARTBEAT_MAX_MISSES {
                continue;
            }
            // DEAD: surface + respawn (the guarded reload).
            let dead = "the worker died (a panic under panic=abort traps the module) — respawning";
            web_sys::console::error_1(&format!("[heartbeat] {dead}").into());
            CORE.write().status = Status::Failed(dead.to_string());
            let w = web_sys::window().unwrap();
            let guard_ok = w
                .local_storage()
                .ok()
                .flatten()
                .and_then(|s| s.get_item("tafy:last-worker-reload").ok().flatten())
                .and_then(|s| s.parse::<f64>().ok())
                .map(|last_reload| js_sys::Date::now() - last_reload >= HEARTBEAT_RELOAD_MIN_GAP_MS)
                .unwrap_or(true);
            if let Some(s) = w.local_storage().ok().flatten() {
                let _ = s.set_item(
                    "tafy:last-worker-reload",
                    &format!("{}", js_sys::Date::now()),
                );
            }
            if guard_ok {
                let _ = js_sys::eval("location.reload()");
            }
            // the guard held: stay surfaced (the Failed face) and keep
            // watching — a worker that comes back alive resets us
            misses = 0;
        }
    });
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

// (the disconnect control verb's client face was REMOVED with the
// tap-to-disconnect affordance it served — the principal's
// three-feedback round, 2026-11-12: the strip is display-only. The
// worker still speaks the {"t":"disconnect"} control verb; a future
// disconnect affordance = one post_message away, by ruling only.)

#[cfg(test)]
mod registry_tests {
    use super::*;
    fn row(from: &str, signers: &[&str]) -> LogRow {
        LogRow {
            ref_id: "1:1:algo:x".into(),
            author: "a".into(),
            text: "hi".into(),
            ts_ms: 0,
            stamp: false,
            from_hex: from.to_string(),
            signers: signers
                .iter()
                .map(|s| SignerRow { anchor_hex: s.to_string() })
                .collect(),
        }
    }
    #[test]
    fn untrusted_join_and_remark_faces() {
        // the PURE-STATE face (no dioxus runtime in tests)
        let st = CoreState {
            self_anchor: Some("aa".repeat(32)),
            identities: vec![IdRow {
                anchor_hex: "bb".repeat(32),
                remark: Some("alice".into()),
            }],
            ..CoreState::default()
        };
        // received + signed by the author -> trusted
        assert!(!row_untrusted_in(&st, &row(&"bb".repeat(32), &[&"bb".repeat(32)])));
        // received + author present but unsigned -> UNTRUSTED
        assert!(row_untrusted_in(&st, &row(&"bb".repeat(32), &[&"cc".repeat(32)])));
        // received + NO signature at all -> untrusted
        assert!(row_untrusted_in(&st, &row(&"bb".repeat(32), &[])));
        // own row exempt (no self-signature needed for the face)
        assert!(!row_untrusted_in(&st, &row(&"aa".repeat(32), &[])));
        // absent author (no mail identity) -> exempt
        assert!(!row_untrusted_in(&st, &row("", &[])));
        // the author face: the remark wins over the anchor face;
        // self stays "Me"; the short-anchor fallback groups the F1 form
        let r = row(&"bb".repeat(32), &[]);
        assert_eq!(author_display_in(&st, &r), "alice");
        let mine = row(&"aa".repeat(32), &[]);
        assert_eq!(author_display_in(&st, &mine), "Me");
        let stranger = row(&"dd".repeat(32), &[]);
        // the fallback = the parse-time face (author_face already
        // rendered the stranger form at parse)
        assert_eq!(author_display_in(&st, &stranger), "a");
        // the signer face: remark-preferred, then the short anchor
        assert_eq!(signer_display_in(&st, &"bb".repeat(32)), "alice");
        assert_eq!(signer_display_in(&st, &"cc".repeat(32)), format!("{}…", &"c".repeat(12)));
    }
}

#[cfg(test)]
mod ts_claim_tests {
    use super::chrono_millis;
    use super::stamp_needed_at;
    #[test]
    fn rfc3339_utc_parses() {
        // cross-checked against an independent epoch calculator
        assert_eq!(
            chrono_millis("2026-11-11T08:47:00.123Z"),
            Some(1_794_386_820_123)
        );
        assert_eq!(
            chrono_millis("2026-11-11T08:47:00Z"),
            Some(1_794_386_820_000)
        );
        assert_eq!(
            chrono_millis("2024-02-29T00:00:00Z"),
            Some(1_709_164_800_000)
        );
        assert_eq!(chrono_millis("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(chrono_millis("2026-11-11T08:47:00+01:00"), None);
        assert_eq!(chrono_millis("2026-02-29T00:00:00Z"), None);
        assert_eq!(chrono_millis("garbage"), None);
    }

    #[test]
    fn stamp_window_aging() {
        // the discipline's window core (107 parity): never stamped
        // stamps; within the 5-minute window does not; the boundary
        // (>= 300s) stamps again; a reload restarts at 0.
        assert!(stamp_needed_at(0, 1_000));
        assert!(!stamp_needed_at(1_000_000, 1_000_000 + 299_999));
        assert!(stamp_needed_at(1_000_000, 1_000_000 + 300_000));
        assert!(stamp_needed_at(1_000_000, 1_000_000 + 400_000));
    }
}
