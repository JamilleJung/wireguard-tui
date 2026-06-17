<div align="center">

# wireguard-tui · `wg-tui`

A native Linux terminal UI for managing WireGuard tunnels.

Built with Rust + ratatui as a single native binary. No GUI stack, no Electron,
no WebView, no desktop dependencies. It works over SSH on minimal servers and
uses plain `/etc/wireguard/*.conf` tunnels through `wg` and `wg-quick`.

<img src="docs/screenshot.svg" alt="wg-tui - the WireGuard terminal UI" width="900">

[![CI](https://github.com/JamilleJung/wireguard-tui/actions/workflows/ci.yml/badge.svg)](https://github.com/JamilleJung/wireguard-tui/actions/workflows/ci.yml)
[![Releases](https://img.shields.io/badge/Releases-latest-2ea44f)](https://github.com/JamilleJung/wireguard-tui/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
![Rust](https://img.shields.io/badge/built%20with-Rust-orange.svg)
![Platform: Linux](https://img.shields.io/badge/platform-Linux-success.svg)
![No GUI deps](https://img.shields.io/badge/deps-pure%20Rust-blueviolet.svg)

</div>

> Prefer a window? The sibling **[wireguard-gui](https://github.com/JamilleJung/wireguard-gui)**
> is the same project philosophy in a desktop UI.

## Design philosophy

This project is intentionally small.

It does not try to become a WireGuard platform, daemon, or configuration
database. It stays close to the Linux WireGuard workflow: plain `.conf` files in
`/etc/wireguard`, `wg`, `wg-quick`, `wg show`, `wg showconf`, `wg syncconf`,
`wg-quick save`, and systemd `wg-quick@<name>` units where available.

The GUI and TUI are separate first-class tools. Install the one you want. Hack
the one you want. They share code where useful, but there is no mandatory
runtime core or hidden platform layer.

The goal is a native client that is easy to use, easy to inspect, and easy to
fork.

## Why a TUI?

Terminal users do not need a browser dashboard just to bring a tunnel up. They
also should not have to memorize scattered commands for normal operations.

`wg-tui` gathers the workflow in one place: live status, editing, imports,
exports, QR codes, diagnostics, and log viewing. It stays terminal-native so it
works the same locally and over SSH on a minimal server.

## What it does

- Lists tunnels from `/etc/wireguard`.
- Shows active/inactive status and live handshake / transfer details.
- Activates and deactivates tunnels with `wg-quick up` / `wg-quick down`.
- Edits configs in `$VISUAL` / `$EDITOR`, with validation before save.
- Imports `.conf` files and QR images from a file browser.
- Creates new tunnels from a generated template and keypair.
- Generates keypairs and preshared keys on demand.
- Displays a tunnel as QR in the terminal.
- Shows running config with `wg showconf` and saves live state with
  `wg-quick save`.
- Exports all tunnels to a zip.
- Renames and removes tunnels.
- Toggles start-on-boot.
- Shows a log tab and a read-only doctor / setup flow.
- Supports Easy mode for everyday work and Advanced mode for expert actions.
- Copies the interface public key with OSC 52 when the terminal supports it.

## What it deliberately does not do

- No GUI stack.
- No Electron or WebView.
- No mandatory daemon or background service.
- No hidden config database.
- No bundled WireGuard kernel module or `wg` binaries.
- No desktop integration requirements.

## Screenshot

<img src="docs/screenshot.svg" alt="wg-tui screenshot" width="900">

## Install

### Prebuilt packages

The release page normally includes:

- `wireguard-tui_*_amd64.deb`
- `wireguard-tui-*-x86_64-linux.tar.gz`
- `SHA256SUMS`, `SHA256SUMS.minisig` when signing is configured, and `minisign.pub`

### From source

```sh
git clone https://github.com/JamilleJung/wireguard-tui.git
cd wireguard-tui
./install.sh
```

`install.sh` detects the package manager, installs `wireguard-tools`, ensures a
Rust toolchain if needed, builds the release binary, installs the helper, and
sets up passwordless helper access for the active local user.

The TUI installer stays minimal. It does not pull GUI libraries or desktop
integration packages.

Supported package managers:

| Distro family | Package manager |
|---|---|
| Debian / Ubuntu / Mint | `apt` |
| Fedora / RHEL / Rocky | `dnf` / `yum` |
| Arch / Manjaro / EndeavourOS | `pacman` |
| openSUSE | `zypper` |
| Alpine | `apk` |
| Void | `xbps-install` |
| Solus | `eopkg` |

Uninstall:

```sh
./install.sh uninstall
```

Auth backend choice:

```sh
./install.sh           # sudoers drop-in (default)
./install.sh --polkit  # polkit rule instead
```

## Verify releases

Use the checksum file and minisign signature from the release page:

```sh
sha256sum -c SHA256SUMS --ignore-missing
minisign -Vm SHA256SUMS -P RWTyrstfFCLYkpMwbcyBRl+aGGcJikl35GY1esJDO6HTEJFIMvUC8f1Q
```

## Usage / key map

| Key | Action |
|---|---|
| `↑`/`k`, `↓`/`j` | Move selection, or scroll the Log tab |
| `Enter` / `a` | Activate or deactivate the selected tunnel |
| `e` | Edit the selected tunnel in `$EDITOR` |
| `n` | Create a new tunnel from a generated template |
| `i` | Import a `.conf` file or QR image |
| `g` | Generate a keypair and preshared key |
| `c` | Show the running config with `wg showconf` |
| `d` | Delete the selected tunnel |
| `R` | Rename the selected tunnel |
| `s` | Toggle start-on-boot |
| `p` | Save live state to disk with `wg-quick save` |
| `Q` | Show the tunnel as a QR code |
| `y` | Copy the interface public key to the clipboard |
| `x` | Export all tunnels to `~/wireguard-tunnels.zip` |
| `Tab` | Switch between the Tunnels and Log tabs |
| `m` | Toggle Easy / Advanced mode |
| `r` | Refresh now |
| `?` | Show help |
| `q` / `Esc` | Quit |

In the import browser:

- `Space` marks files for bulk import.
- `Enter` imports the highlighted file or all marked files.
- `Right` / `l` enters a directory.
- `Left` / `h` / `Backspace` goes up one level.

Easy mode shows only everyday actions: connect/disconnect, import, start-on-boot,
remove, and show QR. Advanced mode adds edit, new tunnel, generate keys, running
config, save live state, rename, and export. The mode choice is remembered.

The editor uses `$VISUAL` / `$EDITOR`, falling back to `nano`. The temporary file
is created mode `0600` and removed afterwards.

## Doctor / setup

```sh
wg-tui doctor
wg-tui setup
```

`doctor` is read-only and does not require root. It reports:

- `wg` and `wg-quick`
- the privileged helper
- helper authorization
- `/etc/wireguard`
- systemd for start-on-boot
- journald for the log tab
- resolvconf or systemd-resolved for tunnel configs with `DNS =`

Exit codes:

- `0` = OK
- `1` = warnings
- `2` = critical missing requirements

`setup` is confirmation-based. It offers to install `wireguard-tools`,
`resolvconf` support for `DNS =`, and `/etc/wireguard`, but it does not connect
tunnels or enable start-on-boot. It points you at the installer for the helper.

## Security model

The TUI runs as a normal user. Privileged operations go through one small shell
helper, `packaging/wg-helper`, which is installed as
`/usr/local/lib/wireguard-tui/wg-helper` or `/usr/lib/wireguard-tui/wg-helper`
when packaged.

The helper exposes fixed verbs only:

`list`, `read`, `dump`, `up`, `down`, `save`, `rename`, `delete`, `enable`,
`disable`, `is-enabled`, `sync`, `showconf`, `persist`, and `log`.

Hardening in the helper:

- Fixed `PATH` and fixed `/etc/wireguard` path.
- Tunnel names must match `^[A-Za-z0-9][A-Za-z0-9_.-]{0,14}$`.
- No path traversal, no caller-controlled root destination, no shell eval of
  user input.
- Atomic config writes with backups before overwrite, delete, or rename.
- Audit log entries via `logger` / journald.
- Start-on-boot changes are kept separate from file writes.

Authorization is passwordless sudoers by default, or a polkit rule with
`--polkit`. If neither is set up, the app falls back to `pkexec`.

QR export and zip export contain the private key. Treat them like the config
file itself.

## Hacking on it

This repo is MIT licensed. Fork it and change it.

### Codebase map

| Path | Purpose |
|---|---|
| `src/main.rs` | App entrypoint, event loop, rendering |
| `src/backend.rs` | WireGuard orchestration, validation, helper client |
| `src/doctor.rs` | Read-only setup checks and fix hints |
| `packaging/wg-helper` | Privileged helper |
| `install.sh` | Distro-aware installer |
| `docs/` | Tutorial, distro notes, release notes |
| `packaging/` | Desktop files, icon, packaging metadata |
| `.github/workflows/` | CI and release automation |

### Build and test

```sh
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

### Run from source

```sh
cargo run --release
```

In development, the binary looks for the in-tree helper first. You can point it
at a local helper with `WG_HELPER=/path/to/wg-helper`. In release builds that
override is only honored for a root-owned, non-world-writable file when
`WG_ALLOW_UNSAFE_HELPER=1` is set.

### Add a feature safely

If you touch privileged behavior, keep the helper verb-based, validate names
before filesystem access, and keep writes atomic. If you touch the UI, keep the
terminal workflow clear and avoid adding hidden state.

## Troubleshooting

- `wg-tui doctor` shows `MISSING` for `wg` / `wg-quick`: install
  `wireguard-tools`.
- `wg-tui doctor` warns about `DNS for tunnels (resolvconf)`: install a
  resolvconf provider such as `openresolv`, or use systemd-resolved.
- `wg-tui` keeps prompting for a password: run `./install.sh` or
  `./install.sh --polkit` so the helper path is authorized.
- Start-on-boot is unavailable: your system does not have `systemctl`.
- The log tab is empty: your system does not have `journalctl`.
- The QR code is too large to fit: widen the terminal before opening QR view.
- `y` does nothing in your terminal: OSC 52 clipboard support is missing.

## Known limitations

- Start-on-boot is systemd-only.
- Prebuilt binaries are x86_64 only.
- The editor is external by design, via `$EDITOR`.
- QR and zip exports include private keys.
- The helper remains a shell script for now; the privileged surface is small,
  but not yet Rust.

## Roadmap

- Rust helper for privileged operations.
- Better multi-peer editing.
- More packaged architectures where the release workflow supports them.
- More doctor checks and tests for rename and import edge cases.

## License

MIT. WireGuard is a registered trademark of Jason A. Donenfeld. This project is
an independent, unofficial client and is not affiliated with or endorsed by the
WireGuard project.
