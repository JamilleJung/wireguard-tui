//! A full-screen terminal UI for managing WireGuard tunnels - the same feature
//! set as the desktop client (list, live status, activate/deactivate, editor,
//! key generation, QR, export, start-on-boot), driven entirely by the keyboard.

mod backend;

use std::io;
use std::io::Write as _;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Tabs, Wrap};

/// Background poll interval (live status refresh).
const TICK: Duration = Duration::from_millis(1500);
/// How long a footer status message stays before it auto-clears.
const STATUS_TTL: Duration = Duration::from_secs(4);

/// One entry in the import file browser.
struct FileEntry {
    name: String,
    path: PathBuf,
    is_dir: bool,
}

/// What the background poller is told to fetch (current selection + tab).
#[derive(Default)]
struct PollIn {
    selected: Option<String>,
    want_log: bool,
}

/// A live snapshot computed off the UI thread by the poller.
struct Snapshot {
    /// `None` means the helper call failed - keep the existing list, just report.
    tunnels: Option<Vec<backend::Tunnel>>,
    detail: Option<(String, backend::Detail)>,
    log: Option<String>,
    error: Option<String>,
}

/// What the foreground is currently doing - a normal view or a modal popup.
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
    /// File browser for importing one or more `.conf` files / QR-code images.
    /// `marked` holds files ticked for a bulk import (kept across directories).
    ImportBrowse {
        dir: PathBuf,
        entries: Vec<FileEntry>,
        sel: usize,
        marked: std::collections::HashSet<PathBuf>,
    },
}

struct App {
    tunnels: Vec<backend::Tunnel>,
    state: ListState,
    detail: Option<backend::Detail>,
    tab: usize, // 0 = Tunnels, 1 = Log
    log: String,
    log_scroll: u16,
    status: String,
    status_at: Option<Instant>,
    mode: Mode,
    quit: bool,
    /// Easy mode hides expert actions for everyday users (toggle with `m`).
    easy: bool,
    // Shared with the background poller thread (off-UI-thread live refresh).
    poll_in: Arc<Mutex<PollIn>>,
    poll_out: Arc<Mutex<Option<Snapshot>>>,
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
            status_at: None,
            mode: Mode::Normal,
            quit: false,
            easy: load_easy(),
            poll_in: Arc::new(Mutex::new(PollIn::default())),
            poll_out: Arc::new(Mutex::new(None)),
        };
        app.reload();
        if !app.tunnels.is_empty() {
            app.state.select(Some(0));
            app.load_detail();
        }
        app.sync_poll_in();
        app
    }

    /// Tell the poller what to fetch next (current selection + whether the Log
    /// tab is showing).
    fn sync_poll_in(&self) {
        let mut pi = self.poll_in.lock().unwrap();
        pi.selected = self.selected_name();
        pi.want_log = self.tab == 1;
    }

    /// Apply the latest background snapshot to the UI state (non-blocking).
    fn apply_snapshot(&mut self) {
        let Some(snap) = self.poll_out.lock().unwrap().take() else {
            return;
        };
        if let Some(err) = snap.error {
            self.flash(err);
        }
        if let Some(tunnels) = snap.tunnels {
            // Reconcile, preserving the selection by name.
            let prev = self.selected_name();
            self.tunnels = tunnels;
            if self.tunnels.is_empty() {
                self.state.select(None);
                self.detail = None;
            } else {
                let idx = prev
                    .and_then(|p| self.tunnels.iter().position(|t| t.name == p))
                    .or_else(|| self.state.selected())
                    .unwrap_or(0)
                    .min(self.tunnels.len() - 1);
                self.state.select(Some(idx));
            }
        }
        // Only apply detail that still matches the current selection.
        if let Some((name, d)) = snap.detail {
            if Some(&name) == self.selected_name().as_ref() {
                self.detail = Some(d);
            }
        }
        if let Some(log) = snap.log {
            self.log = log;
        }
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
        self.status_at = Some(Instant::now());
    }

    /// Clear the footer status once it has been shown for `STATUS_TTL`.
    fn expire_status(&mut self) {
        if let Some(at) = self.status_at {
            if at.elapsed() >= STATUS_TTL {
                self.status.clear();
                self.status_at = None;
            }
        }
    }
}

