//! The OPFS-backed implementation of the storage SPI.
//!
//! Driving model (proven by the tokio-wasm spikes v1-v4, /dev/shm/
//! tokio-wasm-spike — conclusions mirrored in design/s2-real-core/
//! coarse.md R2): there is NO tokio runtime on wasm (the current-
//! thread runtime's park = std thread-park = unusable; `block_on`
//! deadlocks the moment a JS promise must wake it); everything runs
//! on the wasm-bindgen-futures executor. The `Storage` trait is SYNC
//! and must be callable from inside the core's tasks — so the
//! trait's working set is an IN-RAM TREE MIRROR of the OPFS store
//! (seeded by one async walk at boot; persisted write-through by a
//! lazily-resumed drain task). Cached `FileSystemSyncAccessHandle`s
//! make hot files fully synchronous (no drain round trip).
//!
//! Durability residuals (batch 1, recorded in the plan): a path
//! whose handle is not yet cached persists via the drain (FIFO over
//! one channel — order preserved); the drain runs at the next
//! executor quiesce point, so a killed tab can lose the tail of
//! not-yet-drained writes (the same order-of-magnitude window a
//! native fsync-less append has).

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc as std_mpsc};

use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    FileSystemDirectoryHandle, FileSystemFileHandle, FileSystemGetDirectoryOptions,
    FileSystemGetFileOptions, FileSystemSyncAccessHandle,
};

/// The shared handle the core's store components hold.
pub type SharedStorage = Arc<dyn zconned::storage::Storage>;

/// The in-RAM mirror of the store: files = whole contents; dirs =
/// the directory set (a directory that never contained a file is
/// equivalent to an absent one — the boot walk rebuilds dirs from
/// file paths, and the core's semantics only read dirs via their
/// files).
#[derive(Default)]
struct Tree {
    files: BTreeMap<PathBuf, Vec<u8>>,
    dirs: BTreeSet<PathBuf>,
}

impl Tree {
    /// Normalize a relative path: components only, no `..`/roots —
    /// the core's path shapes are simple relative joins, anything
    /// else is a bug to reject loudly.
    /// Normalize a storage path against the OPENED logical root. The
    /// host (the store) passes ROOT-JOINED paths ("/tafy/store/
    /// threads/x") while some callers may pass root-relative ones —
    /// both land as root-relative keys: strip a leading root marker,
    /// then strip the root's own segments when present, then the
    /// escape check. Empty output = the store root itself.
    fn norm(rel: &Path, root: &Path) -> io::Result<PathBuf> {
        let comps: Vec<Component> = rel
            .components()
            .skip_while(|c| matches!(c, Component::RootDir))
            .collect();
        let root_comps: Vec<Component> = root
            .components()
            .filter(|c| !matches!(c, Component::RootDir))
            .collect();
        let mut i = 0;
        while i < root_comps.len()
            && i < comps.len()
            && comps[i].as_os_str() == root_comps[i].as_os_str()
        {
            i += 1;
        }
        let rest = &comps[i..];
        let mut out = PathBuf::new();
        for c in rest {
            match c {
                Component::Normal(s) => out.push(s.to_string_lossy().into_owned()),
                Component::CurDir => {}
                _ => {
                    return Err(io::Error::other(format!(
                        "storage path escapes the tree: {rel:?}"
                    )));
                }
            }
        }
        Ok(out)
    }

    fn mkdirs(&mut self, file: &Path) {
        for a in file.ancestors().skip(1) {
            self.dirs.insert(a.to_path_buf());
        }
    }
}

/// One persistence op for the drain task.
enum Op {
    /// Whole-file write (also the first half of a rename).
    Write { rel: PathBuf, data: Vec<u8> },
    /// Append at the current end.
    Append { rel: PathBuf, data: Vec<u8> },
    /// Remove a file (the second half of a rename).
    Remove { rel: PathBuf },
}

