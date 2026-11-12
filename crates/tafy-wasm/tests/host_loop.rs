//! The host-loop conformance test (native only): a RAM storage + the
//! real core on the tokio current-thread runtime, driven through the
//! full interface link — handshake, self_get, threads_get, a fresh
//! commit — asserting the frame stream. This is the acceptance (c)
//! skeleton (the same op sequence must produce the same frames in
//! the wasm worker; the browser legs — OPFS, MessageChannel — are
//! verified on the device, not here).
//!
//! Framing discipline learned here (also binding for the UI client):
//! the wire stream is varint length-delimited; the Deframer returns
//! PAYLOADS (prefix stripped); chunks are not frames (several frames
//! can ride one chunk — a chunk-forwarding reader MUST deframe).

#![cfg(not(target_arch = "wasm32"))]

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use prost::Message as _;
use zconned_material::CLAIM_KIND_TS;
use zconned::storage::Storage;
use zconned_stdio_types::{IncomingFrame, OutgoingFrame, PROTOCOL_VERSION};

// ---- the RAM storage (the trait's semantics, native test double) ----

#[derive(Default)]
struct Ram {
    files: Mutex<BTreeMap<PathBuf, Vec<u8>>>,
    dirs: Mutex<Vec<PathBuf>>,
}

impl Ram {
    fn norm(p: &Path) -> PathBuf {
        // the store root is absolute ("/tafy/store/..") but the
        // entries are kept relative — strip the root (the OPFS face
        // does the same for its worker-relative store root)
        p.strip_prefix("/")
            .map(|r| r.to_path_buf())
            .unwrap_or_else(|_| p.to_path_buf())
    }
    fn ensure_parents(&self, p: &Path) {
        // ancestors ONLY — a file's own name must not become a dir
        let normed = Self::norm(p);
        let comps: Vec<_> = normed.components().collect();
        let mut acc = PathBuf::new();
        for c in comps.iter().take(comps.len().saturating_sub(1)) {
            acc.push(c.as_os_str());
            self.dirs.lock().unwrap().push(acc.clone());
        }
    }
}

impl Storage for Ram {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.files
            .lock()
            .unwrap()
            .get(&Self::norm(path))
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no file"))
    }
    fn write(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        self.ensure_parents(path);
        self.files
            .lock()
            .unwrap()
            .insert(Self::norm(path), data.to_vec());
        Ok(())
    }
    fn append(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        {
            let files = self.files.lock().unwrap();
            let fresh = files.get(&Self::norm(path)).is_none_or(|v| v.is_empty());
            if fresh {
                drop(files);
                self.ensure_parents(path);
            }
        }
        self.files
            .lock()
            .unwrap()
            .entry(Self::norm(path))
            .or_default()
            .extend_from_slice(data);
        Ok(())
    }
    fn exists(&self, path: &Path) -> io::Result<bool> {
        let p = Self::norm(path);
        Ok(self.files.lock().unwrap().contains_key(&p) || self.dirs.lock().unwrap().contains(&p))
    }
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        let p = Self::norm(path);
        let mut acc = PathBuf::new();
        for c in p.components() {
            acc.push(c.as_os_str());
            self.dirs.lock().unwrap().push(acc.clone());
        }
        Ok(())
    }
    fn read_dir_dirs(&self, path: &Path) -> io::Result<Vec<String>> {
        let p = Self::norm(path);
        let dirs = self.dirs.lock().unwrap();
        let mut out: Vec<String> = dirs
            .iter()
            .filter_map(|d| d.strip_prefix(&p).ok())
            .filter_map(|r| {
                r.components()
                    .next()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
            })
            .filter(|n| !n.is_empty())
            .collect();
        out.sort();
        out.dedup();
        Ok(out)
    }
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.ensure_parents(to);
        let mut files = self.files.lock().unwrap();
        let v = files
            .remove(&Self::norm(from))
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no file"))?;
        files.insert(Self::norm(to), v);
        Ok(())
    }
    fn write_private(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        // day one: the plain write (the platform expresses no perms)
        self.write(path, data)
    }
}

