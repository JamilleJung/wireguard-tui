<div align="center">

# 🙏 Contributing to wireguard-tui

### Small, hackable, and happy to have you.

</div>

Thanks for wanting to make `wg-tui` better! Bug reports, ideas and focused
pull requests are all welcome. The latest release is **1.9.3**; what changed
since then lives under `[Unreleased]` in [CHANGELOG.md](CHANGELOG.md).

## 🚀 Get set up in 3 commands

```sh
git clone https://github.com/JamilleJung/wireguard-tui.git && cd wireguard-tui
scripts/dev-setup.sh      # checks your machine, prints how to install anything missing
cargo test --locked       # if this is green, you're ready to hack
```

Then take it for a spin:

```sh
cargo run --release --locked --bin wg-tui -- --help
```

What you need: **Rust 1.88 or newer** (edition 2024; CI's "MSRV" job enforces
the minimum, current stable is recommended), a C linker/toolchain, and Linux
for the runtime and integration checks. There is no graphical toolkit to
install. `scripts/dev-setup.sh --install` also adds missing rustup components
and prefetches the locked crates; it never uses sudo and only prints the
system packages for you to install.

> [!WARNING]
> Running the UI operates on your **real** `/etc/wireguard` tunnels. Use a
> disposable Linux VM for anything that changes networking, firewall rules or
> authorization. Platform-independent tests also run on other Unix hosts; that
> doesn't make those hosts supported WireGuard targets.

`wg` and `wg-quick` come from `wireguard-tools`; an editor, helper
authorization and DNS/firewall tools are runtime concerns that `./install.sh`
takes care of (see [README.md](README.md)). The two Cargo binaries are
`wg-tui` and `wg-helper`. The `wireguard-tui` command is an alias the source
installer creates, not a third binary.

## 🗺️ Project map

| Path | What lives there |
|---|---|
| `src/main.rs`, `src/cli.rs` | Startup, argument parsing, `doctor` / `setup` entry points |
| `src/runtime.rs` | The event loop, the background poller, the startup update check, editor/update hand-offs |
| `src/state.rs` | `App` state, popups, drafts, and how background results are folded in |
| `src/input.rs`, `src/keymap.rs` | Key handling; `KEYMAP`, the one table that maps keys **and** builds the `?` popup |
| `src/actions.rs`, `src/ops.rs` | Background operations; the operation state machine, state verification, kill-switch repair policy |
| `src/editor.rs` | External editor on a private draft, hand-off to draft recovery |
| `src/ui/`, `src/theme.rs` | Rendering, small-terminal layouts, popups; default/high-contrast/ASCII themes and `NO_COLOR` |
| `src/backend.rs`, `src/backend/` | Helper client (`helper.rs`), backups (`backup.rs`), QR (`qr.rs`), per-peer health (`health.rs`), diagnostics |
| `src/import_parse.rs`, `src/qr_isolate.rs`, `src/safe_file.rs` | Backup/QR parsing (fuzzable), the isolated QR decoder child, the one safe file-open routine |
| `src/config.rs`, `src/secrets.rs` | Config parsing/validation and secret redaction, **also compiled into the helper** |
| `src/validation.rs`, `src/create.rs`, `src/clipboard.rs` | Tunnel names and import-name sanitizing, preset templates, clipboard tools + OSC 52 |
| `src/logging.rs`, `src/doctor.rs`, `src/update.rs` | Opt-in redacted debug log, prerequisite checks, the signed updater |
| `src/bin/wg-helper/` | The privileged helper: verbs, process limits, files, runtime, firewall (`nft.rs`, `iptables.rs`), audit log |
| `tests/` | Shell suites, property tests (`prop_*.rs`), QR isolation test, fake fixtures |
| `fuzz/` | The cargo-fuzz crate |
| `install.sh`, `packaging/`, `.github/workflows/` | Installer, polkit rule, desktop entry, distro recipes, CI and release jobs |
| `docs/help.md`, `CHANGELOG.md` | Embedded into the app with `include_str!` — `H` and `w` show them |

The bigger picture (privilege boundary, data flows, kill-switch design) is in
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## ✅ Required checks

These match the CI quality gates. Run them before you open a PR:

```sh
scripts/dev-setup.sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
scripts/check-version.sh
scripts/gen-fixtures.sh --check
shellcheck -S warning install.sh scripts/*.sh tests/*.sh
bash tests/helper-validation.sh target/release/wg-helper
bash tests/release-regression.sh target/release/wg-tui target/release/wg-helper
bash tests/installer-sanity.sh
target/release/wg-tui --version
target/release/wg-tui --help
```