pub struct OpfsStorage {
    /// The OPFS root directory this store hangs under.
    root: FileSystemDirectoryHandle,
    /// The logical root path as opened: the host joins ITS paths onto
    /// this prefix; norm strips it back off.
    root_path: PathBuf,
    /// The in-RAM mirror the sync trait serves from.
    tree: Mutex<Tree>,
    /// The persistence channel: sync-send from the trait, drained by
    /// the drain task (spawn_local, lazily resumed — see `enqueue`).
    ops_tx: std_mpsc::Sender<Op>,
    /// The last persistence failure (diagnostics; the drain is
    /// fire-and-forget by design — the mirror stays the truth).
    pub(crate) last_persist_error: Mutex<Option<String>>,
    /// The receive end (Mutex: Receiver is not Sync; the drain is
    /// the only consumer, the lock = the Sync bridge).
    rx: Mutex<std_mpsc::Receiver<Op>>,
    /// Whether a drain task is currently queued or running.
    draining: AtomicBool,
    /// The self-ref the drain task needs (set right after open()).
    me: OnceLock<std::sync::Weak<OpfsStorage>>,
}

/// TEMP diag: the newest opened store (the diag export reads its
/// mirror + the persist failure).
pub(crate) static ACTIVE_STORAGE: OnceLock<Arc<OpfsStorage>> = OnceLock::new();

/// TEMP diag: the live store's mirror (files + sizes) + the last
/// persistence failure — the reload-loss hunt's probe.
pub(crate) async fn diag_face() -> String {
    let Some(store) = ACTIVE_STORAGE.get() else {
        return "diag: no active store".to_string();
    };
    let t = store.tree.lock().unwrap();
    let mut lines = vec![format!("diag: root={:?}", store.root_path)];
    for (p, b) in t.files.iter() {
        lines.push(format!(
            "  file {:?} ({}B): {:?}",
            p,
            b.len(),
            String::from_utf8_lossy(&b[..b.len().min(80)])
        ));
    }
    for d in t.dirs.iter() {
        lines.push(format!("  dir {:?}", d));
    }
    drop(t);
    if let Some(e) = store.last_persist_error.lock().unwrap().as_ref() {
        lines.push(format!("  LAST-PERSIST-ERROR: {e}"));
    }
    lines.join("\n")
}

impl OpfsStorage {
    /// Open (creating if needed) the OPFS root and seed the mirror
    /// from what is already on disk. Must run INSIDE a worker
    /// (sync access handles are worker-only).
    pub async fn open(root_path: &str) -> Result<Arc<Self>, JsValue> {
        let arc = Self::open_inner(root_path).await?;
        let _ = ACTIVE_STORAGE.set(arc.clone());
        Ok(arc)
    }

    async fn open_inner(root_path: &str) -> Result<Arc<Self>, JsValue> {
        let navigator: web_sys::Navigator =
            js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("navigator"))?
                .unchecked_into();
        let opfs_root: FileSystemDirectoryHandle =
            navigator.storage().get_directory().await?.unchecked_into();
        let opts = FileSystemGetDirectoryOptions::new();
        opts.set_create(true);
        // the logical root ("/tafy/store") = a PATH — but OPFS handle
        // names are SINGLE components (no slashes): traverse segment
        // by segment, creating as needed
        let mut root: FileSystemDirectoryHandle = opfs_root;
        for seg in root_path.split('/').filter(|s| !s.is_empty()) {
            root = root
                .get_directory_handle_with_options(seg, &opts)
                .await?
                .unchecked_into();
        }

        // Seed the mirror: one recursive walk of the existing tree.
        let mut tree = Tree::default();
        walk(&root, PathBuf::new(), &mut tree).await?;

