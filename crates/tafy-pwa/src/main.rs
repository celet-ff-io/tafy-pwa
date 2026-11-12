// tafy-pwa — the zconn web PWA (the UI layer of the real-core era:
// it speaks the i2c frame contract to the wasm core, crate
// tafy-wasm, over a MessageChannel bridge).
// IA v3.1 (principal design, 2026-11-10): layout principle = put
// length-variant lists where NOTHING sits below them (same reason a
// thread is full-screen). Home topbar = the multi-function key and
// the brand grouped LEFT (a breath apart), Me at the right edge.
// AMENDED (principal, 2026-11-12): Home's key-side slot — EMPTY by
// IA v3.4 — now carries the relay conn status (a one-glyph pill;
// the full honest face rides the title attribute).
// The multi-function key keeps ONE geometry on every page that has
// it, with a two-state icon: hamburger on Home, close-X on the MENU
// page — pressing it again returns Home. The menu is a FULL PAGE
// (route /menu, not an overlay): a curated BOUNDED destination list
// whose Settings is the LAST entry (with nothing above it, it sits
// at the top, right below the key; future entries insert above it;
// if the list ever stopped being bounded, Settings would move to a
// pinned footer row — never below an unbounded list). Me page =
// identity + contacts on ONE page: Me and every contact are
// ordinary peers — the message level is objectified threads, not
// "DM"/"GM". Settings chain = Settings -> About -> License (rare +
// legal content sits deep). Thread = full-screen focused (no menu
// key). The message level knows only THREADS — "conversation" was
// a zconnor-connection term, dropped.
// The native face is TYPECHECK-ONLY (the app never runs natively —
// A17: the core lives in a browser worker; spawn_local is a stub).
// Dead-code truth therefore lives on the wasm32 check, where every
// fn is reachable; silence the analysis here rather than gate the
// shared code into copies.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

// Real-core era (S2 batch 1): the UI speaks the i2c frame protocol
// to the wasm core (tafy-wasm, in a dedicated worker) via core.rs —
// the mock seam below is RETIRED; the typed grammar (threads, the
// message stream) stays as the UI's view model. (A17 WASM-local
// architecture; A16 mock-era-only clause lifted by the principal.)

mod core;

mod ai;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::spawn_local;

use dioxus::prelude::*;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// The web clipboard: fire-and-forget (the promise's rejection cannot
/// be acted on — the principal ruled the clipboard universal on every
/// browser host; no file save/load fallback face exists on the web).
#[cfg(target_arch = "wasm32")]
fn copy_to_clipboard(t: String) {
    spawn_local(async move {
        if let Some(window) = web_sys::window() {
            let clipboard = window.navigator().clipboard();
            let _ = wasm_bindgen_futures::JsFuture::from(clipboard.write_text(&t)).await;
        }
    });
}

/// The relay strip's faces, sourced from the REAL visit state (the
/// worker's control-port statements; no probe theater).
fn conn_face(conn: &core::Conn, relay_unset: bool) -> String {
    match conn {
        core::Conn::Disconnected if relay_unset => "relay: not set".to_string(),
        core::Conn::Disconnected => "relay: not connected".to_string(),
        core::Conn::Connecting => "relay: connecting…".to_string(),
        core::Conn::Connected => "relay: connected ✓".to_string(),
        core::Conn::Error(e) => format!("relay: error ✗ {e}"),
    }
}

fn conn_class(conn: &core::Conn) -> &'static str {
    match conn {
        core::Conn::Connected => "relay-line ok",
        core::Conn::Error(_) => "relay-line bad",
        _ => "relay-line",
    }
}

/// The topbar's compact conn face (principal, 2026-11-12): a status
/// word right of the multi-function key on Home. The word is the
/// summary, never the whole truth — the honest full face rides the
/// title attribute (and the composer's strip carries the detail
/// line). WORDS not glyphs (the principal's field feedback,
/// 2026-11-12: the one-glyph face was too terse).
fn conn_pill(failed: bool, conn: &core::Conn) -> &'static str {
    let _ = failed;
    match conn {
        core::Conn::Connected => "online",
        core::Conn::Connecting => "connecting",
        core::Conn::Error(_) => "error",
        core::Conn::Disconnected => "offline",
    }
}

fn conn_pill_class(failed: bool, conn: &core::Conn) -> &'static str {
    if failed || matches!(conn, core::Conn::Error(_)) {
        return "conn-pill bad";
    }
    if matches!(conn, core::Conn::Connected) {
        return "conn-pill ok";
    }
    "conn-pill"
}

/// The strip's first-line face: a BOOT failure outranks the conn
/// face (a failed boot is why there is no conn at all — the honest
/// face for the deep-link spawn failure that used to sit silently at
/// 'not connected').
fn strip_face() -> String {
    if let core::Status::Failed(e) = &core::CORE.read().status {
        return format!("boot: failed ✗ {e}");
    }
    conn_face(&core::CORE.read().conn, relay_saved().trim().is_empty())
}

/// The strip's class for the first-line face (the boot failure gets
/// the bad face too).
fn strip_class() -> &'static str {
    if matches!(core::CORE.read().status, core::Status::Failed(_)) {
        return "relay-line bad";
    }
    conn_class(&core::CORE.read().conn)
}

/// The strip's second-line face.
fn strip_sub() -> &'static str {
    if matches!(core::CORE.read().status, core::Status::Failed(_)) {
        return "the core failed to boot — reload the page (the console carries the detail)";
    }
    conn_sub(&core::CORE.read().conn)
}

fn conn_sub(conn: &core::Conn) -> &'static str {
    match conn {
        // THE SEND GATE face (the principal, 2026-11-12): the strip
        // reports; the tap-to-disconnect affordance is RETIRED. The
        // protocol's persist-then-queue still covers a sent commit
        // that failed mid-visit (it re-enters the seal task).
        // CONNECTED = NO SUB (the principal's field feedback,
        // 2026-11-12: 'sending through the relay' was redundant —
        // the first line already says relay: connected ✓); the
        // empty string keeps the sub slot's height so the strip
        // never jumps when the state flips.
        core::Conn::Connected => "",
        _ => "the send key unlocks once the relay connects",
    }
}

// native check face (see core.rs): compile-only stub.
#[cfg(not(target_arch = "wasm32"))]
fn copy_to_clipboard(t: String) {
    let _ = t;
}

#[cfg(target_arch = "wasm32")]
fn save_relay(url: &str) {
    if let Some(w) = web_sys::window()
        && let Ok(Some(storage)) = w.local_storage()
    {
        let _ = storage.set_item(RELAY_KEY, url);
    }
}
#[cfg(not(target_arch = "wasm32"))]
fn save_relay(_url: &str) {}

// native check face (see core.rs): compile-only stub — the app never
// runs natively (A17: the core lives in a browser worker).
#[cfg(not(target_arch = "wasm32"))]
fn spawn_local<F: std::future::Future<Output = ()> + 'static>(f: F) {
    let _ = f;
}

fn main() {
    dioxus::launch(App);
}

// ---- single-instance guard (R33 door: multi-tab; the wasm core is a
// single writer over the browser storage pool — a second tab must not
// boot a second core) ----
// The guard is the browser Web Locks API: the first instance holds the
// named lock for its lifetime; a second one boots the degrade view.
// Mock-era note: today's shell is stateless, so the guard is scaffolded
// now to make the invariant real before state exists.

