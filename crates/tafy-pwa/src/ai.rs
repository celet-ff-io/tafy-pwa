// ai.rs — the PWA-local AI feature layer (translation v1; design =
// design/ai-features/coarse.md). THE STORAGE RULING (the principal,
// 2026-11-12): this layer NEVER touches zconned — its settings and
// its cache live in the PWA's own localStorage (the 'tafy.relay_url'
// precedent); the core, the DAG, and the core store are untouched.
// The request = one plain OpenAI-format POST from the page context
// (no streaming; the endpoint must allow browser CORS — stated in
// the settings hint).
//
// Honesty posture (the design's §7): enabling translation sends the
// message content to the user's chosen endpoint IN PLAINTEXT — the
// disclosure faces live in the AI settings page + the Help page.
// Translations are this-device-only, never committed to the DAG,
// never visible to peers.

use std::collections::{HashMap, VecDeque};

use dioxus::prelude::*;

/// The unscoped spawn: translation tasks must SURVIVE the page that
/// started them (a queue drain or an in-flight request killed by a
/// component unmount would stall the queue with DRAINING stuck true).
/// wasm-bindgen-futures::spawn_local is scope-free; the native face
/// never runs (A17: the core lives in a browser) — the future is
/// typecheck-only there.
#[cfg(target_arch = "wasm32")]
pub fn spawn_task<F: std::future::Future<Output = ()> + 'static>(f: F) {
    wasm_bindgen_futures::spawn_local(f);
}
#[cfg(not(target_arch = "wasm32"))]
pub fn spawn_task<F: std::future::Future<Output = ()> + 'static>(_f: F) {}

// ---- settings keys (all localStorage, the PWA's own storage) ----

const K_URL: &str = "tafy.ai.endpoint_url";
const K_KEY: &str = "tafy.ai.key";
const K_MODEL: &str = "tafy.ai.model";
const K_ENABLED: &str = "tafy.ai.translate_enabled";
const K_AUTO: &str = "tafy.ai.translate_auto";
const K_LANG: &str = "tafy.ai.translate_lang";
const K_N: &str = "tafy.ai.translate_n";
const K_TR: &str = "tafy.ai.tr"; // the cache prefix

const DEFAULT_LANG: &str = "Simplified Chinese";
const DEFAULT_N: usize = 5;
pub const MAX_N: usize = 20;

/// The settings snapshot (read fresh at each use site — the values
/// are tiny and localStorage is the single source; no signal copy
/// that could drift).
#[derive(Clone, Debug, PartialEq)]
pub struct Cfg {
    pub url: String,
    pub key: String,
    pub model: String,
    pub enabled: bool,
    pub auto: bool,
    pub lang: String,
    pub n: usize,
}

#[cfg(target_arch = "wasm32")]
fn ls_get(key: &str) -> Option<String> {
    web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item(key).ok().flatten())
}
#[cfg(not(target_arch = "wasm32"))]
fn ls_get(_key: &str) -> Option<String> {
    None
}

#[cfg(target_arch = "wasm32")]
fn ls_set(key: &str, val: &str) {
    if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = s.set_item(key, val);
    }
}
#[cfg(not(target_arch = "wasm32"))]
fn ls_set(_key: &str, _val: &str) {}

#[cfg(target_arch = "wasm32")]
fn ls_del(key: &str) {
    if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = s.remove_item(key);
    }
}
#[cfg(not(target_arch = "wasm32"))]
fn ls_del(_key: &str) {}

pub fn cfg() -> Cfg {
    Cfg {
        url: ls_get(K_URL).unwrap_or_default(),
        key: ls_get(K_KEY).unwrap_or_default(),
        model: ls_get(K_MODEL).unwrap_or_default(),
        enabled: ls_get(K_ENABLED).is_some(),
        auto: ls_get(K_AUTO).is_some(),
        lang: {
            let l = ls_get(K_LANG).unwrap_or_default();
            if l.is_empty() {
                DEFAULT_LANG.to_string()
            } else {
                l
            }
        },
        n: {
            let n = ls_get(K_N)
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(DEFAULT_N)
                .clamp(1, MAX_N);
            n
        },
    }
}

