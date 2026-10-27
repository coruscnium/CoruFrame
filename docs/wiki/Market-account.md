# Market account

Signing in to warframe.market is optional and only needed for orders and presence. The catalog, prices and the rest of the app work signed out.

## Token storage

The session token lives in the freedesktop Secret Service (on KDE, the `kdewallet` collection) under service `xyz.coruscnium.coruframe`, user `warframe.market`.

The account password is sent once over TLS on curl's stdin and is never written to disk or to argv.

- The wallet is read lazily: the window paints first, and a non-secret `~/.config/coruframe/account-configured` marker skips the read entirely for a user with no account.
- A locked or dismissed wallet shows in Settings with a Retry action; it never falls back to a file and never blocks startup.
- Presence only connects after the account is connected.
- If the Secret Service is genuinely absent, the token falls back to `~/.config/coruframe/market-token` (0600) and Settings reports `storage = "file"` with a warning.
- The legacy plaintext token is migrated on the first load: write to the wallet, read back, compare, then delete the file. A failed read-back keeps the file.
- The device id is a random UUID in `~/.config/coruframe/market-device` (0600).

## Disconnect

Disconnect deletes both the wallet entry and the file. Rolling back to a pre-wallet build needs a re-login, because that build cannot read the wallet.

## Overrides

`CORUFRAME_TOKEN_STORE=file` pins the file store for one release. This is for debugging a wallet problem, not for normal use: the file store is less protected than the Secret Service.
