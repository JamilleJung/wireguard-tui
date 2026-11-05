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
//! Before anything is downloaded the updater works out how this copy was
//! installed. Only copies installed by `install.sh` (recorded in its
//! `install-manifest`) are updated in place, with the same prefix,
//! authorization mode and desktop-entry choice; copies owned by a distro
//! package, `cargo install` or an unknown method are left to that method.
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
    setup: InstallerSetup,
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
    // Refuse before any download when another method owns this copy.
    let setup = detect_installation()?;
    let version = latest_tag()?;
    validate_update_version(&version)?;
    let tarball = tarball_name(&version)?;
    let base = format!("https://github.com/{OWNER}/{REPO}/releases/download/v{version}");

    // A throwaway work dir under TMPDIR, owned by us.
    let mut update = VerifiedUpdate {
        workdir: private_workdir()?,
        directory: PathBuf::new(),
        version: version.clone(),
        setup,
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
        // Never refresh pacman's databases without a full upgrade (-Sy):
        // that risks a partial upgrade. Install against the synced databases.
        ("pacman", &["-S", "--needed", "--noconfirm", "minisign"]),
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
/// install did. The prefix, authorization mode and desktop-entry choice recorded
/// at install time are passed on explicitly. The running process is left
/// untouched; the user restarts to use the new binary.
pub fn apply(update: &VerifiedUpdate) -> Result<(), String> {
    let tardir = &update.directory;
    // Run the verified installer as the same user; it self-escalates for the
    // privileged writes.
    let mut installer = Command::new("bash");
    installer
        .arg(tardir.join("install.sh"))
        .args(installer_args(&update.setup));
    // A user who launched the app as root already chose the privileged flow;
    // the verified prebuilt update does not need the source-build root guard.
    if unsafe { libc::geteuid() } == 0 {
        installer.arg("--allow-root");
    }
    let st = installer
        .env("WG_USE_PREBUILT", "1")
        .env("PREFIX", &update.setup.prefix)
        .env_remove("WG_PREBUILT_DIR")
        .env_remove("WG_INSTALL_INJECT_FAILURE")
        .current_dir(tardir)
        .status()
        .map_err(|e| format!("install.sh spawn: {e}"))?;
    if !st.success() {
        return Err(format!("install.sh failed ({st})"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// INSTALLATION METHOD
// ---------------------------------------------------------------------------

/// The installer's record, next to the helper: `<prefix>/lib/<app>/install-manifest`.
const MANIFEST_NAME: &str = "install-manifest";
/// Executable name, used for the duplicate-copy check.
const APP_BIN: &str = "wg-tui";
/// The frontend has a desktop entry the installer can skip (`--no-desktop`).
const DESKTOP_IS_OPTIONAL: bool = true;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthMode {
    Sudoers,
    Polkit,
    /// A root-only install without a passwordless helper grant.
    None,
}

/// An installer-managed copy and the choices an update must keep.
#[derive(Debug, Clone, PartialEq, Eq)]
struct InstallerSetup {
    prefix: PathBuf,
    /// `None` for an install made before the manifest existed: install.sh
    /// then keeps whatever grant it finds on disk.
    auth: Option<AuthMode>,
    desktop: Option<bool>,
    /// The first install was forced next to a distro package copy.
    force: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum Manifest {
    Installer(InstallerSetup),
    /// Shipped by the distro recipes so the updater never touches their files.
    Package {
        manager: String,
    },
}

/// Parse the `key=value` manifest. Unknown keys are ignored so later
/// installers can add fields; anything the updater relies on is validated.
fn parse_manifest(text: &str) -> Result<Manifest, String> {
    let mut fields = std::collections::HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| format!("malformed install manifest line: {line}"))?;
        fields.entry(key.trim()).or_insert(value.trim());
    }
    let field = |key: &str| fields.get(key).copied().unwrap_or("");
    if field("format") != "1" {
        return Err("unsupported install manifest format".into());
    }
    if field("app") != REPO {
        return Err(format!("install manifest belongs to '{}'", field("app")));
    }
    let flag = |key: &str| match field(key) {
        "" | "0" => Ok(false),
        "1" => Ok(true),
        other => Err(format!("invalid install manifest {key}: {other}")),
    };
    match field("method") {
        "installer" => {
            let prefix = field("prefix");
            let safe = prefix.starts_with('/')
                && prefix.len() > 1
                && !prefix.ends_with('/')
                && prefix
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"/_.-".contains(&b))
                && prefix
                    .split('/')
                    .skip(1)
                    .all(|part| !part.is_empty() && part != "." && part != "..");
            if !safe {
                return Err(format!("invalid install manifest prefix: {prefix}"));
            }
            let auth = match field("auth") {
                "sudoers" => AuthMode::Sudoers,
                "polkit" => AuthMode::Polkit,
                "none" => AuthMode::None,
                other => return Err(format!("invalid install manifest auth mode: {other}")),
            };
            let desktop = match field("desktop") {
                "" => None,
                _ => Some(flag("desktop")?),
            };
            Ok(Manifest::Installer(InstallerSetup {
                prefix: PathBuf::from(prefix),
                auth: Some(auth),
                desktop,
                force: flag("force")?,
            }))
        }
        "package" => Ok(Manifest::Package {
            manager: field("manager").to_string(),
        }),
        other => Err(format!("unknown installation method '{other}'")),
    }
}

/// Read the manifest only if root wrote it: it decides which privileged
/// grant the installer re-creates.
fn read_manifest(path: &Path) -> Result<Option<Manifest>, String> {
    use std::os::unix::fs::MetadataExt;
    let md = match std::fs::symlink_metadata(path) {
        Ok(md) => md,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("install manifest {}: {e}", path.display())),
    };
    if !md.file_type().is_file()
        || md.uid() != 0
        || md.mode() & 0o022 != 0
        || md.nlink() != 1
        || md.len() > 16 * 1024
    {
        return Err(format!(
            "install manifest {} is not a root-owned, protected file - refusing to update",
            path.display()
        ));
    }
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("install manifest {}: {e}", path.display()))?;
    parse_manifest(&text).map(Some)
}

/// `<prefix>` for an executable at `<prefix>/bin/<name>`.
fn install_prefix(exe: &Path) -> Option<PathBuf> {
    let bin = exe.parent()?;
    if bin.file_name()? != "bin" {
        return None;
    }
    bin.parent().map(Path::to_path_buf)
}

/// Decide how the running copy was installed; `Err` explains which other
/// method owns it and how to update through that method instead.
fn detect_installation() -> Result<InstallerSetup, String> {
    let exe = std::env::current_exe().map_err(|e| format!("locate the running program: {e}"))?;
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
    if let Some((manager, package)) = package_owner(&exe) {
        return Err(package_refusal(manager, &package));
    }
    let prefix = install_prefix(&exe).ok_or_else(unknown_installation)?;
    let setup = match read_manifest(&prefix.join("lib").join(REPO).join(MANIFEST_NAME))? {
        Some(Manifest::Package { manager }) => return Err(package_refusal(&manager, REPO)),
        Some(Manifest::Installer(setup)) => {
            if setup.prefix != prefix {
                return Err(format!(
                    "the install manifest describes {} but this copy runs from {}; \
                     reinstall with install.sh",
                    setup.prefix.display(),
                    prefix.display()
                ));
            }
            setup
        }
        None => legacy_installation(prefix)?,
    };
    if let Some(copy) = duplicate_copy(&setup, |path| path.exists()) {
        return Err(format!(
            "another copy of {REPO} is installed at {}; an update here would keep both. \
             Remove one of them (or reinstall with install.sh --force to keep both)",
            copy.display()
        ));
    }
    Ok(setup)
}

/// A copy installed by an installer that predates the manifest: a root-owned
/// helper next to the executable. `cargo install` roots are not ours.
fn legacy_installation(prefix: PathBuf) -> Result<InstallerSetup, String> {
    use std::os::unix::fs::MetadataExt;
    if prefix.join(".crates.toml").exists() || prefix.join(".crates2.json").exists() {
        return Err(format!(
            "{REPO} was installed with `cargo install`; update it the same way: \
             cargo install --locked --force --git https://github.com/{OWNER}/{REPO} \
             (then re-run install.sh from that release for the privileged helper)"
        ));
    }
    let helper = prefix.join("lib").join(REPO).join("wg-helper");
    match std::fs::symlink_metadata(&helper) {
        Ok(md) if md.file_type().is_file() && md.uid() == 0 => Ok(InstallerSetup {
            prefix,
            auth: None,
            desktop: None,
            force: false,
        }),
        _ => Err(unknown_installation()),
    }
}

fn unknown_installation() -> String {
    format!(
        "cannot tell how this copy of {REPO} was installed (no install manifest and \
         no package owns it); update it the way you installed it, or install the \
         signed release with its install.sh"
    )
}

/// A distro-package copy next to an installer copy elsewhere, unless the
/// user deliberately forced both at install time.
fn duplicate_copy(setup: &InstallerSetup, exists: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    if setup.force || setup.prefix == Path::new("/usr") {
        return None;
    }
    [
        PathBuf::from("/usr/bin").join(APP_BIN),
        PathBuf::from("/usr/lib").join(REPO).join("wg-helper"),
    ]
    .into_iter()
    .find(|path| exists(path))
}

/// Installer flags that reproduce the recorded setup.
fn installer_args(setup: &InstallerSetup) -> Vec<&'static str> {
    let mut args = vec!["--update"];
    match setup.auth {
        Some(AuthMode::Sudoers) => args.push("--sudoers"),
        Some(AuthMode::Polkit) => args.push("--polkit"),
        Some(AuthMode::None) | None => {}
    }
    if DESKTOP_IS_OPTIONAL {
        match setup.desktop {
            Some(true) => args.push("--desktop"),
            Some(false) => args.push("--no-desktop"),
            None => {}
        }
    }
    if setup.force {
        args.push("--force");
    }
    args
}