/// The endpoint's readiness for a request (the three fields set).
pub fn endpoint_ready(c: &Cfg) -> bool {
    !c.url.is_empty() && !c.key.is_empty() && !c.model.is_empty()
}

pub fn set_url(v: &str) {
    ls_set(K_URL, v);
}
pub fn set_key(v: &str) {
    ls_set(K_KEY, v);
}
pub fn set_model(v: &str) {
    ls_set(K_MODEL, v);
}
pub fn set_enabled(v: bool) {
    if v {
        ls_set(K_ENABLED, "1")
    } else {
        ls_del(K_ENABLED);
        // the master OFF hides everything downstream: the auto switch
        // goes with it (stale sub-state would re-surface on re-enable)
        ls_del(K_AUTO);
    }
}
pub fn set_auto(v: bool) {
    if v {
        ls_set(K_AUTO, "1")
    } else {
        ls_del(K_AUTO)
    }
}
pub fn set_lang(v: &str) {
    ls_set(K_LANG, v);
}
pub fn set_n(v: usize) {
    ls_set(K_N, &v.clamp(1, MAX_N).to_string());
}

// ---- the translation cache (localStorage, keyed digest+lang) ----

fn cache_key(digest: &str, lang: &str) -> String {
    format!("{}.{}.{}", K_TR, digest, lang)
}

pub fn cache_get(digest: &str, lang: &str) -> Option<String> {
    ls_get(&cache_key(digest, lang)).filter(|t| !t.is_empty())
}

fn cache_set(digest: &str, lang: &str, text: &str) {
    ls_set(&cache_key(digest, lang), text);
}

// ---- the per-row render state (reactive; the UI reads TRS) ----

#[derive(Clone, Debug, PartialEq)]
pub enum TrState {
    /// a request is in flight for this row
    Loading,
    /// fetched (or cached-at-press): the text + whether shown
    Ready { text: String, shown: bool },
    /// the last attempt failed (the button retries)
    Failed,
}

pub static TRS: GlobalSignal<HashMap<String, TrState>> = Signal::global(HashMap::new);

fn set_state(ref_id: &str, s: TrState) {
    TRS.write().insert(ref_id.to_string(), s);
}

/// The button label for a row's current state.
pub fn button_label(s: Option<&TrState>) -> &'static str {
    match s {
        None => "translate",
        Some(TrState::Loading) => "…",
        Some(TrState::Ready { shown: true, .. }) => "hide",
        Some(TrState::Ready { shown: false, .. }) => "translate",
        Some(TrState::Failed) => "translate ✗",
    }
}

/// The manual press: cached -> toggle show/hide; in-flight -> noop;
/// otherwise start the fetch (single-flight per row).
pub fn press(thread: &str, ref_id: &str) {
    let c = cfg();
    if !c.enabled {
        return;
    }
    // CLONE-THEN-MATCH: holding the TRS read guard across the match
    // would deadlock the write in the arms (the scrutinee temporary
    // lives through every arm)
    let cur = TRS.read().get(ref_id).cloned();
    match cur {
        Some(TrState::Ready { shown, text }) => {
            set_state(
                ref_id,
                TrState::Ready {
                    text,
                    shown: !shown,
                },
            );
        }
        Some(TrState::Loading) => {}
        Some(TrState::Failed) | None => {
            if !endpoint_ready(&c) {
                return;
            }
            set_state(ref_id, TrState::Loading);
            let (thread, ref_id) = (thread.to_string(), ref_id.to_string());
            spawn_task(async move {
                run_one(&c, &thread, &ref_id).await;
            });
        }
    }
}

// ---- the AUTO queue (strictly serial; the design's §6) ----

