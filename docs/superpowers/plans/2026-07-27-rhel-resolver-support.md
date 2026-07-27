# RHEL Resolver Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make resolver detection, guided repair, installation guidance, and activation errors work correctly on RHEL-family systems without regressing native `resolvconf` distributions.

**Architecture:** Add a capability-based `DnsResolverState` in `src/doctor.rs`, keeping host probes separate from pure classification and policy functions. `doctor` remains read-only, `setup` performs only confirmed root actions, `install.sh` installs missing packages but never silently enables `systemd-resolved`, and all activation failures use the wrapped popup.

**Tech Stack:** Rust 2024, `std::process::Command`, ratatui, POSIX-oriented Bash, RPM spec metadata, GitHub Actions.

## Global Constraints

- Preserve the existing `wg-quick` integration; do not implement a project-specific DNS manager.
- Do not rewrite `/etc/resolv.conf` or change NetworkManager configuration.
- `doctor` must remain read-only and must not require root.
- Never enable `systemd-resolved` without explicit confirmation.
- `install.sh` must remain non-blocking and must not prompt in unattended flows.
- DNS support remains optional for tunnels without `DNS =`.
- Existing helper-side private-key and preshared-key redaction must remain unchanged.
- No test may change resolver, tunnel, route, firewall, or NetworkManager state.
- Keep support for Debian/Ubuntu, Arch, Alpine, Void, openSUSE, Solus, Fedora, RHEL, Rocky, AlmaLinux, and CentOS Stream.

---

### Task 1: Resolver capability model and doctor policy

**Files:**
- Modify: `src/doctor.rs:74-257`
- Modify: `src/doctor.rs:389-409`
- Test: `src/doctor.rs:510-548`

**Interfaces:**
- Consumes: standard command lookup paths, `systemctl is-active --quiet systemd-resolved.service`, and `/run/systemd/resolve` as the no-systemctl fallback.
- Produces: `pub enum DnsResolverState`, `pub enum DnsSetupAction`, `pub fn dns_resolver_state() -> DnsResolverState`, `pub fn dns_setup_action(DnsResolverState) -> Option<DnsSetupAction>`, `pub fn enable_resolved_command() -> &'static str`, and the existing `dns_ok`, install hint, install command, and system-check APIs with corrected behavior.

- [ ] **Step 1: Add failing classification and setup-policy tests**

Add these tests to `src/doctor.rs`:

```rust
#[test]
fn classifies_dns_resolver_capabilities() {
    assert_eq!(
        classify_dns_resolver(None, false),
        DnsResolverState::Missing
    );
    assert_eq!(
        classify_dns_resolver(Some(Path::new("/usr/sbin/openresolv")), false),
        DnsResolverState::NativeResolvconf
    );
    assert_eq!(
        classify_dns_resolver(Some(Path::new("/usr/bin/resolvectl")), true),
        DnsResolverState::ResolvectlActive
    );
    assert_eq!(
        classify_dns_resolver(Some(Path::new("/usr/bin/resolvectl")), false),
        DnsResolverState::ResolvectlInactive
    );
}

#[test]
fn missing_resolvconf_is_missing_even_when_resolved_is_active() {
    assert_eq!(
        classify_dns_resolver(None, true),
        DnsResolverState::Missing
    );
}

#[test]
fn resolver_setup_action_matches_state() {
    assert_eq!(
        dns_setup_action(DnsResolverState::Missing),
        Some(DnsSetupAction::InstallProvider)
    );
    assert_eq!(
        dns_setup_action(DnsResolverState::ResolvectlInactive),
        Some(DnsSetupAction::EnableResolved)
    );
    assert_eq!(
        dns_setup_action(DnsResolverState::NativeResolvconf),
        None
    );
    assert_eq!(
        dns_setup_action(DnsResolverState::ResolvectlActive),
        None
    );
}
```

- [ ] **Step 2: Run the tests and verify RED**

Run:

```bash
cargo test --bin wg-tui doctor::tests::classifies_dns_resolver_capabilities -- --exact
```

