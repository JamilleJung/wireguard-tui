# Releases

This page is the friendly tour of wireguard-tui releases: where to download
them, how to be sure a download is genuine, how each build is produced, and what
changed in every version. The terminal app `wg-tui` and its desktop sibling
`wireguard-gui` are developed together and share a version number, so a `1.5.4`
of one matches `1.5.4` of the other feature-for-feature.

## Where to get releases

Every release lives on GitHub:

- Releases list: <https://github.com/JamilleJung/wireguard-tui/releases>
- Latest release: <https://github.com/JamilleJung/wireguard-tui/releases/latest>

Most people never download a tarball by hand - the one-command installer clones
the repo and builds from source:

```sh
git clone https://github.com/JamilleJung/wireguard-tui.git
cd wireguard-tui
./install.sh
```

The installer detects your package manager (apt, dnf/yum, pacman, zypper, apk,
xbps, eopkg), installs `wireguard-tools` plus a C linker, makes sure a Rust
toolchain is present (via rustup if needed), builds the `wg-tui` binary as your
own user, and installs the binary and privileged helper. By default it does not
install a desktop launcher; `./install.sh --desktop` adds one if you want it.
By default it sets up a passwordless `sudoers` drop-in scoped to the helper;
`./install.sh --polkit` writes a polkit rule instead, and `./install.sh uninstall`
removes the installed files.

> Note: `--polkit`, `--desktop`, and `uninstall` are flags for the **installer** (`install.sh`),
> not for `wg-tui`. The `wg-tui` binary itself only accepts `wg-tui`,
> `wg-tui doctor`, `wg-tui setup`, `wg-tui --version` and `wg-tui --help`.

If you prefer a downloaded artifact, each release also ships build outputs (such
as a `.deb`) together with the signing files described next.

## How artifacts are signed and how to verify them