#[derive(Clone, Copy, Debug, PartialEq)]
enum Instance {
    Primary,
    Secondary,
}

#[cfg(target_arch = "wasm32")]
const CORE_LOCK: &str = "tafy-core";

// Hand-rolled minimal LockManager binding: web-sys gates the typed
// bindings behind `web_sys_unstable_apis`; a raw 3-line bind is
// version-proof and needs no unstable cfg.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
unsafe extern "C" {
    #[wasm_bindgen(js_name = "LockManager")]
    type Locks;
    #[wasm_bindgen(method, js_name = "request")]
    fn request(
        this: &Locks,
        name: &str,
        options: &js_sys::Object,
        callback: &js_sys::Function,
    ) -> js_sys::Promise;
}

#[cfg(target_arch = "wasm32")]
async fn instance_role() -> Instance {
    use wasm_bindgen_futures::JsFuture;

    // Pre-LockManager browsers (or non-window contexts) proceed
    // unlocked: the guard is best-effort platform protection, never a
    // bricking gate.
    let Some(window) = web_sys::window() else {
        return Instance::Primary;
    };
    let navigator = window.navigator();
    let Ok(locks_val) = js_sys::Reflect::get(&navigator, &"locks".into()) else {
        return Instance::Primary;
    };
    if !locks_val.is_object() {
        return Instance::Primary;
    }
    let locks: Locks = locks_val.unchecked_into();

    // `granted` settles TRUE only when the lock was granted (its
    // resolver is captured and fired from the grant callback).
    let resolver = std::rc::Rc::new(std::cell::RefCell::new(None));
    let r2 = resolver.clone();
    let granted = js_sys::Promise::new(&mut |resolve, _| {
        *r2.borrow_mut() = Some(resolve);
    });

    // Grant callback: fire `granted`, then return a promise that never
    // settles — an unsettled callback return is what HOLDS the lock
    // until the tab closes. `forget` = the holder lives as long as the
    // lock does.
    let r3 = resolver.clone();
    let on_grant = wasm_bindgen::closure::Closure::<dyn FnMut(JsValue) -> js_sys::Promise>::new(
        move |_lock: JsValue| {
            if let Some(resolve) = r3.borrow_mut().take() {
                let _ = resolve.call1(&JsValue::NULL, &JsValue::TRUE);
            }
            js_sys::Promise::new(&mut |_, _| {})
        },
    );
    let opts = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&opts, &"ifAvailable".into(), &JsValue::TRUE);
    let request = locks.request(CORE_LOCK, &opts, on_grant.as_ref().unchecked_ref());
    on_grant.forget();

    // Race: granted => TRUE settles first; blocked => the request
    // promise settles undefined (the callback never ran, the lock is
    // held elsewhere).
    let pair = js_sys::Array::of2(&granted, &request);
    match JsFuture::from(js_sys::Promise::race(&pair.into())).await {
        Ok(v) if v == JsValue::TRUE => Instance::Primary,
        _ => Instance::Secondary,
    }
}

#[cfg(not(target_arch = "wasm32"))]
async fn instance_role() -> Instance {
    // native hosts (tests) are always the only instance
    Instance::Primary
}

// the second instance's degrade view: it must NOT mount the app
// (no router, no future core, no storage writes).
#[component]
fn AlreadyRunning() -> Element {
    rsx! {
        div { class: "thread-stream",
            p { class: "hint", "tAfY is already running in another tab or window — this page is stopped (single-writer guard)" }
            p { class: "hint", "Close the other tAfY page and refresh this one to continue" }
        }
    }
}

#[derive(Clone, Routable, Debug, PartialEq)]
enum Route {
    #[route("/")]
    Home {},
    #[route("/me")]
    Me {},
    #[route("/settings")]
    Settings {},
    #[route("/about")]
    About {},
    #[route("/ai")]
    AiPage {},
    #[route("/help")]
    Help {},
    #[route("/menu")]
    Menu {},
    #[route("/thread/:id")]
    ThreadPage { id: String },
    #[route("/:..segments")]
    NotFound { segments: Vec<String> },
}

#[component]
fn App() -> Element {
    let mut role = use_signal(|| None::<Instance>);
    use_future(move || async move {
        role.set(Some(instance_role().await));
    });
    // the core client boots once, on the primary instance only
    use_effect(move || {
        core::boot();
    });
    rsx! {
        document::Link { rel: "stylesheet", href: asset!("/assets/main.css") }
        match role() {
            // the sub-millisecond boot frame renders nothing
            None => rsx! { {} },
            Some(Instance::Primary) => rsx! { Router::<Route> {} },
            Some(Instance::Secondary) => rsx! { AlreadyRunning {} },
        }
    }
}

/// Hand-rolled civil-from-days formatter (no chrono dep in the shell).
/// LOCAL TIMEZONE (the principal, 2026-11-12): ts_ms is a correct
/// epoch; the old civil math assumed UTC and rendered UTC wall time.
/// On the wasm face the shift = the host's own offset (js Date gives
/// UTC-minus-local MINUTES); the native face (tests) stays UTC —
/// no tz source there, documented.
fn fmt_ts(ms: u64) -> String {
    let ms = ms as i64;
    #[cfg(target_arch = "wasm32")]
    let ms = {
        let off_min = js_sys::Date::new_0().get_timezone_offset(); // f64 minutes
        ms - (off_min as i64) * 60_000 // UTC-local shift -> the local wall clock
    };
    let secs = ms.div_euclid(1000);
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let (hh, mm) = (rem / 3600, (rem % 3600) / 60);
    format!("{:04}-{:02}-{:02} {:02}:{:02}", y, m, d, hh, mm)
}

// ---- display helpers over the core state ----

/// A thread id -> the display label: the digest's first 8 hex bytes
/// in two groups (the full ref never reads well at row scale).
fn short_ref(id: &str) -> String {
    let digest = id.split(':').nth(3).unwrap_or("");
    let h = &digest[..digest.len().min(16)];
    match h.len() / 2 {
        0 => id.to_string(),
        n => h[..n * 2]
            .as_bytes()
            .chunks(4)
            .map(|c| String::from_utf8_lossy(c).to_string())
            .collect::<Vec<_>>()
            .join(" "),
    }
}

/// The Home row's sub line: the thread's last loaded message, else a
/// quiet ref echo (the log loads lazily on open).
fn thread_preview(id: &str) -> String {
    core::CORE
        .read()
        .logs
        .get(id)
        // the preview SKIPS stamp rows (a stamp tail would render an
        // empty line — the timestamp message carries no words); a
        // stamp-only thread keeps the honest dash
        .and_then(|rows| rows.iter().rev().find(|r| !r.stamp).map(|r| r.text.clone()))
        .unwrap_or_else(|| "—".to_string())
}

/// The Me page's fingerprint face: the card = base64(prost Peer);
/// the fingerprint = the F1 short notation (the digest's first 8
/// bytes, grouped — zconned_crypto::Fingerprint::short, the canon
/// helper; the raw sign_pub prefix derivation is retired).
/// The self identity's DECODED face: the fingerprint is DISPLAY-ONLY
/// (107's rule — never a copy source; the card is the copy unit) and
/// the row states the algorithm pair (also 107's popup face).
fn self_peer(card: &str) -> Option<zconned_stdio_types::Peer> {
    use base64::Engine as _;
    use prost::Message as _;
    base64::engine::general_purpose::STANDARD
        .decode(card.trim())
        .ok()
        .and_then(|bytes| zconned_stdio_types::Peer::decode(bytes.as_slice()).ok())
}

