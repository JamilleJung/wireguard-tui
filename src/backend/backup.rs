//! Backups and exports: timestamped ZIP archives of every tunnel config, kept
//! under the user's data dir (NOT /etc/wireguard, so no helper is needed for
//! storage), plus restore with rollback.

use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};

use super::helper::HelperError;
use super::validate_tunnel_name;
use crate::import_parse::{self, backup_configs};
#[cfg(test)]
use crate::import_parse::{MAX_BACKUP_BYTES, preflight_zip_directory};
use crate::safe_file::{self, Rules};

/// Metadata about one backup archive.
#[derive(Clone)]
pub struct BackupInfo {
    pub path: PathBuf,
    pub name: String,
    pub when_secs: i64,
    pub size: u64,
    pub count: usize,
}

pub(crate) fn collect_configs(
    names: impl IntoIterator<Item = String>,
    mut read: impl FnMut(&str) -> Result<String, String>,
) -> Result<Vec<(String, String)>, String> {
    names
        .into_iter()
        .map(|name| {
            validate_tunnel_name(&name)?;
            let content = read(&name)?;
            Ok((format!("{name}.conf"), content))
        })
        .collect()
}

/// Export is all-or-error: unreadable configs never silently disappear from a
/// successful backup, and failed writes preserve an existing destination.
pub fn export_zip(dest: &Path) -> Result<usize, String> {
    let files = super::read_all_configs()?;
    write_zip_files(dest, &files, true)
}

pub(crate) fn write_zip_files(
    dest: &Path,
    files: &[(String, String)],
    replace: bool,
) -> Result<usize, String> {
    if files.is_empty() {
        return Err("No tunnels to export.".into());
    }
    atomic_private_write(dest, replace, |file| {
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o600);
        for (name, content) in files {
            zip.start_file(name, opts).map_err(|e| e.to_string())?;
            write_payload(&mut zip, content.as_bytes())?;
        }
        zip.finish().map_err(|e| write_error(&e))?;
        Ok(files.len())
    })
}

fn write_error(error: &dyn fmt::Display) -> String {
    let text = error.to_string();
    if text.contains("No space left") || text.contains("os error 28") {
        format!("{text} - the destination disk is full")
    } else {
        text
    }
}

/// Write all of `data`, naming a full disk plainly.
pub(crate) fn write_payload(writer: &mut impl Write, data: &[u8]) -> Result<(), String> {
    writer
        .write_all(data)
        .and_then(|()| writer.flush())
        .map_err(|e| write_error(&e))
}

/// Publish a completed 0600 sibling file. Replacing a hardlink changes only
/// the requested directory entry; a partial write never truncates another file.
pub(crate) fn atomic_private_write<T>(
    dest: &Path,
    replace: bool,
    write: impl FnOnce(&mut std::fs::File) -> Result<T, String>,
) -> Result<T, String> {
    use std::os::fd::FromRawFd;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    if std::fs::symlink_metadata(dest).is_ok_and(|meta| !meta.is_file()) {
        return Err("Destination must be a regular file, not a directory or symlink.".into());
    }
    let dir = dest
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let template = dir.join(".wireguard-export-XXXXXX");
    let mut name = std::ffi::CString::new(template.as_os_str().as_bytes())
        .map_err(|e| e.to_string())?
        .into_bytes_with_nul();
    let fd = unsafe { libc::mkstemp(name.as_mut_ptr().cast()) };
    if fd < 0 {
        return Err(format!(
            "create private file: {}",
            std::io::Error::last_os_error()
        ));
    }
    let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
    name.pop();
    let tmp = PathBuf::from(std::ffi::OsString::from_vec(name));
    let result = (|| {
        let result = write(&mut file)?;
        file.sync_all().map_err(|e| write_error(&e))?;
        if replace {
            std::fs::rename(&tmp, dest).map_err(|e| format!("publish {}: {e}", dest.display()))?;
        } else {
            // link is an atomic no-clobber publication; remove the temporary
            // link afterward, leaving the completed archive with one link.
            std::fs::hard_link(&tmp, dest)
                .map_err(|e| format!("publish {}: {e}", dest.display()))?;
        }
        if let Ok(parent) = std::fs::File::open(dir) {
            let _ = parent.sync_all();
        }
        Ok(result)
    })();
    let _ = std::fs::remove_file(tmp);
    result
}

