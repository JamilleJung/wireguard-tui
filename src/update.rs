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
            "--proto-redir",
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
    let tag = extract_json_string(&body, "tag_name")
        .ok_or_else(|| "no tag_name in release JSON".to_string())?;
    let version = tag.strip_prefix('v').unwrap_or(&tag);
    parse(version).ok_or_else(|| "invalid release version".to_string())?;
    Ok(version.to_string())
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
        if bytes[idx] == b'\\' || bytes[idx].is_ascii_control() {
            return None; // refuse escapes -> not a plain tag
        }
        idx += 1;
    }
    if idx == bytes.len() {
        return None; // unterminated string
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
    let mut it = v.strip_prefix('v').unwrap_or(v).split('.');
    let component = |part: &str| {
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        part.parse().ok()
    };
    let a = component(it.next()?)?;
    let b = component(it.next()?)?;
    let c = component(it.next()?)?;
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

/// Owns the private download directory until the installer has finished.
/// Failed downloads, failed staging and cancelled/failed installs all clean up.
pub struct VerifiedUpdate {
    workdir: PathBuf,
    directory: PathBuf,
    version: String,
}

impl VerifiedUpdate {
    pub fn version(&self) -> &str {
        &self.version
    }
}

fn validate_update_version(version: &str) -> Result<(), String> {
    match is_newer(version) {
        Some(true) => Ok(()),
        Some(false) => Err(format!(
            "release {version} is not newer than installed {CURRENT}; update refused"
        )),
        None => Err("invalid update version".into()),
    }
}

impl Drop for VerifiedUpdate {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.workdir);
    }
}

fn stage_bundle(update: &mut VerifiedUpdate, application: &str) -> Result<(), String> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let metadata =
        std::fs::symlink_metadata(&update.directory).map_err(|e| format!("update bundle: {e}"))?;
    if !metadata.file_type().is_dir() {
        return Err("unexpected tarball directory".into());
    }
    let required = [
        application.to_string(),
        "wg-helper".to_string(),
        "install.sh".to_string(),
        format!("packaging/{REPO}.desktop"),
        format!("packaging/{REPO}.svg"),
        format!("packaging/49-{REPO}.rules"),
    ];
    for relative in required {
        // Check every component as well as the leaf; staging must not follow a
        // tar entry symlink through packaging/, target/ or the bundle root.
        let mut current = update.directory.clone();
        for component in Path::new(&relative).components() {
            current.push(component);
            let md = std::fs::symlink_metadata(&current)
                .map_err(|e| format!("missing update asset {relative}: {e}"))?;
            if md.file_type().is_symlink() || (!md.is_file() && !md.is_dir()) {
                return Err(format!("invalid update asset: {relative}"));
            }
        }
        let md = std::fs::symlink_metadata(&current).map_err(|e| e.to_string())?;
        if !md.is_file() || md.nlink() != 1 {
            return Err(format!("invalid update asset: {relative}"));
        }
    }
    let target = update.directory.join("target");
    // Release bundles have no Cargo build directory. Reject a pre-existing
    // target rather than copying through archive-provided links or stale files.
    std::fs::create_dir(&target).map_err(|e| format!("stage target: {e}"))?;
    let release = target.join("release");
    std::fs::create_dir(&release).map_err(|e| format!("stage release: {e}"))?;
    for bin in [application, "wg-helper"] {
        let dest = release.join(bin);
        std::fs::copy(update.directory.join(bin), &dest)
            .map_err(|e| format!("stage {bin}: {e}"))?;
        std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("stage permissions: {e}"))?;
    }
    Ok(())
}

/// Download + verify the latest release into a fresh temp dir, returning the
/// extracted tarball directory (which holds `install.sh` and the binaries).
pub fn download_and_verify() -> Result<VerifiedUpdate, String> {
    if !cfg!(target_os = "linux") {
        return Err("automatic release updates are supported on Linux only".into());
    }
    let version = latest_tag()?;
    validate_update_version(&version)?;
    let tarball = tarball_name(&version)?;
    let base = format!("https://github.com/{OWNER}/{REPO}/releases/download/v{version}");

    // A throwaway work dir under TMPDIR, owned by us.
    let mut update = VerifiedUpdate {
        workdir: private_workdir()?,
        directory: PathBuf::new(),
        version: version.clone(),
    };
    let workdir = &update.workdir;

    // Fetch the tarball + signed checksum manifest. (Never the release's own
    // minisign.pub - we only trust the compiled-in one.)
    for asset in [tarball.as_str(), "SHA256SUMS", "SHA256SUMS.minisig"] {
        download_to(&format!("{base}/{asset}"), &workdir.join(asset))?;
    }

    verify_release(workdir, &tarball)?;

    // Unpack only after verification succeeds.
    let st = Command::new("tar")
        .args(["-xzf"])
        .arg(workdir.join(&tarball))
        .arg("-C")
        .arg(workdir)
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
    update.directory = tardir;
    stage_bundle(&mut update, "wg-tui")?;
    Ok(update)
}

