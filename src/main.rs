//! A full-screen terminal UI for managing WireGuard tunnels — the same feature
//! set as the desktop client (list, live status, activate/deactivate, editor,
//! key generation, QR, export, start-on-boot), driven entirely by the keyboard.

mod backend;

use std::io;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Tabs, Wrap};

const TICK: Duration = Duration::from_millis(1500);

/// What the foreground is currently doing — a normal view or a modal popup.
enum Mode {
    Normal,
    Help,
    /// A transient info/error box (any key closes it).
    Message(String),
    /// A pre-rendered QR code (any key closes it).
    Qr(Vec<Line<'static>>),
    /// Delete confirmation for the named tunnel.
    ConfirmDelete(String),
    /// Text entry for a brand-new tunnel name.
    InputNew(String),
    /// Text entry to rename `orig`.
    InputRename {
        orig: String,
        buf: String,
    },
    /// Text entry for a path to import (a `.conf` file or a QR-code image).
    InputImport(String),
}

struct App {
    tunnels: Vec<backend::Tunnel>,
    state: ListState,
    detail: Option<backend::Detail>,
    tab: usize, // 0 = Tunnels, 1 = Log
    log: String,
    log_scroll: u16,
    status: String,
    mode: Mode,
    quit: bool,
}

impl App {
    fn new() -> Self {
        let mut app = App {
            tunnels: Vec::new(),
            state: ListState::default(),
            detail: None,
            tab: 0,
            log: String::new(),
            log_scroll: 0,
            status: String::new(),
            mode: Mode::Normal,
            quit: false,
        };
        app.reload();
        if !app.tunnels.is_empty() {
            app.state.select(Some(0));
            app.load_detail();
        }
        app
    }

    fn selected_name(&self) -> Option<String> {
        self.state
            .selected()
            .and_then(|i| self.tunnels.get(i))
            .map(|t| t.name.clone())
    }

    /// Refresh the tunnel list (and keep a sensible selection).
    fn reload(&mut self) {
        let prev = self.selected_name();
        self.tunnels = backend::list_tunnels();
        if self.tunnels.is_empty() {
            self.state.select(None);
            self.detail = None;
            return;
        }
        let idx = prev
            .and_then(|p| self.tunnels.iter().position(|t| t.name == p))
            .unwrap_or(0)
            .min(self.tunnels.len() - 1);
        self.state.select(Some(idx));
        self.load_detail();
    }

    fn load_detail(&mut self) {
        self.detail = self.selected_name().map(|n| backend::get_detail(&n));
    }

    /// Periodic refresh: live status, and the log if that tab is open.
    fn tick(&mut self) {
        self.reload();
        if self.tab == 1 {
            self.log = backend::get_log();
        }
    }

    fn move_selection(&mut self, delta: isize) {
        if self.tunnels.is_empty() {
            return;
        }
        let len = self.tunnels.len() as isize;
        let cur = self.state.selected().unwrap_or(0) as isize;
        let next = (cur + delta).rem_euclid(len) as usize;
        self.state.select(Some(next));
        self.load_detail();
    }

