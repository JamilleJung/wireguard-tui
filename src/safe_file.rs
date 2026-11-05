//! One routine for opening user-supplied or private files safely.
//!
//! Every read of a config, QR image or backup archive goes through
//! [`open_regular`] / [`open_in_dir`]: the file is opened with `O_NONBLOCK` (a
//! named pipe, socket or device can never hold the operation open), optionally
//! `O_NOFOLLOW`, then `fstat`ed on the open descriptor, required to be a regular
//! file within a size limit, and only then read with a bounded reader.

use std::ffi::CString;
use std::fmt;
use std::fs::File;
use std::io::{self, Read};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, OpenOptionsExt};
use std::path::Path;

/// Limits applied to a single open.
#[derive(Clone, Copy, Debug)]
pub struct Rules {
    /// What the file is, for messages ("Config", "QR image", "Backup").
    pub what: &'static str,
    /// Maximum accepted size in bytes.
    pub limit: u64,
    /// Refuse a symbolic link as the final path component.
    pub no_follow: bool,
}

impl Rules {
    pub const CONFIG: Rules = Rules {
        what: "Config",
        limit: 1024 * 1024,
        no_follow: false,
    };
    pub const QR_IMAGE: Rules = Rules {
        what: "QR image",
        limit: 32 * 1024 * 1024,
        no_follow: false,
    };
    pub const BACKUP: Rules = Rules {
        what: "Backup",
        limit: 64 * 1024 * 1024,
        no_follow: true,
    };

    pub const fn private(self) -> Rules {
        Rules {
            no_follow: true,
            ..self
        }
    }
}

/// Why a file could not be opened or read.
#[derive(Debug)]
pub enum SafeFileError {
    /// `open(2)` failed (missing, permission denied, ...).
    Open {
        what: &'static str,
        error: io::Error,
    },
    /// The path names something other than a regular file.
    NotRegular {
        what: &'static str,
        found: &'static str,
    },
    /// The file is larger than the configured limit.
    TooLarge { what: &'static str, limit: u64 },
    /// Reading the opened file failed.
    Read {
        what: &'static str,
        error: io::Error,
    },
    /// Text was requested but the file is not UTF-8.
    NotUtf8 { what: &'static str },
}

impl SafeFileError {
    /// The underlying I/O error kind, when there is one.
    pub fn io_kind(&self) -> Option<io::ErrorKind> {
        match self {
            Self::Open { error, .. } | Self::Read { error, .. } => Some(error.kind()),
            _ => None,
        }
    }
}

pub fn fmt_limit(limit: u64) -> String {
    if limit >= 1024 * 1024 && limit.is_multiple_of(1024 * 1024) {
        format!("{} MiB", limit / (1024 * 1024))
    } else if limit >= 1024 && limit.is_multiple_of(1024) {
        format!("{} KiB", limit / 1024)
    } else {
        format!("{limit} bytes")
    }
}

impl fmt::Display for SafeFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open { what, error } if error.kind() == io::ErrorKind::PermissionDenied => {
                write!(f, "Permission denied opening the {}", what.to_lowercase())
            }
            Self::Open { what, error } => {
                write!(f, "Couldn't open the {}: {error}", what.to_lowercase())
            }
            Self::NotRegular { what, found } => {
                write!(f, "{what} must be a regular file, not a {found}")
            }
            Self::TooLarge { what, limit } => {
                write!(f, "{what} exceeds the {} limit", fmt_limit(*limit))
            }
            Self::Read { what, error } => {
                write!(f, "Couldn't read the {}: {error}", what.to_lowercase())
            }
            Self::NotUtf8 { what } => write!(f, "{what} is not valid UTF-8 text"),
        }
    }
}

impl std::error::Error for SafeFileError {}

impl From<SafeFileError> for String {
    fn from(error: SafeFileError) -> Self {
        error.to_string()
    }
}

fn open_flags(rules: Rules) -> i32 {
    let mut flags = libc::O_NONBLOCK | libc::O_CLOEXEC | libc::O_NOCTTY;
    if rules.no_follow {
        flags |= libc::O_NOFOLLOW;
    }
    flags
}