/// Download `url` to `dest` with the same hard caps as the API call.
fn download_to(url: &str, dest: &Path) -> Result<(), String> {
    let st = Command::new("curl")
        .args([
            "-fsSL",
            "--proto",
            "=https",
            "--proto-redir",
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

    // 3) Check exactly this archive. A suffix match or --ignore-missing can
    // accidentally accept a manifest that only names a different file.
    let sums = std::fs::read_to_string(workdir.join("SHA256SUMS")).map_err(|e| e.to_string())?;
    let checksum = tarball_checksum(&sums, tarball)?;
    let selected = workdir.join("selected.sha256");
    std::fs::write(&selected, checksum).map_err(|e| e.to_string())?;
    let st = Command::new("sha256sum")
        .args(["-c", "--strict"])
        .arg(&selected)
        .current_dir(workdir)
        .status()
        .map_err(|e| format!("sha256sum spawn: {e}"))?;
    if !st.success() {
        return Err("checksum mismatch on downloaded tarball - refusing".into());
    }
    Ok(())
}

/// mkdtemp creates an exclusive, unpredictable directory with mode 0700, so
/// another local user cannot replace verified files before installation.
fn private_workdir() -> Result<PathBuf, String> {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let template = std::env::temp_dir().join("wg-tui-update-XXXXXX");
    let mut name = std::ffi::CString::new(template.as_os_str().as_bytes())
        .map_err(|e| format!("temp dir: {e}"))?
        .into_bytes_with_nul();
    let result = unsafe { libc::mkdtemp(name.as_mut_ptr().cast()) };
    if result.is_null() {
        return Err(format!("temp dir: {}", std::io::Error::last_os_error()));
    }
    name.pop();
    Ok(PathBuf::from(std::ffi::OsString::from_vec(name)))
}

fn tarball_checksum(sums: &str, tarball: &str) -> Result<String, String> {
    let mut matching = sums.lines().filter_map(|line| {
        let (hash, name) = line.split_once("  ").or_else(|| line.split_once(" *"))?;
        (name == tarball).then_some((hash, line))
    });
    let (hash, line) = matching
        .next()
        .ok_or("downloaded tarball is not listed in SHA256SUMS - refusing")?;
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) || matching.next().is_some()
    {
        return Err("invalid or duplicate tarball checksum - refusing".into());
    }
    Ok(format!("{line}\n"))
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
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|p| p.join(prog).is_file()))
}

// ---------------------------------------------------------------------------
// APPLY
// ---------------------------------------------------------------------------

