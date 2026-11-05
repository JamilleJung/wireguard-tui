# Packaging wireguard-tui

The current recipes target **1.10.0**. They build/install `wg-tui` plus the Rust
`wg-helper`. They do not add the `wireguard-tui` alias created by `install.sh`.
Recipe availability is not a claim that a package is already published in an
external repository.

## Installed files and authorization

| Install path | Source/prebuilt installer | Debian `.deb` | AUR/RPM/Alpine/Void recipes |
|---|---|---|---|
| App | `/usr/local/bin/wg-tui` | `/usr/bin/wg-tui` | `/usr/bin/wg-tui` |
| Helper | `/usr/local/lib/wireguard-tui/wg-helper` | `/usr/lib/wireguard-tui/wg-helper` | `/usr/lib/wireguard-tui/wg-helper` |
| Auth | Per-user sudoers by default, optional polkit | `/etc/polkit-1/rules.d/49-wireguard-tui.rules` | `/usr/share/polkit-1/rules.d/49-wireguard-tui.rules` |
| Launcher/icon | Included unless `--no-desktop` | Included | Included |
| Install record | `/usr/local/lib/wireguard-tui/install-manifest` (`method=installer`) | none (detected with `dpkg-query -S`) | `/usr/lib/wireguard-tui/install-manifest` (`method=package`) |

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

`wireguard-tools` supplies `wg`/`wg-quick`, which need `ip` from iproute2; the
kernel needs usable WireGuard support. Packages use polkit/pkexec
authorization. The kill switch needs nftables (preferred) or iptables. The
recipes declare these: Arch depends on iproute2 (which pulls in iptables) and
suggests nftables; Fedora requires `(nftables or iptables-nft or
iptables-legacy)`; Alpine and Void depend on nftables. systemd is optional for
start-on-boot, journald for log entries, and resolvconf integration for `DNS =`
(optional dependency/recommendation). Native clipboard helpers are optional
(OSC 52 fallback). The updater requires curl, tar, sha256sum and minisign; the
installer installs minisign when available and the updater may try once.

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
`sudo apt install ./wireguard-tui_1.10.0-1_amd64.deb`.

## Arch / AUR

[`aur/PKGBUILD`](aur/PKGBUILD) targets x86_64 and aarch64 and is ready for AUR
submission together with the generated [`aur/.SRCINFO`](aur/.SRCINFO) and
[`aur/wireguard-tui.install`](aur/wireguard-tui.install). It depends on glibc,
libgcc, wireguard-tools, iproute2 and polkit, runs `cargo test` in `check()`,
and builds as an unprivileged user with `makepkg -si`. Bump `pkgrel` for
packaging-only changes and regenerate `.SRCINFO` with the script below.

## Fedora / RHEL-family RPM

