# 📋 Changelog

All notable changes to this project are documented here.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.7.2] - 2026-06-21

### Changed
- **Copy now reaches the system clipboard out of the box.** The installer (and
  the `.deb`) now pull in a clipboard helper — `wl-clipboard` (Wayland), else
  `xclip` / `xsel` (X11) — so `y` / `Y` / visual-selection copy land in the real
  clipboard instead of relying on the OSC 52 terminal escape, which many
  terminals ignore. OSC 52 remains the automatic fallback when no helper is
  present (e.g. over plain SSH).

## [1.7.1] - 2026-06-21

### Added
- **Full keyboard control of the Log tab.** Move a highlighted line cursor with
  the arrows / `j` `k`, jump by page (`PgUp` / `PgDn`), to the top or bottom
  (`g` / `G`, `Home` / `End`), and scroll wide lines sideways (`h` / `l`).
- **Copy anything from the Log.** `y` (or `Enter`) copies the current line; press
  `v` to start a visual selection, move to extend it, then `y` to copy the range;
  `Y` copies the whole filtered log. In a tunnel's Detail view, pick a field with
  `h` / `l` and copy it with `y`. Copying now uses `wl-copy` / `xclip` / `xsel`
  when available, falling back to the OSC 52 terminal escape.
- **Automatic updates from GitHub.** On startup wg-tui quietly checks for a newer
  release and shows `update available … (press u)` in the footer; pressing `u`
  downloads it, verifies it against the bundled minisign key, and installs it (an
  unverifiable update is refused). Opt out with `WG_NO_UPDATE_CHECK=1` or a
  `~/.config/wireguard-tui/no-update` file.

### Fixed
- **The Log tab is no longer blank** when first opened: it loads immediately and
  shows a clear placeholder when there is nothing to display or a filter matches
  no lines.
- **Clear (`c`) now sticks** — the background refresh no longer repaints the log a
  second later.

## [1.7.0] - 2026-06-20

### Added
- **Backup manager** (press **`B`** from any tab): create timestamped archives of
  every tunnel config, see them listed with date / size / tunnel-count, and
  **restore** (Enter/`r`), **export** (`x`), or **delete** (`d`) the selected one.
  Archives are written `0600` under `~/.local/share/wireguard/backups` (shared
  with wireguard-gui).

### Changed
- **The Log tab was overhauled**: severity colouring, follow-tail (`f`),
  this-tunnel-only (`t`), substring filter (`/`), save-to-file (`w`), clear
  (`c`), reload (`r`), and page scrolling (PgUp/PgDn/Home/End) — with a title
  bar showing the line count and active modes.
- **Key hints are complete now.** The footer is **tab-aware** (the Log tab shows
  its own keys), and the `?` help screen lists *every* key in full (Tunnels /
  Log / Backup), so no shortcut is hidden — it no longer truncated on the right.

## [1.6.13] - 2026-06-20

### Fixed
- **The `.deb` now installs with `apt` on current Debian/Ubuntu.** The dependency
  `polkit-1 | policykit-1` was unsatisfiable on systems where polkit was renamed
  to `polkitd` + `pkexec` (e.g. recent Ubuntu), so `apt install ./…deb` failed
  with "polkit-1 but it is not installable". It is now
  `pkexec | polkit-1 | policykit-1`, which resolves on both new and old systems
  (`pkexec` pulls `polkitd`).

## [1.6.12] - 2026-06-20

### Changed
- **`install.sh` handles a root login cleanly.** Run from a bare `root` shell
  with no normal user behind it, the installer now stops and recommends running
  as your normal user (Rust builds with a per-user toolchain and the passwordless
  helper grant is per-user, so a root install gets neither). Pass `--allow-root`
  (or `WG_ALLOW_ROOT=1`) to install system-wide as root anyway — that path
  **reuses an existing prebuilt binary** instead of dropping a Rust toolchain into
  `/root`, and skips the per-user helper grant (as root the app runs the helper
  directly, no escalation needed).

