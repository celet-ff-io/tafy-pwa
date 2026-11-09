// tafy-pwa — empty shell (mock era).
// IA v0 (principal-approved): P1 chats list, P2 chat detail, P3 contacts,
// P4 settings, P5 degrade page; C1 bottom 3-tab navigation, mobile-first.
// Mock seam: typed in-memory placeholders ONLY — the real wasm core
// (LPZ106) plugs in behind this seam at the wiring phase. No network,
// no crypto, no persistence (A16 mock era; A17 WASM-local architecture).

use dioxus::prelude::*;

fn main() {
    dioxus::launch(App);
}

#[derive(Clone, Routable, Debug, PartialEq)]
enum Route {
    #[layout(Shell)]
    #[route("/")]
    Chats {},
    #[route("/contacts")]
    Contacts {},
    #[route("/settings")]
    Settings {},
    #[route("/chat/:id")]
    Chat { id: String },
    #[route("/:..segments")]
    NotFound { segments: Vec<String> },
}

#[component]
fn App() -> Element {
    rsx! {
        document::Link { rel: "stylesheet", href: asset!("/assets/main.css") }
        Router::<Route> {}
    }
}

/// Persistent frame: routed page + bottom tab bar (C1).
#[component]
fn Shell() -> Element {
    rsx! {
        main { class: "shell",
            Outlet::<Route> {}
            nav { class: "tabbar",
                Link { to: Route::Chats {}, class: "tab", active_class: "active", "会话" }
                Link { to: Route::Contacts {}, class: "tab", active_class: "active", "联系人" }
                Link { to: Route::Settings {}, class: "tab", active_class: "active", "设置" }
            }
        }
    }
}

// ---- mock seam (typed; replaced by the real core at wiring phase) ----

struct Session {
    id: &'static str,
    peer: &'static str,
    last: &'static str,
    unread: u8,
}

struct Device {
    id: &'static str,
    name: &'static str,
    fingerprint: &'static str,
}

/// Render-pipeline envelope: typed event -> kind -> card renderer;
/// unknown kind -> degrade ladder -> hint card. Shape follows the i2c
/// grammar so the real core's event stream swaps in 1:1 at wiring.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    /// a chat message (peer or self)
    Message,
    /// system fact (link state, join/leave, errors) -> sys card
    SysFact,
}

struct Event {
    kind: Kind,
    /// owning session id
    session: &'static str,
    /// display name; empty = system (SysFact)
    author: &'static str,
    text: &'static str,
    /// unix ms — rendered standalone, text-level (NOT an author-level
    /// claim); principal ruling 2026-11-10
    ts: u64,
}

const SESSIONS: &[Session] = &[
    Session {
        id: "alpha",
        peer: "Alpha",
        last: "first-light",
        unread: 1,
    },
    Session {
        id: "beta",
        peer: "Beta",
        last: "中继未配置",
        unread: 0,
    },
];

const EVENTS: &[Event] = &[
    Event {
        kind: Kind::SysFact,
        session: "alpha",
        author: "",
        text: "first-light 链路验证通过（mock）",
        ts: 1794271800000,
    },
    Event {
        kind: Kind::Message,
        session: "alpha",
        author: "Alpha",
        text: "first-light",
        ts: 1794271860000,
    },
    Event {
        kind: Kind::Message,
        session: "alpha",
        author: "我",
        text: "收到，链路正常",
        ts: 1794271920000,
    },
    Event {
        kind: Kind::Message,
        session: "alpha",
        author: "Alpha",
        text: "继续空壳实现",
        ts: 1794271980000,
    },
    Event {
        kind: Kind::Message,
        session: "alpha",
        author: "我",
        text: "进行中 — 这条流是 typed mock",
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

const DEVICES: &[Device] = &[
    Device {
        id: "self",
        name: "本机（此浏览器）",
        fingerprint: "···· ···· ···· ····",
    },
    Device {
        id: "peer",
        name: "对端设备（示例）",
        fingerprint: "···· ···· ···· ····",
    },
];

// ---- P1: chats list ----

#[component]
fn Chats() -> Element {
    rsx! {
        h1 { class: "page-title", "会话" }
        ul { class: "chat-list",
            for s in SESSIONS {
                li { key: "{s.id}",
                    Link { to: Route::Chat { id: s.id.to_string() }, class: "chat-row",
                        div { class: "chat-main",
                            span { class: "chat-peer", "{s.peer}" }
                            span { class: "chat-last", "{s.last}" }
                        }
                        if s.unread > 0 {
                            span { class: "badge", "{s.unread}" }
                        }
                    }
                }
            }
        }
    }
}

// ---- P2: chat detail (mock stream; send = disabled) ----

#[component]
fn Chat(id: String) -> Element {
    let session = SESSIONS.iter().find(|s| s.id == id);
    let stream: Vec<&Event> = EVENTS.iter().filter(|e| e.session == id).collect();
    rsx! {
        header { class: "chat-head",
            Link { to: Route::Chats {}, class: "back", "‹" }
            strong { "{session.map(|s| s.peer).unwrap_or(\"未知会话\")}" }
        }
        div { class: "chat-stream",
            if session.is_none() {
                p { class: "hint", "未知会话 — 降级视图" }
            } else if stream.is_empty() {
                p { class: "hint", "消息流待装填（等待 106 的 core）" }
            }
            for e in stream {
                match e.kind {
                    Kind::Message => rsx! {
                        div { key: "{e.ts}", class: "msg",
                            div { class: "msg-author", class: if e.author == "我" { "self" } else { "" }, "{e.author}" }
                            div { class: "msg-text", "{e.text}" }
                            div { class: "msg-ts", "{fmt_ts(e.ts)}" }
                        }
                    },
                    Kind::SysFact => rsx! {
                        div { key: "{e.ts}", class: "sys", "{e.text} · {fmt_ts(e.ts)}" }
                    },
                }
            }
        }
        form { class: "composer", onsubmit: move |e| e.prevent_default(),
            input { r#type: "text", placeholder: "空壳阶段不可发送", disabled: true }
        }
    }
}

// ---- P3: contacts / devices ----

#[component]
fn Contacts() -> Element {
    rsx! {
        h1 { class: "page-title", "联系人" }
        for d in DEVICES {
            div { key: "{d.id}", class: "card",
                h2 { "{d.name}" }
                p { "指纹 {d.fingerprint}" }
            }
        }
    }
}

// ---- P4: settings ----

#[component]
fn Settings() -> Element {
    rsx! {
        h1 { class: "page-title", "设置" }
        div { class: "card",
            h2 { "身份" }
            p { "未配置 — 密钥模型待 D2 轮" }
        }
        div { class: "card",
            h2 { "中继" }
            p { "未配置 — 等待 106 的中继浏览器面" }
        }
        div { class: "card",
            h2 { "关于" }
            p { "tafy · first-light · 空壳（mock 时代，无真实数据）" }
        }
    }
}

// ---- P5: degrade page ----

#[component]
fn NotFound(segments: Vec<String>) -> Element {
    rsx! {
        div { class: "chat-stream",
            p { class: "hint", "未知页面 — 降级视图" }
            p { class: "hint", "路径: /{segments.join(\"/\")}" }
        }
    }
}