        let (tx, rx) = std_mpsc::channel::<Op>();
        let arc = Arc::new(Self {
            root,
            root_path: PathBuf::from(root_path),
            tree: Mutex::new(tree),
            ops_tx: tx,
            last_persist_error: Mutex::new(None),
            rx: Mutex::new(rx),
            draining: AtomicBool::new(false),
            me: OnceLock::new(),
        });
        let _ = arc.me.set(Arc::downgrade(&arc));
        Ok(arc)
    }

    /// Queue one persistence op and make sure a drain task exists.
    /// The channel send is a plain sync send; `start_drain` is a
    /// no-op when a drain is already queued/running (FIFO order
    /// over the single channel is preserved either way).
    fn enqueue(&self, op: Op) {
        let _ = self.ops_tx.send(op);
        self.start_drain();
    }

    fn start_drain(&self) {
        if self.draining.swap(true, Ordering::SeqCst) {
            return; // a drain is queued or running
        }
        let Some(me) = self.me.get().and_then(|w| w.upgrade()) else {
            return;
        };
        wasm_bindgen_futures::spawn_local(async move {
            let rx = me.rx.lock().unwrap();
            loop {
                let op = match rx.try_recv() {
                    Ok(op) => op,
                    Err(_) => break, // empty: stop; the next enqueue restarts
                };
                if let Err(e) = me.persist(op).await {
                    *me.last_persist_error.lock().unwrap() = Some(format!("{e:?}"));
                    log::warn!("storage persist failed: {e:?}");
                    // The mirror stays the source of truth for the
                    // session; the op is dropped (batch-1 residual:
                    // a persistent OPFS failure shows up as missing
                    // rows on the next boot).
                }
            }
            me.draining.store(false, Ordering::SeqCst);
        });
    }

    async fn persist(&self, op: Op) -> Result<(), JsValue> {
        match op {
            Op::Write { rel, data } => {
                // open per op and CLOSE when done: a sync access
                // handle LOCKS its file — a left-open handle blocks
                // every later handle on the same file (the boot walk
                // included) with NoModificationAllowedError
                let h = self.resolve_handle(&rel).await?;
                let r = async {
                    h.write_at(0, &data)?;
                    h.truncate(data.len() as u64)?;
                    h.flush_js()
                }
                .await;
                let _ = h.close_js();
                r?;
            }
            Op::Append { rel, data } => {
                let h = self.resolve_handle(&rel).await?;
                let r = async {
                    let end = h.size()?;
                    h.write_at(end, &data)?;
                    h.flush_js()
                }
                .await;
                let _ = h.close_js();
                r?;
            }
            Op::Remove { rel } => {
                // an open access handle keeps its file locked (the
                // removeEntry below throws NoModificationAllowedError
                // on it) — the open-per-op discipline above means no
                // handle of ours is ever left open here
                let mut comps = components(&rel);
                let name = comps.pop();
                if let Some(name) = name {
                    let dir = self.dir_at(&comps, true).await?;
                    dir.remove_entry(&name).await?;
                }
            }
        }
        Ok(())
    }

    /// Resolve a file's sync handle (creating the file and its
    /// parent chain) and cache it — the hot path for every later op.
    async fn resolve_handle(&self, rel: &Path) -> Result<FileSystemSyncAccessHandle, JsValue> {
        let comps = components(rel);
        let (name, dirs) = comps
            .split_last()
            .ok_or_else(|| JsValue::from_str("empty path"))?;
        let dir = self.dir_at(dirs, true).await?;
        let fopts = FileSystemGetFileOptions::new();
        fopts.set_create(true);
        let fh: FileSystemFileHandle = dir
            .get_file_handle_with_options(name, &fopts)
            .await?
            .unchecked_into();
        let h: FileSystemSyncAccessHandle = JsFuture::from(fh.create_sync_access_handle())
            .await?
            .unchecked_into();
        Ok(h)
    }

    async fn dir_at(
        &self,
        comps: &[String],
        create: bool,
    ) -> Result<FileSystemDirectoryHandle, JsValue> {
        let mut dir = self.root.clone();
        for d in comps {
            let dopts = FileSystemGetDirectoryOptions::new();
            dopts.set_create(create);
            dir = dir
                .get_directory_handle_with_options(d, &dopts)
                .await?
                .unchecked_into();
        }
        Ok(dir)
    }
}