// ---- the driver ----

/// A framed duplex channel to the core: write = frames in, read =
/// frames out (the pump deframes the core's length-delimited stdout;
/// recv yields ONE frame per call).
struct Link {
    stdin_w: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    stdout_r: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
}

impl Link {
    fn send(&self, frame: &IncomingFrame) {
        self.stdin_w
            .send(frame.encode_length_delimited_to_vec())
            .unwrap();
    }
    async fn recv(&mut self) -> OutgoingFrame {
        let bytes = self.stdout_r.recv().await.expect("core stdout ended");
        OutgoingFrame::decode(&mut bytes.as_slice()).expect("bad frame")
    }
    async fn recv_handshake(&mut self) -> u32 {
        let bytes = self.stdout_r.recv().await.expect("no ack");
        zconned_stdio_types::Handshake::decode(&mut bytes.as_slice())
            .expect("the ack is not a Handshake")
            .version
    }
}

fn spawn_core(storage: Arc<dyn Storage>, card: bool) -> Link {
    let (in_tx, mut in_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();

    let (mut core_in_w, core_in_r) = tokio::io::duplex(4096);
    let (mut core_out_r, core_out_w) = tokio::io::duplex(4096);

    // bridge: harness frames -> the core's stdin (length-delimited
    // stream — the core's read link deframes it)
    tokio::task::spawn_local(async move {
        use tokio::io::AsyncWriteExt as _;
        while let Some(bytes) = in_rx.recv().await {
            if core_in_w.write_all(&bytes).await.is_err() {
                break;
            }
        }
    });

    // pump: the core's stdout -> the harness. CHUNKS are not FRAMES —
    // several frames can ride one chunk, so a Deframer sits here.
    tokio::task::spawn_local(async move {
        use tokio::io::AsyncReadExt as _;
        let mut deframer = protobuf_varint_deframer::Deframer::builder().build();
        loop {
            let mut chunk = [0u8; 4096];
            match core_out_r.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let mut data: &[u8] = &chunk[..n];
                    while let Ok(Some(frame)) = deframer.feed(&mut data) {
                        if out_tx.send(frame.to_vec()).is_err() {
                            return;
                        }
                    }
                }
            }
        }
    });

    // the core: the same calls the worker's boot makes (B6 shape:
    // peers-before-kit, the Identity card + record signer)
    let peers =
        Arc::new(zconned::transport::PeerBook::open_with("/tafy/store", storage.clone()).unwrap());
    let kit =
        zconned::session::ArrivalKit::open("/tafy/store", storage.clone(), peers.clone()).unwrap();
    let keyring = zconned::mail::Keyring::open_with(
        "/tafy/store",
        storage,
        zconned::mail::DEFAULT_OPK_POOL_SIZE,
        zconned::mail::DEFAULT_OPK_REPLENISH_THRESHOLD,
        zconned::mail::DEFAULT_OPK_BURN_DEPTH,
    )
    .unwrap();
    let identity_opt = if card {
        Some(
            zconned::session::Identity::builder()
                .card(zconned::transport::peer_face_of(
                    &keyring.public_set(),
                    Some(keyring.record_sign_public_bytes()),
                ))
                .record_sign(keyring.record_signer())
                .build(),
        )
    } else {
        None
    };
    let sender = zconned::transport::Sender::<zconned::transport::Detached>::disabled();
    let mut rd = tokio::io::BufReader::new(core_in_r);
    tokio::task::spawn_local(async move {
        let _ = zconned::run_with_arrivals(
            kit,
            sender,
            identity_opt,
            Arc::new(zconned::transport::TokioSpawner),
            &mut rd,
            core_out_w,
        )
        .await;
    });

    Link {
        stdin_w: in_tx,
        stdout_r: out_rx,
    }
}