[`rpm/wireguard-tui.spec`](rpm/wireguard-tui.spec) covers x86_64 and aarch64 for
COPR/mock: `Source0` is the tagged archive and `Source1` the vendored crates
(`packaging/update-checksums.sh --vendor`), so the build needs no network.
`%check` runs the test suite. Runtime requirements are wireguard-tools,
iproute, polkit and nftables or iptables; systemd-resolved is recommended.
On SELinux hosts the packaged helper lives in `/usr/lib/wireguard-tui/` and is
labelled by the distro policy; see [DISTROS.md](../docs/DISTROS.md#selinux).
Keep `Version`, the changelog and tag synchronized (the script does this).

## Alpine and Void

[`apk/APKBUILD`](apk/APKBUILD) follows aports conventions (Contributor and
Maintainer lines, `cargo fetch` in `prepare()`, `cargo auditable build --frozen`,
`check()`, a `-doc` subpackage, post-install/post-upgrade messages) and
[`void/template`](void/template) uses Void's cargo build style, installing from
`target/$RUST_TARGET/release` with its own `do_install` so the helper goes to
`/usr/lib/wireguard-tui/` rather than `/usr/bin`. Both target x86_64/aarch64
and depend on wireguard-tools, iproute2, nftables and polkit.

The app's start-on-boot toggle is systemd-only; on OpenRC/runit provide an
appropriate distro service if boot activation is desired. Do not promise a
journal-backed Log view without journald.

### Recipe maintenance

Run `packaging/update-checksums.sh` after tagging a release. It reads the
version from `Cargo.toml`, syncs `pkgver`/`version`/`Version` in all four
recipes (resetting the package release numbers and adding an RPM changelog
entry on a version change), downloads the tagged source archive (or takes
`--tarball=FILE`), writes its SHA-256 (AUR, Void) and SHA-512 (Alpine)
checksums, and regenerates `aur/.SRCINFO`. `--vendor` also writes the RPM's
offline crate archive (`Source1`) to `target/packaging/`.
`packaging/update-checksums.sh --check` is the offline CI check that versions
and `.SRCINFO` match (`--strict` also rejects placeholder checksums).
`tests/install-packaging-lint.sh` runs `makepkg --printsrcinfo`, `namcap` and
`makepkg --verifysource` (Arch), `rpmspec -P` and `rpmlint` (Fedora),
`apkbuild-lint` and `abuild verify` (Alpine) and `xlint` (Void) in containers;
`--build` also builds the packages.

`tests/package-build.sh arch|rpm|apk|void` builds the real package from the
current checkout, not from a release. It creates `wireguard-tui-VERSION.tar.gz` with the
release archive's name and layout: `git archive` of `HEAD` for a clean tree,
otherwise the tracked and untracked, non-ignored files. It then points temporary
copies of the recipe at that archive, with checksums from `update-checksums.sh
--tarball`; the committed recipes keep their release URLs. The script runs the
distribution's own tool in a container, and each tool also runs the package's
tests:

- `makepkg -s --noconfirm` + `namcap` (`archlinux:latest`, amd64)
- `rpmbuild -ba` with the `Source1` from `update-checksums.sh --vendor` (`fedora:latest`)
- `abuild -r` with a throwaway `abuild-keygen -a -n` key (`alpine:latest`)
- `xbps-src -Q pkg` as an unprivileged user, after `binary-bootstrap`, in a
  void-packages checkout (`void-glibc`, run `--privileged` for `xbps-uchroot`,
  as in void-packages CI)

It then installs the package with the package manager in a clean container and
runs `wg-tui --version` and `tests/helper-validation.sh` against the installed
helper. Packages are written to `target/package-build/FORMAT/`, or to the
directory given with `--out=DIR`. If a build runs out of memory, set
`CARGO_BUILD_JOBS=2`. On a non-x86_64 host the Arch build runs under amd64
emulation. The emulator does not report a failed exec from a spawned child, so
the script skips the tests that depend on that, and logs the skips. The
`Package builds` workflow runs all four formats and
uploads the packages. The helper's process tests spawn `python3`, so every recipe
declares it as a check dependency. The Void template skips the one test that
needs a root-owned `/bin/sh`, because xbps-src builds in a masterdir that the
build user owns. The PKGBUILD depends on `hicolor-icon-theme` for the icon
directory and sets `!debug`, because Cargo already strips the binaries.

Every recipe fetches crates only in its prepare step (`cargo fetch --locked`),
then builds and tests offline with `--frozen`; the RPM builds from the vendored
`Source1` archive with no network at all. Each package installs the license,
README, helper under `/usr/lib/wireguard-tui/`, polkit rule, desktop entry and icon,
plus `/usr/lib/wireguard-tui/install-manifest` with `method=package` so the in-app
updater refuses to replace package-owned files and names the package-manager
command instead. AUR (`wireguard-tui.install`), RPM (`%post`), Alpine (`.post-install`)
and Void (`INSTALL`) warn when an `install.sh` copy under `/usr/local` shadows
the package and print the `wireguard` group setup the polkit rule needs.

## Tarballs, updates and other formats

GitHub releases provide four static musl tarballs: x86_64, i686, aarch64, armv7.
Each has both executable binaries, installer, `packaging/` assets, README and
license. The installer detects this bundle, stages the binaries, skips build
dependencies, and still installs runtime prerequisites/auth. It defaults to
`/usr/local` plus sudoers and a launcher; `--no-desktop` skips the launcher.
A direct root login requires `--allow-root` after verification.

Distro-managed installations update through their package manager: the in-app
updater detects package ownership and refuses with the matching command. For
`install.sh` installations it re-runs the verified release installer with the
prefix, authorization mode and desktop choice recorded at install time; see
[DISTROS.md](../docs/DISTROS.md#install-records-updates-and-rollback).
There is no supplied Flatpak because system helper/network administration does
not fit this project's current sandbox model. See [SECURITY.md](../SECURITY.md)
for the privilege boundary and [RELEASES.md](../docs/RELEASES.md) for signatures.

## 1.9.3 compatibility and validation

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

Ship the matching helper with the app: release discovery no longer borrows a
helper from the sibling project or an old source build tree. Background reads
need an already authorized sudo/polkit path. The conditional kill-switch
fallback needs iptables-restore and, on IPv6-capable kernels, ip6tables plus
ip6tables-restore; no package recipe should promise a complete firewall setup
from installing the rule alone. User backups require owned private directories
and single-link regular archives; root configs reject symlinks/hardlinks/FIFOs
and group/other write permission. Run the regression and isolated Linux checks
before publishing a new package.
