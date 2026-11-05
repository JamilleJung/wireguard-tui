//! Rendering regression tests: many terminal sizes (including absurdly small
//! ones), every popup, and every theme.

use std::path::PathBuf;

use ratatui::Terminal;
use ratatui::backend::TestBackend;

use super::*;
use crate::backend::{BackupInfo, Detail, KillSwitch, Peer, Tunnel};
use crate::state::{Draft, DraftProblem, FileEntry, Mode};
use crate::theme::{Theme, ThemeName};

const SIZES: &[(u16, u16)] = &[
    (1, 1),
    (2, 40),
    (20, 5),
    (23, 30),
    (24, 6),
    (30, 8),
    (40, 10),
    (59, 20),
    (60, 13),
    (60, 14),
    (80, 24),
    (100, 30),
    (200, 60),
    (300, 3),
];

fn sample_app(theme: Theme) -> App {
    let mut app = App {
        theme,
        tunnels: (0..30)
            .map(|i| Tunnel {
                name: format!("tunnel{i}"),
                active: i % 3 == 0,
            })
            .collect(),
        detail: Some(Detail {
            name: "tunnel0".into(),
            active: true,
            killswitch: KillSwitch::Missing,
            public_key: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopo=".into(),
            listen_port: "51820".into(),
            addresses: "10.0.0.2/24, fd00::2/64".into(),
            dns: "1.1.1.1".into(),
            peers: (0..12)
                .map(|i| Peer {
                    public_key: format!("peer-key-{i}"),
                    allowed_ips: "0.0.0.0/0, ::/0".into(),
                    endpoint: format!("vpn{i}.example.com:51820"),
                    keepalive: "25".into(),
                    latest_handshake: "3 minutes ago".into(),
                    transfer: "1.00 KiB received, 2.00 KiB sent".into(),
                    handshake_age: Some(200),
                    ..Peer::default()
                })
                .collect(),
            handshake_age: Some(200),
            ..Detail::default()
        }),
        log: "first line\nerror: refused — é…\nlast line".into(),
        ..App::default()
    };
    app.state.select(Some(0));
    app.peer_health = vec![crate::backend::Health::Idle; 12];
    app
}

fn draft() -> Draft {
    Draft {
        name: "tunnel0".into(),
        original: String::new(),
        text: "text".into(),
        new_kind: None,
    }
}

fn modes(theme: &Theme) -> Vec<Mode> {
    vec![
        Mode::Normal,
        Mode::Help { scroll: 3 },
        Mode::About,
        Mode::Message("A long message\n".repeat(50)),
        Mode::ConfirmDelete("tunnel0".into()),
        Mode::InputNew("name".into()),
        Mode::NewPreset {
            name: "tunnel0".into(),
        },
        Mode::InputRename {
            orig: "a".into(),
            buf: "b".repeat(40),
        },
        Mode::ImportBrowse {
            dir: PathBuf::from("/home/user/a/very/long/directory/name/for/imports"),
            entries: (0..40)
                .map(|i| FileEntry {
                    name: format!("file{i}.conf"),
                    path: PathBuf::from(format!("/x/file{i}.conf")),
                    is_dir: i < 3,
                })
                .collect(),
            sel: 35,
            marked: [PathBuf::from("/x/file5.conf")].into_iter().collect(),
        },
        Mode::Backup {
            items: (0..30)
                .map(|i| BackupInfo {
                    path: PathBuf::from(format!("/b/{i}.zip")),
                    name: format!("wg-backup-{i}.zip"),
                    when_secs: 0,
                    size: 1234,
                    count: 2,
                })
                .collect(),
            sel: 25,
        },
        Mode::ConfirmRestore(PathBuf::from("/b/1.zip")),
        Mode::ConfirmBackupDelete(PathBuf::from("/b/1.zip")),
        Mode::LogFilter("needle".into()),
        Mode::Doc {
            title: "Guide".into(),
            body: "Some text — with dashes…\n",
            scroll: 0,
        },
        Mode::DraftRecovery {
            draft: draft(),
            problem: DraftProblem::SaveFailed,
            error: "Not saved:\nPermission denied".into(),
        },
        Mode::ConfirmDiscard {
            draft: draft(),
            problem: DraftProblem::Invalid,
            error: String::new(),
        },
        Mode::SyncFailed {
            name: "tunnel0".into(),
            error: "FwMark changed; reconnect".into(),
            needs_reconnect: true,
        },
        Mode::KillSwitchProblem {
            name: "tunnel0".into(),
            status: KillSwitch::Mismatch,
        },
        Mode::Qr(popups::qr_lines("[Interface]\nPrivateKey = x\n", theme).unwrap()),
    ]
}

fn screen(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    let mut text = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            text.push_str(buffer[(x, y)].symbol());
        }
        text.push('\n');
    }
    text
}

fn draw(app: &mut App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| ui(f, app)).unwrap();
    screen(&terminal)
}

#[test]
fn every_size_and_popup_renders_without_panicking() {
    for theme in [
        Theme::default(),
        Theme {
            name: ThemeName::HighContrast,
            color: true,
        },
        Theme {
            name: ThemeName::Ascii,
            color: false,
        },
    ] {
        for &(w, h) in SIZES {
            for tab in [0, 1] {
                for mode in modes(&theme) {
                    let mut app = sample_app(theme);
                    app.tab = tab;
                    app.mode = mode;
                    draw(&mut app, w, h);
                }
            }
        }
    }
}

