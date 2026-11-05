# wireguard-tui tutorial

This guide describes **1.9.3**. `wg-tui` manages plain Linux WireGuard configs
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

Imported config files and external-editor output must be regular files at most
1 MiB. Named pipes and other special files are rejected without waiting for
a writer.

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
and removed after editing. Editor arguments support quoted words but do not expand variables or run a
shell. Use a wrapper script for shell features. The editor draft uses a fresh
private directory; cleanup removes its contents, including swap/backup files,
and returning to the TUI reuses the existing terminal state.

`g` displays a new keypair and preshared key. That popup includes secret keys;
keep the screen private. The TUI uses raw text editing rather than the desktop
client's form editor.

Editing a running tunnel saves to disk then attempts `wg syncconf` for
WireGuard-level changes. Changes to Address, DNS, MTU, or routing still need a
reconnect, even when the app says "applied live". Quick-add peer uses the same save and live-sync path. `c` shows running config; `p` runs `wg-quick save`, replacing
the saved state after a backup.

Config validation is shared by the frontend and privileged helper. It checks
every directive occurrence, known sections/field placement, canonical 32-byte
WireGuard base64 keys (including padding bits), numeric CIDR/port/keepalive
bounds, and required peer fields. Inline comments, repeated Interface sections,
repeated Address/DNS/AllowedIPs, scoped IPv6 endpoints, named/numeric Table,
FwMark hex/off and SaveConfig booleans are supported. Unknown fields or malformed
sections stay visible in raw text but cannot be saved until corrected. If a
PresharedKey directive is present it must contain a valid key; omit it to use
no PSK. The helper independently rejects all four root script-hook directives.

Helper-backed reads/mutations, imports, backups and key generation run on
serialized workers. Navigation does not let a completed operation replace a
newer draft/selection; conflicting mutations are rejected while busy. The
external editor and update installer intentionally take the foreground. Local
browsing/log writes and clipboard attempts can still briefly wait. Exiting the
app does not cancel or roll back a network operation already started.

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
the system journal. Follow mode copies the latest displayed line; visual
selection freezes the displayed log snapshot until selection ends, so new
background entries cannot shift the copied range. `Esc` cancels a visual
selection before it quits the application. Log-tab keys override their tunnel
actions: `g` goes to the top, `c` clears, `w` saves, and `Enter` copies.

## 7. Backup manager

Press `B` from either tab. `n` creates a ZIP of all tunnels; `Enter`/`r` restores,
`d` deletes, and both destructive actions ask for confirmation. `x` copies the
selected archive to your home directory; `Esc` closes the manager. Actions run
on workers and return to the main view; press `B` to reopen it.

User backups live under `$XDG_DATA_HOME/wireguard/backups` (an absolute XDG
path), or `~/.local/share/wireguard/backups`, shared by both applications.
Existing managed backup directories must belong to the current user and are
checked/tightened to `0700`; symlink directories are refused. ZIPs and exported
copies use `0600`. New backup names are unique even within one second and are
published without overwriting another archive.

Export reads every listed tunnel successfully or reports an error. It no longer
reports a successful incomplete archive after a helper/read failure. Exports
and saved logs are written to a completed private sibling file and then
published atomically: failed writes preserve the old destination, final
symlinks are refused, and replacing a hardlink does not modify its other names.

Restore accepts only valid, unique, top-level `.conf` entries. It rejects
malformed/duplicate archive names, invalid configs and hooks before its first
save. Limits are **64 MiB per ZIP**, **4,096 archive entries**, **1,024 configs**,
**1 MiB per config**, and **16 MiB total config text**. Unrelated files are
skipped. Backup reads require an owned regular file with one hard link and
refuse symlinks/FIFOs; copy a hardlinked archive to an ordinary file first.

After complete preflight, restore copies every saved config it is about to
overwrite, then saves sequentially. It overwrites matching saved names, leaves
other tunnels in place, and does not sync running interfaces. If a later save
fails (or times out), restore **rolls back**: overwritten tunnels get their
previous contents back and tunnels the restore created are removed again. The
report names every tunnel restored, removed or, if a rollback step itself
failed, left changed. Helper per-file backups remain a separate recovery
source under `/etc/wireguard/.backup/`. All these archives contain unencrypted
private keys.

## 8. Temporary kill switch

Only **one helper-managed kill switch** can be active at a time across managed
tunnels. Turn the existing one off before enabling another. The helper requires
an already-active interface; it can assign an unused runtime WireGuard fwmark
without changing the saved config. It does not activate a stopped tunnel as a
side effect of enabling a switch.

The helper prefers nftables. It keeps the kill switch in its **own table**,
`inet wg_helper_killswitch`, with its own OUTPUT base chain, and replaces that
table in one nft transaction. It never edits another table, so firewalld, a
distro ruleset or your own rules stay untouched. The fallback uses a dedicated
`WGH-KILLSWITCH` chain reached by one jump at the top of `OUTPUT`. It requires
`iptables` and `iptables-restore`; when the kernel exposes IPv6 support it also
requires `ip6tables` and `ip6tables-restore`. Both families are preflighted;
commits are atomic **per family**, not one transaction across IPv4 and IPv6.