> [!NOTE]
> This block is not just documentation: `tests/dev-onboarding.sh` runs exactly
> these lines in a fresh clone (see [Onboarding test](#onboarding-test)). If
> you change a command here, make sure it still works from a clean checkout.

### 📋 Before you open a PR

- [ ] The checks above pass (or you say which ones you couldn't run, and why).
- [ ] The diff is focused, and changed behaviour has a meaningful regression test.
- [ ] User-visible changes have a line under `[Unreleased]` in `CHANGELOG.md`.
- [ ] The README, tutorial, `docs/help.md` or SECURITY.md are updated if behaviour changed.
- [ ] A new or changed key is in `KEYMAP` (so `?` shows it) and in the footer hints.
- [ ] No private keys, preshared keys, production configs, QR codes or backups anywhere.
- [ ] Dependency changes pass `cargo audit --deny warnings` (below).

`target/release/wg-tui doctor` is deliberately **not** in the list: `0`, `1`
and `2` are all intended results depending on the host, and `setup` returns
the refreshed doctor result too. Neither is a success-only smoke test.

When you change dependencies, install and run the audit gate:

```sh
cargo install cargo-audit --locked
cargo audit --deny warnings
```

The current locked graph needs no advisory exceptions. Please don't mask a new
finding to make CI green: investigate the update, and explain any unavoidable
upstream limitation in the PR description.

## 🧭 How things fit together

**Keys.** All keys work without a mode switch; the "Advanced" heading in `?`
is descriptive only. Add a binding to `KEYMAP` in `src/keymap.rs` (the table
that also builds the help popup) and map it in `command_for()`. Mind the Log
tab's overrides (`g`, `c`, `w`, `Enter`), and remember that popups consume
keys first (`handle_modal()` in `src/input.rs`). Tests check that every
command has a key and every documented key works in its tab. Update the
footer hints too.

**Editing.** Raw editing goes through `src/editor.rs`: it suspends and
restores the terminal, uses a private `0600` draft in a fresh `0700`
directory, hands the result to validation and draft recovery, and cleans up.
Don't bypass it. The TUI uses raw text editing, not the desktop client's form
editor.

**Long operations.** Anything that talks to the helper runs as a background
operation (`src/actions.rs`) and is verified against the state observed
afterwards (`src/ops.rs`), not against an exit code.

### 🔒 Keeping the helper boring (in the best way)

The helper runs as root, so it stays small, strict and predictable:

- **Fixed verbs only** (`verb_shape()` in `src/bin/wg-helper/main.rs`) and
  `/etc/wireguard` destinations. Tools come only from
  `/usr/sbin:/usr/bin:/sbin:/bin`; children get a cleared environment with a
  fixed `PATH` and `LC_ALL=C`.
- **Validate names first**, before any filesystem work: ASCII alphanumeric
  first, then letters/digits/`_`/`-`/`.`, at most 15 bytes, no `..` and no
  `.conf` suffix.
- **argv arrays, never a shell.** Keep the bounded process groups and the
  concurrent pipe draining, so output can't fill a pipe and hang the timeout.
- **Re-validate inside the helper** on save/rename and reject root script
  hooks there, even when the frontend already checked.
- **Write carefully:** exclusive `0600` temp files, `sync_all()`, atomic
  rename, directory sync, and a timestamped backup before overwrite, rename or
  delete. Check the resulting state after every mutation.
- **Log metadata, never keys.** Every mutating verb writes one redacted
  `key=value` journal record.
- **Kill switch:** keep it in its own nftables table / iptables chain with
  the tunnel-name rule comments, exceptions that only end the kill switch's
  own chain (nft `accept`, iptables `RETURN`), the iptables jump first in
  `OUTPUT`, both-family preflight, and fail-closed cleanup paths.
- **Bounded restore and anchored updates:** keep the backup restore preflight
  bounded, and keep updater verification anchored to the compiled-in key —
  never trust a key shipped inside the download.

<details>
<summary>🔧 Pointing the app at a debug helper</summary>

```sh
cargo build --locked --bin wg-helper
WG_HELPER=/absolute/path/to/wg-helper cargo run --locked --bin wg-tui
```

Debug builds accept `WG_HELPER` directly. Release builds also require
`WG_ALLOW_UNSAFE_HELPER=1`, an absolute root-owned executable, no group/other
write bits and trusted directory ancestry. The override does not create a
sudoers/polkit grant: authorization for the installed path still applies.
For normal use, prefer the installed standard paths.

</details>

## 🧪 Testing infrastructure

Beyond the required checks there is a whole toolbox. You won't need all of it
for every change, but it's good to know where things are.

### Property tests and fixtures

`tests/prop_*.rs` are dependency-free property tests. The binaries have no
library crate, so each test pulls the module it checks straight from `src/`
with `#[path = "../src/config.rs"] mod config;` (the helper uses the same
trick). A small deterministic PRNG in `tests/prop_support/` produces the
inputs; a failure prints the seed and case so you can replay it:

```sh
cargo test --locked --test 'prop_*'                       # default 2000 cases each
PROP_CASES=50000 cargo test --locked --release --test prop_config
PROP_SEED=0x1234 cargo test --locked --test prop_secrets  # another sequence
```

Invariants covered: parsing and validation never panic on arbitrary or
mutated input; anything accepted satisfies what its consumers rely on (valid
keys, peers with AllowedIPs, well-formed endpoints); random wg-quick configs
are accepted and parse/render round-trips are stable; LF and CRLF give the
same verdict; no PrivateKey/PresharedKey value survives redaction in any
casing, spacing or log-prefix form, while other lines stay unchanged; every
`src/create.rs` template validates once completed; sanitized import names are
always valid and idempotent; the frontend name validator agrees with the built
`wg-helper` on every random name; and `tests/prop_import.rs` covers backup
archive limits, unsafe and duplicate names, and QR selection.

`tests/fixtures/configs/` holds **fake** sample configs (`valid/`, `hooks/`,
`invalid/`) for tests, fuzzing and manual experiments. Their keys are derived
from SHA-256 of fixed labels. Never add a real config there: change
`scripts/gen-fixtures.sh` and rerun it instead. CI runs
`scripts/gen-fixtures.sh --check` to prove the files match the script.

### Fuzzing

`fuzz/` is a standalone cargo-fuzz crate (its own workspace and lockfile)
with the targets `config_validate`, `redact`, `tunnel_names`,
`import_preflight`, `backup_zip` and `qr_select`. They include the same
`src/` modules by path. You need a nightly toolchain and cargo-fuzz:

```sh
rustup toolchain install nightly
cargo install cargo-fuzz --locked
mkdir -p fuzz/corpus/config_validate
cp tests/fixtures/configs/*/*.conf fuzz/corpus/config_validate/
cargo +nightly fuzz run config_validate -- -max_total_time=300
```

Backup ZIP and QR import parsing lives in `src/import_parse.rs`, which the
fuzz crate includes directly: `backup_zip` fuzzes archive preflight and
limits, `qr_select` the bounded image decode and grid selection. Small valid
seed archives and images live in `fuzz/seeds/`.

> [!NOTE]
> Grid detection in `rqrr` is fuzzed only with
> `cargo +nightly fuzz run qr_select --features detect`, because rqrr 0.11
> asserts on some crafted grids. That's exactly why the apps decode QR images
> in a child process (`src/qr_isolate.rs`, tested by `tests/qr_isolation.rs`):
> such a crash only fails that one import.

CI checks that the targets compile on stable for every pull request ("Fuzz
crate check") and fuzzes each target for five minutes nightly
(`.github/workflows/fuzz.yml`; crash reproducers are uploaded as artifacts).

### Mutation testing

`cargo mutants` checks that the tests notice changes to validation and
security-critical code: `src/validation.rs`, `src/config.rs`,
`src/secrets.rs` and the helper under `src/bin/wg-helper/`. It runs weekly and
on demand (`.github/workflows/mutants.yml`, four shards, report in the job
summary and the `mutants-shard-*` artifacts). Locally:

```sh
cargo install cargo-mutants --locked
cargo mutants --timeout 300 --file src/validation.rs     # one file, quick
cargo mutants --list --file 'src/bin/wg-helper/**/*.rs'  # preview the helper's mutants
```

A "missed" mutant names a change no test detects. Add a test that fails for
it, or mark the function `#[mutants::skip]` with a comment explaining why the
mutation is equivalent. Never run mutation testing of the helper on a host
whose firewall or tunnels matter.

### Firewall namespace and kill switch end-to-end tests

Firewall argument tests don't replace real kernels, so two suites run in
disposable network namespaces (CI does both on every PR):

```sh
bash tests/firewall-namespace.sh                     # rule batches, atomicity, foreign rules kept
cargo build --release --locked --bin wg-helper
sudo bash tests/killswitch-e2e.sh target/release/wg-helper
```

`firewall-namespace.sh` builds as your user and gives privilege only to the
already-built test process inside its own namespace (the test refuses the
host namespace). `killswitch-e2e.sh` builds real WireGuard tunnels for every
combination of IPv4/IPv6 endpoint and nftables/iptables, and checks that
traffic flows, bypasses are rejected, IPv6 Neighbor Discovery survives a
flushed neighbor cache, an administrator's deny rule still applies, and live
config updates keep the fwmark, listen port and kill switch. It needs
`wireguard-tools`, `nftables`, `iptables`, `iproute2` and `ping`.

One managed switch is allowed; nft updates are one batch, iptables families
are preflighted and committed separately. Preserve failure reporting and
protection when the stop state is unknown. **Never** run privileged
integration tests against real production tunnels or a shared host firewall.

### Installer, packaging and SELinux

| Script | What it proves |
|---|---|
| `tests/installer-sanity.sh` | The installer's argument parser, in isolation (never runs package or privilege actions) |
| `tests/install-integration.sh <image> <bin-dir>` | Install, upgrade, rollback and uninstall in a throwaway distro container |
| `tests/install-packaging-lint.sh [arch\|fedora\|alpine\|void\|all] [--build]` | The distro recipes, checked with each distro's own tools |
| `tests/package-build.sh arch\|rpm\|apk\|void` | Builds the real package from the checkout, runs its tests, installs and smoke-tests it |
| `tests/selinux-vm.sh --binaries=DIR` | A real enforcing Fedora VM: helper label, fcontext rule, AVC denials, grants, rollback |

### Screenshots

The images in `docs/images/` are rendered from the real UI code with
fictional tunnels (`src/ui/screenshots.rs`, an ignored test). Regenerate them
with `scripts/render-screenshots.sh`.

### Release checks

| Script | Purpose |
|---|---|
| `scripts/check-version.sh [--tag vX.Y.Z]` | Cargo.toml, Cargo.lock, the top CHANGELOG release, packaging versions (and the tag) agree |
| `tests/release-regression.sh APP HELPER` | Pins the CLI contract: `--version`/`--help` format, exit codes, helper rejections |
| `scripts/stage-tarball.sh VERSION ARCH [DIR]` | Builds a reproducible release tarball (fixed modes, root ownership) |
| `scripts/verify-release.sh --tag vX.Y.Z DIR` | Verifies signature, checksums, archive contents, linkage and embedded version |
| `scripts/apply-repo-settings.sh [--dry-run] OWNER/REPO` | Branch rulesets and repository policy ([docs/REPO_SETTINGS.md](docs/REPO_SETTINGS.md)) |

Verify a published release yourself with
`scripts/verify-release.sh --tag vX.Y.Z --download` (needs `gh`, `minisign`
and `python3`; `upx` and `dpkg-deb` enable the binary and package checks).
Bumping the version means updating Cargo.toml, Cargo.lock
(`cargo update -w`), the CHANGELOG heading and the four packaging recipes
together; CI's "Version consistency" check fails otherwise. The full release
procedure is in [docs/RELEASE_CHECKLIST.md](docs/RELEASE_CHECKLIST.md).

### Onboarding test

`bash tests/dev-onboarding.sh` clones the repository into a temporary
directory and runs `scripts/dev-setup.sh` plus every command in the
[Required checks](#-required-checks) block, then fails if any command fails or
leaves the checkout dirty. Use `--from-worktree` to include uncommitted
changes. It runs in CI (`.github/workflows/onboarding.yml`) whenever this
file, the scripts or the build configuration change, and weekly, so these
instructions stay honest.

<details>
<summary>🔬 Audit regression expectations (what the tests must keep covering)</summary>

- Test all repeated fields and the exact config schema; use canonical key
  fixtures, not merely 44 characters with an equals sign. Exercise the
  supported Table/FwMark/SaveConfig/IPv6 variants as well as invalid controls,
  CIDRs and keepalive bounds. Keep restore preflight and actual saves aligned
  through the shared validator.
- Regression tests cover failed/partial exports preserving old files, other
  hardlink names, symlink directories and linked backups, hidden ZIP
  duplicates, QR grid selection/limits, runtime-only peers, bounded
  output/blocked input, escaped pipe holders and PID lifetime.
- The helper serializes mutations with a 2-second lock deadline and refuses
  unsafe root-managed file types/ownership.
- Keep the child leader unreaped until pipe work finishes if cleanup may
  signal its process group; otherwise PID reuse could target an unrelated
  process.
- Avoid fixed test counts in documentation; they go stale.

</details>

## 📮 Reports and pull requests

For a bug report, include the distro, version/commit, `wg-tui --version`, the
terminal and `$TERM`, reproduction steps and the relevant (redacted) errors.
In a PR, explain the behaviour before and after, and list the checks you
actually ran. Never include private keys, preshared keys or production
configs. Exploitable issues go through the private channels in
[SECURITY.md](SECURITY.md), not a public issue.

By contributing you agree to the [MIT license](LICENSE). Please follow the
[Code of Conduct](CODE_OF_CONDUCT.md), and see
[Accessibility](ACCESSIBILITY.md) for interaction options, known limits and
how to report a barrier. This project is independent of the WireGuard project.

Thanks for helping out — every fix, test and typo patch counts. 💛