Expected: compilation fails because `DnsResolverState`, `classify_dns_resolver`, and `dns_setup_action` do not exist.

- [ ] **Step 3: Implement command lookup, state classification, and host probing**

Replace the boolean-only lookup/detection with:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DnsResolverState {
    NativeResolvconf,
    ResolvectlActive,
    ResolvectlInactive,
    Missing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DnsSetupAction {
    InstallProvider,
    EnableResolved,
}

fn command_path(cmd: &str) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    for dir in ["/usr/bin", "/usr/sbin", "/bin", "/sbin", "/usr/local/bin"] {
        dirs.push(PathBuf::from(dir));
    }
    dirs.into_iter().map(|dir| dir.join(cmd)).find(|p| p.exists())
}

pub fn which(cmd: &str) -> bool {
    command_path(cmd).is_some()
}

fn classify_dns_resolver(
    canonical_resolvconf: Option<&std::path::Path>,
    resolved_active: bool,
) -> DnsResolverState {
    let Some(path) = canonical_resolvconf else {
        return DnsResolverState::Missing;
    };
    if path.file_name().is_some_and(|name| name == "resolvectl") {
        if resolved_active {
            DnsResolverState::ResolvectlActive
        } else {
            DnsResolverState::ResolvectlInactive
        }
    } else {
        DnsResolverState::NativeResolvconf
    }
}

fn systemd_resolved_active() -> bool {
    if let Some(systemctl) = command_path("systemctl") {
        return Command::new(systemctl)
            .args(["is-active", "--quiet", "systemd-resolved.service"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
    }
    std::path::Path::new("/run/systemd/resolve").is_dir()
}

pub fn dns_resolver_state() -> DnsResolverState {
    let canonical = command_path("resolvconf").map(|path| {
        std::fs::canonicalize(&path).unwrap_or(path)
    });
    classify_dns_resolver(canonical.as_deref(), systemd_resolved_active())
}

pub fn dns_setup_action(state: DnsResolverState) -> Option<DnsSetupAction> {
    match state {
        DnsResolverState::Missing => Some(DnsSetupAction::InstallProvider),
        DnsResolverState::ResolvectlInactive => Some(DnsSetupAction::EnableResolved),
        DnsResolverState::NativeResolvconf | DnsResolverState::ResolvectlActive => None,
    }
}
```

Implement `dns_ok()` as a match on `dns_resolver_state()` so only
`NativeResolvconf` and `ResolvectlActive` return true.

- [ ] **Step 4: Add failing package-policy tests**

Add:

```rust
#[test]
fn resolver_package_policy_matches_package_manager() {
    assert_eq!(resolvconf_pkg_for(Some("dnf"), false), "systemd-resolved");
    assert_eq!(resolvconf_pkg_for(Some("yum"), false), "systemd-resolved");
    assert_eq!(
        resolvconf_pkg_for(Some("apt-get"), true),
        "systemd-resolvconf"
    );
    assert_eq!(resolvconf_pkg_for(Some("apt-get"), false), "openresolv");
    assert_eq!(resolvconf_pkg_for(Some("pacman"), false), "openresolv");
}
```

Run:

```bash
cargo test --bin wg-tui doctor::tests::resolver_package_policy_matches_package_manager -- --exact
```

Expected: compilation fails because `resolvconf_pkg_for` does not exist.

- [ ] **Step 5: Implement package and manual-repair policy**

Add the pure package selector and public repair helpers:

```rust
fn resolvconf_pkg_for(
    manager: Option<&str>,
    resolved_active: bool,
) -> &'static str {
    match manager {
        Some("dnf" | "yum") => "systemd-resolved",
        Some("apt-get") if resolved_active => "systemd-resolvconf",
        _ => "openresolv",
    }
}

fn resolvconf_pkg() -> &'static str {
    resolvconf_pkg_for(pkg_manager(), systemd_resolved_active())
}

pub fn enable_resolved_command() -> &'static str {
    "systemctl enable --now systemd-resolved"
}

