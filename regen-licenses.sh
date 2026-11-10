#!/usr/bin/env bash
# regen-licenses.sh — regenerate public/licenses.html (rules.md A18).
# Every deploy is a distribution: the page must always reflect the
# dependency set of the bundle being deployed.
set -euo pipefail
cd "$(dirname "$0")"
nix run nixpkgs#cargo-about -- generate --workspace about.hbs -o crates/tafy-pwa/public/licenses.html
# dx skips existing public/ files in its dist cache (copy-if-missing
# only) — evict the stale copy so the next build recopies the fresh one
rm -f ~/.cache/cargo/dx/tafy-pwa/release/web/public/licenses.html
echo "licenses.html regenerated"
