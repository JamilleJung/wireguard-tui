//! Background operations. Each one runs on a worker thread, re-checks the
//! real state afterwards instead of trusting the helper's exit status, and
//! reports back through the mailbox as a [`Report`].

use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::backend::{self, HelperError, KillSwitch};
use crate::ops::{self, Expectation, OpKind, Outcome, Settled};
use crate::safe_file::{self, Rules};
use crate::state::{App, Draft, DraftProblem, EditorRequest, FileEntry, Mode, Report, UpdateMsg};
use crate::{create, doctor, logging};

/// Keep the event loop alive while a helper may authenticate or wait for
/// wg-quick. Modal input and selection stay on the UI thread; results queue.
pub fn start_action(
    app: &mut App,
    kind: OpKind,
    message: String,
    work: impl FnOnce() -> Report + Send + 'static,
) -> bool {
    if let Err(ops::Busy(running)) = app.op.begin(kind, Instant::now()) {
        app.flash(format!(
            "Another operation is still running ({})",
            running.label()
        ));
        return false;
    }
    logging::event("operation.start", &[("kind", kind.label())]);
    app.flash(message);
    app.status_at = None;
    let out = app.update_out.clone();
    std::thread::spawn(move || {
        let report = work();
        out.lock().unwrap().push_back(UpdateMsg::Finished(report));
    });
    true
}

/// Like [`start_action`], selecting the named tunnel when it appears unless
/// the user moved the selection in the meantime.
fn start_selected_action(
    app: &mut App,
    kind: OpKind,
    message: String,
    work: impl FnOnce() -> Result<(String, String), String> + Send + 'static,
) {
    let generation = app.selection_generation;
    start_action(app, kind, message, move || match work() {
        Ok((message, name)) => Report {
            select: Some((name, generation)),
            ..Report::ok(message)
        },
        Err(error) => Report::failed(error),
    });
}

/// Run a read-only fetch and show its result as a popup mode.
fn start_presentation(
    app: &mut App,
    message: String,
    work: impl FnOnce() -> Result<Mode, String> + Send + 'static,
) {
    start_action(app, OpKind::Read, message, move || match work() {
        Ok(mode) => Report::with_mode(Outcome::Succeeded, mode),
        Err(error) => Report::failed(error),
    });
}

/// Run a connection diagnosis for the selected tunnel off the UI thread (it
/// pings/resolves and can take a few seconds), delivering the report via the
/// shared mailbox so the loop can pop it into a Message popup when it's ready.
pub fn diagnose(app: &mut App) {
    let Some(name) = app.selected_name() else {
        app.flash("Select a tunnel to diagnose");
        return;
    };
    if app
        .diagnosis
        .begin(OpKind::Diagnose, Instant::now())
        .is_err()
    {
        app.flash("Diagnosis already in progress");
        return;
    }
    app.flash(format!("Diagnosing {name}…"));
    let out = app.update_out.clone();
    std::thread::spawn(move || {
        let report = backend::diagnose_report(&name);
        out.lock().unwrap().push_back(UpdateMsg::Diagnosed(report));
    });
}

/// Verify a connect (`was_active == false`) or disconnect against the
/// interface list read afterwards.
pub fn connection_settled(
    name: &str,
    was_active: bool,
    result: &Result<(), HelperError>,
    observed_up: Result<bool, HelperError>,
) -> Settled {
    let expect = if was_active {
        Expectation {
            done: format!("Deactivated {name}"),
            state: format!("{name} is down"),
            not_state: format!("{name} is still up"),
            failed: format!("Couldn't deactivate {name}"),
        }
    } else {
        Expectation {
            done: format!("Activated {name}"),
            state: format!("{name} is up"),
            not_state: format!("{name} is not up"),
            failed: format!("Couldn't activate {name}"),
        }
    };
    ops::settle(
        result,
        observed_up.map(|up| up != was_active),
        &expect,
        |error| doctor::friendly_error(&error.to_string()),
    )
}

