//! Running `$VISUAL`/`$EDITOR` on a private temporary copy of a config, with
//! the TUI suspended, and handing the result to the draft-saving logic.

use std::io::{self, Write as _};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::process::Command;

use crate::actions;
use crate::safe_file::{self, Rules};
use crate::state::{App, Draft, DraftProblem, EditorRequest, Mode};

/// A private editor workspace with no predictable shared-directory or symlink
/// reuse. Cleanup includes editor-created swap/backup files and runs on errors.
pub struct EditorDraft {
    pub directory: PathBuf,
    pub path: PathBuf,
}

impl EditorDraft {
    pub fn new(initial: &str) -> io::Result<Self> {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let template = std::env::temp_dir().join("wg-tui-editor-XXXXXX");
        let mut name = std::ffi::CString::new(template.as_os_str().as_bytes())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?
            .into_bytes_with_nul();
        if unsafe { libc::mkdtemp(name.as_mut_ptr().cast()) }.is_null() {
            return Err(io::Error::last_os_error());
        }
        name.pop();
        let directory = PathBuf::from(std::ffi::OsString::from_vec(name));
        let draft = Self {
            path: directory.join("tunnel.conf"),
            directory,
        };
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&draft.path)?;
        file.write_all(initial.as_bytes())?;
        Ok(draft)
    }

    /// Read the edited file back through the shared safe-open routine. It
    /// lives in our private directory, so a symlink there is refused.
    pub fn read(&self) -> Result<String, String> {
        safe_file::read_text(&self.path, Rules::CONFIG.private()).map_err(String::from)
    }
}

impl Drop for EditorDraft {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

pub fn editor_argv(editor: &str) -> io::Result<Vec<String>> {
    let parts = shell_words::split(editor).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Invalid VISUAL/EDITOR quoting: {error}"),
        )
    })?;
    if parts.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "VISUAL/EDITOR is empty",
        ));
    }
    Ok(parts)
}

/// Resume the existing terminal without installing another panic hook. Calling
/// `ratatui::init()` for each editor/update would repeatedly wrap the old hook.
pub fn resume_terminal(terminal: &mut ratatui::DefaultTerminal) -> io::Result<()> {
    crossterm::terminal::enable_raw_mode()?;
    if let Err(error) = crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::EnterAlternateScreen
    ) {
        let _ = crossterm::terminal::disable_raw_mode();
        return Err(error);
    }
    terminal.autoresize()?;
    terminal.clear()
}