## [1.6.11] - 2026-06-20

### Added
- **About screen** (press `A`): app name, version, GitHub and support links, and
  license — parity with the GUI's About window. Any key closes it; the key is
  listed in the footer hints and the `?` help.

## [1.6.10] - 2026-06-19

### Size
- **`zip` uses `deflate-flate2` instead of the `deflate` meta-feature**, dropping
  the Zopfli encoder — ~120 KB off the static binary pre-UPX (~8% on this small
  binary). Exported archives are still standard DEFLATE `.zip`; only the
  compressor changes.
- **`[profile.release] opt-level` switched `"z"` → `"s"`** — measured a few KB
  smaller for this binary (the GUI stays on `"z"`, which is smaller there). No
  behavior change.
  All tests pass; runtime behavior is identical.

## [1.6.9] - 2026-06-19

### Performance
- `fmt_bytes` now picks its unit with a closed-form `floor(log2(b) / 10)` (a
  single `leading_zeros` instruction) instead of a five-branch if/else ladder,
  collapsing five `format!` sites into one. Branch-free and **byte-for-byte
  identical** to the previous output — proven by the new
  `fmt_bytes_matches_ladder` test across every unit boundary and a full-range
  sweep.

### Size
- Release binaries (every static musl arch + the `.deb`) are now
  **UPX-compressed** (`--best --lzma`), cutting the installed on-disk footprint
  by ~60% (wg-tui ~1.5M → ~0.57M, the helper ~0.4M → ~0.17M). Runtime behavior
  is unchanged. Note: UPX repacks the ELF, so the embedded `cargo auditable`
  SBOM is no longer extractable from the *shipped* binaries via
  `cargo audit bin` (the provenance still runs at build time).

## [1.6.8] - 2026-06-19

### Dependencies
- Refreshed all dependencies to their latest releases: `ratatui` 0.29 → 0.30,
  `crossterm` 0.28 → 0.29, and `zip` 4 → 8. No source changes were required;
  `cargo build --release`, Clippy, and all tests remain green.

### CI / supply chain
- Bumped the pinned GitHub Actions to current: `actions/checkout` v6 → v7,
  `actions/upload-artifact` v4 → v7, `actions/download-artifact` v4 → v8.

## [1.6.7] - 2026-06-18

### CI / supply chain
- Release binaries now **embed an SBOM** via `cargo auditable build` (pinned
  `=0.7.4`). The exact dependency set compiled into each shipped binary can be
  audited directly with `cargo audit bin <file>` - verifiable provenance that
  does not require trusting the build logs.
- The `cargo audit` CI gate documents the accepted, non-shipping informational
  advisories (unmaintained / unsound transitive build/proc-macro/optional
  dependencies of `ratatui` and `rqrr`); it still fails on real or new
  vulnerabilities.

## [1.6.6] - 2026-06-18

### Added
- **Static binaries for every common Linux CPU**: the release now ships
  `x86_64`, `i686` (32-bit), `aarch64` (64-bit ARM), and `armv7` (32-bit ARM)
  musl tarballs - fully static, no glibc version to match.

### Security
- The privileged helper now **refuses to save or rename any config containing
  `PostUp`/`PreUp`/`PostDown`/`PreDown` script hooks**. Previously a user holding
  the passwordless helper grant (or someone who handed the victim a crafted
  `.conf`) could obtain arbitrary root code execution, since `wg-quick` runs
  those hooks as root on activation. (CWE-78 / CWE-269)
- The **polkit rule now requires membership in a dedicated `wireguard` group**
  instead of authorizing every active-local user, closing a path where another
  local user could read your private keys through the helper. `install.sh
  --polkit` and the `.deb` create the group and add the intended user.
  (CWE-200 / CWE-522)