fn frame_tag(f: &OutgoingFrame) -> &'static str {
    use zconned_stdio_types::outgoing_frame::Frame as F;
    match &f.frame {
        Some(F::MsgPackagePush(_)) => "msg_package_push",
        Some(F::MsgCursorPush(_)) => "msg_cursor_push",
        Some(F::MsgDraftPush(_)) => "msg_draft_push",
        Some(F::MsgThreadsPush(_)) => "msg_threads_push",
        Some(F::MsgThreadsLogPush(_)) => "msg_threads_log_push",
        Some(F::Error(_)) => "error",
        Some(F::SelfPush(_)) => "self_push",
        _ => "other",
    }
}

#[tokio::test(flavor = "current_thread")]
async fn host_loop_handshake_identity_threads_commit() {
    // the tokio current_thread runtime hosts the whole loop — the
    // native analogue of the wasm executor arrangement (the LocalSet
    // rides the test's own runtime: no nested block_on)
    tokio::task::LocalSet::new()
        .run_until(async move {
            let ram: Arc<dyn Storage> = Arc::new(Ram::default());

            // -- identity: the keyring creates itself on first use --
            // (acceptance (a)'s native analogue: the keyring file
            // exists)
            {
                let _ = zconned::mail::Keyring::open_with(
                    "/tafy/store",
                    ram.clone(),
                    zconned::mail::DEFAULT_OPK_POOL_SIZE,
                    zconned::mail::DEFAULT_OPK_REPLENISH_THRESHOLD,
                    zconned::mail::DEFAULT_OPK_BURN_DEPTH,
                )
                .unwrap();
                assert!(ram.exists(Path::new("/tafy/store/keyring.json")).unwrap());
            }

            let mut link = spawn_core(ram.clone(), true);

            // -- the handshake (frame #1 both ways; the handshake is
            // NOT an IncomingFrame method — it is its own message
            // type, raw on the wire) --
            let raw = zconned_stdio_types::Handshake {
                version: PROTOCOL_VERSION,
            };
            link.stdin_w
                .send(raw.encode_length_delimited_to_vec())
                .unwrap();
            let ack = link.recv_handshake().await;
            assert_eq!(ack, PROTOCOL_VERSION, "handshake version mismatch");

            // -- self_get: the card answers --
            link.send(&IncomingFrame {
                method: Some(zconned_stdio_types::incoming_frame::Method::SelfGet(
                    zconned_stdio_types::SelfGet {},
                )),
            });
            let f = link.recv().await;
            assert_eq!(frame_tag(&f), "self_push", "self_get answered wrong: {f:?}");

            // -- threads_get: empty day one --
            link.send(&IncomingFrame {
                method: Some(zconned_stdio_types::incoming_frame::Method::MsgThreadsGet(
                    zconned_stdio_types::MsgThreadsGet {},
                )),
            });
            let f = link.recv().await;
            assert_eq!(
                frame_tag(&f),
                "msg_threads_push",
                "threads_get answered wrong: {f:?}"
            );

            // -- the fresh commit chain: cursor fresh -> content ->
            // commit (the core stamps the registry claims and answers
            // the B7 pair: msg_package_push (complete roster) + the
            // resting draft) --
            link.send(&IncomingFrame {
                method: Some(zconned_stdio_types::incoming_frame::Method::MsgCursorSet(
                    zconned_stdio_types::MsgCursorSet {
                        at: vec![],
                        fresh: Some(zconned_stdio_types::Fresh {}),
                    },
                )),
            });
            let f = link.recv().await;
            assert_eq!(
                frame_tag(&f),
                "msg_cursor_push",
                "cursor answered wrong: {f:?}"
            );

            link.send(&IncomingFrame {
                method: Some(
                    zconned_stdio_types::incoming_frame::Method::MsgDraftContentSet(
                        zconned_stdio_types::MsgDraftContentSet {
                            kind: "text/plain".to_string(),
                            data: b"first light".to_vec(),
                        },
                    ),
                ),
            });
            let f = link.recv().await;
            assert_eq!(
                frame_tag(&f),
                "msg_draft_push",
                "draft answered wrong: {f:?}"
            );

            link.send(&IncomingFrame {
                method: Some(zconned_stdio_types::incoming_frame::Method::MsgDraftCommit(
                    zconned_stdio_types::MsgDraftCommit {},
                )),
            });
            let f1 = link.recv().await;
            assert_eq!(
                frame_tag(&f1),
                "msg_package_push",
                "commit's first answer wrong: {f1:?}"
            );
            let f2 = link.recv().await;
            assert_eq!(
                frame_tag(&f2),
                "msg_draft_push",
                "the resting draft wrong: {f2:?}"
            );

            // the commit landed: the threads list now carries ONE
            // thread, and the store holds its transcript tree
            link.send(&IncomingFrame {
                method: Some(zconned_stdio_types::incoming_frame::Method::MsgThreadsGet(
                    zconned_stdio_types::MsgThreadsGet {},
                )),
            });
            let f = link.recv().await;
            match f.frame {
                Some(zconned_stdio_types::outgoing_frame::Frame::MsgThreadsPush(p)) => {
                    assert_eq!(p.threads.len(), 1, "one thread after one commit");
                }
                other => panic!("threads_get answered wrong: {other:?}"),
            }
            let dirs = ram.read_dir_dirs(Path::new("/tafy/store")).unwrap();
            assert!(
                dirs.contains(&"threads".to_string()),
                "the store holds a threads dir after the commit: {dirs:?}"
            );
        })
        .await;
}