/// Suspend the TUI, run `$EDITOR` on a temp copy of `initial`, return the edited
/// text (None if the editor exited unsuccessfully). The temp file is mode 0600
/// and removed afterwards. The outer error means the terminal could not be
/// restored; the inner one is an editor problem shown to the user.
fn run_editor(
    terminal: &mut ratatui::DefaultTerminal,
    initial: &str,
) -> io::Result<Result<Option<String>, String>> {
    let draft = match EditorDraft::new(initial) {
        Ok(draft) => draft,
        Err(error) => return Ok(Err(error.to_string())),
    };
    let editor = std::env::var("VISUAL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            std::env::var("EDITOR")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .unwrap_or_else(|| "nano".to_string());
    // Parse before suspending the terminal; malformed quoting is a useful
    // input error, not a failed shell launch. No shell expansion is performed.
    let parts = match editor_argv(&editor) {
        Ok(parts) => parts,
        Err(error) => return Ok(Err(error.to_string())),
    };
    let (prog, args) = parts.split_first().unwrap();

    ratatui::restore();
    let status = Command::new(prog).args(args).arg(&draft.path).status();
    resume_terminal(terminal)?;

    Ok(match status {
        Ok(status) if status.success() => draft.read().map(Some),
        _ => Ok(None),
    })
}

/// Open the editor for a loaded request and submit the result. A request that
/// carries an unsaved draft is never dropped when the editor is cancelled or
/// fails: it returns to the recovery popup.
pub fn edit_loaded_tunnel(
    app: &mut App,
    terminal: &mut ratatui::DefaultTerminal,
    request: EditorRequest,
) -> io::Result<()> {
    let outcome = run_editor(terminal, &request.initial)?;
    apply_editor_outcome(app, request, outcome);
    Ok(())
}

pub fn apply_editor_outcome(
    app: &mut App,
    request: EditorRequest,
    outcome: Result<Option<String>, String>,
) {
    let EditorRequest {
        name,
        original,
        initial,
        new_kind,
        keep_on_cancel,
    } = request;
    let keep = |error: String| Mode::DraftRecovery {
        draft: Draft {
            name: name.clone(),
            original: original.clone(),
            text: initial.clone(),
            new_kind,
        },
        problem: DraftProblem::Invalid,
        error,
    };
    match outcome {
        Ok(Some(text)) => actions::submit_draft(
            app,
            Draft {
                name,
                original,
                text,
                new_kind,
            },
        ),
        Ok(None) if keep_on_cancel => {
            app.mode = keep("The editor exited without saving; your earlier draft is kept.".into())
        }
        Ok(None) => app.flash("Edit cancelled"),
        Err(error) if keep_on_cancel => {
            app.mode = keep(format!("Editor failed: {error}\n\nYour draft is kept."))
        }
        Err(error) => app.mode = Mode::Message(format!("Editor failed: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn editor_arguments_preserve_quoted_paths_and_do_not_expand_shell_code() {
        assert_eq!(
            editor_argv("'/tmp/editor with spaces' --wait --option 'two words'").unwrap(),
            vec!["/tmp/editor with spaces", "--wait", "--option", "two words"]
        );
        assert_eq!(
            editor_argv("editor '$HOME' '$(touch nope)' ';'").unwrap(),
            vec!["editor", "$HOME", "$(touch nope)", ";"]
        );
        assert!(editor_argv("editor 'unterminated").is_err());
        assert!(editor_argv("  ").is_err());
    }

    #[test]
    fn editor_drafts_are_exclusive_private_and_clean_backups_on_drop() {
        let first = EditorDraft::new("private key").unwrap();
        let second = EditorDraft::new("different key").unwrap();
        assert_ne!(first.directory, second.directory);
        assert_eq!(
            std::fs::metadata(&first.directory)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(&first.path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(first.read().unwrap(), "private key");
        let directory = first.directory.clone();
        std::fs::write(directory.join("editor-backup"), "private key").unwrap();
        drop(first);
        assert!(!directory.exists());
        assert!(second.path.exists());
    }

    #[test]
    fn edited_draft_replaced_by_a_fifo_is_refused_without_hanging() {
        use std::os::unix::ffi::OsStrExt;
        let draft = EditorDraft::new("text").unwrap();
        std::fs::remove_file(&draft.path).unwrap();
        let c = std::ffi::CString::new(draft.path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        assert!(draft.read().unwrap_err().contains("regular file"));
    }

    fn request(keep_on_cancel: bool) -> EditorRequest {
        EditorRequest {
            name: "wg0".into(),
            original: "saved".into(),
            initial: "my unsaved draft".into(),
            new_kind: None,
            keep_on_cancel,
        }
    }

    #[test]
    fn cancelling_or_failing_the_editor_never_loses_a_draft() {
        let mut app = App::default();
        apply_editor_outcome(&mut app, request(true), Ok(None));
        assert!(
            matches!(&app.mode, Mode::DraftRecovery { draft, .. } if draft.text == "my unsaved draft")
        );
        let mut app = App::default();
        apply_editor_outcome(&mut app, request(true), Err("no such editor".into()));
        assert!(matches!(&app.mode, Mode::DraftRecovery { draft, error, .. }
            if draft.text == "my unsaved draft" && error.contains("no such editor")));
        let mut app = App::default();
        apply_editor_outcome(&mut app, request(false), Ok(None));
        assert!(matches!(app.mode, Mode::Normal));
        assert_eq!(app.status, "Edit cancelled");
        let mut app = App::default();
        apply_editor_outcome(&mut app, request(false), Ok(Some("invalid text".into())));
        assert!(
            matches!(&app.mode, Mode::DraftRecovery { draft, problem: DraftProblem::Invalid, .. }
            if draft.text == "invalid text" && draft.original == "saved")
        );
    }
}
