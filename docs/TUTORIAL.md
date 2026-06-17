# wireguard-tui tutorial (wg-tui)

A complete, beginner-friendly walkthrough of `wg-tui`, the terminal UI for
managing WireGuard tunnels on Linux. No prior WireGuard knowledge is assumed.
Every command here is copy-pasteable.

This is the current release: **1.5.4**.

---

## 1. What this is and who it's for

`wg-tui` is a terminal program (a "TUI" - text user interface) that lists your
WireGuard tunnels, shows live status, and lets you connect, edit, import, and
share them - all from the keyboard, with no graphical desktop required. It is a
single native Rust binary with no GUI or C library dependencies, so it works the
same on your laptop and over SSH on a minimal server.

It is built for two kinds of people:

- **VPN clients** - you dial out to a commercial provider, or to your own server,
  and you want an easy way to turn a tunnel on and off and watch the connection.
- **Server / gateway admins** - a box that *accepts* incoming peers and routes
  them somewhere.

The difference matters: a client needs almost nothing, while a server also needs
firewall, IP-forwarding, and NAT changes. To find out which you are, and exactly
what to install for your distro, read the companion guide:

- **[docs/DISTROS.md](DISTROS.md)** - client vs server, per-distro install, what
  survives a reboot, and the firewall question.