fn is_up(name: &str) -> Result<bool, HelperError> {
    backend::active_names().map(|names| names.contains(name))
}

pub fn toggle_active(app: &mut App) {
    let Some(tunnel) = app.state.selected().and_then(|idx| app.tunnels.get(idx)) else {
        return;
    };
    let (name, active) = (tunnel.name.clone(), tunnel.active);
    start_action(
        app,
        if active {
            OpKind::Disconnect
        } else {
            OpKind::Connect
        },
        format!(
            "{} {name}…",
            if active { "Deactivating" } else { "Activating" }
        ),
        move || {
            let result = if active {
                backend::deactivate(&name)
            } else {
                backend::activate(&name)
            };
            let settled = connection_settled(&name, active, &result, is_up(&name));
            let activated =
                (!active && settled.outcome == Outcome::Succeeded).then(|| name.clone());
            Report {
                activated,
                ..Report::from_settled(settled)
            }
        },
    );
}

/// Bring a tunnel down and up again, verifying it ends up running.
pub fn reconnect(app: &mut App, name: String) {
    start_action(
        app,
        OpKind::Reconnect,
        format!("Reconnecting {name}…"),
        move || {
            let down = backend::deactivate(&name);
            let down_settled = connection_settled(&name, true, &down, is_up(&name));
            if down_settled.outcome != Outcome::Succeeded {
                return Report::from_settled(Settled {
                    popup: true,
                    ..down_settled
                });
            }
            let up = backend::activate(&name);
            let settled = connection_settled(&name, false, &up, is_up(&name));
            let activated = (settled.outcome == Outcome::Succeeded).then(|| name.clone());
            let message = if settled.outcome == Outcome::Succeeded {
                format!("Reconnected {name}; the running interface now matches the saved config")
            } else {
                settled.message.clone()
            };
            Report {
                activated,
                ..Report::from_settled(Settled { message, ..settled })
            }
        },
    );
}

pub fn toggle_autostart(app: &mut App) {
    let Some(detail) = &app.detail else { return };
    let (name, want) = (detail.name.clone(), !detail.autostart);
    start_action(
        app,
        OpKind::Autostart,
        format!("Updating start-on-boot for {name}…"),
        move || {
            let word = if want { "enabled" } else { "disabled" };
            let result = backend::set_autostart(&name, want);
            let observed = backend::autostart_status(&name).map(|on| on == want);
            Report::from_settled(ops::settle(
                &result,
                observed,
                &Expectation {
                    done: format!("Start on boot {word} for {name}"),
                    state: format!("start on boot is {word}"),
                    not_state: format!("start on boot is not {word}"),
                    failed: format!("Couldn't change start on boot for {name}"),
                },
                |e| e.to_string(),
            ))
        },
    );
}

pub fn toggle_killswitch(app: &mut App) {
    let Some(detail) = &app.detail else { return };
    if !detail.active {
        app.flash("Kill switch needs an active tunnel");
        return;
    }
    let name = detail.name.clone();
    match detail.killswitch.clone() {
        KillSwitch::Enabled => set_killswitch(app, name, false),
        KillSwitch::Disabled => set_killswitch(app, name, true),
        status @ (KillSwitch::Missing | KillSwitch::Mismatch) => {
            app.mode = Mode::KillSwitchProblem { name, status };
        }
        KillSwitch::Unknown => {
            app.mode = Mode::Message(format!(
                "The kill switch state of {name} could not be read, so it is not toggled.\n\nPress r to refresh, or D to diagnose."
            ));
        }
    }
}

