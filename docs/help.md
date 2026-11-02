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
- `h`/`l`: choose a detail field. `y`: copy it.
- `e`: edit in $VISUAL, then $EDITOR, then nano. `+`: add a peer in the editor.
- `g`: generate keys. `c`: running config. `p`: save live state.
- `s`: systemd start-on-boot. `Q`: QR. `R`: rename. `d`: delete. `x`: export all.
- `K`: temporary host-output kill switch. `D`: connection diagnostics.
- `Tab`: Log. `B`: Backup manager. `w`: changelog. `A`: About. `q`/Esc: quit.

On the Log tab, `y`/Enter copies the current line or selection, `v` starts a
selection, `Y` copies the filtered log, `/` filters, `t` selects this tunnel,
`f` follows, `c` clears the view, `r` reloads, and `w` saves the full fetched
log to a file, regardless of view filters. `g`/Home
and `G`/End move to top/bottom; `h`/`l` scroll horizontally. Esc first cancels a
visual selection. In Backup, `n` creates, Enter/`r` restores, `d` deletes, and
`x` exports the selected archive. Restore/delete ask for confirmation.

Copy tries native clipboard tools before OSC 52. Across SSH, the local terminal
must allow OSC 52 when that fallback is used; a successful remote clipboard
helper can target the remote display instead.

Editing an active tunnel attempts WireGuard-level live sync. Address, DNS,
MTU, and routing changes still need reconnecting. Rename stops an active
interface without reactivating it; restoring a backup updates files without
syncing running interfaces. Backup preflight is bounded and complete before
writes, but later operational failures can leave a partial restore.

## 5. When a tunnel is active but traffic stalls

Press `D` to check clock synchronization, interface state, handshake age,
endpoint ping, and DNS lookup. These are diagnostic clues, not a proof of
end-to-end service health. ICMP ping can be blocked while WireGuard UDP works.
Check both public keys, matching PSK, endpoint/port, AllowedIPs, firewall and
routing, and system time. Unsynchronized time can affect handshakes; a missing
handshake alone does not establish its cause.

Avoid having two tools manage the same interface at once. The kill switch is
temporary firewall state, not a daemon or a server NAT setup. App exit does not
clear it; app deactivation/delete/rename attempts cleanup. Recheck after
external service or firewall changes. Existing switches need off/on after an
upgrade to install new rule logic.

## 6. Prerequisites and updates

`wg-tui doctor` is read-only; `setup` offers confirmed tools/resolver repairs.
Start-on-boot needs systemd, logs need journald, and DNS lines need resolvconf
integration. The app does not bundle WireGuard tools or kernel modules.

At startup, the app may contact GitHub to check for a newer release. `u`
installs only a detected update, verifies it against the compiled-in signing
key and exact checksum, and asks you to restart. It does not force a fresh
check. Disable checking with WG_NO_UPDATE_CHECK=1 or a no-update file under
$XDG_CONFIG_HOME/wireguard-tui (default ~/.config/wireguard-tui). Distro-package
users should prefer their package manager; the in-app installer uses the source
installation layout.
