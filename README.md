<div align="center">

# 🐉 wireguard-tui · `wg-tui`

**A native Linux terminal UI for managing WireGuard tunnels - everything the desktop client does, without leaving your terminal.**

Tunnel list with live status, an Interface/Peer detail pane, one-key
Activate/Deactivate, an editor with config validation, key generation, in-terminal
QR codes, export and start-on-boot.

Written in **Rust** with [ratatui](https://ratatui.rs) - a single native binary
with **no GUI or C library dependencies**, so it runs the same on your desktop and
over SSH on a minimal server.

[![CI](https://github.com/JamilleJung/wireguard-tui/actions/workflows/ci.yml/badge.svg)](https://github.com/JamilleJung/wireguard-tui/actions/workflows/ci.yml)
[![Releases](https://img.shields.io/badge/Releases-latest-2ea44f)](https://github.com/JamilleJung/wireguard-tui/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
![Rust](https://img.shields.io/badge/built%20with-Rust-orange.svg)
![Platform: Linux](https://img.shields.io/badge/platform-Linux-success.svg)
![No GUI deps](https://img.shields.io/badge/deps-pure%20Rust-blueviolet.svg)

<img src="docs/screenshot.svg" alt="wg-tui - the WireGuard terminal UI" width="900">

</div>

> **Prefer a window?** **[wireguard-gui](https://github.com/JamilleJung/wireguard-gui)**
> is the same tool as a desktop GUI (modelled on the WireGuard for Windows client).
> Both share the same hardened privilege model and `wg`/`wg-quick` coverage.

---

## ℹ️ Good to know

- **It drives `wg-quick`, not NetworkManager.** Tunnels are plain `.conf` files in
  `/etc/wireguard`, brought up with `wg-quick up`/`down` - the standard WireGuard
  path, deliberately bypassing NetworkManager (which has historically mangled
  `[Peer]` sections).
- **Start-on-boot needs systemd** - it toggles the `wg-quick@<name>` unit. On
  non-systemd systems (OpenRC, runit, …) that one feature is unavailable;
  everything else works.
- **A QR code - and an exported `.zip` - contains the tunnel's _private key_.**
  Only show a QR when it's safe for someone to photograph your screen, and keep
  exports somewhere safe.

---

## ✨ Features

- 📜 **Tunnel list** of everything under `/etc/wireguard`, with a live ●/○ active dot.
- 🔌 **Activate / Deactivate** with one key (`wg-quick up` / `down`).
- 🧾 **Live detail view** - status, public key, listen port, addresses, DNS,
  and per-peer **latest handshake** + **transfer**, refreshed every second.
- 📝 **Edit** any tunnel in your `$EDITOR`, with **config validation** before saving.
- ♻️ **Apply edits to a running tunnel live** (`wg syncconf`) without dropping peers.
- ➕ **New tunnel** from a generated template (fresh private key), and
  **🔑 Generate** keypairs + preshared keys on demand (`wg genkey`/`genpsk`).
- 📥 **Import** a `.conf` file **or a QR-code image** (`wg-tui` decodes the PNG/JPG).
- 📱 **Show QR** - render a tunnel as a QR code right in the terminal to scan
  into the WireGuard mobile app.
- 🧰 **Running config** (`wg showconf`) and **Save live state** (`wg-quick save`).
- 📦 **Export** all tunnels to a `.zip`.
- ✏️ **Rename** / **Remove**; **Start-on-boot** toggle; a **Log** tab.
- 🔒 Same **tiny, auditable privilege surface** as the desktop client (see below).

Press **`?`** in the app for the full key map.

---

## 🚀 Install (one command)

```sh
git clone https://github.com/JamilleJung/wireguard-tui.git
cd wireguard-tui
./install.sh
```

The installer detects your package manager (apt, dnf/yum, pacman, zypper, apk,
xbps, eopkg), installs `wireguard-tools` + a C linker, ensures a Rust toolchain
(via rustup if needed), builds, and installs the `wg-tui` binary, the privileged
helper and a sudoers drop-in. Use `./install.sh --polkit` for a polkit rule
instead, or `./install.sh uninstall` to remove everything.

Then run:

```sh
wg-tui
```

> **Minimal installs:** the installer checks for a C compiler and
> `wireguard-tools` and tells you clearly if either is missing before building.

---

## 🖥️ Usage / key map

| Key | Action |
|-----|--------|
| `↑`/`k`, `↓`/`j` | Move selection (scroll the Log tab) |
| `Enter` / `a` | Activate or deactivate the selected tunnel |
| `e` | Edit the selected tunnel in `$EDITOR` |
| `n` | Create a new tunnel (generated key + `$EDITOR`) |
| `i` | Import from a `.conf` file or QR image |
| `g` | Generate a keypair + preshared key |
| `c` | Show a running tunnel's live config (`wg showconf`) |
| `d` | Delete the selected tunnel |
| `R` | Rename the selected tunnel |
| `s` | Toggle start-on-boot |
| `p` | Save a running tunnel's live state to its `.conf` |
| `Q` | Show the tunnel as a QR code |
| `y` | Copy the interface public key to the clipboard (OSC 52) |
| `x` | Export all tunnels to `~/wireguard-tunnels.zip` |
| `Tab` | Switch the Tunnels / Log tabs |
| `m` | Toggle **Easy** / **Advanced** mode |
| `r` | Refresh now |
| `?` | Help · `q`/`Esc` Quit |

In the import browser, **Space** marks files for a **bulk import** and **Enter**
imports all marked ones at once.

**Easy mode** (the default for new users) shows only the everyday actions -
connect/disconnect, import, start-on-boot, remove and Show-QR - so it's
approachable for non-technical users. Press **`m`** for Advanced mode (edit, new,
generate keys, running config, save-live, rename, export); the choice is
remembered.

The editor uses `$VISUAL`/`$EDITOR` (falling back to `nano`). The temporary file
it opens is mode `0600` and removed afterwards.

### First run / troubleshooting

Not sure your system is ready? Run the doctor (read-only, no root needed):

```sh
wg-tui doctor    # checklist; exit 0 = OK, 1 = warnings, 2 = something critical missing
wg-tui setup     # guided, confirmation-based fix (e.g. installs wireguard-tools)
```

`wg-tui doctor` is ideal over SSH - it tells you exactly what's missing
(WireGuard tools, the helper, authorization, `/etc/wireguard`, systemd, journald).

> **It does not bundle WireGuard itself** - no kernel modules, no vendored
> `wg`/`wg-quick`. It uses your distro's `wireguard-tools` (the `.deb`/AUR/COPR
> packages depend on it; the tarball needs it present).

See **[docs/DISTROS.md](docs/DISTROS.md)** for a per-distro guide: what to install,
what to set up, what survives a reboot, and when (only as a server/gateway) you need
firewall and IP-forwarding changes.

## Documentation

- **[docs/TUTORIAL.md](docs/TUTORIAL.md)** - a step-by-step walkthrough from install
  to your first tunnel: import a `.conf` or QR image, create one from scratch,
  connect, show a QR for your phone, start-on-boot, edit safely, and troubleshooting.
- **[docs/DISTROS.md](docs/DISTROS.md)** - per-distro install/setup/reboot/firewall.
- **[docs/RELEASES.md](docs/RELEASES.md)** - detailed release history and how to
  verify signed downloads.

---

## 🔐 How privilege works

Everything that needs root goes through a single, small, auditable shell script,
`wg-helper`, installed at `/usr/local/lib/wireguard-tui/wg-helper`. The app never
runs `wg-quick` directly - it calls the helper with a fixed verb (`up`, `down`,
`save`, …). The helper:

- exports a **fixed `PATH`** so a hijacked caller environment can't redirect it,
- **validates tunnel names** strictly (no path traversal),
- writes configs **atomically** and keeps a **timestamped backup** before every
  overwrite/delete,
- logs every privileged action to the journal (`logger -t wireguard-tui`).

Authorisation is a passwordless **sudoers** drop-in for just that one script
(default), or a **polkit** rule (`--polkit`); `pkexec` is the fallback.

If you also have the desktop client installed, this app will reuse its helper
automatically.

---

## 🧗 The story behind it - pain points & fixes

**Managing WireGuard from the terminal meant memorising `wg-quick` invocations
and squinting at `wg show`.** There was no quick way to see, at a glance, which
tunnels exist, which are up, and how much they've transferred - let alone edit
one safely or hand it to a phone.

- **No overview.** `wg show` dumps raw data for *up* interfaces only; inactive
  tunnels are invisible. `wg-tui` lists every `.conf` with a live status dot and
  a clean detail view that merges the on-disk config with live `wg show` data.
- **Editing was risky.** A typo in a `.conf` only surfaces when `wg-quick up`
  fails. `wg-tui` validates the config (keys, addresses, endpoint, peers) *before*
  it ever reaches `wg-quick`, and applies changes to a running tunnel with
  `wg syncconf` so sessions don't drop.
- **Privilege was all-or-nothing.** Running the whole tool as root is a big
  surface. Here, only a tiny audited helper runs as root, via a one-line sudoers
  entry - the rest runs as you.
- **Getting a config onto a phone** used to mean copying files around. `Q`
  renders the tunnel as a scannable QR code in the terminal; `i` imports one back
  from an image.
- **Headless boxes have no GUI.** This is a TUI with zero GUI dependencies, so it
  works the same on a desktop and over SSH on a minimal server.

---

## 🛠️ Manual build (for developers)

```sh
cargo build --release        # ./target/release/wg-tui
cargo test                   # unit tests (parsing, validation, names)
./target/release/wg-tui --version
```

---

## 🤝 Contributing

Issues and PRs welcome - see [CONTRIBUTING.md](CONTRIBUTING.md). Please run
`cargo fmt --all` and `cargo clippy --all-targets -- -D warnings` before pushing.

---

## ⭐ Star this project

If `wg-tui` is useful to you, **please give it a star on GitHub** - it genuinely
helps other people discover the project and motivates further work.

👉 **[Star wireguard-tui on GitHub](https://github.com/JamilleJung/wireguard-tui)** ⭐

You can also **watch** the repo for releases and **fork** it to hack on your own ideas.

---

## ☕ Buy me a coffee

This is a free, open-source project built in spare time. If it saved you some
trouble and you'd like to say thanks, a coffee is hugely appreciated 💛

<div align="center">

[![Buy Me A Coffee](https://img.shields.io/badge/Buy%20Me%20A%20Coffee-support-FFDD00?style=for-the-badge&logo=buy-me-a-coffee&logoColor=black)](https://www.buymeacoffee.com/jamillejung)

**[☕ buymeacoffee.com/jamillejung](https://www.buymeacoffee.com/jamillejung)**

</div>

---

## ⚠️ Known limitations

- The in-terminal QR can be large for long configs; use a roomy terminal, or
  scan from a maximised window.
- Editing happens in your external `$EDITOR` (not an in-app form) - by design, so
  it stays a small, dependency-light TUI.

## 📄 License

MIT - see [LICENSE](LICENSE). WireGuard is a registered trademark of
Jason A. Donenfeld. This is an independent, unofficial client.
