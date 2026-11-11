# Linux distro, DNS, and service guide

This guide describes the current (1.10.x) installer and runtime requirements. Package names
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

On SELinux systems the installer labels and checks the helper; see
[SELinux](#selinux). Standard package helper paths are
`/usr/lib/wireguard-tui/wg-helper`; source installs use
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

## Install records, updates and rollback

`install.sh` records how it installed the app in a root-owned, world-readable
file next to the helper, `PREFIX/lib/wireguard-tui/install-manifest` (by default
`/usr/local/lib/wireguard-tui/install-manifest`). It holds the installation method,
prefix, authorization mode (`sudoers`, `polkit` or `none`), the account the
grant belongs to, whether the desktop entry was installed, the version, and the group/SELinux/directory
changes the installer made so `uninstall` can undo exactly those.

- **Re-running the installer keeps your choices.** Without `--sudoers` or
  `--polkit` a later run (or an update) keeps the recorded authorization mode
  instead of falling back to sudoers. Installs made by earlier releases have no
  record; the installer then detects the grant already on disk.
- **The in-app updater keeps the installation method.** It updates only a copy
  installed by `install.sh`, and passes the recorded prefix and authorization
  mode to the verified release installer. It refuses, before downloading
  anything, when the running copy belongs to a distro package (detected with
  `dpkg-query -S`, `rpm -qf`, `pacman -Qo`, `apk info --who-owns` or
  `xbps-query -o`, or the package's own manifest) and names the package-manager
  command to use instead; AUR packages are pointed at a rebuild after
  `pacman -Syu`. A `cargo install` copy is told to re-run `cargo install`, and a
  copy whose origin is unknown (for example a binary run from a build tree) is
  not touched.
- **No duplicate installs.** If a distro package already provides
  `/usr/bin/wg-tui` or `/usr/lib/wireguard-tui/wg-helper`, `install.sh` refuses to add
  a second copy under `/usr/local` (both would be on `PATH` with separate
  helpers and grants) and names the package to remove. `--force` installs anyway
  and is remembered for later updates. Installing with `PREFIX=/usr` refuses to
  overwrite package-owned files, and warns when a `/usr/local` copy shadows it.
  The AUR, RPM, Alpine and Void packages warn after installation when a
  `/usr/local` copy exists.
- **Failed installs roll back.** Before it changes anything, the installer
  backs up the app, alias, helper, desktop entry, icon, manifest, sudoers
  drop-in and polkit rule. If any later step fails (or the run is interrupted),
  it restores them, removes a `wireguard` group or membership it created,
  removes directories it created and drops a SELinux rule it added; the
  previous installation keeps working. Packages installed as dependencies are
  kept.
- **Local binaries.** `./install.sh --binaries=DIR` installs `wg-tui` and
  `wg-helper` from `DIR` (for example your own build) instead of building or
  using a release bundle.
- **Uninstall.** `./install.sh uninstall` removes the files, grants, the SELinux
  rule and empty directories the installer created, and the `wireguard` group
  or membership it added unless the sibling app still uses the group. Tunnel
  configurations in `/etc/wireguard` are kept.

### Package managers and partial upgrades

The installer installs only packages that are missing, so a repeat run or an
update normally makes no package-manager call. On Arch-family systems it uses
`pacman -S --needed` and never refreshes the databases on its own: refreshing
without upgrading (`pacman -Sy <pkg>`) can leave a rolling system partially
upgraded. If pacman reports missing or stale databases, run `sudo pacman -Syu`
and re-run the installer. On Void it uses `xbps-install -S`; if that fails, run
`sudo xbps-install -Su` first. The in-app updater follows the same rules when it
installs `minisign`.

### SELinux

On SELinux systems (Fedora, RHEL-family, some openSUSE setups) a file written
to `/usr/local/lib` is labelled `lib_t`, and a file moved from `/tmp` keeps a
temporary-file label; sudo or pkexec may then be denied execution of the
helper. After installing, the installer:

1. adds a persistent rule giving the helper an executable label when `semanage`
   is available (`semanage fcontext -a -t bin_t PREFIX/lib/wireguard-tui/wg-helper`),
2. runs `restorecon -F` on every file it installed, and
3. falls back to `chcon -t bin_t` (lost on a full relabel) when `semanage` is
   missing — install `policycoreutils-python-utils` and re-run to make it
   persistent.

It then runs the helper as root once. If execution is denied it stops with a
clear error, restores the previous installation, and points at the audit log:

```sh
sudo ausearch -m avc -ts recent
ls -Z /usr/local/lib/wireguard-tui/wg-helper
sudo semanage fcontext -a -t bin_t /usr/local/lib/wireguard-tui/wg-helper
sudo restorecon -v /usr/local/lib/wireguard-tui/wg-helper
```

The same check reports a helper that cannot run at all, for example a glibc
build on a musl system (Alpine) or a `PREFIX` on a `noexec` mount. Distro
packages install the helper under `/usr/lib/wireguard-tui/`, which the distro policy
labels; uninstall removes the rule the installer added. Do not disable SELinux
to work around a denial.

A helper on a `noexec` mount is also denied with "Permission denied"; the
installer checks the mount options first and names the `noexec` mount point
instead of blaming SELinux.

`tests/selinux-vm.sh --binaries=DIR` checks this on a real system. It boots a
Fedora Cloud Base VM (pinned image URL and SHA-256; KVM when `/dev/kvm` is
usable, TCG otherwise, x86_64 or aarch64) with SELinux enforcing, injects the
checkout and the binaries in `DIR` on an extra disk, and runs `./install.sh
--binaries=DIR` as a normal user, first with the sudoers drop-in and then with
`--polkit`. After each install it asserts that `ls -Z` shows the helper as
`bin_t`, `semanage fcontext -l` has the rule, `restorecon -n -v` has nothing to
change and `ausearch -m avc` has no denials for the helper. It also asserts that
the user can run `sudo -n PREFIX/lib/wireguard-tui/wg-helper list` through the drop-in
alone, or `pkexec` from an active local session in polkit mode. It then checks
that a re-run keeps the label and that `./install.sh uninstall` removes the files
and the fcontext rule. Two failure paths are covered: an install into a `noexec`
mount, and an install onto a fixed-label (`context=`) mount from which SELinux
denies execution. For each it asserts the error message, an AVC record for the
SELinux case, and a complete rollback, including a byte-identical previous
sudoers grant. The `SELinux VM` workflow runs it under KVM on changes to the
installer, packaging or helper, and on release tags. Without KVM the test
falls back to TCG. For example, an aarch64 or x86_64 guest in Docker on an
Apple silicon Mac takes 10 to 15 minutes.

## Integration requirements

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
`firewall-cmd --reload`), or when another rule has been put in front of the
iptables jump. **Mismatch** means rules exist but are stale: the mark
changed, the interface is down, or an older release built them. On an active
tunnel the app
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