fn open_error(rules: Rules, error: io::Error) -> SafeFileError {
    // O_NOFOLLOW reports a final symlink as ELOOP; name it plainly.
    if rules.no_follow && error.raw_os_error() == Some(libc::ELOOP) {
        return SafeFileError::NotRegular {
            what: rules.what,
            found: "symbolic link",
        };
    }
    SafeFileError::Open {
        what: rules.what,
        error,
    }
}

/// Open `path` read-only without blocking and validate it is a regular file
/// within `rules.limit`.
pub fn open_regular(path: &Path, rules: Rules) -> Result<File, SafeFileError> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(open_flags(rules))
        .open(path)
        .map_err(|error| open_error(rules, error))?;
    validate(file, rules)
}

/// Open `name` relative to an already-open directory (`openat`), refusing
/// symlinks, then validate it like [`open_regular`].
pub fn open_in_dir(
    directory: &File,
    name: &std::ffi::OsStr,
    rules: Rules,
) -> Result<File, SafeFileError> {
    let c_name = CString::new(name.as_bytes()).map_err(|_| SafeFileError::Open {
        what: rules.what,
        error: io::Error::new(io::ErrorKind::InvalidInput, "file name contains NUL"),
    })?;
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            c_name.as_ptr(),
            libc::O_RDONLY | open_flags(rules.private()),
        )
    };
    if fd < 0 {
        return Err(open_error(rules.private(), io::Error::last_os_error()));
    }
    validate(unsafe { File::from_raw_fd(fd) }, rules)
}

fn file_kind(meta: &std::fs::Metadata) -> &'static str {
    let kind = meta.file_type();
    if kind.is_dir() {
        "directory"
    } else if kind.is_fifo() {
        "named pipe"
    } else if kind.is_socket() {
        "socket"
    } else if kind.is_char_device() || kind.is_block_device() {
        "device"
    } else if kind.is_symlink() {
        "symbolic link"
    } else {
        "special file"
    }
}

fn validate(file: File, rules: Rules) -> Result<File, SafeFileError> {
    let meta = file.metadata().map_err(|error| SafeFileError::Read {
        what: rules.what,
        error,
    })?;
    if !meta.is_file() {
        return Err(SafeFileError::NotRegular {
            what: rules.what,
            found: file_kind(&meta),
        });
    }
    if meta.len() > rules.limit {
        return Err(SafeFileError::TooLarge {
            what: rules.what,
            limit: rules.limit,
        });
    }
    // Regular files ignore O_NONBLOCK, but clear it so the descriptor behaves
    // normally for any reader it is handed to.
    let fd = file.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags >= 0 {
        unsafe { libc::fcntl(fd, libc::F_SETFL, flags & !libc::O_NONBLOCK) };
    }
    Ok(file)
}

/// Read an opened file completely, never more than `rules.limit` bytes (a file
/// that grows after the size check is still bounded).
pub fn read_limited(file: File, rules: Rules) -> Result<Vec<u8>, SafeFileError> {
    let mut bytes = Vec::new();
    file.take(rules.limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| SafeFileError::Read {
            what: rules.what,
            error,
        })?;
    if bytes.len() as u64 > rules.limit {
        return Err(SafeFileError::TooLarge {
            what: rules.what,
            limit: rules.limit,
        });
    }
    Ok(bytes)
}

/// Open, validate and read a whole file as bytes.
pub fn read_bytes(path: &Path, rules: Rules) -> Result<Vec<u8>, SafeFileError> {
    read_limited(open_regular(path, rules)?, rules)
}

