//! README screenshots, rendered from the real UI code into SVG.
//!
//! Ignored by default. `scripts/render-screenshots.sh` runs it with
//! `WG_TUI_SCREENSHOTS=<dir>`; every scene uses fictional tunnels, keys and
//! documentation addresses (RFC 5737 / RFC 3849), never the host's state.

use std::fmt::Write as _;
use std::path::Path;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};

use super::ui;
use crate::backend::{Detail, Health, HelperError, KillSwitch, Peer, Tunnel};
use crate::state::{App, Draft, DraftProblem, Mode};
use crate::theme::{Theme, ThemeName};

const CELL_W: f64 = 8.6;
const CELL_H: f64 = 18.5;
const PAD: f64 = 14.0;
const BASELINE: f64 = 14.0;
const BACKGROUND: &str = "#181825";
const FOREGROUND: &str = "#cdd6f4";

/// The terminal palette the SVGs are drawn with (the same one for every
/// theme: a theme picks colours, the terminal decides how they look).
fn rgb(color: Color) -> Option<&'static str> {
    Some(match color {
        Color::Reset => return None,
        Color::Black => "#11111b",
        Color::Red => "#f38ba8",
        Color::Green => "#a6e3a1",
        Color::Yellow => "#f9e2af",
        Color::Blue => "#3557a3",
        Color::Magenta => "#cba6f7",
        Color::Cyan => "#89dceb",
        Color::Gray => "#bac2de",
        Color::DarkGray => "#9399b2",
        Color::LightRed => "#ff8fa8",
        Color::LightGreen => "#b9f6b0",
        Color::LightYellow => "#ffe9a8",
        Color::LightBlue => "#9fc2ff",
        Color::LightMagenta => "#e2c6ff",
        Color::LightCyan => "#a8f0ff",
        Color::White => "#f5f5ff",
        other => panic!("screenshot palette has no entry for {other:?}"),
    })
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// One SVG `<text>` (plus background `<rect>`) per run of equally styled cells.
fn to_svg(buffer: &Buffer, title: &str, desc: &str) -> String {
    let area = buffer.area;
    let width = f64::from(area.width) * CELL_W + 2.0 * PAD;
    let height = f64::from(area.height) * CELL_H + 2.0 * PAD;
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\" role=\"img\" aria-labelledby=\"title description\" font-family=\"SFMono-Regular,Menlo,DejaVu Sans Mono,monospace\" font-size=\"15\">\n<title id=\"title\">{}</title>\n<desc id=\"description\">{}</desc>\n<rect width=\"{width}\" height=\"{height}\" rx=\"8\" fill=\"{BACKGROUND}\"/>\n",
        escape(title),
        escape(desc)
    );
    for y in 0..area.height {
        let style = |x: u16| {
            let cell = &buffer[(x, y)];
            (cell.fg, cell.bg, cell.modifier)
        };
        let mut x = 0;
        while x < area.width {
            let key = style(x);
            let mut text = String::new();
            let start = x;
            while x < area.width && style(x) == key {
                text.push_str(buffer[(x, y)].symbol());
                x += 1;
            }
            let (mut fg, mut bg) = (rgb(key.0), rgb(key.1));
            if key.2.contains(Modifier::REVERSED) {
                (fg, bg) = (bg.or(Some(BACKGROUND)), fg.or(Some(FOREGROUND)));
            }
            let left = PAD + f64::from(start) * CELL_W;
            let run = f64::from(x - start) * CELL_W;
            let top = PAD + f64::from(y) * CELL_H;
            if let Some(bg) = bg {
                writeln!(
                    out,
                    "<rect x=\"{left:.1}\" y=\"{top:.1}\" width=\"{run:.1}\" height=\"{CELL_H}\" fill=\"{bg}\"/>"
                )
                .unwrap();
            }
            let underline = key.2.contains(Modifier::UNDERLINED);
            if text.trim().is_empty() && !underline {
                continue;
            }
            let mut attrs = format!("fill=\"{}\"", fg.unwrap_or(FOREGROUND));
            if key.2.contains(Modifier::BOLD) {
                attrs.push_str(" font-weight=\"700\"");
            }
            if underline {
                attrs.push_str(" text-decoration=\"underline\"");
            }
            writeln!(
                out,
                "<text x=\"{left:.1}\" y=\"{:.1}\" {attrs} textLength=\"{run:.1}\" lengthAdjust=\"spacingAndGlyphs\" xml:space=\"preserve\">{}</text>",
                top + BASELINE,
                escape(&text)
            )
            .unwrap();
        }
    }
    out.push_str("</svg>\n");
    out
}