fn self_fingerprint(card: &str) -> String {
    self_peer(card)
        .and_then(|peer| crate::core::fp_of_peer(&peer))
        .unwrap_or_else(|| "···· ···· ···· ····".to_string())
}

// ---- the topbar shared by list-level pages (role-split) ----
// The multi-function key keeps ONE geometry on every page that has
// it, with a two-state icon: hamburger (opens the menu page) vs
// close-X (returns Home). Role split (principal, IA v3.2): the
// brand (wordmark + version) shows ONLY on the menu page — Home
// leaves that slot EMPTY; the Me button shows ONLY on Home.

#[component]
fn TopBar(menu: bool) -> Element {
    // compile-time constant from Cargo.toml — the single version source
    let version = env!("CARGO_PKG_VERSION");
    // the conn pill's sources (read in the body — reactive; the
    // clone keeps borrowck simple across the two face helpers)
    let failed = matches!(core::CORE.read().status, core::Status::Failed(_));
    let conn = core::CORE.read().conn.clone();
    rsx! {
        header { class: "topbar",
            if menu {
                // on the menu page the key's icon flips to close-X:
                // pressing it again returns Home
                Link { to: Route::Home {}, class: "icon-btn", title: "Close",
                    svg { fill: "none", stroke: "currentColor", stroke_width: "1.8", stroke_linecap: "round", stroke_linejoin: "round", view_box: "0 0 24 24",
                        path { d: "M18 6L6 18M6 6l12 12" }
                    }
                }
            } else {
                Link { to: Route::Menu {}, class: "icon-btn", title: "Menu",
                    svg { fill: "none", stroke: "currentColor", stroke_width: "1.8", stroke_linecap: "round", stroke_linejoin: "round", view_box: "0 0 24 24",
                        path { d: "M3 6h18M3 12h18M3 18h18" }
                    }
                }
            }
            if menu {
                // the brand belongs to the menu page: wordmark leads,
                // version trails small and dim — Home leaves it empty
                div { class: "brand",
                    h1 { class: "wordmark", "tAfY" }
                    span { class: "version", "v{version}" }
                }
            }
            if !menu {
                // the relay conn status right of the key (principal,
                // 2026-11-12 — the slot was EMPTY by IA v3.4, amended
                // by this mandate; WORDS not glyphs, 2026-11-12 field
                // feedback). Word = the summary; the honest full face
                // rides the title.
                span {
                    class: conn_pill_class(failed, &conn),
                    title: "{strip_face()}",
                    "{conn_pill(failed, &conn)}",
                }
                div { class: "topbar-actions",
                    Link { to: Route::Me {}, class: "icon-btn", title: "Me",
                        svg { fill: "none", stroke: "currentColor", stroke_width: "1.8", stroke_linecap: "round", stroke_linejoin: "round", view_box: "0 0 24 24",
                            path { d: "M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2" }
                            circle { cx: "12", cy: "7", r: "4" }
                        }
                    }
                }
            }
        }
    }
}

// ---- P-Home: multi-function key + brand + Me, over a free thread list ----

#[component]
fn Home() -> Element {
    let status = core::CORE.read().status.clone();
    let threads = core::CORE.read().threads.clone();
    // the for-loop below moves the vec; the trailing-row gate reads
    // this snapshot instead
    let has_threads = !threads.is_empty();
    let booting = status == core::Status::Booting;
    let failed = match &status {
        core::Status::Failed(e) => Some(e.clone()),
        _ => None,
    };
    rsx! {
        div { class: "page",
            TopBar { menu: false }
            if let Some(e) = failed {
                p { class: "page-hint", "Core failed: {e}" }
            } else if booting {
                p { class: "page-hint", "Booting the core…" }
            } else {
                div { class: "list",
                    if threads.is_empty() {
                        // the honest empty state: the first thread
                        // is born from its first message
                        Link { to: Route::ThreadPage { id: "new".to_string() }, class: "row",
                            div { class: "row-main",
                                span { class: "row-title", "Start the first thread" }
                                span { class: "row-sub", "It appears here once its first message lands" }
                            }
                        }
                    }
                    for t in threads {
                        Link { key: "{t.id}", to: Route::ThreadPage { id: t.id.clone() }, class: "row",
                            div { class: "row-main",
                                span { class: "row-title", "{short_ref(&t.id)}" }
                                span { class: "row-sub", "{thread_preview(&t.id)}" }
                            }
                        }
                    }
                    // the persistent start row, pinned at the list's
                    // very bottom (principal, 2026-11-12): a non-empty
                    // list never loses the new-thread affordance (the
                    // born-here composer is otherwise unreachable).
                    // An empty list needs no second row — the empty
                    // state above IS the start affordance.
                    if has_threads {
                        Link { to: Route::ThreadPage { id: "new".to_string() }, class: "row",
                            div { class: "row-main",
                                span { class: "row-title", "Start new thread" }
                                span { class: "row-sub", "Write its first message — it appears here" }
                            }
                        }
                    }
                }
            }
        }
    }
}

// ---- P-Me: identity hint + contacts list (variable, nothing below) ----

#[component]
fn Me() -> Element {
    rsx! {
        div { class: "page",
            header { class: "page-head",
                Link { to: Route::Home {}, class: "back", "‹" }
                strong { "Me" }
            }
            p { class: "page-hint", "Your card:" }
            match core::CORE.read().self_card.clone() {
                Some(card) => rsx! {
                    div { class: "list",
                        div { class: "row",
                            div { class: "row-main",
                                span { class: "row-title", "This device" }
                                span { class: "row-sub", "{self_sub(&card)}" }
                            }
                        }
                        div { class: "row",
                            div { class: "row-main",
                                span { class: "row-title", "My card" }
                            }
                            CopyBtn { text: card.clone(), label: "copy" }
                        }
                        div { class: "card-blob", "{card}" }
                    }
                },
                None => rsx! {
                    p { class: "page-hint", "Identity not generated yet — wait for the core to finish booting" }
                },
            }
            Contacts {}
            Authors {}
        }
    }
}

// ---- the identity-registry face (B7 c2; the principal's rulings,
// 2026-11-12): the roster of observed anchors + the local remarks.
// A remark names EVERY identity a message carries — the author slot
// and every verified signer. The remark is a LOCAL annotation (the
// core's contract): only the SELF remark rides outgoing packages;
// the display = what THIS device named them, never what they claim.
#[component]
fn Authors() -> Element {
    let editing = use_signal(|| None::<String>);
    let draft = use_signal(String::new);
    let err = use_signal(String::new);
    // the roster grows with observed anchors and the core only
    // pushes on the verbs — re-ask on every mount
    use_effect(core::identities_refresh);
    let ids = core::CORE.read().identities.clone();
    let self_anchor = core::CORE.read().self_anchor.clone();
    rsx! {
        p { class: "page-hint", "People ({ids.len()}):" }
        div { class: "list",
            for id in ids.iter() {
                AuthorsRow {
                    key: "{id.anchor_hex}",
                    id: id.clone(),
                    self_anchor: self_anchor.clone(),
                    editing,
                    draft,
                    err,
                }
            }
            if ids.is_empty() {
                p { class: "page-hint", "No identities observed yet — they appear as messages arrive" }
            }
        }
    }
}