/// THE TIMESTAMP DISCIPLINE (107 parity, conformance): the stamp
/// message commits FIRST (empty text/plain + the ts claim the core
/// ATTESTS — non-empty data at the package push) and the text then
/// chains FROM the stamp row (the text's prev[0] = the stamp's own
/// ref). This is the native proof of the chain the wasm composer
/// produces through core::commit.
#[tokio::test(flavor = "current_thread")]
async fn host_loop_stamp_discipline_chains_text_to_stamp() {
    tokio::task::LocalSet::new()
        .run_until(async move {
            let ram: Arc<dyn Storage> = Arc::new(Ram::default());
            let mut link = spawn_core(ram, true);

            // handshake
            let raw = zconned_stdio_types::Handshake {
                version: PROTOCOL_VERSION,
            };
            link.stdin_w
                .send(raw.encode_length_delimited_to_vec())
                .unwrap();
            let ack = link.recv_handshake().await;
            assert_eq!(ack, PROTOCOL_VERSION, "handshake version mismatch");

            // -- the STAMP commit: fresh cursor -> empty content ->
            // the ts claim (empty data, staged pending) -> commit --
            link.send(&IncomingFrame {
                method: Some(zconned_stdio_types::incoming_frame::Method::MsgCursorSet(
                    zconned_stdio_types::MsgCursorSet {
                        at: vec![],
                        fresh: Some(zconned_stdio_types::Fresh {}),
                    },
                )),
            });
            let f = link.recv().await;
            assert_eq!(frame_tag(&f), "msg_cursor_push", "cursor wrong: {f:?}");

            link.send(&IncomingFrame {
                method: Some(
                    zconned_stdio_types::incoming_frame::Method::MsgDraftContentSet(
                        zconned_stdio_types::MsgDraftContentSet {
                            kind: "text/plain".to_string(),
                            data: Vec::new(),
                        },
                    ),
                ),
            });
            let f = link.recv().await;
            assert_eq!(frame_tag(&f), "msg_draft_push", "draft wrong: {f:?}");

            link.send(&IncomingFrame {
                method: Some(
                    zconned_stdio_types::incoming_frame::Method::MsgDraftClaimsAdd(
                        zconned_stdio_types::MsgDraftClaimsAdd {
                            kind: CLAIM_KIND_TS.to_string(),
                            data: Vec::new(),
                        },
                    ),
                ),
            });
            let f = link.recv().await;
            assert_eq!(frame_tag(&f), "msg_draft_push", "claim wrong: {f:?}");

            link.send(&IncomingFrame {
                method: Some(zconned_stdio_types::incoming_frame::Method::MsgDraftCommit(
                    zconned_stdio_types::MsgDraftCommit {},
                )),
            });
            let f1 = link.recv().await;
            assert_eq!(
                frame_tag(&f1),
                "msg_package_push",
                "stamp commit wrong: {f1:?}"
            );
            let _ = link.recv().await; // the resting draft

            // the stamp entry: empty text + the ATTESTED ts claim
            let stamp_entry = match f1.frame {
                Some(zconned_stdio_types::outgoing_frame::Frame::MsgPackagePush(p)) => {
                    p.package.unwrap().entry.unwrap()
                }
                other => panic!("expected the stamp package: {other:?}"),
            };
            let stamp_text = stamp_entry
                .content
                .as_ref()
                .and_then(|c| c.value.as_ref())
                .map(|v| v.data.clone())
                .unwrap_or_default();
            assert!(stamp_text.is_empty(), "the stamp's text must be empty");
            let ts_claim = stamp_entry
                .claims
                .iter()
                .find(|c| c.kind == CLAIM_KIND_TS)
                .and_then(|c| c.value.as_ref())
                .map(|v| v.data.clone())
                .unwrap_or_default();
            assert!(
                !ts_claim.is_empty(),
                "the core must ATTEST the ts claim at commit (non-empty value)"
            );
            let stamp_ref = stamp_entry
                .meta
                .as_ref()
                .map(|m| zconned_material::MsgRef {
                    ver_major: stamp_entry.ver_major,
                    ver_minor: stamp_entry.ver_minor,
                    algo: m.algo.clone(),
                    digest: m.digest.clone(),
                })
                .unwrap();

            // -- the TEXT commit: the cursor re-positions AT the
            // stamp row (the recomputed leaf) -> the text -> commit --
            link.send(&IncomingFrame {
                method: Some(zconned_stdio_types::incoming_frame::Method::MsgCursorSet(
                    zconned_stdio_types::MsgCursorSet {
                        at: vec![stamp_ref.clone()],
                        fresh: None,
                    },
                )),
            });
            let f = link.recv().await;
            assert_eq!(frame_tag(&f), "msg_cursor_push", "cursor 2 wrong: {f:?}");

            link.send(&IncomingFrame {
                method: Some(
                    zconned_stdio_types::incoming_frame::Method::MsgDraftContentSet(
                        zconned_stdio_types::MsgDraftContentSet {
                            kind: "text/plain".to_string(),
                            data: b"hello after the stamp".to_vec(),
                        },
                    ),
                ),
            });
            let _ = link.recv().await; // msg_draft_push

            link.send(&IncomingFrame {
                method: Some(zconned_stdio_types::incoming_frame::Method::MsgDraftCommit(
                    zconned_stdio_types::MsgDraftCommit {},
                )),
            });
            let f1 = link.recv().await;
            assert_eq!(
                frame_tag(&f1),
                "msg_package_push",
                "text commit wrong: {f1:?}"
            );

            // THE CHAIN: the text's parent = the stamp's own ref
            let text_entry = match f1.frame {
                Some(zconned_stdio_types::outgoing_frame::Frame::MsgPackagePush(p)) => {
                    p.package.unwrap().entry.unwrap()
                }
                other => panic!("expected the text package: {other:?}"),
            };
            assert_eq!(
                text_entry.prev.first().map(|r| r.digest.as_slice()),
                Some(stamp_ref.digest.as_slice()),
                "the text must chain FROM the stamp row"
            );
        })
        .await;
}
