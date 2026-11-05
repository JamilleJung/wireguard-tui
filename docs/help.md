# WireGuard, explained

A friendly guide to what WireGuard is, what every field means, and how to
drive wg-tui. You don't need to be a network engineer: read the first two
sections and you can set up a tunnel. (Press `?` for the key map; this page
is the *concepts*.)

This guide is built into the app, so a published binary shows the version
that existed when it was built.

---

## 1. The 60-second mental model

A WireGuard tunnel is an encrypted link between two machines. Think of it as
a private cable plugged in between you and a server: whatever goes through
the cable is encrypted, and to the rest of the network it looks like it comes
from the machine at the other end.

Each tunnel has two halves:

- The Interface = you (this machine). It has a private key, an address
  inside the tunnel, and optionally DNS servers to use while connected.
- The Peer = the other machine (usually your VPN server). It has a public
  key, a public address to reach it at (the endpoint), and a list of which
  traffic should go through it (AllowedIPs).

A client usually has one peer. A server can have many.

When you activate a tunnel (`Enter` or `a`), wg-quick creates the interface
and sets up routing. "Active" means that interface is up - not yet that the
other side is answering. The two halves prove who they are with a quick
cryptographic hello, the handshake, and then traffic flows. A recent
handshake and growing traffic counters are the real evidence that a peer is
reachable. If something seems off, press `D` (Diagnose).

The app manages plain files in `/etc/wireguard`. It does not register you on
your server or provider: both ends still need each other's keys and
matching routing.

---

## 2. The keys (this is the whole security model)

WireGuard uses public-key cryptography. Every interface has a key pair:

- Private key: your secret. It stays on this machine and you never share
  it.
- Public key: derived from the private key. You DO share this one; it is how
  the other side recognizes you.

The rule is symmetric: the server must have your public key in its peer
list, and you must have the server's public key in yours. If the server
doesn't know your public key, it silently ignores your handshakes - a very
common cause of "it won't connect".

- Preshared key (PSK): an optional extra shared secret on top, for
  belt-and-braces. If your config has one, the other end must have the
  exact same PSK, or the handshake fails.

`g` shows a fresh key pair and PSK. Most of the time you won't need it:
import a config from your provider or server (for example wg-easy) and the
keys are already filled in and already registered on the server.

Secrets leak through pictures too. A QR code, an exported config, a ZIP
backup, the running config, the `g` popup, screenshots and clipboard history
can all contain private keys. Copy and show only what you mean to share.

---

## 3. Every field, in plain words

Interface (you):

- Name: the Linux interface and file name, for example `home`. Up to 15
  ASCII characters; starts with a letter or digit, then letters, digits,
  `_`, `-` or `.`. No `..` and no `.conf` suffix.
- PrivateKey: this interface's secret key (see above).
- Address: your IP inside the tunnel, for example `10.100.0.17/24`. Use the
  address your server assigned, not your normal LAN address.
- DNS: optional DNS servers or search domains to use while connected, for
  example `1.1.1.1`. wg-quick needs a working `resolvconf` command for this;
  `wg-tui doctor` checks it.
- ListenPort: optional UDP port. Leave it out (or 0) for automatic.
- MTU: optional packet size. Usually leave it automatic; lowering it can help
  on flaky links.

Peer (the other side):

- PublicKey: the other side's public key.
- PresharedKey: optional extra secret, identical at both ends.
- Endpoint: where to reach it, `host:port` or `[IPv6-address]:port`, for
  example `vpn.example.com:51820`. This is the one address that travels over
  your normal internet. A peer can also learn a roaming endpoint from
  authenticated traffic.
- AllowedIPs: which destinations are sent into the tunnel to this peer, and
  which source addresses are accepted from it.
  - `0.0.0.0/0` = full tunnel for IPv4: all IPv4 traffic goes through the
    VPN. `::/0` does the same for IPv6.
  - `10.100.0.0/24` = split tunnel: only that subnet uses the VPN; the rest
    stays on your normal connection.
  - IPv4-only full routing does not send your IPv6 traffic through the VPN.
