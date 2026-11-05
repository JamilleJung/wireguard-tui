//! Keyboard input: modal popups consume keys first; in the normal view a key
//! maps to a [`Command`] through one table ([`KEYMAP`]) that also feeds the
//! help popup, so every action is reachable by keyboard and documented.

use crossterm::event::{KeyCode, KeyModifiers};

use crate::actions;
use crate::backend;
use crate::clipboard;
use crate::create::TunnelTemplateKind;
use crate::state::{
    App, DraftProblem, Mode, TerminalTask, changelog_for_display, filtered_log_text_lines,
};
use crate::ui;

use crate::keymap::{Command, command_for};

use Command as C;
use KeyCode as K;

fn scroll_by(scroll: &mut u16, code: KeyCode, max: u16) -> bool {
    match code {
        K::Down | K::Char('j') => *scroll = scroll.saturating_add(1).min(max),
        K::Up | K::Char('k') => *scroll = scroll.saturating_sub(1),
        K::PageDown | K::Char(' ') => *scroll = scroll.saturating_add(15).min(max),
        K::PageUp => *scroll = scroll.saturating_sub(15),
        K::Home | K::Char('g') => *scroll = 0,
        K::End | K::Char('G') => *scroll = max,
        _ => return false,
    }
    true
}

pub fn handle_key(app: &mut App, code: KeyCode, mods: KeyModifiers) {
    if mods.contains(KeyModifiers::CONTROL) && code == K::Char('c') {
        app.quit = true;
        return;
    }
    if !matches!(app.mode, Mode::Normal) {
        handle_modal(app, code);
        return;
    }
    let Some(command) = command_for(app.tab, app.log_select_anchor.is_some(), code) else {
        return;
    };
    if app.op.is_busy() && command.starts_operation() {
        let running = app
            .op
            .running()
            .map_or("operation", |(kind, _)| kind.label());
        app.flash(format!("Another operation is still running ({running})"));
        return;
    }
    run_command(app, command);
}