pub fn enable_resolved_hint() -> String {
    with_priv(enable_resolved_command())
}
```

Keep `install_resolvconf_hint()` and `install_resolvconf_command()` wired to
`resolvconf_pkg()`.

- [ ] **Step 6: Add a failing doctor-check matrix test**

Extract the DNS check construction into `fn dns_check(state:
DnsResolverState) -> Check`, then add:

```rust
#[test]
fn inactive_resolvectl_is_a_warning_with_enable_hint() {
    let check = dns_check(DnsResolverState::ResolvectlInactive);
    assert_eq!(check.status, Status::Warning);
    assert!(check.detail.contains("inactive"));
    assert!(
        check.fix
            .as_deref()
            .is_some_and(|fix| fix.contains("systemctl enable --now systemd-resolved"))
    );
}
```

Run:

```bash
cargo test --bin wg-tui doctor::tests::inactive_resolvectl_is_a_warning_with_enable_hint -- --exact
```

Expected: compilation fails because `dns_check` does not exist.

- [ ] **Step 7: Implement the doctor state matrix**

Make `dns_check` return:

```rust
match state {
    DnsResolverState::NativeResolvconf => (
        Status::Ok,
        "native resolvconf provider available".to_string(),
        None,
    ),
    DnsResolverState::ResolvectlActive => (
        Status::Ok,
        "resolvectl shim available; systemd-resolved is active".to_string(),
        None,
    ),
    DnsResolverState::ResolvectlInactive => (
        Status::Warning,
        "resolvectl shim is installed, but systemd-resolved is inactive".to_string(),
        Some(enable_resolved_hint()),
    ),
    DnsResolverState::Missing => (
        Status::Warning,
        "resolvconf compatibility command is missing; tunnels with 'DNS =' will fail"
            .to_string(),
        Some(install_resolvconf_hint()),
    ),
}
```

Use this check in `system_check()` and keep it non-critical.

- [ ] **Step 8: Run the focused and full Rust tests**

Run:

```bash
cargo test --bin wg-tui doctor::tests
cargo test
```

Expected: all tests pass.

- [ ] **Step 9: Commit Task 1**

```bash
git add src/doctor.rs
git commit -m "fix: detect inactive resolvectl shims"
```

---

### Task 2: Confirmation-based resolver repair in setup

**Files:**
- Modify: `src/main.rs:477-585`
- Test: `src/doctor.rs` setup-action tests from Task 1

**Interfaces:**
- Consumes: `doctor::dns_resolver_state`, `doctor::dns_setup_action`, `doctor::DnsResolverState`, `doctor::DnsSetupAction`, `doctor::enable_resolved_command`, and `doctor::enable_resolved_hint`.
- Produces: `offer_root_action(label: &str, cmd: &str, hint: &str) -> bool` and a setup sequence that can install a missing provider and then separately offer to enable an installed inactive shim.

- [ ] **Step 1: Refactor the confirmation runner to return success**

Rename `offer_root_install` to `offer_root_action`, change its return type to
`bool`, and use generic action copy:

```rust
fn offer_root_action(label: &str, cmd: &str, hint: &str) -> bool {
    use std::io::Write as _;

    let method = if unsafe { libc::geteuid() } == 0 {
        "root"
    } else if doctor::can_sudo() {
        "sudo"
    } else if doctor::which("su") {
        "su"
    } else {
        "none"
    };
    if method == "none" {
        println!("{label} can't be changed automatically (neither sudo nor su is available).");
        println!("Run it as root, e.g.: {hint}");
        return false;
    }

    let shown = match method {
        "root" => format!("sh -c '{cmd}'"),
        "sudo" => format!("sudo sh -c '{cmd}'"),
        _ => format!("su root -c '{cmd}'   (asks for the ROOT password)"),
    };
    println!("{label} can be changed with:");
    println!("    {shown}");
    print!("Run now? [y/N] ");
    let _ = std::io::stdout().flush();

    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err()
        || !line.trim().eq_ignore_ascii_case("y")
    {
        println!("Skipped. Run manually: {hint}");
        return false;
    }

    let status = match method {
        "root" => std::process::Command::new("sh").arg("-c").arg(cmd).status(),
        "sudo" => std::process::Command::new("sudo")
            .arg("sh")
            .arg("-c")
            .arg(cmd)
            .status(),
        _ => std::process::Command::new("su")
            .arg("root")
            .arg("-c")
            .arg(cmd)
            .status(),
    };
    match status {
        Ok(status) if status.success() => {
            println!("Done.");
            true
        }
        _ => {
            println!("Couldn't complete the action. Run it manually: {hint}");
            false
        }
    }
}
```

Use `Run now? [y/N]`, `Done.`, `Couldn't complete the action`, and
`Skipped. Run manually: {hint}` so the function works for both package
installation and service activation.