/// Ask each available package database who owns `path`.
fn package_owner(path: &Path) -> Option<(&'static str, String)> {
    let queries: [(&str, &str, &[&str]); 5] = [
        ("dpkg", "dpkg-query", &["-S"]),
        ("rpm", "rpm", &["-qf", "--queryformat", "%{NAME}\n"]),
        ("pacman", "pacman", &["-Qqo"]),
        ("apk", "apk", &["info", "--who-owns"]),
        ("xbps", "xbps-query", &["-o"]),
    ];
    queries.into_iter().find_map(|(manager, program, args)| {
        if !which(program) {
            return None;
        }
        let out = Command::new(program)
            .args(args)
            .arg(path)
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        parse_owner(manager, &String::from_utf8_lossy(&out.stdout)).map(|pkg| (manager, pkg))
    })
}

/// Extract the owning package from a package database query's output.
fn parse_owner(manager: &str, out: &str) -> Option<String> {
    fn first(text: &str) -> Option<&str> {
        text.lines().map(str::trim).find(|line| !line.is_empty())
    }
    let owner = match manager {
        "dpkg" => out
            .lines()
            .filter(|line| !line.starts_with("diversion"))
            .find_map(|line| line.split_once(": "))
            .and_then(|(packages, _)| packages.split(", ").next())
            .map(|package| package.split(':').next().unwrap_or(package)),
        "apk" => out
            .lines()
            .find_map(|line| line.split_once(" is owned by "))
            .map(|(_, package)| package),
        "xbps" => first(out)
            .and_then(|line| line.split_once(':'))
            .map(|(package, _)| package),
        _ => first(out),
    }?
    .trim();
    let valid = !owner.is_empty()
        && owner.len() <= 128
        && owner
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"+._@-".contains(&b));
    valid.then(|| owner.to_string())
}

