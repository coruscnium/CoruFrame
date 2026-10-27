# CoruFrame

![CoruFrame](../images/coruframe-icon.png)

A Linux-native Warframe companion: a relic-reward HUD, an inventory ledger, a relic planner and warframe.market tools. Rust, GPL-3.0-only, no telemetry, no server.

Current release: 1.0.0.

![Collection browser](../images/app-collection.png)

## Start here

- [Installation](Installation.md): NixOS, home-manager, other distributions and a manual build.
- [Usage](Usage.md): first run, the command list, the portal grant and the reward HUD.
- [Compatibility](Compatibility.md): sessions, compositors, tools and the capture backends.
- [Troubleshooting](Troubleshooting.md): a HUD that does not appear, a blank window, log paths and common questions.

## Supported setup

- KDE Plasma Wayland, sway or Hyprland. The HUD needs `wlr-layer-shell`.
- English game UI at 100% scale.
- Tesseract with `eng` for the reward OCR.
- A 16:9 frame. 16:10 and 21:9 use the same card geometry but have no live sign-off yet.

GNOME/Mutter has no `wlr-layer-shell` and gets no HUD. X11 and XWayland are not supported for the app; a Proton game may still run under XWayland.

## Is this safe?

CoruFrame is unofficial and not affiliated with Digital Extremes. It never injects, writes game memory, synthesizes input or modifies game files. The opt-in inventory snapshot is a read-only memory read, off by default and named on its consent screen. Digital Extremes' policy is that third-party software is used at your own risk; reading `EE.log` is what WFInfo and AlecaFrame have done for years, which is an observation, not a guarantee.

Read [Safety](Safety.md) before enabling the snapshot.

## Other pages

- [Market-account](Market-account.md): token storage, sign-in and disconnect.
- [Data-and-privacy](Data-and-privacy.md): what is read, where it is stored and how to remove it.
- [Development](Development.md): contributor entry points.
