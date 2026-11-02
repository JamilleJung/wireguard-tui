# wireguard-tui tutorial

This guide describes **1.9.1**. `wg-tui` manages plain Linux WireGuard configs
with a keyboard interface. The [distro guide](DISTROS.md) covers prerequisites;
[the concepts guide](help.md) explains keys, peers, and routing.

## 1. Install and check the system

Run the source installer as your normal user:

```sh
git clone https://github.com/JamilleJung/wireguard-tui.git
cd wireguard-tui
./install.sh
wg-tui doctor
```

The installer can install WireGuard tools, runtime dependencies, Rust and a
linker for a source build, then install the application and privileged helper.
It defaults to a scoped sudoers grant and installs a terminal desktop launcher
and icon. Use `./install.sh --no-desktop` to omit the launcher.
`./install.sh --polkit` selects group-restricted polkit authorization instead.
The installer attempts group setup for that mode; log out/in and check
`id -nG`. Its passwordless rule requires an active local session, so SSH users
usually want the default sudoers grant.

If sudo is unusable, the installer can request the root password once through
`su` while remembering your normal user. A direct root login requires an
explicit `--allow-root`; prefer a normal user for source builds. Do not assume
starting a fresh `su -` login preserves the original user.

You can also install a verified prebuilt tarball or `.deb`. See
[release instructions](RELEASES.md) before downloading: a release tarball
contains both binaries and automatically skips Rust/compiler setup, while a
source archive still needs a build. The `.deb` installs the polkit rule but
does not create the `wireguard` group or grant membership automatically.

`doctor` is read-only. Exit `0` means no warnings, `1` means warnings, and `2`
means critical prerequisites are missing. It checks WireGuard tools, the
helper/authorization, `/etc/wireguard`, resolvconf integration, systemd/journal
availability, and clock synchronization. It does not prove a tunnel can connect.

```sh
wg-tui setup       # offers confirmed tools/resolver repairs
wg-tui --version   # -V also works
wg-tui --help      # -h also works
wg-tui             # open the full-screen interface
```

`setup` does not connect tunnels, create `/etc/wireguard`, install the helper,
or enable tunnel start-on-boot. It directs you to the installer for the helper;
the helper creates `/etc/wireguard` on the first save. Setup returns a fresh
doctor status. The source installer also creates a `wireguard-tui` command
alias; packages may provide only `wg-tui`.

## 2. Import a tunnel

Press `i`. The browser starts in the current directory and lists directories
and `.conf`, `.png`, `.jpg`, `.jpeg` files; hidden files are skipped.

1. Use `Up`/`k` and `Down`/`j` to select an entry.
2. Use `Right`/`l` or `Enter` to enter a directory; `Left`/`h`/`Backspace` goes up.
3. Select a `.conf` or QR image and press `Enter` to import it.
4. For several files, mark them with `Space`, then press `Enter` to import the
   marked set while a file is highlighted. `Esc` cancels.

Imported file names are sanitized to Linux interface names and made unique.
Review the imported details before activating. Frontend validation catches
common config mistakes; the helper rechecks saved content and rejects
`PreUp`, `PostUp`, `PreDown`, and `PostDown` root script hooks. A trusted
administrator who needs hooks must manage those config files directly as root.

## 3. Create and edit a tunnel

All keys are available immediately; there is no Easy/Advanced mode switch and
`m` does nothing. Press `?` for the grouped key map.

Press `n`, enter a name up to 15 ASCII characters, then select a preset:
`i` for Interface only, `f`/`Enter` for full tunnel, or `s` for split tunnel.
The app generates a private key and opens the raw config in `$VISUAL`, then
`$EDITOR`, then `nano`. Fill in the assigned address and peer data, save, and
exit successfully. To edit an existing tunnel use `e`; `+` appends a peer
section for you to complete in the same editor. It must have valid PublicKey
and AllowedIPs before it can be saved.

Names start with a letter/digit and use only letters/digits/`_`/`-`/`.`; `..`
and a `.conf` suffix are rejected. WireGuard inline `#` comments and repeated
Address/DNS/AllowedIPs lines are supported. The temporary editor file is `0600`
and removed after editing. Editor command strings are split on whitespace;
use a wrapper script if you need shell quoting or several shell commands.

`g` displays a new keypair and preshared key. That popup includes secret keys;
keep the screen private. The TUI uses raw text editing rather than the desktop
client's form editor.

Editing a running tunnel saves to disk then attempts `wg syncconf` for
WireGuard-level changes. Changes to Address, DNS, MTU, or routing still need a
reconnect, even when the app says "applied live". Quick-add peer saves only and
does not live-sync. `c` shows running config; `p` runs `wg-quick save`, replacing
the saved state after a backup.

## 4. Connect and inspect

Select a tunnel and press `Enter` or `a` to run `wg-quick up`/`down` through the
helper. The list uses a green `*` for active and a dim `-` for inactive. Active
means an interface is up, not that every peer has completed a handshake.

The detail view combines saved config and live `wg show` data about every
1.5 seconds. It shows interface/peer public keys, address, DNS, endpoint,
allowed IPs, keepalive, handshake, counters, and current throughput. The
connection summary uses the most recent peer handshake; check individual peers
when managing a server.

