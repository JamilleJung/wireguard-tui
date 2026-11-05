# WireGuard, explained

A guide to WireGuard concepts and the wg-tui keyboard workflow. Press `?` for
the key map. The source documentation is updated independently of published
binaries, which embed the guide as it existed when they were built.

## 1. Interfaces and peers

A WireGuard interface is a virtual network interface on this machine. It has a
private key and may have an address, listen port, DNS settings, and several
peers. Each peer is another WireGuard participant, identified by its public key.
A client often has one peer (a VPN gateway); a server can have many.

When you activate a config with `Enter` or `a`, wg-quick creates the interface
and applies routing and other settings. "Active" means that interface is up.
A successful handshake and traffic counters are better evidence that a peer
is reachable. An idle peer need not have a very recent handshake.

The app manages plain files in `/etc/wireguard`; it does not register a peer on
your server or provider. Both ends still need compatible keys and routing.

## 2. Keys and secrets

Each interface has a private/public key pair. Keep the private key secret;
share the public key with the other endpoint. The other endpoint must have
your public key in its peer list, and you must have its public key in yours.
The optional preshared key is an extra shared secret and must match at both
ends. `g` generates a keypair and PSK; importing a provider config usually
supplies keys that are already registered.

A QR code or exported config includes the private key. Screenshots, generated
key popups, running config, ZIP backups, and clipboard history can expose
secrets. Copy only what you intend to share.

## 3. Config fields

Interface fields:

- Name: the Linux interface/config name, such as `home`; up to 15 ASCII
  characters, letters/digits first, then letters/digits/`_`/`-`/`.`. No `..`
  or `.conf` suffix.
- PrivateKey: this interface's secret key.
- Address: this machine's tunnel IP/CIDR, such as `10.100.0.17/24`; use the
  address assigned by the server, not an arbitrary LAN address.
- DNS: optional servers or search domains for wg-quick to apply. A working
  `resolvconf` command is required; `doctor` checks integration.
- ListenPort: optional UDP port (automatic if omitted/zero).
- MTU: optional interface packet size; usually leave it automatic.

Peer fields:

- PublicKey: the remote peer's public key.
- PresharedKey: optional extra secret, identical at both ends.
- Endpoint: a reachable `host:port`, or `[IPv6-address]:port`. A peer can learn
  a roaming endpoint after authenticated traffic.
- AllowedIPs: destination prefixes sent to this peer, and source prefixes
  accepted from it. `0.0.0.0/0` covers all IPv4; `::/0` covers all IPv6. A
  specific prefix such as `10.100.0.0/24` is a split tunnel. IPv4-only full
  routing does not automatically route IPv6 through the VPN.
- PersistentKeepalive: optional interval in seconds (`25` is a common NAT
  traversal setting); `off` disables it.

Inline `#` comments and repeated Address/DNS/AllowedIPs lines are supported.
The helper refuses to save root script hooks such as PostUp. An administrator
who needs those must manage the file directly.

## 4. Driving wg-tui

All keys work without a mode switch. "Advanced" in `?` groups operations; `m`
does not toggle anything.

- `i`: import a config or QR image. `n`: generate a preset and edit raw text.
- Up/Down: select a tunnel. `Enter`/`a`: activate/deactivate.
- `h`/`l`: choose a detail field. `y`: copy it. `]`/`[`: jump to the next or
  previous peer. PgDn/PgUp: scroll the detail pane (it also follows the
  focused field).
- `e`: edit in $VISUAL, then $EDITOR, then nano. Quoted editor arguments work
  without shell expansion; a private draft directory is cleaned on return.
  `+`: add a peer in the editor.
- `g`: generate keys. `c`: running config. `p`: save live state.
- `s`: systemd start-on-boot. `Q`: QR. `R`: rename. `d`: delete. `x`: export all.
- `K`: temporary host-output kill switch (one managed tunnel at a time); when
  it is on but not in force, `K` offers repair or turning it off. `D`:
  connection diagnostics.