- PersistentKeepalive: send a tiny packet every N seconds to keep the path
  open through NAT and firewalls. `25` is a common choice behind a home
  router; `off` disables it.

Inline `#` comments and repeated Address, DNS and AllowedIPs lines are fine.
The helper refuses to save root script hooks (PreUp, PostUp, PreDown,
PostDown), because they would run as root. An administrator who needs them
must manage that file directly.

---

## 4. Driving wg-tui

Every key works all the time; there is no mode switch. "Advanced" in the `?`
key map is only a heading, and `m` does nothing.

Get a tunnel in:

- `i`: import a `.conf` or a QR image (PNG/JPEG) with the file browser. The
  usual way. `Space` marks several files, `Enter` imports.
- `n`: new tunnel from a preset (Interface only, Full tunnel, Split tunnel).
  A private key is created for you, then your editor opens so you can add the
  address and the peer's public key and endpoint.

Day to day:

- Up/Down (or `k`/`j`): select a tunnel. `Enter`/`a`: connect/disconnect.
- `h`/`l` (Left/Right): focus a detail field. `y`: copy it.
- `]`/`[`: jump to the next/previous peer. PgDn/PgUp: scroll the detail
  pane (it also follows the focused field).
- `s`: start on boot (systemd). `Q`: show the config as a QR code.
- `D`: diagnose the connection.

Advanced (still always available):

- `e`: edit the raw config in $VISUAL, then $EDITOR, then nano. Quoted editor
  arguments work, without shell expansion. `+`: add a peer section in the
  editor. `E`: reopen a draft you set aside (see section 7).
- `g`: generate keys. `c`: running config. `p`: save the live state to the
  `.conf`.
- `K`: kill switch on/off/repair (section 6). `R`: rename. `d`: delete.
  `x`: export everything.

Elsewhere:

- `Tab`: the Log tab. `B`: the Backup manager. `H`: this guide. `w`: what's
  new. `A`: about. `u`: install a detected update. `r`: refresh.
- `?`: the key map. It scrolls with Up/Down, PgUp/PgDn and `g`/`G`; any other
  key closes it.
- `q`/Esc: quit. Ctrl+C works everywhere.

---

## 5. Is my tunnel OK?

WireGuard is a polite protocol: it only handshakes when it has something to
send. A peer without PersistentKeepalive can go hours without a handshake and
be perfectly fine. So an old handshake on a quiet tunnel isn't a funeral,
it's a nap.

Each peer gets its own health state, worked out from its handshake age, the
traffic counters between refreshes, its keepalive and the time since the
tunnel came up:

- Healthy: a handshake in the last 180 seconds.
- Connecting: handshake attempts in progress (just activated, or just
  started sending).
- No traffic: up, no handshake yet, and nothing has been sent that would
  start one.
- Idle: the last handshake is old because nothing needed to be sent. Normal.
- Handshake stale: traffic or the keepalive should have renewed the
  handshake by now.
- Connection failed: sending without any answer for longer than WireGuard
  retries (90 seconds), or a keepalive that never got a handshake.

The interface line shows the most severe peer; each peer shows its own
state. Only "Handshake stale" and "Connection failed" mean trouble.

Press `D` to check clock sync, the interface, handshake health, an endpoint
ping and a DNS lookup. These are clues, not proof of end-to-end health: ICMP
ping can be blocked while WireGuard's UDP works fine. When handshakes fail,
check both public keys, a matching PSK, the endpoint and port, AllowedIPs,
firewall and routing, and the system time. An unsynchronized clock is named
only as one possible cause, and only when handshakes are actually failing.

---

## 6. The kill switch

`K` on a connected tunnel turns on a temporary kill switch: traffic leaving
this machine outside the tunnel is rejected. Loopback, the tunnel itself,
WireGuard's own transport, IPv6 neighbor discovery, DHCP and your current SSH
session stay allowed, and your own firewall's deny rules still apply.

