# When adding new tools to this template, always check the official registry to
# find the correct attribute name:
# * Search Tool: NixOS Package Search
# * Usage: If you find ripgrep, simply add pkgs.ripgrep to the buildInputs in your flake.nix.
{
  description = "Rust Template with automatic pre-commit hooks";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      rust-overlay,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        extensions = with pkgs.vscode-extensions; [
          rust-lang.rust-analyzer
          tamasfe.even-better-toml
          jnoortheen.nix-ide
          vadimcn.vscode-lldb
          redhat.vscode-yaml
          # ryanluker.vscode-coverage-gutters
          # Note: If swellaby is not in your nixpkgs channel,
          # you may need to use a community overlay or skip it here.
          # swellaby.vscode-rust-test-adapter
          # vscode-extensions.nefrob.vscode-just-syntax
        ];

        # Define the rust toolchain from your toml
        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

        # musl cross toolchain for fully static amd64 builds (`just deb`).
        # `cross` can't drive the Nix-provided rustc, so we build musl natively:
        # rustc gets the musl std (added in rust-toolchain.toml) and the `cc`/`zstd`
        # crates compile their C against this musl gcc instead of the host glibc.
        muslCc = pkgs.pkgsCross.musl64.stdenv.cc;
        muslCcBin = "${muslCc}/bin/${muslCc.targetPrefix}cc";
        muslArBin = "${muslCc.bintools.bintools}/bin/${muslCc.targetPrefix}ar";

        # Read project metadata from Cargo.toml for reuse below
        cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
      in
      {
        # `nix build` and `nix run` — builds the release binary
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = cargoToml.package.name;
          version = cargoToml.package.version;
          src = ./.;
          # Cargo.lock must be committed (true for executables — see .gitignore)
          cargoLock.lockFile = ./Cargo.lock;
          # reqwest loads the system CA store even for the loopback test
          # servers; the build sandbox has none (cacert sets SSL_CERT_FILE).
          nativeCheckInputs = [ pkgs.cacert ];
        };

        # `nix flake check`; build one with `nix build .#checks.<system>.module-vm`
        checks = pkgs.lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
          module-eval = import ./nix/tests/eval.nix {
            inherit pkgs;
            inherit (nixpkgs.lib) nixosSystem;
            module = self.nixosModules.default;
          };
          module-vm = import ./nix/tests/vm.nix {
            inherit pkgs;
            module = self.nixosModules.default;
          };
        };

        devShells.default = pkgs.mkShell {
          # Point cargo/cc at the musl cross toolchain for `--target
          # x86_64-unknown-linux-musl` (used by `just deb`). Underscored target
          # names are what the `cc` crate and cargo expect.
          CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER = muslCcBin;
          CC_x86_64_unknown_linux_musl = muslCcBin;
          AR_x86_64_unknown_linux_musl = muslArBin;

          # Faster linker for native glibc builds. Set here rather than in
          # .cargo/config.toml so environments without mold (CI) still link.
          CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS = "-C link-arg=-fuse-ld=mold";

          buildInputs = with pkgs; [
            rustToolchain
            cargo-dist
            cargo-nextest
            cargo-cross
            cargo-machete
            cargo-audit
            cargo-deny
            cargo-outdated # Reports dependencies with newer releases available
            cargo-llvm-cov # Coverage instrumentation via LLVM
            grcov # Coverage report formatter (lcov, html, cobertura)
            jaq # JSON processor (used by coverage-check)
            prek # pre-commit
            gitleaks # The compiled secret scanner or trufflehog or ripgrep
            just # Command runner
            bacon # Background checker (cargo check/clippy/test on save)
            git-cliff # Changelog generator from conventional commits
            mold # Fast linker (referenced in .cargo/config.toml)
            nil # Nix language server (referenced in .vscode/settings.json)
          ];

          # This runs when the shell starts
          shellHook = ''
            echo "Rust Dev Shell Loaded"

            # Enrich the system-installed VSCodium with our extensions,
            # only adding those not already present in the user's profile.
            if command -v codium >/dev/null 2>&1; then
              ext_dir="$HOME/.vscode-oss/extensions"
              mkdir -p "$ext_dir"
              installed=$(codium --list-extensions 2>/dev/null | tr '[:upper:]' '[:lower:]')
              added=0
              for ext_pkg in ${pkgs.lib.concatMapStringsSep " " (e: "${e}") extensions}; do
                for src in "$ext_pkg"/share/vscode/extensions/*/; do
                  [ -d "$src" ] || continue
                  name=$(basename "$src")
                  lower=$(printf '%s' "$name" | tr '[:upper:]' '[:lower:]')
                  if ! printf '%s\n' "$installed" | grep -qxF "$lower"; then
                    cp -rL --no-preserve=mode,ownership "$src" "$ext_dir/$name"
                    echo "  + added VSCodium extension: $name"
                    added=$((added + 1))
                  fi
                done
              done
              if [ "$added" -eq 0 ]; then
                echo "Tip: Run 'codium .' — all flake-provided extensions already installed."
              else
                echo "Tip: Restart VSCodium to pick up $added new extension(s)."
              fi
            else
              echo "Note: 'codium' not found on PATH — skipping extension seeding."
            fi

            # Automatically install hooks if .git exists
            if [ -d .git ] && command -v prek >/dev/null; then
              prek install
            fi
          '';
        };
      }
    )
    // {
      # `imports = [ frust.nixosModules.default ];` then `services.frust.enable = true;`
      nixosModules.default =
        { lib, pkgs, ... }:
        {
          imports = [ ./nix/module.nix ];
          services.frust.package = lib.mkDefault self.packages.${pkgs.stdenv.hostPlatform.system}.default;
        };
    };
}