- `redact_config` now matches the helper's stricter redactor so a secret-bearing
  log line without an `=` can no longer slip through. (CWE-532)

### CI / supply chain
- Release **signing is fail-closed**: a missing `MINISIGN_KEY` aborts the
  release instead of publishing unsigned artifacts.
- Workflow tokens are **least-privilege**: read-only everywhere except the single
  publish job; CI declares `contents: read`.
- `cargo-deb` is pinned to an exact version (`=2.12.1`); the first-party
  `upload-/download-artifact` actions are pinned to commit SHAs.
- The release tag is passed via the environment and validated against
  `vMAJOR.MINOR.PATCH` (no Actions expression-injection into shell steps).
- CI runs `cargo audit` (RUSTSEC), and Dependabot watches the cargo and
  github-actions ecosystems.

### Changed
- `SECURITY.md` and `README.md` updated to document the new posture.

## [1.6.5] - 2026-06-18

### Security
- Exported tunnel archive (which bundles every `PrivateKey`/`PresharedKey`) is
  now created `0600` and with `O_NOFOLLOW`, closing a world/group-readable +
  symlink-clobber key leak (`File::create` previously used the process umask).
- Kill switch IPv6 fallback now fails closed: when only iptables is available,
  the host has a non-loopback IPv6 address, and `ip6tables` is missing, enabling
  the kill switch errors instead of silently leaving IPv6 unprotected while the
  status still reads "enabled".

### Fixed
- Kill switch no longer blocks the tunnel's own traffic: added an explicit allow
  rule for packets leaving the WireGuard interface (`oifname <iface>`).
  Previously every packet routed into the tunnel hit the terminal REJECT, so
  enabling the kill switch cut off all VPN traffic.
- Deactivating a tunnel now tears the kill switch down with it (`down` calls
  `killswitch_disable`), preventing the orphaned REJECT rule that otherwise
  locked the machine out of all non-loopback egress.
- FwMark is now set at runtime (`wg set <iface> fwmark`) instead of being
  appended to the `.conf`, where it landed inside the last `[Peer]` section and
  corrupted the config; this also removes the down/up restart and its leak
  window, and prefers the fwmark `wg-quick` already assigned.
- `PersistentKeepalive = off` is accepted as the valid WireGuard value it is.
- Repeated `Address`/`AllowedIPs`/`DNS` lines are combined when parsing rather
  than last-write-wins, so both an IPv4 and an IPv6 value survive.
- Endpoint validation rejects malformed hosts (leading `-`, invalid dotted
  quads such as `999.999.999.999`) and ports with a leading `+`.
- Import-name sanitisation collapses `..`, so an imported file can no longer
  produce a name the privileged helper rejects.
- `quick_add_peer` no longer wraps the external editor in a redundant second
  `restore()`/`init()` cycle (it discarded a freshly-initialised terminal and
  grew the panic-hook chain).

### CI
- `cargo-deb` is version-pinned (`^2`) in the release workflow.

## [1.6.4] - 2026-06-18

### Security
- SECURITY.md unified: supported versions, email fallback, threat model, 
  helper path pinning, `WG_ALLOW_UNSAFE_HELPER`, private key handling,
  supply chain verification.
- FwMark detection hardened to exact key-name match (prevents `FwMarkFile` confusion).

### Fixed
- CHANGELOG wording corrected (removed stale `popup_area` claim, fixed `DetailRow` reference).
- `+` quick-add-peer key now visible in Advanced footer and help screen.
- `src/ui/` placeholder module removed.

## [1.6.3] - 2026-06-18

### Fixed
- Clippy warnings eliminated across all targets; `-D warnings` enforced in CI.
- Terminal QR no longer fails with "too large" on wide terminals.
- Shared Slint component fixes synchronized with the desktop client.

### Changed
- Markdown documentation fully audited and synchronized with v1.6.3 codebase.

## [1.6.2] - 2026-06-18

