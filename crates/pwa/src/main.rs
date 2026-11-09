// tafy-pwa — empty shell (mock era).
// IA v2 (principal design, 2026-11-10): layout principle = put
// length-variant lists where NOTHING sits below them (same reason a
// thread is full-screen). Home = topbar (wordmark "tAfY" + me/settings
// buttons) over a free-growing thread list. Me page = identity hint
// over the contacts list (variable, nothing below). Settings = its own
// page. Thread = full-screen focused. The message level knows only
// THREADS (消息链) — "会话" was a zconnor-connection term, dropped.
// Mock seam: typed in-memory placeholders ONLY — the real wasm core
// (LPZ106) plugs in behind this seam at the wiring phase. No network,
// no crypto, no persistence (A16 mock era; A17 WASM-local architecture).

use dioxus::prelude::*;

fn main() {
    dioxus::launch(App);
}

#[derive(Clone, Routable, Debug, PartialEq)]
enum Route {
    #[route("/")]
    Home {},
    #[route("/me")]
    Me {},
    #[route("/settings")]
    Settings {},
    #[route("/thread/:id")]
    ThreadPage { id: String },
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
/// Event kind in the i2c grammar: chat message vs system fact.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    /// a chat message (peer or self)
    Message,
    /// system fact (link state, join/leave, errors) -> sys card
    SysFact,
}

#[derive(Clone, Copy, Debug)]
struct Event {
    kind: Kind,
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
        preview: "进行中 — 这条流是 typed mock",
        unread: 1,
    },
    Thread {
        id: "beta",
        title: "Beta",
        preview: "中继未配置",
        unread: 0,
    },
];

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

const EVENTS: &[Event] = &[
    Event {
        kind: Kind::SysFact,
        thread: "alpha",
        author: "",
        text: "first-light 链路验证通过（mock）",
        ts: 1794271800000,
    },
    Event {
        kind: Kind::Message,
        thread: "alpha",
        author: "Alpha",
        text: "first-light",
        ts: 1794271860000,
    },
    Event {
        kind: Kind::Message,
        thread: "alpha",
        author: "我",
        text: "收到，链路正常",
        ts: 1794271920000,
    },
    Event {
        kind: Kind::Message,
        thread: "alpha",
        author: "Alpha",
        text: "继续空壳实现",
        ts: 1794271980000,
    },
    Event {
        kind: Kind::Message,
        thread: "alpha",
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

// ---- P-Home: topbar over a free-growing thread list ----

#[component]
fn Home() -> Element {
    rsx! {
        div { class: "page",
            header { class: "topbar",
                h1 { class: "wordmark", "tAfY" }
                div { class: "topbar-actions",
                    Link { to: Route::Me {}, class: "icon-btn", title: "Me",
                        svg { fill: "none", stroke: "currentColor", stroke_width: "1.8", stroke_linecap: "round", stroke_linejoin: "round", view_box: "0 0 24 24",
                            path { d: "M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2" }
                            circle { cx: "12", cy: "7", r: "4" }
                        }
                    }
                    Link { to: Route::Settings {}, class: "icon-btn", title: "设置",
                        svg { fill: "none", stroke: "currentColor", stroke_width: "1.8", stroke_linecap: "round", stroke_linejoin: "round", view_box: "0 0 24 24",
                            circle { cx: "12", cy: "12", r: "3" }
                            path { d: "M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z" }
                        }
                    }
                }
            }
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
            p { class: "page-hint", "身份待 D2 轮（密钥模型）— 联系人：" }
            div { class: "list",
                for d in DEVICES {
                    div { key: "{d.id}", class: "row",
                        div { class: "row-main",
                            span { class: "row-title", "{d.name}" }
                            span { class: "row-sub", "指纹 {d.fingerprint}" }
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
    rsx! {
        div { class: "page",
            header { class: "page-head",
                Link { to: Route::Home {}, class: "back", "‹" }
                strong { "设置" }
            }
            div { class: "list",
                div { class: "row",
                    div { class: "row-main",
                        span { class: "row-title", "身份" }
                        span { class: "row-sub", "未配置 — 密钥模型待 D2 轮" }
                    }
                }
                div { class: "row",
                    div { class: "row-main",
                        span { class: "row-title", "中继" }
                        span { class: "row-sub", "未配置 — 等待 106 的中继浏览器面" }
                    }
                }
                div { class: "row",
                    div { class: "row-main",
                        span { class: "row-title", "关于" }
                        span { class: "row-sub", "tAfY · first-light · mock 时代" }
                    }
                }
            }
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
                strong { "{thread.map(|t| t.title).unwrap_or(\"未知线程\")}" }
            }
            div { class: "thread-stream",
                if thread.is_none() {
                    p { class: "hint", "未知线程 — 降级视图" }
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
                div { class: "field",
                    input { r#type: "text", placeholder: "空壳阶段不可发送", disabled: true }
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
            p { class: "hint", "未知页面 — 降级视图" }
            p { class: "hint", "路径: /{segments.join(\"/\")}" }
        }
    }
}