/// `$XDG_DATA_HOME` (or `~/.local/share`) `/<pkg>/backups`, created 0700 - the
/// archives bundle private keys, so keep the whole tree private.
pub fn backup_dir() -> Result<PathBuf, String> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .filter(|p| p.is_absolute())
        .ok_or("Can't locate an absolute home directory for backups.")?;
    ensure_backup_dir(&base)
}

pub(crate) fn ensure_backup_dir(base: &Path) -> Result<PathBuf, String> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
    std::fs::create_dir_all(base).map_err(|e| e.to_string())?;
    let dir = base.join("wireguard").join("backups");
    for path in [base.join("wireguard"), dir.clone()] {
        match std::fs::DirBuilder::new().mode(0o700).create(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.to_string()),
        }
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(&path)
            .map_err(|e| e.to_string())?;
        let meta = file.metadata().map_err(|e| e.to_string())?;
        if meta.uid() != unsafe { libc::geteuid() } {
            return Err("Backup directory is not owned by the current user.".into());
        }
        file.set_permissions(std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
    }
    dir.canonicalize().map_err(|e| e.to_string())
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn tm_of(secs: i64) -> Option<libc::tm> {
    let t = secs as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
        None
    } else {
        Some(tm)
    }
}

/// Local "YYYY-MM-DD HH:MM:SS" for display.
pub fn fmt_time(secs: i64) -> String {
    match tm_of(secs) {
        Some(tm) => format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min,
            tm.tm_sec
        ),
        None => "unknown".into(),
    }
}

fn stamp_compact(secs: i64) -> String {
    match tm_of(secs) {
        Some(tm) => format!(
            "{:04}{:02}{:02}-{:02}{:02}{:02}",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min,
            tm.tm_sec
        ),
        None => format!("{secs}"),
    }
}

/// Create a timestamped backup of every tunnel config. Returns its metadata.
pub fn create_backup() -> Result<BackupInfo, String> {
    let dir = backup_dir()?;
    let secs = now_secs();
    static NEXT_BACKUP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let sequence = NEXT_BACKUP.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let name = format!(
        "wg-backup-{}-{}-{sequence}.zip",
        stamp_compact(secs),
        std::process::id()
    );
    let path = dir.join(&name);
    let count = write_zip_files(&path, &super::read_all_configs()?, false)?;
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    Ok(BackupInfo {
        path,
        name,
        when_secs: secs,
        size,
        count,
    })
}

fn count_zip_entries(path: &Path) -> usize {
    open_backup(path).map_or(0, import_parse::backup_entry_count)
}

/// List existing backups, newest first.
pub fn list_backups() -> Vec<BackupInfo> {
    let dir = match backup_dir() {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return out;
    };
    for ent in rd.flatten() {
        let path = ent.path();
        if path.extension().and_then(|e| e.to_str()) != Some("zip") {
            continue;
        }
        let Ok(file) = open_backup(&path) else {
            continue;
        };
        let Ok(md) = file.metadata() else { continue };
        let when_secs = md
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        out.push(BackupInfo {
            name: ent.file_name().to_string_lossy().into_owned(),
            when_secs,
            size: md.len(),
            count: count_zip_entries(&path),
            path,
        });
    }
    out.sort_by_key(|b| std::cmp::Reverse(b.when_secs));
    out
}

/// Open relative to the already-open backup directory. Do not canonicalize a
/// file then reopen its spelling: a symlink swap could redirect that second IO.
fn open_backup(path: &Path) -> Result<std::fs::File, String> {
    let dir = backup_dir()?;
    open_backup_in(&dir, path)
}

pub(crate) fn open_backup_in(dir: &Path, path: &Path) -> Result<std::fs::File, String> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    if path
        .parent()
        .ok_or("Backup path has no directory.")?
        .canonicalize()
        .map_err(|e| e.to_string())?
        != dir
    {
        return Err("Refusing a path outside the backup directory.".into());
    }
    let name = path.file_name().ok_or("Backup path has no filename.")?;
    let directory = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(dir)
        .map_err(|e| e.to_string())?;
    let file = safe_file::open_in_dir(&directory, name, Rules::BACKUP)?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if meta.nlink() != 1 || meta.uid() != unsafe { libc::geteuid() } {
        return Err(
            "Backup must be an owned regular file with one link and at most 64 MiB.".into(),
        );
    }
    Ok(file)
}