### Added
- SSH auto-allowlist: kill switch auto-allows established SSH return traffic
  when `$SSH_CONNECTION` is detected.
- Terminal QR cell-aspect-ratio auto-detection (square vs 2:1 cells).
- TUI `+` key in Advanced mode to quick-add a `[Peer]` section via `$EDITOR`.
- Validation tests: tunnel name edge cases, max length, import sanitization.
- More helper tests: killswitch rule structure, iptables comment safety,
  SSH port parsing, FwMark detection, config validation edges.

### Changed
- GUI: `DetailRow` copy area uses clickable text + ⧉ icon instead of large
  "Copy" button; fixed text centering regression.
- GUI: `StatusDot` component extracted to `ui/components/statusdot.slint`.
- Log limits increased to 1000 lines (was 300/200).

### Fixed
- Speed/throughput could get stuck at 0 when active detection from dump failed.
- `DetailRow` text centering caused by `TouchArea` default alignment.


## [1.6.0] - 2026-06-17

### Added
- nftables kill switch backend (preferred when `nft` is available; iptables/ip6tables fallback).
- SSH safety warning when enabling kill switch over an SSH connection.
- Kill switch rule-generation tests (nftables handle extraction, iptables rule numbering).

### Changed
- Speed display uses ↓ ↑ icons instead of "down"/"up" text.
- Log lines increased to 1000 (was 300/200) in the privileged helper.
- Throughput polling samples at 0.5s interval for smoother readings.
- Tunnel active detection uses `wg show interfaces` list, fallback to dump.

### Fixed
- Speed/throughput could get stuck at 0 when active detection from dump failed.

## [1.5.5] - 2026-06-17

### Added
- Rust `wg-helper` binary with the same fixed verb contract as the previous
  helper.
- Advanced-mode `K` kill switch toggle backed by helper-managed
  iptables/ip6tables rules for active tunnels.
- aarch64 Linux release tarballs and Alpine/Void packaging templates.
- Easy Mode can create a tunnel from scratch with `n`, preset selection, and
  `$EDITOR` review.

### Changed
- Reframed project docs and package metadata around the small native Linux
  WireGuard client model: plain `/etc/wireguard` configs, `wg`/`wg-quick`, no
  NetworkManager layer, and no mandatory runtime core.
- The first-party TUI installer and packages no longer install desktop
  integration by default. `./install.sh --desktop` remains available for users
  who want a launcher.
- OSC52 copy now normalizes single-field payloads before writing the escape
  sequence.

### Security
- The helper now performs a second, privileged-boundary config shape check before
  save/rename, in addition to frontend validation.
- Helper writes now make a best-effort `sync -f` before atomic rename.
- Runtime helper command execution moved out of shell into Rust argv-based
  process calls with fixed tool lookup and timeouts.
- CI now runs negative helper-name validation tests.
- CI now runs installer sanity checks for helper paths, sudoers validation, and
  non-root build handoff.

### Fixed
- Copying public keys no longer carries accidental leading/trailing whitespace
  or display newlines.

## [1.5.4] - 2026-06-17

### Fixed
- **Installer now finds `sbin` tools after the root re-exec.** A normal user's
  `PATH` (which `su` carries into the as-root re-exec) usually omits `/usr/sbin`
  and `/sbin`, so `visudo` and the `resolvconf` probe silently failed - the
  passwordless drop-in was skipped ("sudoers validation failed") and a present
  `openresolv` was misreported as missing. The installer now puts sbin on `PATH`,
  so the sudoers drop-in is written and resolvconf is detected correctly.
- Clearer hint when the drop-in is skipped: re-run `./install.sh --polkit`
  (a `--polkit` is an installer flag, not a `wg-tui` one).

## [1.5.3] - 2026-06-17