fn handle_modal(app: &mut App, code: KeyCode) {
    let viewport = app.viewport;
    match &mut app.mode {
        Mode::Help { scroll } => {
            let max = ui::popups::help_max_scroll(viewport);
            if !scroll_by(scroll, code, max) {
                app.mode = Mode::Normal;
            }
        }
        Mode::About | Mode::Message(_) | Mode::Qr(_) => app.mode = Mode::Normal,
        Mode::Doc { body, scroll, .. } => {
            // The paragraph wraps at the current viewport width. Counting raw
            // source lines makes the tail unreachable on narrow terminals.
            let max = ui::popups::doc_max_scroll(body, viewport);
            if matches!(code, K::Esc | K::Char('q')) {
                app.mode = Mode::Normal;
            } else {
                scroll_by(scroll, code, max);
            }
        }
        Mode::ConfirmDelete(name) => {
            let name = name.clone();
            app.mode = Mode::Normal;
            if matches!(code, K::Char('y' | 'Y')) {
                actions::delete_tunnel(app, name);
            } else {
                app.flash("Delete cancelled");
            }
        }
        Mode::InputNew(buf) => match code {
            K::Esc => app.mode = Mode::Normal,
            K::Backspace => {
                buf.pop();
            }
            K::Enter => {
                let name = buf.trim().to_string();
                app.mode = match backend::validate_tunnel_name(&name) {
                    Err(e) => Mode::Message(format!("Invalid tunnel name:\n{e}")),
                    Ok(()) => Mode::NewPreset { name },
                };
            }
            K::Char(c) if buf.len() < 15 => buf.push(c),
            _ => {}
        },
        Mode::NewPreset { name } => {
            let name = name.clone();
            let kind = match code {
                K::Esc => {
                    app.mode = Mode::Normal;
                    app.flash("New tunnel cancelled");
                    return;
                }
                K::Enter | K::Char('f' | 'F') => TunnelTemplateKind::ClientFullTunnel,
                K::Char('i' | 'I') => TunnelTemplateKind::InterfaceOnly,
                K::Char('s' | 'S') => TunnelTemplateKind::ClientSplitTunnel,
                _ => return,
            };
            app.mode = Mode::Normal;
            actions::create_tunnel(app, &name, kind);
        }
        Mode::InputRename { orig, buf } => match code {
            K::Esc => app.mode = Mode::Normal,
            K::Backspace => {
                buf.pop();
            }
            K::Enter => {
                let (orig, new) = (orig.clone(), buf.trim().to_string());
                app.mode = Mode::Normal;
                actions::rename_tunnel(app, &orig, &new);
            }
            K::Char(c) if buf.len() < 15 => buf.push(c),
            _ => {}
        },
        Mode::ImportBrowse { .. } => handle_import_browser(app, code),
        Mode::Backup { .. } => handle_backup(app, code),
        Mode::ConfirmRestore(path) => {
            let path = path.clone();
            app.mode = Mode::Normal;
            if matches!(code, K::Char('y' | 'Y')) {
                actions::restore_backup(app, path);
            } else {
                app.flash("Restore cancelled");
            }
        }
        Mode::ConfirmBackupDelete(path) => {
            let path = path.clone();
            app.mode = Mode::Normal;
            if matches!(code, K::Char('y' | 'Y')) {
                actions::delete_backup(app, path);
            } else {
                app.flash("Delete cancelled");
            }
        }
        Mode::LogFilter(buf) => match code {
            K::Esc => app.mode = Mode::Normal,
            K::Enter => {
                app.log_filter = buf.trim().to_string();
                app.log_cursor = 0;
                app.log_select_anchor = None;
                app.log_cleared = false;
                app.mode = Mode::Normal;
            }
            K::Backspace => {
                buf.pop();
            }
            K::Char(c) => buf.push(c),
            _ => {}
        },
        Mode::DraftRecovery { .. } => handle_draft_recovery(app, code),
        Mode::ConfirmDiscard { .. } => {
            let Mode::ConfirmDiscard {
                draft,
                problem,
                error,
            } = std::mem::take(&mut app.mode)
            else {
                return;
            };
            if matches!(code, K::Char('y' | 'Y')) {
                app.flash(format!("Draft for {} discarded", draft.name));
            } else {
                app.mode = Mode::DraftRecovery {
                    draft,
                    problem,
                    error,
                };
            }
        }
        Mode::SyncFailed { name, .. } => {
            let name = name.clone();
            match code {
                K::Char('s' | 'S') | K::Enter => {
                    if busy_refusal(app) {
                        return;
                    }
                    app.mode = Mode::Normal;
                    actions::retry_sync(app, name);
                }
                K::Char('c' | 'C') => {
                    if busy_refusal(app) {
                        return;
                    }
                    app.mode = Mode::Normal;
                    actions::reconnect(app, name);
                }
                K::Char('k' | 'K') | K::Esc => {
                    app.mode = Mode::Normal;
                    app.flash(format!(
                        "Kept as is: {name}'s running interface differs from its saved config until you reconnect"
                    ));
                }
                _ => {}
            }
        }
        Mode::KillSwitchProblem { name, .. } => {
            let name = name.clone();
            match code {
                K::Char('r' | 'R') | K::Enter => {
                    if busy_refusal(app) {
                        return;
                    }
                    app.mode = Mode::Normal;
                    actions::repair_killswitch(app, name, false);
                }
                K::Char('o' | 'O') => {
                    if busy_refusal(app) {
                        return;
                    }
                    app.mode = Mode::Normal;
                    actions::set_killswitch(app, name, false);
                }
                K::Esc => app.mode = Mode::Normal,
                _ => {}
            }
        }
        Mode::Normal => {}
    }
}

/// Refuse a popup action while an operation runs, keeping the popup open.
fn busy_refusal(app: &mut App) -> bool {
    if app.op.is_busy() {
        app.flash("Another operation is still running - try again when it finishes");
        true
    } else {
        false
    }
}

fn handle_draft_recovery(app: &mut App, code: KeyCode) {
    let Mode::DraftRecovery { problem, .. } = &app.mode else {
        return;
    };
    let problem = *problem;
    match code {
        K::Char('e' | 'E') | K::Enter => {
            let Mode::DraftRecovery { draft, .. } = std::mem::take(&mut app.mode) else {
                return;
            };
            app.ready_editor = Some(draft.into_request());
        }
        K::Char('r' | 'R') if problem == DraftProblem::SaveFailed => {
            if busy_refusal(app) {
                return;
            }
            let Mode::DraftRecovery { draft, .. } = std::mem::take(&mut app.mode) else {
                return;
            };
            actions::save_draft(app, draft);
        }
        K::Char('d' | 'D') => {
            if let Mode::DraftRecovery {
                draft,
                problem,
                error,
            } = std::mem::take(&mut app.mode)
            {
                app.mode = Mode::ConfirmDiscard {
                    draft,
                    problem,
                    error,
                };
            }
        }
        K::Esc => {
            if let Mode::DraftRecovery {
                draft,
                problem,
                error,
            } = std::mem::take(&mut app.mode)
            {
                app.flash(format!(
                    "Draft for {} kept - press E to reopen it",
                    draft.name
                ));
                app.parked_draft = Some((draft, problem, error));
            }
        }
        _ => {}
    }
}