- `Tab`: Log. `B`: Backup manager. `w`: changelog. `A`: About. `q`/Esc: quit.
- `?`: the key map. It scrolls with Up/Down, PgUp/PgDn and `g`/`G`; any other
  key closes it.
- `E`: reopen an unsaved draft you set aside (see below).

On the Log tab, `y`/Enter copies the current line or selection, `v` starts a
selection, `Y` copies the filtered log, `/` filters, `t` selects this tunnel,
`f` follows, `c` clears the view, `r` reloads, and `w` saves the full fetched
log to a file, regardless of view filters. `g`/Home
and `G`/End move to top/bottom; `h`/`l` scroll horizontally. Esc first cancels a
visual selection. Visual mode freezes its log snapshot so incoming entries do
not shift the copied range. In Backup, `n` creates, Enter/`r` restores, `d` deletes, and
`x` exports the selected archive. Restore/delete ask for confirmation.

Copy tries native clipboard tools before OSC 52. Across SSH, the local terminal
must allow OSC 52 when that fallback is used; a successful remote clipboard
helper can target the remote display instead.

Editing an active tunnel attempts WireGuard-level live sync. Address, DNS,
MTU, and routing changes still need reconnecting; a changed FwMark is refused
by live sync and always needs a reconnect. If live sync fails after the file
was saved, a popup says that the saved file and the running interface now
differ and offers `s` retry sync, `c` reconnect, or `k` keep as is.

Your edited text is never thrown away. If it does not validate, or the helper
cannot save it (permission, busy lock, timeout, I/O error), a popup offers `r`
retry (save failures only), `e` re-edit with your text, `d` discard (asks
again), or Esc to keep the draft for later; `E` reopens it.

After connect, disconnect, rename, delete, start-on-boot and kill-switch
changes the app re-reads the real state (interface list, saved names,
service and firewall status) instead of trusting the helper's exit status, and
reports a mismatch. A helper timeout means the outcome is unknown: the app
re-checks and shows what actually happened, or says that it could not tell.

Rename stops an active interface without reactivating it; restoring a backup
updates files without syncing running interfaces. Backup preflight requires an
owned single-link regular ZIP and is complete before writes: 64 MiB archive,
4096 entries, 1024 configs, 1 MiB per config and 16 MiB total text. Before the
first write, restore keeps a copy of every config it will overwrite. If a later
write fails, it rolls back: previous contents are put back and tunnels the
restore created are removed again. The report names each tunnel, and any
rollback step that failed.

## 5. When a tunnel is active but traffic stalls

WireGuard only handshakes when it has something to send. A peer without
PersistentKeepalive can go hours without a handshake and be fine, so an old
handshake alone does not mean the VPN is broken. Each peer gets a health state
from its handshake age, the traffic counters between refreshes, its keepalive
and the time since activation:

- Healthy: a handshake in the last 180 seconds.
- Connecting: handshake attempts in progress (just activated or just sending).
- No traffic: up, no handshake yet, and nothing has been sent.
- Idle: the last handshake is old because nothing needed to be sent.
- Handshake stale: traffic or the keepalive should have renewed it by now.
- Connection failed: sending without any answer for longer than WireGuard
  retries (90 seconds), or a keepalive that never got a handshake.

The interface line shows the most severe peer; each peer shows its own state.

Press `D` to check clock synchronization, interface state, handshake health,
endpoint ping, and DNS lookup. These are diagnostic clues, not a proof of
end-to-end service health. ICMP ping can be blocked while WireGuard UDP works.
Check both public keys, matching PSK, endpoint/port, AllowedIPs, firewall and
routing, and system time. An unsynchronized clock is named only as one
possible cause, and only when handshakes are actually failing.

When the kill switch is on but its firewall rules are gone (for example after
a firewall reload) or no longer match the tunnel, the detail pane and footer
say so and the app runs a repair once per detected loss. Repeated losses are
spaced out by a growing delay; a failed repair is not retried automatically.