pub fn set_killswitch(app: &mut App, name: String, on: bool) {
    start_action(
        app,
        OpKind::KillSwitch,
        format!("Updating kill switch for {name}…"),
        move || {
            let word = if on { "enabled" } else { "disabled" };
            let result = backend::set_killswitch(&name, on);
            let status = backend::killswitch_status(&name);
            let wanted = if on {
                KillSwitch::Enabled
            } else {
                KillSwitch::Disabled
            };
            let observed = if status == KillSwitch::Unknown {
                Err(HelperError::Io("kill switch status unavailable".into()))
            } else {
                Ok(status == wanted)
            };
            Report::from_settled(ops::settle(
                &result,
                observed,
                &Expectation {
                    done: format!("Kill switch {word} for {name}"),
                    state: format!("the kill switch is {word}"),
                    not_state: format!("the kill switch is {}", status.label()),
                    failed: "Kill switch failed".into(),
                },
                |e| e.to_string(),
            ))
        },
    );
}

/// Reinstall a lost kill switch, then re-check and report the result.
pub fn repair_killswitch(app: &mut App, name: String, automatic: bool) {
    let message = if automatic {
        format!("Kill switch for {name} is not in force - repairing automatically…")
    } else {
        format!("Repairing kill switch for {name}…")
    };
    start_action(app, OpKind::KillSwitchRepair, message, move || {
        let result = backend::repair_killswitch(&name);
        let status = backend::killswitch_status(&name);
        logging::event(
            "killswitch.repair",
            &[
                ("name", &name),
                ("automatic", if automatic { "yes" } else { "no" }),
                ("status_after", status.label()),
            ],
        );
        repair_report(&name, &result, &status)
    });
}

pub fn repair_report(name: &str, result: &Result<(), HelperError>, status: &KillSwitch) -> Report {
    match (result, status) {
        (_, KillSwitch::Enabled) => {
            Report::ok(format!("Kill switch for {name} repaired and verified"))
        }
        (Ok(()), status) => Report::failed(format!(
            "Kill switch for {name} is still not in force after the repair.\n\nStatus now: {}\n\nPress K to retry or to turn it off.",
            status.label()
        )),
        (Err(error), status) => Report::failed(format!(
            "Kill switch repair for {name} failed:\n\n{error}\n\nStatus now: {}\n\nPress K to retry or to turn it off.",
            status.label()
        )),
    }
}

pub fn export(app: &mut App) {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let dest = Path::new(&home).join("wireguard-tunnels.zip");
    start_action(
        app,
        OpKind::Export,
        "Exporting tunnels…".into(),
        move || {
            Report::from_result(
                backend::export_zip(&dest)
                    .map(|count| format!("Exported {count} tunnel(s) to {}", dest.display()))
                    .map_err(|error| format!("Export failed: {error}")),
            )
        },
    );
}

pub fn generate_show(app: &mut App) {
    start_presentation(app, "Generating keys…".into(), || {
        let (private, public) = backend::generate_keypair()?;
        let preshared = backend::generate_psk()?;
        Ok(Mode::Message(format!(
            "Private key:\n{private}\n\nPublic key:\n{public}\n\nPreshared key:\n{preshared}\n\nKeep private and preshared keys secret. Any key closes."
        )))
    });
}

pub fn show_running(app: &mut App) {
    let Some(detail) = &app.detail else { return };
    if !detail.active {
        app.flash("Tunnel is not active");
        return;
    }
    let name = detail.name.clone();
    start_presentation(
        app,
        format!("Reading running config of {name}…"),
        move || {
            let config = backend::running_config(&name)
                .map_err(|error| format!("showconf failed: {error}"))?;
            Ok(Mode::Message(format!(
                "Running config - {name}\n\n{}",
                config.trim_end()
            )))
        },
    );
}

pub fn persist_live(app: &mut App) {
    let Some(detail) = &app.detail else { return };
    if !detail.active {
        app.flash("Tunnel is not active");
        return;
    }
    let name = detail.name.clone();
    start_action(
        app,
        OpKind::PersistLive,
        format!("Saving live state of {name}…"),
        move || {
            Report::from_result(
                backend::persist_live(&name)
                    .map(|()| format!("Saved live state of {name} to its .conf"))
                    .map_err(|error| format!("Save-live failed: {error}")),
            )
        },
    );
}

