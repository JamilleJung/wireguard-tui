# Changelog

All notable changes to this project are documented here.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/JamilleJung/wireguard-tui/compare/v1.4.1...HEAD
[1.4.1]: https://github.com/JamilleJung/wireguard-tui/compare/v1.4.0...v1.4.1
[1.4.0]: https://github.com/JamilleJung/wireguard-tui/compare/v1.3.5...v1.4.0
[1.3.5]: https://github.com/JamilleJung/wireguard-tui/compare/v1.3.4...v1.3.5
[1.3.4]: https://github.com/JamilleJung/wireguard-tui/compare/v1.1.2...v1.3.4
[1.1.2]: https://github.com/JamilleJung/wireguard-tui/compare/v1.1.1...v1.1.2
[1.1.1]: https://github.com/JamilleJung/wireguard-tui/compare/v1.1.0...v1.1.1
[1.1.0]: https://github.com/JamilleJung/wireguard-tui/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/JamilleJung/wireguard-tui/releases/tag/v1.0.0
