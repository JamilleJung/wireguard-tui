# Contributing to wireguard-tui

Bug reports and focused pull requests are welcome. The current release is
1.9.3; use [CHANGELOG.md](CHANGELOG.md) for user-visible changes.

## Set up and run

```sh
git clone https://github.com/JamilleJung/wireguard-tui.git
cd wireguard-tui
cargo build --release --locked
cargo run --release --locked --bin wg-tui -- --help
```

Use Rust 1.88 or newer (edition 2024; CI's "MSRV" job enforces the minimum,
current stable is recommended), a C linker/toolchain, and Linux for runtime
integration checks. `scripts/dev-setup.sh` checks all of this and prints the
install command for anything missing. The TUI has no graphical toolkit requirement.
`wg` and `wg-quick` come from `wireguard-tools`; an editor, helper authorization,
and suitable DNS/firewall tools are runtime concerns. `./install.sh` installs
these prerequisites and the built app/helper; see [README.md](README.md).
The source installer creates the `wireguard-tui` alias; it is not a second Cargo
binary. The two Cargo binaries are `wg-tui` and `wg-helper`.

Running the UI operates on real `/etc/wireguard` tunnels. Use a disposable Linux
VM for actions that change networking, firewall rules, or authorization.
Platform-independent tests can also run on other Unix hosts; that does not make
those hosts supported WireGuard management targets.

## Required checks

These match the CI quality gates:

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

Run `target/release/wg-tui doctor` separately: `0`, `1`, and `2` are intentional
results depending on the host's prerequisites. `setup` also returns the
refreshed doctor result. Neither is an unconditional success-only smoke test.
Install and run the audit gate when changing dependencies:

```sh
cargo install cargo-audit --locked
cargo audit --deny warnings
```

The current locked graph needs no advisory exceptions. Do not mask a new audit
finding to make CI green. Investigate dependency updates and explain any
unavoidable upstream limitation in the change description.

## Code map

| Path | Purpose |
|---|---|
| `src/main.rs` | Event loop, context-sensitive keys, views, editor and setup flows |
| `src/backend.rs` | Helper client, status, diagnosis, QR, export and backup preflight |
| `src/bin/wg-helper.rs` | Privileged fixed verbs, writes, timeouts and firewall rules |
| `src/config.rs` | Config parsing and frontend validation, including inline comments |
| `src/validation.rs` | Tunnel names and safe import-name generation |
| `src/create.rs` | Interface/full/split tunnel templates |
| `src/clipboard.rs` | Native clipboard tools, OSC 52 fallback and field normalization |
| `src/secrets.rs` | Redaction and root script-hook detection |
| `src/doctor.rs`, `src/update.rs` | Read-only prerequisite checks and signed updates |
| `tests/` | Helper negative tests and installer sanity checks |
| `packaging/`, `.github/workflows/` | Distro recipes, assets, CI and signed release jobs |
| `docs/help.md`, `CHANGELOG.md` | Text embedded into the app by `include_str!` |

## Implementation guidelines

Keep the diff focused and add meaningful regression tests for changed behavior.
Existing tests cover config/name validation, backup preflight/limits, secret
redaction, clipboard normalization, diagnostics, updater checks, large command
output/timeouts, and exact firewall rule names/order. Avoid stale fixed test
counts in documentation. Update the README/tutorial when behavior changes and
add a changelog entry under `[Unreleased]` for future work.

All keys work without a mode switch. The "Advanced" help group is descriptive;
there is no `is_advanced_key()` or saved Easy/Advanced setting to update. Match
keys to `handle_key()` and `render_help()` in `src/main.rs`, including Log-tab
conflicts (`g`, `c`, `w`, `Enter`) and popup consumption. Update footer hints and
help when adding a binding. Use `run_editor()` for raw editing; preserve terminal
suspend/restore, private temporary files, validation, and cleanup. The TUI does
not use the desktop client's structured form editor.