    fn flash(&mut self, msg: impl Into<String>) {
        self.status = msg.into();
    }
}

fn main() -> io::Result<()> {
    backend::init();
    let mut terminal = ratatui::init();
    let res = run(&mut terminal);
    ratatui::restore();
    res
}

fn run(terminal: &mut ratatui::DefaultTerminal) -> io::Result<()> {
    let mut app = App::new();
    let mut last = Instant::now();

    while !app.quit {
        terminal.draw(|f| ui(f, &mut app))?;

        let timeout = TICK
            .saturating_sub(last.elapsed())
            .max(Duration::from_millis(50));
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    handle_key(&mut app, terminal, key.code, key.modifiers)?;
                }
            }
        }
        if last.elapsed() >= TICK {
            app.tick();
            last = Instant::now();
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Input handling
// ---------------------------------------------------------------------------

fn handle_key(
    app: &mut App,
    terminal: &mut ratatui::DefaultTerminal,
    code: KeyCode,
    mods: KeyModifiers,
) -> io::Result<()> {
    if mods.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('c') {
        app.quit = true;
        return Ok(());
    }

    // Modal popups consume keys first.
    match &mut app.mode {
        Mode::Help | Mode::Message(_) | Mode::Qr(_) => {
            app.mode = Mode::Normal;
            return Ok(());
        }
        Mode::ConfirmDelete(name) => {
            let name = name.clone();
            match code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    let _ = backend::deactivate(&name);
                    match backend::delete(&name) {
                        Ok(()) => app.flash(format!("Deleted {name}")),
                        Err(e) => app.flash(format!("Delete failed: {e}")),
                    }
                    app.mode = Mode::Normal;
                    app.reload();
                }
                _ => {
                    app.mode = Mode::Normal;
                    app.flash("Delete cancelled");
                }
            }
            return Ok(());
        }
        Mode::InputNew(buf) => {
            match code {
                KeyCode::Esc => app.mode = Mode::Normal,
                KeyCode::Backspace => {
                    buf.pop();
                }
                KeyCode::Enter => {
                    let name = backend::sanitize_name(buf.trim());
                    let empty = buf.trim().is_empty();
                    app.mode = Mode::Normal;
                    if empty {
                        app.flash("Name is required");
                    } else if backend::tunnel_exists(&name) {
                        app.mode =
                            Mode::Message(format!("A tunnel named “{name}” already exists."));
                    } else {
                        create_tunnel(app, terminal, &name)?;
                    }
                }
                KeyCode::Char(c) if buf.len() < 15 => buf.push(c),
                _ => {}
            }
            return Ok(());
        }
        Mode::InputRename { orig, buf } => {
            let orig = orig.clone();
            match code {
                KeyCode::Esc => app.mode = Mode::Normal,
                KeyCode::Backspace => {
                    buf.pop();
                }
                KeyCode::Enter => {
                    let new = backend::sanitize_name(buf.trim());
                    let empty = buf.trim().is_empty();
                    app.mode = Mode::Normal;
                    rename_tunnel(app, &orig, &new, empty);
                }
                KeyCode::Char(c) if buf.len() < 15 => buf.push(c),
                _ => {}
            }
            return Ok(());
        }
        Mode::InputImport(buf) => {
            match code {
                KeyCode::Esc => app.mode = Mode::Normal,
                KeyCode::Backspace => {
                    buf.pop();
                }
                KeyCode::Enter => {
                    let path = buf.trim().to_string();
                    app.mode = Mode::Normal;
                    import_path(app, &path);
                }
                KeyCode::Char(c) => buf.push(c),
                _ => {}
            }
            return Ok(());
        }
        Mode::Normal => {}
    }

    // Normal-mode keys.
    match code {
        KeyCode::Char('q') | KeyCode::Esc => app.quit = true,
        KeyCode::Tab | KeyCode::BackTab => {
            app.tab = 1 - app.tab;
            if app.tab == 1 {
                app.log = backend::get_log();
                app.log_scroll = 0;
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if app.tab == 1 {
                app.log_scroll = app.log_scroll.saturating_add(1);
            } else {
                app.move_selection(1);
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if app.tab == 1 {
                app.log_scroll = app.log_scroll.saturating_sub(1);
            } else {
                app.move_selection(-1);
            }
        }
        KeyCode::Enter | KeyCode::Char('a') => toggle_active(app),
        KeyCode::Char('e') => edit_tunnel(app, terminal)?,
        KeyCode::Char('n') => app.mode = Mode::InputNew(String::new()),
        KeyCode::Char('d') => {
            if let Some(name) = app.selected_name() {
                app.mode = Mode::ConfirmDelete(name);
            }
        }
        KeyCode::Char('R') => {
            if let Some(name) = app.selected_name() {
                app.mode = Mode::InputRename {
                    orig: name.clone(),
                    buf: name,
                };
            }
        }
        KeyCode::Char('s') => toggle_autostart(app),
        KeyCode::Char('i') => app.mode = Mode::InputImport(String::new()),
        KeyCode::Char('g') => generate_show(app),
        KeyCode::Char('c') => show_running(app),
        KeyCode::Char('p') => persist_live(app),
        KeyCode::Char('x') => export(app),
        KeyCode::Char('Q') => show_qr(app),
        KeyCode::Char('r') => {
            app.tick();
            app.flash("Refreshed");
        }
        KeyCode::Char('?') => app.mode = Mode::Help,
        _ => {}
    }
    Ok(())
}

fn toggle_active(app: &mut App) {
    let Some(idx) = app.state.selected() else {
        return;
    };
    let Some(t) = app.tunnels.get(idx) else {
        return;
    };
    let (name, active) = (t.name.clone(), t.active);
    let res = if active {
        backend::deactivate(&name)
    } else {
        backend::activate(&name)
    };
    match res {
        Ok(()) => app.flash(format!(
            "{} {name}",
            if active { "Deactivated" } else { "Activated" }
        )),
        Err(e) => app.flash(format!("Failed: {e}")),
    }
    app.reload();
}

fn toggle_autostart(app: &mut App) {
    let Some(d) = &app.detail else { return };
    let (name, want) = (d.name.clone(), !d.autostart);
    match backend::set_autostart(&name, want) {
        Ok(()) => app.flash(format!(
            "Start on boot {} for {name}",
            if want { "enabled" } else { "disabled" }
        )),
        Err(e) => app.flash(format!("Failed: {e}")),
    }
    app.load_detail();
}

fn export(app: &mut App) {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let dest = std::path::Path::new(&home).join("wireguard-tunnels.zip");
    match backend::export_zip(&dest) {
        Ok(n) => app.mode = Mode::Message(format!("Exported {n} tunnel(s) to\n{}", dest.display())),
        Err(e) => app.flash(format!("Export failed: {e}")),
    }
}

/// Generate a fresh keypair + preshared key and show them so they can be
/// pasted into the editor (mirrors the desktop client's Generate buttons).
fn generate_show(app: &mut App) {
    match (backend::generate_keypair(), backend::generate_psk()) {
        (Ok((priv_k, pub_k)), Ok(psk)) => {
            app.mode = Mode::Message(format!(
                "Generated — copy what you need into the editor:\n\n\
                 PrivateKey   = {priv_k}\n\
                 PublicKey    = {pub_k}\n\
                 PresharedKey = {psk}"
            ));
        }
        _ => app.flash("Key generation failed (is 'wg' installed?)"),
    }
}

/// Show a running tunnel's live wg-level config (`wg showconf`).
fn show_running(app: &mut App) {
    let Some(d) = &app.detail else { return };
    if !d.active {
        app.flash("Tunnel is not active");
        return;
    }
    let name = d.name.clone();
    match backend::running_config(&name) {
        Ok(c) => app.mode = Mode::Message(format!("Running config — {name}\n\n{}", c.trim_end())),
        Err(e) => app.flash(format!("showconf failed: {e}")),
    }
}

fn persist_live(app: &mut App) {
    let Some(d) = &app.detail else { return };
    if !d.active {
        app.flash("Tunnel is not active");
        return;
    }
    let name = d.name.clone();
    match backend::persist_live(&name) {
        Ok(()) => app.flash(format!("Saved live state of {name} to its .conf")),
        Err(e) => app.flash(format!("Save-live failed: {e}")),
    }
    app.reload();
}

/// Import a tunnel from a `.conf` file or a QR-code image (`.png`/`.jpg`).
fn import_path(app: &mut App, raw: &str) {
    if raw.is_empty() {
        app.flash("Import cancelled");
        return;
    }
    // Expand a leading ~ to $HOME.
    let expanded = if let Some(rest) = raw.strip_prefix("~/") {
        let home = std::env::var("HOME").unwrap_or_default();
        format!("{home}/{rest}")
    } else {
        raw.to_string()
    };
    let path = std::path::PathBuf::from(&expanded);
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let content = if matches!(ext.as_str(), "png" | "jpg" | "jpeg") {
        backend::decode_qr(&path)
    } else {
        std::fs::read_to_string(&path).map_err(|e| e.to_string())
    };
    let content = match content {
        Ok(c) => c,
        Err(e) => {
            app.mode = Mode::Message(format!("Import failed:\n{e}"));
            return;
        }
    };
    if let Err(e) = backend::validate_config(&content) {
        app.mode = Mode::Message(format!("Import failed — invalid config:\n{e}"));
        return;
    }
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("imported");
    let name = backend::unique_name(stem);
    match backend::save_config(&name, &content) {
        Ok(()) => {
            let warn = if backend::config_runs_scripts(&content) {
                " ⚠ runs scripts as root"
            } else {
                ""
            };
            app.flash(format!("Imported as {name}{warn}"));
        }
        Err(e) => app.flash(format!("Import failed: {e}")),
    }
    app.reload();
    if let Some(i) = app.tunnels.iter().position(|t| t.name == name) {
        app.state.select(Some(i));
        app.load_detail();
    }
}

fn show_qr(app: &mut App) {
    let Some(name) = app.selected_name() else {
        return;
    };
    match backend::read_config(&name).and_then(|c| qr_lines(&c)) {
        Ok(lines) => app.mode = Mode::Qr(lines),
        Err(e) => app.flash(format!("QR failed: {e}")),
    }
}

/// Open the user's $EDITOR on an existing tunnel's config, then validate + save.
fn edit_tunnel(app: &mut App, terminal: &mut ratatui::DefaultTerminal) -> io::Result<()> {
    let Some(name) = app.selected_name() else {
        return Ok(());
    };
    let cfg = match backend::read_config(&name) {
        Ok(c) => c,
        Err(e) => {
            app.flash(format!("Read failed: {e}"));
            return Ok(());
        }
    };
    let Some(edited) = run_editor(terminal, &cfg)? else {
        app.flash("Edit cancelled");
        return Ok(());
    };
    if edited == cfg {
        app.flash("No changes");
        return Ok(());
    }
    if let Err(e) = backend::validate_config(&edited) {
        app.mode = Mode::Message(format!("Not saved — invalid config:\n{e}"));
        return Ok(());
    }
    match backend::save_config(&name, &edited) {
        Ok(()) => {
            // Apply live to a running tunnel without dropping sessions.
            let active = app.detail.as_ref().map(|d| d.active).unwrap_or(false);
            if active {
                match backend::sync_running(&name) {
                    Ok(()) => app.flash(format!("Saved {name} (applied live)")),
                    Err(_) => {
                        app.flash(format!("Saved {name} — reconnect to apply Address/DNS/MTU"))
                    }
                }
            } else {
                app.flash(format!("Saved {name}"));
            }
            if backend::config_runs_scripts(&edited) {
                app.flash(format!("{} ⚠ runs scripts as root", app.status));
            }
        }
        Err(e) => app.flash(format!("Save failed: {e}")),
    }
    app.reload();
    Ok(())
}

/// Create a brand-new tunnel: a generated template, opened in $EDITOR.
fn create_tunnel(
    app: &mut App,
    terminal: &mut ratatui::DefaultTerminal,
    name: &str,
) -> io::Result<()> {
    let template = backend::new_tunnel_template();
    let Some(edited) = run_editor(terminal, &template)? else {
        app.flash("New tunnel cancelled");
        return Ok(());
    };
    if let Err(e) = backend::validate_config(&edited) {
        app.mode = Mode::Message(format!("Not created — invalid config:\n{e}"));
        return Ok(());
    }
    match backend::save_config(name, &edited) {
        Ok(()) => {
            let warn = if backend::config_runs_scripts(&edited) {
                " ⚠ runs scripts as root"
            } else {
                ""
            };
            app.flash(format!("Created {name}{warn}"));
        }
        Err(e) => app.flash(format!("Create failed: {e}")),
    }
    app.reload();
    if let Some(i) = app.tunnels.iter().position(|t| t.name == name) {
        app.state.select(Some(i));
        app.load_detail();
    }
    Ok(())
}

fn rename_tunnel(app: &mut App, orig: &str, new: &str, empty: bool) {
    if empty {
        app.flash("Name is required");
        return;
    }
    if new == orig {
        return;
    }
    if backend::tunnel_exists(new) {
        app.mode = Mode::Message(format!("A tunnel named “{new}” already exists."));
        return;
    }
    let cfg = match backend::read_config(orig) {
        Ok(c) => c,
        Err(e) => {
            app.flash(format!("Read failed: {e}"));
            return;
        }
    };
    if let Err(e) = backend::save_config(new, &cfg) {
        app.flash(format!("Rename failed: {e}"));
        return;
    }
    let _ = backend::deactivate(orig);
    let _ = backend::delete(orig);
    app.flash(format!("Renamed {orig} → {new}"));
    app.reload();
    if let Some(i) = app.tunnels.iter().position(|t| t.name == new) {
        app.state.select(Some(i));
        app.load_detail();
    }
}

/// Suspend the TUI, run `$EDITOR` on a temp copy of `initial`, return the edited
/// text (None if the editor failed to launch). The temp file is mode 0600 and
/// removed afterwards.
fn run_editor(
    terminal: &mut ratatui::DefaultTerminal,
    initial: &str,
) -> io::Result<Option<String>> {
    let pid = std::process::id();
    let path = std::env::temp_dir().join(format!("wg-tui-{pid}.conf"));
    std::fs::write(&path, initial)?;
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));

    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "nano".to_string());
    let mut parts = editor.split_whitespace();
    let prog = parts.next().unwrap_or("nano");
    let args: Vec<&str> = parts.collect();

    ratatui::restore();
    let status = Command::new(prog).args(&args).arg(&path).status();
    *terminal = ratatui::init();
    terminal.clear()?;

    let result = match status {
        Ok(s) if s.success() => std::fs::read_to_string(&path).ok(),
        _ => None,
    };
    let _ = std::fs::remove_file(&path);
    Ok(result)
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn ui(f: &mut Frame, app: &mut App) {
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(f.area());

    // Header tabs.
    let tabs = Tabs::new(vec![" Tunnels ", " Log "])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" WireGuard — terminal "),
        )
        .select(app.tab)
        .highlight_style(Style::default().fg(Color::Black).bg(Color::Cyan).bold());
    f.render_widget(tabs, chunks[0]);

    if app.tab == 0 {
        let body =
            Layout::horizontal([Constraint::Length(28), Constraint::Min(0)]).split(chunks[1]);
        render_list(f, app, body[0]);
        render_detail(f, app, body[1]);
    } else {
        render_log(f, app, chunks[1]);
    }

    // Footer: status if set, else key hints.
    let footer = if app.status.is_empty() {
        Line::from(vec![Span::styled(
            " ↑↓ move  ⏎/a on·off  e edit  n new  i import  g gen-key  c showconf  d del  R rename  s boot  p save-live  Q qr  x export  Tab log  ? help  q quit",
            Style::default().fg(Color::DarkGray),
        )])
    } else {
        Line::from(vec![Span::styled(
            format!(" {}", app.status),
            Style::default().fg(Color::Yellow),
        )])
    };
    f.render_widget(Paragraph::new(footer), chunks[2]);

    // Popups.
    match &app.mode {
        Mode::Help => render_help(f),
        Mode::Message(m) => render_message(f, "Notice", m),
        Mode::ConfirmDelete(name) => render_message(
            f,
            "Confirm delete",
            &format!("Delete tunnel “{name}”?\n\n[y] yes    [n] no"),
        ),
        Mode::InputNew(buf) => render_input(f, "New tunnel name", buf),
        Mode::InputRename { buf, .. } => render_input(f, "Rename tunnel", buf),
        Mode::InputImport(buf) => render_input(f, "Import path (.conf or QR .png)", buf),
        Mode::Qr(lines) => render_qr(f, lines),
        Mode::Normal => {}
    }
}

