# Changelog

All notable changes to this project are documented here.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.1.1] - 2026-06-16

### Fixed
- `wg-helper`'s `sync` now verifies `wg-quick strip` succeeded before applying it
  to the live interface — a strip failure previously fed an empty config to
  `wg syncconf` and could wipe every peer off a running tunnel.
- Corrected stray "wireguard-gui" wording and the log header in the helper to
  "wireguard-tui".

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

[Unreleased]: https://github.com/JamilleJung/wireguard-tui/compare/v1.1.1...HEAD
[1.1.1]: https://github.com/JamilleJung/wireguard-tui/compare/v1.1.0...v1.1.1
[1.1.0]: https://github.com/JamilleJung/wireguard-tui/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/JamilleJung/wireguard-tui/releases/tag/v1.0.0