fn components(rel: &Path) -> Vec<String> {
    rel.components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect()
}

/// The sync-handle primitives via Reflect — web-sys 0.3.107's typed
/// surface for these drifted across versions; the IDL is stable, so
/// the calls go straight through it. The primitives are SYNCHRONOUS
/// (that is the whole point of sync access handles); the typed
/// web-sys methods that return Promises are avoided.
trait SyncHandleExt {
    fn write_at(&self, at: u64, data: &[u8]) -> Result<(), JsValue>;
    fn size(&self) -> Result<u64, JsValue>;
    fn truncate(&self, len: u64) -> Result<(), JsValue>;
    fn flush_js(&self) -> Result<(), JsValue>;
    fn close_js(&self) -> Result<(), JsValue>;
    /// Read exactly `buf.len()` bytes starting at `at`.
    fn read_exact_at(&self, buf: &mut [u8], at: u64) -> Result<(), JsValue>;
}

fn call_method(target: &JsValue, name: &str, args: &[JsValue]) -> Result<JsValue, JsValue> {
    let f = js_sys::Reflect::get(target, &JsValue::from_str(name))?;
    if f.is_undefined() {
        return Err(JsValue::from_str(format!("no method {name}").as_str()));
    }
    js_sys::Function::from(f).apply(target, &js_sys::Array::from_iter(args.iter()))
}

impl SyncHandleExt for FileSystemSyncAccessHandle {
    fn write_at(&self, at: u64, data: &[u8]) -> Result<(), JsValue> {
        let opts = js_sys::Object::new();
        js_sys::Reflect::set(
            &opts,
            &JsValue::from_str("at"),
            &JsValue::from_f64(at as f64),
        )?;
        let buf = js_sys::Uint8Array::from(data);
        call_method(self, "write", &[buf.into(), opts.into()])?;
        Ok(())
    }

    fn size(&self) -> Result<u64, JsValue> {
        let v = call_method(self, "getSize", &[])?;
        try_into_u64(&v)
    }

    fn truncate(&self, len: u64) -> Result<(), JsValue> {
        call_method(self, "truncate", &[JsValue::from_f64(len as f64)])?;
        Ok(())
    }

    fn flush_js(&self) -> Result<(), JsValue> {
        call_method(self, "flush", &[])?;
        Ok(())
    }

    fn close_js(&self) -> Result<(), JsValue> {
        call_method(self, "close", &[])?;
        Ok(())
    }

    fn read_exact_at(&self, buf: &mut [u8], at: u64) -> Result<(), JsValue> {
        let view = js_sys::Uint8Array::new_with_length(buf.len() as u32);
        let opts = js_sys::Object::new();
        js_sys::Reflect::set(
            &opts,
            &JsValue::from_str("at"),
            &JsValue::from_f64(at as f64),
        )?;
        call_method(self, "read", &[view.clone().into(), opts.into()])?;
        view.copy_to(buf);
        Ok(())
    }
}

fn try_into_u64(v: &JsValue) -> Result<u64, JsValue> {
    v.as_f64()
        .map(|f| f as u64)
        .ok_or_else(|| JsValue::from_str("expected a number"))
}

/// Recursive boot walk: every file under `dir` lands in the mirror.
/// `entries()` yields an async iterator; js_sys's try_iter covers
/// only sync iterators, so the next() promises are awaited by hand.
fn walk<'a>(
    dir: &'a FileSystemDirectoryHandle,
    prefix: PathBuf,
    tree: &'a mut Tree,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), JsValue>> + 'a>> {
    Box::pin(walk_inner(dir, prefix, tree))
}