fn tunnels(active: &[&str]) -> Vec<Tunnel> {
    ["home", "office", "travel", "lab-v6"]
        .into_iter()
        .map(|name| Tunnel {
            name: name.into(),
            active: active.contains(&name),
        })
        .collect()
}

fn peer(key: &str, ips: &str, endpoint: &str, age: u64, ago: &str, transfer: &str) -> Peer {
    Peer {
        public_key: key.into(),
        allowed_ips: ips.into(),
        endpoint: endpoint.into(),
        keepalive: "25".into(),
        keepalive_secs: Some(25),
        latest_handshake: ago.into(),
        transfer: transfer.into(),
        handshake_age: Some(age),
        ..Peer::default()
    }
}

fn home_detail() -> Detail {
    Detail {
        name: "home".into(),
        active: true,
        autostart: true,
        killswitch: KillSwitch::Enabled,
        public_key: "EXAMPLE+home+publ+key+not+real000000000000A=".into(),
        listen_port: "51820".into(),
        addresses: "192.0.2.10/32, 2001:db8:10::a/128".into(),
        dns: "192.0.2.1".into(),
        peers: vec![Peer {
            preshared: true,
            ..peer(
                "EXAMPLE+home+server+key+not+real0000000000A=",
                "0.0.0.0/0, ::/0",
                "vpn.home.example:51820",
                38,
                "38 seconds ago",
                "1.24 GiB received, 86.31 MiB sent",
            )
        }],
        handshake_age: Some(38),
        ..Detail::default()
    }
}

/// `home` up and selected, healthy, kill switch on.
fn main_app(theme: Theme) -> App {
    let mut app = App {
        theme,
        tunnels: tunnels(&["home"]),
        detail: Some(home_detail()),
        rate_rx: 2_516_582,
        rate_tx: 319_488,
        clock_synced: Some(true),
        ..App::default()
    };
    app.state.select(Some(0));
    app.peer_health = vec![Health::Healthy];
    app
}

/// `office` up with three peers in different health states.
fn peers_app() -> App {
    let mut app = App {
        tunnels: tunnels(&["home", "office"]),
        detail: Some(Detail {
            name: "office".into(),
            active: true,
            killswitch: KillSwitch::Disabled,
            public_key: "EXAMPLE+office+publ+key+not+real0000000000A=".into(),
            listen_port: "51821".into(),
            addresses: "198.51.100.20/24".into(),
            dns: "198.51.100.1".into(),
            peers: vec![
                peer(
                    "EXAMPLE+office+gateway+not+real00000000000A=",
                    "198.51.100.0/24",
                    "gw.office.example:51820",
                    52,
                    "52 seconds ago",
                    "312.40 MiB received, 41.08 MiB sent",
                ),
                Peer {
                    keepalive: String::new(),
                    keepalive_secs: None,
                    ..peer(
                        "EXAMPLE+office+printer+not+real00000000000A=",
                        "198.51.100.40/32",
                        "203.0.113.40:51820",
                        2_710,
                        "45 minutes, 10 seconds ago",
                        "88.12 KiB received, 102.55 KiB sent",
                    )
                },
                peer(
                    "EXAMPLE+office+backup+not+real000000000000A=",
                    "198.51.100.60/32",
                    "203.0.113.60:51820",
                    431,
                    "7 minutes, 11 seconds ago",
                    "4.03 MiB received, 12.77 MiB sent",
                ),
            ],
            handshake_age: Some(52),
            ..Detail::default()
        }),
        rate_rx: 48_128,
        rate_tx: 12_800,
        clock_synced: Some(true),
        ..App::default()
    };
    app.state.select(Some(1));
    app.peer_health = vec![Health::Healthy, Health::Idle, Health::HandshakeStale];
    // Peer navigation: `]` three times from the interface focuses peer 3.
    app.jump_peer(true);
    app.jump_peer(true);
    app.jump_peer(true);
    // Scrolled to the end (PgDn), so all of the focused peer is in view.
    app.detail_scroll = u16::MAX;
    app
}