pub fn show_backups(app: &mut App) {
    start_presentation(app, "Reading backups…".into(), || {
        Ok(Mode::Backup {
            items: backend::list_backups(),
            sel: 0,
        })
    });
}

pub fn create_backup(app: &mut App) {
    start_action(app, OpKind::Backup, "Creating backup…".into(), || {
        Report::from_result(
            backend::create_backup()
                .map(|info| {
                    format!(
                        "Backed up {} tunnel(s) -> {} (B to reopen backups)",
                        info.count, info.name
                    )
                })
                .map_err(|error| format!("Backup failed: {error}")),
        )
    });
}

pub fn restore_backup(app: &mut App, path: PathBuf) {
    start_action(
        app,
        OpKind::Restore,
        "Restoring backup…".into(),
        move || match backend::restore_backup(&path) {
            Ok(summary) => Report::ok(summary.to_string()),
            Err(error) => Report::failed(error.to_string()),
        },
    );
}

pub fn delete_backup(app: &mut App, path: PathBuf) {
    start_action(
        app,
        OpKind::Backup,
        "Deleting backup…".into(),
        move || {
            Report::from_result(
                backend::delete_backup(&path)
                    .map(|()| "Backup deleted (B to reopen backups)".to_string())
                    .map_err(|error| format!("Delete failed: {error}")),
            )
        },
    );
}

pub fn export_backup(app: &mut App, path: PathBuf, name: String) {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let dest = Path::new(&home).join(&name);
    start_action(
        app,
        OpKind::Export,
        "Exporting backup…".into(),
        move || {
            Report::from_result(
                backend::export_backup_to(&path, &dest)
                    .map(|()| format!("Exported to {}", dest.display()))
                    .map_err(|error| format!("Export failed: {error}")),
            )
        },
    );
}

/// Build the import browser, starting in the current directory (or $HOME).
pub fn open_import_browser(app: &mut App) {
    let dir = std::env::current_dir()
        .ok()
        .filter(|p| p.is_dir())
        .or_else(|| std::env::var("HOME").ok().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("/"));
    let entries = read_dir_entries(&dir);
    app.mode = Mode::ImportBrowse {
        dir,
        entries,
        sel: 0,
        marked: std::collections::HashSet::new(),
    };
}

/// List a directory for the import browser: a `..` entry, then sub-directories,
/// then importable files (`.conf`/`.png`/`.jpg`/`.jpeg`). Hidden names are
/// skipped except the parent shortcut.
pub fn read_dir_entries(dir: &Path) -> Vec<FileEntry> {
    let mut out = Vec::new();
    if let Some(parent) = dir.parent() {
        out.push(FileEntry {
            name: "..".to_string(),
            path: parent.to_path_buf(),
            is_dir: true,
        });
    }
    let (mut dirs, mut files) = (Vec::new(), Vec::new());
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let path = e.path();
            if path.is_dir() {
                dirs.push(FileEntry {
                    name,
                    path,
                    is_dir: true,
                });
            } else {
                let ext = path
                    .extension()
                    .and_then(|x| x.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if matches!(ext.as_str(), "conf" | "png" | "jpg" | "jpeg") {
                    files.push(FileEntry {
                        name,
                        path,
                        is_dir: false,
                    });
                }
            }
        }
    }
    let by_name = |a: &FileEntry, b: &FileEntry| a.name.to_lowercase().cmp(&b.name.to_lowercase());
    dirs.sort_by(by_name);
    files.sort_by(by_name);
    out.extend(dirs);
    out.extend(files);
    out
}

/// User-selected configs follow the helper's 1 MiB limit and go through the
/// shared safe-open routine (a named pipe cannot hold the import open).
pub fn read_config_file(path: &Path) -> Result<String, String> {
    safe_file::read_text(path, Rules::CONFIG).map_err(String::from)
}