#[component]
fn AuthorsRow(
    id: core::IdRow,
    self_anchor: Option<String>,
    editing: Signal<Option<String>>,
    mut draft: Signal<String>,
    mut err: Signal<String>,
) -> Element {
    let is_self = self_anchor.as_deref() == Some(id.anchor_hex.as_str());
    // the name face: the remark when set, else the F1 short form of
    // the anchor itself
    let name = id
        .remark
        .clone()
        .unwrap_or_else(|| core::short_anchor(&id.anchor_hex));
    let is_editing = editing.read().as_deref() == Some(id.anchor_hex.as_str());
    // per-closure captures (a move closure takes its variable): one
    // hex clone per button
    let hex_edit = id.anchor_hex.clone();
    let hex_clear = id.anchor_hex.clone();
    let rem = id.remark.clone();
    let has_remark = id.remark.is_some();
    rsx! {
        div { class: "row",
            div { class: "row-main",
                span { class: "row-title", if is_self { "{name} (this device)" } else { "{name}" } }
                span { class: "row-sub",
                    if is_self && id.remark.is_some() {
                        "the name peers see on your messages"
                    } else if id.remark.is_some() {
                        "remark set — tap rename to change, clear to remove"
                    } else {
                        "no remark — messages show the fingerprint"
                    }
                }
            }
            if is_editing {
                div { class: "paste-box",
                    input {
                        r#type: "text",
                        placeholder: "A name for this identity",
                        value: "{draft}",
                        oninput: move |e| draft.set(e.value()),
                    }
                    if !err.read().is_empty() {
                        p { class: "paste-err", "{err}" }
                    }
                    button {
                        class: "copy-btn wide",
                        disabled: draft.read().trim().is_empty(),
                        onclick: move |_| {
                            let hex = id.anchor_hex.clone();
                            let text = draft.read().trim().to_string();
                            spawn(async move {
                                match core::set_remark(&hex, &text).await {
                                    Ok(()) => {
                                        editing.set(None);
                                        draft.set(String::new());
                                        err.set(String::new());
                                    }
                                    Err(e) => err.set(e),
                                }
                            });
                        },
                        "Save"
                    }
                    button {
                        class: "copy-btn wide",
                        onclick: move |_| {
                            editing.set(None);
                            err.set(String::new());
                        },
                        "Cancel"
                    }
                }
            } else {
                button {
                    class: "copy-btn",
                    onclick: move |_| {
                        draft.set(rem.clone().unwrap_or_default());
                        err.set(String::new());
                        editing.set(Some(hex_edit.clone()));
                    },
                    "rename"
                }
                if has_remark {
                    button {
                        class: "copy-btn",
                        onclick: move |_| {
                            let hex = hex_clear.clone();
                            spawn(async move {
                                match core::reset_remark(&hex).await {
                                    Ok(()) => err.set(String::new()),
                                    Err(e) => err.set(e),
                                }
                            });
                        },
                        "clear"
                    }
                }
            }
        }
    }
}

// ---- the contacts face (107's W2 ruled UX, web-adapted: the
// paste-in is the add channel — the browser clipboard is the
// transport both ways; no file channels) ----

/// The self row's sub: fingerprint + the algo pair (107's popup face;
/// the fp is display-only — the card is the copy unit).
fn self_sub(card: &str) -> String {
    let mut sub = format!("fingerprint {}", self_fingerprint(card));
    if let Some(p) = self_peer(card) {
        sub.push_str(&format!("  \u{b7}  {} / {}", p.sign_algo, p.dh_algo));
    }
    sub
}

fn peer_sub(p: &core::PeerRow) -> String {
    let mut sub = format!("{} / {}", p.sign_algo, p.dh_algo);
    if !p.endpoints.is_empty() {
        sub.push_str(&format!("  \u{b7}  {}", p.endpoints.join(", ")));
    }
    // 107's B6 pitfall 3: the third key must not vanish at the
    // display face — an old card (no record_sign_pub) means this
    // peer's record signatures stay unverifiable forever until the
    // card is re-exchanged; the user must SEE that.
    if !p.has_record_key {
        sub.push_str("  \u{b7}  no record key");
    }
    sub
}

#[component]
fn Contacts() -> Element {
    let mut adding = use_signal(|| false);
    let mut paste = use_signal(String::new);
    let mut err = use_signal(String::new);
    let peers = core::CORE.read().peers.clone();
    rsx! {
        p { class: "page-hint", "Contacts ({peers.len()}):" }
        div { class: "list",
            for p in peers.iter() {
                div { class: "row",
                    div { class: "row-main",
                        span { class: "row-title", "fingerprint {p.fp}" }
                        span { class: "row-sub", "{peer_sub(p)}" }
                    }
                    CopyBtn { text: p.card.clone(), label: "copy" }
                }
            }
            div { class: "row row-btn",
                button {
                    class: "copy-btn wide",
                    onclick: move |_| {
                        adding.set(!adding());
                        err.set(String::new());
                    },
                    if *adding.read() { "Cancel" } else { "+ add contact" }
                }
            }
            if *adding.read() {
                div { class: "paste-box",
                    textarea {
                        class: "paste-area",
                        placeholder: "Paste their card here",
                        rows: "4",
                        value: "{paste}",
                        oninput: move |e| paste.set(e.value()),
                    }
                    if !err.read().is_empty() {
                        p { class: "paste-err", "{err}" }
                    }
                    button {
                        class: "copy-btn wide",
                        disabled: paste.read().trim().is_empty(),
                        onclick: move |_| {
                            let card = paste.read().trim().to_string();
                            spawn(async move {
                                match core::add_peer(&card).await {
                                    Ok(_) => {
                                        paste.set(String::new());
                                        adding.set(false);
                                    }
                                    Err(e) => err.set(e),
                                }
                            });
                        },
                        "Add"
                    }
                }
            }
        }
    }
}

/// The one-click copy face (the 107 acceptance lesson, web-adapted):
/// long identity strings never rely on the screen — the clipboard IS
/// the transport (every browser host is assumed to have full
/// clipboard support; no file save/load channels on the web).
#[component]
fn CopyBtn(text: String, label: String) -> Element {
    let mut copied = use_signal(|| false);
    rsx! {
        button {
            class: "copy-btn",
            onclick: move |_| {
                copy_to_clipboard(text.clone());
                copied.set(true);
            },
            if *copied.read() { "✓" } else { "{label}" }
        }
    }
}

// ---- P-Settings: the node's own configuration + static rows ----

// the relay address: the node's own configuration (the self card
// carries empty endpoints — the proto's stated rule). The setting
// lives in localStorage (per-browser origin, ours); the carrier leg
// (batch 3) reads it when assembling the Transport.
static RELAY_KEY: &str = "tafy.relay_url";

#[cfg(target_arch = "wasm32")]
fn relay_saved() -> String {
    web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item(RELAY_KEY).ok().flatten())
        .unwrap_or_default()
}
#[cfg(not(target_arch = "wasm32"))]
fn relay_saved() -> String {
    String::new()
}

