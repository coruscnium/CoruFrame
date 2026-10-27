# Safety

CoruFrame is unofficial, not affiliated with and not endorsed by Digital Extremes. Warframe and its assets are trademarks of Digital Extremes.

## What CoruFrame does

- It reads Warframe's `EE.log`, a plain local text file.
- It calls public APIs for item data, drop tables and market prices.
- It captures the screen for the reward OCR, using the source you grant once in the system portal.
- It never writes game memory, never injects into the game, never synthesizes input and never modifies game files.

The opt-in inventory snapshot is the only memory read. It is described below.

## Digital Extremes' position

DE's policy is that all third-party software is used at your own risk, and no allow-list exists. Reading `EE.log` is what WFInfo and AlecaFrame have done for years without bans; that is an observation, not a guarantee.

## The inventory snapshot

The opt-in snapshot reads the game process memory for the same inventory data Overwolf hands AlecaFrame. It is:

- off for fresh installs;
- local-only and read-only (`O_RDONLY`, no tracer, no writes);
- named on its consent screen, including EULA §2.h.

It is a grey area, it is host-observable, and it is not "undetectable". This project does not claim that it is. If you do not want that trade, leave the snapshot off and use the log-ledger features only.

## Reporting a problem

Security issues go to the private advisory linked from [SECURITY.md](https://github.com/coruscnium/CoruFrame/blob/main/SECURITY.md), not a public issue. Never paste a real market token, a wallet entry, or a screenshot containing another player's name.
