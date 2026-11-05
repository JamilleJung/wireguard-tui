//! The event loop, the background poller, and the terminal-suspending tasks
//! (external editor, self-update).

use std::io::{self, Write as _};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyEventKind};

use crate::editor::{edit_loaded_tunnel, resume_terminal};
use crate::state::{App, PollIn, STATUS_TTL, Snapshot, TICK, TerminalTask, UpdateMsg};
use crate::theme::Theme;
use crate::{backend, doctor, input, logging, ui, update};

pub fn run(terminal: &mut ratatui::DefaultTerminal, theme: Theme) -> io::Result<()> {
    let mut app = App::new(theme);
    spawn_poller(app.poll_in.clone(), app.poll_out.clone());

    // One-shot startup update check on a detached thread - never blocks the UI,
    // opt-out via WG_NO_UPDATE_CHECK or a persistent file. Offline/curl error is
    // a silent no-op.
    if !update::disabled() {
        let out = app.update_out.clone();
        std::thread::spawn(move || {
            if let Ok(Some(info)) = update::check() {
                out.lock().unwrap().push_back(UpdateMsg::Available(info));
            }
        });
    }

    while !app.quit {
        app.apply_snapshot();
        app.drain_update();
        if let Some(request) = app.ready_editor.take() {
            edit_loaded_tunnel(&mut app, terminal, request)?;
        }
        if let Some(TerminalTask::SelfUpdate) = app.terminal_task.take() {
            self_update(&mut app, terminal)?;
        }
        app.sync_poll_in();
        app.expire_status();
        terminal.draw(|f| ui::ui(f, &mut app))?;

        // Short wake-ups so background snapshots apply promptly and the status
        // clears on time; capped so input stays responsive but the loop idles.
        let mut timeout = Duration::from_millis(250);
        if let Some(at) = app.status_at {
            timeout = timeout.min(STATUS_TTL.saturating_sub(at.elapsed()));
        }
        let timeout = timeout.max(Duration::from_millis(50));

        if event::poll(timeout)?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            input::handle_key(&mut app, key.code, key.modifiers);
        }
    }
    Ok(())
}

const MIN_POLL_GAP: Duration = Duration::from_millis(250);

/// Background thread: does the blocking `wg`/`sudo` calls so the UI never
/// freezes, dropping the result into `poll_out` for the UI loop to apply.
/// It refreshes every `TICK`, or sooner when the UI bumps the generation
/// (after a mutation, a timeout, or a selection change).
fn spawn_poller(poll_in: Arc<Mutex<PollIn>>, poll_out: Arc<Mutex<Option<Snapshot>>>) {
    std::thread::spawn(move || {
        loop {
            let (sel, want_log, generation) = {
                let pi = poll_in.lock().unwrap();
                (pi.selected.clone(), pi.want_log, pi.generation)
            };
            let taken_at = Instant::now();
            let clock_synced = doctor::clock_synced();
            let snap = match backend::try_list_tunnels() {
                Ok(tunnels) => {
                    let detail = sel
                        .as_ref()
                        .filter(|n| tunnels.iter().any(|t| &t.name == *n))
                        .map(|n| (n.clone(), backend::get_detail(n)));
                    let log = want_log.then(backend::get_log);
                    Snapshot {
                        tunnels: Some(tunnels),
                        detail,
                        log,
                        error: None,
                        clock_synced,
                        taken_at,
                    }
                }
                Err(e) => {
                    logging::event("poll.error", &[("error", &e.to_string())]);
                    Snapshot {
                        error: Some(format!("Helper error: {e}")),
                        clock_synced,
                        taken_at,
                        ..Snapshot::default()
                    }
                }
            };
            *poll_out.lock().unwrap() = Some(snap);
            // A refresh request cuts the wait short, but polls stay at least
            // MIN_POLL_GAP apart so holding an arrow key cannot flood the helper.
            let now = Instant::now();
            let (deadline, earliest) = (now + TICK, now + MIN_POLL_GAP);
            while Instant::now() < deadline
                && (Instant::now() < earliest || poll_in.lock().unwrap().generation == generation)
            {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    });
}

/// Download + verify + install the pending update with the TUI suspended.
///
/// Running `install.sh` (which may prompt for a sudo/su password) while ratatui
/// owns the raw-mode alternate screen corrupts the terminal and deadlocks on the
/// hidden prompt, so - exactly like the editor - we drop back to the cooked
/// foreground terminal for the duration and resume the existing terminal afterwards.
fn self_update(app: &mut App, terminal: &mut ratatui::DefaultTerminal) -> io::Result<()> {
    let Some(info) = &app.update else {
        app.flash("No update available");
        return Ok(());
    };
    let latest = info.latest.clone();

    // Suspend the TUI and run the whole flow on the foreground cooked terminal.
    ratatui::restore();

    println!("Downloading and verifying v{latest}...");
    let final_status = match update::download_and_verify() {
        Err(e) => {
            println!("Update failed: {e}");
            format!("Update failed: {e}")
        }
        Ok(tardir) => {
            let latest = tardir.version();
            println!("Installing v{latest} - you may be prompted for your password...");
            match update::apply(&tardir) {
                Ok(()) => {
                    println!(
                        "Updated to v{latest}. Restart wg-tui to use it. Press w to see what's new."
                    );
                    format!(
                        "Updated to v{latest} - restart wg-tui to use it (press w for what's new)"
                    )
                }
                Err(e) => {
                    println!("Update failed: {e}");
                    format!("Update failed: {e}")
                }
            }
        }
    };

    print!("Press Enter to return to wg-tui...");
    io::stdout().flush()?;
    let mut _line = String::new();
    let _ = io::stdin().read_line(&mut _line);

    resume_terminal(terminal)?;
    app.update_status = Some(final_status);
    Ok(())
}