pub fn import_files(app: &mut App, paths: &[PathBuf]) {
    let paths = paths.to_vec();
    start_selected_action(
        app,
        OpKind::Import,
        format!("Importing {} file(s)…", paths.len()),
        move || {
            let (mut count, mut skipped) = (0, 0);
            let mut first_error = None;
            let mut last = None;
            for path in paths {
                match import_one_file(&path) {
                    Ok(name) => {
                        count += 1;
                        last = Some(name);
                    }
                    Err(error) => {
                        skipped += 1;
                        first_error.get_or_insert(error);
                    }
                }
            }
            match last {
                None => Err(format!(
                    "Nothing imported ({skipped} skipped): {}",
                    first_error.unwrap_or_default()
                )),
                Some(last) => Ok((
                    format!("Imported {count} tunnel(s), {skipped} skipped"),
                    last,
                )),
            }
        },
    );
}

fn import_one_file(path: &Path) -> Result<String, String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let content = if matches!(ext.as_str(), "png" | "jpg" | "jpeg") {
        backend::decode_qr(path)?
    } else {
        read_config_file(path)?
    };
    backend::validate_config(&content)?;
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("imported");
    let name = backend::unique_name(stem)?;
    backend::save_config(&name, &content)?;
    Ok(name)
}

pub fn show_qr(app: &mut App) {
    let Some(name) = app.selected_name() else {
        return;
    };
    let theme = app.theme;
    start_presentation(app, format!("Preparing QR for {name}…"), move || {
        let config = backend::read_config(&name)?;
        Ok(Mode::Qr(crate::ui::popups::qr_lines(&config, &theme)?))
    });
}

pub fn request_editor(app: &mut App, add_peer: bool) {
    let Some(name) = app.selected_name() else {
        return;
    };
    if let Err(ops::Busy(running)) = app.op.begin(OpKind::LoadEditor, Instant::now()) {
        app.flash(format!(
            "Another operation is still running ({})",
            running.label()
        ));
        return;
    }
    app.flash(format!("Reading {name} for editor…"));
    app.status_at = None;
    let out = app.update_out.clone();
    std::thread::spawn(move || {
        let result = backend::read_config(&name)
            .map_err(String::from)
            .map(|original| {
                let initial = if add_peer {
                    format!("{}\n\n[Peer]\n# PublicKey = \n# AllowedIPs = \n# Endpoint = \n# PersistentKeepalive = 25\n", original.trim_end())
                } else {
                    original.clone()
                };
                EditorRequest {
                    name,
                    original,
                    initial,
                    new_kind: None,
                    keep_on_cancel: false,
                }
            });
        out.lock()
            .unwrap()
            .push_back(UpdateMsg::EditorLoaded(result));
    });
}

/// Create a brand-new tunnel: a generated template, opened in $EDITOR.
pub fn create_tunnel(app: &mut App, name: &str, kind: create::TunnelTemplateKind) {
    if let Err(ops::Busy(running)) = app.op.begin(OpKind::LoadEditor, Instant::now()) {
        app.flash(format!(
            "Another operation is still running ({})",
            running.label()
        ));
        return;
    }
    app.flash("Generating tunnel keys…");
    app.status_at = None;
    let name = name.to_string();
    let out = app.update_out.clone();
    std::thread::spawn(move || {
        let result = backend::generate_keypair().map(|(private, _)| EditorRequest {
            name,
            original: String::new(),
            initial: create::generate_template(kind, &private),
            new_kind: Some(kind),
            keep_on_cancel: false,
        });
        out.lock()
            .unwrap()
            .push_back(UpdateMsg::EditorLoaded(result));
    });
}

/// What to do with the text the editor returned. Pure, so the draft-keeping
/// rules are unit-tested.
#[derive(Debug, PartialEq, Eq)]
pub enum EditedDraft {
    Unchanged,
    Invalid(String),
    Save(Draft),
}

