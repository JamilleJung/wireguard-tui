//! GitHub-based, dependency-light auto-update.
//!
//! The whole feature shells out to programs that are already present at runtime
//! (`curl`, `tar`, `sha256sum`) plus `minisign` for signature verification - no
//! new Rust crates. The trust chain is fail-closed:
//!
//!   1. `SHA256SUMS.minisig` is verified against the *compiled-in* `minisign.pub`
//!      (baked with `include_str!`, so swapping the on-disk key cannot defeat it).
//!   2. The downloaded tarball's digest is checked against the now-trusted
//!      `SHA256SUMS`.
//!   3. Only then is the verified `install.sh` from the tarball re-run; it owns
//!      the privileged writes exactly as the first install did.
//!
//! Network errors / offline simply yield `Err` and the callers treat that as a
//! silent "no update".

use std::path::{Path, PathBuf};
use std::process::Command;

const OWNER: &str = "JamilleJung";
const REPO: &str = "wireguard-tui";
const CURRENT: &str = env!("CARGO_PKG_VERSION");
/// GitHub requires a User-Agent on the REST API.
const UA: &str = concat!("wireguard-tui/", env!("CARGO_PKG_VERSION"));
/// The public key committed to the repo, baked into the binary at build time so
/// it cannot be swapped on disk (never trust the release's own `minisign.pub`).
const MINISIGN_PUB: &str = include_str!("../minisign.pub");

/// A pending update: the running version and the newer published one.
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
}

// ---------------------------------------------------------------------------
// CHECK
// ---------------------------------------------------------------------------

/// One curl call, hard caps so it can never hang the UI.
fn curl(url: &str) -> Result<Vec<u8>, String> {
    let out = Command::new("curl")
        .args([
            "-fsSL",
            "--proto",
            "=https",
            "--tlsv1.2",
            "--max-time",
            "8",
            "--connect-timeout",
            "5",
            "--retry",
            "0",
            "-A",
            UA,
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            "X-GitHub-Api-Version: 2022-11-28",
            url,
        ])
        .output()
        .map_err(|e| format!("curl spawn: {e}"))?;
    if !out.status.success() {
        return Err(format!("curl failed ({})", out.status));
    }
    Ok(out.stdout)
}

/// Latest published tag, e.g. "1.8.0" (leading 'v' stripped).
fn latest_tag() -> Result<String, String> {
    let body = curl(&format!(
        "https://api.github.com/repos/{OWNER}/{REPO}/releases/latest"
    ))?;
    let body = String::from_utf8_lossy(&body);
    extract_json_string(&body, "tag_name")
        .map(|t| t.trim_start_matches('v').to_string())
        .ok_or_else(|| "no tag_name in release JSON".to_string())
}

/// Minimal, dependency-free extraction of a top-level JSON string field
/// `"key":"value"`. Sufficient for tag_name (a flat ASCII value). Handles
/// surrounding whitespace; refuses values containing control/escape chars.
fn extract_json_string(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let mut idx = json.find(&needle)? + needle.len();
    let bytes = json.as_bytes();
    while idx < bytes.len() && (bytes[idx] as char).is_whitespace() {
        idx += 1;
    }
    if idx >= bytes.len() || bytes[idx] != b':' {
        return None;
    }
    idx += 1;
    while idx < bytes.len() && (bytes[idx] as char).is_whitespace() {
        idx += 1;
    }
    if idx >= bytes.len() || bytes[idx] != b'"' {
        return None;
    }
    idx += 1;
    let start = idx;
    while idx < bytes.len() && bytes[idx] != b'"' {
        if bytes[idx] == b'\\' {
            return None; // refuse escapes -> not a plain tag
        }
        idx += 1;
    }
    let val = &json[start..idx];
    if val.is_empty() || val.len() > 32 {
        return None;
    }
    Some(val.to_string())
}