fn handle_import_browser(app: &mut App, code: KeyCode) {
    let Mode::ImportBrowse {
        dir,
        entries,
        sel,
        marked,
    } = &mut app.mode
    else {
        return;
    };
    match code {
        K::Esc => app.mode = Mode::Normal,
        K::Up | K::Char('k') => *sel = sel.saturating_sub(1),
        K::Down | K::Char('j') => {
            if *sel + 1 < entries.len() {
                *sel += 1;
            }
        }
        K::PageUp => *sel = sel.saturating_sub(10),
        K::PageDown => *sel = (*sel + 10).min(entries.len().saturating_sub(1)),
        K::Home => *sel = 0,
        K::End => *sel = entries.len().saturating_sub(1),
        // Space toggles a file's mark for bulk import (dirs can't be marked).
        K::Char(' ') => {
            if let Some(e) = entries.get(*sel)
                && !e.is_dir
                && !marked.remove(&e.path)
            {
                marked.insert(e.path.clone());
            }
        }
        K::Backspace | K::Left | K::Char('h') => {
            if let Some(parent) = dir.parent().map(|p| p.to_path_buf()) {
                let entries = actions::read_dir_entries(&parent);
                let marked = std::mem::take(marked); // keep marks across dirs
                app.mode = Mode::ImportBrowse {
                    dir: parent,
                    entries,
                    sel: 0,
                    marked,
                };
            }
        }
        K::Enter | K::Right | K::Char('l') => {
            let chosen = entries.get(*sel).map(|e| (e.is_dir, e.path.clone()));
            if let Some((is_dir, path)) = chosen {
                if is_dir {
                    // Descend, preserving any marks.
                    let entries = actions::read_dir_entries(&path);
                    let marked = std::mem::take(marked);
                    app.mode = Mode::ImportBrowse {
                        dir: path,
                        entries,
                        sel: 0,
                        marked,
                    };
                } else {
                    // Bulk import everything marked (the highlighted file is
                    // included automatically if it was marked).
                    let mut paths: Vec<_> = if marked.is_empty() {
                        vec![path]
                    } else {
                        marked.iter().cloned().collect()
                    };
                    paths.sort();
                    app.mode = Mode::Normal;
                    actions::import_files(app, &paths);
                }
            }
        }
        _ => {}
    }
}

fn handle_backup(app: &mut App, code: KeyCode) {
    let Mode::Backup { items, sel } = &mut app.mode else {
        return;
    };
    // Navigation stays inside the borrow; actions clone first, then act.
    match code {
        K::Up | K::Char('k') => *sel = sel.saturating_sub(1),
        K::Down | K::Char('j') => {
            if *sel + 1 < items.len() {
                *sel += 1;
            }
        }
        K::Home | K::Char('g') => *sel = 0,
        K::End | K::Char('G') => *sel = items.len().saturating_sub(1),
        _ => {
            let selected = items.get(*sel).map(|b| (b.path.clone(), b.name.clone()));
            match (code, selected) {
                (K::Esc | K::Char('q'), _) => app.mode = Mode::Normal,
                (K::Char('n'), _) => {
                    app.mode = Mode::Normal;
                    actions::create_backup(app);
                }
                (K::Enter | K::Char('r'), Some((path, _))) => app.mode = Mode::ConfirmRestore(path),
                (K::Char('d'), Some((path, _))) => app.mode = Mode::ConfirmBackupDelete(path),
                (K::Char('x'), Some((path, name))) => {
                    app.mode = Mode::Normal;
                    actions::export_backup(app, path, name);
                }
                _ => {}
            }
        }
    }
}

