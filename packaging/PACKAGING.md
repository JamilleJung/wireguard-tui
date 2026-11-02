# Packaging wireguard-tui

The current recipes target **1.9.2**. They build/install `wg-tui` plus the Rust
`wg-helper`. They do not add the `wireguard-tui` alias created by `install.sh`.
Recipe availability is not a claim that a package is already published in an
external repository.

## Installed files and authorization

| Install path | Source/prebuilt installer | Debian `.deb` | AUR/RPM/Alpine/Void recipes |
|---|---|---|---|
| App | `/usr/local/bin/wg-tui` | `/usr/bin/wg-tui` | `/usr/bin/wg-tui` |
| Helper | `/usr/local/lib/wireguard-tui/wg-helper` | `/usr/lib/wireguard-tui/wg-helper` | `/usr/lib/wireguard-tui/wg-helper` |
| Auth | Per-user sudoers by default, optional polkit | `/etc/polkit-1/rules.d/49-wireguard-tui.rules` | `/usr/share/polkit-1/rules.d/49-wireguard-tui.rules` |
| Launcher/icon | Included unless `--no-desktop` | Included | Not installed by current recipes |

The polkit rule recognizes both standard helper paths and grants passwordless
access only to an active **local** session in the `wireguard` group. Current
package recipes and Cargo metadata have **no post-install group or membership
script**. Packaging a rule is not enough to grant passwordless use. An
administrator must choose trusted users and arrange group setup, for example
on systems with shadow utilities:

```sh
sudo groupadd -f wireguard
sudo usermod -aG wireguard "$USER"
# Log out and back in; then check: id -nG
```

Use your distro's equivalent utilities when those commands are unavailable.
Membership gives access to managed private keys and networking operations.
The active-local rule does not cover SSH sessions; document a scoped sudoers
alternative or the required admin authentication path for headless use. The
source/prebuilt installer's `--polkit` path attempts group setup automatically.

## Runtime and build requirements

`wireguard-tools` supplies `wg`/`wg-quick`; the kernel needs usable WireGuard
support. Packages use polkit/pkexec authorization; systemd is optional for
start-on-boot, journald for log entries, and resolvconf integration for `DNS =`.
Native clipboard helpers are optional (OSC 52 fallback). The updater requires
curl, tar, sha256sum and minisign; it may attempt to install minisign if absent.
Do not claim the current recipes declare every optional integration dependency.

Building needs a suitable Rust 2024 toolchain and C linker, without graphical
libraries. Recipes use the checked-in lockfile; inspect it and run the
[contribution checks](../CONTRIBUTING.md) when updating versions.

## Debian / Ubuntu

The signed release workflow builds one **amd64** package with pinned
`cargo-deb` (3.8.0); assets are defined in
[`Cargo.toml`](../Cargo.toml). It includes the terminal launcher/icon, helper,
README, and polkit rule. Runtime dependency metadata includes `wireguard-tools`
and a pkexec/polkit alternative; clipboard helper packages are recommended.
It does not create the `wireguard` group or add users.

For a package built from source:

```sh
cargo build --release --locked
cargo install cargo-deb --version =3.8.0 --locked
cargo deb --no-build --no-strip
```

Release packages use an auditable build and UPX compression before packaging;
see [RELEASES.md](../docs/RELEASES.md) for verification and packed-binary audit
instructions. Install a verified release with the package manager, for example
`sudo apt install ./wireguard-tui_1.9.2-1_amd64.deb`.

## Arch / AUR

[`aur/PKGBUILD`](aur/PKGBUILD) targets x86_64 and aarch64. It fetches the tagged
source, fetches locked dependencies, builds/tests with `--frozen`, and installs
the helper/rule. Replace `sha256sums=('SKIP')` using `makepkg -g` and generate
`.SRCINFO` before publishing. Bump `pkgver`/`pkgrel` deliberately and keep the
checksum aligned with the tagged source. Build with `makepkg -si` from the
recipe directory as an unprivileged packager.

