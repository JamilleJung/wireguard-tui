# RHEL Resolver Support Design

**Date:** 2026-07-27
**Status:** Approved for planning
**Target:** `wireguard-tui` doctor, setup, installer, activation UX, RPM guidance, and documentation

## Problem

`wg-quick` invokes a command named `resolvconf` whenever a tunnel contains an
`[Interface]` `DNS =` directive. The project currently assumes that either of
these conditions is sufficient:

- a `resolvconf` executable exists; or
- `/run/systemd/resolve` exists.

That assumption fails on current RHEL-family systems. On the observed Rocky
Linux 10 host:

- the `systemd-resolved` RPM supplies `/usr/sbin/resolvconf` as a symlink to
  `/usr/bin/resolvectl`;
- `systemd-resolved.service` is installed but disabled and inactive;
- `resolvectl` cannot activate `org.freedesktop.resolve1`;
- `wg-tui doctor` incorrectly reports `resolvconf available`;
- `wg-quick up` creates the WireGuard link, applies its address and MTU, then
  fails when it tries to register DNS;
- the TUI puts this unrecognised multiline error in its one-line footer, hiding
  the actionable end of the error.

The current fallback recommendation also suggests `openresolv` for `dnf`/`yum`.
The enabled Rocky Linux 10 repositories used for this investigation provide
`systemd-resolved`, but not `openresolv` or `systemd-resolvconf`.

## Goals

- Detect whether the installed `resolvconf` command is operational for
  `wg-quick`, based on capabilities rather than distro names alone.
- Correctly diagnose the inactive `resolvectl` compatibility shim used on
  RHEL, Rocky Linux, AlmaLinux, CentOS Stream, and related systems.
- Support both recovery paths requested by the user:
  - a confirmation-based automatic repair through `wg-tui setup`; and
  - a copy-pasteable manual repair command in `doctor` and activation errors.
- Keep `doctor` strictly read-only.
- Never enable a network service silently from `install.sh`.
- Preserve existing native `resolvconf` behavior on Debian, Ubuntu, Arch,
  Alpine, Void, and openSUSE.
- Prevent long activation failures from being truncated in the footer.
- Add automated regression coverage and update distro, installer, packaging,
  and troubleshooting guidance.
- Deliver the work as a reviewable feature branch and draft pull request.

## Non-goals

- Replace `wg-quick` or implement a project-specific DNS manager.
- Change NetworkManager configuration or rewrite `/etc/resolv.conf`.
- Enable `systemd-resolved` without an explicit confirmation.
- Make a DNS provider a hard runtime dependency for tunnels that do not contain
  `DNS =`.
- Change WireGuard routing, firewall, key handling, or privilege boundaries.

## Resolver Capability Model

Introduce a small resolver-state model in `src/doctor.rs`:

```rust
enum DnsResolverState {
    NativeResolvconf,
    ResolvectlActive,
    ResolvectlInactive,
    Missing,
}
```

Production detection gathers these facts:

1. Resolve the `resolvconf` executable using the same standard paths already
   searched by `which`.
2. Canonicalize the executable when possible.
3. Treat it as the systemd compatibility shim when the executable or canonical
   target basename is `resolvectl`.
4. For that shim, check `systemd-resolved.service` with a read-only
   `systemctl is-active --quiet` probe. When `systemctl` is unavailable, fall
   back to checking `/run/systemd/resolve`.
5. Treat a non-`resolvectl` executable as a native provider.
6. Treat an absent executable as missing even when systemd-resolved happens to
   be active, because `wg-quick` still executes the command named
   `resolvconf`.

Classification and user-facing policy will be separated. Pure functions will
accept discovered facts and return state, status text, and repair actions so
the important matrix can be tested without changing the test host.

## Doctor Behavior

The DNS check in `wg-tui doctor` will report:

| State | Status | Detail | Repair |
|---|---|---|---|
| `NativeResolvconf` | `OK` | native resolvconf provider available | none |
| `ResolvectlActive` | `OK` | resolvectl shim with systemd-resolved active | none |
| `ResolvectlInactive` | `WARN` | resolvectl shim installed but service inactive | `sudo systemctl enable --now systemd-resolved` |
| `Missing` | `WARN` | resolvconf compatibility command missing | package-manager-specific install action |

The repair command will use the existing privilege selection (`root`, usable
`sudo`, or `su root -c`) rather than assuming every user has `sudo`.

For `dnf`/`yum`, the missing-provider action is to install
`systemd-resolved`, followed by explicit activation. Apt uses
`systemd-resolvconf` when systemd-resolved is already active and `openresolv`
otherwise. Pacman, zypper, apk, xbps, and eopkg continue to use `openresolv`.