/// Apply a verified update by re-running the tarball's own `install.sh`, which
/// owns the privileged writes and the sudoers/polkit grant exactly as the first
/// install did. The running process is left untouched; the user restarts to use
/// the new binary.
pub fn apply(update: &VerifiedUpdate) -> Result<(), String> {
    let tardir = &update.directory;
    // Run the verified installer as the same user; it self-escalates for the
    // /usr/local writes. Keep existing desktop settings during the update.
    let mut installer = Command::new("bash");
    installer.arg(tardir.join("install.sh")).arg("--no-desktop");
    // A user who launched the app as root already chose the privileged flow;
    // the verified prebuilt update does not need the source-build root guard.
    if unsafe { libc::geteuid() } == 0 {
        installer.arg("--allow-root");
    }
    let st = installer
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
        .filter(|path| path.is_absolute())
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
    #[test]
    fn downloaded_release_cannot_silently_downgrade() {
        let current = super::parse(super::CURRENT).unwrap();
        assert!(super::validate_update_version(super::CURRENT).is_err());
        assert!(super::validate_update_version("0.0.0").is_err());
        assert!(super::validate_update_version("bad").is_err());
        assert!(super::validate_update_version(&format!("{}.0.0", current.0 + 1)).is_ok());
        let update = bundle_fixture();
        assert_eq!(update.version(), "1.9.2");
    }

    use super::{REPO, VerifiedUpdate, stage_bundle};

    fn bundle_fixture() -> VerifiedUpdate {
        let workdir = private_workdir().unwrap();
        let directory = workdir.join("bundle");
        std::fs::create_dir(&directory).unwrap();
        std::fs::create_dir(directory.join("packaging")).unwrap();
        for file in [
            "wg-tui".to_string(),
            "wg-helper".into(),
            "install.sh".into(),
            format!("packaging/{REPO}.desktop"),
            format!("packaging/{REPO}.svg"),
            format!("packaging/49-{REPO}.rules"),
        ] {
            std::fs::write(directory.join(file), b"fixture").unwrap();
        }
        VerifiedUpdate {
            workdir,
            directory,
            version: "1.9.2".into(),
        }
    }

    #[test]
    fn update_requires_complete_bundle_before_staging() {
        let mut update = bundle_fixture();
        let path = update.workdir.clone();
        std::fs::remove_file(update.directory.join("wg-helper")).unwrap();
        assert!(stage_bundle(&mut update, "wg-tui").is_err());
        assert!(!update.directory.join("target").exists());
        drop(update);
        assert!(!path.exists());
    }

    #[test]
    fn update_staging_and_lifetime_cleanup() {
        use std::os::unix::fs::PermissionsExt;
        let mut update = bundle_fixture();
        let path = update.workdir.clone();
        stage_bundle(&mut update, "wg-tui").unwrap();
        for bin in ["wg-tui", "wg-helper"] {
            let file = update.directory.join("target/release").join(bin);
            assert_eq!(std::fs::read(&file).unwrap(), b"fixture");
            assert_eq!(
                std::fs::metadata(file).unwrap().permissions().mode() & 0o777,
                0o755
            );
        }
        drop(update);
        assert!(!path.exists());
    }

    #[test]
    fn update_rejects_archive_links_and_existing_build_tree() {
        let mut update = bundle_fixture();
        std::fs::remove_file(update.directory.join("wg-helper")).unwrap();
        std::os::unix::fs::symlink("install.sh", update.directory.join("wg-helper")).unwrap();
        assert!(stage_bundle(&mut update, "wg-tui").is_err());
        let mut update = bundle_fixture();
        std::os::unix::fs::symlink("packaging", update.directory.join("target")).unwrap();
        assert!(stage_bundle(&mut update, "wg-tui").is_err());
    }
    use super::{extract_json_string, is_newer, parse, private_workdir, tarball_checksum};

    #[test]
    fn private_update_directories_are_exclusive_and_private() {
        use std::os::unix::fs::PermissionsExt;
        let first = private_workdir().unwrap();
        let second = private_workdir().unwrap();
        assert_ne!(first, second);
        assert_eq!(
            std::fs::metadata(&first).unwrap().permissions().mode() & 0o777,
            0o700
        );
        std::fs::remove_dir(first).unwrap();
        std::fs::remove_dir(second).unwrap();
    }

    #[test]
    fn checksum_requires_one_exact_archive_name() {
        let hash = "a".repeat(64);
        let name = "wireguard-tui-1.9.1-x86_64-linux.tar.gz";
        let line = format!("{hash}  {name}\n");
        assert_eq!(tarball_checksum(&line, name).unwrap(), line);
        assert!(tarball_checksum(&format!("{hash}  prefix-{name}\n"), name).is_err());
        assert!(tarball_checksum(&format!("{line}{line}"), name).is_err());
        assert!(tarball_checksum(&format!("invalid  {name}\n"), name).is_err());
        assert!(tarball_checksum("", name).is_err());
    }

    #[test]
    fn malformed_release_strings_are_rejected() {
        assert_eq!(
            extract_json_string("{\"tag_name\":\"v1.9.1", "tag_name"),
            None
        );
        assert_eq!(
            extract_json_string("{\"tag_name\":\"v1.9\n.1\"}", "tag_name"),
            None
        );
        for invalid in ["../1.2.3", "+1.2.3", "vv1.2.3", "1.2.3 ", "1.2.3/evil"] {
            assert_eq!(parse(invalid), None, "{invalid}");
        }
    }

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
        assert_eq!(
            extract_json_string(r#"{"name":"v1.0.0"}"#, "tag_name"),
            None
        );
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
        assert_eq!(
            is_newer("0.0.1"),
            Some(super::parse("0.0.1") > super::parse(super::CURRENT))
        );
        // A clearly-old version is never newer than the compiled-in one.
        assert_eq!(is_newer("0.0.0"), Some(false));
        // Unparseable remote -> None (never offers an update).
        assert_eq!(is_newer("not-a-version"), None);
    }
}