// ---------------------------------------------------------------------------
// Restore with rollback
// ---------------------------------------------------------------------------

/// The saved-tunnel operations restore needs; the helper in production, an
/// in-memory map in tests.
pub trait TunnelStore {
    fn names(&mut self) -> Result<Vec<String>, HelperError>;
    fn read(&mut self, name: &str) -> Result<String, HelperError>;
    fn save(&mut self, name: &str, content: &str) -> Result<(), HelperError>;
    fn delete(&mut self, name: &str) -> Result<(), HelperError>;
}

struct HelperStore;

impl TunnelStore for HelperStore {
    fn names(&mut self) -> Result<Vec<String>, HelperError> {
        super::saved_tunnel_names()
    }
    fn read(&mut self, name: &str) -> Result<String, HelperError> {
        super::read_config(name)
    }
    fn save(&mut self, name: &str, content: &str) -> Result<(), HelperError> {
        super::save_config(name, content)
    }
    fn delete(&mut self, name: &str) -> Result<(), HelperError> {
        super::delete(name)
    }
}

/// What a successful restore changed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RestoreSummary {
    pub overwritten: Vec<String>,
    pub created: Vec<String>,
    pub unchanged: Vec<String>,
}

impl RestoreSummary {
    pub fn total(&self) -> usize {
        self.overwritten.len() + self.created.len() + self.unchanged.len()
    }
}

impl fmt::Display for RestoreSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Restored {} tunnel(s) from backup ({} overwritten, {} new, {} unchanged)",
            self.total(),
            self.overwritten.len(),
            self.created.len(),
            self.unchanged.len()
        )
    }
}

#[derive(Debug)]
pub enum RestoreError {
    /// The archive itself was rejected; nothing was written.
    Archive(String),
    /// Current state could not be captured for rollback; nothing was written.
    Prepare(String),
    /// A write failed part-way; earlier writes were rolled back.
    Partial(Box<PartialRestore>),
}

/// What happened when a restore failed part-way.
#[derive(Debug)]
pub struct PartialRestore {
    pub tunnel: String,
    pub error: HelperError,
    /// Tunnels whose previous contents were put back.
    pub restored_previous: Vec<String>,
    /// Tunnels the restore had created and then removed again.
    pub removed_new: Vec<String>,
    /// Rollback steps that failed: these need manual attention.
    pub rollback_failed: Vec<(String, HelperError)>,
}

impl fmt::Display for RestoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Archive(error) => write!(f, "Restore failed: {error}\n\nNothing was changed."),
            Self::Prepare(error) => write!(f, "Restore failed: {error}\n\nNothing was changed."),
            Self::Partial(partial) => {
                let PartialRestore {
                    tunnel,
                    error,
                    restored_previous,
                    removed_new,
                    rollback_failed,
                } = partial.as_ref();
                writeln!(f, "Restore failed while writing {tunnel}:\n{error}\n")?;
                if error.is_timeout() {
                    writeln!(
                        f,
                        "The helper timed out, so {tunnel} may have been written; it was rolled back too.\n"
                    )?;
                }
                if restored_previous.is_empty()
                    && removed_new.is_empty()
                    && rollback_failed.is_empty()
                {
                    writeln!(f, "No tunnel had been written yet; nothing was changed.")?;
                }
                if !restored_previous.is_empty() {
                    writeln!(
                        f,
                        "Previous contents restored: {}",
                        restored_previous.join(", ")
                    )?;
                }
                if !removed_new.is_empty() {
                    writeln!(
                        f,
                        "Removed again (did not exist before): {}",
                        removed_new.join(", ")
                    )?;
                }
                if rollback_failed.is_empty() {
                    write!(f, "Your tunnels are as they were before the restore.")
                } else {
                    writeln!(
                        f,
                        "\nRollback FAILED for these tunnels - check them manually:"
                    )?;
                    for (name, error) in rollback_failed {
                        writeln!(f, "  {name}: {error}")?;
                    }
                    Ok(())
                }
            }
        }
    }
}