- [ ] **Step 2: Compile after the signature change**

Run:

```bash
cargo check --bin wg-tui
```

Expected: compilation fails at the old `offer_root_install` call sites until
they are updated to `offer_root_action`.

- [ ] **Step 3: Update existing package-install call sites**

Change the WireGuard-tools and resolver-provider call sites to:

```rust
let _ = offer_root_action(
    "WireGuard tools",
    &cmd,
    &doctor::install_tools_hint(),
);
```

and:

```rust
let installed = offer_root_action(
    "A resolvconf provider",
    &cmd,
    &doctor::install_resolvconf_hint(),
);
```

- [ ] **Step 4: Implement the two-stage resolver setup flow**

Add a helper that matches the current state:

```rust
fn offer_enable_resolved() -> bool {
    println!(
        "The resolvectl compatibility shim needs systemd-resolved to be active."
    );
    offer_root_action(
        "systemd-resolved",
        doctor::enable_resolved_command(),
        &doctor::enable_resolved_hint(),
    )
}
```

In `run_setup()`:

```rust
match doctor::dns_setup_action(doctor::dns_resolver_state()) {
    Some(doctor::DnsSetupAction::EnableResolved) => {
        let _ = offer_enable_resolved();
        println!();
    }
    Some(doctor::DnsSetupAction::InstallProvider) => {
        if let Some(cmd) = doctor::install_resolvconf_command() {
            println!(
                "Tunnels with a 'DNS =' line need a working resolvconf compatibility command."
            );
            let installed = offer_root_action(
                "A resolvconf provider",
                &cmd,
                &doctor::install_resolvconf_hint(),
            );
            if installed
                && matches!(
                    doctor::dns_resolver_state(),
                    doctor::DnsResolverState::ResolvectlInactive
                )
            {
                let _ = offer_enable_resolved();
            }
        }
        println!();
    }
    None => {}
}
```

Update comments and final copy to say setup never connects tunnels or enables
**tunnel** start-on-boot. Keep the final `doctor::system_check()` re-check.

- [ ] **Step 5: Verify setup compiles and existing tests stay green**

Run:

```bash
cargo fmt --all
cargo test --bin wg-tui
```

Expected: all tests pass.

- [ ] **Step 6: Commit Task 2**

```bash
git add src/main.rs
git commit -m "fix: guide RHEL resolver activation in setup"
```

---

### Task 3: Actionable resolver errors and wrapped activation failures

**Files:**
- Modify: `src/doctor.rs:246-257`
- Modify: `src/main.rs:1278-1329`
- Test: `src/doctor.rs:510-548`
- Test: `src/main.rs` new `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes: `doctor::enable_resolved_hint()` and the backend's already-redacted error text.
- Produces: resolver-aware `doctor::friendly_error` and `activation_failure_message(name: &str, raw: &str) -> String`.

- [ ] **Step 1: Add failing resolver-error tests**

Add to the doctor tests:

```rust
#[test]
fn friendly_error_explains_inactive_systemd_resolved() {
    let raw = "resolvconf -a Drite -m 0 -x: Could not activate remote peer \
               'org.freedesktop.resolve1': activation request failed: unknown unit";
    let friendly = friendly_error(raw);
    assert!(friendly.contains("systemd-resolved"));
    assert!(friendly.contains("systemctl enable --now systemd-resolved"));
}