/// Path of the saved Easy/Advanced preference: $XDG_CONFIG_HOME (or ~/.config)
/// /wireguard-tui/mode.
fn mode_state_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("wireguard-tui").join("mode"))
}

/// Load the saved mode. New users default to Easy mode.
fn load_easy() -> bool {
    match mode_state_path().and_then(|p| std::fs::read_to_string(p).ok()) {
        Some(s) => s.trim() != "advanced",
        None => true,
    }
}

/// Persist the mode so the choice sticks across runs.
fn save_easy(easy: bool) {
    if let Some(p) = mode_state_path() {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, if easy { "easy" } else { "advanced" });
    }
}

/// Keys hidden in Easy mode (expert/raw-config actions).
fn is_advanced_key(code: KeyCode) -> bool {
    matches!(
        code,
        KeyCode::Char('e') // edit raw config
            | KeyCode::Char('n') // new from scratch
            | KeyCode::Char('g') // generate keys
            | KeyCode::Char('c') // show running config
            | KeyCode::Char('p') // save live state
            | KeyCode::Char('R') // rename
            | KeyCode::Char('x') // export all
    )
}

/// Minimal standard-alphabet base64 (for the OSC52 clipboard escape).
fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let b0 = c[0] as u32;
        let b1 = *c.get(1).unwrap_or(&0) as u32;
        let b2 = *c.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(if c.len() > 1 {
            T[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if c.len() > 2 {
            T[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Copy `text` to the system clipboard via the OSC 52 terminal escape. Works in
/// terminals that support it (xterm, kitty, wezterm, tmux with passthrough, …);
/// a no-op elsewhere. Doesn't draw, so it won't disturb the ratatui frame.
fn osc52_copy(text: &str) {
    use std::io::Write as _;
    let seq = format!("\x1b]52;c;{}\x07", base64(text.as_bytes()));
    let mut out = std::io::stdout();
    let _ = out.write_all(seq.as_bytes());
    let _ = out.flush();
}

/// Set a status and repaint immediately - so slow privileged calls (which block
/// this single-threaded UI for a second or two) still give instant feedback.
fn flash_now(
    app: &mut App,
    terminal: &mut ratatui::DefaultTerminal,
    msg: impl Into<String>,
) -> io::Result<()> {
    app.flash(msg);
    terminal.draw(|f| ui(f, app))?;
    Ok(())
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
    spawn_poller(app.poll_in.clone(), app.poll_out.clone());

    while !app.quit {
        app.apply_snapshot();
        app.sync_poll_in();
        app.expire_status();
        terminal.draw(|f| ui(f, &mut app))?;

        // Short wake-ups so background snapshots apply promptly and the status
        // clears on time; capped so input stays responsive but the loop idles.
        let mut timeout = Duration::from_millis(250);
        if let Some(at) = app.status_at {
            timeout = timeout.min(STATUS_TTL.saturating_sub(at.elapsed()));
        }
        let timeout = timeout.max(Duration::from_millis(50));

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    handle_key(&mut app, terminal, key.code, key.modifiers)?;
                }
            }
        }
    }
    Ok(())
}

/// Background thread: does the blocking `wg`/`sudo` calls so the UI never
/// freezes, dropping the result into `poll_out` for the UI loop to apply.
fn spawn_poller(poll_in: Arc<Mutex<PollIn>>, poll_out: Arc<Mutex<Option<Snapshot>>>) {
    std::thread::spawn(move || loop {
        let (sel, want_log) = {
            let pi = poll_in.lock().unwrap();
            (pi.selected.clone(), pi.want_log)
        };
        let snap = match backend::try_list_tunnels() {
            Ok(tunnels) => {
                let detail = sel
                    .as_ref()
                    .filter(|n| tunnels.iter().any(|t| &t.name == *n))
                    .map(|n| (n.clone(), backend::get_detail(n)));
                let log = if want_log {
                    Some(backend::get_log())
                } else {
                    None
                };
                Snapshot {
                    tunnels: Some(tunnels),
                    detail,
                    log,
                    error: None,
                }
            }
            Err(e) => Snapshot {
                tunnels: None,
                detail: None,
                log: None,
                error: Some(format!("Helper error: {e}")),
            },
        };
        *poll_out.lock().unwrap() = Some(snap);
        std::thread::sleep(TICK);
    });
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
                    app.mode = Mode::Normal;
                    flash_now(app, terminal, format!("Deleting {name}..."))?;
                    let _ = backend::deactivate(&name);
                    match backend::delete(&name) {
                        Ok(()) => app.flash(format!("Deleted {name}")),
                        Err(e) => app.flash(format!("Delete failed: {e}")),
                    }
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
                    // Require at least one letter/digit, else sanitize_name would
                    // silently fall back to "tunnel".
                    let invalid = !buf.chars().any(|c| c.is_ascii_alphanumeric());
                    app.mode = Mode::Normal;
                    if invalid {
                        app.flash("Name needs a letter or number");
                    } else if backend::tunnel_exists(&name) {
                        app.mode =
                            Mode::Message(format!("A tunnel named '{name}' already exists."));
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
                    let invalid = !buf.chars().any(|c| c.is_ascii_alphanumeric());
                    app.mode = Mode::Normal;
                    rename_tunnel(app, &orig, &new, invalid);
                }
                KeyCode::Char(c) if buf.len() < 15 => buf.push(c),
                _ => {}
            }
            return Ok(());
        }
        Mode::ImportBrowse {
            dir,
            entries,
            sel,
            marked,
        } => {
            match code {
                KeyCode::Esc => app.mode = Mode::Normal,
                KeyCode::Up | KeyCode::Char('k') => {
                    *sel = sel.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if *sel + 1 < entries.len() {
                        *sel += 1;
                    }
                }
                // Space toggles a file's mark for bulk import (dirs can't be marked).
                KeyCode::Char(' ') => {
                    if let Some(e) = entries.get(*sel) {
                        if !e.is_dir && !marked.remove(&e.path) {
                            marked.insert(e.path.clone());
                        }
                    }
                }
                KeyCode::Backspace | KeyCode::Left | KeyCode::Char('h') => {
                    if let Some(parent) = dir.parent().map(|p| p.to_path_buf()) {
                        let entries = read_dir_entries(&parent);
                        let marked = std::mem::take(marked); // keep marks across dirs
                        app.mode = Mode::ImportBrowse {
                            dir: parent,
                            entries,
                            sel: 0,
                            marked,
                        };
                    }
                }
                KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                    let chosen = entries.get(*sel).map(|e| (e.is_dir, e.path.clone()));
                    if let Some((is_dir, path)) = chosen {
                        if is_dir {
                            // Descend, preserving any marks.
                            let entries = read_dir_entries(&path);
                            let marked = std::mem::take(marked);
                            app.mode = Mode::ImportBrowse {
                                dir: path,
                                entries,
                                sel: 0,
                                marked,
                            };
                        } else if marked.is_empty() {
                            app.mode = Mode::Normal;
                            import_file(app, &path);
                        } else {
                            // Bulk import everything marked (the highlighted file
                            // is included automatically if it was marked).
                            let mut paths: Vec<PathBuf> = marked.iter().cloned().collect();
                            paths.sort();
                            app.mode = Mode::Normal;
                            import_files(app, &paths);
                        }
                    }
                }
                _ => {}
            }
            return Ok(());
        }
        Mode::Normal => {}
    }

    // Easy mode hides expert actions; tell the user how to reach them.
    if app.easy && is_advanced_key(code) {
        app.flash("Advanced action - press 'm' to switch to Advanced mode");
        return Ok(());
    }

    // Normal-mode keys.
    match code {
        KeyCode::Char('q') | KeyCode::Esc => app.quit = true,
        KeyCode::Char('m') => {
            app.easy = !app.easy;
            save_easy(app.easy);
            app.flash(if app.easy {
                "Easy mode (everyday actions)"
            } else {
                "Advanced mode (all actions)"
            });
        }
        KeyCode::Char('y') => match app.detail.as_ref().map(|d| d.public_key.clone()) {
            Some(pk) if !pk.is_empty() => {
                osc52_copy(&pk);
                app.flash("Public key copied to clipboard");
            }
            _ => app.flash("No public key to copy"),
        },
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
        KeyCode::Enter | KeyCode::Char('a') => toggle_active(app, terminal)?,
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
        KeyCode::Char('s') => toggle_autostart(app, terminal)?,
        KeyCode::Char('i') => open_import_browser(app),
        KeyCode::Char('g') => generate_show(app),
        KeyCode::Char('c') => show_running(app),
        KeyCode::Char('p') => persist_live(app, terminal)?,
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

fn toggle_active(app: &mut App, terminal: &mut ratatui::DefaultTerminal) -> io::Result<()> {
    let Some(idx) = app.state.selected() else {
        return Ok(());
    };
    let Some(t) = app.tunnels.get(idx) else {
        return Ok(());
    };
    let (name, active) = (t.name.clone(), t.active);
    // `wg-quick` can take a couple of seconds; show progress before we block.
    flash_now(
        app,
        terminal,
        format!(
            "{} {name}...",
            if active { "Deactivating" } else { "Activating" }
        ),
    )?;
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
    Ok(())
}

fn toggle_autostart(app: &mut App, terminal: &mut ratatui::DefaultTerminal) -> io::Result<()> {
    let Some(d) = &app.detail else { return Ok(()) };
    let (name, want) = (d.name.clone(), !d.autostart);
    flash_now(
        app,
        terminal,
        format!("Updating start-on-boot for {name}..."),
    )?;
    match backend::set_autostart(&name, want) {
        Ok(()) => app.flash(format!(
            "Start on boot {} for {name}",
            if want { "enabled" } else { "disabled" }
        )),
        Err(e) => app.flash(format!("Failed: {e}")),
    }
    app.load_detail();
    Ok(())
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
                "Generated - copy what you need into the editor:\n\n\
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
        Ok(c) => app.mode = Mode::Message(format!("Running config - {name}\n\n{}", c.trim_end())),
        Err(e) => app.flash(format!("showconf failed: {e}")),
    }
}

