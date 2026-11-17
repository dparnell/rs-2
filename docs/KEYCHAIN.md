# The system keychain

rs2 keeps secrets in one place: a keychain owned by the kernel. The first
users are WiFi passphrases. Before the keychain, they sat as plain text in
`\MPTN\ETC\WIFI.CFG`.

## Where things live

| Piece | Where |
|-------|-------|
| File format, encryption | `crates/keychain` (host-tested) |
| Cryptography | `crates/rs2crypto` (SHA-1, HMAC, PBKDF2, AES-128, CTR, key wrap) |
| The open keychain, saving, access rules | `kernel/src/keychain.rs` |
| Programs' interface | RS2API 630-635, `kernel/src/api/keychain.rs` |
| Command | `KEYCHAIN.EXE` (`userland/netapps/src/bin/keychain.rs`) |
| Stored in | `C:\RS2\KEYCHAIN\KEYCHAIN.DAT` |

## Entries

An entry is a secret named by a **service** and an **account**. For a
WiFi network that is `wifi` and the SSID. There are two kinds:

- **System entries.** Programs may store, replace and delete them, but
  only the kernel reads them back. WiFi passphrases are system entries.
- **Readable entries** (`KEYCHAIN SET ... /READABLE`). Any program may
  read them with Rs2KeyGet. OS/2 has no users or process identities, so
  "readable" means every program can read it.

## What protects it

1. **Programs cannot reach the file.** Any program path inside
   `C:\RS2\KEYCHAIN` gets ERROR_ACCESS_DENIED: open, list, copy, delete
   and the rest. The check is in `api::file::resolve`, which every path a
   program gives passes through. Moving or renaming `C:\` or `C:\RS2` is
   refused, and a DosCopy of a tree skips the keychain directory.
   Raw disk access (DASD opens) is not blocked. That is what the
   encryption is for.
2. **The file is encrypted.**
   - The entries are AES-128-CTR encrypted with a random master key, and
     the whole file carries an HMAC-SHA1. A changed or damaged file is
     refused and left alone.
   - The master key is wrapped (RFC 3394) with a key-encryption key.
     - **No master password (the default):** that key comes from the
       file's own salt. The secrets are not stored as plain text, but
       anyone who has the file (for example by reading the disk from
       another OS) can decrypt them.
     - **With a master password** (`KEYCHAIN PASSWORD`): the key is
       PBKDF2-HMAC-SHA1(password, salt, 20000 iterations). Without the
       password the file is useless.
   - **Locked at boot:** a keychain with a master password stays locked
     until `KEYCHAIN UNLOCK`. Until then a WiFi network that needs its
     saved passphrase is not joined; WIFI STATUS says the keychain is
     locked. Unlocking wakes the WiFi manager.

The format is in the header comment of `crates/keychain/src/lib.rs`.
Saves write `KEYCHAIN.NEW` first, then rename it over `KEYCHAIN.DAT`.

## WiFi

- `WIFI CONNECT ssid passphrase` saves the network in WIFI.CFG without
  the passphrase. The kernel stores the passphrase in the keychain
  (`wifi`, ssid).
- A `PASSPHRASE=` line still in WIFI.CFG (written by hand or by an older
  WIFI.EXE) is moved into the keychain when the file is read. The file
  is then rewritten without it.
- `WIFI FORGET ssid` removes the network and its keychain entry.

## Commands

```
KEYCHAIN                         state and entries (never secrets)
KEYCHAIN SET service account [secret] [/READABLE]
KEYCHAIN GET service account     a readable entry's secret
KEYCHAIN DELETE service account
KEYCHAIN PASSWORD                set, change or remove the master password
KEYCHAIN LOCK | UNLOCK [password]
```

## Not done yet

- **No program identity.** Every program can read readable entries.
  Tying an entry to the program that stored it would need process
  identities.
- **No TPM.** The Surface Pro 4 has a TPM 2.0 that could hold the master
  key, which would give at-rest protection without a password.
- **Live stick:** C: is a RAM disk there, so the keychain goes away at
  reboot.
