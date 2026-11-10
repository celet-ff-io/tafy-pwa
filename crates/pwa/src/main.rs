// tafy-pwa — empty shell (mock era).
// IA v3.1 (principal design, 2026-11-10): layout principle = put
// length-variant lists where NOTHING sits below them (same reason a
// thread is full-screen). Home topbar = the multi-function key and
// the brand grouped LEFT (a breath apart), Me at the right edge.
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
// Mock seam: typed in-memory placeholders ONLY — the real wasm core
// (LPZ106) plugs in behind this seam at the wiring phase. No network,
// no crypto, no persistence (A16 mock era; A17 WASM-local architecture).

use dioxus::prelude::*;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

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

// ---- mock seam (typed; replaced by the real core at wiring phase) ----

#[derive(Clone, Debug)]
struct Thread {
    id: &'static str,
    title: &'static str,
    preview: &'static str,
    unread: u8,
}

#[derive(Clone, Debug)]
struct Device {
    id: &'static str,
    name: &'static str,
    fingerprint: &'static str,
}

// mock seam: the real core's event stream swaps in behind this
// grammar 1:1 at the wiring phase.
/// Event kind in the interface-to-core grammar: chat message vs system
/// fact.
#[derive(Clone, Copy, Debug, PartialEq)]
enum EventKind {
    /// a chat message (peer or self)
    Message,
    /// system fact (link state, join/leave, errors) -> sys card
    SysFact,
}

#[derive(Clone, Copy, Debug)]
struct Event {
    kind: EventKind,
    /// owning thread id
    thread: &'static str,
    /// display name; empty = system (SysFact)
    author: &'static str,
    text: &'static str,
    /// unix ms; rendered as its own text-level line
    // layout rule: the ts line is STANDALONE text — never glued to the
    // author line (principal ruling, 2026-11-10)
    ts: u64,
}

const THREADS: &[Thread] = &[
    Thread {
        id: "alpha",
        title: "Alpha",
        preview: "In progress — this stream is a typed mock",
        unread: 1,
    },
    Thread {
        id: "beta",
        title: "Beta",
        preview: "Relay not configured",
        unread: 0,
    },
];

const DEVICES: &[Device] = &[
    Device {
        id: "self",
        name: "This device (this browser)",
        fingerprint: "···· ···· ···· ····",
    },
    Device {
        id: "peer",
        name: "Peer device (sample)",
        fingerprint: "···· ···· ···· ····",
    },
];

const EVENTS: &[Event] = &[
    Event {
        kind: EventKind::SysFact,
        thread: "alpha",
        author: "",
        text: "first-light link verified (mock)",
        ts: 1794271800000,
    },
    Event {
        kind: EventKind::Message,
        thread: "alpha",
        author: "Alpha",
        text: "first-light",
        ts: 1794271860000,
    },
    Event {
        kind: EventKind::Message,
        thread: "alpha",
        author: "Me",
        text: "Got it — the link is solid",
        ts: 1794271920000,
    },
    Event {
        kind: EventKind::Message,
        thread: "alpha",
        author: "Alpha",
        text: "Continue the empty-shell implementation",
        ts: 1794271980000,
    },
    Event {
        kind: EventKind::Message,
        thread: "alpha",
        author: "Me",
        text: "In progress — this stream is a typed mock",
        ts: 1794272040000,
    },
];

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
    rsx! {
        div { class: "page",
            TopBar { menu: false }
            div { class: "list",
                for t in THREADS {
                    Link { key: "{t.id}", to: Route::ThreadPage { id: t.id.to_string() }, class: "row",
                        div { class: "row-main",
                            span { class: "row-title", "{t.title}" }
                            span { class: "row-sub", "{t.preview}" }
                        }
                        if t.unread > 0 {
                            span { class: "badge", "{t.unread}" }
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
            p { class: "page-hint", "Identity pending the D2 round (key model) — contacts:" }
            div { class: "list",
                for d in DEVICES {
                    div { key: "{d.id}", class: "row",
                        div { class: "row-main",
                            span { class: "row-title", "{d.name}" }
                            span { class: "row-sub", "fingerprint {d.fingerprint}" }
                        }
                    }
                }
            }
        }
    }
}

// ---- P-Settings: static rows ----

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
                div { class: "row",
                    div { class: "row-main",
                        span { class: "row-title", "Relay" }
                        span { class: "row-sub", "Not configured — waiting on 106's relay browser face" }
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
    rsx! {
        div { class: "page",
            header { class: "page-head",
                Link { to: Route::Settings {}, class: "back", "‹" }
                strong { "About" }
            }
            p { class: "page-hint", "tAfY — the zconn web client (mock era, first-light)" }
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

#[component]
fn ThreadPage(id: String) -> Element {
    let thread = THREADS.iter().find(|t| t.id == id);
    let stream: Vec<&Event> = EVENTS.iter().filter(|e| e.thread == id).collect();
    rsx! {
        div { class: "thread",
            header { class: "page-head",
                Link { to: Route::Home {}, class: "back", "‹" }
                strong { "{thread.map(|t| t.title).unwrap_or(\"Unknown thread\")}" }
            }
            div { class: "thread-stream",
                if thread.is_none() {
                    p { class: "hint", "Unknown thread — degraded view" }
                } else if stream.is_empty() {
                    p { class: "hint", "Stream pending load (waiting on 106's core)" }
                }
                for e in stream {
                    match e.kind {
                        EventKind::Message => rsx! {
                            div { key: "{e.ts}", class: "msg",
                                div { class: "msg-author", class: if e.author == "Me" { "self" } else { "" }, "{e.author}" }
                                div { class: "msg-text", "{e.text}" }
                                div { class: "msg-ts", "{fmt_ts(e.ts)}" }
                            }
                        },
                        EventKind::SysFact => rsx! {
                            div { key: "{e.ts}", class: "sys", "{e.text} · {fmt_ts(e.ts)}" }
                        },
                    }
                }
            }
            form { class: "composer", onsubmit: move |e| e.prevent_default(),
                div { class: "field",
                    input { r#type: "text", placeholder: "Sending disabled in the shell stage", disabled: true }
                    button { class: "send", r#type: "submit", disabled: true, "↑" }
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
