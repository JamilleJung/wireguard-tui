# wireguard-tui — `wg-tui`

A native **terminal UI** for managing WireGuard tunnels on Linux. It does
everything the desktop client does — tunnel list, live status, activate/
deactivate, an editor, key generation, QR codes, export and start-on-boot —
without leaving your terminal. Pure Rust, no GUI libraries, runs great over SSH.

> Sibling project of the desktop GUI. Same hardened privilege model, same
> `wg`/`wg-quick` coverage — just keyboard-driven.

```
┌ WireGuard — terminal ───────────────────────────────────────────────────┐
│  Tunnels │ Log                                                           │
└──────────────────────────────────────────────────────────────────────────┘
┌ Tunnels ─────────────┐┌ home-server ───────────────────────────────────┐
│  ● home-server       ││ Interface                                       │
│  ○ work-vpn          ││   Status        Active                          │
│  ○ tokyo-relay       ││   Public key    Hk3pQ2vN8sLrYwZ1aFcJ4mD6tB9…    │
│  ○ us-east-1         ││   Listen port   51820                           │
│                      ││   Addresses     10.7.0.2/24, fd00:7::2/64       │
│                      ││   DNS           1.1.1.1, 1.0.0.1                 │
│                      ││   Start on boot Yes                             │
│                      ││                                                 │
│                      ││ Peer 1                                          │
│                      ││   Endpoint      vpn.example.com:51820           │
│                      ││   Latest hs     38 seconds ago                  │
│                      ││   Transfer      1.24 GiB received, 318 MiB sent │
└──────────────────────┘└─────────────────────────────────────────────────┘
 ↑↓ move  ⏎/a on·off  e edit  n new  i import  g gen-key  Q qr  x export  q quit
```

---

## ✨ Features

- 📜 **Tunnel list** of everything under `/etc/wireguard`, with a live ●/○ active dot.
- 🔌 **Activate / Deactivate** with one key (`wg-quick up` / `down`).
- 🧾 **Live detail view** — status, public key, listen port, addresses, DNS,
  and per-peer **latest handshake** + **transfer**, refreshed every second.
- 📝 **Edit** any tunnel in your `$EDITOR`, with **config validation** before saving.
- ♻️ **Apply edits to a running tunnel live** (`wg syncconf`) without dropping peers.
- ➕ **New tunnel** from a generated template (fresh private key), and
  **🔑 Generate** keypairs + preshared keys on demand (`wg genkey`/`genpsk`).
- 📥 **Import** a `.conf` file **or a QR-code image** (`wg-tui` decodes the PNG/JPG).
- 📱 **Show QR** — render a tunnel as a QR code right in the terminal to scan
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
| `x` | Export all tunnels to `~/wireguard-tunnels.zip` |
| `Tab` | Switch the Tunnels / Log tabs |
| `r` | Refresh now |
| `?` | Help · `q`/`Esc` Quit |

The editor uses `$VISUAL`/`$EDITOR` (falling back to `nano`). The temporary file
it opens is mode `0600` and removed afterwards.

---

## 🔐 How privilege works

Everything that needs root goes through a single, small, auditable shell script,
`wg-helper`, installed at `/usr/local/lib/wireguard-tui/wg-helper`. The app never
runs `wg-quick` directly — it calls the helper with a fixed verb (`up`, `down`,
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

## 🧗 The story behind it — pain points & fixes

**Managing WireGuard from the terminal meant memorising `wg-quick` invocations
and squinting at `wg show`.** There was no quick way to see, at a glance, which
tunnels exist, which are up, and how much they've transferred — let alone edit
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
  entry — the rest runs as you.
- **Getting a config onto a phone** used to mean copying files around. `Q`
  renders the tunnel as a scannable QR code in the terminal; `i` imports one back
  from an image.
- **Headless boxes have no GUI.** This is a TUI with zero GUI dependencies, so it
  works the same on a desktop and over SSH on a minimal server.

---

## 🛠️ Manual build (for developers)

```sh
cargo build --release        # ./target/release/wg-tui
WGTUI_DEMO=1 cargo run        # demo mode: sample tunnels, no root, no real configs
```

`WGTUI_DEMO=1` (or `WGGUI_DEMO=1`) shows polished sample data — handy for
screenshots and for trying the UI without touching `/etc/wireguard`.

---

## 🤝 Contributing

Issues and PRs welcome — see [CONTRIBUTING.md](CONTRIBUTING.md). Please run
`cargo fmt --all` and `cargo clippy --all-targets -- -D warnings` before pushing.

---

## ⭐ Star this project

If `wg-tui` saves you some keystrokes, a **star** helps other people find it —
thank you!

## ☕ Buy me a coffee

If it saved you time and you'd like to say thanks:
**https://www.buymeacoffee.com/jamillejung** ☕

---

## ⚠️ Known limitations

- The in-terminal QR can be large for long configs; use a roomy terminal, or
  scan from a maximised window.
- Editing happens in your external `$EDITOR` (not an in-app form) — by design, so
  it stays a small, dependency-light TUI.

## 📄 License

MIT — see [LICENSE](LICENSE). WireGuard is a registered trademark of
Jason A. Donenfeld. This is an independent, unofficial client.