pub fn check_edited(draft: Draft) -> (EditedDraft, Option<Draft>) {
    if draft.text == draft.original {
        return (EditedDraft::Unchanged, None);
    }
    match backend::validate_config(&draft.text) {
        Ok(()) => (EditedDraft::Save(draft), None),
        Err(error) => (EditedDraft::Invalid(error), Some(draft)),
    }
}

/// Validate and save an edited draft. A draft that fails to validate or save
/// is never dropped: it comes back in the recovery popup.
pub fn submit_draft(app: &mut App, draft: Draft) {
    match check_edited(draft) {
        (EditedDraft::Unchanged, _) => app.flash("No changes"),
        (EditedDraft::Invalid(error), Some(draft)) => {
            app.mode = Mode::DraftRecovery {
                draft,
                problem: DraftProblem::Invalid,
                error: format!("Not saved - invalid config:\n{error}"),
            };
        }
        (EditedDraft::Invalid(_), None) => {}
        (EditedDraft::Save(draft), _) => save_draft(app, draft),
    }
}

fn draft_failed(draft: Draft, outcome: Outcome, error: String) -> Report {
    Report::with_mode(
        outcome,
        Mode::DraftRecovery {
            draft,
            problem: DraftProblem::SaveFailed,
            error,
        },
    )
}

/// After a failed or timed-out save, decide from the file actually on disk.
/// `Ok(None)` means the draft is saved after all.
pub fn reconcile_save(
    draft: &Draft,
    error: &HelperError,
    reread: Result<String, HelperError>,
) -> Option<(Outcome, String)> {
    if !error.is_timeout() {
        return Some((Outcome::Failed, format!("Not saved:\n{error}")));
    }
    match reread {
        Ok(current) if current == draft.text => None,
        Ok(_) => Some((
            Outcome::Failed,
            "Not saved: the helper timed out and the saved file does not contain your changes."
                .into(),
        )),
        Err(check) => Some((
            Outcome::Unknown,
            format!(
                "The helper timed out and the saved file could not be re-checked ({check}). Your draft is kept; Retry is safe."
            ),
        )),
    }
}

pub fn save_draft(app: &mut App, draft: Draft) {
    let name = draft.name.clone();
    if let Some(kind) = draft.new_kind {
        let generation = app.selection_generation;
        start_action(
            app,
            OpKind::Create,
            format!("Creating {name}…"),
            move || {
                match backend::tunnel_exists(&name) {
                    Ok(false) => {}
                    Ok(true) => {
                        return draft_failed(
                            draft,
                            Outcome::Failed,
                            format!(
                                "A tunnel named '{name}' already exists. Re-edit to keep your text, then rename it later."
                            ),
                        );
                    }
                    Err(error) => {
                        return draft_failed(
                            draft,
                            Outcome::Failed,
                            format!("Not saved:\n{error}"),
                        );
                    }
                }
                if let Err(error) = backend::save_config(&name, &draft.text)
                    && let Some((outcome, message)) =
                        reconcile_save(&draft, &error, backend::read_config(&name))
                {
                    return draft_failed(draft, outcome, message);
                }
                Report {
                    select: Some((name.clone(), generation)),
                    ..Report::ok(format!("Created {name} from {}", kind.label()))
                }
            },
        );
        return;
    }
    start_action(app, OpKind::Save, format!("Saving {name}…"), move || {
        let active = match is_up(&name) {
            Ok(active) => active,
            Err(error) => {
                return draft_failed(draft, Outcome::Failed, format!("Not saved:\n{error}"));
            }
        };
        if let Err(error) = backend::save_config(&name, &draft.text)
            && let Some((outcome, message)) =
                reconcile_save(&draft, &error, backend::read_config(&name))
        {
            return draft_failed(draft, outcome, message);
        }
        if active {
            sync_report(&name, backend::sync_running(&name))
        } else {
            Report::ok(format!("Saved {name}"))
        }
    });
}