fn persist_live(app: &mut App, terminal: &mut ratatui::DefaultTerminal) -> io::Result<()> {
    let Some(d) = &app.detail else { return Ok(()) };
    if !d.active {
        app.flash("Tunnel is not active");
        return Ok(());
    }
    let name = d.name.clone();
    flash_now(app, terminal, format!("Saving live state of {name}..."))?;
    match backend::persist_live(&name) {
        Ok(()) => app.flash(format!("Saved live state of {name} to its .conf")),
        Err(e) => app.flash(format!("Save-live failed: {e}")),
    }
    app.reload();
    Ok(())
}

/// Build the import browser, starting in the current directory (or $HOME).
fn open_import_browser(app: &mut App) {
    let dir = std::env::current_dir()
        .ok()
        .filter(|p| p.is_dir())
        .or_else(|| std::env::var("HOME").ok().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("/"));
    let entries = read_dir_entries(&dir);
    app.mode = Mode::ImportBrowse {
        dir,
        entries,
        sel: 0,
        marked: std::collections::HashSet::new(),
    };
}

/// List a directory for the import browser: a `..` entry, then sub-directories,
/// then importable files (`.conf`/`.png`/`.jpg`/`.jpeg`). Hidden names are
/// skipped except the parent shortcut.
fn read_dir_entries(dir: &Path) -> Vec<FileEntry> {
    let mut out = Vec::new();
    if let Some(parent) = dir.parent() {
        out.push(FileEntry {
            name: "..".to_string(),
            path: parent.to_path_buf(),
            is_dir: true,
        });
    }
    let (mut dirs, mut files) = (Vec::new(), Vec::new());
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let path = e.path();
            if path.is_dir() {
                dirs.push(FileEntry {
                    name,
                    path,
                    is_dir: true,
                });
            } else {
                let ext = path
                    .extension()
                    .and_then(|x| x.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if matches!(ext.as_str(), "conf" | "png" | "jpg" | "jpeg") {
                    files.push(FileEntry {
                        name,
                        path,
                        is_dir: false,
                    });
                }
            }
        }
    }
    let by_name = |a: &FileEntry, b: &FileEntry| a.name.to_lowercase().cmp(&b.name.to_lowercase());
    dirs.sort_by(by_name);
    files.sort_by(by_name);
    out.extend(dirs);
    out.extend(files);
    out
}