## Fedora / RHEL-family RPM

[`rpm/wireguard-tui.spec`](rpm/wireguard-tui.spec) covers x86_64 and aarch64,
uses locked release builds/tests, and installs the standard `/usr` helper and
polkit rule. It declares Rust/cargo/gcc/pkgconf build requirements and
WireGuard/polkit runtime requirements. `systemd-resolved` is a recommendation.
An inactive resolvectl compatibility shim still needs administrator approval to
enable its backing service (`wg-tui setup` can offer that repair).

Build with the distro's RPM tooling/COPR after preparing the tagged `Source0`
archive and build environment. The recipe is supplied here; independently
verify any external COPR repository before enabling it. Keep `Version`, the
changelog and tag synchronized. Follow the distro's SELinux labeling/policy
requirements for the installed helper.

## Alpine and Void

[`apk/APKBUILD`](apk/APKBUILD) and [`void/template`](void/template) are maintainer
templates for x86_64/aarch64. They install the terminal app/helper and polkit
rule without a graphical stack. Replace Alpine's `sha512sums="SKIP"` and Void's
`checksum=@CHECKSUM@` with verified source checksums and supply real maintainer
metadata before submission. Review distro Rust/dependency availability.

The app's start-on-boot toggle is systemd-only; on OpenRC/runit provide an
appropriate distro service if boot activation is desired. Do not promise a
journal-backed Log view without journald.

## Tarballs, updates and other formats

GitHub releases provide four static musl tarballs: x86_64, i686, aarch64, armv7.
Each has both executable binaries, installer, `packaging/` assets, README and
license. The installer detects this bundle, stages the binaries, skips build
dependencies, and still installs runtime prerequisites/auth. It defaults to
`/usr/local` plus sudoers and a launcher; `--no-desktop` skips the launcher.
A direct root login requires `--allow-root` after verification.

Prefer package-manager upgrades for a distro-managed installation. The in-app
updater invokes that source/prebuilt installer with `--no-desktop`; it does
not update a package database and can create a parallel `/usr/local` install.
There is no supplied Flatpak because system helper/network administration does
not fit this project's current sandbox model. See [SECURITY.md](../SECURITY.md)
for the privilege boundary and [RELEASES.md](../docs/RELEASES.md) for signatures.

## 1.9.2 compatibility and validation

Before privileged writes or helper grants, `PREFIX` must be an absolute ASCII
path containing only letters, digits, `/`, `_`, `.` and `-`, with no empty,
`.` or `..` components or trailing slash. Existing directory ancestors must be
root-owned without group/other write permission; symlinks are refused. A
group-writable `/usr/local` can therefore fail these checks. An administrator
must choose or prepare a protected path; the installer does not take ownership
of caller directories. `--polkit` supports only `/usr` or `/usr/local`, matching
the bundled rule. Custom sudoers prefixes still require matching frontend helper
discovery configuration.
Existing app/helper targets must be root-owned regular files with one link and
no group/other write bits; the installed helper must be executable before an
authorization grant is written.

Installer options are validated; unknown options fail instead of being ignored.
Source builds use the lockfile. Switching sudoers/polkit modes installs and
validates the selected grant before removing the previous grant. Failed sudoers
validation or group enrollment reports failure and keeps prior authorization.
`--polkit` requires pkexec up front; it does not install a rule and then claim
success without the escalation tool. Packages still do not enroll users.

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

Ship the matching helper with the app: release discovery no longer borrows a
helper from the sibling project or an old source build tree. Background reads
need an already authorized sudo/polkit path. The conditional kill-switch
fallback needs iptables-restore and, on IPv6-capable kernels, ip6tables plus
ip6tables-restore; no package recipe should promise a complete firewall setup
from installing the rule alone. User backups require owned private directories
and single-link regular archives; root configs reject symlinks/hardlinks/FIFOs
and group/other write permission. Run the regression and isolated Linux checks
before publishing a new package.