async fn walk_inner(
    dir: &FileSystemDirectoryHandle,
    prefix: PathBuf,
    tree: &mut Tree,
) -> Result<(), JsValue> {
    let iterator = call_method(&dir.clone().into(), "entries", &[])?;
    loop {
        let next = call_method(&iterator, "next", &[])?;
        let result = JsFuture::from(js_sys::Promise::<JsValue>::unchecked_from_js(next)).await?;
        let done = js_sys::Reflect::get(&result, &JsValue::from_str("done"))?;
        if try_into_bool(&done)? {
            break;
        }
        let value = js_sys::Reflect::get(&result, &JsValue::from_str("value"))?;
        let pair: js_sys::Array = value.unchecked_into();
        let name: String = pair
            .get(0)
            .unchecked_into::<js_sys::JsString>()
            .as_string()
            .unwrap_or_default();
        let handle = pair.get(1);
        let path = prefix.join(&name);
        if handle.is_instance_of::<FileSystemDirectoryHandle>() {
            let sub: FileSystemDirectoryHandle = handle.unchecked_into();
            tree.dirs.insert(path.clone());
            let tree = &mut *tree;
            walk(&sub, path, tree).await?;
        } else if handle.is_instance_of::<FileSystemFileHandle>() {
            let fh: FileSystemFileHandle = handle.unchecked_into();
            let h: FileSystemSyncAccessHandle = JsFuture::from(fh.create_sync_access_handle())
                .await?
                .unchecked_into();
            let len = h.size()?;
            let mut buf = vec![0u8; len as usize];
            h.read_exact_at(&mut buf, 0)?;
            h.close_js()?;
            tree.files.insert(path, buf);
        }
    }
    Ok(())
}

fn try_into_bool(v: &JsValue) -> Result<bool, JsValue> {
    v.as_bool()
        .ok_or_else(|| JsValue::from_str("expected a boolean"))
}

