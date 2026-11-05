//! The Detail pane: interface and per-peer fields, health, scrolling, and
//! keeping the focused field in view.

use ratatui::prelude::*;
use ratatui::widgets::{Paragraph, Wrap};

use crate::backend::{self, Health, KillSwitch, fmt_ago};
use crate::ops::OpKind;
use crate::state::App;
use crate::theme::Theme;

fn kv(theme: &Theme, label: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("  {label:<14}"), theme.label()),
        Span::raw(theme.text(value).into_owned()),
    ])
}

fn kv_styled(theme: &Theme, label: &str, value: String, style: Style) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("  {label:<14}"), theme.label()),
        Span::styled(theme.text(&value).into_owned(), style),
    ])
}

fn dash(s: &str) -> String {
    if s.trim().is_empty() {
        "-".to_string()
    } else {
        s.to_string()
    }
}

fn health_style(theme: &Theme, health: Health) -> Style {
    match health {
        Health::Healthy => theme.ok(),
        Health::Idle | Health::NoTraffic => theme.plain(),
        Health::Connecting | Health::HandshakeStale => theme.warn(),
        Health::ConnectionFailed => theme.error(),
    }
}

/// The pane's lines plus where the copyable fields and peer headings are.
pub struct DetailLines {
    pub lines: Vec<Line<'static>>,
    /// Line index of each copyable field, in `App::detail_entries` order.
    pub field_rows: Vec<usize>,
    pub title: String,
}

pub fn build(app: &App) -> DetailLines {
    let theme = app.theme;
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut field_rows = Vec::new();
    let Some(d) = &app.detail else {
        lines.push(Line::styled("No tunnels yet", theme.heading()));
        lines.push(Line::from(""));
        lines.push(Line::from(
            "Import a WireGuard .conf file or a QR-code image to begin:",
        ));
        lines.push(Line::from(""));
        lines.push(Line::from("  i   Import a .conf file or QR image"));
        lines.push(Line::from("  n   Create a new tunnel from scratch"));
        return DetailLines {
            lines,
            field_rows,
            title: " Details ".into(),
        };
    };

    // Copyable fields are highlighted when focused; their order matches
    // `App::detail_entries` exactly, so `y` copies the highlighted row.
    let mut push_field = |lines: &mut Vec<Line<'static>>, label: &str, value: &str| {
        if value.is_empty() {
            lines.push(kv(&theme, label, "-"));
            return;
        }
        let mut line = kv(&theme, label, value);
        if field_rows.len() == app.detail_cursor {
            line = line.style(theme.focus());
        }
        field_rows.push(lines.len());
        lines.push(line);
    };

    lines.push(Line::styled("Interface", theme.heading()));
    lines.push(if d.active {
        kv_styled(&theme, "Status", "Active".into(), theme.ok())
    } else {
        kv_styled(&theme, "Status", "Inactive".into(), theme.label())
    });
    if d.active {
        let ago = d
            .handshake_age
            .map(|s| format!("last handshake {} ago", fmt_ago(s)))
            .unwrap_or_else(|| "no handshake yet".into());
        match app.tunnel_health() {
            Some(health) => lines.push(kv_styled(
                &theme,
                "Connection",
                format!("{} ({}; {ago})", health.label(), health.explanation()),
                health_style(&theme, health),
            )),
            None => lines.push(kv(&theme, "Connection", &ago)),
        }
        lines.push(kv(
            &theme,
            "Speed",
            &format!(
                "↓ {}/s   ↑ {}/s",
                backend::fmt_bytes(app.rate_rx),
                backend::fmt_bytes(app.rate_tx)
            ),
        ));
    }
    push_field(&mut lines, "Public key", &d.public_key);
    push_field(&mut lines, "Listen port", &d.listen_port);
    push_field(&mut lines, "Addresses", &d.addresses);
    push_field(&mut lines, "DNS", &d.dns);
    lines.push(kv(
        &theme,
        "Start on boot",
        if d.autostart { "Yes" } else { "No" },
    ));
    let repairing = app
        .op
        .running()
        .is_some_and(|(kind, _)| kind == OpKind::KillSwitchRepair);
    let (ks_text, ks_style) = match &d.killswitch {
        KillSwitch::Enabled => ("Yes".to_string(), theme.ok()),
        KillSwitch::Disabled => ("No".to_string(), theme.plain()),
        status if status.is_broken() && d.active => (
            format!(
                "{} ({})",
                status.label(),
                if repairing {
                    "repairing…"
                } else {
                    "press K to repair"
                }
            ),
            theme.error(),
        ),
        status => (status.label().to_string(), theme.warn()),
    };
    lines.push(kv_styled(&theme, "Kill switch", ks_text, ks_style));

    let count = d.peers.len();
    for (i, p) in d.peers.iter().enumerate() {
        lines.push(Line::from(""));
        let mut heading = vec![Span::styled(
            if count > 1 {
                format!("Peer {}/{count}", i + 1)
            } else {
                format!("Peer {}", i + 1)
            },
            theme.heading(),
        )];
        if d.active
            && let Some(health) = app.peer_health.get(i)
        {
            heading.push(Span::styled(
                format!("  [{}]", health.label()),
                health_style(&theme, *health),
            ));
        }
        lines.push(Line::from(heading));
        push_field(&mut lines, "Public key", &p.public_key);
        lines.push(kv(
            &theme,
            "Preshared key",
            if p.preshared { "Yes" } else { "-" },
        ));
        push_field(&mut lines, "Allowed IPs", &p.allowed_ips);
        push_field(&mut lines, "Endpoint", &p.endpoint);
        lines.push(kv(&theme, "Keepalive", &dash(&p.keepalive)));
        lines.push(kv(&theme, "Latest hs", &dash(&p.latest_handshake)));
        lines.push(kv(&theme, "Transfer", &dash(&p.transfer)));
    }

    let focused_peer = app
        .detail_entries()
        .get(app.detail_cursor)
        .and_then(|(_, _, peer)| *peer);
    let title = match focused_peer {
        Some(peer) if count > 1 => format!(" {} — peer {}/{count} ", d.name, peer + 1),
        _ => format!(" {} ", d.name),
    };
    DetailLines {
        lines,
        field_rows,
        title,
    }
}