### Added
- **"No usable sudo -> install sudo and set it up."** When you can't use sudo
  (e.g. a Debian server where you aren't in the sudoers file), the installer
  re-runs itself as root with a **single** ROOT-password prompt (instead of one
  per step), installs `sudo` if it's missing, and writes a passwordless drop-in
  scoped to the helper for *your* user - which makes `wg-tui` work as a normal
  user (no root, no prompt) even though you aren't in the `sudo` group.

### Security
- **Don't trust `$USER` when writing the sudoers rule.** The target user is now
  taken from the real uid (`id -un`), validated against a strict username pattern
  and confirmed to exist, so a crafted `$USER` can't inject a wider sudoers spec
  (e.g. `NOPASSWD: ALL`) - `visudo -cf` alone does not catch that.
- **The build no longer runs as root.** `cargo`/`rustup` run as the invoking user
  via `runuser`, so dependency build scripts never execute with root privilege and
  the toolchain/artifacts stay in that user's home.

### Fixed
- Clear, actionable error when the app can't gain root (no passwordless sudo and
  no `pkexec`) instead of a cryptic `spawn failed: No such file or directory`.
- Installer fails loudly if it can't set up any privilege path, rather than
  reporting success and leaving a non-working install; `apt-get update` hiccups no
  longer abort the run.

## [1.5.2] - 2026-06-17

### Fixed
- **Install works when `sudo` is present but you're not a sudoer.** The installer
  (and `wg-tui setup`) previously assumed that having the `sudo` binary meant you
  could use it - so on a Debian server where the login user isn't in the sudoers
  file (`<user> is not in the sudoers file`) it aborted. It now probes real sudo
  usability (`sudo -n` / admin-group membership) and falls back to `su` (the ROOT
  password), and auto-switches the helper authorization to a polkit rule. Tip:
  run `su -` first, then `./install.sh`, to be prompted once instead of per step.

## [1.5.1] - 2026-06-17

### Added
- **DNS / resolvconf check.** `wg-tui doctor` now reports whether a resolvconf
  provider is available for tunnels that use a `DNS =` line, and `wg-tui setup`
  offers to install one (`openresolv`; systemd-resolved also counts). This is the
  fix for minimal Debian, where such tunnels failed with
  `resolvconf: command not found`.
- **Per-distro guide** ([docs/DISTROS.md](docs/DISTROS.md)): what to install, what
  to set up, what survives a reboot, and when - only as a server/gateway - you need
  firewall and IP-forwarding changes.

### Changed
- **Works without `sudo`.** The installer and `wg-tui setup` fall back to `su`
  (the ROOT password) on Debian-minimal where `sudo` isn't present, and the
  installer auto-switches the helper authorization from a sudoers drop-in to a
  polkit rule when there's no `sudo`.
- Activation failures are now explained in plain language (e.g. the missing
  resolvconf provider) instead of the raw `wg-quick` output.

### Fixed
- Tunnels with a `DNS =` line no longer fail on systems without a resolvconf
  provider: the installer best-effort installs `openresolv` when neither it nor
  systemd-resolved is present.

## [1.5.0] - 2026-06-17

### Added
- **`wg-tui doctor`** - prints a plain-language system checklist (WireGuard
  tools, the privileged helper + authorization, `/etc/wireguard`, systemd,
  journald) and exits **0** (all OK) / **1** (warnings) / **2** (critical
  missing). Read-only; useful over SSH.
- **`wg-tui setup`** - a guided, confirmation-based fix: offers to install
  `wireguard-tools` via your package manager and points you at the installer for
  the helper. Never connects tunnels or enables start-on-boot.
- A friendly **first-run hint** inside the app when something critical is
  missing, and a **beginner empty state** ("Import a .conf or QR image to begin")
  that adapts to Easy/Advanced mode.

### Notes
- The app **does not bundle WireGuard kernel modules or `wg`/`wg-quick`** - it
  uses your system's `wireguard-tools`, and helps you install them.

## [1.4.1] - 2026-06-17

