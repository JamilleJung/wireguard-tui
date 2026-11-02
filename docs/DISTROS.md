# Linux distro, DNS, and service guide

This guide describes the 1.9.1 installer and runtime requirements. Package names
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

App deactivation/delete/rename attempts kill-switch cleanup; exiting the app
or stopping the tunnel elsewhere does not guarantee cleanup. Firewall reloads
can also remove the rules. After upgrading, toggle an existing kill switch
off/on to install current rules. Keep an alternate administrative access path
when testing firewall policy over SSH.

Systems without systemd can still use connect/edit/import, but the app's
start-on-boot toggle is unavailable. Without journald, the Log tab explains the
missing journal source. Avoid managing one interface simultaneously with this
app, NetworkManager, and an independent service configuration.
