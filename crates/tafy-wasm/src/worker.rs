//! The worker bootstrap: one worker = one core instance.
//!
//! JS glue (public/tafy-core/tafy-worker.js in the PWA bundle) loads
//! the wasm module and calls [`tafy_worker_main`]. The UI side
//! creates a MessageChannel and transfers its second port as the
//! FIRST message to this worker; every later message = one varint-
//! framed i2c frame; every frame the core produces = one postMessage
//! back. The single-instance guard (Web Locks 'tafy-core') lives on
//! the UI side — the worker is spawned by the winning tab only.
//!
//! BATCH 3 (2026-11-11): the carrier leg is WIRED. The UI passes a
//! second port (the control channel) in the same transfer; on it the
//! UI sends the boot JSON {"t":"boot","relay":...} and later
//! {"t":"connect",...} / {"t":"disconnect"}; the worker replies with
//! {"t":"conn",...} state faces. The core runs with the TRANSPORT's
//! sender (commits persist into the outbox and seal on the visit's
//! attach; no visit = queued, delivered at the next one — the
//! protocol's persist-then-queue discipline). The visit routine,
//! the plane pumps and the seal task are the library's; this worker
//! drives them through the WasmSpawner + the tafy-carrier gRPC-Web
//! carrier.

use std::sync::Arc;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

/// The UI→worker control commands (JSON on the control port).
#[derive(Debug)]
enum Ctrl {
    Boot { relay: String },
    Connect { relay: String },
    Disconnect,
    Diag,
}

/// A wasm sleep (tokio's time driver is off on wasm32): a Promise
/// resolved by the worker scope's setTimeout.
fn wasm_sleep(ms: i32) -> wasm_bindgen_futures::JsFuture {
    let p = js_sys::Promise::new(&mut |resolve, _| {
        let scope: web_sys::DedicatedWorkerGlobalScope = js_sys::global().unchecked_into();
        let _ = scope.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
    });
    wasm_bindgen_futures::JsFuture::from(p)
}

// The visit gate: a relay whose plane streams never flush their
// response headers (the live-relay finding) holds the visit future
// forever — the face must fail loudly, not hang.
//
// THE ATTACH FLAKE ABSORB lives INSIDE the carrier now (106's §5
// verdict, 2026-11-15): the plane-OPEN RPCs race a SHORT timeout
// and retry promptly (a retry opens a fresh pooled connection —
// the re-test data: every retry connects; only the OPEN flakes).
// The visit ceremony itself is NOT retried here and the gate is
// NOT shortened: the ceremony legitimately runs tens of seconds
// (tunnel unary latency × the call ladder — measured; a 10s visit
// gate cut every healthy attempt and just re-ran the whole
// ceremony, churning tokens). This gate stays the single-attempt
// backstop it always was.
const VISIT_TIMEOUT_MS: i32 = 120_000;

impl Ctrl {
    fn parse(raw: &str) -> Option<Self> {
        let v: serde_json::Value = serde_json::from_str(raw).ok()?;
        match v.get("t")?.as_str()? {
            "boot" => Some(Ctrl::Boot {
                relay: v
                    .get("relay")
                    .and_then(|r| r.as_str())
                    .unwrap_or("")
                    .to_string(),
            }),
            "connect" => Some(Ctrl::Connect {
                relay: v
                    .get("relay")
                    .and_then(|r| r.as_str())
                    .unwrap_or("")
                    .to_string(),
            }),
            "disconnect" => Some(Ctrl::Disconnect),
            "diag" => Some(Ctrl::Diag),
            _ => None,
        }
    }
}

/// The worker→UI control faces: JSON on the CONTROL PORT (the UI's
/// conn-state parser lives there; the scope route = breadcrumbs only).
fn conn_face(port: &web_sys::MessagePort, state: &str, detail: &str) {
    let face = serde_json::json!({ "t": "conn", "state": state, "detail": detail });
    let _ = port.post_message(&JsValue::from_str(&face.to_string()));
}

/// The wasm host's spawner: the pump runs on the wasm-bindgen-futures
/// executor (spawn_local — no runtime exists to park a task on; the
/// spike proved tokio mpsc/select wake fine from JS microtasks).
/// Abort = the spike-proven cancel combinator (flag + Notify under a
/// select at the pump's next await point).
struct WasmSpawner;

struct WasmPump {
    done: std::sync::Arc<std::sync::atomic::AtomicBool>,
    notify: std::sync::Arc<tokio::sync::Notify>,
}