/// Report for applying a saved config to the running interface. A failure
/// never claims success: the saved file and the interface now differ.
pub fn sync_report(name: &str, result: Result<(), HelperError>) -> Report {
    match result {
        Ok(()) => Report::ok(format!(
            "Saved {name} (WireGuard settings applied live; reconnect for Address/DNS/MTU/routes)"
        )),
        Err(error) => Report::with_mode(
            Outcome::Failed,
            Mode::SyncFailed {
                name: name.to_string(),
                needs_reconnect: error.mentions("reconnect"),
                error: if error.is_timeout() {
                    "The helper timed out while applying the change; the running interface may or may not have it.".into()
                } else {
                    error.to_string()
                },
            },
        ),
    }
}

pub fn retry_sync(app: &mut App, name: String) {
    start_action(
        app,
        OpKind::Sync,
        format!("Applying {name} live…"),
        move || sync_report(&name, backend::sync_running(&name)),
    );
}

/// Verify a rename or delete against the saved tunnel list.
pub fn names_settled(
    result: &Result<(), HelperError>,
    names: Result<Vec<String>, HelperError>,
    present: &[&str],
    absent: &[&str],
    expect: &Expectation,
) -> Settled {
    let observed = names.map(|names| {
        present.iter().all(|n| names.iter().any(|x| x == n))
            && absent.iter().all(|n| !names.iter().any(|x| x == n))
    });
    ops::settle(result, observed, expect, |e| e.to_string())
}

pub fn rename_tunnel(app: &mut App, orig: &str, new: &str) {
    if let Err(error) = backend::validate_tunnel_name(new) {
        app.mode = Mode::Message(format!("Invalid tunnel name:\n{error}"));
        return;
    }
    if new == orig {
        return;
    }
    let (orig, new) = (orig.to_string(), new.to_string());
    let generation = app.selection_generation;
    start_action(
        app,
        OpKind::Rename,
        format!("Renaming {orig}…"),
        move || {
            match backend::tunnel_exists(&new) {
                Ok(false) => {}
                Ok(true) => {
                    return Report::failed(format!("A tunnel named '{new}' already exists."));
                }
                Err(error) => return Report::failed(format!("Rename failed: {error}")),
            }
            let config = match backend::read_config(&orig) {
                Ok(config) => config,
                Err(error) => return Report::failed(format!("Rename failed: {error}")),
            };
            let result = backend::rename_config(&orig, &new, &config);
            let settled = names_settled(
                &result,
                backend::saved_tunnel_names(),
                &[&new],
                &[&orig],
                &Expectation {
                    done: format!("Renamed {orig} -> {new}"),
                    state: format!("{new} exists and {orig} is gone"),
                    not_state: format!("the rename did not take effect ({orig} -> {new})"),
                    failed: format!("Couldn't rename {orig}"),
                },
            );
            let renamed = settled.outcome == Outcome::Succeeded;
            Report {
                select: renamed.then(|| (new.clone(), generation)),
                ..Report::from_settled(settled)
            }
        },
    );
}

pub fn delete_tunnel(app: &mut App, name: String) {
    start_action(
        app,
        OpKind::Delete,
        format!("Deleting {name}…"),
        move || {
            let result = backend::delete(&name);
            Report::from_settled(names_settled(
                &result,
                backend::saved_tunnel_names(),
                &[],
                &[&name],
                &Expectation {
                    done: format!("Deleted {name}"),
                    state: format!("{name} is gone"),
                    not_state: format!("{name} still exists"),
                    failed: "Delete failed".into(),
                },
            ))
        },
    );
}

/// Write the full (unfiltered) log to a file in $HOME.
pub fn save_log(app: &mut App) {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let dest = Path::new(&home).join("wireguard-tui-log.txt");
    match backend::save_text_to(&dest, &app.log) {
        Ok(()) => app.flash(format!("Log saved to {}", dest.display())),
        Err(e) => app.flash(format!("Save failed: {e}")),
    }
}

#[cfg(test)]
mod tests;