Every release is signed with [minisign](https://jedisct1.github.io/minisign/), a
tiny, modern signature tool. Two extra files travel with each release:

- `SHA256SUMS` - the SHA-256 checksum of every release artifact.
- `SHA256SUMS.minisig` - a minisign signature over that `SHA256SUMS` file.
- `minisign.pub` - the public key used to verify the signature.

Verifying is a two-step check: first prove the checksum file is authentic
(minisign), then prove your downloaded file matches the checksum (`sha256sum`).

```sh
# 1. Verify the checksum list was signed by the project key.
minisign -Vm SHA256SUMS -p minisign.pub

# 2. Verify your downloaded artifact matches its recorded checksum.
sha256sum -c SHA256SUMS
```

If step 1 reports a good signature and step 2 prints `OK` for your file, the
download is genuine and untampered. If either step fails, do not install the file.

## How releases are built

### Continuous integration (every push and pull request)

CI guards quality before anything is tagged. On each change it runs:

- `cargo fmt --all -- --check` - formatting must be clean.
- `cargo clippy --all-targets -- -D warnings` - lints are treated as errors, so
  no warning slips through.
- `cargo test` - the unit tests for config parsing, validation and tunnel-name
  sanitisation run on every push and pull request.
- `cargo build --release` - the project must compile in release mode.
- A smoke test: `wg-tui --version` and `wg-tui --help` start and exit cleanly
  without launching the full-screen UI, and `wg-tui doctor` prints its checklist
  and exits with a valid code (0, 1 or 2).
- A shell lint: `bash -n` syntax checks plus `shellcheck -S warning` on the
  scripts (`wg-helper` and `install.sh`) - it hard-fails on any warning, because
  those scripts run with elevated privilege.
- Negative helper tests prove traversal-style tunnel names are rejected before
  any filesystem access.

### The tagged release flow

Cutting a release is triggered by pushing a version tag (for example `v1.5.4`).
At a high level the release job:

1. Runs the CLI smoke checks and helper shell validation.
2. Builds the release artifacts (including the mandatory `.deb` - a missing one
   fails the release).
3. Generates `SHA256SUMS` over the artifacts (using `nullglob` and verifying the
   list is non-empty, so an empty or partial set cannot ship).
4. Signs `SHA256SUMS` with minisign, producing `SHA256SUMS.minisig`, and publishes
   `minisign.pub` alongside.
5. Attaches everything to the GitHub Release for that tag.

Because `wg-tui` and `wireguard-gui` are version-aligned, a release of one is cut
in step with the other so the two stay feature-identical.

## Version history

Newest first.

### 1.5.4 - Installer finds `sbin` tools after the root re-exec

Highlights:

- The installer now puts `sbin` directories on `PATH` when it re-runs as root, so
  it reliably finds `visudo` and the `resolvconf` probe. Previously a normal
  user's `PATH` (which `su` carries into the as-root step) usually omitted
  `/usr/sbin` and `/sbin`, which caused two silent failures: the passwordless
  sudoers drop-in was skipped ("sudoers validation failed"), and an already
  installed `openresolv` was misreported as missing.
- Clearer hint when the sudoers drop-in is skipped - re-run with
  `./install.sh --polkit` to use a polkit rule instead.

Upgrade notes: if a previous install on a minimal box left you without
passwordless authorization, just re-run `./install.sh` (or `./install.sh --polkit`)
to get the drop-in or polkit rule written correctly.

### 1.5.3 - Install `sudo` and set up passwordless auth when sudo is unusable

Highlights:

- "No usable sudo? Install sudo and set it up." When you cannot use sudo (for
  example a Debian server where your login user is not in the sudoers file), the
  installer re-runs itself as root with a **single** ROOT-password prompt instead
  of one prompt per step, installs `sudo` if it is missing, and writes a
  passwordless drop-in scoped to the helper for your user. The result: `wg-tui`
  works as a normal user, with no root and no password prompt, even though you are
  not in the `sudo` group.
- Clear, actionable error when the app cannot gain root (no passwordless sudo and
  no `pkexec`), instead of a cryptic `spawn failed: No such file or directory`.
- The installer now fails loudly if it cannot set up any privilege path, rather
  than reporting success and leaving a non-working install. Transient
  `apt-get update` hiccups no longer abort the run.

Security hardening in this release:

- The sudoers rule no longer trusts `$USER`. The target user is taken from the
  real uid (`id -un`), validated against a strict username pattern, and confirmed
  to exist, so a crafted `$USER` cannot inject a wider sudoers spec (something
  `visudo -cf` alone does not catch).
- The build no longer runs as root. `cargo`/`rustup` run as the invoking user via
  `runuser`, so dependency build scripts never execute with root privilege and the
  toolchain and artifacts stay in that user's home.

### 1.5.2 - Install works when `sudo` is present but you are not a sudoer

Highlights:

- The installer and `wg-tui setup` no longer assume that having the `sudo` binary
  means you can use it. On a Debian server where the login user is not in the
  sudoers file (`<user> is not in the sudoers file`), the install used to abort.
  It now probes real sudo usability (`sudo -n` / admin-group membership), falls
  back to `su` (the ROOT password), and auto-switches the helper authorization to
  a polkit rule.

Upgrade notes: tip - run `su -` first, then `./install.sh`, to be prompted for the
root password once rather than per step.

### 1.5.1 - DNS/resolvconf check and a per-distro guide

Highlights:

- `wg-tui doctor` now reports whether a resolvconf provider is available for
  tunnels that use a `DNS =` line, and `wg-tui setup` offers to install one
  (`openresolv`; an active systemd-resolved also counts). This is the fix for the
  classic minimal-Debian failure where such tunnels died with
  `resolvconf: command not found`.
- A new per-distro guide, [docs/DISTROS.md](DISTROS.md): what to install, what has
  to be set up, what survives a reboot, and when - only as a server or gateway -
  you need firewall and IP-forwarding changes.
- Activation failures are explained in plain language (for example a missing
  resolvconf provider) instead of the raw `wg-quick` output.
- The installer works without `sudo`: it falls back to `su` (the ROOT password) on
  Debian-minimal where `sudo` is absent, auto-switches the helper authorization
  from a sudoers drop-in to a polkit rule, and best-effort installs `openresolv`
  when neither it nor systemd-resolved is present.

### 1.5.0 - First-run doctor/setup and a friendly empty state

Highlights:

- New `wg-tui doctor` subcommand: a read-only, plain-language system checklist
  covering WireGuard tools, the privileged helper and its authorization,
  `/etc/wireguard`, systemd and journald. It exits `0` (all OK), `1` (warnings) or
  `2` (something critical is missing) - useful to run first over SSH on a new box.
- New `wg-tui setup` subcommand: a guided, confirmation-based fix that offers to
  install `wireguard-tools` via your package manager and points you at the
  installer for the helper. It never connects tunnels or enables start-on-boot.
- A friendly first-run hint inside the app when something critical is missing, and
  a beginner empty state ("Import a .conf or QR image to begin") that adapts to
  Easy/Advanced mode.

Note: the app still does not bundle WireGuard kernel modules or `wg`/`wg-quick` -
it uses your system's `wireguard-tools` and helps you install them.

### 1.4.1 - Distro packaging

Highlights:

- Distro packaging arrives: an AUR `PKGBUILD` (Arch) and an RPM spec for COPR
  (Fedora/RHEL/Rocky), plus `packaging/PACKAGING.md`.
- Documentation switched to plain ASCII hyphens instead of em dashes.

### 1.4.0 - Live throughput, connection health, and CLI hygiene

Highlights:

- The detail view shows real-time down/up speed for the selected tunnel, plus a
  Connection health line derived from the most recent handshake
  (OK / stale / waiting).
- `wg-tui --version` and `wg-tui --help` now print and exit cleanly instead of
  launching the full-screen UI.
- The privileged helper bounds every `wg`/`wg-quick` call with a timeout, so a
  hang (DNS, a stuck `PostUp`, a wedged interface) can no longer lock up the app.
- CI gained a smoke test confirming `--version`/`--help` start and exit cleanly.

Upgrade notes: the old demo mode (`WGTUI_DEMO`) was removed - the app always talks
to real tunnels now.

### 1.3.5 - Copy the interface public key

Highlights:

- Press `y` to copy the selected tunnel's interface public key to the clipboard,
  using the OSC 52 terminal escape so it works both locally and over SSH in
  terminals that support it. This closed the last copy-to-clipboard parity gap
  with the desktop client.

### 1.3.4 - Version realignment, Easy mode, and a shared icon

> The version was realigned with the desktop client (`wireguard-gui`) so the
> family shares one number - this is the release after 1.1.2.

Highlights:

- Easy mode (now the default for new users) shows only the everyday actions -
  Connect/Disconnect, Import, Start-on-boot, Remove and Show-QR - so it is
  approachable for non-technical users. Press `m` for Advanced mode (edit, new,
  generate keys, running config, save-live, rename, export). Your choice is
  remembered in `~/.config/wireguard-tui/mode`.
- Bulk import: in the import browser, Space marks files and Enter imports all
  marked ones at once (matching the desktop client).
- A shared desktop icon (the same one the GUI uses) is installed and used by the
  launcher.

Also fixed: terminal rendering corruption on some fonts/locales. The UI no longer
uses ambiguous or wide Unicode glyphs (arrows, the filled/empty status dots,
ellipsis, dingbats) that some terminals render double-width or as tofu and
garbled the layout - it is now ASCII-only, aside from box-drawing borders. The
active/inactive markers in the list are a plain green `*` and a dim `-`.

### 1.1.2 - Release pipeline hardened to match the desktop client

Highlights:

- The `.deb` is now mandatory: a missing one fails the release.
- `SHA256SUMS` is generated with `nullglob` and verified non-empty, and it is
  signed with minisign - `SHA256SUMS.minisig` and `minisign.pub` now ship with
  every release.
- The privileged helper is also discovered next to the binary, so an extracted
  release tarball works without first running `install.sh`.
- CI now hard-fails on `shellcheck` warnings for `wg-helper` and `install.sh`.

### 1.1.1 - IPv6 endpoint fix and helper safety

Highlights:

- A bracketed IPv6 `Endpoint` (for example `[2001:db8::1]:51820`) is accepted by
  config validation again. The stricter endpoint check added in 1.1.0 wrongly
  rejected it, which blocked saving and importing IPv6-endpoint tunnels. A
  regression test now covers it.
- The helper's `sync` verifies that `wg-quick strip` succeeded before applying it
  to the live interface. A strip failure previously fed an empty config to
  `wg syncconf` and could wipe every peer off a running tunnel.
- Stray "wireguard-gui" wording and the helper's log header were corrected to
  "wireguard-tui".
- Helper portability: `wg-helper` no longer needs GNU `find -printf` (it uses a
  pure-bash glob, so BusyBox `find` on Alpine/Void works) and filters listed
  tunnels to valid names. Start-on-boot detects `systemctl` first and fails
  clearly on non-systemd systems; the log view explains when `journalctl` is
  unavailable.
- Helper-path override hardening: `$WG_HELPER` is honoured freely in debug builds,
  but in release builds it is ignored unless `WG_ALLOW_UNSAFE_HELPER=1` is set and
  the target is an absolute, root-owned, non-world-writable file.
- Added unit tests for config parsing, validation and name sanitisation; a CI step
  that shell-syntax-checks `wg-helper` and `install.sh`; an expanded `SECURITY.md`;
  and README cross-links to the desktop sibling.

### 1.1.0 - File-browser import and snappier feedback

Highlights:

- Import is now a file browser: press `i` to navigate directories and pick a
  `.conf` file or a QR image, instead of typing a path.
- Status messages auto-dismiss after a few seconds, so the footer returns to the
  key hints instead of leaving an alert stuck on screen.
- Slow actions give instant feedback: activate/deactivate, delete, start-on-boot
  and save-live repaint a progress line before the blocking privileged call, so a
  key press always shows it is working.
- The footer key-hint bar adapts to terminal width, falling back to a compact set
  (always including `? help` / `q quit`) on narrow terminals.
- A rendered terminal screenshot was added to the README.

### 1.0.0 - First stable release

Highlights:

- The first stable release: a native Linux terminal UI (`wg-tui`) for managing
  WireGuard tunnels, with the same feature set as the desktop client.
- Tunnel list with a live active/inactive indicator and a merged detail view
  (interface plus peers), refreshed every second with latest handshake and
  transfer.
- Activate / Deactivate (`wg-quick up`/`down`).
- Edit a tunnel in `$EDITOR` with config validation; apply changes live to a
  running tunnel without dropping peers (`wg syncconf`).
- New tunnel from a generated template; Generate keypairs and preshared keys
  (`wg genkey`/`genpsk`); Show running config (`wg showconf`) and Save live state
  (`wg-quick save`).
- Import a `.conf` file or a QR-code image; Show QR in the terminal; Export all
  tunnels to a `.zip`.
- Rename / Remove tunnels; a Start-on-boot toggle; a Log tab.
- A hardened privileged helper (`wg-helper`): fixed paths, strict tunnel-name
  validation, atomic writes, timestamped backups, and journald audit logging.
- Privilege backends: sudoers (default) or polkit (`--polkit`), with `pkexec` as a
  fallback. Reuses a co-installed desktop-client helper automatically.
- A universal installer (`install.sh`) for apt, dnf/yum, pacman, zypper, apk, xbps
  and eopkg, with a minimal-install dependency check.

## Versioning policy

- The project follows [Semantic Versioning](https://semver.org/) - `MAJOR.MINOR.PATCH`.
  In short: PATCH for backward-compatible fixes, MINOR for backward-compatible new
  features, and MAJOR for incompatible changes.
- `wg-tui` (terminal UI) and `wireguard-gui` (desktop GUI) are kept in lockstep:
  they share one version number and are released together so they stay
  feature-identical. A given version of one behaves the same as the same version
  of the other, apart from the obvious terminal-versus-window difference. The
  realignment to a single shared number happened at 1.3.4 (see above).
- Every release is tagged (`vMAJOR.MINOR.PATCH`), recorded in
  [CHANGELOG.md](../CHANGELOG.md), and published on GitHub Releases with the
  minisign signing files described at the top of this page.
