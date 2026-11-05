use super::*;
use std::time::Duration;

const KEY: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopo=";

fn draft(text: &str) -> Draft {
    Draft {
        name: "wg0".into(),
        original: format!("[Interface]\nPrivateKey = {KEY}\n"),
        text: text.into(),
        new_kind: None,
    }
}

#[test]
fn helper_action_keeps_ui_state_and_serializes_until_result_is_consumed() {
    let mut app = App::default();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    assert!(start_action(
        &mut app,
        OpKind::Save,
        "Working".into(),
        move || {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Report::ok("Finished")
        }
    ));
    started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(!start_action(
        &mut app,
        OpKind::Delete,
        "Duplicate".into(),
        || panic!("overlapping worker")
    ));
    app.mode = Mode::LogFilter("unsaved filter".into());
    release_tx.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    while app.update_out.lock().unwrap().is_empty() {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    app.drain_update();
    assert!(matches!(&app.mode, Mode::LogFilter(value) if value == "unsaved filter"));
    assert!(app.op.is_busy());
    app.mode = Mode::Normal;
    app.drain_update();
    assert!(!app.op.is_busy());
    assert_eq!(app.status, "Finished");
}

#[test]
fn invalid_edit_keeps_the_draft_text() {
    let (result, kept) = check_edited(draft("not a config at all"));
    assert!(matches!(result, EditedDraft::Invalid(_)));
    assert_eq!(kept.unwrap().text, "not a config at all");
    let mut app = App::default();
    submit_draft(&mut app, draft("still not valid"));
    match &app.mode {
        Mode::DraftRecovery {
            draft,
            problem: DraftProblem::Invalid,
            error,
        } => {
            assert_eq!(draft.text, "still not valid");
            assert!(error.contains("invalid config"));
        }
        _ => panic!("draft was not kept"),
    }
    let unchanged = draft(&draft("").original);
    assert_eq!(check_edited(unchanged).0, EditedDraft::Unchanged);
}

#[test]
fn failed_saves_keep_the_draft_and_timeouts_are_reconciled() {
    let d = draft("edited");
    let denied = HelperError::Failed {
        command: "wg-helper save wg0".into(),
        stderr: "Permission denied".into(),
    };
    let (outcome, message) = reconcile_save(&d, &denied, Ok("old".into())).unwrap();
    assert_eq!(outcome, Outcome::Failed);
    assert!(message.contains("Permission denied"));
    let busy = HelperError::Busy("another helper operation is busy".into());
    assert!(reconcile_save(&d, &busy, Ok("edited".into())).is_some());
    // Timed out, but the file holds the draft: it was saved.
    assert!(reconcile_save(&d, &HelperError::Timeout, Ok("edited".into())).is_none());
    let (outcome, _) = reconcile_save(&d, &HelperError::Timeout, Ok("old".into())).unwrap();
    assert_eq!(outcome, Outcome::Failed);
    let (outcome, message) =
        reconcile_save(&d, &HelperError::Timeout, Err(HelperError::Timeout)).unwrap();
    assert_eq!(outcome, Outcome::Unknown);
    assert!(message.contains("draft is kept"));
    // The worker report carries the whole draft back to the popup.
    let report = draft_failed(d.clone(), Outcome::Failed, message);
    assert!(matches!(
        report.mode,
        Some(Mode::DraftRecovery { draft, problem: DraftProblem::SaveFailed, .. }) if draft == d
    ));
}

#[test]
fn sync_failure_never_claims_success() {
    assert_eq!(sync_report("wg0", Ok(())).outcome, Outcome::Succeeded);
    let refused = sync_report(
        "wg0",
        Err(HelperError::Failed {
            command: "wg-helper sync wg0".into(),
            stderr: "FwMark changed; reconnect to apply it".into(),
        }),
    );
    assert_eq!(refused.outcome, Outcome::Failed);
    assert!(matches!(
        refused.mode,
        Some(Mode::SyncFailed {
            needs_reconnect: true,
            ..
        })
    ));
    let timeout = sync_report("wg0", Err(HelperError::Timeout));
    match timeout.mode {
        Some(Mode::SyncFailed {
            error,
            needs_reconnect: false,
            ..
        }) => {
            assert!(error.contains("may or may not"))
        }
        _ => panic!("timeout must offer recovery"),
    }
}

#[test]
fn activation_failure_message_preserves_multiline_unknown_error() {
    let settled = connection_settled(
        "Drite",
        false,
        &Err(HelperError::Failed {
            command: "wg-helper up Drite".into(),
            stderr: "wg-quick up Drite:\n[#] ip link add Drite\nRTNETLINK failure".into(),
        }),
        Ok(false),
    );
    assert!(settled.message.starts_with("Couldn't activate Drite:"));
    assert!(
        settled
            .message
            .contains("[#] ip link add Drite\nRTNETLINK failure")
    );
}

#[test]
fn connect_and_disconnect_are_verified_against_the_interface_list() {
    let up = connection_settled("wg0", false, &Ok(()), Ok(true));
    assert_eq!(
        (up.outcome, up.message.as_str()),
        (Outcome::Succeeded, "Activated wg0")
    );
    assert!(
        !up.message.contains("NTP"),
        "no clock claim without evidence"
    );
    let not_up = connection_settled("wg0", false, &Ok(()), Ok(false));
    assert_eq!(not_up.outcome, Outcome::Failed);
    assert!(not_up.message.contains("wg0 is not up"));
    let still_up = connection_settled("wg0", true, &Ok(()), Ok(true));
    assert!(still_up.message.contains("still up"));
    let timeout = connection_settled("wg0", true, &Err(HelperError::Timeout), Ok(false));
    assert_eq!(timeout.outcome, Outcome::Succeeded);
    assert!(timeout.message.contains("wg0 is down"));
}

#[test]
fn rename_and_delete_are_verified_against_saved_names() {
    let expect = Expectation {
        done: "Renamed a -> b".into(),
        state: "b exists".into(),
        not_state: "the rename did not take effect".into(),
        failed: "Couldn't rename a".into(),
    };
    let names = |list: &[&str]| Ok(list.iter().map(|s| s.to_string()).collect());
    assert_eq!(
        names_settled(&Ok(()), names(&["b"]), &["b"], &["a"], &expect).outcome,
        Outcome::Succeeded
    );
    let both = names_settled(&Ok(()), names(&["a", "b"]), &["b"], &["a"], &expect);
    assert_eq!(both.outcome, Outcome::Failed);
    assert!(both.message.contains("did not take effect"));
    let timed_out = names_settled(
        &Err(HelperError::Timeout),
        names(&["b"]),
        &["b"],
        &["a"],
        &expect,
    );
    assert_eq!(timed_out.outcome, Outcome::Succeeded);
}

#[test]
fn kill_switch_repair_report_is_based_on_the_recheck() {
    assert_eq!(
        repair_report("wg0", &Ok(()), &KillSwitch::Enabled).outcome,
        Outcome::Succeeded
    );
    let still = repair_report("wg0", &Ok(()), &KillSwitch::Missing);
    assert_eq!(still.outcome, Outcome::Failed);
    assert!(still.message.contains("still not in force"));
    let failed = repair_report("wg0", &Err(HelperError::Timeout), &KillSwitch::Mismatch);
    assert!(failed.message.contains("timed out"));
    assert!(failed.message.contains("MISMATCH"));
}

#[test]
fn config_file_reads_are_bounded_and_never_wait_for_named_pipes() {
    use std::os::unix::ffi::OsStrExt as _;
    let directory = crate::test_support::scratch_dir("config-read");
    let path = directory.join("example.conf");
    std::fs::write(&path, "example config").unwrap();
    assert_eq!(read_config_file(&path).unwrap(), "example config");
    let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.set_len(1024 * 1024 + 1).unwrap();
    assert!(read_config_file(&path).unwrap_err().contains("1 MiB"));
    drop(file);
    std::fs::remove_file(&path).unwrap();
    let cpath = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(cpath.as_ptr(), 0o600) }, 0);
    let start = Instant::now();
    assert!(
        read_config_file(&path)
            .unwrap_err()
            .contains("regular file")
    );
    assert!(start.elapsed() < Duration::from_secs(2));
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn helper_timeout_in_a_worker_becomes_a_reconciled_report() {
    use crate::backend::helper::with_fake_helper;
    let dir = crate::test_support::scratch_dir("worker-timeout");
    // `up` hangs; `active` reports the interface up afterwards.
    let program = crate::test_support::fake_helper(
        &dir,
        "case \"$1\" in\nup) sleep 30;;\nactive) echo wg0;;\nesac",
    );
    let settled = with_fake_helper(&program, Duration::from_millis(200), || {
        let result = backend::activate("wg0");
        assert!(result.as_ref().unwrap_err().is_timeout());
        connection_settled("wg0", false, &result, is_up("wg0"))
    });
    assert_eq!(settled.outcome, Outcome::Succeeded);
    assert!(
        settled
            .message
            .contains("timed out, but re-checking shows wg0 is up")
    );
    std::fs::remove_dir_all(dir).unwrap();
}