static QUEUE: std::sync::Mutex<VecDeque<(String, String)>> = std::sync::Mutex::new(VecDeque::new());
static DRAINING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Enqueue a row for auto-translation; the drain is serial — one
/// request at a time, a failing item is marked and the queue moves on.
pub fn enqueue(thread: String, ref_id: String) {
    {
        let mut q = QUEUE.lock().unwrap();
        if q.iter().any(|(_, r)| r == &ref_id) {
            return; // already queued (single-flight at the queue too)
        }
        q.push_back((thread, ref_id));
    }
    drain();
}

fn drain() {
    if DRAINING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    spawn_task(async move {
        loop {
            let job = QUEUE.lock().unwrap().pop_front();
            let Some((thread, ref_id)) = job else {
                break;
            };
            let c = cfg();
            // the guards: the feature can be switched off mid-queue,
            // the endpoint can be unset, and the row may have ceased
            // to exist (a thread deleted while queued)
            let exists = crate::core::CORE
                .read()
                .logs
                .get(&thread)
                .map(|rows| rows.iter().any(|r| r.ref_id == ref_id))
                .unwrap_or(false);
            if exists && c.enabled && endpoint_ready(&c) {
                set_state(&ref_id, TrState::Loading);
                run_one(&c, &thread, &ref_id).await;
            } else {
                let mut t = TRS.write();
                t.remove(&ref_id);
            }
        }
        DRAINING.store(false, std::sync::atomic::Ordering::SeqCst);
    });
}

/// One translation attempt: build the prompt from the log, POST, on
/// success store the cache + mark Ready(shown). Manual and auto both
/// land here (auto = "as if the button had been pressed" — it shows).
async fn run_one(c: &Cfg, thread: &str, ref_id: &str) {
    let rows = crate::core::CORE.read().logs.get(thread).cloned();
    let Some(rows) = rows else {
        set_state(ref_id, TrState::Failed);
        return;
    };
    // the cache is the FIRST source: a press on an already-
    // translated row (e.g. after a reload, when TRS is empty) is
    // served locally — zero network, zero re-pay
    if let Some(text) = cache_get(ref_id, &c.lang) {
        set_state(ref_id, TrState::Ready { text, shown: true });
        return;
    }
    let Some(prompt) = build_prompt(c, &rows, ref_id) else {
        set_state(ref_id, TrState::Failed);
        return;
    };
    match chat(c, &prompt).await {
        Ok(text) => {
            cache_set(ref_id, &c.lang, &text);
            set_state(ref_id, TrState::Ready { text, shown: true });
        }
        Err(e) => {
            let _ = e; // the face is the row-level failure mark; the
            // endpoint's own error text is not echoed into the thread
            // (it could read as a message row)
            set_state(ref_id, TrState::Failed);
        }
    }
}

/// The prompt builder: the target + the (n-1) preceding CONTENT rows
/// (stamps and empty-text rows are excluded — not language content;
/// they never occupy context slots). Speakers are anonymized A/B by
/// first appearance; the labels only need to be consistent WITHIN one
/// request.
fn build_prompt(
    c: &Cfg,
    rows: &[crate::core::LogRow],
    ref_id: &str,
) -> Option<Vec<(String, String)>> {
    let pos = rows.iter().position(|r| r.ref_id == ref_id)?;
    let before: Vec<&crate::core::LogRow> = rows[..pos]
        .iter()
        .filter(|r| !r.stamp && !r.text.is_empty())
        .collect();
    let target = &rows[pos];
    if target.stamp || target.text.is_empty() {
        return None; // nothing translatable (the button never shows for these)
    }
    let ctx = if before.len() >= c.n {
        &before[before.len() + 1 - c.n..]
    } else {
        &before[..]
    };
    let mut labels: HashMap<String, String> = HashMap::new();
    // the A/B slots assign by first appearance; a third distinct
    // speaker folds into B (two-party anonymization is the design)
    let next_label = |labels: &mut HashMap<String, String>, author: &str| -> String {
        if let Some(l) = labels.get(author) {
            return l.clone();
        }
        let slot = ["A", "B"][labels.len().min(1)];
        labels.insert(author.to_string(), slot.to_string());
        slot.to_string()
    };
    let mut lines: Vec<(String, String)> = Vec::new();
    for r in ctx {
        let who = next_label(&mut labels, &r.author);
        lines.push(("user".into(), format!("{}: {}", who, r.text)));
    }
    let who = next_label(&mut labels, &target.author);
    lines.push(("user".into(), format!("{}: {}", who, target.text)));
    let system = format!(
        "You translate chat messages. The LAST line is the message to translate. \
Earlier lines are context only (speakers anonymized as A/B). Translate the LAST \
line into {}, preserving tone, register, and idiomatic meaning. Output ONLY the \
translation text.",
        c.lang
    );
    let mut out = vec![("system".into(), system)];
    out.extend(lines);
    Some(out)
}

