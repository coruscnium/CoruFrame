# Security

CoruFrame is a local, single-user Linux desktop app. This file states what it trusts, what it stores, and what it deliberately does not defend against.

## Supported versions

| Version | Supported |
|---|---|
| 1.0.x (current) | Yes |
| Older | No; upgrade to the newest 1.0.x |

Only the source tag and the flake at that tag are supported. A build from a dirty tree or a locally modified flake is not a release and gets no security support.

## Reporting

Open a private security advisory at <https://github.com/coruscnium/CoruFrame/security/advisories/new>, or contact the maintainer directly if the issue involves a credential or a way to make CoruFrame read the wrong memory. Do not include a real market token, wallet entry, or screenshot of another window in a report.

## Threat model

What is in scope:

- A remote API or catalog response that tries to escape a path, choose a URL host, inject a CSS token, or redirect a request to a non-HTTPS scheme.
- A wrong-window capture: a browser, editor or settings window whose title matches the game.
- A local file that an older build left world-readable (token, snapshot, frames, ledger, log).
- A malicious or malformed D-Bus screenshot reply, or a broken compositor geometry reply.
- A webview that renders untrusted item names, icons and prices.

What is out of scope:

- Any other process running as the same user. Secrets in memory are readable by that user, and `/proc` is not a trust boundary between processes of one UID.
- Root, kernel, ptrace, and a compromised compositor.
- Anti-cheat or EULA guarantees. The snapshot is a grey area and is documented as such.
- A browser or another app that the user deliberately points `CORUFRAME_CAPTURE_CMD` at.

## Trust boundaries

| Boundary | Trusted side | Untrusted side | Control |
|---|---|---|---|
| Network | curl with `--proto =https`, redirects capped at 3, response size capped | warframe.market, warframestat, DE CDN, GitHub | Validate slugs, ids, subtypes, cache names; fixed hosts for images |
| Webview | bundled assets | item names, icon paths, prices | Production CSP without `unsafe-inline`; URL and CSS escaping |
| Process memory | the game process | anything else | PID verification via `/proc/<pid>/cmdline` and `environ`; `O_RDONLY` snapshot |
| File system | `~/.config/coruframe`, `~/.cache/coruframe` 0700 | other users | 0600 files, `O_NOFOLLOW`, startup mode hardening |
| Wallet | Secret Service via keyring | the collection contents | service `xyz.coruscnium.coruframe`, user `warframe.market`; read-back before deleting the file |
| HUD payload | the probe | a stale or corrupt file | 0700 session directory, 0600 payload, deleted on every path, monitor/clamp geometry |
| HUD probe | one app-spawned child | EE.log, the portal, OCR output | app starts and stops exactly one child, gates on layer-shell and Tesseract, and kills its process group so an in-flight HUD dies with it |
| OCR log | the probe | reward text from the screen | `~/.cache/coruframe/*.log` at 0600, no frame bytes, rotated at 2 MB; the log view in Settings reads only that file |
| Debug capture | the user | a frame containing other players | opt-in only, 0600, 7-day TTL, never uploaded and never in an artifact |

## Secrets

- The warframe.market session token is the only long-lived credential. It lives in the Secret Service (KWallet's `kdewallet` on KDE) and only falls back to `~/.config/coruframe/market-token` (0600) when the Secret Service is genuinely unavailable, with a visible Settings warning.
- The account password exists only in the login request body, which is piped to curl on stdin. It is never in argv and never in a file.
- The non-secret `account-configured` marker lets startup skip the wallet for users with no account.
- The device id is a random UUID in `~/.config/coruframe/market-device` (0600).
- Snapshot diagnostics contain inventory offsets/counts; snippets are empty unless `CORUFRAME_SNAPSHOT_DEBUG=1` is set, and are redacted when collected.

## Same-UID limit

CoruFrame cannot protect you from another process running as your user. That process can read your config and cache, attach to the app, or read `/proc/<pid>/mem` if the kernel policy allows it. The 0600/0700 modes and the wallet exist to stop *other* users and accidental world-readable copies, not a same-UID attacker.

## Wallet scope

The wallet entry is scoped to this app and this account only. CoruFrame does not enumerate or read other wallet entries. A locked or cancelled wallet is reported and retried; it never silently falls back to a file. Disconnect deletes both the wallet entry and the legacy file.