#[test]
fn friendly_error_preserves_unknown_errors() {
    let raw = "wg-quick up Drite: RTNETLINK answers: File exists";
    assert_eq!(friendly_error(raw), raw);
}
```

Run:

```bash
cargo test --bin wg-tui doctor::tests::friendly_error_explains_inactive_systemd_resolved -- --exact
```

Expected: the first test fails because the current function returns the raw
resolver error.

- [ ] **Step 2: Implement resolver-error recognition**

Before the existing missing-command branch, recognise:

```rust
let resolved_unavailable = r.contains("org.freedesktop.resolve1")
    || (r.contains("Failed to set DNS configuration")
        && (r.contains("resolvectl") || r.contains("systemd-resolved")));
if resolved_unavailable {
    return format!(
        "this tunnel's 'DNS =' reached the resolvectl shim, but \
         systemd-resolved is unavailable - run: {}",
        enable_resolved_hint()
    );
}
```

Keep the existing missing-`resolvconf` mapping and exact passthrough for
unknown errors.

- [ ] **Step 3: Add a failing popup-message test**

Add a small pure formatter near `toggle_active` and test it:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activation_failure_message_preserves_multiline_unknown_error() {
        let message = activation_failure_message(
            "Drite",
            "wg-quick up Drite:\n[#] ip link add Drite\nRTNETLINK failure",
        );
        assert!(message.starts_with("Couldn't activate Drite:"));
        assert!(message.contains("[#] ip link add Drite\nRTNETLINK failure"));
    }
}
```

Run:

```bash
cargo test --bin wg-tui tests::activation_failure_message_preserves_multiline_unknown_error -- --exact
```

Expected: compilation fails because `activation_failure_message` does not exist.

- [ ] **Step 4: Route every activation failure to the wrapped popup**

Implement:

```rust
fn activation_failure_message(name: &str, raw: &str) -> String {
    format!(
        "Couldn't activate {name}:\n\n{}",
        doctor::friendly_error(raw)
    )
}
```

Replace the footer-vs-popup branch with:

```rust
Err(e) => {
    app.mode = Mode::Message(activation_failure_message(&name, &e));
}
```

The existing `render_message` path already uses `Wrap { trim: false }`; do not
change secret handling or helper output.

- [ ] **Step 5: Run focused and complete tests**

Run:

```bash
cargo fmt --all
cargo test --bin wg-tui doctor::tests
cargo test --bin wg-tui tests::activation_failure_message_preserves_multiline_unknown_error -- --exact
cargo test
```

Expected: all tests pass.

- [ ] **Step 6: Commit Task 3**

```bash
git add src/doctor.rs src/main.rs
git commit -m "fix: show actionable resolver activation errors"
```

---

### Task 4: Installer and RPM support for RHEL-family resolvers

**Files:**
- Modify: `tests/installer-sanity.sh:26-33`
- Modify: `install.sh:267-289`
- Modify: `packaging/rpm/wireguard-tui.spec:17-18`
- Modify: `packaging/PACKAGING.md:26-33`

**Interfaces:**
- Consumes: `resolvconf`, canonical paths from `readlink -f`, `systemctl is-active --quiet`, and the detected package manager in `$PM`.
- Produces: shell helpers `resolvconf_uses_resolvectl`, `systemd_resolved_active`, and `warn_enable_resolved`; RPM `Recommends: systemd-resolved`.

- [ ] **Step 1: Add failing installer-sanity assertions**

Append:

```bash
require 'readlink -f.*resolvconf' 'resolvectl shim detection'
require 'systemctl is-active --quiet systemd-resolved\.service' 'active resolver service probe'
require 'systemctl enable --now systemd-resolved' 'manual inactive-shim recovery'
require 'dnf\|yum\).*systemd-resolved' 'RHEL-family resolver package'
reject 'dnf\|yum\).*openresolv' 'unavailable RHEL-family openresolv package'
```

Run:

```bash
bash tests/installer-sanity.sh
```

Expected: failure at the first new requirement because `install.sh` does not
canonicalize `resolvconf`.

- [ ] **Step 2: Implement shell capability probes**