- It needs an active interface and full firewall tools (nftables, or
  iptables with iptables-restore, plus ip6tables when IPv6 is present).
- Only one tunnel can own the kill switch at a time.
- It is not a daemon and not server NAT setup: it is temporary host-output
  policy. A reboot clears it; quitting the app does not. Disconnect, delete
  and rename try to clean it up.
- nftables changes are one batch; the iptables fallback commits each family
  (IPv4, IPv6) atomically, but not both together.

If the switch is on but its firewall rules are gone (for example after a
firewall reload) or no longer match the tunnel, the detail pane and footer
say so, and the app runs one repair per detected loss. Repeated losses are
spaced out by a growing delay; a failed repair is not retried on its own.
Pressing `K` then offers `r` repair now, `o` turn it off, or Esc cancel.

A kill switch built by an older release shows as a mismatch and is rebuilt
in the new layout. Recheck the status after changing firewall or system
services yourself.

---

## 7. Editing safely

Editing an active tunnel saves the file, then applies the WireGuard settings
live, so the connection isn't dropped. Address, DNS, MTU and routing changes
still need a reconnect. A changed FwMark is refused by the live sync and
always needs a reconnect. If the live sync fails after the file was saved, a
popup says the saved file and the running interface now differ and offers
`s` retry sync, `c` reconnect, or `k` keep as is.

Your edited text is never thrown away. If it does not validate, or the
helper cannot save it (permission, busy lock, timeout, I/O error), a popup
offers:

- `r`: retry the save (save failures only)
- `e`: re-edit, starting from your text
- `d`: discard (it asks once more)
- Esc: keep the draft for later; `E` reopens it.

After connect, disconnect, rename, delete, start-on-boot and kill-switch
changes, the app re-reads the real state (interfaces, saved names, service
and firewall status) instead of trusting the helper's exit status, and tells
you if they disagree. A helper timeout means the outcome is unknown: the app
re-checks and shows what actually happened, or says it could not tell.

Rename stops an active interface and leaves it stopped.

---

## 8. Backups, logs and copying

Backups (`B`): `n` creates a ZIP of every tunnel, Enter/`r` restores, `d`
deletes, `x` copies the selected archive to your home directory. Restore and
delete ask first. Backups are shared with the desktop app and contain
unencrypted private keys.

A restore checks the whole archive before it writes anything, keeps a copy of
every config it will overwrite, and then saves one by one. If a later save
fails, it rolls back: the old contents go back and tunnels the restore
created are removed again. The report names each tunnel, and any rollback
step that failed. A restore updates files but does not sync running
interfaces.

Log tab (`Tab`):

- `y`/Enter copies the current line or the selection; `v` starts a
  selection (Esc cancels it first); `Y` copies the whole filtered log.
- `/` filters, `t` shows only this tunnel, `f` follows new lines.
- `g`/Home and `G`/End jump to top/bottom; `h`/`l` scroll sideways.
- `c` clears the view (not the journal), `r` reloads, `w` saves the full
  fetched log to a file, regardless of the view filters.

A visual selection freezes its log snapshot, so incoming lines cannot shift
the copied range.

Copying tries native clipboard tools first and OSC 52 last. Over SSH, the
local terminal must allow OSC 52 when that fallback is used, and a working
remote clipboard tool may target the remote display instead.

---

## 9. Themes and the debug log

Pick a theme with `--theme NAME` or `WG_TUI_THEME=NAME`:

- `default`: the standard colours.
- `high-contrast`: bright text, no dark grey, a yellow selection bar.
- `ascii`: only ASCII characters (`+-|` borders, no arrows or ellipses; the
  QR code uses `##` or coloured spaces).

A non-empty `NO_COLOR` turns colours off in every theme; selection and state
then use bold, reverse video and underline. Below 24x6 the app only shows a
"terminal too small" note, and narrow terminals stack the list above the
details.