/// Strict semver triple. Release tags are validated `^v[0-9]+\.[0-9]+\.[0-9]+$`
/// by the workflow, so this exact compare avoids the `semver` crate.
fn parse(v: &str) -> Option<(u64, u64, u64)> {
    let mut it = v.trim().trim_start_matches('v').split('.');
    let a = it.next()?.parse().ok()?;
    let b = it.next()?.parse().ok()?;
    let c = it.next()?.parse().ok()?;
    if it.next().is_some() {
        return None; // reject 4+ components
    }
    Some((a, b, c))
}

/// `Some(true)` iff `remote` is strictly newer than the compiled-in version.
/// `None` if either side is unparseable (never offers an update then).
fn is_newer(remote: &str) -> Option<bool> {
    Some(parse(remote)? > parse(CURRENT)?)
}

/// Query GitHub for the latest release. `Ok(None)` when up to date or
/// unparseable; `Err` on any network/curl failure (caller treats as silent).
pub fn check() -> Result<Option<UpdateInfo>, String> {
    let latest = latest_tag()?;
    match is_newer(&latest) {
        Some(true) => Ok(Some(UpdateInfo {
            current: CURRENT.to_string(),
            latest,
        })),
        _ => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// DOWNLOAD + VERIFY
// ---------------------------------------------------------------------------

/// The release tarball name for this host, e.g.
/// `wireguard-tui-1.8.0-x86_64-linux.tar.gz`. Maps `uname -m` to the arch
/// strings the workflow publishes (x86_64 | i686 | aarch64 | armv7).
fn tarball_name(version: &str) -> Result<String, String> {
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "x86" => "i686",
        "aarch64" => "aarch64",
        "arm" => "armv7",
        other => return Err(format!("no release build for this architecture ({other})")),
    };
    Ok(format!("wireguard-tui-{version}-{arch}-linux.tar.gz"))
}

/// Download + verify the latest release into a fresh temp dir, returning the
/// extracted tarball directory (which holds `install.sh` and the binaries).
pub fn download_and_verify() -> Result<PathBuf, String> {
    let version = latest_tag()?;
    let tarball = tarball_name(&version)?;
    let base = format!("https://github.com/{OWNER}/{REPO}/releases/download/v{version}");

    // A throwaway work dir under TMPDIR, owned by us.
    let workdir = std::env::temp_dir().join(format!("wg-tui-update-{version}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&workdir);
    std::fs::create_dir_all(&workdir).map_err(|e| format!("temp dir: {e}"))?;

    // Fetch the tarball + signed checksum manifest. (Never the release's own
    // minisign.pub - we only trust the compiled-in one.)
    for asset in [tarball.as_str(), "SHA256SUMS", "SHA256SUMS.minisig"] {
        download_to(&format!("{base}/{asset}"), &workdir.join(asset))?;
    }

    verify_release(&workdir, &tarball)?;

    // Unpack only after verification succeeds.
    let st = Command::new("tar")
        .args(["-xzf"])
        .arg(workdir.join(&tarball))
        .arg("-C")
        .arg(&workdir)
        .status()
        .map_err(|e| format!("tar spawn: {e}"))?;
    if !st.success() {
        return Err("could not extract the downloaded tarball".into());
    }

    // Tarball top dir == basename without .tar.gz.
    let tardir = workdir.join(tarball.trim_end_matches(".tar.gz"));
    if !tardir.is_dir() {
        return Err("unexpected tarball layout".into());
    }
    Ok(tardir)
}

/// Download `url` to `dest` with the same hard caps as the API call.
fn download_to(url: &str, dest: &Path) -> Result<(), String> {
    let st = Command::new("curl")
        .args([
            "-fsSL",
            "--proto",
            "=https",
            "--tlsv1.2",
            "--max-time",
            "60",
            "--connect-timeout",
            "5",
            "--retry",
            "1",
            "-A",
            UA,
            "-o",
        ])
        .arg(dest)
        .arg(url)
        .status()
        .map_err(|e| format!("curl spawn: {e}"))?;
    if !st.success() {
        return Err(format!("download failed: {url}"));
    }
    Ok(())
}

/// Verify the signed `SHA256SUMS` against the baked-in pubkey, then confirm the
/// tarball's digest is listed in it. Fail-closed: any error refuses the update.
fn verify_release(workdir: &Path, tarball: &str) -> Result<(), String> {
    ensure_minisign()?; // installs via PM once, else Err -> caller refuses to apply

    // 1) Write the *baked-in* pubkey to a temp file (never trust an on-disk one,
    //    and never the minisign.pub from the release - only the compiled-in one).
    let pubfile = workdir.join("trusted.pub");
    std::fs::write(&pubfile, MINISIGN_PUB).map_err(|e| e.to_string())?;

    // 2) Verify SHA256SUMS against the trusted pubkey. minisign exits nonzero on
    //    any mismatch.
    let st = Command::new("minisign")
        .arg("-V")
        .arg("-p")
        .arg(&pubfile)
        .arg("-m")
        .arg(workdir.join("SHA256SUMS"))
        .current_dir(workdir)
        .status()
        .map_err(|e| format!("minisign spawn: {e}"))?;
    if !st.success() {
        return Err("signature verification FAILED - refusing to update".into());
    }

    // 3) Now SHA256SUMS is trusted; verify the tarball's digest is listed in it.
    //    `--ignore-missing` so only our one tarball line must match (SHA256SUMS
    //    lists every asset). Restrict the check to our tarball to avoid spurious
    //    failures on assets we never downloaded.
    let st = Command::new("sha256sum")
        .args(["-c", "--ignore-missing", "--strict"])
        .arg("SHA256SUMS")
        .current_dir(workdir)
        .status()
        .map_err(|e| format!("sha256sum spawn: {e}"))?;
    if !st.success() {
        return Err("checksum mismatch on downloaded tarball - refusing".into());
    }
    // Sanity: the tarball line must actually be present (ignore-missing would
    // otherwise pass an empty check).
    let sums = std::fs::read_to_string(workdir.join("SHA256SUMS")).map_err(|e| e.to_string())?;
    if !sums.lines().any(|l| l.trim_end().ends_with(tarball)) {
        return Err("downloaded tarball is not listed in SHA256SUMS - refusing".into());
    }
    Ok(())
}

/// Ensure the `minisign` CLI is available, attempting a single best-effort
/// install if not. Fail-closed: returns `Err` if it still cannot be found.
fn ensure_minisign() -> Result<(), String> {
    if which("minisign") {
        return Ok(());
    }
    try_install_minisign();
    if which("minisign") {
        Ok(())
    } else {
        Err("`minisign` is not installed and could not be installed; \
             cannot verify the update - aborting (run install.sh to update manually)"
            .into())
    }
}

/// Best-effort single install of `minisign` via the host package manager. Any
/// failure is ignored; `ensure_minisign` re-checks `which` afterwards.
fn try_install_minisign() {
    // Mirror the package managers install.sh supports. Run via the privilege
    // wrapper the user already has set up; if none works, this is a no-op.
    let pm_cmds: &[(&str, &[&str])] = &[
        ("apt-get", &["install", "-y", "minisign"]),
        ("dnf", &["install", "-y", "minisign"]),
        ("yum", &["install", "-y", "minisign"]),
        ("pacman", &["-Sy", "--noconfirm", "--needed", "minisign"]),
        ("zypper", &["--non-interactive", "install", "minisign"]),
        ("apk", &["add", "--no-cache", "minisign"]),
        ("xbps-install", &["-Sy", "minisign"]),
        ("eopkg", &["install", "-y", "minisign"]),
    ];
    let Some((pm, args)) = pm_cmds.iter().find(|(pm, _)| which(pm)) else {
        return;
    };
    let is_root = unsafe { libc::geteuid() } == 0;
    let mut cmd = if is_root {
        let mut c = Command::new(pm);
        c.args(*args);
        c
    } else if which("sudo") {
        let mut c = Command::new("sudo");
        c.arg("-n").arg(pm).args(*args);
        c
    } else {
        return;
    };
    let _ = cmd.status();
}

/// True if `prog` is on PATH (cheap, no shell).
fn which(prog: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|p| p.join(prog).is_file())
    })
}