Use `h`/`l` or Left/Right to focus a detail field and `y` to copy it. Copy tries
native tools (`wl-copy`, `xclip`, `xsel`) before OSC 52. Over SSH, OSC 52 works
only when the local terminal/multiplexer allows it; a working remote display
clipboard can take precedence. A sent OSC 52 escape is not proof the local
clipboard accepted it.

`D` diagnoses the selected tunnel: clock status, active interface, handshake,
endpoint ping, and DNS lookup. A failed ICMP ping does not prove the WireGuard
UDP endpoint is down. Check endpoint/port, both peer public keys, matching PSK,
firewall/routing, and time synchronization when a tunnel is active but stalled.

## 5. Everyday actions

| Key | Action |
|---|---|
| `s` | Enable/disable the systemd `wg-quick@<name>` unit for future boots |
| `Q` | Show the full saved config as a terminal QR code |
| `R` | Rename; the helper stops an active tunnel and does not reactivate it |
| `d` | Delete after `y`/`Y` confirmation; backs up the file first |
| `x` | Export all configs to `~/wireguard-tunnels.zip` |
| `H`, `w`, `A` | Concepts guide, changelog, About |

The QR and ZIP contain private keys. Enlarge the terminal if a QR does not fit.
Start-on-boot is systemd-only; it does not connect immediately. On OpenRC/runit,
manage boot activation with your distro's service manager.

## 6. Log tab

Press `Tab` (or Shift+Tab). The helper reads its activity log and `wg-quick@*`
journal entries; systems without journald show a placeholder.

| Key | Action |
|---|---|
| `Up`/`k`, `Down`/`j`, `PgUp`/`PgDn` | Move cursor or page |
| `Home`/`g`, `End`/`G`, `h`/`l` | Top, bottom, horizontal scroll |
| `f`, `t`, `/` | Follow newest, filter selected tunnel, edit text filter |
| `y`/`Enter` | Copy current line or visual selection |
| `v`, `Y` | Start/end visual selection, copy whole filtered log |
| `c`, `r` | Clear the displayed view, reload from journal |
| `w` | Save full fetched log to `~/wireguard-tui-log.txt` (`0600`) |

View filters do not limit the saved file. Clearing affects only the view, not
the system journal. `Esc` cancels a visual
selection before it quits the application. Log-tab keys override their tunnel
actions: `g` goes to the top, `c` clears, `w` saves, and `Enter` copies.

## 7. Backup manager

Press `B` from either tab. `n` creates a ZIP of all tunnels; `Enter`/`r` restores,
`d` deletes, and both destructive actions ask for confirmation. `x` copies the
selected archive to your home directory; `Esc` closes the manager.

Archives live in `$XDG_DATA_HOME/wireguard/backups` or
`~/.local/share/wireguard/backups`, shared with the desktop client. They contain
private keys and are stored as `0600` files. Exported backup copies also use
`0600`. The helper's separate per-file backups live in `/etc/wireguard/.backup/`.

Restore validates the entire archive before starting any write: only valid
root-level `.conf` names, no duplicates or hooks, valid config content, at most
1 MiB per config, 16 MiB total and 1024 configs. Other file types are skipped.
It then saves sequentially, overwriting same-named tunnels with helper backups.
An operational failure during saving can leave a partial restore; it is not a
transaction or a live-interface update. Review and reconnect affected tunnels.

## 8. Temporary kill switch

On an active tunnel, press `K`. The helper sets a runtime fwmark when needed,
then prefers nftables over iptables/ip6tables. It places reject rules before
existing allow rules, with exceptions above the reject for loopback, the tunnel,
its mark, and established SSH return traffic when `SSH_CONNECTION` is available
to the helper. IPv6 protection is checked; the iptables fallback refuses when
the host has non-loopback IPv6 and lacks `ip6tables`.

This changes host output policy and can disrupt remote access or interact with
another firewall manager. It does not configure a server's forwarding/NAT.
Exact tunnel-name comments keep `wg0` and `wg01` separate. After upgrading to
1.9.1, toggle an existing switch off/on to replace older rules.

App deactivation, deletion and rename attempt cleanup. App exit alone does not
remove rules. External `wg-quick`/service changes can leave stale rules, and a
firewall reload can remove them. There is no daemon or persistence service;
reboot normally clears runtime rules unless an external firewall restores them.
Use a separate access path when testing on a remote host.

## 9. Updates and removal

A background startup request checks the latest GitHub release. When an update
is detected, `u` suspends the TUI, verifies and installs the release, then asks
you to restart. It does not force a check when no update was detected. Network
failures are silent. Verification needs `curl`, `tar`, `sha256sum`, and
`minisign`; a best-effort minisign install is attempted if missing, but an
unverifiable update is refused. The installer may ask for privilege.

Disable automatic checking with `WG_NO_UPDATE_CHECK=1` or the
`~/.config/wireguard-tui/no-update` marker (respecting `XDG_CONFIG_HOME`).
For distro packages, prefer the package manager: in-app updates install to the
source installer's `/usr/local` layout and default sudoers mode, with
`--no-desktop`, rather than updating a package database.

To remove a source/prebuilt installation, run its installer with the same
`PREFIX` used originally:

```sh
./install.sh uninstall
```

For a distro package, use that distro's package manager. Configs and user
backups are left in place. For build/debug instructions see
[CONTRIBUTING.md](../CONTRIBUTING.md); for privilege limits see
[SECURITY.md](../SECURITY.md).