// SHOW RECEIVED-MESSAGE TIMES (the principal, 2026-11-12): the
// meta.ts line under each message becomes OPTIONAL — a Settings
// toggle. DEFAULT OFF (the principal, pre-deploy: the meta.ts lines
// eat screen space in volume; the stamps — the ts FEATURE — always
// render). Flipping ON restores the per-message time lines.
static SHOW_RECV_TS_KEY: &str = "tafy.show_recv_ts";

#[cfg(target_arch = "wasm32")]
fn show_recv_ts() -> bool {
    web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item(SHOW_RECV_TS_KEY).ok().flatten())
        .map(|v| v == "1")
        .unwrap_or(false)
}
#[cfg(not(target_arch = "wasm32"))]
fn show_recv_ts() -> bool {
    false
}

#[cfg(target_arch = "wasm32")]
fn set_show_recv_ts(v: bool) {
    if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = s.set_item(SHOW_RECV_TS_KEY, if v { "1" } else { "0" });
    }
}
#[cfg(not(target_arch = "wasm32"))]
fn set_show_recv_ts(_v: bool) {}

// ---- the received-time toggle (the principal, 2026-11-12): the
// meta.ts line under each message becomes optional. Default ON —
// the shipped face is preserved; the toggle's purpose is the ability
// to hide. The draft-signal shape (read faces, write-through LS).
#[component]
fn RecvTsRow() -> Element {
    let mut on_d = use_signal(show_recv_ts);
    rsx! {
        div { class: "row row-config",
            div { class: "row-main",
                span { class: "row-title", "Show received-message times" }
                span { class: "row-sub", "The time under each message (sender stamps always show)" }
            }
            button {
                class: "switch-btn",
                onclick: move |_| {
                    let next = !on_d();
                    set_show_recv_ts(next);
                    on_d.set(next);
                },
                if on_d() { "ON" } else { "OFF" }
            }
        }
    }
}

#[component]
fn RelayRow() -> Element {
    let mut draft = use_signal(relay_saved);
    let mut err = use_signal(String::new);
    rsx! {
        div { class: "row row-config",
            div { class: "row-main",
                span { class: "row-title", "Relay address" }
                span { class: "row-sub",
                    if draft.read().is_empty() { "Not set — the carrier leg will need it" } else { "{draft}" } }
            }
        }
        div { class: "paste-box",
            input {
                class: "paste-area",
                placeholder: "https:// or wss:// (the relay's browser face)",
                value: "{draft}",
                oninput: move |e| {
                    err.set(String::new());
                    draft.set(e.value());
                },
            }
            if !err.read().is_empty() {
                p { class: "paste-err", "{err}" }
            }
            // the test/save pair (principal, 2026-11-12): Test starts
            // a REAL visit with the current setting and shows the
            // outcome in place below — the full ceremony (claim ->
            // planes) IS the connectivity truth; no probe theater.
            div { class: "btn-row",
                button {
                    class: "copy-btn wide",
                    onclick: move |_| {
                        // the draft if edited, else the saved address;
                        // what gets tested is what gets saved
                        let url = draft.read().trim().to_string();
                        let url = if url.is_empty() { relay_saved() } else { url };
                        err.set(String::new());
                        let ok = url.starts_with("https://")
                            || url.starts_with("wss://");
                        if !ok {
                            err.set(
                                "set a https:// or wss:// relay address first"
                                    .to_string(),
                            );
                            return;
                        }
                        save_relay(&url);
                        core::relay_connect(url);
                    },
                    "Test"
                }
                button {
                    class: "copy-btn wide",
                    onclick: move |_| {
                        let url = draft.read().trim().to_string();
                        // The carrier ruling (principal, alpha.14): the
                        // relay's browser face is gRPC-WEB over https;
                        // the deployed relay speaks wss. Both encrypted
                        // schemes are accepted — plain ws/http is not (a
                        // https page could not open them anyway).
                        let ok = url.starts_with("https://")
                            || url.starts_with("wss://");
                        if !ok {
                            err.set(
                                "the relay address must start with https:// or wss://"
                                    .to_string(),
                            );
                            return;
                        }
                        save_relay(&url);
                        draft.set(url.clone());
                        // saving = connecting: the visit starts right away
                        // (the strip on the threads reports the outcome)
                        core::relay_connect(url);
                    },
                    "Save"
                }
            }
            // the live outcome, in place (the REAL visit state — the
            // same face the composer's strip carries; a boot failure
            // outranks the conn face)
            span { class: strip_class(), "{strip_face()}" }
            span { class: "relay-sub", "{strip_sub()}" }
        }
    }
}

#[component]
fn Settings() -> Element {
    // back = the menu page in BOTH the tap and the system swipe: the
    // tap replaces this entry with Menu (no stacked history), the
    // swipe pops to it (principal, alpha.8)
    let nav = use_navigator();
    rsx! {
        div { class: "page",
            header { class: "page-head",
                a { class: "back", href: "#", onclick: move |e| {
                    e.prevent_default();
                    nav.replace(Route::Menu {});
                }, "‹" }
                strong { "Settings" }
            }
            div { class: "list",
                div { class: "row",
                    div { class: "row-main",
                        span { class: "row-title", "Identity" }
                        span { class: "row-sub", "Not configured — key model pending the D2 round" }
                    }
                }
                RelayRow {}
                RecvTsRow {}
                Link { to: Route::AiPage {}, class: "row",
                    div { class: "row-main",
                        span { class: "row-title", "AI" }
                        span { class: "row-sub", "Endpoint and AI features" }
                    }
                }
                Link { to: Route::About {}, class: "row",
                    div { class: "row-main",
                        span { class: "row-title", "About" }
                        span { class: "row-sub", "License and copyright" }
                    }
                }
            }
        }
    }
}

// ---- P-Menu: the multi-function page (full page, not an overlay) ----
// The curated bounded destination list. Settings is the LAST entry of
// the list — with nothing above it (today) it sits at the top, right
// below the key. Future entries insert above it; the list stays
// bounded; if it ever stopped being bounded, Settings would move to a
// pinned footer row (never below an unbounded list).

#[component]
fn Menu() -> Element {
    rsx! {
        div { class: "page",
            TopBar { menu: true }
            div { class: "list",
                // normal push: Settings sits ON TOP of the menu in
                // history, so the system swipe (left edge = history
                // back) returns to the MENU — back = the page you
                // came from (principal, alpha.8; the alpha.7 reading
                // "back = Home" is retracted by the principal)
                Link { to: Route::Settings {}, class: "row",
                    // one lead group so .row's space-between can't
                    // spread icon and text to opposite edges
                    div { class: "row-lead",
                    div { class: "row-icon",
                        svg { fill: "none", stroke: "currentColor", stroke_width: "1.8", stroke_linecap: "round", stroke_linejoin: "round", view_box: "0 0 24 24",
                            circle { cx: "12", cy: "12", r: "3" }
                            path { d: "M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z" }
                        }
                    }
                        div { class: "row-main",
                            span { class: "row-title", "Settings" }
                        }
                    }
                }
            }
        }
    }
}

// ---- P-About: the legal face (chain: Settings -> About -> License) ----
// License sits deep: rare + legal content. The copyright line is the
// repo LICENSE's display mirror (A18 closed posture); the © line uses
// the UTF-8 mark; the year range is 2026-present — static hosting may
// outlive active development, the page keeps serving as-is.

