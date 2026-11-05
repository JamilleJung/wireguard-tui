//! Rendering. Layout degrades gracefully: below [`MIN_WIDTH`] x [`MIN_HEIGHT`]
//! only a "too small" note is drawn; short terminals get a one-line header;
//! narrow ones stack the tunnel list above the details.

pub mod detail;
pub mod log;
pub mod popups;

use ratatui::prelude::*;
use ratatui::widgets::{List, ListItem, Paragraph, Tabs, Wrap};

use crate::backend::health;
use crate::state::App;
use crate::theme::Theme;

pub const MIN_WIDTH: u16 = 24;
pub const MIN_HEIGHT: u16 = 6;

/// Header is bordered only when there is room for it.
fn header_height(area: Rect) -> u16 {
    if area.height >= 14 { 3 } else { 1 }
}

pub fn ui(f: &mut Frame, app: &mut App) {
    let area = f.area();
    app.viewport = area;
    let theme = app.theme;
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        render_too_small(f, &theme);
        theme.finish(f.buffer_mut());
        return;
    }
    let chunks = Layout::vertical([
        Constraint::Length(header_height(area)),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(area);

    let mut tabs = Tabs::new(vec![" Tunnels ", " Log "])
        .select(app.tab)
        .divider(theme.tab_divider())
        .highlight_style(theme.tab());
    if chunks[0].height >= 3 {
        tabs = tabs.block(theme.block().title(" WireGuard - terminal "));
    }
    f.render_widget(tabs, chunks[0]);

    if app.tab == 0 {
        let body = chunks[1];
        let (list_area, detail_area) = if body.width >= 60 {
            let parts =
                Layout::horizontal([Constraint::Length(28), Constraint::Min(0)]).split(body);
            (parts[0], parts[1])
        } else {
            let wanted = (app.tunnels.len() as u16).saturating_add(2).max(3);
            let list_h = wanted.min(body.height / 2).max(body.height.min(3));
            let parts =
                Layout::vertical([Constraint::Length(list_h), Constraint::Min(0)]).split(body);
            (parts[0], parts[1])
        };
        render_list(f, app, list_area);
        detail::render_detail(f, app, detail_area);
    } else {
        log::render_log(f, app, chunks[1]);
    }

    let footer = footer_line(app, chunks[2].width);
    f.render_widget(Paragraph::new(footer), chunks[2]);

    popups::render_mode(f, app);
    theme.finish(f.buffer_mut());
}

fn render_too_small(f: &mut Frame, theme: &Theme) {
    let text =
        format!("Terminal too small. Enlarge to at least {MIN_WIDTH}x{MIN_HEIGHT}. q quits.");
    f.render_widget(
        Paragraph::new(text)
            .style(theme.warn())
            .wrap(Wrap { trim: true }),
        f.area(),
    );
}

/// Pick the full hint line if it fits, else the compact one.
fn fit<'a>(full: &'a str, compact: &'a str, width: u16) -> &'a str {
    if full.chars().count() as u16 <= width {
        full
    } else {
        compact
    }
}

/// Footer priority: update progress > status > kill switch problem > update
/// banner > connection problem > kept draft > key hints.
pub fn footer_line(app: &App, width: u16) -> Line<'static> {
    let theme = app.theme;
    let styled = |text: String, style: Style| {
        Line::from(Span::styled(theme.text(&text).into_owned(), style))
    };
    if let Some(s) = &app.update_status {
        return styled(format!(" {s}"), theme.ok());
    }
    if !app.status.is_empty() {
        return styled(format!(" {}", app.status), theme.warn());
    }
    if let Some(d) = app
        .detail
        .as_ref()
        .filter(|d| d.active && d.killswitch.is_broken())
    {
        let next = if app.killswitch_watch.attempted(&d.name) {
            "automatic repair attempted - press K"
        } else {
            "repair pending - press K"
        };
        return styled(
            format!(
                " Kill switch for {}: {} ({next})",
                d.name,
                d.killswitch.label()
            ),
            theme.error(),
        );
    }
    if let Some(info) = &app.update {
        return styled(
            format!(" update available v{} (press u)", info.latest),
            theme.ok(),
        );
    }
    if app.tab == 0
        && let Some(state) = app.tunnel_health().filter(|h| h.is_problem())
    {
        let mut text = format!(
            " {}: {} — press D to diagnose",
            state.label(),
            state.explanation()
        );
        if health::clock_hint(state, app.clock_synced) {
            text.push_str(" (clock not NTP-synced: one possible cause)");
        }
        return styled(text, theme.error());
    }
    if let Some((draft, _, _)) = &app.parked_draft {
        return styled(
            format!(
                " Unsaved draft for {} kept — press E to reopen it",
                draft.name
            ),
            theme.warn(),
        );
    }
    let hint = if app.tab == 1 {
        fit(
            " j/k move  g/G top/bot  PgUp/Dn  h/l ←→  f follow  t tunnel  / filter  y copy  v select  Y copy-all  w save  c clear  r reload  Tab tunnels  ? help  q quit",
            " j/k move  f follow  / filter  y copy  v select  Y all  w save  Tab tunnels  ? help  q quit",
            width,
        )
    } else {
        fit(
            " Up/Dn move  Enter/a on/off  n new  i import  d remove  s on-boot  Q qr  h/l field  [/] peer  y copy  D diagnose  B backup  Tab log  H explained  …  ? all keys",
            " Up/Dn move  Enter on/off  n new  i import  d remove  Q qr  y copy  D diagnose  Tab log  …  ? all keys",
            width,
        )
    };
    styled(hint.to_string(), theme.hint())
}

fn render_list(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme;
    let items: Vec<ListItem> = app
        .tunnels
        .iter()
        .map(|t| {
            let style = if t.active { theme.ok() } else { theme.label() };
            ListItem::new(Line::from(vec![
                Span::styled(format!(" {} ", theme.marker(t.active)), style),
                Span::raw(t.name.clone()),
            ]))
        })
        .collect();
    let list = List::new(items)
        .block(theme.block().title(" Tunnels "))
        .highlight_style(theme.selection())
        .highlight_symbol("");
    f.render_stateful_widget(list, area, &mut app.state);
}

#[cfg(test)]
mod tests;