## Setup Behavior

`wg-tui setup` remains guided and confirmation-based:

- `ResolvectlInactive`: explain why `DNS =` tunnels fail, show the exact
  `systemctl enable --now systemd-resolved` action, ask for confirmation, and
  run it through the existing root execution path only after approval.
- `Missing` on `dnf`/`yum`: offer to install `systemd-resolved`; after a
  successful install, separately explain and confirm service activation.
- `Missing` on other supported package managers: retain the provider
  installation flow, using the corrected package policy.
- Working states: take no action.

After an accepted repair, setup re-runs resolver detection and reports the
result. A failed or declined repair always leaves the manual command visible.

## Installer Behavior

`install.sh` will use the same conceptual state matrix in shell:

- a native provider is accepted;
- an active `resolvectl` shim is accepted;
- an inactive shim is not reported as working;
- a missing provider uses the correct package for the detected manager.

The installer continues to install a missing provider package as it does today,
but it will not enable `systemd-resolved` silently. For an inactive shim it
will print a clear warning and the exact
`systemctl enable --now systemd-resolved` command, then point to `wg-tui setup`
for the confirmation-based repair.

Installer checks must stay non-blocking for unattended package/build flows.

## Activation Error UX

Extend `doctor::friendly_error` to recognise resolver failures including:

- `org.freedesktop.resolve1`;
- failure to activate the remote resolver peer;
- failure to set DNS through `resolvectl` or systemd-resolved;
- the existing missing-`resolvconf` error.

Recognised errors will explain that the tunnel's `DNS =` directive reached an
inactive resolver shim and include the manual repair command.

All activation failures, recognised or not, will be displayed in the existing
wrapped message popup instead of the single-line footer. This preserves the
full helper error for unusual failures while keeping the helper's existing
private-key and preshared-key redaction boundary unchanged.

## Packaging and Documentation

- Keep DNS integration optional in RPM packaging because DNS-less tunnels do
  not need it.
- Add `Recommends: systemd-resolved` to the RPM spec. This keeps DNS support
  optional while installing the RHEL-family compatibility shim by default.
- Correct `README.md`, `docs/DISTROS.md`, `docs/TUTORIAL.md`,
  `docs/RELEASES.md`, `CHANGELOG.md`, and packaging guidance that currently
  describe Fedora/RHEL resolver behavior as already covered or recommend an
  unavailable package.
- Document both automatic and manual recovery paths.
- Explicitly distinguish a native `resolvconf` provider from the
  `resolvectl` compatibility shim.

## Testing Strategy

Use test-driven development for each production behavior.

Rust unit tests will cover:

- native provider classification;
- active `resolvectl` shim classification;
- inactive `resolvectl` shim classification;
- missing compatibility command even when resolved appears active;
- repair command selection for `dnf`/`yum` and apt-family systems;
- friendly mapping of `org.freedesktop.resolve1` and DNS-setting failures;
- preservation of unknown raw errors.

Setup command generation will be tested as pure behavior. Tests will not enable
or disable services on the host.

Shell and integration verification will cover:

- installer syntax and the existing installer sanity suite;
- static assertions for inactive-shim handling and RHEL package selection;
- `cargo fmt --check`;
- `cargo test --all-targets`;
- `cargo clippy --all-targets -- -D warnings`;
- the project's existing helper-validation and installer tests;
- a read-only run of the built `wg-tui doctor` on the Rocky Linux host,
  expecting an inactive-shim warning before the host is repaired.

## Security and Operational Safety

- Resolver probes are read-only and need no root.
- `doctor` never mutates the host.
- `setup` requires a visible confirmation before any package install or
  `systemctl enable --now` action.
- `install.sh` does not silently enable the resolver service.
- No test changes resolver state, `/etc/resolv.conf`, NetworkManager, tunnel
  state, or route state.
- Activation errors continue to pass through helper-side secret redaction.

## Acceptance Criteria

- On the observed Rocky Linux 10 state, `wg-tui doctor` no longer reports DNS
  support as OK; it reports the inactive shim and the correct repair command.
- `wg-tui setup` can repair that state after explicit confirmation.
- Declining the repair leaves the host untouched and prints the manual command.
- A tunnel with `DNS =` gets an actionable wrapped popup when resolver
  activation fails.
- Native resolvconf providers continue to pass the doctor check.
- Missing providers receive the package guidance defined in the Doctor
  Behavior package policy.
- All Rust and shell verification passes.
- The final draft PR documents the reproduced root cause, behavior matrix,
  safety decisions, and verification evidence.
