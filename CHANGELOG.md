# Changelog

All notable changes to this project are documented here.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.5.5] - 2026-06-17

### Added
- Rust `wg-helper` binary with the same fixed verb contract as the previous
  helper.
- Advanced-mode `K` kill switch toggle backed by helper-managed
  iptables/ip6tables rules for active tunnels.
- aarch64 Linux release tarballs and Alpine/Void packaging templates.
- Easy Mode can create a tunnel from scratch with `n`, preset selection, and
  `$EDITOR` review.

### Changed
- Reframed project docs and package metadata around the small native Linux
  WireGuard client model: plain `/etc/wireguard` configs, `wg`/`wg-quick`, no
  NetworkManager layer, and no mandatory runtime core.
- The first-party TUI installer and packages no longer install desktop
  integration by default. `./install.sh --desktop` remains available for users
  who want a launcher.
- OSC52 copy now normalizes single-field payloads before writing the escape
  sequence.

### Security
- The helper now performs a second, privileged-boundary config shape check before
  save/rename, in addition to frontend validation.
- Helper writes now make a best-effort `sync -f` before atomic rename.
- Runtime helper command execution moved out of shell into Rust argv-based
  process calls with fixed tool lookup and timeouts.
- CI now runs negative helper-name validation tests.
- CI now runs installer sanity checks for helper paths, sudoers validation, and
  non-root build handoff.

### Fixed
- Copying public keys no longer carries accidental leading/trailing whitespace
  or display newlines.

## [1.5.4] - 2026-06-17

### Fixed
- **Installer now finds `sbin` tools after the root re-exec.** A normal user's
  `PATH` (which `su` carries into the as-root re-exec) usually omits `/usr/sbin`
  and `/sbin`, so `visudo` and the `resolvconf` probe silently failed - the
  passwordless drop-in was skipped ("sudoers validation failed") and a present
  `openresolv` was misreported as missing. The installer now puts sbin on `PATH`,
  so the sudoers drop-in is written and resolvconf is detected correctly.
- Clearer hint when the drop-in is skipped: re-run `./install.sh --polkit`
  (a `--polkit` is an installer flag, not a `wg-tui` one).

## [1.5.3] - 2026-06-17

### Added
- **"No usable sudo -> install sudo and set it up."** When you can't use sudo
  (e.g. a Debian server where you aren't in the sudoers file), the installer
  re-runs itself as root with a **single** ROOT-password prompt (instead of one
  per step), installs `sudo` if it's missing, and writes a passwordless drop-in
  scoped to the helper for *your* user - which makes `wg-tui` work as a normal
  user (no root, no prompt) even though you aren't in the `sudo` group.

### Security
- **Don't trust `$USER` when writing the sudoers rule.** The target user is now
  taken from the real uid (`id -un`), validated against a strict username pattern
  and confirmed to exist, so a crafted `$USER` can't inject a wider sudoers spec
  (e.g. `NOPASSWD: ALL`) - `visudo -cf` alone does not catch that.
- **The build no longer runs as root.** `cargo`/`rustup` run as the invoking user
  via `runuser`, so dependency build scripts never execute with root privilege and
  the toolchain/artifacts stay in that user's home.

### Fixed
- Clear, actionable error when the app can't gain root (no passwordless sudo and
  no `pkexec`) instead of a cryptic `spawn failed: No such file or directory`.
- Installer fails loudly if it can't set up any privilege path, rather than
  reporting success and leaving a non-working install; `apt-get update` hiccups no
  longer abort the run.

## [1.5.2] - 2026-06-17

### Fixed
- **Install works when `sudo` is present but you're not a sudoer.** The installer
  (and `wg-tui setup`) previously assumed that having the `sudo` binary meant you
  could use it - so on a Debian server where the login user isn't in the sudoers
  file (`<user> is not in the sudoers file`) it aborted. It now probes real sudo
  usability (`sudo -n` / admin-group membership) and falls back to `su` (the ROOT
  password), and auto-switches the helper authorization to a polkit rule. Tip:
  run `su -` first, then `./install.sh`, to be prompted once instead of per step.

## [1.5.1] - 2026-06-17

### Added
- **DNS / resolvconf check.** `wg-tui doctor` now reports whether a resolvconf
  provider is available for tunnels that use a `DNS =` line, and `wg-tui setup`
  offers to install one (`openresolv`; systemd-resolved also counts). This is the
  fix for minimal Debian, where such tunnels failed with
  `resolvconf: command not found`.
- **Per-distro guide** ([docs/DISTROS.md](docs/DISTROS.md)): what to install, what
  to set up, what survives a reboot, and when - only as a server/gateway - you need
  firewall and IP-forwarding changes.