/// `name-1.2.3-r0` (apk) / `name-1.2.3_1` (xbps) -> `name`.
fn strip_package_version(manager: &str, package: &str) -> String {
    let mut parts: Vec<&str> = package.rsplitn(3, '-').collect();
    parts.reverse();
    match manager {
        "apk" if parts.len() == 3 && parts[2].starts_with('r') => parts[0].to_string(),
        "xbps" => match package.rsplit_once('-') {
            Some((name, version)) if version.contains('_') => name.to_string(),
            _ => package.to_string(),
        },
        _ => package.to_string(),
    }
}

/// The refusal for a package-managed copy, naming the command that updates it.
fn package_refusal(manager: &str, package: &str) -> String {
    let name = strip_package_version(manager, package);
    let command = match manager {
        "dpkg" => format!("sudo apt update && sudo apt install --only-upgrade {name}"),
        "rpm" if which("zypper") => format!("sudo zypper update {name}"),
        "rpm" => format!("sudo dnf upgrade {name}"),
        "pacman" if pacman_foreign(&name) => format!(
            "sudo pacman -Syu, then rebuild the AUR package {name} (for example with your AUR helper)"
        ),
        "pacman" => "sudo pacman -Syu".to_string(),
        "apk" => format!("sudo apk upgrade {name}"),
        "xbps" => format!("sudo xbps-install -Su {name}"),
        _ => "your distribution's package manager".to_string(),
    };
    format!(
        "{REPO} is installed from the distribution package '{name}', so the package \
         manager owns its files and the in-app updater will not replace them. \
         Update it with: {command}"
    )
}

