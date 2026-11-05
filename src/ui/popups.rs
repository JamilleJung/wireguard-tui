//! Popups and full-screen viewers. Every popup is clamped to the frame, and
//! the ones with options keep their key line visible even when the text above
//! has to be clipped.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use ratatui::prelude::*;
use ratatui::widgets::{Clear, List, ListItem, ListState, Paragraph, Wrap};

use crate::backend;
use crate::keymap;
use crate::state::{App, DraftProblem, FileEntry, Mode};
use crate::theme::Theme;

pub fn render_mode(f: &mut Frame, app: &App) {
    let theme = app.theme;
    let t = &theme;
    match &app.mode {
        Mode::Normal => {}
        Mode::Help { scroll } => render_help(f, t, *scroll),
        Mode::About => render_about(f, t),
        Mode::Message(m) => render_message(f, t, "Notice", m),
        Mode::ConfirmDelete(name) => render_message(
            f,
            t,
            "Confirm delete",
            &format!("Delete tunnel '{name}'?\n\n[y] yes    [n] no"),
        ),
        Mode::InputNew(buf) => render_input(f, t, "New tunnel name", buf),
        Mode::NewPreset { name } => render_message(
            f,
            t,
            "Create tunnel",
            &format!(
                "Tunnel '{name}'\n\n\
                 Enter / f   Client full tunnel\n\
                 i           Interface only\n\
                 s           Client split tunnel\n\
                 Esc         Cancel\n\n\
                 A generated private key is inserted. Review the config in $EDITOR before it is saved."
            ),
        ),
        Mode::InputRename { buf, .. } => render_input(f, t, "Rename tunnel", buf),
        Mode::ImportBrowse {
            dir,
            entries,
            sel,
            marked,
        } => render_browse(f, t, dir, entries, *sel, marked),
        Mode::Qr(lines) => render_qr(f, t, lines),
        Mode::Backup { items, sel } => render_backup(f, t, items, *sel),
        Mode::ConfirmRestore(path) => render_message(
            f,
            t,
            "Confirm restore",
            &format!(
                "Restore all tunnels from\n  {}\n\nExisting tunnels with the same name are overwritten.\nIf a write fails, the ones already written are rolled back.\n\n[y] yes    [n] no",
                file_name(path)
            ),
        ),
        Mode::ConfirmBackupDelete(path) => render_message(
            f,
            t,
            "Delete backup",
            &format!(
                "Delete the backup\n  {}\n\n[y] yes    [n] no",
                file_name(path)
            ),
        ),
        Mode::LogFilter(buf) => render_input(f, t, "Filter log (substring)", buf),
        Mode::Doc {
            title,
            body,
            scroll,
        } => render_doc(f, t, title, body, *scroll),
        Mode::DraftRecovery {
            draft,
            problem,
            error,
        } => {
            let choices = match problem {
                DraftProblem::SaveFailed => {
                    "[r] retry save   [e] re-edit   [d] discard   [Esc] keep for later"
                }
                DraftProblem::Invalid => "[e] re-edit   [d] discard   [Esc] keep for later",
            };
            render_choice(
                f,
                t,
                &format!("Unsaved changes - {}", draft.name),
                &format!("{error}\n\nYour edited text is kept and has not been lost."),
                choices,
            );
        }
        Mode::ConfirmDiscard { draft, .. } => render_message(
            f,
            t,
            "Discard draft",
            &format!(
                "Throw away your unsaved changes to {}?\n\n[y] discard    any other key: keep",
                draft.name
            ),
        ),
        Mode::SyncFailed {
            name,
            error,
            needs_reconnect,
        } => render_choice(
            f,
            t,
            &format!("Saved, not applied - {name}"),
            &format!(
                "{name} was saved, but applying it to the running interface failed:\n\n{error}\n\nThe saved file and the running interface now differ.{}",
                if *needs_reconnect {
                    "\nThis change needs a reconnect to take effect."
                } else {
                    ""
                }
            ),
            "[s] retry sync   [c] reconnect   [k/Esc] keep as is",
        ),
        Mode::KillSwitchProblem { name, status } => render_choice(
            f,
            t,
            &format!("Kill switch - {name}"),
            &format!(
                "The kill switch for {name} is on but not in force:\n\n  {}\n\nTraffic may leave outside the tunnel until it is repaired.",
                status.label()
            ),
            "[r] repair now   [o] turn it off   [Esc] cancel",
        ),
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub fn popup_area(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w, h)
}

fn text_size(msg: &str) -> (u16, u16) {
    let lines = msg.lines().count().min(u16::MAX as usize) as u16;
    let width = msg
        .lines()
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(20)
        .min(u16::MAX as usize) as u16;
    (width, lines)
}

pub fn render_message(f: &mut Frame, theme: &Theme, title: &str, msg: &str) {
    let (width, lines) = text_size(msg);
    let title_width = title.chars().count().min(u16::MAX as usize) as u16;
    let area = popup_area(
        f.area(),
        width.saturating_add(4).max(title_width.saturating_add(4)),
        lines.saturating_add(3),
    );
    f.render_widget(Clear, area);
    let p = Paragraph::new(theme.text(msg).into_owned())
        .block(theme.block().title(format!(" {} ", theme.text(title))))
        .wrap(Wrap { trim: false });
    f.render_widget(p, area);
}

/// A message with a key line that always stays visible at the bottom.
pub fn render_choice(f: &mut Frame, theme: &Theme, title: &str, body: &str, choices: &str) {
    let (width, lines) = text_size(body);
    let width = width
        .max(choices.chars().count() as u16)
        .max(title.chars().count() as u16)
        .saturating_add(4);
    let area = popup_area(f.area(), width, lines.saturating_add(5));
    f.render_widget(Clear, area);
    let block = theme.block().title(format!(" {} ", theme.text(title)));
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height == 0 {
        return;
    }
    let parts = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(inner);
    f.render_widget(
        Paragraph::new(theme.text(body).into_owned()).wrap(Wrap { trim: false }),
        parts[0],
    );
    f.render_widget(
        Paragraph::new(Line::styled(
            theme.text(choices).into_owned(),
            theme.accent(),
        )),
        parts[1],
    );
}

/// A one-line text field; when the text is wider than the box, the tail
/// (where the cursor is) stays visible.
pub fn render_input(f: &mut Frame, theme: &Theme, title: &str, buf: &str) {
    let area = popup_area(f.area(), 44, 3);
    f.render_widget(Clear, area);
    let room = area.width.saturating_sub(3) as usize;
    let count = buf.chars().count();
    let shown: String = buf.chars().skip(count.saturating_sub(room)).collect();
    let p = Paragraph::new(Line::from(vec![
        Span::raw(shown),
        Span::styled("_", theme.input_cursor()),
    ]))
    .block(
        theme
            .block()
            .title(format!(" {title}  (Enter ok / Esc x) ")),
    );
    f.render_widget(p, area);
}

fn help_area(full: Rect) -> Rect {
    popup_area(full, 70, full.height.saturating_sub(2).clamp(3, 44))
}

/// Last scroll offset of the key map at this frame size.
pub fn help_max_scroll(full: Rect) -> u16 {
    let area = help_area(full);
    let lines = Paragraph::new(keymap::help_text())
        .wrap(Wrap { trim: false })
        .line_count(area.width.saturating_sub(2).max(1));
    lines
        .saturating_sub(area.height.saturating_sub(2) as usize)
        .min(u16::MAX as usize) as u16
}

pub fn render_help(f: &mut Frame, theme: &Theme, scroll: u16) {
    let area = help_area(f.area());
    f.render_widget(Clear, area);
    let max = help_max_scroll(f.area());
    let scroll = scroll.min(max);
    let hint = if max > 0 {
        "↑↓ PgUp/PgDn scroll, "
    } else {
        ""
    };
    let title = format!(
        " wg-tui v{} keys ({hint}other keys close) ",
        env!("CARGO_PKG_VERSION")
    );
    let p = Paragraph::new(keymap::help_text())
        .block(theme.block().title(theme.text(&title).into_owned()))
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    f.render_widget(p, area);
}

pub fn render_about(f: &mut Frame, theme: &Theme) {
    let about = format!(
        "\n\
         \x20   WireGuard TUI  \u{b7}  v{}\n\
         \n\
         \x20 A native Linux terminal UI for plain WireGuard configs,\n\
         \x20 wg, wg-quick, and a small privileged helper.\n\
         \n\
         \x20 GitHub     https://github.com/JamilleJung/wireguard-tui\n\
         \x20 Support    https://www.buymeacoffee.com/jamillejung\n\
         \n\
         \x20 Press w for what's new (changelog) \u{b7} H for concepts\n\
         \n\
         \x20 MIT License  \u{b7}  Built with Rust + ratatui",
        env!("CARGO_PKG_VERSION")
    );
    let area = popup_area(f.area(), 62, 15);
    f.render_widget(Clear, area);
    let p = Paragraph::new(theme.text(&about).into_owned())
        .block(theme.block().title(" About (press any key to close) "));
    f.render_widget(p, area);
}

/// A near-full-screen scrollable document viewer (concepts guide, changelog).
/// `scroll` is the top visible line. Modelled on how the Log view scrolls.
fn doc_area(full: Rect) -> Rect {
    let width = full.width.saturating_sub(4).max(20);
    let height = full.height.saturating_sub(2).max(6);
    popup_area(full, width, height)
}

pub fn doc_max_scroll(body: &str, full: Rect) -> u16 {
    let area = doc_area(full);
    let lines = Paragraph::new(body)
        .wrap(Wrap { trim: false })
        .line_count(area.width.saturating_sub(2).max(1));
    lines
        .saturating_sub(area.height.saturating_sub(2) as usize)
        .min(u16::MAX as usize) as u16
}

pub fn render_doc(f: &mut Frame, theme: &Theme, title: &str, body: &str, scroll: u16) {
    let area = doc_area(f.area());
    f.render_widget(Clear, area);
    let title = format!(" {title}  (↑↓/jk PgUp/PgDn g/G scroll · Esc/q close) ");
    let p = Paragraph::new(theme.text(body).into_owned())
        .block(theme.block().title(theme.text(&title).into_owned()))
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    f.render_widget(p, area);
}

pub fn render_qr(f: &mut Frame, theme: &Theme, lines: &[Line<'static>]) {
    let need_h = (lines.len() as u16).saturating_add(2);
    let need_w = (lines.first().map(|l| l.width()).unwrap_or(20) as u16).saturating_add(2);
    let a = f.area();
    // A clamped QR would crop into an unscannable code - say so instead.
    if need_w > a.width || need_h > a.height {
        render_message(
            f,
            theme,
            "QR too large",
            "This QR is bigger than the terminal.\nEnlarge the window (or zoom out), then press Q again.",
        );
        return;
    }
    let area = popup_area(a, need_w, need_h);
    f.render_widget(Clear, area);
    let p = Paragraph::new(lines.to_vec()).block(
        theme
            .block()
            .title(" QR - scan in the WireGuard app (any key closes) "),
    );
    f.render_widget(p, area);
}

/// Render text to a QR code as terminal lines. Default: half-block cells,
/// dark-on-white so it scans regardless of the terminal's colour theme. The
/// ASCII theme uses two spaces per module with background colours (or `##`
/// without colour); without colour, full/half blocks in the terminal's own
/// foreground colour are used.
pub fn qr_lines(text: &str, theme: &Theme) -> Result<Vec<Line<'static>>, String> {
    let code = qrcode::QrCode::new(text.as_bytes()).map_err(|e| e.to_string())?;
    let w = code.width() as i32;
    let colors = code.to_colors();
    let dark = |x: i32, y: i32| -> bool {
        x >= 0 && y >= 0 && x < w && y < w && colors[(y * w + x) as usize] == qrcode::Color::Dark
    };
    let q = 2i32; // quiet zone
    let total = w + 2 * q;
    let shade = |on: bool| Style::default().bg(if on { Color::Black } else { Color::White });

    let mut lines: Vec<Line<'static>> = Vec::new();
    if theme.ascii() {
        for y in 0..total {
            let spans: Vec<Span<'static>> = (0..total)
                .map(|x| {
                    let on = dark(x - q, y - q);
                    if theme.color {
                        Span::styled("  ", shade(on))
                    } else {
                        Span::raw(if on { "##" } else { "  " })
                    }
                })
                .collect();
            lines.push(Line::from(spans));
        }
        return Ok(lines);
    }
    let mut y = 0;
    while y < total {
        let mut spans: Vec<Span<'static>> = Vec::with_capacity(total as usize);
        for x in 0..total {
            let top = dark(x - q, y - q);
            let bot = dark(x - q, y + 1 - q);
            if theme.color {
                let fg = if top { Color::Black } else { Color::White };
                let bg = if bot { Color::Black } else { Color::White };
                spans.push(Span::styled("▀", Style::default().fg(fg).bg(bg)));
            } else {
                spans.push(Span::raw(match (top, bot) {
                    (true, true) => "█",
                    (true, false) => "▀",
                    (false, true) => "▄",
                    (false, false) => " ",
                }));
            }
        }
        lines.push(Line::from(spans));
        y += 2; // half-block: doubles vertical resolution for 2:1 cells
    }
    Ok(lines)
}

pub fn render_browse(
    f: &mut Frame,
    theme: &Theme,
    dir: &Path,
    entries: &[FileEntry],
    sel: usize,
    marked: &HashSet<PathBuf>,
) {
    let h = entries.len().saturating_add(2).clamp(3, 22) as u16;
    let area = popup_area(f.area(), 72, h);
    f.render_widget(Clear, area);

    let items: Vec<ListItem> = entries
        .iter()
        .map(|e| {
            if e.is_dir {
                ListItem::new(Line::styled(format!("    {}/", e.name), theme.accent()))
            } else if marked.contains(&e.path) {
                ListItem::new(Line::styled(format!("[x] {}", e.name), theme.ok()))
            } else {
                ListItem::new(Line::from(format!("[ ] {}", e.name)))
            }
        })
        .collect();

    // Show the trailing part of the path so the title fits (char-safe).
    let full = dir.to_string_lossy();
    let shown = if full.chars().count() > 26 {
        let tail: String = full
            .chars()
            .rev()
            .take(25)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        format!("...{tail}")
    } else {
        full.into_owned()
    };
    let title = if marked.is_empty() {
        format!(" Import: {shown}  (Space mark | Enter open/import | Bksp up | Esc) ")
    } else {
        format!(
            " Import: {shown}  ({} marked | Enter import all | Esc) ",
            marked.len()
        )
    };

    // ListState scrolls so the selected entry is always visible.
    let mut st = ListState::default();
    st.select(if entries.is_empty() { None } else { Some(sel) });
    let list = List::new(items)
        .block(theme.block().title(title))
        .highlight_style(theme.selection());
    f.render_stateful_widget(list, area, &mut st);
}

/// The backup manager popup: one archive per row (date, size, tunnel count,
/// name), with the selection highlighted and kept in view.
pub fn render_backup(f: &mut Frame, theme: &Theme, items: &[backend::BackupInfo], sel: usize) {
    let area = popup_area(f.area(), 80, 22);
    f.render_widget(Clear, area);
    let view = area.height.saturating_sub(2) as usize;
    let mut lines: Vec<Line> = Vec::new();
    if items.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from("  No backups yet."));
        lines.push(Line::from(""));
        lines.push(Line::styled(
            "  Press  n  to create your first backup of every tunnel config.",
            theme.accent(),
        ));
    } else {
        for (i, b) in items.iter().enumerate() {
            let label = format!(
                " {}   {:>9}   {:>2} cfg   {}",
                backend::fmt_time(b.when_secs),
                backend::fmt_size(b.size),
                b.count,
                b.name
            );
            let style = if i == sel { theme.tab() } else { theme.plain() };
            lines.push(Line::styled(label, style));
        }
    }
    let offset = if items.is_empty() || sel < view {
        0u16
    } else {
        (sel + 1 - view) as u16
    };
    let title = format!(
        " Backups ({}) — n new  Enter/r restore  d delete  x export  Esc close ",
        items.len()
    );
    let p = Paragraph::new(lines)
        .block(theme.block().title(theme.text(&title).into_owned()))
        .scroll((offset, 0));
    f.render_widget(p, area);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn visible(terminal: &ratatui::Terminal<ratatui::backend::TestBackend>) -> String {
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn document_end_reaches_wrapped_tail_in_narrow_terminal() {
        let body = format!("{}\nEND-MARKER", "word ".repeat(100));
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(24, 10)).unwrap();
        let max = doc_max_scroll(&body, terminal.get_frame().area());
        assert!(usize::from(max) > body.lines().count());
        terminal
            .draw(|frame| render_doc(frame, &Theme::default(), "Guide", &body, max))
            .unwrap();
        assert!(visible(&terminal).contains("END-MARKER"));
    }

    #[test]
    fn long_messages_do_not_overflow_popup_dimensions() {
        let backend = ratatui::backend::TestBackend::new(40, 8);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_message(frame, &Theme::default(), "Failure", &"x".repeat(65535)))
            .unwrap();
    }

    #[test]
    fn choice_popups_keep_their_keys_visible_when_text_is_clipped() {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(50, 9)).unwrap();
        let body = "error line\n".repeat(40);
        terminal
            .draw(|frame| {
                render_choice(
                    frame,
                    &Theme::default(),
                    "Draft",
                    &body,
                    "[r] retry  [e] re-edit",
                )
            })
            .unwrap();
        assert!(visible(&terminal).contains("[e] re-edit"));
    }

    #[test]
    fn help_scrolls_to_the_last_binding_in_a_short_terminal() {
        let full = Rect::new(0, 0, 50, 12);
        let max = help_max_scroll(full);
        assert!(max > 0);
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(50, 12)).unwrap();
        terminal
            .draw(|frame| render_help(frame, &Theme::default(), max))
            .unwrap();
        assert!(
            visible(&terminal).contains("Draft kept"),
            "{}",
            visible(&terminal)
        );
    }

    #[test]
    fn long_input_keeps_the_cursor_end_visible() {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(30, 5)).unwrap();
        let text = format!("{}END", "a".repeat(80));
        terminal
            .draw(|frame| render_input(frame, &Theme::default(), "Filter", &text))
            .unwrap();
        assert!(visible(&terminal).contains("END_"));
    }

    #[test]
    fn ascii_qr_uses_only_ascii() {
        let theme = Theme {
            name: crate::theme::ThemeName::Ascii,
            color: false,
        };
        let lines = qr_lines("[Interface]", &theme).unwrap();
        assert!(
            lines
                .iter()
                .all(|l| l.spans.iter().all(|s| s.content.is_ascii()))
        );
        assert!(
            lines
                .iter()
                .any(|l| l.spans.iter().any(|s| s.content == "##"))
        );
    }
}