> Prefer a window over a terminal? The sibling app
> **[wireguard-gui](https://github.com/JamilleJung/wireguard-gui)** is the same
> tool as a desktop GUI. Both share the same privilege model.

---

## 2. Requirements and install

### One-command install

```sh
git clone https://github.com/JamilleJung/wireguard-tui.git
cd wireguard-tui
./install.sh
```

Then launch it:

```sh
wg-tui
```

### What the installer does

`./install.sh` detects your package manager - apt, dnf/yum, pacman, zypper, apk,
xbps, or eopkg - and then:

- installs `wireguard-tools` (the `wg` / `wg-quick` commands) and a C linker,
- ensures a Rust toolchain (via `rustup` if you don't already have one),
- builds the project **as your normal user** (never as root), and
- installs the `wg-tui` binary and privileged helper,
- sets up passwordless authorization so day-to-day use never prompts you.

By default the installer does not install a desktop launcher or icon, keeping
server installs minimal. If you want a local application-menu entry, use:

```sh
./install.sh --desktop
```

> It does **not** bundle WireGuard itself - no kernel modules, no vendored
> `wg`/`wg-quick`. It uses your distro's `wireguard-tools`. WireGuard has been in
> the mainline Linux kernel since 5.6 (2020); `wg-quick` loads the module on
> demand when you connect.

### The no-sudo / passwordless explanation

All root-level work in `wg-tui` goes through one tiny, auditable Rust helper
(`wg-helper`). So that you are never nagged for a password during normal use, the
installer grants passwordless access to *just that one helper*:

- By default it writes a **sudoers drop-in** scoped to the helper.
- If your system can't use `sudo` at all (for example a fresh Debian server where
  your login user is not in the sudoers file), the installer re-runs itself as
  root with a **single ROOT-password prompt**, installs `sudo` if it is missing,
  and writes a passwordless drop-in for *your* user - so `wg-tui` works as a normal
  user even though you aren't in the `sudo` group.

The build always runs as the invoking user; only the privilege-setup steps need
root.

### --polkit (use a polkit rule instead of sudoers)

If you prefer a polkit rule over a sudoers drop-in:

```sh
./install.sh --polkit
```

Note `--polkit` is an **installer** flag, not a `wg-tui` flag. This is also the
fix to try if the installer reports that the sudoers drop-in was skipped.

### Manual build (developers)

```sh
cargo build --release        # ./target/release/wg-tui
cargo test                   # unit tests (parsing, validation, names)
./target/release/wg-tui --version
```

### Uninstall

```sh
./install.sh uninstall
```

(See also section 12.)

---

## 3. First run

`wg-tui` has two read-only / guided commands that check and fix your system. They
do **not** launch the full-screen UI - they print and exit, which makes them
useful to run first over SSH.

### Check your system: wg-tui doctor

```sh
wg-tui doctor
```

`doctor` is read-only and needs no root. It prints a plain-language checklist:
WireGuard tools, the privileged helper and its authorization, `/etc/wireguard`,
DNS-for-tunnels (resolvconf), systemd, and journald. It exits with a status code
so you (or a script) can tell at a glance how healthy the box is:

| Exit code | Meaning |
|-----------|---------|
| `0` | All OK |
| `1` | Usable, with warnings |
| `2` | Something critical is missing |

### Fix what's missing: wg-tui setup

```sh
wg-tui setup
```

`setup` is a guided, confirmation-based fix. It offers to install
`wireguard-tools` via your package manager, offers to install a resolvconf
provider if a `DNS =` line would otherwise fail, and points you at the installer
for the helper. It **never connects tunnels or enables start-on-boot** - you stay
in control.

### Other CLI commands

```sh
wg-tui              # launch the interactive UI
wg-tui --version    # print the version and exit
wg-tui --help       # show usage and exit
```

### Easy vs Advanced mode

When you launch the UI, you start in **Easy mode** (the default for new users).
Easy mode shows only the everyday actions:

- connect / disconnect
- import
- start-on-boot
- remove
- Show QR

Press **`m`** to switch to **Advanced mode**, which adds the expert actions: edit,
new tunnel, generate keys, show running config, kill switch, save live state,
rename, and export. Your choice is remembered (saved under
`~/.config/wireguard-tui/mode`), so the next launch starts in the mode you left.

If you press an advanced key while in Easy mode, the footer reminds you:
`Advanced action - press 'm' to switch to Advanced mode`.

---

## 4. Get a tunnel in three ways

A "tunnel" is just a `.conf` file in `/etc/wireguard`. Here are the three ways to
get one. Press **`r`** any time to refresh the list.

### (a) Import an existing provider .conf

If your VPN provider (or your own server) gave you a `.conf` file:

1. Press **`i`** to open the import file browser.
2. Move with **`Up`/`k`** and **`Down`/`j`**; press **`Enter`** (or `Right`/`l`)
   to enter a directory, **`Backspace`** (or `Left`/`h`) to go up a level.
3. Highlight the `.conf` file and press **`Enter`** to import it.
4. To import several at once, press **`Space`** to mark each file, then **`Enter`**
   to bulk-import all the marked ones.

Press **`Esc`** to cancel the browser.

### (b) Import from a QR-code image

If you have a screenshot or photo saved as a PNG or JPG of a WireGuard QR code,
`wg-tui` can decode it:

1. Press **`i`** to open the same import browser.
2. Navigate to the image file (`.png` / `.jpg`) and press **`Enter`**.

`wg-tui` reads the QR out of the image and imports it as a normal tunnel. (To go
the other way - put a tunnel onto your phone - see section 6.)

### (c) Create a new tunnel from scratch and generate keys

This needs **Advanced mode** (press **`m`** first if you are in Easy mode).

1. Press **`n`** to create a new tunnel. You will be prompted for a name (up to
   15 characters); press **`Enter`** to confirm or **`Esc`** to cancel.
2. `wg-tui` creates a template with a freshly generated private key and opens it
   in your editor (`$VISUAL`/`$EDITOR`, falling back to `nano`). Fill in the rest
   (address, peer public key, endpoint, allowed IPs), then save and quit the
   editor.

Need extra keys while editing? Press **`g`** to generate a keypair plus a preshared
key (`wg genkey` / `wg genpsk`) and have them shown to you to copy into the config.

> The temporary file the editor opens is mode `0600` and is removed afterwards.

---

## 5. Connect, disconnect, and read status

### Connect / disconnect

Highlight a tunnel in the list and press **`Enter`** or **`a`** to toggle it
between connected and disconnected. Behind the scenes this runs `wg-quick up` /
`wg-quick down` through the helper. In the list, a green `*` to the left of a
tunnel name means it is up; a dim `-` means it is down.

### Reading the detail view

With a tunnel selected, the detail pane merges its on-disk config with live data
from `wg show`, refreshed about every 1.5 seconds:

- **Public key**, **listen port**, **addresses**, and **DNS** for the interface.
- For each peer: the **latest handshake** and **transfer** (bytes sent/received).
- **Live down/up speed** - real-time throughput for the selected tunnel.
- A **Connection** health line derived from the most recent handshake:
  roughly **OK** (recent handshake), **stale** (handshake getting old), or
  **waiting** (no handshake yet).

Quick reference for what a healthy client tunnel looks like: a recent **latest
handshake** (seconds/minutes ago, not hours), transfer counters that climb as you
use the network, and a non-zero down/up speed when traffic is flowing.

Press **`y`** to copy the interface public key to your clipboard (via the OSC 52
terminal escape, so it works locally and over SSH in supporting terminals) - handy
when a server admin asks for your public key.

---

## 6. Put a tunnel on your phone with Show QR

To move a tunnel to the WireGuard mobile app, render it as a scannable QR code:

1. Highlight the tunnel.
2. Press **`Q`** (capital Q). The QR code is drawn right in the terminal.
3. In the WireGuard mobile app, choose "Add tunnel" -> "Scan from QR code" and
   point your phone at the screen.

> **Security warning:** the QR code contains the tunnel's **private key**. Anyone
> who photographs your screen gets full access to that tunnel. Only show a QR when
> it is safe for someone to see your screen.

If the terminal is too small, `wg-tui` will tell you the QR is bigger than the
window rather than drawing an unscannable, cropped code. Enlarge the window (or
zoom out) and press **`Q`** again.

---

## 7. Start-on-boot

To have a tunnel come up automatically at every boot, highlight it and press
**`s`** to toggle start-on-boot. This flips the systemd `wg-quick@<name>` unit.

> **Non-systemd limitation:** this one feature needs systemd. On OpenRC (Alpine),
> runit (Void), or other non-systemd setups, the toggle is unavailable - everything
> else in the app still works. There, enable boot-time tunnels with your distro's
> own service manager (for example an OpenRC init script or runit service that runs
> `wg-quick up <name>`). See [docs/DISTROS.md](DISTROS.md) for the details.

Connecting a tunnel once (section 5) does **not** survive a reboot; only
start-on-boot does.

---

## 8. Edit safely

This needs **Advanced mode** (press **`m`**).

1. Highlight a tunnel and press **`e`** to open it in your editor
   (`$VISUAL`/`$EDITOR`, falling back to `nano`).
2. Make your changes, save, and quit.

`wg-tui` **validates the config before saving** - it checks keys, addresses,
endpoint, and peers, so a typo is caught up front instead of only when
`wg-quick up` fails later. The helper also performs a second basic config-shape
check before replacing files. (A bracketed IPv6 endpoint like
`[2001:db8::1]:51820` is accepted.)

If the tunnel you edited is **currently running**, the change is applied live with
`wg syncconf`, so your connection and its peers are not dropped while you tweak it.

> If a config contains `PostUp` / `PreUp` / `PostDown` / `PreDown` lines, the app
> warns you - those run shell commands as root, so review them before activating.
> On a server you will deliberately have a NAT/masquerade `PostUp`; that warning is
> expected. See [docs/DISTROS.md](DISTROS.md) section 6.

---

## 9. Rename, remove, export

| Action | Key | Mode | Notes |
|--------|-----|------|-------|
| Rename a tunnel | `R` (capital) | Advanced | Type the new name (up to 15 chars), `Enter` to confirm, `Esc` to cancel. |
| Remove a tunnel | `d` | Easy or Advanced | Confirmation prompt: press `y`/`Y` to confirm, anything else to cancel. |
| Export all tunnels | `x` | Advanced | Writes a `.zip` of every tunnel to `~/wireguard-tunnels.zip`. |

Two more Advanced-mode tools for live tunnels:

- **`c`** - show a running tunnel's live config (`wg showconf`).
- **`p`** - save a running tunnel's live state back into its `.conf`
  (`wg-quick save`).

> **Security warning:** an exported `.zip` contains your tunnels' **private keys**.
> Keep it somewhere safe and delete it when you're done.

---

## 10. Full key map reference

These are the keys in the interactive UI. Press **`?`** at any time for the
in-app help.

| Key | Action |
|-----|--------|
| `Up`/`k`, `Down`/`j` | Move selection (scroll the Log tab) |
| `Enter` / `a` | Activate or deactivate the selected tunnel |
| `e` | Edit the selected tunnel in `$EDITOR` |
| `n` | Create a new tunnel (generated key + `$EDITOR`) |
| `i` | Import from a `.conf` file or QR image |
| `g` | Generate a keypair + preshared key |
| `c` | Show a running tunnel's live config (`wg showconf`) |
| `d` | Delete the selected tunnel |
| `R` | Rename the selected tunnel |
| `s` | Toggle start-on-boot |
| `K` | Toggle the helper-managed kill switch for an active tunnel |
| `p` | Save a running tunnel's live state to its `.conf` |
| `Q` | Show the tunnel as a QR code |
| `y` | Copy the interface public key to the clipboard (OSC 52) |
| `x` | Export all tunnels to `~/wireguard-tunnels.zip` |
| `Tab` | Switch between the Tunnels and Log tabs |
| `m` | Toggle Easy / Advanced mode |
| `r` | Refresh now |
| `?` | Show help |
| `q` / `Esc` | Quit |

**In the import browser:** `Up`/`k`, `Down`/`j` move; `Enter` (or `Right`/`l`)
opens a directory or imports the highlighted file; `Backspace` (or `Left`/`h`)
goes up a level; **`Space`** marks a file for bulk import and **`Enter`** then
imports all marked files; `Esc` cancels.

**Easy mode** shows only the everyday actions (connect/disconnect, import,
start-on-boot, remove, Show QR). **Advanced mode** (press `m`) adds edit, new,
generate keys, running config, kill switch, save-live, rename, and export.

---

## 11. Troubleshooting

### "resolvconf: command not found" / a tunnel with DNS = won't connect

If a tunnel's `[Interface]` has a `DNS =` line, `wg-quick` calls `resolvconf` to
apply it. On minimal Debian (and bare Arch/Alpine/Void) there is no `resolvconf`
binary, so activation fails like this:

```
[#] resolvconf -a wg0 -m 0 -x
/usr/bin/wg-quick: line 32: resolvconf: command not found
```

`wg-tui` rewrites this into a friendly message telling you to install a provider.
Fix it once:

```sh
sudo apt install openresolv        # Debian/Arch/etc.
# or, if you have no sudo (Debian-minimal):
su root -c 'apt install openresolv'
```

If **systemd-resolved** is already active (the default on Ubuntu and Fedora), this
is already covered and you need nothing. `wg-tui doctor` shows a
"DNS for tunnels (resolvconf)" line, and `wg-tui setup` offers to install the
provider for you.

### "<user> is not in the sudoers file"

This means your login user can't use `sudo`. Re-run the installer - on a box
without usable sudo it re-runs itself as root with a single ROOT-password prompt,
installs `sudo` if missing, and writes a passwordless drop-in for your user:

```sh
./install.sh
```

Tip: you can run `su -` first and then `./install.sh` to be prompted once rather
than per step.

### Helper authorization was skipped / day-to-day use prompts for a password

If the installer reports that the sudoers drop-in was skipped ("sudoers
validation failed"), re-run it asking for a polkit rule instead:

```sh
./install.sh --polkit
```

(`--polkit` is an installer flag, not a `wg-tui` one.)

### "spawn failed: No such file or directory" (or any escalation error)

This cryptic message means the app could not gain root: there is no passwordless
`sudo` for the helper and no `pkexec` to fall back to. Two fixes:

- Re-run the installer to set up a working privilege path:

  ```sh
  ./install.sh
  ```

- Or run `wg-tui` as root, in which case it talks to the helper directly:

  ```sh
  sudo wg-tui
  ```

### Check everything at once

```sh
wg-tui doctor      # tools, helper, authorization, /etc/wireguard, DNS, systemd
wg-tui setup       # guided, confirmation-based fix for anything missing
```

For per-distro specifics (what to install, what survives a reboot, the firewall
and IP-forwarding questions for servers), see **[docs/DISTROS.md](DISTROS.md)**.

---

## 12. Uninstall

Remove the binary, helper, optional desktop files, and the authorization rule:

```sh
./install.sh uninstall
```

Your tunnel `.conf` files in `/etc/wireguard` are your data and are left in place;
remove them yourself if you no longer want them.
