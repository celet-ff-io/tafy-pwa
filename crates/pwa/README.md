# tafy-pwa — build & deploy runbook (S1 first-light)

> Verified pipeline, 2026-11-09. Everything below was actually run and
> its output checked (assets 200 across the board; PWA head intact).

## One-time toolchain

- `rustup target add wasm32-unknown-unknown`
- `wasm-bindgen-cli` MUST be **0.2.130** — dioxus 0.7.10 pins it
  `=0.2.130` (cargo `--precise` downgrade is refused); nixpkgs ships
  0.2.126/0.2.127, both rejected by dx 0.7.10:
  `cargo install wasm-bindgen-cli --version 0.2.130 --locked`
- `dioxus-cli` from nixpkgs (0.7.10, matches the dioxus dep): run the
  store binary DIRECTLY, not via `nix run` — nix run prepends its own
  wasm-bindgen 0.2.126 to PATH and shadows ours:
  `DXBIN=$(nix build nixpkgs#dioxus-cli --print-out-paths --no-link)/bin/dx`
- `wasm-opt` (binaryen) must be on PATH (dx requires it for release):
  `WOBIN=$(nix build nixpkgs#binaryen --print-out-paths --no-link)/bin`

## Build

```sh
cd crates/pwa
rm -rf ~/.cache/cargo/dx/tafy-pwa   # dx does NOT clean stale hashed assets
env PATH="$WOBIN:$HOME/.local/bin:$PATH" "$DXBIN" build --release
# output: ~/.cache/cargo/dx/tafy-pwa/release/web/public
```

## Verified quirks (do not re-trip)

1. **Title single-source**: dx APPENDS its title into any existing
   `<title>` in index.html — keep `<title></title>` empty in the
   static template and set `title` ONLY in Dioxus.toml `[web.app]`.
2. **PWA static files live in `public/`** (copied verbatim): manifest,
   sw.js, pwa.js (SW registration), icons. `asset!()`-hashed files
   (css/wasm/glue) cannot be referenced by the static head — hashed
   names break manifest/apple-touch-icon refs.
3. **Custom index.html** at the crate root is honored; dx injects its
   script tags alongside the static head (keep the static links for
   manifest/theme-color/viewport).
4. `base_path = "/"` (Dioxus.toml). gh-pages PROJECT sites serve under
   a subpath — set `base_path = "/<repo>"` there, or prefer a USER
   site repo (`<user>.github.io`) which serves at root and needs no
   change. hrefs like `/./assets/...` resolve fine at root.

## Deploy (A16 channel: anonymous gh-pages)

1. Create the burner repo; if USER-site repo (`<user>.github.io`),
   base_path stays `/`; else set it to `/<repo>` and rebuild.
2. From the dist dir: `git init -b gh-pages && git add -A && git
   commit -m deploy && git push <burner-remote> gh-pages --force`
3. Enable Pages (repo settings → Pages → deploy from gh-pages branch).
4. Phone: Android → install prompt; iOS Safari → share → Add to Home
   Screen. Offline shell via sw.js (cache-first, version-bumped file).
5. Update = rebuild + push again; SW swaps the shell on refresh.
