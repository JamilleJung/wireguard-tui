use super::*;
use crate::test_support::scratch_dir;
use std::collections::BTreeMap;

const KEY: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopo=";

fn scratch() -> PathBuf {
    scratch_dir("backup")
}

#[test]
fn atomic_exports_preserve_old_data_and_other_hardlinks_on_failure() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch();
    let dest = dir.join("export.zip");
    let other = dir.join("other-file");
    std::fs::write(&dest, b"old archive").unwrap();
    std::fs::hard_link(&dest, &other).unwrap();
    let failed: Result<(), String> = atomic_private_write(&dest, true, |file| {
        file.write_all(b"partial secret").unwrap();
        Err("simulated full disk".into())
    });
    assert!(failed.is_err());
    assert_eq!(std::fs::read(&dest).unwrap(), b"old archive");
    write_private(&dest, b"new secret").unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), b"new secret");
    assert_eq!(std::fs::read(&other).unwrap(), b"old archive");
    assert_eq!(dest.metadata().unwrap().permissions().mode() & 0o777, 0o600);
    assert!(
        atomic_private_write(&dest, false, |f| f
            .write_all(b"overwrite")
            .map_err(|e| e.to_string()))
        .is_err()
    );
    assert_eq!(std::fs::read(&dest).unwrap(), b"new secret");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn full_disk_writes_fail_cleanly_and_keep_the_old_file() {
    let Ok(mut full) = std::fs::OpenOptions::new().write(true).open("/dev/full") else {
        return; // not every platform has /dev/full
    };
    let error = write_payload(&mut full, &[0u8; 64 * 1024]).unwrap_err();
    assert!(error.contains("disk is full"), "{error}");
    // The same writer path inside an atomic publish: the destination keeps
    // its previous contents and no temporary file is left behind.
    let dir = scratch();
    let dest = dir.join("wireguard-tui-log.txt");
    std::fs::write(&dest, b"previous log").unwrap();
    let result = atomic_private_write(&dest, true, |_tmp| {
        let mut full = std::fs::OpenOptions::new()
            .write(true)
            .open("/dev/full")
            .unwrap();
        write_payload(&mut full, b"new log")
    });
    assert!(result.unwrap_err().contains("disk is full"));
    assert_eq!(std::fs::read(&dest).unwrap(), b"previous log");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    // A device can never be the destination itself.
    assert!(
        save_text_to(Path::new("/dev/full"), "log")
            .unwrap_err()
            .contains("regular file")
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn export_refuses_symlinks_and_never_silently_omits_failed_reads() {
    use std::os::unix::fs::symlink;
    let dir = scratch();
    let real = dir.join("real");
    let link = dir.join("link");
    std::fs::write(&real, b"do not touch").unwrap();
    symlink(&real, &link).unwrap();
    assert!(write_private(&link, b"secret").is_err());
    assert_eq!(std::fs::read(&real).unwrap(), b"do not touch");
    let result = collect_configs(["wg0".to_string(), "wg1".to_string()], |name| {
        if name == "wg0" {
            Ok("valid content".into())
        } else {
            Err("permission denied".into())
        }
    });
    assert_eq!(result.unwrap_err(), "permission denied");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn backup_storage_rejects_symlink_directories_and_linked_archives() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let base = scratch();
    let outside = scratch();
    symlink(&outside, base.join("wireguard")).unwrap();
    assert!(ensure_backup_dir(&base).is_err());
    assert!(!outside.join("backups").exists());
    std::fs::remove_file(base.join("wireguard")).unwrap();
    let dir = ensure_backup_dir(&base).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    ensure_backup_dir(&base).unwrap();
    assert_eq!(dir.metadata().unwrap().permissions().mode() & 0o777, 0o700);
    let real = dir.join("real.zip");
    let linked = dir.join("linked.zip");
    std::fs::write(&real, b"archive").unwrap();
    assert!(open_backup_in(&dir, &real).is_ok());
    symlink(&real, &linked).unwrap();
    assert!(open_backup_in(&dir, &linked).is_err());
    std::fs::remove_file(&linked).unwrap();
    std::fs::hard_link(&real, &linked).unwrap();
    assert!(open_backup_in(&dir, &real).is_err());
    std::fs::remove_file(&linked).unwrap();
    // Special files in the backup directory are refused without blocking.
    let fifo = dir.join("pipe.zip");
    let c =
        std::ffi::CString::new(std::os::unix::ffi::OsStrExt::as_bytes(fifo.as_os_str())).unwrap();
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    assert!(
        open_backup_in(&dir, &fifo)
            .unwrap_err()
            .contains("regular file")
    );
    std::fs::create_dir(dir.join("folder.zip")).unwrap();
    assert!(open_backup_in(&dir, &dir.join("folder.zip")).is_err());
    std::fs::remove_dir_all(base).unwrap();
    std::fs::remove_dir_all(outside).unwrap();
}

#[test]
fn zip_entry_budget_is_checked_before_eager_metadata_allocation() {
    let mut end = vec![0; 22];
    end[..4].copy_from_slice(b"PK\x05\x06");
    end[8..10].copy_from_slice(&4097u16.to_le_bytes());
    end[10..12].copy_from_slice(&4097u16.to_le_bytes());
    assert!(
        backup_configs(std::io::Cursor::new(end))
            .unwrap_err()
            .contains("too many")
    );
    let mut zip64 = vec![0; 56 + 20 + 22];
    zip64[..4].copy_from_slice(b"PK\x06\x06");
    zip64[4..12].copy_from_slice(&44u64.to_le_bytes());
    zip64[24..32].copy_from_slice(&4097u64.to_le_bytes());
    zip64[32..40].copy_from_slice(&4097u64.to_le_bytes());
    zip64[56..60].copy_from_slice(b"PK\x06\x07");
    zip64[76..80].copy_from_slice(b"PK\x05\x06");
    zip64[84..86].copy_from_slice(&u16::MAX.to_le_bytes());
    zip64[86..88].copy_from_slice(&u16::MAX.to_le_bytes());
    assert!(
        backup_configs(std::io::Cursor::new(zip64))
            .unwrap_err()
            .contains("too many")
    );
}

#[test]
fn small_fake_zip_footer_cannot_mask_an_oversized_fallback_directory() {
    let mut entries = vec![0; 44];
    entries[..4].copy_from_slice(b"PK\x05\x06");
    entries[8..10].copy_from_slice(&4097u16.to_le_bytes());
    entries[10..12].copy_from_slice(&4097u16.to_le_bytes());
    entries[20..22].copy_from_slice(&22u16.to_le_bytes());
    entries[22..26].copy_from_slice(b"PK\x05\x06");
    assert!(
        backup_configs(std::io::Cursor::new(entries))
            .unwrap_err()
            .contains("too many")
    );
    // A ZIP64 candidate can cross the scanner's chunk boundary and the
    // parser also accepts a 40-byte declared body before its later checks.
    let mut zip64 = vec![0; 64 * 1024 + 80];
    let index = 64 * 1024 - 20;
    zip64[index..index + 4].copy_from_slice(b"PK\x06\x06");
    zip64[index + 4..index + 12].copy_from_slice(&40u64.to_le_bytes());
    zip64[index + 24..index + 32].copy_from_slice(&4097u64.to_le_bytes());
    assert!(
        preflight_zip_directory(&mut std::io::Cursor::new(&zip64), zip64.len() as u64)
            .unwrap_err()
            .contains("too many")
    );
    let valid = format!("[Interface]\nPrivateKey = {KEY}\n");
    let mut trailing = backup_archive(&[("wg0.conf", &valid)]).into_inner();
    trailing.extend_from_slice(b"harmless trailing data");
    assert_eq!(
        backup_configs(std::io::Cursor::new(trailing))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn duplicate_central_directory_names_are_not_hidden_by_zip_indexing() {
    let valid = format!("[Interface]\nPrivateKey = {KEY}\n");
    let mut archive = backup_archive(&[("wg0.conf", &valid), ("wg1.conf", &valid)]).into_inner();
    for index in 0..archive.len().saturating_sub(8) {
        if &archive[index..index + 8] == b"wg1.conf" {
            archive[index + 2] = b'0';
        }
    }
    assert!(
        backup_configs(std::io::Cursor::new(archive))
            .unwrap_err()
            .contains("duplicate")
    );
}

#[test]
fn oversized_zip_is_rejected_before_parsing_metadata() {
    let dir = scratch();
    let path = dir.join("large.zip");
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(MAX_BACKUP_BYTES + 1).unwrap();
    drop(file);
    assert!(
        backup_configs(std::fs::File::open(path).unwrap())
            .unwrap_err()
            .contains("64 MiB")
    );
    std::fs::remove_dir_all(dir).unwrap();
}

pub(crate) fn backup_archive(files: &[(&str, &str)]) -> std::io::Cursor<Vec<u8>> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, content) in files {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(content.as_bytes()).unwrap();
    }
    zip.finish().unwrap()
}

#[test]
fn backup_restore_only_reads_tunnel_configs() {
    let valid = format!("[Interface]\nPrivateKey = {KEY}\n");
    let configs = backup_configs(backup_archive(&[
        ("README.txt", "not a config"),
        ("wg0.conf", &valid),
    ]))
    .unwrap();
    assert_eq!(configs, vec![("wg0".to_string(), valid)]);
    assert!(backup_configs(backup_archive(&[("notes.txt", "text")])).is_err());
}

#[test]
fn backup_restore_preflights_names_and_all_content() {
    let valid = format!("[Interface]\nPrivateKey = {KEY}\n");
    for name in ["../wg0.conf", "nested/wg0.conf", "wg 0.conf", " wg0.conf"] {
        assert!(
            backup_configs(backup_archive(&[(name, &valid)])).is_err(),
            "{name}"
        );
    }
    assert!(
        backup_configs(backup_archive(&[
            ("wg0.conf", &valid),
            ("wg1.conf", "invalid config"),
        ]))
        .is_err()
    );
    let scripted = format!("{valid}PostUp = id\n");
    assert!(backup_configs(backup_archive(&[("wg0.conf", &scripted)])).is_err());
}

#[test]
fn backup_restore_rejects_oversized_configs() {
    let oversized = "x".repeat(1024 * 1024 + 1);
    assert!(backup_configs(backup_archive(&[("wg0.conf", &oversized)])).is_err());
}

/// In-memory tunnels with scripted failures.
#[derive(Default)]
struct FakeStore {
    tunnels: BTreeMap<String, String>,
    fail_save: Option<(String, HelperError)>,
    fail_rollback: Option<String>,
    saves: Vec<String>,
}

impl TunnelStore for FakeStore {
    fn names(&mut self) -> Result<Vec<String>, HelperError> {
        Ok(self.tunnels.keys().cloned().collect())
    }
    fn read(&mut self, name: &str) -> Result<String, HelperError> {
        self.tunnels
            .get(name)
            .cloned()
            .ok_or_else(|| HelperError::Io("missing".into()))
    }
    fn save(&mut self, name: &str, content: &str) -> Result<(), HelperError> {
        self.saves.push(name.to_string());
        if let Some((failing, error)) = &self.fail_save
            && failing == name
        {
            let error = error.clone();
            if error.is_timeout() {
                // The write landed even though the helper timed out.
                self.tunnels.insert(name.into(), content.into());
            }
            self.fail_save = None;
            return Err(error);
        }
        if self.fail_rollback.as_deref() == Some(name)
            && self.saves.iter().filter(|s| *s == name).count() > 1
        {
            return Err(HelperError::Io("disk error".into()));
        }
        self.tunnels.insert(name.into(), content.into());
        Ok(())
    }
    fn delete(&mut self, name: &str) -> Result<(), HelperError> {
        self.tunnels.remove(name);
        Ok(())
    }
}

fn store(entries: &[(&str, &str)]) -> FakeStore {
    FakeStore {
        tunnels: entries
            .iter()
            .map(|(n, c)| (n.to_string(), c.to_string()))
            .collect(),
        ..FakeStore::default()
    }
}

fn configs(entries: &[(&str, &str)]) -> Vec<(String, String)> {
    entries
        .iter()
        .map(|(n, c)| (n.to_string(), c.to_string()))
        .collect()
}

#[test]
fn restore_reports_overwritten_created_and_unchanged_tunnels() {
    let mut fake = store(&[("home", "old home"), ("same", "same text")]);
    let summary = restore_configs(
        &mut fake,
        &configs(&[
            ("home", "new home"),
            ("work", "new work"),
            ("same", "same text"),
        ]),
    )
    .unwrap();
    assert_eq!(summary.overwritten, ["home"]);
    assert_eq!(summary.created, ["work"]);
    assert_eq!(summary.unchanged, ["same"]);
    assert!(summary.to_string().starts_with("Restored 3 tunnel(s)"));
    assert_eq!(fake.tunnels["home"], "new home");
}

#[test]
fn failed_restore_rolls_back_earlier_writes_exactly() {
    let mut fake = store(&[("home", "old home")]);
    fake.fail_save = Some((
        "third".into(),
        HelperError::Failed {
            command: "wg-helper save third".into(),
            stderr: "Permission denied".into(),
        },
    ));
    let error = restore_configs(
        &mut fake,
        &configs(&[("home", "new home"), ("work", "new work"), ("third", "x")]),
    )
    .unwrap_err();
    assert_eq!(fake.tunnels.len(), 1);
    assert_eq!(fake.tunnels["home"], "old home");
    let RestoreError::Partial(partial) = &error else {
        panic!("{error:?}");
    };
    let PartialRestore {
        tunnel,
        restored_previous,
        removed_new,
        rollback_failed,
        ..
    } = partial.as_ref();
    assert_eq!(tunnel, "third");
    assert_eq!(restored_previous, &["home"]);
    assert_eq!(removed_new, &["work"]);
    assert!(rollback_failed.is_empty());
    let text = error.to_string();
    assert!(text.contains("Permission denied"), "{text}");
    assert!(text.contains("Previous contents restored: home"), "{text}");
    assert!(
        text.contains("Removed again (did not exist before): work"),
        "{text}"
    );
    assert!(text.contains("as they were before"), "{text}");
}

#[test]
fn timed_out_restore_write_is_rolled_back_and_rollback_failures_are_named() {
    let mut fake = store(&[("home", "old home"), ("work", "old work")]);
    fake.fail_save = Some(("work".into(), HelperError::Timeout));
    fake.fail_rollback = Some("home".into());
    let error = restore_configs(
        &mut fake,
        &configs(&[("home", "new home"), ("work", "new work")]),
    )
    .unwrap_err();
    // The timed-out write landed, so it was undone too.
    assert_eq!(fake.tunnels["work"], "old work");
    // Undoing "home" failed: it keeps the new content and is reported.
    assert_eq!(fake.tunnels["home"], "new home");
    let text = error.to_string();
    assert!(text.contains("may have been written"), "{text}");
    assert!(text.contains("Rollback FAILED"), "{text}");
    assert!(text.contains("home: disk error"), "{text}");
}

#[test]
fn restore_writes_nothing_when_a_rollback_copy_cannot_be_read() {
    struct Unreadable(FakeStore);
    impl TunnelStore for Unreadable {
        fn names(&mut self) -> Result<Vec<String>, HelperError> {
            self.0.names()
        }
        fn read(&mut self, _: &str) -> Result<String, HelperError> {
            Err(HelperError::Timeout)
        }
        fn save(&mut self, name: &str, content: &str) -> Result<(), HelperError> {
            self.0.save(name, content)
        }
        fn delete(&mut self, name: &str) -> Result<(), HelperError> {
            self.0.delete(name)
        }
    }
    let mut fake = Unreadable(store(&[("home", "old")]));
    let error = restore_configs(&mut fake, &configs(&[("new", "a"), ("home", "b")])).unwrap_err();
    assert!(matches!(error, RestoreError::Prepare(_)));
    assert!(fake.0.saves.is_empty());
    assert!(error.to_string().contains("Nothing was changed"));
}
