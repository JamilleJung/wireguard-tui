# Security policy

## Reporting a vulnerability

Report exploitable issues privately through
[GitHub private vulnerability reporting](https://github.com/JamilleJung/wireguard-tui/security/advisories/new)
or email **izeystudio@gmail.com**. Include the version/commit, distro, steps,
and impact; omit real private keys and production configs. Only the latest
release (**1.9.1**) and `main` receive fixes.

## Trust and scope

The normal UI runs unprivileged, and a Rust helper performs operations on
`/etc/wireguard` with root privilege. The intended boundary limits a helper
authorization grant to managing tunnels rather than arbitrary root commands.
It is a security design, not a guarantee that no exploitable bugs exist.

A trusted operator with helper access can read private tunnel keys, change
routing/DNS, connect/disconnect tunnels, and change supported firewall rules.
Do not grant access to every local user. The app does not isolate secrets from
root, another process running as the same user, a compromised editor/terminal,
clipboard history, screenshots, or a compromised WireGuard/kernel/toolchain.
It depends on trusted system files and administrative control of
`/etc/wireguard`, installed helper paths, and system tools.

## Authorization and privileged flows

The app uses an installed helper at a fixed standard path (including a
co-installed desktop helper), or a supported portable/development fallback.
It uses direct execution when root, otherwise probes scoped `sudo -n` and then
falls back to `pkexec`. A grant belongs to the helper executable path, not to
the UI executable.

- `install.sh` defaults to `/usr/local/lib/wireguard-tui/wg-helper` plus a
  per-user sudoers drop-in. A normal-user build is preserved even when the
  installer re-executes through `su`.
- `--polkit` installs a rule requiring an active **local** session and membership
  in the `wireguard` group. The source/prebuilt installer attempts group/user
  setup; verify it and re-login. An SSH session does not meet the local-session
  condition. Outside the rule, pkexec may use its normal admin authentication
  policy or fail if no agent is available.
- The `.deb` and distro recipes install the polkit rule but have no membership
  setup script. An administrator must arrange the trusted group separately;
  see [packaging](packaging/PACKAGING.md).
- A direct root install without a normal user requires `--allow-root` (or
  `WG_ALLOW_ROOT=1`). A source build may then run build scripts as root. Prefer
  a normal-user build or a verified prebuilt bundle.

Release builds ignore `WG_HELPER` unless `WG_ALLOW_UNSAFE_HELPER=1` and the
specified file is absolute, regular, root-owned, and not group/other writable.
Debug builds accept the override directly. These checks do not establish
trust in every parent directory or create an authorization grant. Treat helper
overrides and writable portable build directories as operator-controlled
inputs, especially when launching the UI as root.

The helper is not the only privileged code path: the installer and guided
`setup` may use the package manager, sudo/su, or fixed shell commands after the
user's installation request/confirmation. The updater runs a verified installer.

## Helper protections

The helper exposes fixed verbs: `list`, `active`, `read`, `dump`, `up`, `down`,
`save`, `rename`, `delete`, `enable`, `disable`, `is-enabled`, `sync`, `showconf`,
`persist`, `log`, `killswitch-status`, `killswitch-enable`, `killswitch-disable`.

It validates names before file operations: at most 15 ASCII bytes, first
character alphanumeric, remaining characters alphanumeric/`_`/`-`/`.`, with no
`..`, separators, or `.conf` suffix. Destination configs stay under the fixed
`/etc/wireguard` path; external programs are found in
`/usr/sbin:/usr/bin:/sbin:/bin` and invoked with argv arrays. Calls have bounded
process groups with concurrent output draining; timeout terminates the group.

Save/rename revalidate basic config shape and reject root script hooks. Writes
use exclusive `0600` temporary files, file synchronization, atomic rename, and
directory synchronization. Existing configs get timestamped `0600` backups in
`/etc/wireguard/.backup/` before overwrite, rename, deletion or save-live.
Mutation metadata is logged through `logger`, with secret-field redaction for
reported command errors. These protections do not replace access controls on
root-managed files or full validation by WireGuard itself.

## Script hooks and imported data

`PreUp`, `PostUp`, `PreDown`, and `PostDown` execute shell commands as root
through wg-quick. The helper refuses to save/rename configs containing them;
backup restore also rejects them during preflight. Valid inline `#` comments
do not hide a directive. If an administrator creates a config with hooks
externally, activation/deactivation can still execute those trusted commands:
the app does not promise to sandbox an existing root-managed config.

Backup restore validates all relevant archive entries before its first write:
valid top-level `.conf` names, no duplicate names or hooks, config validation,
at most 1 MiB per config, 16 MiB combined and 1024 configs. It ignores other file
types and bounds actual decompressed reads. Saves then happen sequentially;
operational failure can leave a partial restore, and running interfaces are
not synchronized by restore.

## Private keys, editor and clipboard

Saved configs and exports/backups contain private keys. Managed config writes
and ZIP exports use `0600`; export destinations reject symlinks. User backups
live under `$XDG_DATA_HOME/wireguard/backups` (default
`~/.local/share/wireguard/backups`) and are shared with the desktop client.

The external editor receives a `0600` temporary copy with exclusive creation,
under `$XDG_RUNTIME_DIR` when present or a per-user temporary directory intended
to be `0700`. That design relies on a trustworthy runtime/temp environment and
editor. Editor swap files or plugins have their own behavior. The app removes
its temporary copy after editing; it cannot guarantee cleanup after a crash.

`Q` exposes the complete config as a QR; `g` displays generated secret keys and
`c` can display private running config. `y` copies the focused public/detail
field, while log selection can copy multiple lines. Copy tries native
`wl-copy`/`xclip`/`xsel` tools before OSC 52, whose acceptance depends on the
terminal/multiplexer. Remote display forwarding, terminal recordings, and
clipboard history can affect where copied data goes or is retained.

## Kill switch limits

The helper requires an active tunnel, setting a runtime fwmark if missing.
It prefers nftables (`inet filter`) and otherwise uses iptables/ip6tables.
Rules allow loopback, the tunnel, its mark, and detected established SSH return
traffic; remaining host output is rejected. Rule comments match exact tunnel
names, and reject rules precede pre-existing allows in the managed chain. With
the iptables fallback, a host with non-loopback IPv6 and no `ip6tables` is
refused. This is not a general proof of leak prevention under every firewall
configuration, future IPv6 change, or simultaneous tunnel arrangement.

There is no daemon or persistence service. App deactivation/delete/rename
attempts cleanup; app exit alone does not remove rules. External tunnel stops
can leave blocking rules behind, and firewall reloads can remove protection.
Reboot normally clears runtime rules unless another firewall manager restores
them. A binary upgrade does not rewrite existing rules: toggle an active
switch off/on after updating. Keep an alternate administrative connection
when changing remote firewall state. The switch manages host output, not
server forwarding/NAT.

## Signed releases and updates

The release workflow refuses publication if minisign signing/verification
fails or `MINISIGN_KEY` is absent. It attaches four architecture tarballs,
the amd64 `.deb`, `SHA256SUMS`, `SHA256SUMS.minisig`, and `minisign.pub`.
GitHub Actions are SHA-pinned; release tools are version-pinned; the publishing
job alone has a write token. These measures reduce supply-chain risk; source
builds are not claimed to be reproducible or free of dependency risk.

Verify the checksum manifest with an independently trusted project key, then
require an `OK` result for the exact downloaded artifact:

```sh
minisign -Vm SHA256SUMS -P RWTyrstfFCLYkpMwbcyBRl+aGGcJikl35GY1esJDO6HTEJFIMvUC8f1Q
sha256sum -c SHA256SUMS --ignore-missing --strict
```

A key downloaded alongside an artifact needs independent comparison with the
committed/trusted key. A checksum alone does not authenticate its origin.

The startup update check contacts GitHub unless opted out. The updater validates
release-version syntax, uses HTTPS, verifies the signed manifest against the
**compiled-in** key, checks one exact tarball entry, and extracts into an
exclusive private `0700` directory. It then runs the verified installer with
prebuilt binaries. Missing verification tools or verification failure aborts
the update. It may attempt a package-manager install of minisign. The running
process is unchanged until restart. Use the package manager for distro-managed
installs to avoid mixing `/usr` and `/usr/local` layouts.

CI and release packaging run `cargo audit --deny warnings`; the current
lockfile passes without advisory exceptions. Audits cover known advisories,
not all possible defects. Release builds embed dependency metadata via
`cargo auditable build` **before UPX compression**. To inspect a shipped binary,
work on a copy, unpack it with `upx -d`, then use `cargo audit bin <file>`; do not
assume the audit command reads the packed ELF directly. See
[release details](docs/RELEASES.md).
