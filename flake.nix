{
  description = "CoruFrame — Linux-native Warframe companion";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs, ... }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
      lib = pkgs.lib;
      version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).workspace.package.version;

      cli = pkgs.rustPlatform.buildRustPackage {
        pname = "coruframe";
        inherit version;
        src = ./.;
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [
          "-p"
          "coruframe"
        ];
        doCheck = false;
        nativeBuildInputs = with pkgs; [
          pkg-config
          wrapGAppsHook3
          gobject-introspection
          makeWrapper
          rustPlatform.bindgenHook
        ];
        buildInputs = with pkgs; [
          gtk4
          gtk4-layer-shell
          graphene
          gdk-pixbuf
          glib
          pango
          cairo
          librsvg
          harfbuzz
          wayland
          libxkbcommon
          pipewire
          openssl
          gsettings-desktop-schemas
        ];
        # Tesseract is a process, not a link dependency; the wrapper puts it on
        # PATH so `ocr-eval` and the HUD work without a second Nix install.
        postFixup = ''
          wrapProgram $out/bin/coruframe \
            --prefix PATH : ${lib.makeBinPath [ pkgs.tesseract ]}
        '';
        meta = {
          description = "Relic-reward HUD, inventory ledger and market tooling for Warframe";
          homepage = "https://github.com/coruscnium/CoruFrame";
          license = lib.licenses.gpl3Only;
          mainProgram = "coruframe";
          platforms = [ "x86_64-linux" ];
        };
      };

      # The full application: the Tauri GUI from its own workspace, plus the CLI
      # on its PATH. The Tauri hook supplies the desktop entry and icons; no deb
      # or other binary artifact is published.
      app = pkgs.rustPlatform.buildRustPackage {
        pname = "coruframe-app";
        inherit version;
        src = ./.;

        pnpmRoot = "app";
        pnpmDeps = pkgs.fetchPnpmDeps {
          pname = "coruframe-app";
          inherit version;
          src = ./.;
          pnpm = pkgs.pnpm_10;
          postPatch = "cd app";
          fetcherVersion = 4;
          hash = "sha256-saCWM7cbtb2NIj5CGr+46Y8AHDsLBexpzZclMuSqeZE=";
        };

        cargoRoot = "app/src-tauri";
        buildAndTestSubdir = "app/src-tauri";
        cargoHash = "sha256-zY4BA1ttly6qDL+qUOxMJ2Te34jyI8Ho0k8ZTvAnovk=";
        doCheck = false;

        nativeBuildInputs = with pkgs; [
          cargo-tauri.hook
          nodejs
          pnpmConfigHook
          pnpm_10
          pkg-config
          wrapGAppsHook3
          rustPlatform.bindgenHook
        ];
        buildInputs = with pkgs; [
          webkitgtk_4_1
          libsoup_3
          glib-networking
          gtk3
          gtk4
          gtk4-layer-shell
          graphene
          pipewire
          openssl
          gsettings-desktop-schemas
        ];

        env.OPENSSL_NO_VENDOR = 1;

        # The app spawns the CLI for the HUD; install it alongside.
        # The hook writes a relative Exec= and a themed icon name; both need to
        # be absolute because PATH and icon lookup are unreliable from an entry.
        postInstall = ''
          ln -s ${cli}/bin/coruframe $out/bin/coruframe
          substituteInPlace $out/share/applications/CoruFrame.desktop \
            --replace-fail "Exec=coruframe-app" "Exec=$out/bin/coruframe-app" \
            --replace-fail "Icon=coruframe-app" "Icon=$out/share/icons/hicolor/512x512/apps/coruframe-app.png"
          # KWin matches the taskbar icon by Wayland app_id, which is the binary name.
          mv $out/share/applications/CoruFrame.desktop $out/share/applications/coruframe-app.desktop
        '';

        postFixup = ''
          wrapProgram $out/bin/coruframe-app \
            --prefix PATH : ${lib.makeBinPath [ cli pkgs.tesseract ]}
        '';

        meta = {
          description = "CoruFrame desktop app: inventory ledger, relic planner and HUD";
          homepage = "https://github.com/coruscnium/CoruFrame";
          license = lib.licenses.gpl3Only;
          mainProgram = "coruframe-app";
          platforms = [ "x86_64-linux" ];
        };
      };

      # The package a consumer gets. It follows their nixpkgs when the input is
      # declared with `inputs.coruframe.inputs.nixpkgs.follows = "nixpkgs";`,
      # so the module and overlay never pull a second nixpkgs.
      packageFor = pkgs: self.packages.${pkgs.stdenv.hostPlatform.system}.coruframe-app;

      coruframeModule =
        { config, lib, pkgs, ... }:
        let
          cfg = config.programs.coruframe;
        in
        {
          options.programs.coruframe = {
            enable = lib.mkEnableOption "CoruFrame";
            package = lib.mkOption {
              type = lib.types.package;
              default = packageFor pkgs;
              defaultText = lib.literalExpression "inputs.coruframe.packages.\${pkgs.system}.coruframe-app";
            };
          };
          config = lib.mkIf cfg.enable {
            environment.systemPackages = [ cfg.package ];
          };
        };

      coruframeHomeModule =
        { config, lib, pkgs, ... }:
        let
          cfg = config.programs.coruframe;
        in
        {
          options.programs.coruframe = {
            enable = lib.mkEnableOption "CoruFrame";
            package = lib.mkOption {
              type = lib.types.package;
              default = packageFor pkgs;
              defaultText = lib.literalExpression "inputs.coruframe.packages.\${pkgs.system}.coruframe-app";
            };
          };
          config = lib.mkIf cfg.enable {
            home.packages = [ cfg.package ];
          };
        };

      # Runs the already-built binary with its runtime dependencies in place, so
      # the probe does not need a `nix develop` shell whose link warnings bury
      # its output. `nix build` produces the packaged binary instead.
      probe = pkgs.writeShellScriptBin "coruframe" ''
        export PATH=${lib.makeBinPath [ pkgs.tesseract pkgs.kdotool ]}:$PATH
        export LD_LIBRARY_PATH=${pkgs.lib.makeLibraryPath [ pkgs.gtk4 pkgs.gtk4-layer-shell pkgs.graphene pkgs.pipewire ]}''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}
        bin="''${CORUFRAME_BIN:-}"
        if [ -z "$bin" ]; then
          if [ -x "$PWD/target/release/coruframe" ]; then
            bin="$PWD/target/release/coruframe"
          else
            bin="$PWD/target/debug/coruframe"
          fi
        fi
        if [ ! -x "$bin" ]; then
          echo "coruframe: no binary at $bin; run cargo build --release (or cargo build) or set CORUFRAME_BIN" >&2
          exit 2
        fi
        exec "$bin" "$@"
      '';
    in
    {
      packages.${system} = {
        default = app;
        coruframe-app = app;
        coruframe = cli;
        probe = probe;
      };

      apps.${system} = {
        default = {
          type = "app";
          program = "${app}/bin/coruframe-app";
        };
        probe = {
          type = "app";
          program = "${probe}/bin/coruframe";
        };
      };
      checks.${system}.default = app;

      overlays.default = final: _prev: {
        coruframe = self.packages.${final.stdenv.hostPlatform.system}.coruframe;
        coruframe-app = self.packages.${final.stdenv.hostPlatform.system}.coruframe-app;
      };
      nixosModules.default = coruframeModule;
      homeManagerModules.default = coruframeHomeModule;

      devShells.${system}.default = pkgs.mkShell {
        NIX_NO_SELF_RPATH = "1";
        # The pipewire crate builds its bindings with bindgen.
        LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
        BINDGEN_EXTRA_CLANG_ARGS = "-isystem ${pkgs.glibc.dev}/include";

        packages = with pkgs; [
          cargo
          rustc
          rust-analyzer
          rustfmt
          clippy
          nodejs
          pnpm
          tesseract
          kdotool
          grim
          pkg-config
          gobject-introspection
          wrapGAppsHook3
          gtk3
          gtk4
          graphene
          gtk4-layer-shell
          pipewire
          webkitgtk_4_1
          libsoup_3
          glib
          cairo
          pango
          gdk-pixbuf
          atk
          librsvg
          openssl
          dbus
          libGL
          wayland
          libxkbcommon
          vulkan-loader
        ];
      };
    };
}
