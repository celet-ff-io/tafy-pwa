// tafy-worker.js — the wasm core's worker glue. Loaded as a MODULE
// worker by the PWA client (core.rs spawns it at the document base +
// /tafy-core/tafy-worker.js). It loads the wasm-bindgen web bundle
// (tafy_wasm.js, same directory), then enters tafy_worker_main:
// MessagePort bridge in, i2c frames out. The worker is the ONLY
// consumer of the OPFS storage face (sync access handles are
// worker-only); the single-instance guarantee lives on the UI side
// (the Web Locks guard) — this worker is spawned by the winning tab
// only, so one worker = one core = one writer.
//
// The files here are generated/staged build artifacts (the wasm
// bundle via wasm-bindgen 0.2.130; this glue is static) — regenerate
// with the runbook's build steps after every tafy-wasm change.

import init, { tafy_worker_main } from "./tafy_wasm.js";

await init();
tafy_worker_main();