#[component]
fn About() -> Element {
    // compile-time constant from Cargo.toml — the single version source
    let version = env!("CARGO_PKG_VERSION");
    rsx! {
        div { class: "page",
            header { class: "page-head",
                Link { to: Route::Settings {}, class: "back", "‹" }
                strong { "About" }
            }
            p { class: "page-hint", "tAfY — the zconned web client" }
            p { class: "page-hint", "v{version}" }
            div { class: "list",
                // the usage instructions (the principal, 2026-11-12:
                // the PWA is PUBLIC test software — the help face is
                // part of the product, not an afterthought)
                Link { to: Route::Help {}, class: "row",
                    div { class: "row-main",
                        span { class: "row-title", "Help" }
                        span { class: "row-sub", "How to use tAfY" }
                    }
                }
                // licenses.html = generated by cargo-about; regenerate
                // whenever the dependency set changes before deploying
                // (rules.md A18 — every deploy is a distribution).
                a { class: "row", href: "./licenses.html",
                    div { class: "row-main",
                        span { class: "row-title", "Open-source licenses" }
                        span { class: "row-sub", "Full license texts of third-party components" }
                    }
                }
            }
            p { class: "copyright", "Copyright © 2026-present tAfY Developers. All rights reserved." }
        }
    }
}

// ---- P-Help: the usage instructions (public test software) ----
// The principal, 2026-11-12: the PWA is PUBLIC test software — a Help
// entry under About carries the usage instructions. Content = the
// app's OWN vocabulary only (threads, visits, the relay); every claim
// is a shipped behavior, no promises. English per the standing
// UI-language ruling (the i18n switch stays queued).
#[component]
fn Help() -> Element {
    rsx! {
        div { class: "page",
            header { class: "page-head",
                Link { to: Route::About {}, class: "back", "‹" }
                strong { "Help" }
            }
            p { class: "page-hint", "How to use tAfY" }
            div { class: "list",
                div { class: "row help",
                    div { class: "row-main",
                        span { class: "row-title", "Threads" }
                        span { class: "row-sub",
                            "The Home screen lists your threads. Start a new one from the row at the list's very bottom — a thread is born from its first message. Type in the composer and tap the arrow to send." }
                    }
                }
                div { class: "row help",
                    div { class: "row-main",
                        span { class: "row-title", "Timestamps" }
                        span { class: "row-sub",
                            "When a thread has been quiet for a while, a timestamp line is committed before your next message — that standalone dim line attests the time your device sent it." }
                    }
                }
                div { class: "row help",
                    div { class: "row-main",
                        span { class: "row-title", "People" }
                        span { class: "row-sub",
                            "Your identity lives on the Me screen — the fingerprint is for comparing, the card is for sharing. Copy your card with one tap; add a contact by pasting theirs. The People list on Me names every identity your device has seen: give any of them a remark and messages show that name instead of the fingerprint (only you see your remarks; your own remark is what peers see on your messages). A received message with no signature from its author is marked untrusted." }
                    }
                }
                div { class: "row help",
                    div { class: "row-main",
                        span { class: "row-title", "The relay" }
                        span { class: "row-sub",
                            "Messages travel through a relay during visits. Set the relay address in Settings and use Test to check it. The pill next to the menu key on Home shows the connection: connected, connecting, trouble, or not connected. Sending needs a live connection — the send key lights up only while connected, and a message that could not leave keeps trying on the next visit." }
                    }
                }
                div { class: "row help",
                    div { class: "row-main",
                        span { class: "row-title", "Deleting a thread" }
                        span { class: "row-sub",
                            "The Delete button in a thread removes THIS device's copy (tap twice to confirm). An arriving message re-creates the thread." }
                    }
                }
                div { class: "row help",
                    div { class: "row-main",
                        span { class: "row-title", "Translation" }
                        span { class: "row-sub",
                            "With translation enabled in Settings → AI, every message gains a translate button; the translation shows under the message and stays on this device only — it is never sent to your peers. Translating sends the message (with a few preceding messages as context) to the AI endpoint YOU configured, in plaintext — pick an endpoint you trust." }
                    }
                }
                div { class: "row help",
                    div { class: "row-main",
                        span { class: "row-title", "Installing" }
                        span { class: "row-sub",
                            "tAfY is an installable web app: use your browser's add-to-home-screen to keep it like an app. Your data stays on this device." }
                    }
                }
            }
        }
    }
}

// ---- P-AI: the AI big item (the principal, 2026-11-12) ----
// The ENDPOINT config lives at the item level (shared by every
// future AI feature); Translation is PART 1. The design =
// design/ai-features/coarse.md; the storage ruling = everything
// PWA-local (localStorage, the relay-url precedent). The disclosure
// faces are mandatory (the honesty posture): enabling translation
// sends message content to the user's chosen endpoint in plaintext.
#[component]
fn AiPage() -> Element {
    // the toggle drafts: the dioxus lesson (the principal's toggle
    // feedback, 2026-11-12) — a signal written but NEVER READ in the
    // rsx re-renders nothing (the old write-only 'tick' hack left the
    // labels and the master's hide gate stale until a remount). The
    // faces derive from READ signals, kept in sync with localStorage
    // on every write (the same shape as the field drafts below).
    let mut enabled_d = use_signal(|| ai::cfg().enabled);
    let mut auto_d = use_signal(|| ai::cfg().auto);
    // the endpoint drafts (write-through on every keystroke)
    let mut url_d = use_signal(|| ai::cfg().url);
    let mut key_d = use_signal(|| ai::cfg().key);
    let mut model_d = use_signal(|| ai::cfg().model);
    let mut lang_d = use_signal(|| ai::cfg().lang);
    let mut n_d = use_signal(|| ai::cfg().n.to_string());
    // the endpoint test's live face (the relay-Test shape)
    let mut testing = use_signal(|| false);
    let mut test_face = use_signal(String::new);
    let do_test = move |_| {
        let cfg = ai::cfg();
        if !ai::endpoint_ready(&cfg) {
            test_face
                .set("endpoint: failed ✗ — set the base URL, the key, and the model first".into());
            return;
        }
        testing.set(true);
        test_face.set("endpoint: testing…".into());
        // the component-scoped spawn (the established pattern): the
        // writes land in this page's signals
        spawn(async move {
            let face = match ai::test_endpoint(&cfg).await {
                Ok(ms) => format!("endpoint: ok ✓ {ms}"),
                Err(e) => format!("endpoint: failed ✗ {e}"),
            };
            testing.set(false);
            test_face.set(face);
        });
    };
    rsx! {
        div { class: "page",
            header { class: "page-head",
                Link { to: Route::Settings {}, class: "back", "‹" }
                strong { "AI" }
            }
            p { class: "page-hint", "AI features — powered by your own endpoint" }
            div { class: "list",
                div { class: "row row-config",
                    div { class: "row-main",
                        span { class: "row-title", "Endpoint" }
                        span { class: "row-sub", "OpenAI-format; shared by every AI feature" }
                    }
                }
                div { class: "paste-box",
                    input {
                        class: "paste-area",
                        placeholder: "Base URL (e.g. https://api.openai.com/v1)",
                        value: "{url_d}",
                        oninput: move |e| {
                            ai::set_url(e.value().trim());
                            url_d.set(e.value());
                        },
                    }
                    input {
                        class: "paste-area",
                        r#type: "password",
                        placeholder: "API key",
                        value: "{key_d}",
                        oninput: move |e| {
                            ai::set_key(&e.value());
                            key_d.set(e.value());
                        },
                    }
                    input {
                        class: "paste-area",
                        placeholder: "Model (e.g. gpt-4o-mini)",
                        value: "{model_d}",
                        oninput: move |e| {
                            ai::set_model(&e.value());
                            model_d.set(e.value());
                        },
                    }
                    div { class: "btn-row",
                        button { class: "copy-btn wide", disabled: testing(), onclick: do_test, "Test" }
                    }
                    span { class: "relay-sub", "{test_face}" }
                    p { class: "ai-hint",
                        "The endpoint must allow browser CORS. Requests carry YOUR message content in plaintext — choose an endpoint you trust."
                    }
                }
                div { class: "row row-config",
                    div { class: "row-main",
                        span { class: "row-title", "Translation" }
                        span { class: "row-sub", "AI translation of messages, part 1 of the AI features" }
                    }
                    button {
                        class: "switch-btn",
                        onclick: move |_| {
                            let next = !enabled_d();
                            ai::set_enabled(next);
                            enabled_d.set(next);
                        },
                        if enabled_d() { "ON" } else { "OFF" }
                    }
                }
                if enabled_d() {
                    div { class: "row row-config",
                        div { class: "row-main",
                            span { class: "row-title", "Auto-translate" }
                            span { class: "row-sub", "Translate each arriving message as if pressed" }
                        }
                        button {
                            class: "switch-btn",
                            onclick: move |_| {
                                let next = !auto_d();
                                ai::set_auto(next);
                                auto_d.set(next);
                            },
                            if auto_d() { "ON" } else { "OFF" }
                        }
                    }
                    div { class: "paste-box",
                        input {
                            class: "paste-area",
                            placeholder: "Target language (e.g. Simplified Chinese)",
                            value: "{lang_d}",
                            oninput: move |e| {
                                ai::set_lang(&e.value());
                                lang_d.set(e.value());
                            },
                        }
                        input {
                            class: "paste-area",
                            r#type: "number",
                            placeholder: "Context n (1-20; the target + the (n-1) preceding messages)",
                            value: "{n_d}",
                            oninput: move |e| {
                                let n = e.value().parse::<usize>().unwrap_or(ai::cfg().n).clamp(1, ai::MAX_N);
                                ai::set_n(n);
                                n_d.set(e.value());
                            },
                        }
                        p { class: "ai-hint",
                            "Translations are this device only — never sent to your peers. Each message's translation is requested with the preceding n-1 messages as context."
                        }
                    }
                }
            }
        }
    }
}