For troubleshooting, `WG_TUI_LOG=/path/file` appends `key=value` lines to a
private file, and `--debug` writes them to stderr (redirect it, for example
`wg-tui --debug 2>wg-tui.log`). Keys are removed before anything is written.

---

## 10. Prerequisites and updates

`wg-tui doctor` is a read-only checklist; anything unknown or unverified is a
warning. `wg-tui setup` offers confirmed repairs for tools and the resolver.
Start-on-boot needs systemd, the Log tab needs journald, and DNS lines need
resolvconf integration. The app does not bundle WireGuard tools or kernel
modules.

At startup the app may contact GitHub to check for a newer release. `u`
installs only an update it already detected: it verifies the download
against the signing key compiled into the app and an exact checksum, then
asks you to restart. It does not force a fresh check. Turn checking off with
WG_NO_UPDATE_CHECK=1 or a `no-update` file in $XDG_CONFIG_HOME/wireguard-tui
(default ~/.config/wireguard-tui). Updates keep the prefix and authorization
mode the installer recorded. If a distro package installed the app, use your
package manager instead.

---

## 11. Gotchas worth knowing

- "Active, but nothing loads." The interface is up, but the handshake isn't
  completing, so no real traffic flows. Usual suspects: the server doesn't
  have your public key, the endpoint or port is wrong or unreachable, or the
  PSK doesn't match. A clock that is not synced is one more possible cause.
  `D` helps narrow it down.
- Don't manage the same tunnel from two places (for example NetworkManager
  and wg-tui). They'll fight. Pick one.

---

## Audited limits (for the curious)

Imported config files and external-editor output must be regular files of
at most 1 MiB. Config, QR and backup files share one open routine:
non-blocking open (and no symlinks for private files and backups), a check
on the opened file, then a bounded read. Named pipes, sockets, devices and
directories are refused without waiting for a writer.

Config fields are validated on every occurrence, including canonical key pad
bits, CIDR/keepalive bounds and exact section/field placement. Comments,
repeated fields, scoped IPv6 and the supported Table/FwMark/SaveConfig
options remain available; an unknown field cannot be saved just because it
stays in the raw text.

QR import accepts source files up to 32 MiB and images up to 4096x4096, with
a 128 MiB decoder budget, and needs exactly one distinct valid tunnel. The
image is decoded in a short-lived child process with a memory limit and a
deadline, so a hostile image can only fail its own import.

Backup restore requires an owned, single-link, regular ZIP and checks it
completely before writing: at most 64 MiB per archive, 4096 entries, 1024
configs, 1 MiB per config and 16 MiB of config text in total.

Release builds only use a trusted installed or adjacent helper with
protected parent directories; install a verified bundle before using a
user-owned extracted one. Debug helper overrides do not grant authorization.
Background reads check polkit without password dialogs; explicit changes can
use a desktop authentication agent. A failed status refresh keeps the
previous data instead of inventing an inactive state.

Changes are serialized by a bounded helper lock. A failed or unknown stop
keeps kill-switch protection and reports the error; check the state before
retrying. The 90-second helper deadline bounds the waiting, not the rollback
of network changes that already happened. Local key generation has its own
shorter deadline.

Verified update bundles own their private staging directory, check every
required file before asking for privilege, refuse a version that is not
newer, and clean up after success or failure. Success reports the version
actually verified.

Helper-backed actions (tunnels, configs, imports, backups, key generation)
run on serialized background workers; a conflicting action is refused while
another runs. The external editor and the update installer take the
foreground on purpose. Backup actions return to the main screen; `B`
reopens the manager. Quitting does not undo an operation already started.

---

Your configs live in `/etc/wireguard`, and everything that needs root goes
through one small helper that runs `wg`, `wg-quick` and friends. Apart from
your tunnels, the app only reaches out to the network for the optional
update check (and the update download), and for the ping and DNS lookup that
`D` runs when you ask.
