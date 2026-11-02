# Releases and verification

Current release: [1.9.3](https://github.com/JamilleJung/wireguard-tui/releases/tag/v1.9.3).
Use the [release list](https://github.com/JamilleJung/wireguard-tui/releases) or
[latest release](https://github.com/JamilleJung/wireguard-tui/releases/latest)
for downloads. The [changelog](../CHANGELOG.md) is the canonical history.
The desktop sibling shares project/backend work and may use the same release
number; terminal and desktop controls/features are not identical.

## Artifacts

| Artifact | Build |
|---|---|
| `wireguard-tui-1.9.3-x86_64-linux.tar.gz` | Static musl, 64-bit Intel/AMD |
| `wireguard-tui-1.9.3-i686-linux.tar.gz` | Static musl, 32-bit Intel/AMD |
| `wireguard-tui-1.9.3-aarch64-linux.tar.gz` | Static musl, 64-bit ARM |
| `wireguard-tui-1.9.3-armv7-linux.tar.gz` | Static musl, 32-bit ARMv7 |
| `wireguard-tui_1.9.3-1_amd64.deb` | Debian/Ubuntu amd64 package |
| `SHA256SUMS`, `SHA256SUMS.minisig`, `minisign.pub` | Checksum manifest, signature, verification key |

A release tarball has `wg-tui`, `wg-helper`, `install.sh`, README/license, and
polkit/desktop/icon files under `packaging/`. The source-code archives provided
by GitHub are different: they contain Cargo sources and require compilation.
The tarballs need no Rust compiler at installation but still use runtime tools
and authorization. `uname -m` helps choose a CPU archive; `armv7l` maps to armv7.

## Verify before installation

Download the chosen artifact and both manifest/signature files. Verify the
manifest using an independently trusted project key:

```sh
minisign -Vm SHA256SUMS -P RWTyrstfFCLYkpMwbcyBRl+aGGcJikl35GY1esJDO6HTEJFIMvUC8f1Q
sha256sum -c SHA256SUMS --ignore-missing --strict
```

Require an `OK` line for the **exact artifact** you intend to install. The
`--ignore-missing` option allows checking a subset rather than downloading all
architectures. Missing/invalid signature data or a failed artifact check means
stop. The attached `minisign.pub` is a convenient copy, but verify that it
matches a previously trusted/committed key before using `-p minisign.pub`.
Checksums alone cannot authenticate a release.

For a verified tarball, extract it and run its installer as your normal user:

```sh
tar -xzf wireguard-tui-1.9.3-x86_64-linux.tar.gz
cd wireguard-tui-1.9.3-x86_64-linux
./install.sh
```

Both adjacent binaries are automatically staged; Rust/compiler installation is
skipped. Runtime packages and helper authorization are still configured. The
installer defaults to `/usr/local`, a scoped sudoers grant, and a terminal
launcher/icon. Use `--no-desktop` to skip launcher files, or `--polkit` for
active-local/group-based authorization. From a direct root login use the
explicit `--allow-root` override after verification.

For the verified package, use
`sudo apt install ./wireguard-tui_1.9.3-1_amd64.deb`. The `.deb` ships a polkit
rule and desktop assets but no group-membership setup script. Follow the
[packaging guide](../packaging/PACKAGING.md), especially for SSH access.
The source/prebuilt installer adds the `wireguard-tui` alias; packages may
provide only `wg-tui`.

## Source installs and in-app updates

```sh
git clone https://github.com/JamilleJung/wireguard-tui.git
cd wireguard-tui
./install.sh
```

A source install detects packages, ensures Rust/a linker when needed, builds
as the invoking user and installs both binaries. A direct root source install
is guarded because `--allow-root` can permit building as root. The installer
can use `su` when sudo is unusable while retaining the original normal user.
It does not silently enable systemd-resolved; `setup` offers a confirmed repair.

At startup `wg-tui` checks GitHub for a newer release. `u` acts on a detected
update; it does not force another network check. The updater validates version
syntax, downloads over HTTPS, verifies the signed manifest against the
compiled-in public key, selects an exact checksum entry, and extracts to an
exclusive `0700` directory. It re-runs the verified installer with prebuilt
binaries and `--no-desktop`; a root-running app also supplies `--allow-root`.
Restart the app after installation. Missing verification tools or invalid data
refuse the update; minisign installation is attempted on a best-effort basis.

Disable checking with `WG_NO_UPDATE_CHECK=1` or the `no-update` file in
`$XDG_CONFIG_HOME/wireguard-tui` (default `~/.config/wireguard-tui`). Prefer the
package manager for packaged installs; the updater uses the source installer's
`/usr/local` layout/default sudoers mode, not a distro package transaction.

## CI and release pipeline

The checked-in [CI workflow](../.github/workflows/ci.yml) runs formatting,
clippy, locked tests/build, CLI doctor/help/version smoke checks, shellcheck,
helper negative checks and installer sanity checks. Correct commands include:

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
cargo audit --deny warnings
```

The [release workflow](../.github/workflows/release.yml) is triggered by a
`vMAJOR.MINOR.PATCH` tag, and validates that it matches Cargo.toml. Manual
workflow dispatch must also target a valid version tag. It:

1. Builds four musl targets with pinned cargo-auditable and per-target rust-lld.
2. Audits dependencies, runs tests/smoke/shell checks, and builds the mandatory
   native amd64 `.deb` using pinned cargo-deb.
3. UPX-compresses binaries, stages tarballs with installer assets, and requires
   each expected upload to exist.
4. Collects the artifacts, generates a non-empty checksum manifest, signs it
   with `MINISIGN_KEY`, and verifies with the committed public key.
5. Publishes only after all build/package jobs succeed; signing failure or a
   missing key stops publication. Only the publishing job has write permission.

CI and release audit currently pass without advisory exceptions. That checks
known advisories, not every possible bug. SHA-pinned Actions and version-pinned
build tools improve traceability; no bit-for-bit reproducibility guarantee is
made. The x86_64 output is smoke-tested on its runner; that does not establish
runtime testing on every supported CPU/distro combination.

## Inspecting embedded dependency metadata

`cargo auditable build` embeds dependency data **before UPX compression**.
The packed release ELF cannot be inspected directly by `cargo audit bin`.
Work on a verified extracted binary copy, leaving the installed/original file
untouched:

```sh
cp wg-tui wg-tui.audit
upx -d wg-tui.audit
cargo audit bin wg-tui.audit
rm wg-tui.audit
```

The same process applies to `wg-helper`. Install suitable UPX/cargo-audit tools
for your audit host. An uncompressed output from `cargo auditable build --release --locked` in a
source checkout is another option for inspection. Updating source docs does
not retroactively update the guide embedded into an already published binary;
that requires a future build.

## 1.9.3 behavior and upgrade checks

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

Repeating `--polkit` checks the existing `wireguard` group and account
membership before invoking group-management tools. An already configured
Alpine/BusyBox system can reinstall without optional Shadow `groupadd`/`usermod`.
Missing groups or membership must still be created and verified before the
previous authorization is removed.

Verified updates retain ownership of their private download directory through
installation and clean it up on download, staging or installation failure and
normal completion. Before privilege escalation, the whole required bundle is
checked: regular single-link application/helper/installer/assets, no symlink
components, and a fresh exclusive staging tree. Missing/unsafe assets fail before
installation starts. HTTPS-only redirects are enforced. TUI automatic updates select its
published x86_64/i686/aarch64/armv7 builds; unsupported CPUs fail explicitly.

Download refuses a version that is no longer newer and reports the actual
verified version on success. Install the app/helper together, reapply existing
kill-switch rules deliberately, and review the current backup/QR limits before
using an archive created by another tool. Tests cover concrete failure paths;
passing them does not prove compatibility with every host firewall or distro.

## Recent release highlights

- **1.9.3 (2026-11-02):** fix repeat `--polkit` installation on Alpine/BusyBox
  by reusing existing group/membership state without optional Shadow utilities;
  missing setup must still succeed before previous authorization is removed.

- **1.9.2 (2026-11-02):** deep-audit fixes for shared validation, trusted helpers,
  bounded IPC, complete atomic exports, bounded archive/QR reads, helper files,
  serialized firewall rules, TUI selection/editor/clipboard behavior and verified
  update lifetimes. Exact limits and compatibility notes are in the current
  README/security policy.
- **1.9.1 (2026-11-02):** bounded/preflight backup restore, inline comments,
  helper output/timeout fixes, exact kill-switch names and rule ordering,
  private signed-update staging and exact checksums, installer assets/prebuilt
  handling, and refreshed dependencies with no audit exceptions. Existing
  kill-switch rules need an off/on cycle after upgrading.
- **1.9.0 (2026-07-27 source changes):** inactive resolvectl shim detection and
  confirmed systemd-resolved repair.
- **1.8.0:** diagnostics, in-app guide/changelog, all keys enabled without a
  mode switch, and a default desktop launcher.
- **1.7.x:** backup manager, log/copy controls and signed in-app updates.

The notes below are retained release history, not current installation or key
instructions. Some legacy entries describe the desktop sibling's copy/log UI;
use the [current tutorial](TUTORIAL.md) for terminal behavior.

## 📋 Version history

### 1.6.0 (2026-06-17)

- **nftables kill switch backend** - preferred when `nft` is available (`inet filter` table covers both IPv4+IPv6); iptables/ip6tables fallback.
- **SSH safety warning** - the helper warns (stderr) before enabling a kill switch over an SSH connection.
- **Copy UX** - click the value text or trailing ⧉ icon to copy; no more large "Copy" button.
- **Log tab** - `TextEdit` (read-only) for native text selection and scrolling; journal limits raised to 1000 lines.
- **Speed display** - uses ↓ ↑ icons instead of "down"/"up"; throughput calculation is more robust (no longer gated on active flag).
- **More tests** - kill switch rule generation tests: nftables handle extraction and iptables rule numbering.

## 1.6.3 (2026-06-18) - Clippy clean, QR fix, docs sync

- Zero clippy warnings across all targets (`-D warnings` enforced).
- Terminal QR simplified to half-block only (auto-detect caused false "too large").
- All markdown documentation audited and synchronized.

## 1.6.2 (2026-06-18) - SSH allowlist, 92 tests, copy/log UX

- **SSH auto-allowlist.** Kill switch now auto-allows established SSH traffic when `$SSH_CONNECTION` is set.
- **Copy UX.** Click the value text or ⧉ icon to copy - no more large "Copy" button.
- **Log tab.** 1000 journal lines, `TextEdit` read-only for text selection and scrolling.
- **Speed fix.** Reliable active detection via `wg show interfaces`; ↓ ↑ icons.
- **92 tests** across both repos (helper + main).

## 1.5.5 - Rust helper, kill switch, aarch64 tarballs, and packaging templates

Highlights:

- The privileged runtime helper is now Rust (`src/bin/wg-helper.rs`) with the
  same fixed verb contract.
- Advanced mode now has `K` to toggle a tunnel-scoped kill switch for active
  tunnels using helper-managed iptables/ip6tables rules.
- Easy Mode can now create a tunnel from scratch with `n`, choose an
  Interface-only/full-tunnel/split-tunnel preset, then review it in `$EDITOR`.
- OSC52 copy now normalizes single-field payloads before emitting the terminal
  clipboard escape.
- Release automation now builds portable x86_64 and aarch64 Linux tarballs.
- Alpine APKBUILD and Void template packaging starts are included for
  maintainers while keeping the TUI free of GUI dependencies.

Newest first.

### 1.5.4 - Installer finds `sbin` tools after the root re-exec

Highlights:

- The installer now puts `sbin` directories on `PATH` when it re-runs as root, so
  it reliably finds `visudo` and the `resolvconf` probe. Previously a normal
  user's `PATH` (which `su` carries into the as-root step) usually omitted
  `/usr/sbin` and `/sbin`, which caused two silent failures: the passwordless
  sudoers drop-in was skipped ("sudoers validation failed"), and an already
  installed `openresolv` was misreported as missing.
- Clearer hint when the sudoers drop-in is skipped - re-run with
  `./install.sh --polkit` to use a polkit rule instead.

Upgrade notes: if a previous install on a minimal box left you without
passwordless authorization, just re-run `./install.sh` (or `./install.sh --polkit`)
to get the drop-in or polkit rule written correctly.

### 1.5.3 - Install `sudo` and set up passwordless auth when sudo is unusable

Highlights:

- "No usable sudo? Install sudo and set it up." When you cannot use sudo (for
  example a Debian server where your login user is not in the sudoers file), the
  installer re-runs itself as root with a **single** ROOT-password prompt instead
  of one prompt per step, installs `sudo` if it is missing, and writes a
  passwordless drop-in scoped to the helper for your user. The result: `wg-tui`
  works as a normal user, with no root and no password prompt, even though you are
  not in the `sudo` group.
- Clear, actionable error when the app cannot gain root (no passwordless sudo and
  no `pkexec`), instead of a cryptic `spawn failed: No such file or directory`.
- The installer now fails loudly if it cannot set up any privilege path, rather
  than reporting success and leaving a non-working install. Transient
  `apt-get update` hiccups no longer abort the run.

Security hardening in this release:

- The sudoers rule no longer trusts `$USER`. The target user is taken from the
  real uid (`id -un`), validated against a strict username pattern, and confirmed
  to exist, so a crafted `$USER` cannot inject a wider sudoers spec (something
  `visudo -cf` alone does not catch).
- The build no longer runs as root. `cargo`/`rustup` run as the invoking user via
  `runuser`, so dependency build scripts never execute with root privilege and the
  toolchain and artifacts stay in that user's home.

### 1.5.2 - Install works when `sudo` is present but you are not a sudoer

Highlights:

- The installer and `wg-tui setup` no longer assume that having the `sudo` binary
  means you can use it. On a Debian server where the login user is not in the
  sudoers file (`<user> is not in the sudoers file`), the install used to abort.
  It now probes real sudo usability (`sudo -n` / admin-group membership), falls
  back to `su` (the ROOT password), and auto-switches the helper authorization to
  a polkit rule.

Upgrade notes: tip - run `su -` first, then `./install.sh`, to be prompted for the
root password once rather than per step.

### 1.5.1 - DNS/resolvconf check and a per-distro guide

Highlights:

- `wg-tui doctor` now reports whether a resolvconf provider is available for
  tunnels that use a `DNS =` line, and `wg-tui setup` offers to install one
  (`openresolv`; an active systemd-resolved also counts). This is the fix for the
  classic minimal-Debian failure where such tunnels died with
  `resolvconf: command not found`.
- A new per-distro guide, [docs/DISTROS.md](DISTROS.md): what to install, what has
  to be set up, what survives a reboot, and when - only as a server or gateway -
  you need firewall and IP-forwarding changes.
- Activation failures are explained in plain language (for example a missing
  resolvconf provider) instead of the raw `wg-quick` output.
- The installer works without `sudo`: it falls back to `su` (the ROOT password) on
  Debian-minimal where `sudo` is absent, auto-switches the helper authorization
  from a sudoers drop-in to a polkit rule, and best-effort installs `openresolv`
  when neither it nor systemd-resolved is present.

### 1.5.0 - First-run doctor/setup and a friendly empty state

Highlights:

- New `wg-tui doctor` subcommand: a read-only, plain-language system checklist
  covering WireGuard tools, the privileged helper and its authorization,
  `/etc/wireguard`, systemd and journald. It exits `0` (all OK), `1` (warnings) or
  `2` (something critical is missing) - useful to run first over SSH on a new box.
- New `wg-tui setup` subcommand: a guided, confirmation-based fix that offers to
  install `wireguard-tools` via your package manager and points you at the
  installer for the helper. It never connects tunnels or enables start-on-boot.
- A friendly first-run hint inside the app when something critical is missing, and
  a beginner empty state ("Import a .conf or QR image to begin") that adapts to
  Easy/Advanced mode.

Note: the app still does not bundle WireGuard kernel modules or `wg`/`wg-quick` -
it uses your system's `wireguard-tools` and helps you install them.

### 1.4.1 - Distro packaging

Highlights:

- Distro packaging arrives: an AUR `PKGBUILD` (Arch) and an RPM spec for COPR
  (Fedora/RHEL/Rocky), plus `packaging/PACKAGING.md`.
- Documentation switched to plain ASCII hyphens instead of em dashes.

### 1.4.0 - Live throughput, connection health, and CLI hygiene

Highlights:

- The detail view shows real-time down/up speed for the selected tunnel, plus a
  Connection health line derived from the most recent handshake
  (OK / stale / waiting).
- `wg-tui --version` and `wg-tui --help` now print and exit cleanly instead of
  launching the full-screen UI.
- The privileged helper bounds every `wg`/`wg-quick` call with a timeout, so a
  hang (DNS, a stuck `PostUp`, a wedged interface) can no longer lock up the app.
- CI gained a smoke test confirming `--version`/`--help` start and exit cleanly.

Upgrade notes: the old demo mode (`WGTUI_DEMO`) was removed - the app always talks
to real tunnels now.

### 1.3.5 - Copy the interface public key

Highlights:

- Press `y` to copy the selected tunnel's interface public key to the clipboard,
  using the OSC 52 terminal escape so it works both locally and over SSH in
  terminals that support it. This closed the last copy-to-clipboard parity gap
  with the desktop client.

### 1.3.4 - Version realignment, Easy mode, and a shared icon

> The version was realigned with the desktop client (`wireguard-gui`) so the
> family shares one number - this is the release after 1.1.2.

Highlights:

- Easy mode (now the default for new users) shows only the everyday actions -
  Connect/Disconnect, Import, Start-on-boot, Remove and Show-QR - so it is
  approachable for non-technical users. Press `m` for Advanced mode (edit, new,
  generate keys, running config, save-live, rename, export). Your choice is
  remembered in `~/.config/wireguard-tui/mode`.
- Bulk import: in the import browser, Space marks files and Enter imports all
  marked ones at once (matching the desktop client).
- A shared desktop icon (the same one the GUI uses) is installed and used by the
  launcher.

Also fixed: terminal rendering corruption on some fonts/locales. The UI no longer
uses ambiguous or wide Unicode glyphs (arrows, the filled/empty status dots,
ellipsis, dingbats) that some terminals render double-width or as tofu and
garbled the layout - it is now ASCII-only, aside from box-drawing borders. The
active/inactive markers in the list are a plain green `*` and a dim `-`.

### 1.1.2 - Release pipeline hardened to match the desktop client

Highlights:

- The `.deb` is now mandatory: a missing one fails the release.
- `SHA256SUMS` is generated with `nullglob` and verified non-empty, and it is
  signed with minisign - `SHA256SUMS.minisig` and `minisign.pub` now ship with
  every release.
- The privileged helper is also discovered next to the binary, so an extracted
  release tarball works without first running `install.sh`.
- CI now hard-fails on `shellcheck` warnings for `wg-helper` and `install.sh`.

### 1.1.1 - IPv6 endpoint fix and helper safety

Highlights:

- A bracketed IPv6 `Endpoint` (for example `[2001:db8::1]:51820`) is accepted by
  config validation again. The stricter endpoint check added in 1.1.0 wrongly
  rejected it, which blocked saving and importing IPv6-endpoint tunnels. A
  regression test now covers it.
- The helper's `sync` verifies that `wg-quick strip` succeeded before applying it
  to the live interface. A strip failure previously fed an empty config to
  `wg syncconf` and could wipe every peer off a running tunnel.
- Stray "wireguard-gui" wording and the helper's log header were corrected to
  "wireguard-tui".
- Helper portability: `wg-helper` no longer needs GNU `find -printf` (it uses a
  pure-bash glob, so BusyBox `find` on Alpine/Void works) and filters listed
  tunnels to valid names. Start-on-boot detects `systemctl` first and fails
  clearly on non-systemd systems; the log view explains when `journalctl` is
  unavailable.
- Helper-path override hardening: `$WG_HELPER` is honoured freely in debug builds,
  but in release builds it is ignored unless `WG_ALLOW_UNSAFE_HELPER=1` is set and
  the target is an absolute, root-owned, non-world-writable file.
- Added unit tests for config parsing, validation and name sanitisation; a CI step
  that shell-syntax-checks `wg-helper` and `install.sh`; an expanded `SECURITY.md`;
  and README cross-links to the desktop sibling.

### 1.1.0 - File-browser import and snappier feedback

Highlights:

- Import is now a file browser: press `i` to navigate directories and pick a
  `.conf` file or a QR image, instead of typing a path.
- Status messages auto-dismiss after a few seconds, so the footer returns to the
  key hints instead of leaving an alert stuck on screen.
- Slow actions give instant feedback: activate/deactivate, delete, start-on-boot
  and save-live repaint a progress line before the blocking privileged call, so a
  key press always shows it is working.
- The footer key-hint bar adapts to terminal width, falling back to a compact set
  (always including `? help` / `q quit`) on narrow terminals.
- A rendered terminal screenshot was added to the README.

### 1.0.0 - First stable release

Highlights:

- The first stable release: a native Linux terminal UI (`wg-tui`) for managing
  WireGuard tunnels, with the same feature set as the desktop client.
- Tunnel list with a live active/inactive indicator and a merged detail view
  (interface plus peers), refreshed every second with latest handshake and
  transfer.
- Activate / Deactivate (`wg-quick up`/`down`).
- Edit a tunnel in `$EDITOR` with config validation; apply changes live to a
  running tunnel without dropping peers (`wg syncconf`).
- New tunnel from a generated template; Generate keypairs and preshared keys
  (`wg genkey`/`genpsk`); Show running config (`wg showconf`) and Save live state
  (`wg-quick save`).
- Import a `.conf` file or a QR-code image; Show QR in the terminal; Export all
  tunnels to a `.zip`.
- Rename / Remove tunnels; a Start-on-boot toggle; a Log tab.
- A hardened privileged helper (`wg-helper`): fixed paths, strict tunnel-name
  validation, atomic writes, timestamped backups, and journald audit logging.
- Privilege backends: sudoers (default) or polkit (`--polkit`), with `pkexec` as a
  fallback. Reuses a co-installed desktop-client helper automatically.
- A universal installer (`install.sh`) for apt, dnf/yum, pacman, zypper, apk, xbps
  and eopkg, with a minimal-install dependency check.

## Versioning policy

Versions follow `MAJOR.MINOR.PATCH`; tagged builds use `vMAJOR.MINOR.PATCH` and
must match Cargo.toml. Coordinate version/package metadata, release notes,
lockfile changes, quality checks and signed artifacts when releasing. Sharing a
version number with the desktop sibling does not imply identical frontend
features. Historical notes above refer to their own release; current behavior
is documented in the README/tutorial and current changelog entries.