/// Rows each line occupies when wrapped to `width`.
fn line_heights(lines: &[Line<'static>], width: u16) -> Vec<u16> {
    lines
        .iter()
        .map(|line| {
            Paragraph::new(line.clone())
                .wrap(Wrap { trim: false })
                .line_count(width)
                .clamp(1, u16::MAX as usize) as u16
        })
        .collect()
}

/// New scroll offset: keep `[top, bottom)` visible inside a `view`-row window,
/// clamped to `max`.
pub fn keep_visible(scroll: u16, top: u16, bottom: u16, view: u16, max: u16) -> u16 {
    let scroll = if top < scroll {
        top
    } else if view > 0 && bottom > scroll.saturating_add(view) {
        bottom.saturating_sub(view).min(top)
    } else {
        scroll
    };
    scroll.min(max)
}

pub fn render_detail(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme;
    let built = build(app);
    let width = area.width.saturating_sub(2);
    let view = area.height.saturating_sub(2);
    let mut title = built.title.clone();
    if width > 0 && view > 0 {
        let heights = line_heights(&built.lines, width);
        let mut starts = Vec::with_capacity(heights.len());
        let mut total: u16 = 0;
        for h in &heights {
            starts.push(total);
            total = total.saturating_add(*h);
        }
        let max = total.saturating_sub(view);
        app.detail_scroll = if app.detail_follow_focus
            && let Some(&row) = built.field_rows.get(app.detail_cursor)
        {
            // Show up to two lines of context above (e.g. the peer heading).
            let top = starts[row].saturating_sub(2);
            let bottom = starts[row].saturating_add(heights[row]);
            keep_visible(app.detail_scroll, top, bottom, view, max)
        } else {
            app.detail_scroll.min(max)
        };
        if max > 0 {
            let more = match (app.detail_scroll > 0, app.detail_scroll < max) {
                (true, true) => "↑↓",
                (true, false) => "↑",
                _ => "↓",
            };
            title.push_str(&format!("{more} PgUp/PgDn "));
        }
    }
    let p = Paragraph::new(built.lines)
        .block(theme.block().title(theme.text(&title).into_owned()))
        .wrap(Wrap { trim: false })
        .scroll((app.detail_scroll, 0));
    f.render_widget(p, area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keep_visible_scrolls_only_as_far_as_needed() {
        // Already visible: unchanged.
        assert_eq!(keep_visible(0, 2, 3, 10, 50), 0);
        // Below the window: scroll so it is the last visible row.
        assert_eq!(keep_visible(0, 20, 21, 10, 50), 11);
        // Above the window: scroll up to it.
        assert_eq!(keep_visible(30, 5, 6, 10, 50), 5);
        // Taller than the window: show its top.
        assert_eq!(keep_visible(0, 20, 40, 10, 50), 20);
        // Never past the end.
        assert_eq!(keep_visible(0, 60, 61, 10, 50), 50);
    }
}