/// Import a tunnel from a `.conf` file or a QR-code image (`.png`/`.jpg`).
fn import_file(app: &mut App, path: &Path) {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let content = if matches!(ext.as_str(), "png" | "jpg" | "jpeg") {
        backend::decode_qr(path)
    } else {
        std::fs::read_to_string(path).map_err(|e| e.to_string())
    };
    let content = match content {
        Ok(c) => c,
        Err(e) => {
            app.mode = Mode::Message(format!("Import failed:\n{e}"));
            return;
        }
    };
    if let Err(e) = backend::validate_config(&content) {
        app.mode = Mode::Message(format!("Import failed - invalid config:\n{e}"));
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
                " ! runs scripts as root"
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

/// Import several files at once (bulk). Each is validated and auto-deduplicated;
/// invalid ones are skipped and any that run root scripts are flagged.
fn import_files(app: &mut App, paths: &[PathBuf]) {
    let (mut count, mut skipped, mut scripts) = (0, 0, false);
    let mut last = None;
    for path in paths {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let content = if matches!(ext.as_str(), "png" | "jpg" | "jpeg") {
            backend::decode_qr(path)
        } else {
            std::fs::read_to_string(path).map_err(|e| e.to_string())
        };
        let Ok(content) = content else {
            skipped += 1;
            continue;
        };
        if backend::validate_config(&content).is_err() {
            skipped += 1;
            continue;
        }
        scripts |= backend::config_runs_scripts(&content);
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("imported");
        let name = backend::unique_name(stem);
        match backend::save_config(&name, &content) {
            Ok(()) => {
                last = Some(name);
                count += 1;
            }
            Err(_) => skipped += 1,
        }
    }
    if count > 0 {
        let warn = if scripts {
            " ! some run scripts as root"
        } else {
            ""
        };
        let skip = if skipped > 0 {
            format!(", {skipped} skipped")
        } else {
            String::new()
        };
        app.flash(format!("Imported {count} tunnel(s){skip}{warn}"));
    } else {
        app.flash(format!("Nothing imported ({skipped} skipped/invalid)"));
    }
    app.reload();
    if let Some(name) = last {
        if let Some(i) = app.tunnels.iter().position(|t| t.name == name) {
            app.state.select(Some(i));
            app.load_detail();
        }
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
        app.mode = Mode::Message(format!("Not saved - invalid config:\n{e}"));
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
                        app.flash(format!("Saved {name} - reconnect to apply Address/DNS/MTU"))
                    }
                }
            } else {
                app.flash(format!("Saved {name}"));
            }
            if backend::config_runs_scripts(&edited) {
                app.flash(format!("{} ! runs scripts as root", app.status));
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
        app.mode = Mode::Message(format!("Not created - invalid config:\n{e}"));
        return Ok(());
    }
    match backend::save_config(name, &edited) {
        Ok(()) => {
            let warn = if backend::config_runs_scripts(&edited) {
                " ! runs scripts as root"
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

fn rename_tunnel(app: &mut App, orig: &str, new: &str, invalid: bool) {
    if invalid {
        app.flash("Name needs a letter or number");
        return;
    }
    if new == orig {
        return;
    }
    if backend::tunnel_exists(new) {
        app.mode = Mode::Message(format!("A tunnel named '{new}' already exists."));
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
    app.flash(format!("Renamed {orig} -> {new}"));
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
    // Put the temp config in a per-user PRIVATE directory: $XDG_RUNTIME_DIR
    // (already mode 0700) when available, else a 0700 subdir of the temp dir.
    // This, plus an O_EXCL 0600 create, means no other user can pre-plant a
    // symlink/file to read the private key or steer the editor.
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .unwrap_or_else(|| {
            let uid = unsafe { libc::geteuid() };
            let d = std::env::temp_dir().join(format!("wg-tui-{uid}"));
            let _ = std::fs::create_dir_all(&d);
            let _ = std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700));
            d
        });
    let pid = std::process::id();
    let path = dir.join(format!("wg-tui-{pid}.conf"));
    let _ = std::fs::remove_file(&path); // clear any stale file from a recycled PID
    {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true) // O_EXCL: never follow a symlink or reuse a file
            .mode(0o600)
            .open(&path)?;
        f.write_all(initial.as_bytes())?;
    }

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
                .title(" WireGuard - terminal "),
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

    // Footer: status if set, else key hints. Easy mode shows only the everyday
    // actions; Advanced shows everything (with a compact fallback on narrow
    // terminals). Both keep ? help / q quit visible.
    let hint = if app.easy {
        " Up/Dn move  Enter/a connect/disconnect  i import  s on-boot  d remove  Q qr  y copy-key  Tab log  m advanced  ? help  q quit"
    } else {
        let full = " Up/Dn move  Enter/a on/off  e edit  n new  i import  g gen-key  y copy-key  c showconf  d del  R rename  s boot  p save-live  Q qr  x export  Tab log  m easy  ? help  q quit";
        let compact = " Up/Dn move  Enter on/off  e edit  n new  i import  y copy-key  d del  Q qr  Tab log  m easy  ? help  q quit";
        if full.chars().count() as u16 <= chunks[2].width {
            full
        } else {
            compact
        }
    };
    let footer = if app.status.is_empty() {
        Line::from(vec![Span::styled(
            hint,
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
            &format!("Delete tunnel '{name}'?\n\n[y] yes    [n] no"),
        ),
        Mode::InputNew(buf) => render_input(f, "New tunnel name", buf),
        Mode::InputRename { buf, .. } => render_input(f, "Rename tunnel", buf),
        Mode::ImportBrowse {
            dir,
            entries,
            sel,
            marked,
        } => render_browse(f, dir, entries, *sel, marked),
        Mode::Qr(lines) => render_qr(f, lines),
        Mode::Normal => {}
    }
}

fn render_browse(
    f: &mut Frame,
    dir: &Path,
    entries: &[FileEntry],
    sel: usize,
    marked: &std::collections::HashSet<PathBuf>,
) {
    let h = (entries.len() as u16 + 2).clamp(3, 22);
    let area = popup_area(f.area(), 72, h);
    f.render_widget(Clear, area);

    let items: Vec<ListItem> = entries
        .iter()
        .map(|e| {
            if e.is_dir {
                ListItem::new(Line::from(Span::styled(
                    format!("    {}/", e.name),
                    Style::default().fg(Color::Cyan),
                )))
            } else if marked.contains(&e.path) {
                ListItem::new(Line::from(Span::styled(
                    format!("[x] {}", e.name),
                    Style::default().fg(Color::Green),
                )))
            } else {
                ListItem::new(Line::from(Span::raw(format!("[ ] {}", e.name))))
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

    let mut st = ListState::default();
    st.select(if entries.is_empty() { None } else { Some(sel) });
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(Style::default().bg(Color::Blue).fg(Color::White).bold());
    f.render_stateful_widget(list, area, &mut st);
}

fn render_list(f: &mut Frame, app: &mut App, area: Rect) {
    let items: Vec<ListItem> = app
        .tunnels
        .iter()
        .map(|t| {
            let (dot, color) = if t.active {
                ("*", Color::Green)
            } else {
                ("-", Color::DarkGray)
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
            lines.push(kv("Preshared key", if p.preshared { "Yes" } else { "-" }));
            lines.push(kv("Allowed IPs", &dash(&p.allowed_ips)));
            lines.push(kv("Endpoint", &dash(&p.endpoint)));
            lines.push(kv("Keepalive", &dash(&p.keepalive)));
            lines.push(kv("Latest hs", &dash(&p.latest_handshake)));
            lines.push(kv("Transfer", &dash(&p.transfer)));
        }
    } else {
        lines.push(Line::from("No tunnels yet - press 'n' to create one."));
    }

    let p = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(title))
        .wrap(Wrap { trim: false });
    f.render_widget(p, area);
}

fn dash(s: &str) -> String {
    if s.trim().is_empty() {
        "-".to_string()
    } else {
        s.to_string()
    }
}

fn render_log(f: &mut Frame, app: &App, area: Rect) {
    let p = Paragraph::new(app.log.clone())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Log  (Up/Dn scroll) "),
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
        Span::styled("_", Style::default().fg(Color::Cyan)),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" {title}  (Enter ok / Esc x) ")),
    );
    f.render_widget(p, area);
}

fn render_help(f: &mut Frame) {
    let help = "\
  Up / k, Dn / j   Move selection (scroll the Log tab)
  Enter / a      Activate or deactivate the selected tunnel
  i              Import a tunnel (file browser; Space marks many, Enter imports)
  d              Delete the selected tunnel
  s              Toggle start-on-boot for the selected tunnel
  Q              Show the tunnel as a QR code (scan into mobile)
  y              Copy the interface public key to the clipboard (OSC 52)
  Tab            Switch between the Tunnels and Log tabs
  m              Toggle Easy / Advanced mode    r   Refresh now

  Advanced mode also adds:
  e edit in $EDITOR   n new tunnel   g generate keys   c show running config
  p save live state   R rename   x export all tunnels

  ?              This help    q / Esc   Quit";
    let area = popup_area(f.area(), 70, 23);
    f.render_widget(Clear, area);
    let title = format!(
        " wg-tui v{} | keys (press any key to close) ",
        env!("CARGO_PKG_VERSION")
    );
    let p = Paragraph::new(help).block(Block::default().borders(Borders::ALL).title(title));
    f.render_widget(p, area);
}

fn render_qr(f: &mut Frame, lines: &[Line<'static>]) {
    let need_h = lines.len() as u16 + 2;
    let need_w = lines.first().map(|l| l.width()).unwrap_or(20) as u16 + 2;
    let a = f.area();
    // A clamped QR would crop into an unscannable code - say so instead.
    if need_w > a.width || need_h > a.height {
        render_message(
            f,
            "QR too large",
            "This QR is bigger than the terminal.\nEnlarge the window (or zoom out), then press Q again.",
        );
        return;
    }
    let area = popup_area(a, need_w, need_h);
    f.render_widget(Clear, area);
    let p = Paragraph::new(lines.to_vec()).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" QR - scan in the WireGuard app (any key closes) "),
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