impl zconned::transport::PumpHandle for WasmPump {
    fn abort(&self) {
        self.done.store(true, std::sync::atomic::Ordering::SeqCst);
        self.notify.notify_one();
    }
}

impl zconned::transport::Spawner for WasmSpawner {
    fn spawn(
        &self,
        pump: std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>,
    ) -> Box<dyn zconned::transport::PumpHandle> {
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let notify = std::sync::Arc::new(tokio::sync::Notify::new());
        let handle = WasmPump {
            done: done.clone(),
            notify: notify.clone(),
        };
        wasm_bindgen_futures::spawn_local(async move {
            let cancel = async {
                notify.notified().await;
            };
            futures::future::select(Box::pin(pump), Box::pin(cancel)).await;
            done.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        Box::new(handle)
    }
}

/// The worker's entry point (called by the JS glue once wasm is up).
#[wasm_bindgen]
pub fn tafy_worker_main() {
    // TEMP: a posting panic hook (a release-wasm panic = a bare trap;
    // the worker's console is unreachable from the page — surface the
    // message through the scope; production drops it silently)
    let boot_scope: web_sys::DedicatedWorkerGlobalScope = js_sys::global().unchecked_into();
    {
        let hs = boot_scope.clone();
        std::panic::set_hook(Box::new(move |info| {
            let _ = hs.post_message(&JsValue::from_str(&format!("PANIC: {info}")));
        }));
    }
    wasm_bindgen_futures::spawn_local(async move {
        if let Err(e) = boot().await {
            web_sys::console::error_1(&format!("tafy worker boot failed: {e:?}").into());
        }
    });
}

/// The storage face's self-test (usable before the core's pump ask
/// lands in 106): open the store, write, append, rename, exists,
/// read back, read dirs — then leave the results as console logs.
#[wasm_bindgen]
pub fn tafy_worker_storage_selftest() {
    wasm_bindgen_futures::spawn_local(async move {
        let scope: web_sys::DedicatedWorkerGlobalScope = js_sys::global().unchecked_into();
        // a panic in release wasm = a bare trap; surface the message
        // through the scope (the worker's own console is unreachable
        // from the page)
        {
            let hook_scope = scope.clone();
            std::panic::set_hook(Box::new(move |info| {
                let _ = hook_scope.post_message(&JsValue::from_str(&format!("PANIC: {info}")));
            }));
        }
        let step = move |m: &str| {
            let _ = scope.post_message(&JsValue::from_str(m));
        };
        step("selftest: start");
        match run_selftest().await {
            Ok(()) => step("selftest: PASS"),
            Err(e) => step(&format!("selftest: FAIL {e:?}")),
        }
    });
}

/// The boot sequence (the full core path — waits on the 106 pump ask).
async fn boot() -> Result<(), JsValue> {
    let scope: web_sys::DedicatedWorkerGlobalScope = js_sys::global().unchecked_into();

    // The inbound bridge: onmessage → the bridge channel (the
    // wasm-executor-proven wake path, spike v4).
    let (in_tx, mut in_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    let (port_tx, port_rx) = tokio::sync::oneshot::channel::<web_sys::MessagePort>();
    // the CONTROL port (batch 3): connect/disconnect/conn-state faces
    let (ctrl_tx, mut ctrl_rx) = tokio::sync::mpsc::unbounded_channel::<Ctrl>();
    let (ctrl_port_tx, ctrl_port_rx) = tokio::sync::oneshot::channel::<web_sys::MessagePort>();

    // the transfer message carries TWO ports: [i2c, control] (order
    // fixed by the UI's post); strings ride the scope (breadcrumbs),
    // the boot JSON rides the control port
    let reply = std::cell::RefCell::new(Some(port_tx));
    let reply_ctrl = std::cell::RefCell::new(Some(ctrl_port_tx));
    let onmessage = Closure::wrap(Box::new(move |e: web_sys::MessageEvent| {
        // the diag string hook (TEMP): a plain string = a command
        if let Some(raw) = e.data().as_string()
            && raw == "diag:storage"
        {
            tafy_worker_storage_diag();
            return;
        }
        let ports = e.ports();
        for p in ports {
            if p.is_undefined() {
                continue;
            }
            let p: web_sys::MessagePort = p.unchecked_into();
            if let Some(tx) = reply.borrow_mut().take() {
                let _ = tx.send(p);
            } else if let Some(tx) = reply_ctrl.borrow_mut().take() {
                let _ = tx.send(p);
            }
        }
    }) as Box<dyn FnMut(web_sys::MessageEvent)>);
    scope.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    onmessage.forget(); // the worker lives as long as the page
    // the step face travels WITH the spawned tasks (owned clones —
    // the page sees the boot's steps on the diag page; production
    // drops them, the client sets no worker.onmessage)
    let step = {
        let scope_steps = scope.clone();
        move |m: &str| {
            let _ = scope_steps.post_message(&JsValue::from_str(m));
        }
    };
    step("[worker] onmessage set");
    // THE READY SIGNAL: the UI must not transfer the port before this
    // (a message posted into a module worker before its script's
    // evaluation completes can be DROPPED — the ready handshake makes
    // the transfer race-free)
    let _ = scope.post_message(&JsValue::from_str("worker:ready"));

    // The storage face (the seed walk happens here).
    let storage = crate::storage::OpfsStorage::open("/tafy/store").await?;
    step("[worker] opfs open");
    let port = port_rx
        .await
        .map_err(|_| JsValue::from_str("port channel closed"))?;
    let ctrl_port = ctrl_port_rx
        .await
        .map_err(|_| JsValue::from_str("ctrl port channel closed"))?;
    step("[worker] ports in, wiring ctrl onmessage");

    // The control port: JSON faces both ways (the connect /
    // disconnect commands; the conn-state faces ride back).
    let feed_ctrl = ctrl_tx.clone();
    let scope_ctrl = scope.clone();
    let ctrl_onmessage = Closure::wrap(Box::new(move |e: web_sys::MessageEvent| {
        let raw = e.data().as_string().unwrap_or_default();
        match Ctrl::parse(&raw) {
            Some(cmd) => {
                let _ = feed_ctrl.send(cmd);
            }
            None => {
                let _ = scope_ctrl.post_message(&JsValue::from_str(&format!(
                    "[worker] bad ctrl face: {raw}"
                )));
            }
        }
    }) as Box<dyn FnMut(web_sys::MessageEvent)>);
    ctrl_port.set_onmessage(Some(ctrl_onmessage.as_ref().unchecked_ref()));
    ctrl_onmessage.forget();
    ctrl_port.start();

    // The boot gate: the UI's relay URL rides the FIRST control face
    // (the core's sender shape depends on it).
    let relay = match ctrl_rx.recv().await {
        Some(Ctrl::Boot { relay }) => relay,
        Some(_) => return Err(JsValue::from_str("first ctrl face must be boot")),
        None => return Err(JsValue::from_str("ctrl channel closed")),
    };
    step("[worker] boot face in");

    // Wire the port: the UI posts its frames on PORT1 (this side =
    // PORT2); every frame here -> the bridge channel. The transfer
    // itself may have raced ahead of this wiring — start() drains
    // whatever queued.
    let feed = in_tx;
    let scope_in = scope.clone();
    let port_onmessage = Closure::wrap(Box::new(move |e: web_sys::MessageEvent| {
        let bytes = js_sys::Uint8Array::new(&e.data()).to_vec();
        let _ = scope_in.post_message(&JsValue::from_str(&format!(
            "[worker] port frame in ({}B)",
            bytes.len()
        )));
        let _ = feed.send(bytes);
    }) as Box<dyn FnMut(web_sys::MessageEvent)>);
    port.set_onmessage(Some(port_onmessage.as_ref().unchecked_ref()));
    port_onmessage.forget();
    port.start();
    step("[worker] port wired");

    // The core: in-memory duplex halves; the core reads frames from
    // one, writes frames to the other; the bridge pumps both.
    let (mut core_in_write, core_in_read) = tokio::io::duplex(4096);
    let (mut core_out_read, core_out_write) = tokio::io::duplex(4096);

    // The carrier leg (batch 3): the core runs with the TRANSPORT's
    // sender — commits persist into the outbox and seal on the
    // visit's attach (persist-then-queue; no visit = queued). The
    // transport is built here ONCE and lives in the control task;
    // visit/repoint/disconnect drive it from the UI's faces.
    let storage_dyn: zconned::SharedStorage = storage;
    let keyring = zconned::mail::Keyring::open_with(
        "/tafy/store",
        storage_dyn.clone(),
        zconned::mail::DEFAULT_OPK_POOL_SIZE,
        zconned::mail::DEFAULT_OPK_REPLENISH_THRESHOLD,
        zconned::mail::DEFAULT_OPK_BURN_DEPTH,
    )
    .map_err(|e| JsValue::from_str(&format!("keyring open: {e}")))?;
    // The peers book opens BEFORE the kit (B6-c1): the landing
    // verifies record signatures against the SAME instance the
    // transport upserts — one in-memory state.
    let peers = Arc::new(
        zconned::transport::PeerBook::open_with("/tafy/store", storage_dyn.clone())
            .map_err(|e| JsValue::from_str(&format!("peers open: {e}")))?,
    );
    // The core's own identity: the card (the self_get answer) plus
    // the detached record-signing capability (the commit-time
    // endorsement; the day-one self-sign uses it silently).
    let identity = zconned::session::Identity::builder()
        .card(zconned::transport::peer_face_of(
            &keyring.public_set(),
            Some(keyring.record_sign_public_bytes()),
        ))
        .record_sign(keyring.record_signer())
        .build();
    step("[worker] keyring open, identity built");
    let kit = zconned::session::ArrivalKit::open("/tafy/store", storage_dyn.clone(), peers.clone())
        .map_err(|e| JsValue::from_str(&format!("store open: {e}")))?;

    let outbox = Arc::new(
        zconned::mail::Outbox::open_with("/tafy/store", storage_dyn)
            .map_err(|e| JsValue::from_str(&format!("outbox open: {e}")))?,
    );
    let spawner: Arc<dyn zconned::transport::Spawner> = Arc::new(WasmSpawner);
    // an unset relay = a never-opened endpoint (visits are explicit;
    // the open would fail only if the UI asked for one)
    let endpoint = if relay.trim().is_empty() {
        "https://relay.not-configured.invalid".to_string()
    } else {
        relay.trim().to_string()
    };
    let mut transport = zconned::transport::Transport::builder()
        .carrier(tafy_carrier::GrpcWebCarrier::new(endpoint))
        .spawner(spawner.clone())
        .keyring(Arc::new(std::sync::Mutex::new(keyring)))
        .landing(Box::new(zconned::session::RecordLanding::new(
            kit.store.clone(),
            peers.clone(),
            kit.registry.clone(),
            kit.notices_tx.clone(),
        )))
        .outbox(outbox)
        .peers(peers)
        .notices(Some(kit.notices_tx.clone()))
        .build();
    let sender = transport.sender();
    step("[worker] transport built, sender wired");
    let scope_for_task = scope.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let mut core_in_read = tokio::io::BufReader::new(core_in_read);
        let result = zconned::run_with_arrivals(
            kit,
            sender,
            Some(identity),
            spawner,
            &mut core_in_read,
            core_out_write,
        )
        .await;
        let step = move |m: &str| {
            let _ = scope_for_task.post_message(&JsValue::from_str(m));
        };
        step("[worker] core task started");
        if let Err(e) = result {
            web_sys::console::error_1(&format!("core exited: {e}").into());
        }
    });

    // Inbound: UI frames → the core's stdin side (the UI frames them).
    wasm_bindgen_futures::spawn_local(async move {
        use tokio::io::AsyncWriteExt as _;
        while let Some(frame) = in_rx.recv().await {
            if core_in_write.write_all(&frame).await.is_err() {
                break;
            }
        }
    });

    // Outbound: the core's stdout side → deframe → one postMessage
    // per frame (the UI receives discrete frames).
    step("[worker] bridges up");
    let mut deframer = protobuf_varint_deframer::Deframer::builder().build();
    let mut chunk = [0u8; 4096];
    use tokio::io::AsyncReadExt as _;
    let scope_out = scope.clone();
    wasm_bindgen_futures::spawn_local(async move {
        loop {
            match core_out_read.read(&mut chunk).await {
                Ok(0) => {
                    let _ = scope_out.post_message(&JsValue::from_str("[worker] core stdout EOF"));
                    break;
                }
                Ok(n) => {
                    // THE DRAIN (the reload-loss class bug, batch-1
                    // residual): the deframer's feed stops at ONE
                    // frame and leaves the rest of the read buffer
                    // untouched — a pump's frames coalescing into a
                    // single read used to DESTROY every frame after
                    // the first (the commit pair: msg_package_push arrived,
                    // its MsgDraftPush twin silently died -> the UI's
                    // reply matcher hung forever). Feed until the
                    // buffer is exhausted.
                    let mut data: &[u8] = &chunk[..n];
                    loop {
                        match deframer.feed(&mut data) {
                            Ok(Some(frame)) => {
                                let arr = js_sys::Uint8Array::from(frame.as_ref());
                                let _ = port.post_message(&arr);
                                if data.is_empty() {
                                    break;
                                }
                            }
                            Ok(None) => break, // the buffer is exhausted
                            Err(e) => {
                                web_sys::console::error_1(&format!("deframe: {e}").into());
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    web_sys::console::error_1(&format!("core stdout: {e}").into());
                    break;
                }
            }
        }
    });
    // The control task: owns the transport + the ctrl port; drives
    // visits; the conn/diag faces ride the port (the UI parses them).
    conn_face(&ctrl_port, "disconnected", "");
    wasm_bindgen_futures::spawn_local(async move {
        while let Some(cmd) = ctrl_rx.recv().await {
            match cmd {
                Ctrl::Boot { .. } => {}
                Ctrl::Diag => {
                    let _ = ctrl_port.post_message(&JsValue::from_str("diag: START"));
                    let text = crate::storage::diag_face().await;
                    let _ = ctrl_port.post_message(&JsValue::from_str(&text));
                }
                Ctrl::Connect { relay } => {
                    conn_face(&ctrl_port, "connecting", &relay);
                    transport.repoint(tafy_carrier::GrpcWebCarrier::new(relay));
                    // THE VISIT GATE: the ceremony normally returns in
                    // tens of seconds (the tunnel's unary latency × the
                    // call ladder); a relay whose plane streams never
                    // flush (reported upstream) holds visit() forever —
                    // the dropped future aborts its fetch (the client's
                    // abort guard), the face fails loudly, the queued
                    // mail stays for the next visit. The attach-open
                    // flake is absorbed INSIDE the carrier (short open
                    // timeouts + prompt retries — see the carrier).
                    tokio::select! {
                        r = transport.visit() => match r {
                            Ok(()) => conn_face(&ctrl_port, "connected", ""),
                            Err(e) => {
                                conn_face(&ctrl_port, "error", &e.to_string());
                            }
                        },
                        _ = wasm_sleep(VISIT_TIMEOUT_MS) => {
                            conn_face(&ctrl_port, "error", &format!("the visit ran past its {}s gate — the relay's plane stream never answered (reported upstream)", VISIT_TIMEOUT_MS / 1000));
                        }
                    }
                }
                Ctrl::Disconnect => {
                    transport.disconnect();
                    conn_face(&ctrl_port, "disconnected", "");
                }
            }
        }
    });

    Ok::<(), JsValue>(())
}

/// The storage self-test body (the batch-1 acceptance of the storage
/// face, independent of the core).
async fn run_selftest() -> Result<(), JsValue> {
    use zconned::storage::Storage as _;
    fn jsv<T, E: std::fmt::Display>(r: Result<T, E>) -> Result<T, JsValue> {
        r.map_err(|e| JsValue::from_str(&format!("{e}")))
    }
    // a FRESH root per run: the selftest must not seed from the
    // accumulated state of earlier failed runs (learned the hard way:
    // the pollution masked the real failure)
    let root = format!("/tafy/selftest-{}", js_sys::Date::now() as u64);
    let scope: web_sys::DedicatedWorkerGlobalScope = js_sys::global().unchecked_into();
    let mark = |scope: &web_sys::DedicatedWorkerGlobalScope, m: &str| {
        let _ = scope.post_message(&JsValue::from_str(m));
    };
    let storage = crate::storage::OpfsStorage::open(&root).await?;
    mark(&scope, "selftest: opened");
    let p = std::path::Path::new("probe/probe.jsonl");

    // append + read
    mark(&scope, "selftest: appending 1");
    jsv(storage.append(p, b"{\"row\":1}\n"))?;
    mark(&scope, "selftest: appended 1");
    jsv(storage.append(p, b"{\"row\":2}\n"))?;
    mark(&scope, "selftest: appended 2");
    let want: Vec<u8> = b"{\"row\":1}\n{\"row\":2}\n".to_vec();
    assert_eq!(jsv(storage.read(p))?, want);
    // write replaces whole
    mark(&scope, "selftest: writing");
    jsv(storage.write(p, b"{\"row\":3}\n"))?;
    mark(&scope, "selftest: written");
    assert_eq!(jsv(storage.read(p))?, b"{\"row\":3}\n".to_vec());
    // exists
    assert!(jsv(storage.exists(p))?);
    // rename (the atomic-replace shape)
    mark(&scope, "selftest: renaming");
    jsv(storage.rename(p, std::path::Path::new("probe/probe2.jsonl")))?;
    mark(&scope, "selftest: renamed");
    mark(&scope, "selftest: exists-check");
    assert!(!jsv(storage.exists(p))?);
    mark(&scope, "selftest: reread-src");
    match jsv(storage.read(std::path::Path::new("probe/probe2.jsonl"))) {
        Ok(v) => {
            mark(&scope, "selftest: reread-src ok");
            assert_eq!(v, b"{\"row\":3}\n".to_vec());
        }
        Err(e) => {
            mark(&scope, &format!("selftest: REREAD-SRC FAIL {e:?}"));
            return Err(e);
        }
    }
    // dirs
    mark(&scope, "selftest: dirs-check");
    let dirs = jsv(storage.read_dir_dirs(std::path::Path::new("")))?;
    mark(&scope, &format!("selftest: dirs-root={dirs:?}"));
    assert_eq!(dirs, vec!["probe".to_string()]);
    let dirs = jsv(storage.read_dir_dirs(std::path::Path::new("probe")))?;
    assert_eq!(dirs, Vec::<String>::new());

    // Drain the persistence queue, then verify OPFS REALLY has it:
    // a fresh open re-walks the tree from disk.
    mark(&scope, "selftest: pre-drain");
    drain_until_idle(&storage).await;
    if let Some(e) = storage.last_persist_error.lock().unwrap().clone() {
        mark(&scope, &format!("selftest: DRAIN ERROR {e}"));
        return Err(JsValue::from_str(&format!("drain failed: {e}")));
    }
    mark(&scope, "selftest: drained");
    let storage2 = crate::storage::OpfsStorage::open(&root).await?;
    mark(&scope, "selftest: reopened");
    match jsv(storage2.read(std::path::Path::new("probe/probe2.jsonl"))) {
        Ok(v) => {
            mark(&scope, "selftest: reread ok");
            assert_eq!(v, b"{\"row\":3}\n".to_vec());
        }
        Err(e) => {
            mark(&scope, &format!("selftest: REREAD FAIL {e:?}"));
            return Err(JsValue::from_str("selftest reread failed"));
        }
    }

    // cleanup: remove the probe tree's file (via a write of nothing
    // is wrong — use the OPFS face directly)
    cleanup(&root, "probe/probe2.jsonl").await;
    Ok(())
}

/// Let the drain task empty the persistence queue: yield until the
/// mirror has no pending ops (the drain empties it at quiesce).
async fn drain_until_idle(_storage: &Arc<crate::storage::OpfsStorage>) {
    // The drain runs at the next microtask boundary; a few macrotask
    // ticks cover it.
    for _ in 0..10 {
        wasm_bindgen_futures::spawn_local(async {});
        let tick = js_sys::Promise::new(&mut |res, _rej| {
            let set_timeout =
                js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("setTimeout")).unwrap();
            let f = js_sys::Function::from(set_timeout);
            // call the timer; resolve the promise from its own tick
            let _ = f.call1(&JsValue::NULL, &res);
        });
        let _ = JsFuture::from(tick).await;
    }
}

async fn cleanup(root: &str, file: &str) {
    let navigator: web_sys::Navigator =
        js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("navigator"))
            .ok()
            .unwrap()
            .unchecked_into();
    let Ok(opfs) = navigator.storage().get_directory().await else {
        return;
    };
    let opfs: web_sys::FileSystemDirectoryHandle = opfs.unchecked_into();
    if let Ok(r) = opfs
        .get_directory_handle_with_options(root, &web_sys::FileSystemGetDirectoryOptions::new())
        .await
    {
        let r: web_sys::FileSystemDirectoryHandle = r.unchecked_into();
        let comps: Vec<&str> = file.split('/').collect();
        let (name, dirs) = comps.split_last().unwrap();
        let mut dir = r;
        for d in dirs {
            if let Ok(next) = dir
                .get_directory_handle_with_options(
                    d,
                    &web_sys::FileSystemGetDirectoryOptions::new(),
                )
                .await
            {
                dir = next.unchecked_into();
            }
        }
        let _ = dir.remove_entry(name).await;
    }
}

/// TEMP diag: the live store's mirror (files + sizes) + the last
/// persistence failure — the reload-loss hunt's probe.
#[wasm_bindgen]
pub fn tafy_worker_storage_diag() {
    wasm_bindgen_futures::spawn_local(async move {
        let scope: web_sys::DedicatedWorkerGlobalScope = js_sys::global().unchecked_into();
        let text = crate::storage::diag_face().await;
        let _ = scope.post_message(&JsValue::from_str(&text));
    });
}
