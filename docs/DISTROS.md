# Linux distro, DNS, and service guide

This guide describes the 1.9.3 installer and runtime requirements. Package names
and system policy can vary by distro release; check the package manager's output
and `wg-tui doctor` on the actual host.

## Client or server/gateway?

A client dialing a VPN provider needs the app/helper, `wg`/`wg-quick`, kernel
WireGuard support, and a working resolvconf integration if the config has
`DNS =`. It usually does not need an inbound listen-port exception or IP
forwarding, but local policies must still permit the required outbound traffic.

A server accepting peers may need an inbound UDP rule for its ListenPort. A
host forwarding peers to a LAN or the internet additionally needs appropriate
forwarding, routing and, when required, NAT. A host accepting access only to
itself does not automatically need forwarding or NAT. The app's temporary kill
switch is a host-output policy, not a server forwarding/NAT wizard.

The app does not bundle kernel modules. `wg-quick` relies on your kernel's
WireGuard implementation (or an administrator-configured supported alternative).
`doctor` checks user-space tools and prerequisites; it does **not** report the
kernel version or verify that a module can load. Containers also depend on
host kernel support and privileges.

## What the installer and setup do

`./install.sh` detects a package manager and installs tools/runtime packages,
source-build prerequisites when needed, app/helper files, and a default
per-user sudoers grant. It attempts resolver and clipboard-helper installation.
A terminal launcher/icon is included unless `--no-desktop` is used.

A verified release bundle has adjacent app/helper binaries and no Cargo
manifest: the installer stages both, skips Rust/compiler setup, and still
installs runtime prerequisites and authorization. A source checkout/archive
requires a Rust build. Run as a normal user; a direct root login requires
`--allow-root`, which can permit building source as root.

`wg-tui setup` offers confirmed tools/resolver package installation and can
enable an inactive systemd-resolved shim. It does not install the helper,
create `/etc/wireguard`, connect a tunnel, or enable tunnel start-on-boot.
The helper creates `/etc/wireguard` with mode `0700` on the first save.
Manual activation uses the configured tunnel, so existing administrator-managed
wg-quick hooks may still perform their own privileged actions.

## Package-manager reference

The commands below install WireGuard tools as an administrator. The resolver
column reflects the installer's package choices when `resolvconf` is absent;
it is not a statement that a given desktop already has DNS configured.

| Family | WireGuard tools | Resolver package path | Service notes |
|---|---|---|---|
| Debian/Ubuntu/Mint | `apt-get install wireguard-tools` | `openresolv`, or `systemd-resolvconf` if systemd-resolved is active | systemd on typical installations |
| Fedora/RHEL/Rocky/Alma | `dnf install wireguard-tools` (or `yum`) | `systemd-resolved` | Check its service when using the resolvectl shim |
| Arch/Manjaro | `pacman -S wireguard-tools` | `openresolv` | Existing systemd-resolved setups need the compatibility command |
| openSUSE | `zypper install wireguard-tools` | `openresolv` | Check the host's resolver arrangement |
| Alpine | `apk add wireguard-tools` | `openresolv` | Commonly OpenRC; app boot toggle unavailable without systemd |
| Void | `xbps-install -S wireguard-tools` | `openresolv` | Commonly runit; use its service manager for boot activation |
| Solus | `eopkg install wireguard-tools` | `openresolv` | Check the host's resolver arrangement |

The source build also needs Rust and a C toolchain/linker. The installer uses
`build-essential` on apt systems, `base-devel` for pacman/Void, `build-base` for
Alpine, or gcc elsewhere. It does not need a graphical toolkit. Native clipboard
helpers are optional; the installer attempts `wl-clipboard` or `xclip`, with
OSC 52 as the app's fallback. Updates additionally need curl, tar, sha256sum,
and minisign verification.

Distro recipe files are starting points, not proof that an AUR/COPR/Alpine/Void
package is published. See [packaging](../packaging/PACKAGING.md).

## Authorization on local desktops and SSH hosts

The source/prebuilt installer defaults to a sudoers rule limited to the helper
for the invoking user. It can fall back to a one-time root-password `su`
re-exec if sudo is unusable. A root login without an identifiable normal user
has a deliberate `--allow-root` guard.

`--polkit` instead installs a rule requiring both an active local session and
membership in `wireguard`. The installer attempts group setup; verify membership
after logout/login. The `.deb` and other recipes install only the rule, so an
administrator must create/manage the group separately. These grants allow
reading all managed private keys as well as changing network state; membership
must be limited to trusted operators.

