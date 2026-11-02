<div align="center">

# wireguard-tui - `wg-tui`

A native Linux terminal UI for plain WireGuard tunnels. Current release: **1.9.2**.

[![CI](https://github.com/JamilleJung/wireguard-tui/actions/workflows/ci.yml/badge.svg)](https://github.com/JamilleJung/wireguard-tui/actions/workflows/ci.yml)
[![Releases](https://img.shields.io/badge/Releases-latest-2ea44f)](https://github.com/JamilleJung/wireguard-tui/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

<img src="docs/screenshot.svg" alt="Illustrative wg-tui interface with fictional tunnel data" width="900">

</div>

*Illustrative UI example rendered from application code with fictional tunnel data.*

Built with Rust and ratatui, `wg-tui` manages `/etc/wireguard/*.conf` through
`wg`, `wg-quick`, and a small privileged Rust helper. It works in a local
terminal or over SSH without a graphical desktop, NetworkManager, or a daemon
of its own. The separate [wireguard-gui](https://github.com/JamilleJung/wireguard-gui)
project provides a desktop interface; the two frontends have different controls.

## What it does

- List, connect, disconnect, rename, and remove tunnels; show live peer details,
  handshake age, transfer totals, and throughput.
- Import `.conf` files or PNG/JPEG QR images, create a tunnel from a preset,
  edit raw config in an external editor, generate keys, and display QR codes.
- Apply WireGuard-level edits with `wg syncconf`, inspect running config, save
  live state, and toggle systemd start-on-boot.
- Copy focused detail fields and log lines, filter/save logs, and manage ZIP
  backups shared with the desktop client.
- Run connection diagnostics, read the in-app guide/changelog, and install
  verified release updates when a newer version is detected.
- Toggle a temporary helper-managed kill switch using nftables or
  iptables/ip6tables.

All keys are available immediately. Since 1.8.0 there is no Easy/Advanced mode
switch; **`m` has no action**. The "Advanced" group in `?` is just a key category.

## Install

### Source installer

Run as your normal user:

```sh
git clone https://github.com/JamilleJung/wireguard-tui.git
cd wireguard-tui
./install.sh
wg-tui doctor
wg-tui
```

The installer detects `apt-get`, `dnf`/`yum`, `pacman`, `zypper`, `apk`,
`xbps-install`, or `eopkg`. It installs runtime packages, a C build toolchain
when needed, and Rust through rustup if absent; then builds as the invoking
user. It installs under `/usr/local` by default, with `wg-helper` under
`/usr/local/lib/wireguard-tui/`, and creates the `wireguard-tui` command alias.

A terminal application-menu launcher and icon are installed **by default**.
Use `--no-desktop` for a server without a launcher. Installer options are:

| Option | Effect |
|---|---|
| `--sudoers` | Default: passwordless helper grant for the invoking user |
| `--polkit` | Install a group-restricted polkit rule instead |
| `--desktop` / `--no-desktop` | Install / skip the launcher and icon |
| `--allow-root` | Explicitly allow installation from a direct root login |
| `uninstall` | Remove installed app/helper/auth files; keep tunnel configs |

If usable sudo is unavailable, the installer can re-run through `su` using the
root password while retaining the invoking user for the build and helper grant.
A direct root login without a normal user is refused unless `--allow-root` (or
`WG_ALLOW_ROOT=1`) is set. That override can permit a source build as root;
prefer a normal user or a verified prebuilt bundle. `PREFIX` can change install
paths, but helper discovery and shipped polkit rules use the documented standard
paths. Custom layouts require matching authorization and an explicit supported
helper override; see CONTRIBUTING.md.

### Signed prebuilt releases

[Release 1.9.2](https://github.com/JamilleJung/wireguard-tui/releases/tag/v1.9.2)
provides:

| Artifact | Host |
|---|---|
| `wireguard-tui-1.9.2-x86_64-linux.tar.gz` | 64-bit Intel/AMD |
| `wireguard-tui-1.9.2-i686-linux.tar.gz` | 32-bit Intel/AMD |
| `wireguard-tui-1.9.2-aarch64-linux.tar.gz` | 64-bit ARM |
| `wireguard-tui-1.9.2-armv7-linux.tar.gz` | 32-bit ARMv7 |
| `wireguard-tui_1.9.2-1_amd64.deb` | Debian/Ubuntu amd64 |

The tarballs contain static musl application/helper binaries, `install.sh`,
installer assets under `packaging/`, README, and license. Choose the archive
matching `uname -m` (`armv7l` uses armv7). Verify it **before** extraction:

```sh
# Download the chosen artifact, SHA256SUMS, and SHA256SUMS.minisig first.
minisign -Vm SHA256SUMS -P RWTyrstfFCLYkpMwbcyBRl+aGGcJikl35GY1esJDO6HTEJFIMvUC8f1Q
sha256sum -c SHA256SUMS --ignore-missing --strict
# Require an OK line for the exact artifact you chose.
tar -xzf wireguard-tui-1.9.2-x86_64-linux.tar.gz
cd wireguard-tui-1.9.2-x86_64-linux
./install.sh
```

The bundle installer stages both adjacent binaries and skips Rust and compiler
setup. It still needs runtime packages and authorization. From a direct root
login, use `./install.sh --allow-root` after verification. The attached
`minisign.pub` should match the key committed in this repository; a signature
verified only against an untrusted key downloaded with the artifact is not a
trust check. Missing or invalid signing files are a reason to stop installation.

For a verified `.deb`, install with `sudo apt install ./wireguard-tui_1.9.2-1_amd64.deb`.
It includes a launcher and polkit rule but no script to create the `wireguard`
group or add users. See [authorization](#authorization) and the
[packaging guide](packaging/PACKAGING.md). Distro recipes are supplied for
AUR/RPM/Alpine/Void; their presence does not imply publication in a distro
repository.

## Usage and keys

The binary accepts `--version` / `-V`, `--help` / `-h`, `doctor`, and `setup`.
An unknown argument exits `2`. The `wireguard-tui` alias is created by
`install.sh`; distro packages may provide only `wg-tui`.

| Tunnels tab | Action |
|---|---|
| `Up`/`k`, `Down`/`j` | Select tunnel |
| `Enter` / `a` | Connect or disconnect |
| `n` | Name a tunnel, choose Interface/full/split preset, open editor |
| `i` | Import `.conf`, `.png`, `.jpg`, or `.jpeg` through file browser |
| `e`, `+` | Edit raw config / append a peer section in the editor |
| `g`, `c`, `p` | Generate keys / show running config / save live state |
| `d`, `R` | Confirm deletion / rename (stops an active tunnel) |
| `s`, `K` | Toggle systemd start-on-boot / temporary kill switch |
| `Q`, `x` | Show QR / export all configs to `~/wireguard-tunnels.zip` |
| `h`/`Left`, `l`/`Right`, `y` | Focus a detail field, then copy it |
| `D`, `H`, `w` | Diagnose / concepts guide / changelog |

| Log tab | Action |
|---|---|
| `Up`/`k`, `Down`/`j`, `PgUp`/`PgDn` | Move cursor / page |
| `Home`/`g`, `End`/`G`, `h`/`l` | Top / bottom / horizontal scroll |
| `f`, `t`, `/` | Follow newest / selected-tunnel filter / text filter |
| `y` / `Enter`, `v`, `Y` | Copy line or selection / visual select / copy filtered log |
| `c`, `r`, `w` | Clear view / reload / save full fetched log to `~/wireguard-tui-log.txt` |

`Tab`/`Shift+Tab` switches tabs. `B` opens backups from either tab; `n` creates,
`Enter`/`r` restores after confirmation, `d` confirms deletion, and `x` exports
the selected ZIP to your home directory. `?` shows the key map, `A` shows About,
`u` installs a detected update, and `q`/`Esc` quits (`Esc` first cancels an active
log selection). Popups consume keys; `Ctrl+C` exits the app.

The import browser uses `Space` to mark files, `Enter` on a file to import marked files,
`Right`/`l` to enter directories, `Left`/`h`/`Backspace` to go up, and `Esc` to
cancel. Hidden files are not listed.

Log filters affect the view and `Y` copy; `w` saves the full fetched snapshot.

Copy tries native clipboard tools first (`wl-copy`, `xclip`, `xsel`, with the
order adjusted for Wayland), then OSC 52. Each native helper attempt has a
2-second delivery/exit deadline. Over SSH, a working remote display
clipboard can take precedence; otherwise OSC 52 requires support in the local
terminal and multiplexer. The "copied" status for OSC 52 means the escape was
sent, not that the terminal accepted it.

Most helper-backed tunnel/config/import/backup/key-generation actions run on
serialized workers while navigation/help/log reading remain available. Conflicting
actions report that the app is busy. The external editor and cooked-terminal
update install deliberately take the foreground; local directory browsing,
clipboard attempts and log-file writes can still briefly wait. Leaving the app
does not roll back an operation that has already started. Backup actions return
to the main view; press `B` to reopen the manager.

## Editing, backups, and kill switch limits

Imported config files and external-editor output must be regular files at most
1 MiB. Named pipes and other special files are rejected without waiting for
a writer.

The editor is `$VISUAL`, then `$EDITOR`, then `nano`; command words support shell
quoting but no shell expansion or execution. Use a wrapper script for pipelines
or environment expansion. The app suspends and restores its existing terminal
state, opens a `0600` draft in an exclusive `0700` temporary directory, then
validates and cleans the directory, including editor swap/backup files. Configs remain plain text: inline `#` comments
and repeated Address/DNS/AllowedIPs are supported. The helper rejects script
hooks (`PreUp`, `PostUp`, `PreDown`, `PostDown`) on save/rename.

Editing an active tunnel attempts `wg syncconf`. Address/DNS/MTU/routing changes
still need a reconnect even if WireGuard-level synchronization succeeds.
Quick-add peer uses the same save and live-sync path as normal editing. Saving live state (`p`) can
replace saved WireGuard settings, so review the result.

User backups live under `$XDG_DATA_HOME/wireguard/backups` (an absolute XDG
path), or `~/.local/share/wireguard/backups`, shared by both applications.
Existing managed backup directories must belong to the current user and are
checked/tightened to `0700`; symlink directories are refused. ZIPs and exported
copies use `0600`. New backup names are unique even within one second and are
published without overwriting another archive.

Export reads every listed tunnel successfully or reports an error. It no longer
reports a successful incomplete archive after a helper/read failure. Exports
and saved logs are written to a completed private sibling file and then
published atomically: failed writes preserve the old destination, final
symlinks are refused, and replacing a hardlink does not modify its other names.

Restore accepts only valid, unique, top-level `.conf` entries. It rejects
malformed/duplicate archive names, invalid configs and hooks before its first
save. Limits are **64 MiB per ZIP**, **4,096 archive entries**, **1,024 configs**,
**1 MiB per config**, and **16 MiB total config text**. Unrelated files are
skipped. Backup reads require an owned regular file with one hard link and
refuse symlinks/FIFOs; copy a hardlinked archive to an ordinary file first.

After complete preflight, restore still saves sequentially. It overwrites
matching saved names, leaves other tunnels in place, and does not sync running
interfaces. A later operational failure can leave a partial restore; it is not
a multi-file transaction. Helper per-file backups remain a separate recovery
source under `/etc/wireguard/.backup/`. All these archives contain unencrypted
private keys.

Only **one helper-managed kill switch** can be active at a time across managed
tunnels. Turn the existing one off before enabling another. The helper requires
an already-active interface; it can assign an unused runtime WireGuard fwmark
without changing the saved config. It does not activate a stopped tunnel as a
side effect of enabling a switch.

The helper prefers nftables and requires a real hooked filter OUTPUT base chain.
It applies its complete rule set as one nft batch. The fallback requires
`iptables` and `iptables-restore`; when the kernel exposes IPv6 support it also
requires `ip6tables` and `ip6tables-restore`, even before a non-loopback IPv6
address is assigned. Both families are preflighted; commits are atomic **per
family**, not one transaction across IPv4 and IPv6. Failures are reported.

The rules allow loopback, the selected tunnel, its marked transport traffic,
and detected established SSH return traffic when `SSH_CONNECTION` reaches the
helper, then reject other host output ahead of existing allows. Exact comments
isolate tunnel names; status requires the complete expected rule set. This is
host-output control, not server forwarding/NAT or a general no-leak guarantee.
It may disrupt remote access or interact with another firewall manager.

Down/delete/rename retain protection if the interface is still active or its
state is unknown, and report stop/cleanup failures. A failed operation can
leave a tunnel stopped or partially changed; inspect the error and current
state before retrying. App exit does not clear rules. External service changes
may leave stale rules, and firewall reloads may remove them. No persistence
daemon is installed. After upgrading, reapply an existing switch (off/on) to
install current rules, planning for that brief unprotected interval and keeping
another administrative access path available.

Config validation is shared by the frontend and privileged helper. It checks
every directive occurrence, known sections/field placement, canonical 32-byte
WireGuard base64 keys (including padding bits), numeric CIDR/port/keepalive
bounds, and required peer fields. Inline comments, repeated Interface sections,
repeated Address/DNS/AllowedIPs, scoped IPv6 endpoints, named/numeric Table,
FwMark hex/off and SaveConfig booleans are supported. Unknown fields or malformed
sections stay visible in raw text but cannot be saved until corrected. If a
PresharedKey directive is present it must contain a valid key; omit it to use
no PSK. The helper independently rejects all four root script-hook directives.

## Doctor, setup, and updates

`wg-tui doctor` checks tools, helper/authorization, `/etc/wireguard`, resolvconf
integration, systemd/journal availability, and Linux clock synchronization. It
is read-only and exits `0` (no warnings), `1` (warnings), or `2` (critical missing
requirements). It does not test the kernel module or prove connectivity.

`wg-tui setup` offers confirmed package installation for WireGuard tools and a
resolver provider, or activation of an inactive `resolvectl` shim. It points to
the installer for the helper and returns the refreshed doctor status. It does
not create tunnels, create `/etc/wireguard`, or enable tunnel start-on-boot.
The helper creates `/etc/wireguard` on the first save. `D` diagnoses a selected
tunnel using clock/handshake checks, endpoint ping, and DNS lookup; blocked ICMP
can make the ping inconclusive.

At startup the app checks GitHub for a newer release without blocking the UI.
`u` installs only when that startup check found one; it does not force a new
check. The updater uses HTTPS, a compiled-in minisign public key, an exact
archive checksum, and a private temporary directory. It re-runs the verified
installer using prebuilt binaries, may prompt for privilege, and requires an
app restart. Prefer your package manager for a distro-managed installation:
the updater uses the source installer's `/usr/local` layout and default sudoers
mode. Disable the startup check with `WG_NO_UPDATE_CHECK=1` or a persistent
`$XDG_CONFIG_HOME/wireguard-tui/no-update` file (default `~/.config/...`).

## Authorization

A passwordless helper grant can read private tunnel keys and change networking.
Give it only to trusted operators. The source installer defaults to a per-user
sudoers grant; polkit instead requires an active local session **and** membership
in `wireguard`. `--polkit` attempts group creation and membership setup; verify
membership with `id -nG` after logging out and back in. Package recipes only
install the rule, so an administrator must arrange membership separately.
An SSH session does not satisfy the active-local rule; use the scoped sudoers
installation or an administrator-approved privilege path.

The app tries direct execution when already root, otherwise authorized
`sudo -n`, then `pkexec`. It selects only its own trusted helper paths. Helper
verbs, fixed paths, atomic writes, backup behavior, updater trust, and known
security limits are detailed in [SECURITY.md](SECURITY.md). QR codes, generated
keys, running config, and exported backups may reveal private keys.

## Audited helper, installer and update behavior

Release helper discovery uses this project's own standard `/usr/local/lib/`
and `/usr/lib/` paths, or an adjacent **trusted** executable. The executable must
be root-owned, executable and not group/other writable; its directory ancestry
must also be trusted. A normal user-owned extracted helper is not automatically
trusted: install the verified bundle first. Release builds no longer fall back
to the source build directory or another application's helper. Debug overrides
remain available for development; they do not create an authorization grant.

Background read-only requests use a noninteractive polkit check before pkexec,
so missing authorization produces an actionable error instead of repeated
password dialogs. Explicit mutating actions can still request authentication.
Helper IPC drains input/output concurrently, retains at most 4 MiB per output
stream, and has an overall 90-second deadline plus bounded pipe cleanup. Local
key-generation commands have a 10-second deadline. A privileged process that
the UI user cannot signal may finish independently after a UI-side timeout;
check actual tunnel state before retrying.

Before privileged writes or helper grants, `PREFIX` must be an absolute ASCII
path containing only letters, digits, `/`, `_`, `.` and `-`, with no empty,
`.` or `..` components or trailing slash. Existing directory ancestors must be
root-owned without group/other write permission; symlinks are refused. A
group-writable `/usr/local` can therefore fail these checks. An administrator
must choose or prepare a protected path; the installer does not take ownership
of caller directories. `--polkit` supports only `/usr` or `/usr/local`, matching
the bundled rule. Custom sudoers prefixes still require matching frontend helper
discovery configuration.

Installer options are validated; unknown options fail instead of being ignored.
Source builds use the lockfile. Switching sudoers/polkit modes installs and
validates the selected grant before removing the previous grant. Failed sudoers
validation or group enrollment reports failure and keeps prior authorization.
`--polkit` requires pkexec up front; it does not install a rule and then claim
success without the escalation tool. Packages still do not enroll users.

Verified updates retain ownership of their private download directory through
installation and clean it up on download, staging or installation failure and
normal completion. Before privilege escalation, the whole required bundle is
checked: regular single-link application/helper/installer/assets, no symlink
components, and a fresh exclusive staging tree. Missing/unsafe assets fail before
installation starts. HTTPS-only redirects are enforced. TUI automatic updates select its
published x86_64/i686/aarch64/armv7 builds; unsupported CPUs fail explicitly.

Update download rechecks that the verified version is newer than the running
version. The completed update reports that verified version, and still needs
an application restart. Prefer a package-manager upgrade for a packaged install.

## Development and further reading

```sh
cargo build --release --locked
cargo run --release --locked --bin wg-tui -- --help
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the full check list and helper override
rules. The application and helper are separate binaries; the project does not
bundle `wg`/`wg-quick`, kernel modules, or an editor.

- [Step-by-step tutorial](docs/TUTORIAL.md)
- [WireGuard concepts](docs/help.md)
- [Distro and DNS guide](docs/DISTROS.md)
- [Release verification/build process](docs/RELEASES.md)
- [Changelog](CHANGELOG.md)
- [Packaging](packaging/PACKAGING.md)

If the app is useful, [star the project](https://github.com/JamilleJung/wireguard-tui)
or [buy the maintainer a coffee](https://www.buymeacoffee.com/jamillejung).

MIT licensed. WireGuard is a registered trademark of Jason A. Donenfeld; this
is an independent, unofficial project.