fn render_list(f: &mut Frame, app: &mut App, area: Rect) {
    let items: Vec<ListItem> = app
        .tunnels
        .iter()
        .map(|t| {
            let (dot, color) = if t.active {
                ("●", Color::Green)
            } else {
                ("○", Color::DarkGray)
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!(" {dot} "), Style::default().fg(color)),
                Span::raw(t.name.clone()),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" Tunnels "))
        .highlight_style(Style::default().bg(Color::Blue).fg(Color::White).bold())
        .highlight_symbol("");
    f.render_stateful_widget(list, area, &mut app.state);
}

fn kv(label: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("  {label:<14}"),
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw(value.to_string()),
    ])
}

fn render_detail(f: &mut Frame, app: &App, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();
    let title = match &app.detail {
        Some(d) => format!(" {} ", d.name),
        None => " Details ".to_string(),
    };

    if let Some(d) = &app.detail {
        lines.push(Line::from(Span::styled(
            "Interface",
            Style::default().fg(Color::Cyan).bold(),
        )));
        let (st, sc) = if d.active {
            ("Active", Color::Green)
        } else {
            ("Inactive", Color::DarkGray)
        };
        lines.push(Line::from(vec![
            Span::styled("  Status        ", Style::default().fg(Color::DarkGray)),
            Span::styled(st, Style::default().fg(sc).bold()),
        ]));
        lines.push(kv("Public key", &dash(&d.public_key)));
        lines.push(kv("Listen port", &dash(&d.listen_port)));
        lines.push(kv("Addresses", &dash(&d.addresses)));
        lines.push(kv("DNS", &dash(&d.dns)));
        lines.push(kv("Start on boot", if d.autostart { "Yes" } else { "No" }));

        for (i, p) in d.peers.iter().enumerate() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("Peer {}", i + 1),
                Style::default().fg(Color::Cyan).bold(),
            )));
            lines.push(kv("Public key", &dash(&p.public_key)));
            lines.push(kv("Preshared key", if p.preshared { "Yes" } else { "—" }));
            lines.push(kv("Allowed IPs", &dash(&p.allowed_ips)));
            lines.push(kv("Endpoint", &dash(&p.endpoint)));
            lines.push(kv("Keepalive", &dash(&p.keepalive)));
            lines.push(kv("Latest hs", &dash(&p.latest_handshake)));
            lines.push(kv("Transfer", &dash(&p.transfer)));
        }
    } else {
        lines.push(Line::from("No tunnels yet — press 'n' to create one."));
    }

    let p = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(title))
        .wrap(Wrap { trim: false });
    f.render_widget(p, area);
}