/// Open, validate and read a whole file as UTF-8 text.
pub fn read_text(path: &Path, rules: Rules) -> Result<String, SafeFileError> {
    String::from_utf8(read_bytes(path, rules)?)
        .map_err(|_| SafeFileError::NotUtf8 { what: rules.what })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    pub(crate) fn scratch() -> std::path::PathBuf {
        crate::test_support::scratch_dir("safe-file")
    }

    #[test]
    fn reads_regular_files_and_enforces_the_limit() {
        let dir = scratch();
        let path = dir.join("tunnel.conf");
        std::fs::write(&path, "example config").unwrap();
        assert_eq!(read_text(&path, Rules::CONFIG).unwrap(), "example config");
        let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.set_len(1024 * 1024 + 1).unwrap();
        let error = read_text(&path, Rules::CONFIG).unwrap_err();
        assert!(matches!(error, SafeFileError::TooLarge { .. }));
        assert!(error.to_string().contains("1 MiB"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn special_files_are_refused_without_blocking() {
        let dir = scratch();
        let fifo = dir.join("pipe.conf");
        let c = CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        let socket_path = dir.join("socket.conf");
        let _listener = std::os::unix::net::UnixListener::bind(&socket_path).unwrap();
        let started = Instant::now();
        for (path, found) in [
            (fifo.as_path(), "named pipe"),
            (socket_path.as_path(), "socket"),
            (dir.as_path(), "directory"),
            (Path::new("/dev/null"), "device"),
            (Path::new("/dev/zero"), "device"),
        ] {
            for rules in [Rules::CONFIG, Rules::QR_IMAGE] {
                match read_bytes(path, rules) {
                    // A socket cannot be open(2)ed at all on Linux (ENXIO).
                    Err(SafeFileError::Open { .. }) if found == "socket" => {}
                    Err(SafeFileError::NotRegular { found: actual, .. }) => {
                        assert_eq!(actual, found, "{}", path.display())
                    }
                    other => panic!("{}: unexpected {other:?}", path.display()),
                }
            }
        }
        assert!(started.elapsed() < Duration::from_secs(2));
        let message = read_text(&fifo, Rules::CONFIG).unwrap_err().to_string();
        assert!(message.contains("regular file"), "{message}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn private_rules_refuse_symlinks() {
        let dir = scratch();
        let real = dir.join("real.conf");
        let link = dir.join("link.conf");
        std::fs::write(&real, "data").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert_eq!(read_text(&link, Rules::CONFIG).unwrap(), "data");
        assert!(matches!(
            read_text(&link, Rules::CONFIG.private()),
            Err(SafeFileError::NotRegular {
                found: "symbolic link",
                ..
            })
        ));
        let directory = File::open(&dir).unwrap();
        assert!(open_in_dir(&directory, std::ffi::OsStr::new("link.conf"), Rules::BACKUP).is_err());
        assert!(open_in_dir(&directory, std::ffi::OsStr::new("real.conf"), Rules::BACKUP).is_ok());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn permission_denied_and_missing_files_are_typed() {
        let dir = scratch();
        let missing = read_text(&dir.join("missing.conf"), Rules::CONFIG).unwrap_err();
        assert_eq!(missing.io_kind(), Some(io::ErrorKind::NotFound));
        // Root bypasses mode bits, so exercise the real path only when the test
        // runs unprivileged and check the classification either way.
        if unsafe { libc::geteuid() } != 0 {
            use std::os::unix::fs::PermissionsExt;
            let locked = dir.join("locked.conf");
            std::fs::write(&locked, "secret").unwrap();
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
            let error = read_text(&locked, Rules::CONFIG).unwrap_err();
            assert_eq!(error.io_kind(), Some(io::ErrorKind::PermissionDenied));
        }
        let denied = open_error(Rules::CONFIG, io::Error::from_raw_os_error(libc::EACCES));
        assert_eq!(denied.io_kind(), Some(io::ErrorKind::PermissionDenied));
        assert_eq!(denied.to_string(), "Permission denied opening the config");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn non_utf8_text_is_rejected() {
        let dir = scratch();
        let path = dir.join("binary.conf");
        std::fs::write(&path, [0xff, 0xfe]).unwrap();
        assert!(matches!(
            read_text(&path, Rules::CONFIG),
            Err(SafeFileError::NotUtf8 { .. })
        ));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
