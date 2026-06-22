# 📦 Packaging wireguard-tui

Distro packages so users don't have to run `install.sh` by hand. Each installs:

- the `wg-tui` binary → `/usr/bin`
- the privileged helper → `/usr/lib/wireguard-tui/wg-helper`
- the **polkit** rule → `/usr/share/polkit-1/rules.d/49-wireguard-tui.rules`

`wireguard-tools` is the only runtime dependency; `polkit` provides the
privilege; `systemd` is *optional* (only for start-on-boot). The app itself is
pure Rust with **no GUI/C library dependencies**.

The desktop launcher/icon files remain in `packaging/` for downstreams that want
them, but the first-party packages keep the TUI install minimal by default.

## Arch (AUR) - `aur/PKGBUILD`

```sh
cd packaging/aur && makepkg -si
```

To publish: `makepkg -g` for real `sha256sums`, then push `PKGBUILD` + `.SRCINFO`
to `ssh://aur@aur.archlinux.org/wireguard-tui.git`. Bump `pkgver` per release.
(The PKGBUILD runs `cargo test` in `check()`.)

## Fedora / RHEL / Rocky (COPR) - `rpm/wireguard-tui.spec`

Build on **COPR** (SCM build against the repo + tagged `Source0` tarball), or
locally with `rpmbuild -ba`. Runs `cargo test` in `%check`. Bump `Version` per
release. The TUI needs no GUI libraries — only `cargo`, `rust`, `gcc`, and
`pkgconf-pkg-config` at build time, plus `wireguard-tools` and `polkit` at
runtime. `ExclusiveArch` covers `x86_64 aarch64` (static binaries ship for
both).

## Debian / Ubuntu (.deb)

Produced by the release workflow via `cargo deb` (`[package.metadata.deb]`).

## Alpine - `apk/APKBUILD`

Template for Alpine maintainers. It builds the single TUI binary plus the Rust
`wg-helper`, installs the polkit rule, and keeps runtime dependencies to
`wireguard-tools` and `polkit`. Replace `sha512sums="SKIP"` with the real
release tarball checksum before submitting to an Alpine repository.

## Void Linux - `void/template`

Template for Void maintainers. It keeps the package terminal-only: `wg-tui`,
`wg-helper`, the polkit rule, and no GUI libraries. Replace
`checksum=@CHECKSUM@` with the real release tarball checksum before submitting.

## Flatpak

Not provided: a terminal tool that manages system WireGuard as root (via
sudoers/polkit, writing `/etc/wireguard`) does not fit the Flatpak sandbox. Use
the native install, the AUR package, the RPM/COPR build, Alpine/Void templates,
or the `.deb`.