The rules allow loopback, packets routed into the selected tunnel, its
transport traffic (identified by the tunnel's runtime fwmark), IPv6
router/neighbor solicitation and neighbor advertisement (hop limit 255) and
MLDv2 reports, DHCP client traffic, and established SSH return traffic when
`SSH_CONNECTION` reaches the helper; everything else leaving the host is
rejected. Without the Neighbor Discovery exception an IPv6 endpoint becomes
unreachable once its neighbor cache entry expires. The exceptions **never
override your deny rules**: nftables `accept` only ends the kill switch's own
chain, and the iptables chain `RETURN`s allowed traffic to `OUTPUT` instead of
accepting it, so later rules still apply. This is host-output control, not
server forwarding/NAT or a general no-leak guarantee. It may disrupt remote
access.

Status is one of `enabled`, `disabled`, `missing` or `mismatch`. **Enabled**
requires complete rules whose mark equals the interface's current fwmark.
**Missing** means the switch was turned on but its rules are gone or
incomplete, for example after a firewall reload (`nft flush ruleset`,
`firewall-cmd --reload`). **Mismatch** means rules exist but are stale: the
mark changed, the interface is down, an older release built them, or the
iptables jump is no longer first in `OUTPUT`. On an active tunnel the app
repairs both automatically (`wg-helper killswitch-repair`), once per loss and
with backoff, and reports the result.

Editing a connected tunnel keeps the kill switch working. `wg syncconf` would
reset the fwmark that wg-quick set at runtime; the helper carries the running
fwmark and listen port into the synced config, refuses a config that changes
`FwMark` on a running tunnel (reconnect to apply it), verifies the mark
afterwards and repairs the kill switch if needed.

Down/delete/rename retain protection if the interface is still active or its
state is unknown, and report stop/cleanup failures. If the interface
disappears without a disconnect, the rules stay and keep blocking traffic
(status `mismatch`) until you disconnect. App exit does not clear rules. The
switch is not persistent: the rules and the record that it was requested live
in the kernel and under `/run`, so both are gone after a reboot. A switch built
by an earlier release (1.9.3 or older) is reported as `mismatch` and rebuilt in the
current layout on the next refresh.

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

Release helper discovery uses this project's own standard `/usr/local/lib/`
and `/usr/lib/` paths, or an adjacent **trusted** executable. The executable must
be root-owned, executable and not group/other writable; its directory ancestry
must also be trusted. A normal user-owned extracted helper is not automatically
trusted: install the verified bundle first. Release builds no longer fall back
to the source build directory or another application's helper. Debug overrides
remain available for development; they do not create an authorization grant.

Background read-only requests use a noninteractive polkit check before pkexec,
so missing authorization produces an actionable error instead of repeated
password dialogs. Explicit mutating actions can still request authentication.
Helper IPC drains input/output concurrently, retains at most 4 MiB per output
stream, and has an overall 90-second deadline plus bounded pipe cleanup. Local
key-generation commands have a 10-second deadline. A privileged process that
the UI user cannot signal may finish independently after a UI-side timeout;
check actual tunnel state before retrying.

Before privileged writes or helper grants, `PREFIX` must be an absolute ASCII
path containing only letters, digits, `/`, `_`, `.` and `-`, with no empty,
`.` or `..` components or trailing slash. Existing directory ancestors must be
root-owned without group/other write permission; symlinks are refused. A
group-writable `/usr/local` can therefore fail these checks. An administrator
must choose or prepare a protected path; the installer does not take ownership
of caller directories. `--polkit` supports only `/usr` or `/usr/local`, matching
the bundled rule. Custom sudoers prefixes still require matching frontend helper
discovery configuration.

Installer options are validated; unknown options fail instead of being ignored.
Source builds use the lockfile. Switching sudoers/polkit modes installs and
validates the selected grant before removing the previous grant. Failed sudoers
validation or group enrollment reports failure and keeps prior authorization.
`--polkit` requires pkexec up front; it does not install a rule and then claim
success without the escalation tool. Packages still do not enroll users.

Repeating `--polkit` checks the existing `wireguard` group and account
membership before invoking group-management tools. An already configured
Alpine/BusyBox system can reinstall without optional Shadow `groupadd`/`usermod`.
Missing groups or membership must still be created and verified before the
previous authorization is removed.

Verified updates retain ownership of their private download directory through
installation and clean it up on download, staging or installation failure and
normal completion. Before privilege escalation, the whole required bundle is
checked: regular single-link application/helper/installer/assets, no symlink
components, and a fresh exclusive staging tree. Missing/unsafe assets fail before
installation starts. HTTPS-only redirects are enforced. TUI automatic updates select its
published x86_64/i686/aarch64/armv7 builds; unsupported CPUs fail explicitly.

The download step refuses a release that is no longer newer; success reports
the version actually verified. Restart to use the installed version.

To remove a source/prebuilt installation, run its installer with the same
`PREFIX` used originally:

```sh
./install.sh uninstall
```

For a distro package, use that distro's package manager. Configs and user
backups are left in place. For build/debug instructions see
[CONTRIBUTING.md](../CONTRIBUTING.md); for privilege limits see
[SECURITY.md](../SECURITY.md).