/// `home` up, kill switch rules gone, automatic repair already tried.
fn killswitch_app() -> App {
    let mut app = main_app(Theme::default());
    let detail = app.detail.as_mut().unwrap();
    detail.killswitch = KillSwitch::Missing;
    let now = std::time::Instant::now();
    assert!(
        app.killswitch_watch
            .observe("home", true, &KillSwitch::Missing, true, now)
    );
    app
}

fn draft_app() -> App {
    let mut app = main_app(Theme::default());
    let error = HelperError::Failed {
        command: "wg-helper save home".into(),
        stderr: "cannot write /etc/wireguard/home.conf: Read-only file system".into(),
    };
    app.mode = Mode::DraftRecovery {
        draft: Draft {
            name: "home".into(),
            original: String::new(),
            text: "[Interface]\n".into(),
            new_kind: None,
        },
        problem: DraftProblem::SaveFailed,
        error: format!("Not saved:\n{error}"),
    };
    app
}

fn help_app() -> App {
    let mut app = main_app(Theme::default());
    app.mode = Mode::Help { scroll: 0 };
    app
}

fn render(app: &mut App, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    // Twice: the first draw records the viewport the detail pane scrolls in.
    terminal.draw(|f| ui(f, app)).unwrap();
    terminal.draw(|f| ui(f, app)).unwrap();
    terminal.backend().buffer().clone()
}

fn write(dir: &Path, file: &str, mut app: App, size: (u16, u16), what: &str) {
    let buffer = render(&mut app, size.0, size.1);
    let title = format!("wg-tui: {what}");
    let desc = format!(
        "Rendered from application code with a {} by {} Ratatui TestBackend and fictional data: example tunnel names, keys and documentation addresses. {what}. No live system or network was accessed.",
        size.0, size.1
    );
    std::fs::write(dir.join(file), to_svg(&buffer, &title, &desc)).unwrap();
}

#[test]
#[ignore = "writes README screenshots; run scripts/render-screenshots.sh"]
fn render_docs_screenshots() {
    let Some(dir) = std::env::var_os("WG_TUI_SCREENSHOTS") else {
        panic!("set WG_TUI_SCREENSHOTS to the output directory");
    };
    let dir = Path::new(&dir);
    std::fs::create_dir_all(dir).unwrap();
    let big = (110, 30);
    write(
        dir,
        "tui-main.svg",
        main_app(Theme::default()),
        big,
        "Tunnels tab with the active home tunnel, a healthy peer, throughput and the kill switch on",
    );
    write(
        dir,
        "tui-peers.svg",
        peers_app(),
        big,
        "A tunnel with three peers in different health states, the third peer focused with peer navigation",
    );
    write(
        dir,
        "tui-killswitch.svg",
        killswitch_app(),
        big,
        "Kill switch rules missing on an active tunnel, with the repair prompt",
    );
    write(
        dir,
        "tui-draft.svg",
        draft_app(),
        big,
        "Draft recovery after a failed save: retry, re-edit or discard",
    );
    write(
        dir,
        "tui-help.svg",
        help_app(),
        big,
        "The scrollable key map",
    );
    write(
        dir,
        "tui-high-contrast.svg",
        main_app(Theme {
            name: ThemeName::HighContrast,
            color: true,
        }),
        big,
        "The main view in the high-contrast theme",
    );
    write(
        dir,
        "tui-ascii.svg",
        main_app(Theme {
            name: ThemeName::Ascii,
            color: true,
        }),
        big,
        "The main view in the ASCII theme",
    );
    write(
        dir,
        "tui-small.svg",
        main_app(Theme::default()),
        (58, 22),
        "A narrow terminal: the tunnel list stacks above the details",
    );
}