// ---- P-Thread: full-screen focused thread ----
// id == "new" = the born-here flow: an empty focused page whose
// composer starts a FRESH chain (cursor fresh — the core generates
// the genesis parent); the commit receipt's entry ref BECOMES the
// thread's id and the page navigates there. A real id = the thread
// page proper: the log loads on mount (msg_threads_log_get); the
// composer commits onto the thread's leaf.

#[component]
fn ThreadPage(id: String) -> Element {
    let nav = use_navigator();
    let is_new = id == "new";

    // mount: load the log once (real threads only). The fetch keys
    // off the boot gate: a deep-link mount races the boot (the ask
    // used to die with 'core not booted' and pend forever) — the
    // effect re-fires when `booted` flips true.
    let mut loaded = use_signal(|| false);
    let id_for_load = id.clone();
    use_effect(move || {
        if is_new || loaded() {
            return;
        }
        if !core::CORE.read().booted {
            return;
        }
        loaded.set(true);
        core::load_thread(id_for_load.clone());
    });

    // the composer: text -> the commit chain (single-flight); on a
    // fresh commit the page navigates to the new thread's real id
    let mut draft = use_signal(String::new);
    let mut sending = use_signal(|| false);
    let id_for_send = id.clone();
    let submit = move |e: dioxus::prelude::Event<dioxus::prelude::FormData>| {
        e.prevent_default();
        let text = draft().trim().to_string();
        // TEMP trace (retire list pending): wasm32-only — web-sys is
        // a wasm32-gated dep, the native check face must stay clean
        #[cfg(target_arch = "wasm32")]
        web_sys::console::log_1(&format!("[composer] submit: len={}", text.len()).into());
        if text.is_empty() || sending() {
            #[cfg(target_arch = "wasm32")]
            web_sys::console::log_1(&"[composer] submit: rejected (empty or in-flight)".into());
            return;
        }
        // THE SEND GATE (the principal, 2026-11-12): no relay
        // connection -> no send, even around a disabled button
        // (Enter still submits the form).
        if !matches!(core::CORE.read().conn, core::Conn::Connected) {
            #[cfg(target_arch = "wasm32")]
            web_sys::console::log_1(&"[composer] submit: rejected (relay not connected)".into());
            return;
        }
        sending.set(true);
        #[cfg(target_arch = "wasm32")]
        web_sys::console::log_1(&"[composer] submit: accepted, spawning commit".into());
        let id = id_for_send.clone();
        let new = is_new;
        // the reactive spawn (NOT spawn_local): this task writes the
        // component-local sending/draft signals + the navigator — a
        // bare wasm task has no scope and panics at runtime.rs:223
        spawn(async move {
            let hex = if new { None } else { Some(id.as_str()) };
            match core::commit(hex, &text).await {
                // a fresh commit re-targets the page to the born thread
                Ok(new_id) if new => {
                    sending.set(false);
                    draft.set(String::new());
                    nav.replace(Route::ThreadPage { id: new_id });
                }
                Ok(_) => {
                    sending.set(false);
                    draft.set(String::new());
                }
                Err(e) => {
                    sending.set(false);
                    core::CORE.write().error = Some(e); // surfaced in the stream
                }
            }
        });
    };

    // THE DELETE AFFORDANCE (B8): two taps — arm, then confirm (a
    // destructive op deserves a beat). Local curation only: the
    // honest title carries the resurrection caveat (an arriving
    // message re-grows the thread — expected UX, not a bug).
    let mut armed = use_signal(|| false);
    let id_for_delete = id.clone();
    let do_delete = move |_| {
        if !armed() {
            armed.set(true);
            return;
        }
        armed.set(false);
        let id = id_for_delete.clone();
        spawn(async move {
            match core::delete_thread(&id).await {
                Ok(()) => {
                    nav.replace(Route::Home {});
                }
                Err(e) => core::CORE.write().error = Some(e),
            }
        });
    };

    // THE AUTO TRANSLATE + CACHE HYDRATION (design/ai-features/
    // coarse.md §3/§6): the effect watches the log. The FIRST pass =
    // the baseline (NO backfill — settled semantics); growth = the
    // arrivals -> enqueue (arrivals only: not own sends, not stamps/
    // empty rows). Every pass hydrates cached rows so the press is
    // served from the cache with zero network.
    let mut ai_seen = use_signal(|| None::<usize>);
    let id_for_ai = id.clone();
    use_effect(move || {
        let c = ai::cfg();
        let rows = core::CORE.read().logs.get(&id_for_ai).cloned();
        let Some(rows) = rows else { return };
        if c.enabled {
            let mut t = ai::TRS.write();
            for r in &rows {
                if r.stamp || r.text.is_empty() || t.contains_key(&r.ref_id) {
                    continue;
                }
                if let Some(text) = ai::cache_get(&r.ref_id, &c.lang) {
                    t.insert(r.ref_id.clone(), ai::TrState::Ready { text, shown: false });
                }
            }
        }
        // the baseline write is change-guarded: dioxus 0.7's set
        // notifies subscribers even on an equal value — an unguarded
        // set here re-fires THIS effect forever and freezes the page
        // (caught live in the sim: the freeze started the moment the
        // first log landed)
        let prev = ai_seen();
        let next = Some(rows.len());
        if ai_seen() != next {
            ai_seen.set(next);
        }
        if !c.enabled || !c.auto {
            return;
        }
        let Some(prev) = prev else { return };
        if rows.len() <= prev {
            return;
        }
        for r in &rows[prev..] {
            if r.stamp || r.text.is_empty() || r.author == "Me" {
                continue;
            }
            ai::enqueue(id_for_ai.clone(), r.ref_id.clone());
        }
    });

    rsx! {
        div { class: "thread",
            header { class: "page-head",
                Link { to: Route::Home {}, class: "back", "‹" }
                strong { if is_new { "New thread" } else { "{short_ref(&id)}" } }
                if !is_new {
                    button {
                        class: if armed() { "del armed" } else { "del" },
                        title: "Delete this thread — removes THIS device's copy; an arriving message re-grows it",
                        onclick: do_delete,
                        if armed() { "sure?" } else { "Delete" }
                    }
                }
            }
            div { class: "thread-stream",
                if is_new {
                    p { class: "hint", "Write the first message — it creates the thread" }
                } else if let Some(rows) = core::CORE.read().logs.get(&id) {
                    if rows.is_empty() {
                        p { class: "hint", "Empty thread — say something" }
                    }
                    for e in rows {
                        if e.stamp {
                            // THE TIMESTAMP MESSAGE (107's timestamp
                            // discipline, web face): the stamp row =
                            // the standalone dim ts line — no author,
                            // no bubble; the row IS the timestamp.
                            // It is still a SIGNED message: the
                            // untrusted face applies (the principal,
                            // 2026-11-12).
                            StampRow { row: e.clone() }
                        } else {
                            // THE TRANSLATE AFFORDANCE rides MsgRow
                            // (the per-row button when the master is
                            // ON; the block under the row carries the
                            // translation — shown by a press or AUTO;
                            // stamps/empty rows never gain it)
                            MsgRow { row: e.clone(), thread: id.clone() }
                        }
                    }
                } else {
                    p { class: "hint", "Stream pending load…" }
                }
                if let Some(err) = core::CORE.read().error.clone() {
                    div { class: "sys", "error: {err}" }
                }
            }
            form { class: "composer", onsubmit: submit,
                div {
                    class: if matches!(core::CORE.read().conn, core::Conn::Connected) { "relay-strip live" } else { "relay-strip" },
                    // DISPLAY-ONLY (the principal, 2026-11-12): the old
                    // tap-to-disconnect affordance is RETIRED — tapping
                    // the strip must never kill the visit, and a tap
                    // must never fake a reconnect either. Connection
                    // control lives in Settings (save = connect, Test
                    // = a real visit); the strip only REPORTS state.
                    span { class: strip_class(), "{strip_face()}" }
                    span { class: "relay-sub", "{strip_sub()}" }
                }
                div { class: "field",
                    input { r#type: "text", placeholder: if is_new { "First message" } else { "Message" },
                        value: "{draft}",
                        oninput: move |e| draft.set(e.value()),
                        disabled: sending,
                    }
                    button {
                        // THE SEND GATE (the principal, 2026-11-12):
                        // the key lights only with a draft AND a live
                        // relay connection — an unconnected session
                        // cannot send (the honest queueing face is
                        // traded for the gate by mandate; the draft
                        // stays in the box until the relay connects).
                        class: if draft().trim().is_empty() || sending()
                            || !matches!(core::CORE.read().conn, core::Conn::Connected)
                        { "send" } else { "send ready" },
                        r#type: "submit",
                        disabled: sending() || !matches!(core::CORE.read().conn, core::Conn::Connected),
                        "↑"
                    }
                }
            }
        }
    }
}

