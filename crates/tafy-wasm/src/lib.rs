//! tafy-wasm — the WASM HOST of the zconned core.
//!
//! Architecture (design/s2-real-core/coarse.md §1): the core runs on
//! a dedicated worker thread; the UI layer (tafy-pwa) speaks the i2c
//! frame contract to it over a MessageChannel bridge. Inside this
//! crate:
//! - `storage` — the OPFS implementation of `zconned::storage::Storage`
//!   (the 8 primitives; the path vocabulary = OPFS tree paths under
//!   the root directory).
//! - `carrier` — the gRPC-Web implementation of the carrier traits
//!   (batch 3; not yet present).
//! - `worker` — the bootstrap: runtime + core (`host::run_with_storage`)
//!   + the duplex-to-MessageChannel bridge.

pub mod storage;
pub mod worker;
