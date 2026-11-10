{
  description = "tafy";

  # follows the machine's global registry (nixos-unstable). Do NOT
  # bump to a raw github rev blindly: newer unstable revs build
  # dioxus-cli from source and FAIL on a missing libzstd pkg-config
  # probe (verified 2026-11-09); the registry's rev has a prebuilt
  # dioxus-cli 0.7.10 closure in the binary-cache mirrors.
  # flake.lock pins the resolved rev for reproducibility.
  inputs.nixpkgs.url = "nixpkgs";

  outputs =
    { self, ... }@inputs:
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
      ];

      forEachSupportedSystem =
        f:
        inputs.nixpkgs.lib.genAttrs supportedSystems (
          system:
          f {
            inherit system;
            pkgs = import inputs.nixpkgs {
              inherit system;
            };
          }
        );
    in
    {
      devShells = forEachSupportedSystem (
        { pkgs, ... }:
        let
          rustcSysroot = pkgs.symlinkJoin {
            name = "rustc-sysroot-with-completion";
            paths = [ pkgs.rustc.unwrapped ];
            postBuild = ''
              mkdir -p $out/share/zsh/site-functions
              install ${pkgs.cargo}/share/zsh/site-functions/_cargo $out/share/zsh/site-functions/
            '';
          };
        in
        {
          default = pkgs.mkShellNoCC {
            packages = with pkgs; [
              cargo
              (symlinkJoin {
                name = "rustc-with-completion";
                paths = [ rustc ];
                nativeBuildInputs = [ makeWrapper ];
                postBuild = ''
                  rm $out/bin/rustc
                  makeWrapper ${rustc}/bin/rustc $out/bin/rustc \
                    --add-flags "--sysroot=${rustcSysroot}"
                '';
              })
              rustc
              rust-analyzer
              rustfmt
              clippy
              # web leg (S1 first-light): dioxus-cli 0.7.10 matches the
              # workspace dioxus dep; binaryen provides wasm-opt which
              # dx requires for release builds.
              dioxus-cli
              binaryen
            ];
            shellHook = ''
              # the dioxus-cli closure drags wasm-bindgen 0.2.126 onto PATH
              # ahead of the user's dirs, which would SHADOW the required
              # cargo-installed 0.2.130 — put ~/.local/bin first (verified
              # 2026-11-09: dx errors on any wasm-bindgen != 0.2.130).
              export PATH="$HOME/.local/bin:$PATH"
              # wasm-bindgen-cli is deliberately NOT in the shell:
              # dioxus 0.7.10 pins wasm-bindgen =0.2.130 (Cargo refuses a
              # --precise downgrade), nixpkgs ships only 0.2.127 and
              # older, and a PATH-included 0.2.127 would SHADOW the
              # cargo-installed 0.2.130 and break dx. Keep the pinned
              # one outside the flake:
              #   cargo install wasm-bindgen-cli --version 0.2.130 --locked
              # (it must be first on PATH, i.e. ~/.local/bin.)
              # nixpkgs rustc ships the wasm32-unknown-unknown std, so
              # `cargo check --target wasm32-unknown-unknown` and the
              # dx build both run inside this shell with no rustup.
              # Build the web bundle with (runbook: crates/tafy-pwa/README.md):
              #   rm -rf ~/.cache/cargo/dx/tafy-pwa
              #   dx build --release   # from crates/tafy-pwa
            '';
          };
        }
      );
    };
}