/// The `Storage` SPI implementation: sync over the RAM mirror,
/// write-through to OPFS. The core's path shapes are kept verbatim —
/// they are interpreted as tree paths under this store's root.
impl zconned::storage::Storage for OpfsStorage {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        let rel = Tree::norm(path, &self.root_path)?;
        match self.tree.lock().unwrap().files.get(&rel) {
            Some(bytes) => Ok(bytes.clone()),
            None => Err(io::Error::new(io::ErrorKind::NotFound, "no such file")),
        }
    }

    fn write(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        let rel = Tree::norm(path, &self.root_path)?;
        {
            let mut t = self.tree.lock().unwrap();
            t.files.insert(rel.clone(), data.to_vec());
            t.mkdirs(&rel);
        }
        self.enqueue(Op::Write {
            rel,
            data: data.to_vec(),
        });
        Ok(())
    }

    fn append(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        let rel = Tree::norm(path, &self.root_path)?;
        {
            let mut t = self.tree.lock().unwrap();
            t.files
                .entry(rel.clone())
                .or_default()
                .extend_from_slice(data);
            t.mkdirs(&rel);
        }
        self.enqueue(Op::Append {
            rel,
            data: data.to_vec(),
        });
        Ok(())
    }

    fn exists(&self, path: &Path) -> io::Result<bool> {
        let rel = Tree::norm(path, &self.root_path)?;
        let t = self.tree.lock().unwrap();
        Ok(t.files.contains_key(&rel) || t.dirs.contains(&rel))
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        let rel = Tree::norm(path, &self.root_path)?;
        self.tree.lock().unwrap().mkdirs(&rel);
        // No persistence op: a directory that holds no file is
        // equivalent to an absent one (the mirror rebuilds dirs from
        // file paths; the core never reads a dir without its files).
        Ok(())
    }

    fn read_dir_dirs(&self, path: &Path) -> io::Result<Vec<String>> {
        let rel = Tree::norm(path, &self.root_path)?;
        let t = self.tree.lock().unwrap();
        let prefix = if rel.as_os_str().is_empty() {
            PathBuf::new()
        } else {
            rel.clone()
        };
        Ok(t.dirs
            .iter()
            .filter_map(|d| d.strip_prefix(&prefix).ok())
            .filter(|r| r.components().count() == 1)
            .map(|r| {
                r.components()
                    .next()
                    .unwrap()
                    .as_os_str()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect())
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        let src = Tree::norm(from, &self.root_path)?;
        let dst = Tree::norm(to, &self.root_path)?;
        let data = {
            let mut t = self.tree.lock().unwrap();
            match t.files.remove(&src) {
                Some(data) => data,
                None => return Err(io::Error::new(io::ErrorKind::NotFound, "no such file")),
            }
            // THE MIRROR IS THE SESSION'S SOURCE OF TRUTH: the move
            // must land in the tree NOW (the drain only persists the
            // disk side — a mirror-less insert would make the moved
            // file vanish from every later session read)
        };
        {
            let mut t = self.tree.lock().unwrap();
            t.files.insert(dst.clone(), data.clone());
            t.mkdirs(&dst);
        }
        // The whole-state replace: the destination ends up either the
        // old complete content or the new complete content (native =
        // one atomic rename; here = a queued ordered pair).
        self.enqueue(Op::Write {
            rel: dst.clone(),
            data: data.clone(),
        });
        self.enqueue(Op::Remove { rel: src });
        Ok(())
    }

    fn write_private(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        // wasm expresses no permissions: the trait's own comment
        // makes this the plain-write case on this platform.
        self.write(path, data)
    }
}

#[cfg(test)]
impl OpfsStorage {
    fn mirror_for_test(tree: Tree) -> Self {
        let (tx, rx) = std_mpsc::channel();
        Self {
            // a JS-free root: the mirror tests never touch OPFS
            root: web_sys::FileSystemDirectoryHandle::from(JsValue::NULL),
            root_path: PathBuf::new(),
            tree: Mutex::new(tree),
            ops_tx: tx,
            last_persist_error: Mutex::new(None),
            rx: Mutex::new(rx),
            draining: AtomicBool::new(false),
            me: OnceLock::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zconned::storage::Storage as _;

    #[test]
    fn norm_rejects_escape_and_accepts_root() {
        let r = Path::new("/tafy/store");
        // root-relative forms pass through
        assert!(Tree::norm(Path::new("a/b/c"), r).is_ok());
        assert!(Tree::norm(Path::new("threads/x/transcript.jsonl"), r).is_ok());
        assert!(Tree::norm(Path::new("../escape"), r).is_err());
        assert!(Tree::norm(Path::new(""), r).is_ok()); // the store root
        // root-joined absolute forms (what the store passes) strip back
        let joined = Tree::norm(Path::new("/tafy/store/threads/x/t.jsonl"), r).unwrap();
        assert_eq!(joined, PathBuf::from("threads/x/t.jsonl"));
        // the host-joined root itself = the store root (dir ops)
        assert!(
            Tree::norm(Path::new("/tafy/store"), r)
                .unwrap()
                .as_os_str()
                .is_empty()
        );
        // escape still rejected in the joined form
        assert!(Tree::norm(Path::new("/tafy/store/../escape"), r).is_err());
    }

    #[test]
    fn read_dir_dirs_lists_immediate_children_only() {
        let mut t = Tree::default();
        t.files
            .insert(PathBuf::from("threads/a/transcript.jsonl"), vec![]);
        t.files
            .insert(PathBuf::from("threads/b/transcript.jsonl"), vec![]);
        t.dirs.insert(PathBuf::from("threads/a"));
        t.dirs.insert(PathBuf::from("threads/b"));
        t.dirs.insert(PathBuf::from("threads/a/deep"));
        t.dirs.insert(PathBuf::from("threads/a/deep/leaf"));
        t.dirs.insert(PathBuf::from("threads"));
        let st = OpfsStorage::mirror_for_test(t);
        let mut got = st.read_dir_dirs(Path::new("threads")).unwrap();
        got.sort();
        assert_eq!(got, vec!["a", "b"]);
    }
}