fn dash(s: &str) -> String {
    if s.trim().is_empty() {
        "—".to_string()
    } else {
        s.to_string()
    }
}

fn render_log(f: &mut Frame, app: &App, area: Rect) {
    let p = Paragraph::new(app.log.clone())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Log  (↑↓ scroll) "),
        )
        .scroll((app.log_scroll, 0))
        .wrap(Wrap { trim: false });
    f.render_widget(p, area);
}

// ---- popups ----------------------------------------------------------------

fn popup_area(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w, h)
}

fn render_message(f: &mut Frame, title: &str, msg: &str) {
    let lines = msg.lines().count() as u16;
    let width = msg.lines().map(|l| l.chars().count()).max().unwrap_or(20) as u16;
    let area = popup_area(f.area(), (width + 4).max(title.len() as u16 + 4), lines + 3);
    f.render_widget(Clear, area);
    let p = Paragraph::new(msg.to_string())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {title} ")),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(p, area);
}

fn render_input(f: &mut Frame, title: &str, buf: &str) {
    let area = popup_area(f.area(), 44, 3);
    f.render_widget(Clear, area);
    let p = Paragraph::new(Line::from(vec![
        Span::raw(buf.to_string()),
        Span::styled("▏", Style::default().fg(Color::Cyan)),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" {title}  (Enter ✓ / Esc ✗) ")),
    );
    f.render_widget(p, area);
}

