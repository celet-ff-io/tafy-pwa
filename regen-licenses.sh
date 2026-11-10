#!/usr/bin/env bash
# regen-licenses.sh — regenerate public/licenses.html (rules.md A18).
# Every deploy is a distribution and carries the shipped version stamp
# in the page, so EVERY version bump / deploy requires a regen first.
set -euo pipefail
cd "$(dirname "$0")"
ver=$(awk -F'"' '/^version = "/{print $2; exit}' crates/pwa/Cargo.toml)
nix run nixpkgs#cargo-about -- generate --workspace about.hbs -o crates/pwa/public/licenses.html
sed -i "s/@@TAFY_VERSION@@/$ver/" crates/pwa/public/licenses.html
echo "licenses.html regenerated (tAfY $ver)"