Add before `ensure_resolvconf`:

```bash
resolvconf_uses_resolvectl() {
    local resolver_path resolver_target
    command -v resolvconf >/dev/null 2>&1 || return 1
    resolver_path="$(command -v resolvconf)"
    resolver_target="$(readlink -f "$resolver_path" 2>/dev/null || printf '%s' "$resolver_path")"
    [ "$(basename "$resolver_target")" = "resolvectl" ]
}

systemd_resolved_active() {
    if command -v systemctl >/dev/null 2>&1; then
        systemctl is-active --quiet systemd-resolved.service
    else
        [ -d /run/systemd/resolve ]
    fi
}

warn_enable_resolved() {
    warn "resolvconf is the resolvectl shim, but systemd-resolved is inactive."
    warn "Run: sudo systemctl enable --now systemd-resolved"
    warn "Or run: wg-tui setup"
}
```

Keep `resolver_path` and `resolver_target` local to prevent global leakage.

- [ ] **Step 3: Correct provider installation and post-install validation**

Rewrite `ensure_resolvconf` so:

```bash
if command -v resolvconf >/dev/null 2>&1; then
    if resolvconf_uses_resolvectl && ! systemd_resolved_active; then
        warn_enable_resolved
    else
        ok "resolvconf provider available."
    fi
    return 0
fi
```

Use this package matrix:

```bash
case "$PM" in
    apt-get)
        if systemd_resolved_active; then
            as_root apt-get install -y systemd-resolvconf || true
        else
            as_root apt-get install -y openresolv || true
        fi
        ;;
    dnf|yum)      as_root "$PM" install -y systemd-resolved || true ;;
    pacman)       as_root pacman -Sy --noconfirm openresolv || true ;;
    zypper)       as_root zypper --non-interactive install openresolv || true ;;
    apk)          as_root apk add openresolv || true ;;
    xbps-install) as_root xbps-install -Sy openresolv || true ;;
    eopkg)        as_root eopkg install -y openresolv || true ;;
esac
```

After installation, distinguish working, inactive shim, and still missing.
Never call `systemctl enable` from the installer.

- [ ] **Step 4: Add the RPM soft dependency and packaging explanation**

Add:

```spec
Recommends:     systemd-resolved
```

Update `packaging/PACKAGING.md` to state that `systemd-resolved` is a soft
dependency for `DNS =` tunnels and must be explicitly enabled when the
`resolvectl` shim is installed but inactive.

- [ ] **Step 5: Run shell and packaging checks**

Run:

```bash
bash -n install.sh tests/installer-sanity.sh
shellcheck -S warning install.sh tests/installer-sanity.sh
bash tests/installer-sanity.sh
rg -n '^Recommends:[[:space:]]+systemd-resolved$' packaging/rpm/wireguard-tui.spec
```

Expected: all commands pass and no shellcheck warning is emitted.

- [ ] **Step 6: Commit Task 4**

```bash
git add install.sh tests/installer-sanity.sh packaging/rpm/wireguard-tui.spec packaging/PACKAGING.md
git commit -m "fix: support RHEL resolver packaging"
```

---

### Task 5: Cross-distro documentation and complete verification

**Files:**
- Modify: `README.md:280-306`
- Modify: `README.md:446-450`
- Modify: `docs/DISTROS.md:69-129`
- Modify: `docs/TUTORIAL.md:387-412`
- Modify: `docs/RELEASES.md:25-32`
- Modify: `docs/help.md:63-64`
- Modify: `CHANGELOG.md:7`

**Interfaces:**
- Consumes: the final resolver state matrix, setup behavior, installer policy, and RPM soft dependency.
- Produces: one consistent explanation of native resolvconf, active/inactive resolvectl shims, automatic confirmed repair, and manual repair.

- [ ] **Step 1: Add an unreleased changelog entry**

Insert before `1.8.0`:

```markdown
## [Unreleased]

### Fixed
- **RHEL-family DNS activation.** Detect when `resolvconf` is the `resolvectl`
  compatibility shim but `systemd-resolved` is inactive; `doctor` now warns,
  `setup` offers a confirmation-based repair, the installer uses the correct
  RPM package, and activation errors are shown in a wrapped popup.
```