// ---- the OpenAI-format request (page context, plain fetch) ----

/// The chat request: `POST {url}/chat/completions`, non-stream; the
/// reply = choices[0].message.content trimmed.
#[cfg(target_arch = "wasm32")]
async fn chat(c: &Cfg, messages: &[(String, String)]) -> Result<String, String> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;
    let window = web_sys::window().ok_or("no window")?;
    let url = format!("{}/chat/completions", c.url.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": c.model,
        "messages": messages
            .iter()
            .map(|(role, content)| serde_json::json!({"role": role, "content": content}))
            .collect::<Vec<_>>(),
    });
    let init = web_sys::RequestInit::new();
    init.set_method("POST");
    init.set_body(&body.to_string().into());
    let req = web_sys::Request::new_with_str_and_init(&url, &init)
        .map_err(|_| "bad request".to_string())?;
    let headers = req.headers();
    headers
        .set("Content-Type", "application/json")
        .map_err(|_| "header".to_string())?;
    headers
        .set("Authorization", &format!("Bearer {}", c.key))
        .map_err(|_| "header".to_string())?;
    let resp = JsFuture::from(window.fetch_with_request_and_init(&req, &init))
        .await
        .map_err(|_| "fetch failed".to_string())?;
    let resp: web_sys::Response = resp.dyn_into().map_err(|_| "bad response".to_string())?;
    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let val = JsFuture::from(resp.json().map_err(|_| "no json".to_string())?)
        .await
        .map_err(|_| "bad json".to_string())?;
    let choices =
        js_sys::Reflect::get(&val, &"choices".into()).map_err(|_| "no choices".to_string())?;
    let first =
        js_sys::Reflect::get(&choices, &0u32.into()).map_err(|_| "empty choices".to_string())?;
    let message =
        js_sys::Reflect::get(&first, &"message".into()).map_err(|_| "no message".to_string())?;
    let content = js_sys::Reflect::get(&message, &"content".into())
        .map_err(|_| "no content".to_string())?
        .as_string()
        .ok_or("content not a string".to_string())?;
    let text = content.trim().to_string();
    if text.is_empty() {
        return Err("empty content".to_string());
    }
    Ok(text)
}

#[cfg(not(target_arch = "wasm32"))]
async fn chat(_c: &Cfg, _messages: &[(String, String)]) -> Result<String, String> {
    Err("ai: the wasm face only".to_string())
}

/// The endpoint Test: a real tiny request (the relay-Test shape — no
/// probe theater), the live outcome in place. Returns the ok face
/// with the elapsed ms, or the error text.
#[cfg(target_arch = "wasm32")]
pub async fn test_endpoint(c: &Cfg) -> Result<String, String> {
    let t0 = js_sys::Date::now();
    let msgs = vec![("user".to_string(), "Reply with the word OK.".to_string())];
    chat(c, &msgs).await?;
    Ok(format!("{}ms", (js_sys::Date::now() - t0) as u64))
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn test_endpoint(_c: &Cfg) -> Result<String, String> {
    Err("ai: the wasm face only".to_string())
}
