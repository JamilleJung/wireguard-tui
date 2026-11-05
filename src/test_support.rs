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
    let path = dir.join("wg-helper");
    write_script(&path, &format!("#!/bin/sh\n{body}\n"), 0o700);
    path
}

/// Write an executable script from a short-lived child process. Writing it
/// from this (multi-threaded) test process would let a concurrent test fork
/// inherit the open write handle, and exec would fail with ETXTBSY.
fn write_script(path: &std::path::Path, content: &str, mode: u32) {
    use std::io::Write;
    let mut child = std::process::Command::new("/bin/sh")
        .args(["-c", "cat > \"$1\" && chmod \"$2\" \"$1\"", "sh"])
        .arg(path)
        .arg(format!("{mode:o}"))
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(content.as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success());
}
