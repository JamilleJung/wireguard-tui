//! The Log tab.

use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use crate::state::{App, filtered_log, filtered_log_count};
use crate::theme::Theme;

/// Colour a log line by a crude severity guess so problems stand out.
fn log_line_style(theme: &Theme, line: &str) -> Style {
    let l = line.to_ascii_lowercase();
    if l.contains("error") || l.contains("fail") || l.contains("denied") || l.contains("refused") {
        theme.error()
    } else if l.contains("warn") {
        theme.warn()
    } else if l.contains("=====") {
        theme.heading()
    } else if l.contains("activat") || l.contains("enabled") || l.contains(" added") {
        theme.ok()
    } else {
        theme.plain()
    }
}

/// The log split into styled lines after the active filters (substring +
/// this-tunnel) are applied, with the current line and any visual selection
/// highlighted.
fn filtered_log_lines(app: &App) -> Vec<Line<'static>> {
    let theme = app.theme;
    // Visual-selection range (inclusive) over filtered-line indices.
    let sel = app.log_select_anchor.map(|a| {
        let c = app.log_cursor;
        (a.min(c), a.max(c))
    });
    filtered_log(app)
        .enumerate()
        .map(|(i, line)| {
            let base = log_line_style(&theme, line);
            let styled = if sel.is_some_and(|(lo, hi)| i >= lo && i <= hi) {
                base.patch(theme.selection())
            } else if i == app.log_cursor {
                base.patch(theme.cursor_line())
            } else {
                base
            };
            Line::styled(line.to_string(), styled)
        })
        .collect()
}

pub fn render_log(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme;
    // Cursor, highlight and copy commands must refer to the line that is visible
    // in follow mode. Clamp before building styles, including after filtering.
    let last = filtered_log_count(app).saturating_sub(1);
    app.log_cursor = if app.log_follow {
        last
    } else {
        app.log_cursor.min(last)
    };
    let mut lines = filtered_log_lines(app);
    if lines.is_empty() {
        let why = if !app.log_filter.is_empty() || app.log_only_selected {
            "No log lines match the current filter.  Press / to edit, t to toggle this-tunnel, or c to clear."
        } else if app.log.trim().is_empty() {
            "(no log loaded — press r to reload from the journal)"
        } else {
            // app.log holds the backend's own placeholder/error string.
            app.log.as_str()
        };
        lines.push(Line::styled(theme.text(why).into_owned(), theme.label()));
    }

    let total = lines.len().min(u16::MAX as usize) as u16;
    let view_h = area.height.saturating_sub(2);
    let max_scroll = total.saturating_sub(view_h);

    // Clamp cursor into range first (filters may have shrunk the list).
    let last_line = total.saturating_sub(1);
    if app.log_cursor > last_line as usize {
        app.log_cursor = last_line as usize;
    }
    let cursor = app.log_cursor as u16;

    // Scroll so the cursor stays visible; follow pins to the bottom.
    app.log_scroll = if app.log_follow {
        max_scroll
    } else if cursor < app.log_scroll {
        cursor // cursor above viewport → scroll up
    } else if view_h > 0 && cursor >= app.log_scroll.saturating_add(view_h) {
        cursor.saturating_sub(view_h - 1) // cursor below viewport → scroll down
    } else {
        app.log_scroll.min(max_scroll)
    };

    let mut title = format!(" Log — {total} lines");
    if !app.log_filter.is_empty() {
        title.push_str(&format!("  /{}", app.log_filter));
    }
    if app.log_only_selected {
        title.push_str("  [this-tunnel]");
    }
    title.push_str(if app.log_follow {
        "  [follow]"
    } else {
        "  [paused]"
    });
    title.push(' ');
    // No wrap: horizontal scroll (h/l) needs single-row lines to be meaningful.
    let p = Paragraph::new(lines)
        .block(theme.block().title(theme.text(&title).into_owned()))
        .scroll((app.log_scroll, app.log_hscroll));
    f.render_widget(p, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Snapshot;

    #[test]
    fn follow_cursor_and_visual_snapshot_match_displayed_log() {
        let mut app = App {
            log: "first\nsecond\nlatest".into(),
            log_follow: true,
            ..App::default()
        };
        let backend = ratatui::backend::TestBackend::new(40, 5);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_log(frame, &mut app, frame.area()))
            .unwrap();
        assert_eq!(app.log_cursor, 2);
        app.log_select_anchor = Some(1);
        *app.poll_out.lock().unwrap() = Some(Snapshot {
            log: Some("rotated".into()),
            ..Snapshot::default()
        });
        app.apply_snapshot();
        assert_eq!(app.log, "first\nsecond\nlatest");
    }
}