#[test]
fn tiny_terminals_explain_instead_of_overlapping() {
    let mut app = sample_app(Theme::default());
    let text = draw(&mut app, 20, 5);
    assert!(text.contains("small"), "{text}");
    assert!(!text.contains("tunnel0"));
    let mut app = sample_app(Theme::default());
    draw(&mut app, 1, 1);
}

#[test]
fn key_content_is_present_at_usable_sizes() {
    for &(w, h) in &[(40, 10), (80, 24), (200, 60), (24, 6), (59, 20)] {
        let mut app = sample_app(Theme::default());
        let text = draw(&mut app, w, h);
        assert!(text.contains("Tunnels"), "{w}x{h}\n{text}");
        if h >= 10 {
            assert!(text.contains("tunnel0"), "{w}x{h}\n{text}");
        }
    }
    let mut app = sample_app(Theme::default());
    let text = draw(&mut app, 80, 24);
    assert!(text.contains("Interface"), "{text}");
    assert!(text.contains("Kill switch"), "{text}");
    assert!(text.contains("press K"), "{text}");
    let mut app = sample_app(Theme::default());
    app.tab = 1;
    let text = draw(&mut app, 80, 24);
    assert!(text.contains("Log"), "{text}");
    assert!(text.contains("last line"), "{text}");
}

#[test]
fn narrow_terminals_stack_the_list_above_the_details() {
    let mut app = sample_app(Theme::default());
    let text = draw(&mut app, 40, 20);
    let lines: Vec<&str> = text.lines().collect();
    let list_row = lines.iter().position(|l| l.contains("Tunnels ")).unwrap();
    let detail_row = lines.iter().position(|l| l.contains("Interface")).unwrap();
    assert!(detail_row > list_row, "{text}");
}

#[test]
fn detail_pane_follows_focus_to_the_last_peer() {
    let mut app = sample_app(Theme::default());
    // Focus the last peer's endpoint.
    app.detail_cursor = app.detail_entries().len() - 1;
    app.detail_follow_focus = true;
    let text = draw(&mut app, 80, 24);
    assert!(text.contains("vpn11.example.com"), "{text}");
    assert!(text.contains("peer 12/12"), "{text}");
    assert!(app.detail_scroll > 0);
    // Scrolling back up shows the interface again.
    app.scroll_detail(-10_000);
    let text = draw(&mut app, 80, 24);
    assert!(text.contains("Interface"), "{text}");
}

#[test]
fn resizing_repeatedly_keeps_state_consistent() {
    let mut app = sample_app(Theme::default());
    app.detail_cursor = 20;
    app.detail_follow_focus = true;
    for &(w, h) in SIZES.iter().chain(SIZES.iter().rev()) {
        draw(&mut app, w, h);
        assert_eq!(app.viewport, ratatui::layout::Rect::new(0, 0, w, h));
    }
}

#[test]
fn ascii_theme_renders_only_ascii() {
    for color in [true, false] {
        let theme = Theme {
            name: ThemeName::Ascii,
            color,
        };
        for &(w, h) in &[(80, 24), (40, 10), (120, 50)] {
            for tab in [0, 1] {
                for mode in modes(&theme) {
                    let mut app = sample_app(theme);
                    app.tab = tab;
                    app.mode = mode;
                    app.status = "Reading tunnel0…".into();
                    let text = draw(&mut app, w, h);
                    assert!(
                        text.is_ascii(),
                        "non-ASCII output at {w}x{h} tab {tab}:\n{text}"
                    );
                }
            }
        }
    }
}

#[test]
fn no_color_rendering_carries_no_colours() {
    let theme = Theme {
        name: ThemeName::Default,
        color: false,
    };
    for mode in modes(&theme) {
        if matches!(mode, Mode::Qr(_)) {
            continue;
        }
        let mut app = sample_app(theme);
        app.mode = mode;
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| ui(f, &mut app)).unwrap();
        for cell in &terminal.backend().buffer().content {
            assert_eq!(cell.fg, ratatui::style::Color::Reset);
            assert_eq!(cell.bg, ratatui::style::Color::Reset);
        }
    }
}

#[test]
fn footer_mentions_the_clock_only_as_a_possible_cause_of_failure() {
    let mut app = sample_app(Theme::default());
    app.detail.as_mut().unwrap().killswitch = KillSwitch::Enabled;
    app.clock_synced = Some(false);
    // Idle peers with an old handshake: no alarm, no clock claim.
    let idle = footer_line(&app, 200).to_string();
    assert!(!idle.contains("NTP"), "{idle}");
    assert!(
        !idle.contains("diagnose") || idle.contains("D diagnose"),
        "{idle}"
    );
    app.peer_health = vec![crate::backend::Health::ConnectionFailed; 12];
    let failed = footer_line(&app, 200).to_string();
    assert!(failed.contains("Connection failed"), "{failed}");
    assert!(failed.contains("one possible cause"), "{failed}");
    app.clock_synced = Some(true);
    assert!(!footer_line(&app, 200).to_string().contains("NTP"));
}