fn run_command(app: &mut App, command: Command) {
    match command {
        C::LogCancelSelection => {
            app.log_select_anchor = None;
            app.flash("Selection cancelled");
        }
        C::Quit => app.quit = true,
        C::LogCopy => copy_log_current_or_selection(app),
        C::Copy => copy_detail_field(app),
        C::LogVisual => {
            // Toggle visual selection mode, anchored at the cursor.
            if app.log_select_anchor.take().is_none() {
                app.log_select_anchor = Some(app.log_cursor);
                app.flash("Visual select: move to extend, y to copy, Esc to cancel");
            } else {
                app.flash("Selection cleared");
            }
        }
        C::LogCopyAll => {
            let lines = filtered_log_text_lines(app);
            let n = lines.len();
            clipboard::copy_text(&lines.join("\n"));
            app.flash(format!("Copied {n} log line(s)"));
        }
        C::SwitchTab => {
            if app.tab == 0 {
                app.enter_log_tab();
            } else {
                app.tab = 0;
                app.sync_poll_in();
            }
        }
        C::LogDown => app.log_move_cursor(1),
        C::LogUp => app.log_move_cursor(-1),
        C::MoveDown => app.move_selection(1),
        C::MoveUp => app.move_selection(-1),
        C::LogPageDown => app.log_move_cursor(10),
        C::LogPageUp => app.log_move_cursor(-10),
        C::LogTop => app.log_cursor_top(),
        C::LogBottom => app.log_cursor_bottom(),
        C::LogLeft => app.log_hscroll = app.log_hscroll.saturating_sub(8),
        C::LogRight => app.log_hscroll = app.log_hscroll.saturating_add(8),
        C::NextField => app.move_field(1),
        C::PrevField => app.move_field(-1),
        C::NextPeer => app.jump_peer(true),
        C::PrevPeer => app.jump_peer(false),
        C::DetailPageDown => app.scroll_detail(page(app)),
        C::DetailPageUp => app.scroll_detail(-page(app)),
        C::LogFollow => {
            app.log_follow = !app.log_follow;
            app.log_cleared = false;
            app.flash(if app.log_follow {
                "Log: following newest"
            } else {
                "Log: paused (End to follow again)"
            });
        }
        C::LogOnlyTunnel => {
            app.log_only_selected = !app.log_only_selected;
            app.log_cleared = false;
            app.flash(if app.log_only_selected {
                "Log: showing the selected tunnel only"
            } else {
                "Log: showing all entries"
            });
        }
        C::LogFilter => app.mode = Mode::LogFilter(app.log_filter.clone()),
        C::LogSave => actions::save_log(app),
        C::LogClear => {
            app.log = "(cleared — press r to reload from the journal)".into();
            app.log_scroll = 0;
            app.log_follow = false;
            app.log_cleared = true;
        }
        C::Backup => actions::show_backups(app),
        C::Toggle => actions::toggle_active(app),
        C::Edit => actions::request_editor(app, false),
        C::AddPeer => actions::request_editor(app, true),
        C::ResumeDraft => match app.parked_draft.take() {
            Some((draft, problem, error)) => {
                app.mode = Mode::DraftRecovery {
                    draft,
                    problem,
                    error,
                }
            }
            None => app.flash("No kept draft"),
        },
        C::New => app.mode = Mode::InputNew(String::new()),
        C::Delete => {
            if let Some(name) = app.selected_name() {
                app.mode = Mode::ConfirmDelete(name);
            }
        }
        C::Rename => {
            if let Some(name) = app.selected_name() {
                app.mode = Mode::InputRename {
                    orig: name.clone(),
                    buf: name,
                };
            }
        }
        C::Autostart => actions::toggle_autostart(app),
        C::KillSwitch => actions::toggle_killswitch(app),
        C::Import => actions::open_import_browser(app),
        C::Generate => actions::generate_show(app),
        C::ShowRunning => actions::show_running(app),
        C::PersistLive => actions::persist_live(app),
        C::Export => actions::export(app),
        C::Qr => actions::show_qr(app),
        C::Refresh => {
            app.tick();
            app.flash("Refresh requested");
        }
        C::Update => {
            if app.update.is_some() {
                app.terminal_task = Some(TerminalTask::SelfUpdate);
            } else {
                app.flash("No update available");
            }
        }
        C::Diagnose => actions::diagnose(app),
        C::Help => app.mode = Mode::Help { scroll: 0 },
        C::About => app.mode = Mode::About,
        C::Explained => {
            app.mode = Mode::Doc {
                title: "WireGuard explained".into(),
                body: include_str!("../docs/help.md"),
                scroll: 0,
            };
        }
        C::WhatsNew => {
            app.mode = Mode::Doc {
                title: "What's new".into(),
                body: changelog_for_display(include_str!("../CHANGELOG.md")),
                scroll: 0,
            };
        }
    }
}

fn page(app: &App) -> i32 {
    i32::from(app.viewport.height.saturating_sub(8).max(3))
}

/// Copy the current Log line, or the visual selection if one is active.
fn copy_log_current_or_selection(app: &mut App) {
    let lines = filtered_log_text_lines(app);
    if lines.is_empty() {
        app.flash("Nothing to copy");
        return;
    }
    let (lo, hi) = match app.log_select_anchor {
        Some(a) => (a.min(app.log_cursor), a.max(app.log_cursor)),
        None => (app.log_cursor, app.log_cursor),
    };
    let lo = lo.min(lines.len() - 1);
    let hi = hi.min(lines.len() - 1);
    let chunk = &lines[lo..=hi];
    let n = chunk.len();
    clipboard::copy_text(&chunk.join("\n"));
    app.log_select_anchor = None; // exit visual mode after copying
    app.flash(format!("Copied {n} line(s)"));
}

/// Copy the focused Detail field (Tunnels tab).
fn copy_detail_field(app: &mut App) {
    let fields = app.detail_fields();
    if fields.is_empty() {
        app.flash("No fields to copy");
        return;
    }
    let i = app.detail_cursor.min(fields.len() - 1);
    let (label, value) = &fields[i];
    clipboard::copy_text(&clipboard::normalize_single_field_copy_value(value));
    app.flash(format!("Copied {label}"));
}

#[cfg(test)]
mod tests;