Avoid having two tools manage the same interface at once. The kill switch is temporary host-output policy, not a daemon or server NAT
setup. It requires an active interface and full firewall tools; only one
managed switch is supported. nft uses one batch; fallback commits are atomic
per IPv4/IPv6 family, not across both families. App exit does not
clear it; app deactivation/delete/rename attempts cleanup. Recheck after
external service or firewall changes. Existing switches need off/on after an
upgrade to install new rule logic.

## 6. Display themes and logging

Choose a theme with `--theme NAME` or `WG_TUI_THEME=NAME`:

- `default`: the standard colours.
- `high-contrast`: bright text, no dark grey, a yellow selection bar.
- `ascii`: only ASCII characters (`+-|` borders, no arrows or ellipses; the QR
  code uses `##` or coloured spaces).

A non-empty `NO_COLOR` turns colours off in every theme; selection and state
then use bold, reverse video and underline. Below 24x6 the app shows only a
"terminal too small" note; narrow terminals stack the list above the details.

For troubleshooting, `WG_TUI_LOG=/path/file` appends `key=value` lines to a
private file, and `--debug` writes them to stderr (redirect it, for example
`wg-tui --debug 2>wg-tui.log`). Keys are removed before anything is written.

## 7. Prerequisites and updates

`wg-tui doctor` is read-only; unknown/unverified prerequisites are warnings.
`setup` offers confirmed tools/resolver repairs.
Start-on-boot needs systemd, logs need journald, and DNS lines need resolvconf
integration. The app does not bundle WireGuard tools or kernel modules.

At startup, the app may contact GitHub to check for a newer release. `u`
installs only a detected update, verifies it against the compiled-in signing
key and exact checksum, and asks you to restart. It does not force a fresh
check. Disable checking with WG_NO_UPDATE_CHECK=1 or a no-update file under
$XDG_CONFIG_HOME/wireguard-tui (default ~/.config/wireguard-tui). Distro-package
users should prefer their package manager; the in-app installer uses the source
installation layout.

## Audited input and operation limits

Imported config files and external-editor output must be regular files at most
1 MiB. Config, QR and backup files share one open routine: non-blocking open
(and no symlinks for private files and backups), a check on the opened file,
then a bounded read. Named pipes, sockets, devices and directories are
rejected without waiting for a writer.

Config fields are validated on every occurrence, including canonical key pad
bits, CIDR/keepalive bounds and exact section/field placement. Valid comments,
repeated fields, scoped IPv6 and supported Table/FwMark/SaveConfig options remain
available; unknown fields cannot be saved just because they remain in raw text.
QR import uses at most 32 MiB source files and 4096 × 4096 dimensions with a
configured 128 MiB decoder budget, and requires one distinct valid tunnel.

Release helpers must be trusted installed/adjacent executables with protected
ancestry; install a verified user-owned bundle before using its helper. Debug
helper overrides do not grant authorization. Background reads check polkit
without password dialogs; explicit mutations can use a desktop agent. A failed
status refresh preserves prior data rather than inventing an inactive state.

Mutations are serialized by a bounded helper lock. Failed/unknown stops retain
kill-switch protection and report errors; inspect state before retrying. A
90-second UI helper deadline bounds waiting, not rollback of already completed
network changes. Local key generation is bounded separately.

Verified update bundles own their private staging directory, check all required
assets before privilege escalation, reject a non-newer version at download time,
and clean staging after completion/failure. Success reports the actual verified
version. Restart after installing, and prefer the package manager for a packaged
installation.

Helper-backed tunnel/config/import/backup/key-generation actions use serialized
workers; conflicting actions are blocked while busy. The external editor and
update installer deliberately take the foreground. Backup actions return to
the main screen; B reopens the manager. Quitting does not undo an operation
already started. Quoted editor arguments are parsed without shell expansion.