### Added
- **Distro packaging**: an **AUR `PKGBUILD`** (Arch) and an **RPM spec for COPR**
  (Fedora/RHEL/Rocky), plus `packaging/PACKAGING.md`.

### Changed
- Documentation now uses plain ASCII hyphens instead of em dashes.

## [1.4.0] - 2026-06-17

### Added
- **Live throughput** - the detail view shows real-time **down/up speed** for the
  selected tunnel, plus a **Connection** health line derived from the most recent
  handshake (OK / stale / waiting).
- **`wg-tui --version` / `--help`** - flags now print and exit instead of
  launching the full-screen UI.

### Changed
- **Removed demo mode** (`WGTUI_DEMO`) - the app always talks to real tunnels.
- The privileged helper now **bounds every `wg`/`wg-quick` call with a timeout**,
  so a hang (DNS, a stuck `PostUp`, a wedged interface) can't lock up the app.
- CI now runs a **smoke test** (`--version`/`--help` start and exit cleanly).

## [1.3.5] - 2026-06-17

### Added
- **Copy the interface public key** to the clipboard with **`y`** (via the OSC 52
  terminal escape, so it works locally and over SSH in supporting terminals) -
  closes the last copy-to-clipboard parity gap with the desktop client.

## [1.3.4] - 2026-06-17

> Version realigned with the desktop client (`wireguard-gui`) so the family
> shares one number - this is the release after 1.1.2.

### Added
- **Easy mode** (default) for everyday users: shows only Connect/Disconnect,
  Import, Start-on-boot, Remove and Show-QR; press **`m`** for Advanced mode
  (edit, new, generate keys, running config, save-live, rename, export). The
  choice is remembered (`~/.config/wireguard-tui/mode`).
- **Bulk import**: in the import browser, **Space** marks files and **Enter**
  imports all marked ones at once (matches the desktop client).
- A **desktop icon** (shared with the GUI) is installed and used by the launcher.

### Fixed
- **Terminal rendering corruption on some fonts/locales.** The UI no longer uses
  ambiguous/wide Unicode glyphs (arrows, `●`/`○` dots, `...`, dingbats) that some
  terminals render double-width or as tofu, garbling the layout - it is now
  ASCII-only (box-drawing borders aside).

## [1.1.2] - 2026-06-16

### Changed
- **Release pipeline hardened to match the desktop client.** The `.deb` is now
  mandatory (a missing one fails the release), `SHA256SUMS` is generated with
  `nullglob` and verified non-empty, and it is **signed with minisign** -
  `SHA256SUMS.minisig` and `minisign.pub` now ship with every release.
- The privileged helper is also discovered **next to the binary**, so an
  extracted release tarball works without first running `install.sh`.
- CI now **hard-fails on `shellcheck` warnings** for `wg-helper` and `install.sh`.

## [1.1.1] - 2026-06-16

### Fixed
- **A bracketed-IPv6 `Endpoint`** (e.g. `[2001:db8::1]:51820`) is now accepted by
  config validation again. The stricter endpoint check added in 1.1.0 wrongly
  rejected it, which blocked saving/importing IPv6-endpoint tunnels. Covered by a
  regression test.
- `wg-helper`'s `sync` now verifies `wg-quick strip` succeeded before applying it
  to the live interface - a strip failure previously fed an empty config to
  `wg syncconf` and could wipe every peer off a running tunnel.
- Corrected stray "wireguard-gui" wording and the log header in the helper to
  "wireguard-tui".

### Changed
- **Helper portability.** `wg-helper` no longer needs GNU `find -printf` (uses a
  pure-bash glob, so it works with BusyBox `find` on Alpine/Void) and filters
  listed tunnels to valid names. Start-on-boot detects `systemctl` first and
  fails clearly on non-systemd systems (`is-enabled` reports "unknown"); the log
  view explains when `journalctl` is unavailable.