Keep helper changes inside its restricted boundary:

- Fixed verbs, `/etc/wireguard` destinations, and trusted tool search directories
  `/usr/sbin:/usr/bin:/sbin:/bin`; privileged children receive fixed PATH and
  `LC_ALL=C`.
- Validate names before filesystem operations: ASCII alphanumeric first,
  letters/digits/`_`/`-`/`.` thereafter, at most 15 bytes, no `..` or `.conf` suffix.
- Use argv arrays for helper commands. Preserve bounded process groups and
  concurrent pipe draining so output cannot fill a pipe and hang the timeout.
- Revalidate saved/renamed configs and reject root script hooks inside the
  helper, even when the frontend already checked them.
- Preserve exclusive `0600` temp creation, file `sync_all()`, atomic rename,
  directory sync, and timestamped backups before overwrite/rename/delete.
- Log mutation metadata without private or preshared key material.
- Preserve exact tunnel-name firewall comments, reject-before-existing-allow
  placement, IPv6 coverage checks, and cleanup paths.
- Keep backup restore preflight bounded and updater verification anchored to
  the compiled-in key; do not trust a key shipped inside the download.

For a debug helper override:

```sh
cargo build --locked --bin wg-helper
WG_HELPER=/absolute/path/to/wg-helper cargo run --locked --bin wg-tui
```

Debug builds accept `WG_HELPER` directly. Release builds require
`WG_ALLOW_UNSAFE_HELPER=1`, an absolute root-owned executable, no group/other
write bits, and trusted directory ancestry. This override does not create a sudoers/polkit grant;
installed-path authorization still applies. Prefer installed standard paths for
normal use.

## Testing infrastructure

### Property tests and fixtures

`tests/prop_*.rs` are dependency-free property tests. The binaries have no
library crate, so each test pulls the module it checks straight from `src/`
with `#[path = "../src/config.rs"] mod config;` (the helper uses the same
technique). A small deterministic PRNG in `tests/prop_support/` produces the
inputs; a failure prints the seed and case to replay it:

```sh
cargo test --locked --test 'prop_*'                       # default 2000 cases each
PROP_CASES=50000 cargo test --locked --release --test prop_config
PROP_SEED=0x1234 cargo test --locked --test prop_secrets  # another sequence
```

Invariants covered: parsing and validation never panic on arbitrary or
mutated input; anything accepted satisfies what its consumers rely on (valid
keys, peers with AllowedIPs, well-formed endpoints); random wg-quick
configs are accepted and parse/render round-trips are stable; LF and CRLF
give the same verdict; no PrivateKey/PresharedKey value survives redaction
in any casing, spacing or log-prefix form, while other lines stay unchanged;
every `src/create.rs` template validates once completed; sanitized import
names are always valid and idempotent; and the frontend name validator agrees
with the built `wg-helper` on every random name.

`tests/fixtures/configs/` holds **fake** sample configs (`valid/`, `hooks/`,
`invalid/`) for tests, fuzzing and manual experiments. Keys are derived from
SHA-256 of fixed labels. Never add a real config there; change
`scripts/gen-fixtures.sh` and rerun it instead. CI runs
`scripts/gen-fixtures.sh --check` to prove the files match the script output.

### Fuzzing

`fuzz/` is a standalone cargo-fuzz crate (own workspace and lockfile) with
targets `config_validate`, `redact`, `tunnel_names` and `import_preflight`.
They include the same `src/` modules by path. Running them needs a nightly
toolchain and cargo-fuzz:

```sh
rustup toolchain install nightly
cargo install cargo-fuzz --locked
mkdir -p fuzz/corpus/config_validate
cp tests/fixtures/configs/*/*.conf fuzz/corpus/config_validate/
cargo +nightly fuzz run config_validate -- -max_total_time=300
```

