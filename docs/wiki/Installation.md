# Installation

Official support is the Nix flake. It produces the desktop app, the CLI, the desktop entry and the icons, and pins GTK, WebKit and Tesseract. A manual build is documented for development and for distributions that package CoruFrame themselves; it is not officially supported. Report issues, but try Nix first.

## Requirements

- A Wayland session. KDE Plasma, sway or Hyprland for the HUD. GNOME/Mutter has no `wlr-layer-shell` and gets no HUD. X11 and XWayland are not supported for the app or the HUD; a Proton game may run under XWayland.
- The English game UI at 100% scale. Other languages and scales are not calibrated.
- Tesseract with `eng` for the reward HUD. The Nix package carries both; a manual build needs `tesseract` on `PATH`.
- GTK 4.14 or newer, `gtk4-layer-shell` and PipeWire for the HUD and the portal capture. The Nix package supplies them.

`coruframe doctor` prints the detected session, compositor hints, tools, capture backend, layer-shell support, Secret Service presence, OCR readiness and the cache/config/data modes. Run it first when something does not work, and include its output in a bug report. See [Compatibility](Compatibility.md) for the matrix.

## Nix

The flake exposes `packages`, `overlays.default`, `nixosModules.default` and `homeManagerModules.default`.

### NixOS

Add the input, then import the module:

```nix
# flake.nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    coruframe = {
      url = "github:coruscnium/CoruFrame";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { nixpkgs, coruframe, ... }: {
    nixosConfigurations.host = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        ./configuration.nix
        coruframe.nixosModules.default
      ];
    };
  };
}
```

```nix
# configuration.nix
{
  programs.coruframe.enable = true;
  # programs.coruframe.package = ...;   # optional override
}
```

### home-manager

Add the same input, then either import the module into the home configuration:

```nix
home-manager.sharedModules = [ coruframe.homeManagerModules.default ];
```

or, with a standalone home configuration:

```nix
homeConfigurations."coru" = home-manager.lib.homeManagerConfiguration {
  pkgs = nixpkgs.legacyPackages.x86_64-linux;
  modules = [
    ./home.nix
    coruframe.homeManagerModules.default
  ];
};
```

```nix
# home.nix
{
  programs.coruframe.enable = true;
}
```

### Overlay or direct install

```nix
# use the package in an existing nixpkgs
nixpkgs.overlays = [ inputs.coruframe.overlays.default ];
environment.systemPackages = [ pkgs.coruframe-app ];
```

```sh
# no config at all
nix run github:coruscnium/CoruFrame
nix profile install github:coruscnium/CoruFrame#coruframe-app
```

`coruframe-app` installs both `coruframe-app` and `coruframe`.

### Building with the flake

```sh
nix build .#            # ./result/bin/coruframe-app, with ./result/bin/coruframe on its PATH
nix build .#coruframe   # the CLI alone
nix run .#              # build and launch the app
nix develop             # development shell (Rust, Node, pnpm, Tesseract, GTK)
```

`nix run .#probe -- <args>` runs an already-built `target/release/coruframe` (falling back to `target/debug/coruframe`) with the GTK libraries on its path. Set `CORUFRAME_BIN` to point it elsewhere.

## Manual build

Install Rust stable, Node 18.12+ (20 recommended), pnpm 10, Tesseract with `eng`, and the Tauri Linux dependencies: WebKitGTK 4.1, GTK3, JavaScriptCoreGTK 4.1, libsoup3, librsvg2, patchelf, GTK4 4.14 or newer, `gtk4-layer-shell`, PipeWire and libclang.

Per distribution:

**Debian 13 (trixie)**

```sh
sudo apt install build-essential pkg-config libssl-dev libwebkit2gtk-4.1-dev \
  libgtk-3-dev libgtk-4-dev libjavascriptcoregtk-4.1-dev libsoup-3.0-dev \
  librsvg2-dev patchelf libpipewire-0.3-dev libclang-dev meson ninja-build \
  tesseract-ocr tesseract-ocr-eng libgtk4-layer-shell-dev nodejs
sudo npm install -g pnpm@10
```

Debian bookworm's GTK 4.8 is too old; trixie ships 4.18. Install Rust with rustup: trixie's `rustc` 1.85 is below the 1.90 minimum.

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

**Ubuntu 24.04**

```sh
sudo apt install build-essential pkg-config libssl-dev libwebkit2gtk-4.1-dev \
  libgtk-3-dev libgtk-4-dev libjavascriptcoregtk-4.1-dev libsoup-3.0-dev \
  librsvg2-dev patchelf libpipewire-0.3-dev libclang-dev meson ninja-build \
  tesseract-ocr tesseract-ocr-eng nodejs npm
sudo npm install -g pnpm@10
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Ubuntu 24.04 has no `gtk4-layer-shell` package; build the pinned release:

```sh
curl -sSfL https://github.com/wmww/gtk4-layer-shell/archive/refs/tags/v1.3.0.tar.gz | tar xz
meson setup build gtk4-layer-shell-1.3.0 --prefix=/usr -Dintrospection=false -Dvapi=false
ninja -C build
sudo ninja -C build install
sudo ldconfig
```

**Arch Linux**

```sh
sudo pacman -S --needed base-devel rust nodejs pnpm pkgconf openssl \
  webkit2gtk-4.1 gtk3 gtk4 libsoup3 librsvg patchelf pipewire clang \
  meson ninja tesseract tesseract-data-eng gtk4-layer-shell
```

**Fedora 41+**

```sh
sudo dnf install webkit2gtk4.1-devel gtk3-devel gtk4-devel libsoup3-devel \
  librsvg2-devel patchelf pipewire-devel clang-devel tesseract tesseract-langpack-eng \
  nodejs pnpm rust cargo meson ninja-build gtk4-layer-shell-devel \
  pkgconf-pkg-config openssl-devel
```

Then build and run:

```sh
cd app
pnpm install
pnpm tauri build --no-bundle        # GUI at src-tauri/target/release/coruframe-app

cd ..                               # the CLI the app spawns for the HUD
cargo build --release -p coruframe
export PATH="$PWD/target/release:$PATH"
./app/src-tauri/target/release/coruframe-app
```

`--no-bundle` skips the platform bundle. There are no official binary packages; the Nix flake is the supported install. `pnpm tauri build` runs `pnpm build` for the frontend through `beforeBuildCommand`, so the frontend does not need a separate step.

The `PATH` export matters: the app starts the HUD probe by finding `coruframe` by name. Without the release directory on `PATH`, the HUD toggle fails to start. The alternative is to set `CORUFRAME_CLI` to the absolute path of the built CLI.

## First run

```sh
coruframe doctor
coruframe itemdb update
coruframe catalog update
coruframe-app
```

Then enable the reward HUD in Settings. Continue with [Usage](Usage.md).