### Changed
- **Works without `sudo`.** The installer and `wg-tui setup` fall back to `su`
  (the ROOT password) on Debian-minimal where `sudo` isn't present, and the
  installer auto-switches the helper authorization from a sudoers drop-in to a
  polkit rule when there's no `sudo`.
- Activation failures are now explained in plain language (e.g. the missing
  resolvconf provider) instead of the raw `wg-quick` output.

### Fixed
- Tunnels with a `DNS =` line no longer fail on systems without a resolvconf
  provider: the installer best-effort installs `openresolv` when neither it nor
  systemd-resolved is present.

## [1.5.0] - 2026-06-17

### Added
- **`wg-tui doctor`** - prints a plain-language system checklist (WireGuard
  tools, the privileged helper + authorization, `/etc/wireguard`, systemd,
  journald) and exits **0** (all OK) / **1** (warnings) / **2** (critical
  missing). Read-only; useful over SSH.
- **`wg-tui setup`** - a guided, confirmation-based fix: offers to install
  `wireguard-tools` via your package manager and points you at the installer for
  the helper. Never connects tunnels or enables start-on-boot.
- A friendly **first-run hint** inside the app when something critical is
  missing, and a **beginner empty state** ("Import a .conf or QR image to begin")
  that adapts to Easy/Advanced mode.

### Notes
- The app **does not bundle WireGuard kernel modules or `wg`/`wg-quick`** - it
  uses your system's `wireguard-tools`, and helps you install them.

## [1.4.1] - 2026-06-17

### Added
- **Distro packaging**: an **AUR `PKGBUILD`** (Arch) and an **RPM spec for COPR**
  (Fedora/RHEL/Rocky), plus `packaging/PACKAGING.md`.

### Changed
- Documentation now uses plain ASCII hyphens instead of em dashes.

## [1.4.0] - 2026-06-17

### Added
- **Live throughput** - the detail view shows real-time **down/up speed** for the
  selected tunnel, plus a **Connection** health line derived from the most recent
  handshake (OK / stale / waiting).
- **`wg-tui --version` / `--help`** - flags now print and exit instead of
  launching the full-screen UI.

### Changed
- **Removed demo mode** (`WGTUI_DEMO`) - the app always talks to real tunnels.
- The privileged helper now **bounds every `wg`/`wg-quick` call with a timeout**,
  so a hang (DNS, a stuck `PostUp`, a wedged interface) can't lock up the app.
- CI now runs a **smoke test** (`--version`/`--help` start and exit cleanly).

## [1.3.5] - 2026-06-17

### Added
- **Copy the interface public key** to the clipboard with **`y`** (via the OSC 52
  terminal escape, so it works locally and over SSH in supporting terminals) -
  closes the last copy-to-clipboard parity gap with the desktop client.

## [1.3.4] - 2026-06-17

> Version realigned with the desktop client (`wireguard-gui`) so the family
> shares one number - this is the release after 1.1.2.

### Added
- **Easy mode** (default) for everyday users: shows only Connect/Disconnect,
  Import, Start-on-boot, Remove and Show-QR; press **`m`** for Advanced mode
  (edit, new, generate keys, running config, save-live, rename, export). The
  choice is remembered (`~/.config/wireguard-tui/mode`).
- **Bulk import**: in the import browser, **Space** marks files and **Enter**
  imports all marked ones at once (matches the desktop client).
- A **desktop icon** (shared with the GUI) is installed and used by the launcher.

### Fixed
- **Terminal rendering corruption on some fonts/locales.** The UI no longer uses
  ambiguous/wide Unicode glyphs (arrows, `●`/`○` dots, `…`, dingbats) that some
  terminals render double-width or as tofu, garbling the layout - it is now
  ASCII-only (box-drawing borders aside).

## [1.1.2] - 2026-06-16

### Changed
- **Release pipeline hardened to match the desktop client.** The `.deb` is now
  mandatory (a missing one fails the release), `SHA256SUMS` is generated with
  `nullglob` and verified non-empty, and it is **signed with minisign** -
  `SHA256SUMS.minisig` and `minisign.pub` now ship with every release.
- The privileged helper is also discovered **next to the binary**, so an
  extracted release tarball works without first running `install.sh`.
- CI now **hard-fails on `shellcheck` warnings** for `wg-helper` and `install.sh`.

## [1.1.1] - 2026-06-16

### Fixed
- **A bracketed-IPv6 `Endpoint`** (e.g. `[2001:db8::1]:51820`) is now accepted by
  config validation again. The stricter endpoint check added in 1.1.0 wrongly
  rejected it, which blocked saving/importing IPv6-endpoint tunnels. Covered by a
  regression test.