fn render_help(f: &mut Frame) {
    let help = "\
  ↑ / k, ↓ / j   Move selection (scroll the Log tab)
  Enter / a      Activate or deactivate the selected tunnel
  e              Edit the selected tunnel in $EDITOR
  n              Create a new tunnel (generated key + $EDITOR)
  i              Import a tunnel from a .conf file or QR image
  g              Generate a keypair + preshared key (to paste in)
  c              Show a running tunnel's live config (wg showconf)
  d              Delete the selected tunnel
  R              Rename the selected tunnel
  s              Toggle start-on-boot for the selected tunnel
  p              Save a running tunnel's live state to its .conf
  Q              Show the tunnel as a QR code (scan into mobile)
  x              Export all tunnels to ~/wireguard-tunnels.zip
  Tab            Switch between the Tunnels and Log tabs
  r              Refresh now
  ?              This help    q / Esc   Quit";
    let area = popup_area(f.area(), 66, 20);
    f.render_widget(Clear, area);
    let p = Paragraph::new(help).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Keys  (press any key to close) "),
    );
    f.render_widget(p, area);
}

fn render_qr(f: &mut Frame, lines: &[Line<'static>]) {
    let h = lines.len() as u16 + 2;
    let w = lines.first().map(|l| l.width()).unwrap_or(20) as u16 + 2;
    let area = popup_area(f.area(), w, h);
    f.render_widget(Clear, area);
    let p = Paragraph::new(lines.to_vec()).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" QR — scan in the WireGuard app (any key closes) "),
    );
    f.render_widget(p, area);
}

/// Render text to a QR code as half-block terminal lines (dark-on-white, so it
/// scans regardless of the terminal's color theme).
fn qr_lines(text: &str) -> Result<Vec<Line<'static>>, String> {
    let code = qrcode::QrCode::new(text.as_bytes()).map_err(|e| e.to_string())?;
    let w = code.width() as i32;
    let colors = code.to_colors();
    let dark = |x: i32, y: i32| -> bool {
        x >= 0 && y >= 0 && x < w && y < w && colors[(y * w + x) as usize] == qrcode::Color::Dark
    };
    let q = 2i32; // quiet zone
    let total = w + 2 * q;
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut y = 0;
    while y < total {
        let mut spans: Vec<Span<'static>> = Vec::with_capacity(total as usize);
        for x in 0..total {
            let top = dark(x - q, y - q);
            let bot = dark(x - q, y + 1 - q);
            let fg = if top { Color::Black } else { Color::White };
            let bg = if bot { Color::Black } else { Color::White };
            spans.push(Span::styled("▀", Style::default().fg(fg).bg(bg)));
        }
        lines.push(Line::from(spans));
        y += 2;
    }
    Ok(lines)
}
