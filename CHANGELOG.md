# Changelog

All notable changes to this project are documented here.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.1.2] - 2026-06-16

### Changed
- **Release pipeline hardened to match the desktop client.** The `.deb` is now
  mandatory (a missing one fails the release), `SHA256SUMS` is generated with
  `nullglob` and verified non-empty, and it is **signed with minisign** —
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
  to the live interface — a strip failure previously fed an empty config to
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
- **Import is now a file browser** — press `i` to navigate directories and pick a
  `.conf` file or a QR image, instead of typing a path.
- A rendered terminal screenshot in the README.

### Changed
- **Status messages auto-dismiss** after a few seconds, so the footer returns to
  the key hints instead of leaving an alert stuck on screen.
- **Instant feedback for slow actions** — activate/deactivate, delete,
  start-on-boot and save-live now repaint a progress line *before* the (blocking)
  privileged call, so a key press always shows it is working.
- The footer key-hint bar **adapts to the terminal width**, falling back to a
  compact set (always including `? help` / `q quit`) on narrow terminals.

## [1.0.0] - 2026-06-16

### Added
- First release: a native Linux **terminal UI** (`wg-tui`) for managing
  WireGuard tunnels, with the same feature set as the desktop client.
- Tunnel list with a live active/inactive indicator and a merged detail view
  (interface + peers), refreshed every second — latest handshake and transfer.
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

[Unreleased]: https://github.com/JamilleJung/wireguard-tui/compare/v1.1.2...HEAD
[1.1.2]: https://github.com/JamilleJung/wireguard-tui/compare/v1.1.1...v1.1.2
[1.1.1]: https://github.com/JamilleJung/wireguard-tui/compare/v1.1.0...v1.1.1
[1.1.0]: https://github.com/JamilleJung/wireguard-tui/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/JamilleJung/wireguard-tui/releases/tag/v1.0.0