- **Helper-path override hardening.** `$WG_HELPER` is honoured freely in debug
  builds, but in release builds it is ignored unless `WG_ALLOW_UNSAFE_HELPER=1`
  is set *and* the target is an absolute, root-owned, non-world-writable file.

### Added
- Unit tests for config parsing, validation and name sanitisation; a CI step
  that shell-syntax-checks `wg-helper` and `install.sh`.
- An expanded `SECURITY.md` (threat model, privilege boundary, `PostUp` root
  hooks, private-key/QR handling, supply-chain verification).
- README cross-links the desktop sibling (`wireguard-gui`) and explains the
  `wg-quick` (not NetworkManager) model, the init-system limitation, and the
  QR/private-key warning.

## [1.1.0] - 2026-06-16

### Added
- **Import is now a file browser** - press `i` to navigate directories and pick a
  `.conf` file or a QR image, instead of typing a path.
- A rendered terminal screenshot in the README.

### Changed
- **Status messages auto-dismiss** after a few seconds, so the footer returns to
  the key hints instead of leaving an alert stuck on screen.
- **Instant feedback for slow actions** - activate/deactivate, delete,
  start-on-boot and save-live now repaint a progress line *before* the (blocking)
  privileged call, so a key press always shows it is working.
- The footer key-hint bar **adapts to the terminal width**, falling back to a
  compact set (always including `? help` / `q quit`) on narrow terminals.

## [1.0.0] - 2026-06-16

### Added
- First release: a native Linux **terminal UI** (`wg-tui`) for managing
  WireGuard tunnels, with the same feature set as the desktop client.
- Tunnel list with a live active/inactive indicator and a merged detail view
  (interface + peers), refreshed every second - latest handshake and transfer.
- Activate / Deactivate (`wg-quick up`/`down`).
- Edit a tunnel in `$EDITOR` with config validation; apply live to a running
  tunnel without dropping peers (`wg syncconf`).
- New tunnel from a generated template; **Generate** keypairs + preshared keys
  (`wg genkey`/`genpsk`); **Show running config** (`wg showconf`) and
  **Save live state** (`wg-quick save`).
- Import a `.conf` file or a **QR-code image**; **Show QR** in the terminal;
  **Export** all tunnels to a `.zip`.
- Rename / Remove tunnels; **Start-on-boot** toggle; a **Log** tab.
- **Hardened privileged helper** (`wg-helper`): fixed paths, strict tunnel-name
  validation, atomic writes, timestamped backups, journald audit logging.
- Privilege backends: **sudoers** (default) or **polkit** (`--polkit`); `pkexec`
  fallback. Reuses a co-installed desktop-client helper automatically.
- **Universal installer** (`install.sh`) for apt, dnf/yum, pacman, zypper, apk,
  xbps and eopkg, with a minimal-install dependency check.

[Unreleased]: https://github.com/JamilleJung/wireguard-tui/compare/v1.5.0...HEAD
[1.5.0]: https://github.com/JamilleJung/wireguard-tui/compare/v1.4.1...v1.5.0
[1.4.1]: https://github.com/JamilleJung/wireguard-tui/compare/v1.4.0...v1.4.1
[1.4.0]: https://github.com/JamilleJung/wireguard-tui/compare/v1.3.5...v1.4.0
[1.3.5]: https://github.com/JamilleJung/wireguard-tui/compare/v1.3.4...v1.3.5
[1.3.4]: https://github.com/JamilleJung/wireguard-tui/compare/v1.1.2...v1.3.4
[1.1.2]: https://github.com/JamilleJung/wireguard-tui/compare/v1.1.1...v1.1.2
[1.1.1]: https://github.com/JamilleJung/wireguard-tui/compare/v1.1.0...v1.1.1
[1.1.0]: https://github.com/JamilleJung/wireguard-tui/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/JamilleJung/wireguard-tui/releases/tag/v1.0.0
