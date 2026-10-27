# CoruFrame

<img src="docs/images/coruframe-icon.png" alt="CoruFrame icon" width="96">

[![Docs](https://img.shields.io/badge/docs-wiki-blue)](https://github.com/coruscnium/CoruFrame/wiki) [![License](https://img.shields.io/badge/license-GPL--3.0--only-blue)](LICENSE) [![Platform](https://img.shields.io/badge/platform-Linux-blue)](https://github.com/coruscnium/CoruFrame/wiki/Compatibility) [![Nix](https://img.shields.io/badge/Nix-flake-blue)](flake.nix)

A Linux-native Warframe companion: a relic-reward HUD, an inventory ledger, a relic planner and warframe.market tools. Rust, GPL-3.0-only, no telemetry, no server.

![CoruFrame collection browser](docs/images/app-collection.png)

## Features

- **Reward HUD.** Reads the relic reward screen locally and draws names, owned parts and prices on the layer-shell surface.
- **Inventory ledger.** Tracks drops and rewards from `EE.log`; an opt-in snapshot adds the full inventory and mastery.
- **Relic planner.** Ranks relics by the value of what they can drop.
- **Market tools.** Catalog, live prices, orders, presence and Rivens.
- **Nix.** A flake with NixOS and home-manager modules, pinned GTK, WebKit and Tesseract.

## Screenshots

![Inventory and market value](docs/images/app-inventory.png)

![Relic planner ranked by expected value](docs/images/app-relics.png)

![warframe.market item panel](docs/images/app-market.png)

![Trading orders](docs/images/app-trading.png)

## Install

The flake exposes `packages`, `overlays.default`, `nixosModules.default` and `homeManagerModules.default`.

```nix
# flake.nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    coruframe = {
      url = "github:coruscnium/CoruFrame/v1.0.1";
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
{ programs.coruframe.enable = true; }
```

For home-manager, import `coruframe.homeManagerModules.default` and set `programs.coruframe.enable = true`.

The input pins a release tag, so `nix flake update` stays on that version until you bump the ref yourself.

```sh
nix run github:coruscnium/CoruFrame/v1.0.1
```

`coruframe-app` installs both `coruframe-app` and `coruframe`.

Official support is the Nix flake. Source builds are documented in the [Installation guide](https://github.com/coruscnium/CoruFrame/wiki/Installation) but are not officially supported; issues are welcome, but try Nix first.

## Quick start

```sh
coruframe doctor                 # session, tools, capture backend, OCR
coruframe itemdb update          # build the item database cache
coruframe catalog update         # build the market catalog
coruframe-app                    # open the app, then enable the HUD in Settings
```

The [Usage guide](https://github.com/coruscnium/CoruFrame/wiki/Usage) covers the command list, the portal grant and the first run.

## Reward HUD

The probe tails `EE.log`. A `RelicExtraction` line arms it, it captures the portal source you grant once, OCRs each card and draws the panel for 20 seconds. A card is priced only at high OCR confidence; everything else shows `?`.

It needs `wlr-layer-shell`, Tesseract with `eng`, the English UI at 100% scale and a 16:9 frame. [Usage](https://github.com/coruscnium/CoruFrame/wiki/Usage) explains the toggle and the logs; [Troubleshooting](https://github.com/coruscnium/CoruFrame/wiki/Troubleshooting) covers a HUD that does not appear.

## Is this safe?

CoruFrame is unofficial and not affiliated with Digital Extremes. It never injects, writes game memory, synthesizes input or modifies game files. The opt-in inventory snapshot is a read-only memory read, off by default and named on its consent screen. DE's policy is that third-party software is used at your own risk; reading `EE.log` is what WFInfo and AlecaFrame have done for years, which is an observation, not a guarantee. [Safety](https://github.com/coruscnium/CoruFrame/wiki/Safety) has the details.

## Documentation

The [user guide](https://github.com/coruscnium/CoruFrame/wiki) is on the GitHub wiki:

- [Installation](https://github.com/coruscnium/CoruFrame/wiki/Installation)
- [Usage](https://github.com/coruscnium/CoruFrame/wiki/Usage)
- [Compatibility](https://github.com/coruscnium/CoruFrame/wiki/Compatibility)
- [Safety](https://github.com/coruscnium/CoruFrame/wiki/Safety)
- [Market-account](https://github.com/coruscnium/CoruFrame/wiki/Market-account)
- [Data-and-privacy](https://github.com/coruscnium/CoruFrame/wiki/Data-and-privacy)
- [Troubleshooting](https://github.com/coruscnium/CoruFrame/wiki/Troubleshooting)
- [Development](https://github.com/coruscnium/CoruFrame/wiki/Development): contributor entry point

Developer and legal records:

- [docs/legal/assets.md](docs/legal/assets.md): art and game-data provenance
- [docs/legal/dependencies.md](docs/legal/dependencies.md): every locked crate
- [CONTRIBUTING.md](CONTRIBUTING.md), [SECURITY.md](SECURITY.md), [CHANGELOG.md](CHANGELOG.md), [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)

## Contributing

Bug reports and patches are welcome. [CONTRIBUTING.md](CONTRIBUTING.md) covers the toolchain and the gates: fmt, clippy and tests in both workspaces, `pnpm build`, the UI audit and the docs check.

## License

GPL-3.0-only, except the Warframe art and game data in `app/src/assets/wf/`, `crates/cli/assets/` and `app/src-tauri/data/riven-base.json`, which stay Digital Extremes' property and are excluded from the GPL grant. See [docs/legal/assets.md](docs/legal/assets.md) for the provenance and the non-commercial fan-content notice, and [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for everything else.

CoruFrame is not affiliated with Digital Extremes.

Icon art by p33p4w.
