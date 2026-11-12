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

/// The topbar's compact conn face (principal, 2026-11-12): a one-
/// glyph status right of the multi-function key on Home. The glyph
/// is the summary, never the whole truth — the honest full face
/// rides the title attribute (and the composer's strip carries the
/// detail line).
fn conn_pill(failed: bool, conn: &core::Conn) -> &'static str {
    if failed {
        return "✗";
    }
    match conn {
        core::Conn::Connected => "✓",
        core::Conn::Connecting => "…",
        core::Conn::Error(_) => "✗",
        core::Conn::Disconnected => "–",
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
        // the protocol's persist-then-queue: an unvisited commit
        // re-enters the seal task at the next visit
        core::Conn::Connected => "sending through the relay — tap to end the visit",
        _ => "messages queue on this device until the next visit",
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
fn fmt_ts(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
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
        .and_then(|rows| rows.last().map(|r| r.text.clone()))
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
                // by this mandate). Glyph = the summary; the honest
                // full face rides the title.
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
    rsx! {
        div { class: "page",
            header { class: "page-head",
                Link { to: Route::Settings {}, class: "back", "‹" }
                strong { "About" }
            }
            p { class: "page-hint", "tAfY — the zconned web client" }
            div { class: "list",
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
            p { class: "copyright", "Copyright © 2026-present tAfY Developers" }
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

    rsx! {
        div { class: "thread",
            header { class: "page-head",
                Link { to: Route::Home {}, class: "back", "‹" }
                strong { if is_new { "New thread" } else { "{short_ref(&id)}" } }
            }
            div { class: "thread-stream",
                if is_new {
                    p { class: "hint", "Write the first message — it creates the thread" }
                } else if let Some(rows) = core::CORE.read().logs.get(&id) {
                    if rows.is_empty() {
                        p { class: "hint", "Empty thread — say something" }
                    }
                    for e in rows {
                        div { key: "{e.ref_id}", class: "msg",
                            div { class: "msg-author", class: if e.author == "Me" { "self" } else { "" }, "{e.author}" }
                            div { class: "msg-text", "{e.text}" }
                            div { class: "msg-ts", "{fmt_ts(e.ts_ms)}" }
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
                    // the connected strip = the visit's end face (the
                    // disconnect verb was dead until this affordance)
                    onclick: move |_| {
                        if matches!(core::CORE.read().conn, core::Conn::Connected) {
                            core::relay_disconnect();
                        }
                    },
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
                        class: if draft().trim().is_empty() || sending() { "send" } else { "send ready" },
                        r#type: "submit",
                        disabled: sending,
                        "↑"
                    }
                }
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