CI checks that the targets compile on stable for every pull request ("Fuzz
crate check") and fuzzes each target for five minutes nightly
(`.github/workflows/fuzz.yml`, crash reproducers are uploaded as artifacts).
The ZIP backup and QR container parsers are private functions in the
backend and are not reachable without a library crate; the fuzz targets cover
the validation every imported config passes through.

### Mutation testing

`cargo mutants` checks that the tests notice changes to validation and
security-critical code: `src/validation.rs`, `src/config.rs`,
`src/secrets.rs` and the helper under `src/bin/wg-helper/`. It runs weekly
and on demand (`.github/workflows/mutants.yml`, four shards, report in the job
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

### Release checks

| Script | Purpose |
|---|---|
| `scripts/check-version.sh [--tag vX.Y.Z]` | Cargo.toml, Cargo.lock, top CHANGELOG release, packaging versions (and tag) agree |
| `tests/release-regression.sh APP HELPER` | Pins the CLI contract: `--version`/`--help` format, exit codes, helper rejections |
| `scripts/stage-tarball.sh VERSION ARCH [DIR]` | Builds a reproducible release tarball (fixed modes, root ownership) |
| `scripts/verify-release.sh --tag vX.Y.Z DIR` | Verifies signature, checksums, archive contents, linkage and embedded version |
| `scripts/apply-repo-settings.sh [--dry-run] OWNER/REPO` | Branch rulesets and repository policy ([docs/REPO_SETTINGS.md](docs/REPO_SETTINGS.md)) |

Verify a published release yourself with
`scripts/verify-release.sh --tag vX.Y.Z --download` (needs `gh`, `minisign`,
`python3`; `upx` and `dpkg-deb` enable the binary and package checks).
Bumping the version means updating Cargo.toml, Cargo.lock (`cargo update -w`),
the CHANGELOG heading and the four packaging recipes together; CI's "Version
consistency" check fails otherwise.

### Onboarding test

`bash tests/dev-onboarding.sh` clones the repository into a temporary
directory and runs `scripts/dev-setup.sh` plus every command in the "Required
checks" block above, then fails if any command fails or leaves the checkout
dirty. Use `--from-worktree` to include uncommitted changes. It runs in CI
(`.github/workflows/onboarding.yml`) whenever this file, the scripts or the
build configuration change, and weekly, so these instructions stay accurate.

## Audit regression expectations

Test all repeated fields and exact config schema; use canonical key fixtures,
not merely 44 characters with an equals sign. Exercise supported Table/FwMark/
SaveConfig/IPv6 variants as well as invalid controls, CIDRs and keepalive bounds.
Keep restore preflight and actual saves aligned through the shared validator.

Regression tests cover failed/partial exports preserving old files, other
hardlink names, symlink directories and linked backups, hidden ZIP duplicates,
QR grid selection/limits, runtime-only peers, bounded output/blocked input,
escaped pipe holders and PID lifetime. The helper serializes mutations with a
2-second lock deadline and refuses unsafe root-managed file types/ownership.
Keep the child leader unreaped until pipe work finishes if cleanup may signal
its process group; otherwise PID reuse can target an unrelated process.

Firewall argument tests do not replace disposable Linux namespace integration.
One managed switch is allowed; nft updates are one batch, iptables families are
preflighted and committed separately. Preserve failure reporting and protection
when stop state is unknown. Never run privileged integration tests on real
production tunnels or a shared host firewall.

## Reports and pull requests

Include the distro, version/commit, `wg-tui --version`, terminal and `$TERM`,
reproduction steps, and relevant redacted errors. Explain the behavior before
and after the change and list the checks actually run. Do not include private
keys, preshared keys, or production configs. Report exploitable issues privately
using [SECURITY.md](SECURITY.md).

By contributing, you agree to the [MIT license](LICENSE). This project is
independent of the WireGuard project.

Follow the [Code of Conduct](CODE_OF_CONDUCT.md) when participating. See
[Accessibility](ACCESSIBILITY.md) for current interaction options, known limits
and how to report a barrier.