// ---- the stamp row (its own component: the untrusted join reads
// the registry signal, tracked per row) ----
#[component]
fn StampRow(row: core::LogRow) -> Element {
    let untrusted = core::row_untrusted(&row);
    rsx! {
        div { key: "{row.ref_id}", class: if untrusted { "msg-stamp untrust" } else { "msg-stamp" }, "{fmt_ts(row.ts_ms)}" }
        if untrusted {
            div { class: "msg-stamp untrust-note", "this message is currently untrusted — no signature from its author yet" }
        }
    }
}

// ---- one message row (the translate affordance lives here) ----
// A component of its own so the TRS reads are tracked per row: a
// translation arriving re-renders ONLY the affected row's component.
#[component]
fn MsgRow(row: core::LogRow, thread: String) -> Element {
    let tr = ai::TRS.read().get(&row.ref_id).cloned();
    // THE REMARK-PREFERRED FACES (the principal, 2026-11-12): the
    // author AND every signer render by their registry remark when
    // one exists — the lookup is LIVE (a remark edit re-faces every
    // row, author_display/signer_display read the registry signal).
    let author = core::author_display(&row);
    let signers = row
        .signers
        .iter()
        .map(|s| core::signer_display(&s.anchor_hex))
        .collect::<Vec<_>>()
        .join(", ");
    // THE UNTRUSTED FACE (the interface's conjunction): a RECEIVED
    // message that has an author but NO verified signature from that
    // author renders yellow + the honest line.
    let untrusted = core::row_untrusted(&row);
    rsx! {
        div { key: "{row.ref_id}", class: if untrusted { "msg untrust" } else { "msg" },
            div { class: "msg-author", class: if author == "Me" { "self" } else { "" }, "{author}" }
            div { class: "msg-text", "{row.text}" }
            if !row.signers.is_empty() {
                div { class: "msg-signers", "✍ {signers}" }
            }
            if untrusted {
                div { class: "msg-untrust", "this message is currently untrusted — no signature from its author yet" }
            }
            // THE meta.ts LINE IS OPTIONAL (the principal, 2026-11-12):
            // these rows display the OBSERVER's time (no ts claim on
            // ordinary messages) — shown while the Settings toggle is
            // on, hidden when it is off. The stamp rows (the ts
            // FEATURE) always render, already local-tz via fmt_ts.
            if show_recv_ts() {
                div { class: "msg-ts", "{fmt_ts(row.ts_ms)}" }
            }
            if !row.stamp && !row.text.is_empty() && ai::cfg().enabled {
                button {
                    class: "tr-btn",
                    onclick: move |_| ai::press(&thread, &row.ref_id),
                    "{ai::button_label(tr.as_ref())}"
                }
            }
            if let Some(ai::TrState::Ready { text, shown: true }) = tr {
                div { class: "msg-tr", "{text}" }
            }
        }
    }
}

// ---- P5: degrade page ----

#[component]
fn NotFound(segments: Vec<String>) -> Element {
    rsx! {
        div { class: "thread-stream",
            p { class: "hint", "Unknown page — degraded view" }
            p { class: "hint", "Path: /{segments.join(\"/\")}" }
        }
    }
}