- `wg-helper`'s `sync` now verifies `wg-quick strip` succeeded before applying it
  to the live interface - a strip failure previously fed an empty config to
  `wg syncconf` and could wipe every peer off a running tunnel.
- Corrected stray "wireguard-gui" wording and the log header in the helper to
  "wireguard-tui".

### Changed
- **Helper portability.** `wg-helper` no longer needs GNU `find -printf` (uses a
  pure-bash glob, so it works with BusyBox `find` on Alpine/Void) and filters
  listed tunnels to valid names. Start-on-boot detects `systemctl` first and
  fails clearly on non-systemd systems (`is-enabled` reports "unknown"); the log
  view explains when `journalctl` is unavailable.
- **Helper-path override hardening.** `$WG_HELPER` is honoured freely in debug
  builds, but in release builds it is ignored unless `WG_ALLOW_UNSAFE_HELPER=1`
  is set *and* the target is an absolute, root-owned, non-world-writable file.

### Added
- Unit tests for config parsing, validation and name sanitisation; a CI step
  that shell-syntax-checks `wg-helper` and `install.sh`.
- An expanded `SECURITY.md` (threat model, privilege boundary, `PostUp` root
  hooks, private-key/QR handling, supply-chain verification).
- README cross-links the desktop sibling (`wireguard-gui`) and explains the
  `wg-quick` (not NetworkManager) model, the init-system limitation, and the
  QR/private-key warning.

## [1.1.0] - 2026-06-16

### Added
- **Import is now a file browser** - press `i` to navigate directories and pick a
  `.conf` file or a QR image, instead of typing a path.
- A rendered terminal screenshot in the README.

### Changed
- **Status messages auto-dismiss** after a few seconds, so the footer returns to
  the key hints instead of leaving an alert stuck on screen.
- **Instant feedback for slow actions** - activate/deactivate, delete,
  start-on-boot and save-live now repaint a progress line *before* the (blocking)
  privileged call, so a key press always shows it is working.
- The footer key-hint bar **adapts to the terminal width**, falling back to a
  compact set (always including `? help` / `q quit`) on narrow terminals.

## [1.0.0] - 2026-06-16

### Added
- First release: a native Linux **terminal UI** (`wg-tui`) for managing
  WireGuard tunnels, with the same feature set as the desktop client.
- Tunnel list with a live active/inactive indicator and a merged detail view
  (interface + peers), refreshed every second - latest handshake and transfer.
- Activate / Deactivate (`wg-quick up`/`down`).
- Edit a tunnel in `$EDITOR` with config validation; apply live to a running
  tunnel without dropping peers (`wg syncconf`).
- New tunnel from a generated template; **Generate** keypairs + preshared keys
  (`wg genkey`/`genpsk`); **Show running config** (`wg showconf`) and
  **Save live state** (`wg-quick save`).
- Import a `.conf` file or a **QR-code image**; **Show QR** in the terminal;
  **Export** all tunnels to a `.zip`.
- Rename / Remove tunnels; **Start-on-boot** toggle; a **Log** tab.
- **Hardened privileged helper** (`wg-helper`): fixed paths, strict tunnel-name
  validation, atomic writes, timestamped backups, journald audit logging.
- Privilege backends: **sudoers** (default) or **polkit** (`--polkit`); `pkexec`
  fallback. Reuses a co-installed desktop-client helper automatically.
- **Universal installer** (`install.sh`) for apt, dnf/yum, pacman, zypper, apk,
  xbps and eopkg, with a minimal-install dependency check.

[Unreleased]: https://github.com/JamilleJung/wireguard-tui/compare/v1.5.0...HEAD
[1.5.0]: https://github.com/JamilleJung/wireguard-tui/compare/v1.4.1...v1.5.0
[1.4.1]: https://github.com/JamilleJung/wireguard-tui/compare/v1.4.0...v1.4.1
[1.4.0]: https://github.com/JamilleJung/wireguard-tui/compare/v1.3.5...v1.4.0
[1.3.5]: https://github.com/JamilleJung/wireguard-tui/compare/v1.3.4...v1.3.5
[1.3.4]: https://github.com/JamilleJung/wireguard-tui/compare/v1.1.2...v1.3.4
[1.1.2]: https://github.com/JamilleJung/wireguard-tui/compare/v1.1.1...v1.1.2
[1.1.1]: https://github.com/JamilleJung/wireguard-tui/compare/v1.1.0...v1.1.1
[1.1.0]: https://github.com/JamilleJung/wireguard-tui/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/JamilleJung/wireguard-tui/releases/tag/v1.0.0