- [ ] **Step 2: Correct the distro matrix and troubleshooting guide**

Document these exact state rules in `docs/DISTROS.md` and
`docs/TUTORIAL.md`:

```text
native resolvconf                 -> ready
resolvconf -> resolvectl + active -> ready
resolvconf -> resolvectl + inactive -> run:
  sudo systemctl enable --now systemd-resolved
no resolvconf on dnf/yum          -> install systemd-resolved, then enable it
```

Replace the unavailable RHEL `systemd-resolvconf`/`openresolv` guidance with
`dnf install systemd-resolved`. Keep Debian's `openresolv` and active
systemd-resolved guidance.

- [ ] **Step 3: Align README, release, help, and packaging copy**

Update the named files so they all state:

```markdown
`wg-tui doctor` checks both the `resolvconf` command and, when it is the
`resolvectl` shim, whether `systemd-resolved` is active. `wg-tui setup` can run
the repair after confirmation; `doctor` always prints the manual command.
```

In `docs/RELEASES.md`, clarify that the installer installs the
package-manager-specific DNS compatibility package but does not silently enable
`systemd-resolved`.

- [ ] **Step 4: Run formatting, Rust, shell, and helper gates**

Run exactly the local equivalents of CI:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
bash -n install.sh tests/helper-validation.sh tests/installer-sanity.sh
shellcheck -S warning install.sh tests/helper-validation.sh tests/installer-sanity.sh
bash tests/helper-validation.sh target/release/wg-helper
bash tests/installer-sanity.sh
target/release/wg-tui --version
target/release/wg-tui --help
```

Expected: every command exits zero.

- [ ] **Step 5: Verify doctor read-only on the Rocky Linux host**

Run:

```bash
set +e
doctor_output="$(target/release/wg-tui doctor)"
doctor_rc=$?
set -e
printf '%s\n' "$doctor_output"
test "$doctor_rc" -le 2
printf '%s\n' "$doctor_output" | rg 'resolvectl shim.*inactive'
printf '%s\n' "$doctor_output" | rg 'systemctl enable --now systemd-resolved'
systemctl is-active systemd-resolved.service | rg '^inactive$'
```

Expected: doctor reports the inactive shim and manual repair, returns only
0/1/2, and the final command proves verification did not activate the service.

- [ ] **Step 6: Review the complete diff**

Run:

```bash
git diff --check origin/main...HEAD
git diff --stat origin/main...HEAD
git status --short
```

Expected: no whitespace errors; only the approved resolver support, tests,
packaging, and documentation are changed.

- [ ] **Step 7: Commit Task 5**

```bash
git add README.md docs/DISTROS.md docs/TUTORIAL.md docs/RELEASES.md docs/help.md CHANGELOG.md
git commit -m "docs: explain resolver compatibility states"
```

- [ ] **Step 8: Prepare the draft PR**

Use title:

```text
fix: support inactive resolvectl shims on RHEL
```

Use a body containing:

```markdown
## Summary
- classify native resolvconf and resolvectl compatibility shims by capability
- diagnose and optionally repair inactive systemd-resolved on RHEL-family hosts
- show complete activation failures and correct installer/RPM/documentation guidance

## Root cause
Rocky Linux 10 supplies `resolvconf` as a symlink to `resolvectl`, while
`systemd-resolved.service` is disabled by default. Presence-only detection
reported DNS support as healthy even though `wg-quick` failed immediately
after interface setup.

## Safety
- doctor remains read-only
- setup requires confirmation
- install.sh never silently enables systemd-resolved
- tests do not modify host networking

## Verification
- cargo fmt --all -- --check
- cargo clippy --all-targets -- -D warnings
- cargo test
- cargo build --release
- shellcheck and installer/helper tests
- read-only Rocky Linux doctor probe
```

Push `fix/rhel-resolver-support` over SSH and open a draft pull request against
`JamilleJung/wireguard-tui:main`.
