// tafy-worker-diag.js — the DIAGNOSTIC face. Mode A (default): the
// real boot (tafy_worker_main). Mode B (selftest): the storage face's
// full round trip — picked by the ?mode=selftest query on the page.
import init, { tafy_worker_main, tafy_worker_storage_selftest } from "./tafy_wasm.js";
await init();
if (globalThis.location.search.includes("selftest")) {
  tafy_worker_storage_selftest();
} else {
  tafy_worker_main();
}