/// Whether pacman lists `name` as a foreign (AUR / locally built) package.
fn pacman_foreign(name: &str) -> bool {
    Command::new("pacman")
        .args(["-Qqm", name])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
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

    use super::{InstallerSetup, REPO, VerifiedUpdate, stage_bundle};

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
            setup: InstallerSetup {
                prefix: "/usr/local".into(),
                auth: None,
                desktop: None,
                force: false,
            },
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

    use super::{
        AuthMode, Manifest, duplicate_copy, install_prefix, installer_args, package_refusal,
        parse_manifest, parse_owner, read_manifest, strip_package_version,
    };
    use std::path::{Path, PathBuf};

    fn manifest(body: &str) -> String {
        format!("# comment\nformat=1\napp={REPO}\n{body}")
    }

    #[test]
    fn update_manifest_records_method_prefix_and_auth() {
        let parsed = parse_manifest(&manifest(
            "method=installer\norigin=release\nprefix=/usr/local\nauth=polkit\n\
             auth_user=alice\ndesktop=0\nforce=0\nversion=1.9.3\nfuture_key=x\n",
        ))
        .unwrap();
        let Manifest::Installer(setup) = parsed else {
            panic!("installer manifest parsed as {parsed:?}");
        };
        assert_eq!(setup.prefix, PathBuf::from("/usr/local"));
        assert_eq!(setup.auth, Some(AuthMode::Polkit));
        assert_eq!(setup.desktop, Some(false));
        assert!(!setup.force);
        assert_eq!(
            installer_args(&setup),
            ["--update", "--polkit", "--no-desktop"]
        );
        let package = parse_manifest(&manifest("method=package\nmanager=pacman\n")).unwrap();
        assert_eq!(
            package,
            Manifest::Package {
                manager: "pacman".into()
            }
        );
    }

    #[test]
    fn update_preserves_auth_mode_and_force() {
        let mut setup = InstallerSetup {
            prefix: "/opt/wg".into(),
            auth: Some(AuthMode::Sudoers),
            desktop: Some(true),
            force: true,
        };
        assert_eq!(
            installer_args(&setup),
            ["--update", "--sudoers", "--desktop", "--force"]
        );
        setup.auth = Some(AuthMode::None);
        setup.desktop = None;
        setup.force = false;
        assert_eq!(installer_args(&setup), ["--update"]);
        setup.auth = None;
        assert_eq!(installer_args(&setup), ["--update"]);
    }

    #[test]
    fn update_manifest_rejects_unsafe_or_foreign_records() {
        for body in [
            "method=installer\nprefix=relative\nauth=sudoers\n",
            "method=installer\nprefix=/usr/../tmp\nauth=sudoers\n",
            "method=installer\nprefix=/usr/local/\nauth=sudoers\n",
            "method=installer\nprefix=/usr/lo cal\nauth=sudoers\n",
            "method=installer\nprefix=/usr/local\nauth=everyone\n",
            "method=installer\nprefix=/usr/local\nauth=sudoers\ndesktop=yes\n",
            "method=curl-pipe\n",
            "garbage line\n",
        ] {
            assert!(parse_manifest(&manifest(body)).is_err(), "{body}");
        }
        assert!(parse_manifest("format=2\nmethod=installer\n").is_err());
        assert!(
            parse_manifest("format=1\napp=other\nmethod=installer\nprefix=/usr\nauth=none\n")
                .is_err()
        );
    }

    #[test]
    fn update_manifest_must_be_protected() {
        use std::os::unix::fs::PermissionsExt;
        let dir = private_workdir().unwrap();
        let path = dir.join("install-manifest");
        assert_eq!(read_manifest(&path), Ok(None));
        std::fs::write(
            &path,
            manifest("method=installer\nprefix=/usr/local\nauth=sudoers\n"),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
        assert!(read_manifest(&path).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        // Only a root-written record may choose the privileged grant.
        assert_eq!(
            read_manifest(&path).is_ok(),
            unsafe { libc::geteuid() } == 0
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn update_detects_package_owners() {
        assert_eq!(
            parse_owner("dpkg", "wireguard-tui: /usr/bin/wg-tui\n").as_deref(),
            Some("wireguard-tui")
        );
        assert_eq!(
            parse_owner(
                "dpkg",
                "diversion by x from: /usr/bin/wg-tui\nwireguard-tui:amd64, other: /usr/bin/wg-tui\n"
            )
            .as_deref(),
            Some("wireguard-tui")
        );
        assert_eq!(
            parse_owner("rpm", "wireguard-tui\n").as_deref(),
            Some("wireguard-tui")
        );
        assert_eq!(
            parse_owner("pacman", "wireguard-tui\n").as_deref(),
            Some("wireguard-tui")
        );
        assert_eq!(
            parse_owner(
                "apk",
                "/usr/bin/wg-tui is owned by wireguard-tui-1.9.3-r0\n"
            )
            .as_deref(),
            Some("wireguard-tui-1.9.3-r0")
        );
        assert_eq!(
            parse_owner(
                "xbps",
                "wireguard-tui-1.9.3_1: /usr/bin/wg-tui (regular file)\n"
            )
            .as_deref(),
            Some("wireguard-tui-1.9.3_1")
        );
        assert_eq!(parse_owner("rpm", ""), None);
        assert_eq!(parse_owner("pacman", "bad name; rm -rf /\n"), None);
        assert_eq!(
            strip_package_version("apk", "wireguard-tui-1.9.3-r0"),
            "wireguard-tui"
        );
        assert_eq!(
            strip_package_version("xbps", "wireguard-tui-1.9.3_1"),
            "wireguard-tui"
        );
        let refusal = package_refusal("dpkg", "wireguard-tui");
        assert!(refusal.contains("apt install --only-upgrade wireguard-tui"));
        assert!(
            package_refusal("apk", "wireguard-tui-1.9.3-r0").contains("apk upgrade wireguard-tui")
        );
    }

    #[test]
    fn update_refuses_duplicate_package_copy() {
        let setup = InstallerSetup {
            prefix: "/usr/local".into(),
            auth: Some(AuthMode::Sudoers),
            desktop: None,
            force: false,
        };
        assert_eq!(duplicate_copy(&setup, |_| false), None);
        assert_eq!(
            duplicate_copy(&setup, |path| path == Path::new("/usr/bin/wg-tui")),
            Some(PathBuf::from("/usr/bin/wg-tui"))
        );
        let forced = InstallerSetup {
            force: true,
            ..setup.clone()
        };
        assert_eq!(duplicate_copy(&forced, |_| true), None);
        let usr = InstallerSetup {
            prefix: "/usr".into(),
            ..setup
        };
        assert_eq!(duplicate_copy(&usr, |_| true), None);
        assert_eq!(
            install_prefix(Path::new("/usr/local/bin/wg-tui")),
            Some(PathBuf::from("/usr/local"))
        );
        assert_eq!(
            install_prefix(Path::new("/src/target/release/wg-tui")),
            None
        );
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