// ---------------------------------------------------------------------------
// APPLY
// ---------------------------------------------------------------------------

/// Apply a verified update by re-running the tarball's own `install.sh`, which
/// owns the privileged writes and the sudoers/polkit grant exactly as the first
/// install did. The running process is left untouched; the user restarts to use
/// the new binary.
pub fn apply(tardir: &Path) -> Result<(), String> {
    // install.sh reuses target/release/<bin> when WG_USE_PREBUILT=1 is set, so
    // stage the prebuilt, already-verified binaries there to skip any compile.
    let rel = tardir.join("target").join("release");
    std::fs::create_dir_all(&rel).map_err(|e| format!("stage dir: {e}"))?;
    for bin in ["wg-tui", "wg-helper"] {
        let src = tardir.join(bin);
        if src.is_file() {
            std::fs::copy(&src, rel.join(bin)).map_err(|e| format!("stage {bin}: {e}"))?;
        }
    }

    // Run the verified installer as the same user; it self-escalates for the
    // /usr/local writes. --no-desktop: the TUI ships no desktop entry by default.
    let st = Command::new("bash")
        .arg(tardir.join("install.sh"))
        .arg("--no-desktop")
        .env("WG_USE_PREBUILT", "1")
        .current_dir(tardir)
        .status()
        .map_err(|e| format!("install.sh spawn: {e}"))?;
    if !st.success() {
        return Err(format!("install.sh failed ({st})"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// OPT-OUT GATE
// ---------------------------------------------------------------------------

/// Persistent opt-out marker path:
/// $XDG_CONFIG_HOME (or ~/.config)/wireguard-tui/<name>.
fn state_path(name: &str) -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("wireguard-tui").join(name))
}

/// True when the startup update check should be skipped. `WG_NO_UPDATE_CHECK`
/// disables it for one run; a persistent `no-update` file disables it for good.
pub fn disabled() -> bool {
    if std::env::var_os("WG_NO_UPDATE_CHECK").is_some() {
        return true;
    }
    state_path("no-update").is_some_and(|p| p.exists())
}

#[cfg(test)]
mod tests {
    use super::{extract_json_string, is_newer, parse};

    #[test]
    fn extract_json_string_valid() {
        assert_eq!(
            extract_json_string(r#"{"tag_name":"v1.8.0","x":1}"#, "tag_name"),
            Some("v1.8.0".to_string())
        );
        // Surrounding whitespace around colon and value is tolerated.
        assert_eq!(
            extract_json_string(r#"{ "tag_name" :  "1.2.3" }"#, "tag_name"),
            Some("1.2.3".to_string())
        );
    }

    #[test]
    fn extract_json_string_missing_key() {
        assert_eq!(extract_json_string(r#"{"name":"v1.0.0"}"#, "tag_name"), None);
    }

    #[test]
    fn extract_json_string_rejects_escapes_and_overlong() {
        // Backslash escape inside the value is refused (not a plain tag).
        assert_eq!(
            extract_json_string(r#"{"tag_name":"v1.0\n0"}"#, "tag_name"),
            None
        );
        // Empty value rejected.
        assert_eq!(extract_json_string(r#"{"tag_name":""}"#, "tag_name"), None);
    }

    #[test]
    fn parse_rejects_bad_shapes() {
        assert_eq!(parse("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse("1.2.3.4"), None); // 4 components
        assert_eq!(parse("1.2"), None); // too few
        assert_eq!(parse("1.x.3"), None); // non-numeric
    }

    #[test]
    fn is_newer_compares_triples() {
        assert_eq!(is_newer("0.0.1"), Some(super::parse("0.0.1") > super::parse(super::CURRENT)));
        // A clearly-old version is never newer than the compiled-in one.
        assert_eq!(is_newer("0.0.0"), Some(false));
        // Unparseable remote -> None (never offers an update).
        assert_eq!(is_newer("not-a-version"), None);
    }
}