An SSH session does not satisfy the active-local polkit condition. Use the
scoped sudoers installation or an administrator-approved path; pkexec may prompt
or fail when no authentication agent is available. `doctor` reports available
authorization but cannot guarantee every future privileged action will succeed.

On SELinux systems, use the distro's normal policy/label management if a helper
is denied, and inspect the audit log rather than disabling SELinux. Standard
package helper paths are `/usr/lib/wireguard-tui/wg-helper`; source installs use
`/usr/local/lib/wireguard-tui/wg-helper`.

## DNS: a service alone is not enough

For a config with `DNS =`, wg-quick calls `resolvconf` by name:

| Detected integration | Required action |
|---|---|
| Native resolvconf command | Check its provider if DNS still fails |
| resolvconf points to resolvectl; systemd-resolved active | Compatibility path is available |
| resolvconf points to resolvectl; service inactive | Enable the backing service deliberately |
| No resolvconf command | Install a compatible provider |

Typical manual package commands (run only for the appropriate host) are:

```sh
sudo apt-get install openresolv       # Debian/Ubuntu native-provider option
sudo pacman -S openresolv             # Arch native-provider option
sudo dnf install systemd-resolved     # RHEL-family compatibility shim
```

An installed but inactive resolvectl shim can be repaired with:

```sh
sudo systemctl enable --now systemd-resolved
```

Enabling/replacing a resolver changes host DNS behavior; review your existing
resolver setup. The installer warns about an inactive shim but does not enable
a network service silently. `wg-tui doctor` checks the command and backing
service; `setup` offers the repair after confirmation. Merely having
systemd-resolved active does not supply a missing `resolvconf` executable.

## Server policy and persistence

Manage inbound UDP, forwarding and NAT with the distro's supported firewall and
service manager. Use the actual ListenPort, WAN interface, peer subnets and
routing policy. Do not import generic NAT `PostUp`/`PostDown` snippets expecting
the helper to accept them: it refuses all four root script-hook directives on
save/rename. Either manage firewall policy separately, or have a trusted
administrator maintain the config directly as root.

| State | Reboot behavior |
|---|---|
| Installed tools, helper/auth rules, config and user backup files | Remain on disk |
| Connected interface | Needs systemd start-on-boot or another configured service |
| `s` in the app | Enables/disables `wg-quick@<name>`; does not connect immediately |
| Runtime kill switch | No app persistence; normally lost at reboot, may be restored by another firewall |
| Forwarding/NAT/firewall policy | Depends on the administrator's persistent service configuration |

App deactivation/delete/rename retains protection on failed/unknown stop; exiting the app
or stopping the tunnel elsewhere does not guarantee cleanup. Firewall reloads
can also remove the rules. After upgrading, toggle an existing kill switch
off/on to install current rules. Keep an alternate administrative access path
when testing firewall policy over SSH.

Systems without systemd can still use connect/edit/import, but the app's
start-on-boot toggle is unavailable. Without journald, the Log tab explains the
missing journal source. Avoid managing one interface simultaneously with this
app, NetworkManager, and an independent service configuration.

## 1.9.3 integration requirements

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

Only **one helper-managed kill switch** can be active at a time across managed
tunnels. Turn the existing one off before enabling another. The helper requires
an already-active interface; it can assign an unused runtime WireGuard fwmark
without changing the saved config. It does not activate a stopped tunnel as a
side effect of enabling a switch.

The helper prefers nftables and requires a real hooked filter OUTPUT base chain.
It applies its complete rule set as one nft batch. The fallback requires
`iptables` and `iptables-restore`; when the kernel exposes IPv6 support it also
requires `ip6tables` and `ip6tables-restore`, even before a non-loopback IPv6
address is assigned. Both families are preflighted; commits are atomic **per
family**, not one transaction across IPv4 and IPv6. Failures are reported.

The rules allow loopback, the selected tunnel, its marked transport traffic,
and detected established SSH return traffic when `SSH_CONNECTION` reaches the
helper, then reject other host output ahead of existing allows. Exact comments
isolate tunnel names; status requires the complete expected rule set. This is
host-output control, not server forwarding/NAT or a general no-leak guarantee.
It may disrupt remote access or interact with another firewall manager.

Down/delete/rename retain protection if the interface is still active or its
state is unknown, and report stop/cleanup failures. A failed operation can
leave a tunnel stopped or partially changed; inspect the error and current
state before retrying. App exit does not clear rules. External service changes
may leave stale rules, and firewall reloads may remove them. No persistence
daemon is installed. After upgrading, reapply an existing switch (off/on) to
install current rules, planning for that brief unprotected interval and keeping
another administrative access path available.
