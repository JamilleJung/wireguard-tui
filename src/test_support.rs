//! Shared helpers for unit tests.

use std::path::PathBuf;

/// A fresh private directory under the temp dir (mkdtemp, mode 0700).
pub fn scratch_dir(tag: &str) -> PathBuf {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let template = std::env::temp_dir().join(format!("wireguard-{tag}-test-XXXXXX"));
    let mut bytes = std::ffi::CString::new(template.as_os_str().as_bytes())
        .unwrap()
        .into_bytes_with_nul();
    assert!(!unsafe { libc::mkdtemp(bytes.as_mut_ptr().cast()) }.is_null());
    bytes.pop();
    PathBuf::from(std::ffi::OsString::from_vec(bytes))
        .canonicalize()
        .unwrap()
}

/// Write an executable shell script (a fake `wg-helper`) into `dir`.
pub fn fake_helper(dir: &std::path::Path, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("wg-helper");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}
