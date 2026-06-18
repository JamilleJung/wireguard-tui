# 🛡️ Security Policy

## 📧 Reporting a vulnerability

Please report security issues **privately** - do not open a public issue for
anything exploitable.

- Preferred: GitHub **[Private vulnerability reporting](https://github.com/JamilleJung/wireguard-tui/security/advisories/new)**
  (Security → Report a vulnerability).
- Or email: **izeystudio@gmail.com**

Please include the version (or commit), your distro + package manager, repro
steps, and impact. Do **not** include real private keys or production configs.
Coordinated disclosure is appreciated - you'll get an acknowledgement as soon as
possible and be kept updated on a fix.

## 📋 Supported versions

This is an early project; only the latest release (and `main`) receive fixes.
If you are running an older version, please upgrade before reporting.

## 🔍 Threat model

`wg-tui` manages WireGuard tunnels under `/etc/wireguard`, which requires root.
The design goal is to keep the part that runs as root as small and auditable as
possible, and to keep everything else unprivileged.

**In scope** (things we actively defend against):

- A bug or hijacked environment in the **unprivileged UI** escalating to root.
- A user who can run the **helper** (via the passwordless grant) escalating
  beyond "manage WireGuard tunnels" to arbitrary root code execution.
- The privileged helper being tricked into touching files outside
  `/etc/wireguard` (path traversal) or running attacker-chosen commands.
- A malicious or malformed `.conf`/QR import corrupting existing tunnels or
  smuggling code that would run as root.
- Another local user reading your private keys through the helper.
- Truncated/lost configs from interrupted writes.

**Out of scope** (cannot be defended against here):

- An attacker who is **already root**, or who can already run code **as your
  user** (they can read your keys directly; pinning a binary path doesn't help).
- The security of WireGuard itself, the kernel module, or `wg`/`wg-quick`.
- Physical access / a compromised terminal emulator / clipboard history.

## 🔒 The privilege boundary

The TUI runs as your normal user. **The only thing that runs as root is one
small Rust helper binary**, `wg-helper` (`src/bin/wg-helper.rs` in source),
invoked as `sudo -n wg-helper <verb> [name]` (sudoers mode) or
`pkexec wg-helper ...` (polkit / fallback). Authorisation is scoped to
**exactly that one helper path**:

- the **sudoers** drop-in grants passwordless execution of only
  `/usr/local/lib/wireguard-tui/wg-helper` for your user;
- the **polkit** rule allows `pkexec` of only that program, and only for a user
  who is both in an **active local session** *and* a member of the **`wireguard`
  group** (the installer creates the group and adds you). The helper can read
  configs that contain private keys, so a passwordless grant must not be handed
  to every logged-in user; until you are in the group the rule denies and
  `pkexec` asks for the admin password (fail closed).

Because the grant is bound to the absolute helper path, pointing the app at a
different program (e.g. via `$WG_HELPER`) cannot silently gain root - it would
fall outside the sudoers/polkit grant and prompt or fail. In release builds the
helper-path override is additionally refused unless `WG_ALLOW_UNSAFE_HELPER=1`
is set and the target is an absolute, root-owned, non-world-writable file.

The helper itself:

- exports a **fixed `PATH`** (`/usr/sbin:/usr/bin:/sbin:/bin`) so a hijacked
  caller `PATH` can't redirect the `wg`/`wg-quick`/`logger` it runs as root;
- **validates every tunnel name** against `^[A-Za-z0-9][A-Za-z0-9_.-]{0,14}$`
  and rejects `..`, so `"$WG_DIR/<name>.conf"` can never escape `/etc/wireguard`;
- **rejects `PostUp` / `PreUp` / `PostDown` / `PreDown` script hooks** in any
  config it saves (see below) - blocked *at the privilege boundary*;
- **no `sh -c`** - all subprocess calls use argv arrays directly, each with a
  timeout;
- writes configs **atomically** (temp file with `O_EXCL` + `fsync` + `rename`,
  mode `600`) and keeps a **timestamped 0600 backup** before any overwrite,
  rename, or delete;
- validates the saved config shape in the helper before save/rename, in addition
  to the unprivileged frontend validation;
- **logs every mutating action** (with the invoking user) to the journal
  (`logger -t wireguard-tui`), with private/preshared keys redacted.

## 🚫 Script hooks are blocked

The `wg-quick` directives `PostUp` / `PreUp` / `PostDown` / `PreDown` execute
**arbitrary shell commands as root** when a tunnel is brought up or down. Left
unchecked, a config saved through the helper could turn the narrow "manage
tunnels" grant into full root.

To keep the privilege boundary meaningful, **the helper refuses to save or
rename any config that contains those directives** (the UI surfaces the error),
and a tunnel is only ever activated through `wg-quick`, never by interpreting
script lines itself. If you genuinely need a hook, edit the file under
`/etc/wireguard` directly as root - outside this constrained helper.

## 🔌 Kill switch scope

The helper can add/remove tunnel-scoped firewall rules for an active `wg-quick`
tunnel, preferring **nftables** (`inet filter`) with an iptables/ip6tables
fallback. The rules allow loopback, the tunnel interface, the tunnel's fwmark,
and (when `$SSH_CONNECTION` is set) established SSH return traffic, and reject
everything else. The iptables fallback **fails closed** on IPv6 if `ip6tables`
is missing rather than leaving v6 unprotected.

The kill switch is **not persistent** - no daemon, and the rules live only as
long as the helper-managed tunnel. They are torn down on deactivate/delete/
rename. A tunnel stopped by some other path (manual `wg-quick down`, a service
restart, a reboot) can leave the rules lingering until you next use the app;
toggling the kill switch off clears them. Do not rely on it as a permanent
system firewall.

## 🔑 Private keys and QR codes

- A tunnel `.conf` contains the interface **private key**. Files are written
  `0600`; backups are `0600` in `/etc/wireguard/.backup`.
- The **editor** opens a temporary copy of the config, created with `O_EXCL` and
  mode `0600` inside a per-user private directory (`$XDG_RUNTIME_DIR`, or a
  `0700` fallback), so another local user cannot pre-plant a symlink to read the
  key or steer your editor; it is removed afterwards.
- **Show QR** renders the full config - *including the private key* - as a QR
  code. Anyone who photographs your screen gets the key. Only display it when
  it's safe, and prefer a maximised window so it scans.
- **Copy public key** (`y`) sends only the *public* key via the OSC 52 terminal
  escape. Be aware terminal multiplexers/loggers can capture OSC 52 data.
- **Export** writes every tunnel's `.conf` into a `.zip`; that archive contains
  private keys. It is created `0600` and refuses to follow a symlink at the
  destination. Store it somewhere safe and delete it when done.

## ✅ Supply chain & verifying a download

- Each release ships a `SHA256SUMS` file, **signed with minisign**
  (`SHA256SUMS.minisig`; public key `minisign.pub`, committed here and attached
  to every release). Signing is **fail-closed**: the release aborts rather than
  publish unsigned artifacts.
- **All** GitHub Actions are pinned to commit SHAs (including first-party
  `actions/*`); `cargo-deb` is pinned to an exact version.
- The release token is **least-privilege**: read-only everywhere except the one
  job that publishes.
- CI runs `cargo audit` against the RUSTSEC advisory DB, and Dependabot watches
  the crate and Action pins.
- Verify a download with:

```sh
# 1) check the signature on the checksum file
minisign -Vm SHA256SUMS -P RWTyrstfFCLYkpMwbcyBRl+aGGcJikl35GY1esJDO6HTEJFIMvUC8f1Q
# 2) then verify the artifacts against it
sha256sum -c SHA256SUMS --ignore-missing
```

When in doubt, **build from source** - the project is pure Rust with no GUI/C
dependencies, so `cargo build --release` is reproducible on any supported distro.
