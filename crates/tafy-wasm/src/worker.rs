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
//! BLOCKED-FOR-THE-CORE (batch 1, 2026-11-10): `zconned::host::
//! run_with_arrivals` hardcodes `tokio::spawn(write_pump(..))` —
//! tokio::spawn requires a tokio runtime, and no tokio runtime can
//! run on wasm (the spike: std thread-park panics; JS events cannot
//! wake a parked block_on). The negotiated change = the pump goes
//! through the `Spawner` trait (already in the library since the
//! carrier round). REPORTED to 106 (A17a), never patched here.
//! Until it lands, the worker offers the STORAGE SELF-TEST face —
//! the OPFS storage face's full round trip, no core needed.

use std::sync::Arc;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

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

    let reply = std::cell::RefCell::new(Some(port_tx));
    let onmessage = Closure::wrap(Box::new(move |e: web_sys::MessageEvent| {
        // the ONLY message on the worker scope = the transferred port;
        // the frames themselves ride PORT2 (its onmessage is wired
        // after the transfer below)
        let ports = e.ports();
        let first = ports.get(0);
        if !first.is_undefined() {
            let p: web_sys::MessagePort = first.unchecked_into();
            if let Some(tx) = reply.borrow_mut().take() {
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
    step("[worker] port in, wiring port onmessage");

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
    // Batch 1 = the stdio-only core shape: the detached carrier
    // (no relay visits); the real carrier arrives in batch 3.
    let sender = zconned::transport::Sender::<zconned::transport::Detached>::disabled();

    // The core: the arrival kit opens on OUR storage face; the pump
    // runs on the WasmSpawner (the 106 seam since 1b94be7 — the
    // negotiated ask, landed). The kit's notices stream is a tokio
    // mpsc receiver (Send, executor-independent) — fine here. The
    // identity: the keyring lazily creates itself at
    // `<root>/keyring.json` on first use (Keyring::open_with); the
    // card rides the host-supplied `card` param (the stdio-only core
    // has no mail identity of its own). The sender = the detached
    // carrier (stdio-only semantics) for batch 1; the notices bus
    // stays pure stdio until batch 3 wires the transport's ack
    // statements.
    let storage_dyn: zconned::SharedStorage = storage;
    let kit = zconned::session::ArrivalKit::open("/tafy/store", storage_dyn.clone())
        .map_err(|e| JsValue::from_str(&format!("store open: {e}")))?;
    let keyring = zconned::mail::Keyring::open_with(
        "/tafy/store",
        storage_dyn,
        zconned::mail::DEFAULT_OPK_POOL_SIZE,
        zconned::mail::DEFAULT_OPK_REPLENISH_THRESHOLD,
        zconned::mail::DEFAULT_OPK_BURN_DEPTH,
    )
    .map_err(|e| JsValue::from_str(&format!("keyring open: {e}")))?;
    let card = zconned::transport::peer_face_of(&keyring.public_set());
    step("[worker] keyring open, card built");
    let scope_for_task = scope.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let mut core_in_read = tokio::io::BufReader::new(core_in_read);
        let result = zconned::run_with_arrivals(
            kit,
            sender,
            Some(card),
            std::sync::Arc::new(WasmSpawner),
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
    let scope_out = scope.clone();
    let mut deframer = protobuf_varint_deframer::Deframer::builder().build();
    let mut chunk = [0u8; 4096];
    use tokio::io::AsyncReadExt as _;
    loop {
        match core_out_read.read(&mut chunk).await {
            Ok(0) => break,
            Ok(n) => {
                let mut data: &[u8] = &chunk[..n];
                while let Some(frame) = deframer
                    .feed(&mut data)
                    .map_err(|e| JsValue::from_str(&format!("deframe: {e}")))?
                {
                    let _ = scope_out.post_message(&JsValue::from_str(&format!(
                        "[worker] outbound frame ({}B): {:02x?}",
                        frame.len(),
                        &frame.as_ref()[..frame.len().min(12)]
                    )));
                    let arr = js_sys::Uint8Array::from(frame.as_ref());
                    let _ = port.post_message(&arr);
                }
            }
            Err(e) => return Err(JsValue::from_str(&format!("core stdout: {e}"))),
        }
    }
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
