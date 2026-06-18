# 🙏 Contributing to wireguard-tui

Thanks for your interest in improving `wg-tui` (also `wireguard-tui`)!
This is a small, hackable project - issues, ideas, discussions, and pull
requests are all welcome.

## 🛠️ Getting set up

```sh
git clone https://github.com/JamilleJung/wireguard-tui.git
cd wireguard-tui

# One-command install (build deps + wireguard-tools + Rust + build + install):
./install.sh

# Or, if you already have the toolchain:
cargo build --release
cargo run --release
```

The binary is `wg-tui`; a symlink `wireguard-tui` is created by `install.sh`.

Build prerequisites (the installer handles these automatically):
- Rust toolchain (via [rustup](https://rustup.rs))
- `wireguard-tools` (`wg` + `wg-quick`)
- No GUI libraries - the TUI is deliberately dependency-light and works over SSH.

`cargo run --release` talks to real tunnels. Test changes in a throwaway VM
or with a local test `/etc/wireguard` setup if you don't want to touch
production configs.

## 📁 Project layout

| Path | Purpose |
|------|---------|
| `src/main.rs` | App entrypoint, event loop, key handling, rendering |
| `src/backend.rs` | Helper client, WireGuard/system ops, QR, export |
| `src/config.rs` | WireGuard config parsing and validation |
| `src/create.rs` | Easy Mode tunnel templates and presets |
| `src/clipboard.rs` | OSC52 single-field copy normalization |
| `src/secrets.rs` | Secret redaction and script-hook detection |
| `src/validation.rs` | Tunnel name validation and import-name sanitization |
| `src/doctor.rs` | Read-only system checks and setup hints |
| `src/bin/wg-helper.rs` | Privileged Rust helper - the security boundary |
| `install.sh` | Distro-aware source installer |
| `tests/helper-validation.sh` | Shell tests for helper name validation |
| `tests/installer-sanity.sh` | Installer content sanity checks |
| `docs/` | Tutorial, distro guide, release notes |
| `packaging/` | Polkit, AUR/RPM/APK/Void metadata, optional desktop assets |
| `.github/workflows/` | CI and release automation |

## ✅ Before you open a PR

CI runs these and **will fail the build** if they don't pass. Run them locally:

```sh
# Rust quality gates
cargo fmt --all - --check
cargo clippy --all-targets - -D warnings
cargo test
cargo build --release

# Shell linting
bash -n install.sh
bash -n tests/installer-sanity.sh
shellcheck -S warning install.sh tests/helper-validation.sh tests/installer-sanity.sh

# Helper smoke tests
bash tests/helper-validation.sh target/release/wg-helper
bash tests/installer-sanity.sh

# App smoke tests
target/release/wg-tui --version
target/release/wg-tui --help
target/release/wg-tui doctor
```

## 📝 Guidelines

### 📝 General

- Keep the diff focused; match the surrounding Rust style.
- Write clear commit messages: short imperative subject, optional body
  explaining *why*. Reference issues with `#123`.
- Add a line to `CHANGELOG.md` under `[Unreleased]` for user-facing changes.
- Prefer pure-Rust dependencies; the TUI builds with no GUI/C library deps.

### 🔒 Helper safety (`src/bin/wg-helper.rs`)

The helper is the **privileged boundary** - it runs as root. Preserve or
strengthen these properties:

- **Fixed verbs only.** Every operation is a named verb (`list`, `up`, `save`,
  `killswitch-enable`, ...). No caller-controlled executable paths.
- **Fixed paths.** `WG_DIR` is `/etc/wireguard`. Tools are looked up in
  `/usr/sbin:/usr/bin:/sbin:/bin` only.
- **Strict name validation.** Tunnel names must match
  `^[A-Za-z0-9][A-Za-z0-9_.-]{0,14}$`. No `..`, `/`, `\`, or shell
  metacharacters.
- **No `sh -c`.** All subprocess calls use argv arrays directly.
- **Timeouts.** Every external call has a `Duration` bound.
- **Atomic writes.** Temp file + best-effort `sync -f` + `rename`.
- **Backups.** Timestamped copy in `/etc/wireguard/.backup/` before any
  destructive operation.
- **Logging.** Mutating actions are logged via `logger` without key material.
- **Config re-validation.** The helper validates config shape before save/rename
  (second check, inside the privileged boundary).

### 🎨 UI notes

- Keep the UI keyboard-driven and discoverable.
- Update the `?` help screen and footer hints when you add or change a key.
- Advanced-mode keys: add to `is_advanced_key()` and the Advanced keymap table.
- Editor flow: use `run_editor()` - it handles temp files (mode `0600`,
  `O_EXCL`), validation, and cleanup. Don't bypass it.

### ➕ Adding a feature

1. **New key binding** → add to `handle_key()` + update `is_advanced_key()` if
   needed + update help screen + footer keymap.
2. **New backend operation** → add a helper verb in `src/bin/wg-helper.rs`,
   then call it from `src/backend.rs`.
3. **New Easy Mode preset** → add to `src/create.rs`.
4. **New validation rule** → add to `src/config.rs` or `src/validation.rs`.
5. **New doctor check** → add to `src/doctor.rs`.

Add tests for new logic. The test suite covers:
- `src/bin/wg-helper.rs` - name validation, config shape, secret redaction,
  kill switch rule generation (18 tests)
- `src/main.rs` - backend, clipboard, config, create, doctor, secrets,
  validation (25 tests)

## 🐛 Reporting bugs

Open an issue with:
- Your distro and version
- Terminal emulator and `$TERM`
- `wg-tui --version` output
- Steps to reproduce
- Any relevant `wg-quick` or `journalctl` output (redact private keys!)

**Never paste real private keys, preshared keys, or production configs into an issue.**

## 💡 Feature requests

Open a discussion or issue. Describe the workflow you want, not just the key
binding. The project philosophy values staying close to `wg`/`wg-quick`
primitives - proposals that respect that are more likely to be accepted.

## 📄 License

By contributing, you agree that your contributions are licensed under the
project's [MIT License](LICENSE). WireGuard® is a registered trademark of
Jason A. Donenfeld. This is an independent, unofficial project.