/// Restore every `.conf` inside a backup archive (overwriting tunnels of the
/// same name). If any write fails, roll back the writes already made.
pub fn restore_backup(path: &Path) -> Result<RestoreSummary, RestoreError> {
    let file = open_backup(path).map_err(RestoreError::Archive)?;
    let configs = backup_configs(file).map_err(RestoreError::Archive)?;
    restore_configs(&mut HelperStore, &configs)
}

pub fn restore_configs(
    store: &mut impl TunnelStore,
    configs: &[(String, String)],
) -> Result<RestoreSummary, RestoreError> {
    let existing: std::collections::HashSet<String> = store
        .names()
        .map_err(|e| RestoreError::Prepare(format!("couldn't list the current tunnels: {e}")))?
        .into_iter()
        .collect();
    // Capture every config about to be overwritten before the first write, so
    // a failure part-way through can always put it back.
    let mut plan = Vec::new();
    for (name, content) in configs {
        let previous = if existing.contains(name) {
            Some(store.read(name).map_err(|e| {
                RestoreError::Prepare(format!(
                    "couldn't read the current {name} to keep a rollback copy: {e}"
                ))
            })?)
        } else {
            None
        };
        plan.push((name, content, previous));
    }
    let mut summary = RestoreSummary::default();
    let mut written: Vec<(&String, Option<&String>)> = Vec::new();
    for (name, content, previous) in &plan {
        if previous.as_ref() == Some(*content) {
            summary.unchanged.push(name.to_string());
            continue;
        }
        match store.save(name, content) {
            Ok(()) => {
                written.push((*name, previous.as_ref()));
                match previous {
                    Some(_) => summary.overwritten.push(name.to_string()),
                    None => summary.created.push(name.to_string()),
                }
            }
            Err(error) => {
                // A timeout leaves this write's outcome unknown: undo it too.
                if error.is_timeout() {
                    written.push((*name, previous.as_ref()));
                }
                let mut restored_previous = Vec::new();
                let mut removed_new = Vec::new();
                let mut rollback_failed = Vec::new();
                for (done, before) in written.into_iter().rev() {
                    let undo = match before {
                        Some(text) => store.save(done, text),
                        None => store.delete(done),
                    };
                    match (undo, before) {
                        (Ok(()), Some(_)) => restored_previous.push(done.to_string()),
                        (Ok(()), None) => removed_new.push(done.to_string()),
                        (Err(e), _) => rollback_failed.push((done.to_string(), e)),
                    }
                }
                crate::logging::event(
                    "restore.rollback",
                    &[
                        ("failed", name.as_str()),
                        ("restored_previous", &restored_previous.join(",")),
                        ("removed_new", &removed_new.join(",")),
                        ("rollback_failed", &rollback_failed.len().to_string()),
                    ],
                );
                return Err(RestoreError::Partial(Box::new(PartialRestore {
                    tunnel: name.to_string(),
                    error,
                    restored_previous,
                    removed_new,
                    rollback_failed,
                })));
            }
        }
    }
    Ok(summary)
}

/// Delete a backup archive (only within the backup directory).
pub fn delete_backup(path: &Path) -> Result<(), String> {
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;
    let _file = open_backup(path)?;
    let dir = backup_dir()?;
    let directory = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(dir)
        .map_err(|e| e.to_string())?;
    let name = std::ffi::CString::new(
        path.file_name()
            .ok_or("Missing backup filename.")?
            .as_bytes(),
    )
    .map_err(|e| e.to_string())?;
    let result = unsafe { libc::unlinkat(directory.as_raw_fd(), name.as_ptr(), 0) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().to_string())
    }
}

pub(crate) fn write_private(dest: &Path, data: &[u8]) -> Result<(), String> {
    atomic_private_write(dest, true, |file| write_payload(file, data))
}

pub fn export_backup_to(src: &Path, dest: &Path) -> Result<(), String> {
    let data = safe_file::read_limited(open_backup(src)?, Rules::BACKUP)?;
    write_private(dest, &data)
}

pub fn save_text_to(dest: &Path, text: &str) -> Result<(), String> {
    write_private(dest, text.as_bytes())
}

#[cfg(test)]
mod tests;
