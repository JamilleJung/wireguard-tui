//! A full-screen terminal UI for managing WireGuard tunnels - the same feature
//! set as the desktop client (list, live status, activate/deactivate, editor,
//! key generation, QR, export, start-on-boot), driven entirely by the keyboard.

mod backend;
mod clipboard;
mod config;
mod create;
mod doctor;
mod secrets;
mod update;
mod validation;

use std::collections::VecDeque;
use std::io;
use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt;
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
#[derive(Default)]
enum Mode {
    #[default]
    Normal,
    Help,
    /// The About box: app name, version, links, license (any key closes it).
    About,
    /// A transient info/error box (any key closes it).
    Message(String),
    /// A pre-rendered QR code (any key closes it).
    Qr(Vec<Line<'static>>),
    /// Delete confirmation for the named tunnel.
    ConfirmDelete(String),
    /// Text entry for a brand-new tunnel name.
    InputNew(String),
    /// Preset choice for a brand-new tunnel after the name is valid.
    NewPreset {
        name: String,
    },
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
    /// Backup manager: list of archives + a selection cursor.
    Backup {
        items: Vec<backend::BackupInfo>,
        sel: usize,
    },
    /// Confirm restoring every tunnel from the named backup.
    ConfirmRestore(PathBuf),
    /// Confirm deleting the named backup archive.
    ConfirmBackupDelete(PathBuf),
    /// Text entry for a live Log-tab filter (substring match).
    LogFilter(String),
    /// A scrollable full-screen document viewer (e.g. the concepts guide or the
    /// changelog). `scroll` is the top visible line; Esc/`q` closes it.
    Doc {
        title: String,
        body: &'static str,
        scroll: u16,
    },
}

#[derive(Default)]
struct App {
    tunnels: Vec<backend::Tunnel>,
    state: ListState,
    detail: Option<backend::Detail>,
    tab: usize, // 0 = Tunnels, 1 = Log
    log: String,
    log_scroll: u16,
    /// Auto-scroll the Log tab to the newest line on every refresh.
    log_follow: bool,
    /// Case-insensitive substring filter applied to the Log tab.
    log_filter: String,
    /// Show only Log lines that mention the selected tunnel.
    log_only_selected: bool,
    /// Highlighted "current line" in the Log view (index into the *filtered*
    /// lines). Drives scroll, copy-current-line, and visual selection.
    log_cursor: usize,
    /// Visual-selection anchor (Some when in visual mode). The selected range is
    /// [min(anchor,cursor) ..= max(anchor,cursor)].
    log_select_anchor: Option<usize>,
    /// Horizontal scroll for wide log lines (h/l or Left/Right).
    log_hscroll: u16,
    /// True after `c` clears the log; suppresses the poller's snapshot from
    /// overwriting the cleared placeholder until the next explicit log action.
    log_cleared: bool,
    /// Cursor line in the Detail view (index into a flat list of copyable fields).
    detail_cursor: usize,
    status: String,
    status_at: Option<Instant>,
    mode: Mode,
    quit: bool,
    /// Live throughput (bytes/sec) for the selected tunnel + the last sample it
    /// was derived from (name, rx, tx, when) so deltas reset on a new selection.
    rate_rx: u64,
    rate_tx: u64,
    prev_sample: Option<(String, u64, u64, Instant)>,
    // Shared with the background poller thread (off-UI-thread live refresh).
    poll_in: Arc<Mutex<PollIn>>,
    poll_out: Arc<Mutex<Option<Snapshot>>>,
    /// A pending newer release found by the detached startup check (if any).
    update: Option<update::UpdateInfo>,
    /// Short status line for an in-progress/just-finished self-update.
    update_status: Option<String>,
    /// Off-UI-thread mailbox for update check + apply results.
    update_out: Arc<Mutex<VecDeque<UpdateMsg>>>,
    /// Avoid overlapping diagnosis workers and their delayed popups.
    diagnosing: bool,
    /// A foreground helper mutation is running on a worker.
    operation_pending: bool,
    ready_editor: Option<EditorRequest>,
    selection_generation: u64,
    pending_selection: Option<(String, u64)>,
}

/// A message from a background worker thread, drained each UI loop.
enum UpdateMsg {
    /// A newer release is available.
    Available(update::UpdateInfo),
    /// A finished connection diagnosis report, to show in a popup.
    Diagnosed(String),
    ActionComplete(Result<String, String>),
    Present(Result<Mode, String>),
    EditorLoaded(Result<EditorRequest, String>),
    SelectWhenPresent(String, u64),
}

struct EditorRequest {
    name: String,
    original: String,
    initial: String,
    new_kind: Option<create::TunnelTemplateKind>,
}

impl App {
    fn new() -> Self {
        let mut app = Self {
            log_follow: true,
            ..Self::default()
        };
        // The poller performs the initial list/detail reads after the first draw.
        app.sync_poll_in();
        // Friendly first-run hint if something critical is missing (cheap check,
        // no sudo). Any key dismisses it.
        let blockers = doctor::critical_blockers();
        if !blockers.is_empty() {
            let mut msg = String::from("Setup needed before you can manage tunnels:\n\n");
            for b in &blockers {
                msg.push_str(&format!("  - {b}\n"));
            }
            msg.push_str("\nRun  wg-tui doctor  for a full checklist, or  wg-tui setup  to fix.");
            app.mode = Mode::Message(msg);
        }
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
            let mut prev = self.selected_name();
            if let Some((name, generation)) = &self.pending_selection {
                if *generation != self.selection_generation {
                    self.pending_selection = None;
                } else if tunnels.iter().any(|tunnel| &tunnel.name == name) {
                    prev = Some(name.clone());
                    self.pending_selection = None;
                }
            }
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
            if self
                .detail
                .as_ref()
                .is_some_and(|detail| Some(&detail.name) != self.selected_name().as_ref())
            {
                self.detail = None;
                self.detail_cursor = 0;
                self.rate_rx = 0;
                self.rate_tx = 0;
                self.prev_sample = None;
            }
        }
        // Only apply detail that still matches the current selection.
        if let Some((name, d)) = snap.detail
            && Some(&name) == self.selected_name().as_ref()
        {
            let (rx, tx) = (d.rx_bytes, d.tx_bytes);
            self.detail = Some(d);
            self.update_speed(&name, rx, tx);
        }
        if let Some(log) = snap.log
            && !self.log_cleared
            && self.log_select_anchor.is_none()
        {
            self.log = log;
        }
    }

    /// Compute live throughput (bytes/sec) from successive samples of the same
    /// tunnel; reset when the selection changes or on the first sample.
    fn update_speed(&mut self, name: &str, rx: u64, tx: u64) {
        let now = Instant::now();
        let same = self
            .prev_sample
            .as_ref()
            .map(|(n, _, _, _)| n == name)
            .unwrap_or(false);
        if same {
            let (_, prx, ptx, pt) = self.prev_sample.as_ref().unwrap();
            let (prx, ptx, pt) = (*prx, *ptx, *pt);
            let dt = now.duration_since(pt).as_secs_f64();
            if dt >= 0.3 {
                self.rate_rx = (rx.saturating_sub(prx) as f64 / dt) as u64;
                self.rate_tx = (tx.saturating_sub(ptx) as f64 / dt) as u64;
                self.prev_sample = Some((name.to_string(), rx, tx, now));
            }
        } else {
            self.rate_rx = 0;
            self.rate_tx = 0;
            self.prev_sample = Some((name.to_string(), rx, tx, now));
        }
    }

    fn selected_name(&self) -> Option<String> {
        self.state
            .selected()
            .and_then(|i| self.tunnels.get(i))
            .map(|t| t.name.clone())
    }

    fn load_detail(&mut self) {
        // Navigation must not wait for privileged helper reads. Clear stale
        // actions/keys immediately, then let the poller load this selection.
        self.detail = None;
        self.detail_cursor = 0;
        self.rate_rx = 0;
        self.rate_tx = 0;
        self.prev_sample = None;
        self.sync_poll_in();
    }

    /// Switch to the Log tab and prime it so the pane is never blank on first
    /// view: request a background load, reset cursor/selection, re-enable follow, and
    /// flip `want_log` so the poller keeps it fresh.
    fn enter_log_tab(&mut self) {
        self.tab = 1;
        if self.log.is_empty() || self.log_cleared {
            self.log = "(loading journal…)".into();
        }
        self.log_follow = true;
        self.log_scroll = 0;
        self.log_cursor = 0;
        self.log_select_anchor = None;
        self.log_cleared = false;
        self.sync_poll_in();
    }

    /// Move the Log cursor by `delta` lines, clamped; auto-pause follow when the
    /// user scrolls up, re-enable it when the cursor reaches the last line.
    fn log_move_cursor(&mut self, delta: isize) {
        let n = filtered_log_count(self);
        if n == 0 {
            self.log_cursor = 0;
            return;
        }
        let last = n - 1;
        let cur = self.log_cursor.min(last) as isize;
        self.log_cursor = (cur + delta).clamp(0, last as isize) as usize;
        // Follow only when parked on the newest line.
        self.log_follow = self.log_cursor == last;
    }

    fn log_cursor_top(&mut self) {
        self.log_cursor = 0;
        self.log_follow = false;
    }

    fn log_cursor_bottom(&mut self) {
        let n = filtered_log_count(self);
        self.log_cursor = n.saturating_sub(1);
        self.log_follow = true;
    }

    /// Drain any pending message from the update worker threads (non-blocking).
    fn drain_update(&mut self) {
        // Never discard a name/filter draft or confirmation when a delayed
        // diagnosis finishes. Keep messages queued until normal mode resumes.
        if !matches!(self.mode, Mode::Normal) {
            return;
        }
        let Some(msg) = self.update_out.lock().unwrap().pop_front() else {
            return;
        };
        match msg {
            UpdateMsg::Available(info) => {
                self.flash(format!(
                    "Update available: v{} → v{} (press u)",
                    info.current, info.latest
                ));
                self.update = Some(info);
            }
            UpdateMsg::SelectWhenPresent(name, generation) => {
                if generation == self.selection_generation {
                    self.pending_selection = Some((name, generation));
                }
            }
            UpdateMsg::Present(result) => {
                self.status.clear();
                self.status_at = None;
                self.operation_pending = false;
                self.mode = result.unwrap_or_else(Mode::Message);
            }
            UpdateMsg::EditorLoaded(result) => {
                self.status.clear();
                self.status_at = None;
                self.operation_pending = false;
                match result {
                    Ok(request) => self.ready_editor = Some(request),
                    Err(error) => self.mode = Mode::Message(error),
                }
            }
            UpdateMsg::ActionComplete(result) => {
                self.operation_pending = false;
                match result {
                    Ok(message) => self.flash(message),
                    Err(error) => self.mode = Mode::Message(error),
                }
            }
            UpdateMsg::Diagnosed(report) => {
                self.diagnosing = false;
                self.status.clear();
                self.status_at = None;
                self.mode = Mode::Message(report);
            }
        }
    }

    /// Flat list of copyable Detail fields in render order: (label, value).
    fn detail_fields(&self) -> Vec<(String, String)> {
        let mut v = Vec::new();
        let Some(d) = &self.detail else { return v };
        if !d.public_key.is_empty() {
            v.push(("Public key".into(), d.public_key.clone()));
        }
        if !d.listen_port.is_empty() {
            v.push(("Listen port".into(), d.listen_port.clone()));
        }
        if !d.addresses.is_empty() {
            v.push(("Addresses".into(), d.addresses.clone()));
        }
        if !d.dns.is_empty() {
            v.push(("DNS".into(), d.dns.clone()));
        }
        for (i, p) in d.peers.iter().enumerate() {
            let pre = format!("Peer {} ", i + 1);
            if !p.public_key.is_empty() {
                v.push((format!("{pre}public key"), p.public_key.clone()));
            }
            if !p.allowed_ips.is_empty() {
                v.push((format!("{pre}allowed IPs"), p.allowed_ips.clone()));
            }
            if !p.endpoint.is_empty() {
                v.push((format!("{pre}endpoint"), p.endpoint.clone()));
            }
        }
        v
    }

    /// Periodic refresh: live status, and the log if that tab is open. An
    /// explicit reload re-shows fresh log data, so it clears the cleared flag.
    fn tick(&mut self) {
        self.log_cleared = false;
        self.sync_poll_in();
    }

    fn move_selection(&mut self, delta: isize) {
        if self.tunnels.is_empty() {
            return;
        }
        let len = self.tunnels.len() as isize;
        let cur = self.state.selected().unwrap_or(0) as isize;
        let next = (cur + delta).rem_euclid(len) as usize;
        if next != cur as usize {
            self.selection_generation = self.selection_generation.wrapping_add(1);
            self.pending_selection = None;
        }
        self.state.select(Some(next));
        self.load_detail();
    }

    fn flash(&mut self, msg: impl Into<String>) {
        self.status = msg.into();
        self.status_at = Some(Instant::now());
    }

    /// Clear the footer status once it has been shown for `STATUS_TTL`.
    fn expire_status(&mut self) {
        if let Some(at) = self.status_at
            && at.elapsed() >= STATUS_TTL
        {
            self.status.clear();
            self.status_at = None;
        }
    }
}

/// `wg-tui doctor` - print a system checklist; exit 0 (ok) / 1 (warnings) /
/// 2 (critical missing). Read-only.
fn run_doctor() -> i32 {
    let report = doctor::system_check();
    println!("wg-tui doctor\n");
    for c in &report.checks {
        println!("  [{:<7}] {}: {}", c.status.label(), c.name, c.detail);
        if !matches!(c.status, doctor::Status::Ok)
            && let Some(fix) = &c.fix
        {
            println!("            fix: {fix}");
        }
    }
    println!();
    match report.exit_code() {
        0 => println!("All good - you're ready to import or create a tunnel."),
        1 => println!("Usable, with the warnings above. `wg-tui setup` can help."),
        _ => println!("Something critical is missing - run `wg-tui setup` or use the fixes above."),
    }
    report.exit_code()
}

/// Offer to run a privileged action after confirmation, picking a privilege
/// method that works here (root / sudo / su). Debian-minimal often has no sudo,
/// so fall back to `su` (asks for the ROOT password).
fn offer_root_action(label: &str, cmd: &str, hint: &str) -> bool {
    use std::io::Write as _;

    let method = if unsafe { libc::geteuid() } == 0 {
        "root"
    } else if doctor::can_sudo() {
        "sudo"
    } else if doctor::which("su") {
        "su"
    } else {
        "none"
    };
    if method == "none" {
        println!("{label} can't be changed automatically (neither sudo nor su is available).");
        println!("Run it as root, e.g.: {hint}");
        return false;
    }

    let shown = match method {
        "root" => format!("sh -c '{cmd}'"),
        "sudo" => format!("sudo sh -c '{cmd}'"),
        _ => format!("su root -c '{cmd}'   (asks for the ROOT password)"),
    };
    println!("{label} can be changed with:");
    println!("    {shown}");
    print!("Run now? [y/N] ");
    let _ = std::io::stdout().flush();

    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() || !line.trim().eq_ignore_ascii_case("y") {
        println!("Skipped. Run manually: {hint}");
        return false;
    }

    let safe_path = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";
    let safe_cmd = format!("export PATH={safe_path}; {cmd}");
    let status = match method {
        "root" => std::process::Command::new("sh")
            .arg("-c")
            .arg(&safe_cmd)
            .status(),
        "sudo" => std::process::Command::new("sudo")
            .arg("sh")
            .arg("-c")
            .arg(&safe_cmd)
            .status(),
        _ => std::process::Command::new("su")
            .arg("root")
            .arg("-c")
            .arg(&safe_cmd)
            .status(),
    };
    match status {
        Ok(status) if status.success() => {
            println!("Done.");
            true
        }
        _ => {
            println!("Couldn't complete the action. Run it manually: {hint}");
            false
        }
    }
}

fn offer_enable_resolved() -> bool {
    println!("The resolvectl compatibility shim needs systemd-resolved to be active.");
    offer_root_action(
        "systemd-resolved",
        doctor::enable_resolved_command(),
        &doctor::enable_resolved_hint(),
    )
}

/// `wg-tui setup` - guided, confirmation-based fix for missing requirements.
/// Never connects tunnels or enables tunnel start-on-boot.
fn run_setup() -> i32 {
    let report = doctor::system_check();
    println!("wg-tui setup\n");
    for c in &report.checks {
        println!("  [{:<7}] {}: {}", c.status.label(), c.name, c.detail);
    }
    println!();

    // Offer to install wireguard-tools if missing (with confirmation).
    let tools_missing = report
        .checks
        .iter()
        .any(|c| c.name.starts_with("WireGuard tools") && !matches!(c.status, doctor::Status::Ok));
    if tools_missing {
        if let Some(cmd) = doctor::install_tools_command() {
            println!("WireGuard tools (wg/wg-quick) are missing.");
            let _ = offer_root_action("WireGuard tools", &cmd, &doctor::install_tools_hint());
        } else {
            println!("Couldn't detect a package manager - install 'wireguard-tools' manually.");
        }
        println!();
    }

    // DNS support is optional, but a config with `DNS =` needs both a
    // `resolvconf` compatibility command and its backing service.
    match doctor::dns_setup_action(doctor::dns_resolver_state()) {
        Some(doctor::DnsSetupAction::EnableResolved) => {
            let _ = offer_enable_resolved();
            println!();
        }
        Some(doctor::DnsSetupAction::InstallProvider) => {
            println!(
                "Tunnels with a 'DNS =' line need a working resolvconf compatibility command."
            );
            if let Some(cmd) = doctor::install_resolvconf_command() {
                let installed = offer_root_action(
                    "A resolvconf provider",
                    &cmd,
                    &doctor::install_resolvconf_hint(),
                );
                if installed
                    && matches!(
                        doctor::dns_resolver_state(),
                        doctor::DnsResolverState::ResolvectlInactive
                    )
                {
                    let _ = offer_enable_resolved();
                }
            } else {
                println!("Install it manually: {}", doctor::install_resolvconf_hint());
            }
            println!();
        }
        None => {}
    }

    // Helper install can't be done safely from here - point at the installer.
    let helper_missing = report
        .checks
        .iter()
        .any(|c| c.name == "Privileged helper" && !matches!(c.status, doctor::Status::Ok));
    if helper_missing {
        println!("The privileged helper isn't installed. Install the packaged build");
        println!("(.deb / AUR / COPR) or run ./install.sh from the source tree.");
        println!();
    }

    println!("Setup never connects tunnels or enables tunnel start-on-boot - you stay in control.");
    doctor::system_check().exit_code()
}

/// User-selected configs follow the helper's 1 MiB limit. Open nonblocking
/// before checking the file type so a named pipe cannot hold an operation open.
fn read_config_file(path: &std::path::Path) -> Result<String, String> {
    use std::io::Read as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    const LIMIT: u64 = 1024 * 1024;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .map_err(|error| error.to_string())?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Err("Config must be a regular file".into());
    }
    if metadata.len() > LIMIT {
        return Err("Config exceeds the 1 MiB limit".into());
    }
    let mut config = String::new();
    file.take(LIMIT + 1)
        .read_to_string(&mut config)
        .map_err(|error| error.to_string())?;
    if config.len() as u64 > LIMIT {
        return Err("Config exceeds the 1 MiB limit".into());
    }
    Ok(config)
}

fn main() -> io::Result<()> {
    // Handle --version/--help before touching the terminal, so they work in
    // scripts and don't accidentally launch the full-screen UI.
    if let Some(arg) = std::env::args().nth(1) {
        match arg.as_str() {
            "-V" | "--version" => {
                println!("wg-tui {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "-h" | "--help" => {
                println!(
                    "wg-tui {} - a terminal UI for managing WireGuard tunnels\n\n\
                     Usage: wg-tui            launch the interactive UI\n       \
                     wg-tui doctor       check your system and print a checklist\n       \
                     wg-tui setup        guided fix for anything missing\n       \
                     wg-tui --version    print the version\n       \
                     wg-tui --help       show this help\n\n\
                     Inside the app, press '?' for the full key map.",
                    env!("CARGO_PKG_VERSION")
                );
                return Ok(());
            }
            "doctor" => std::process::exit(run_doctor()),
            "setup" => std::process::exit(run_setup()),
            other => {
                eprintln!("wg-tui: unknown argument '{other}' (try --help)");
                std::process::exit(2);
            }
        }
    }
    backend::init();
    let mut terminal = ratatui::init();
    let res = run(&mut terminal);
    ratatui::restore();
    res
}

fn run(terminal: &mut ratatui::DefaultTerminal) -> io::Result<()> {
    let mut app = App::new();
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

        if event::poll(timeout)?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            handle_key(&mut app, terminal, key.code, key.modifiers)?;
        }
    }
    Ok(())
}

/// Background thread: does the blocking `wg`/`sudo` calls so the UI never
/// freezes, dropping the result into `poll_out` for the UI loop to apply.
fn spawn_poller(poll_in: Arc<Mutex<PollIn>>, poll_out: Arc<Mutex<Option<Snapshot>>>) {
    std::thread::spawn(move || {
        loop {
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
        }
    });
}

/// Trim the dev-facing Keep-a-Changelog header/intro and the (usually empty)
/// `[Unreleased]` section for the in-app "What's new" view — start at the first
/// `## [x.y.z]` heading. Returns a sub-slice, so the body stays `&'static str`.
fn changelog_for_display(src: &'static str) -> &'static str {
    let mut offset = 0;
    for line in src.lines() {
        let t = line.trim_start();
        if t.starts_with("## [") && t.as_bytes().get(4).is_some_and(u8::is_ascii_digit) {
            return &src[offset..];
        }
        offset += line.len() + 1; // +1 for the '\n' that `lines()` stripped
    }
    src
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
        Mode::Help | Mode::About | Mode::Message(_) | Mode::Qr(_) => {
            app.mode = Mode::Normal;
            return Ok(());
        }
        Mode::Doc { body, scroll, .. } => {
            // The paragraph wraps at the current viewport width. Counting raw
            // source lines makes the tail unreachable on narrow terminals.
            let max = doc_max_scroll(body, terminal.get_frame().area());
            match code {
                KeyCode::Esc | KeyCode::Char('q') => app.mode = Mode::Normal,
                KeyCode::Down | KeyCode::Char('j') => *scroll = scroll.saturating_add(1).min(max),
                KeyCode::Up | KeyCode::Char('k') => *scroll = scroll.saturating_sub(1),
                KeyCode::PageDown => *scroll = scroll.saturating_add(15).min(max),
                KeyCode::PageUp => *scroll = scroll.saturating_sub(15),
                KeyCode::Home | KeyCode::Char('g') => *scroll = 0,
                KeyCode::End | KeyCode::Char('G') => *scroll = max,
                _ => {}
            }
            return Ok(());
        }
        Mode::ConfirmDelete(name) => {
            let name = name.clone();
            match code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    app.mode = Mode::Normal;
                    start_action(app, format!("Deleting {name}…"), move || {
                        backend::delete(&name)
                            .map_err(|error| format!("Delete failed: {error}"))?;
                        Ok(format!("Deleted {name}"))
                    });
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
                    let name = buf.trim().to_string();
                    if let Err(e) = backend::validate_tunnel_name(&name) {
                        app.mode = Mode::Message(format!("Invalid tunnel name:\n{e}"));
                    } else {
                        app.mode = Mode::NewPreset { name };
                    }
                }
                KeyCode::Char(c) if buf.len() < 15 => buf.push(c),
                _ => {}
            }
            return Ok(());
        }
        Mode::NewPreset { name } => {
            let name = name.clone();
            match code {
                KeyCode::Esc => {
                    app.mode = Mode::Normal;
                    app.flash("New tunnel cancelled");
                }
                KeyCode::Enter | KeyCode::Char('f') | KeyCode::Char('F') => {
                    app.mode = Mode::Normal;
                    create_tunnel(
                        app,
                        terminal,
                        &name,
                        create::TunnelTemplateKind::ClientFullTunnel,
                    )?;
                }
                KeyCode::Char('i') | KeyCode::Char('I') => {
                    app.mode = Mode::Normal;
                    create_tunnel(
                        app,
                        terminal,
                        &name,
                        create::TunnelTemplateKind::InterfaceOnly,
                    )?;
                }
                KeyCode::Char('s') | KeyCode::Char('S') => {
                    app.mode = Mode::Normal;
                    create_tunnel(
                        app,
                        terminal,
                        &name,
                        create::TunnelTemplateKind::ClientSplitTunnel,
                    )?;
                }
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
                    let new = buf.trim().to_string();
                    app.mode = Mode::Normal;
                    rename_tunnel(app, &orig, &new);
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
                    if let Some(e) = entries.get(*sel)
                        && !e.is_dir
                        && !marked.remove(&e.path)
                    {
                        marked.insert(e.path.clone());
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
        Mode::Backup { items, sel } => {
            // Navigation stays inside the borrow; actions clone first, then act.
            match code {
                KeyCode::Up | KeyCode::Char('k') => {
                    *sel = sel.saturating_sub(1);
                    return Ok(());
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if *sel + 1 < items.len() {
                        *sel += 1;
                    }
                    return Ok(());
                }
                KeyCode::Home | KeyCode::Char('g') => {
                    *sel = 0;
                    return Ok(());
                }
                KeyCode::End | KeyCode::Char('G') => {
                    *sel = items.len().saturating_sub(1);
                    return Ok(());
                }
                _ => {}
            }
            let selected = items.get(*sel).map(|b| (b.path.clone(), b.name.clone()));
            match code {
                KeyCode::Esc | KeyCode::Char('q') => app.mode = Mode::Normal,
                KeyCode::Char('n') => {
                    app.mode = Mode::Normal;
                    start_action(app, "Creating backup…".into(), || {
                        let info = backend::create_backup()
                            .map_err(|error| format!("Backup failed: {error}"))?;
                        Ok(format!(
                            "Backed up {} tunnel(s) -> {} (B to reopen backups)",
                            info.count, info.name
                        ))
                    });
                }
                KeyCode::Enter | KeyCode::Char('r') => {
                    if let Some((path, _)) = selected {
                        app.mode = Mode::ConfirmRestore(path);
                    }
                }
                KeyCode::Char('d') => {
                    if let Some((path, _)) = selected {
                        app.mode = Mode::ConfirmBackupDelete(path);
                    }
                }
                KeyCode::Char('x') => {
                    if let Some((path, name)) = selected {
                        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                        let dest = std::path::Path::new(&home).join(&name);
                        app.mode = Mode::Normal;
                        start_action(app, "Exporting backup…".into(), move || {
                            backend::export_backup_to(&path, &dest)
                                .map_err(|error| format!("Export failed: {error}"))?;
                            Ok(format!("Exported to {}", dest.display()))
                        });
                    }
                }
                _ => {}
            }
            return Ok(());
        }
        Mode::ConfirmRestore(path) => {
            let path = path.clone();
            match code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    app.mode = Mode::Normal;
                    start_action(app, "Restoring backup…".into(), move || {
                        let count = backend::restore_backup(&path)
                            .map_err(|error| format!("Restore failed: {error}"))?;
                        Ok(format!("Restored {count} tunnel(s) from backup"))
                    });
                }
                _ => {
                    app.mode = Mode::Normal;
                    app.flash("Restore cancelled");
                }
            }
            return Ok(());
        }
        Mode::ConfirmBackupDelete(path) => {
            let path = path.clone();
            match code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    app.mode = Mode::Normal;
                    start_action(app, "Deleting backup…".into(), move || {
                        backend::delete_backup(&path)
                            .map_err(|error| format!("Delete failed: {error}"))?;
                        Ok("Backup deleted (B to reopen backups)".into())
                    });
                }
                _ => {
                    app.mode = Mode::Normal;
                    app.flash("Delete cancelled");
                }
            }
            return Ok(());
        }
        Mode::LogFilter(buf) => {
            match code {
                KeyCode::Esc => app.mode = Mode::Normal,
                KeyCode::Enter => {
                    let v = buf.trim().to_string();
                    app.log_filter = v;
                    app.log_cursor = 0;
                    app.log_select_anchor = None;
                    app.log_cleared = false;
                    app.mode = Mode::Normal;
                }
                KeyCode::Backspace => {
                    buf.pop();
                }
                KeyCode::Char(c) => buf.push(c),
                _ => {}
            }
            return Ok(());
        }
        Mode::Normal => {}
    }

    if app.operation_pending
        && (matches!(
            code,
            KeyCode::Char(
                'a' | 'e' | 'n' | 'd' | 'R' | 's' | 'K' | '+' | 'i' | 'p' | 'x' | 'u' | 'B' | 'Q'
            )
        ) || (app.tab == 0 && matches!(code, KeyCode::Enter | KeyCode::Char('g' | 'c'))))
    {
        app.flash("Another operation is still running");
        return Ok(());
    }

    // Normal-mode keys.
    match code {
        // Esc cancels a Log visual selection instead of quitting (must precede q/Esc).
        KeyCode::Esc if app.tab == 1 && app.log_select_anchor.is_some() => {
            app.log_select_anchor = None;
            app.flash("Selection cancelled");
        }
        KeyCode::Char('q') | KeyCode::Esc => app.quit = true,
        // --- Copy (context-aware) ---
        KeyCode::Char('y') => {
            if app.tab == 1 {
                copy_log_current_or_selection(app);
            } else {
                // Tunnels tab: copy the focused Detail field.
                copy_detail_field(app);
            }
        }
        KeyCode::Char('v') if app.tab == 1 => {
            // Toggle visual selection mode, anchored at the cursor.
            if app.log_select_anchor.take().is_none() {
                app.log_select_anchor = Some(app.log_cursor);
                app.flash("Visual select: move to extend, y to copy, Esc to cancel");
            } else {
                app.flash("Selection cleared");
            }
        }
        KeyCode::Char('Y') if app.tab == 1 => {
            // Copy the whole filtered log.
            let lines = filtered_log_text_lines(app);
            let n = lines.len();
            clipboard::copy_text(&lines.join("\n"));
            app.flash(format!("Copied {n} log line(s)"));
        }
        KeyCode::Tab | KeyCode::BackTab => {
            if app.tab == 0 {
                app.enter_log_tab();
            } else {
                app.tab = 0;
                app.sync_poll_in();
            }
        }
        // Enter copies on the Log tab; on the Tunnels tab it still toggles (below).
        KeyCode::Enter if app.tab == 1 => copy_log_current_or_selection(app),
        KeyCode::Down | KeyCode::Char('j') => {
            if app.tab == 1 {
                app.log_move_cursor(1);
            } else {
                app.move_selection(1);
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if app.tab == 1 {
                app.log_move_cursor(-1);
            } else {
                app.move_selection(-1);
            }
        }
        // ---- Log-tab cursor navigation (only while the Log tab is open) ----
        KeyCode::PageDown if app.tab == 1 => app.log_move_cursor(10),
        KeyCode::PageUp if app.tab == 1 => app.log_move_cursor(-10),
        KeyCode::Home if app.tab == 1 => app.log_cursor_top(),
        KeyCode::End if app.tab == 1 => app.log_cursor_bottom(),
        KeyCode::Char('g') if app.tab == 1 => app.log_cursor_top(), // vim top
        KeyCode::Char('G') if app.tab == 1 => app.log_cursor_bottom(), // vim bottom
        // Horizontal scroll for wide log lines.
        KeyCode::Left | KeyCode::Char('h') if app.tab == 1 => {
            app.log_hscroll = app.log_hscroll.saturating_sub(8);
        }
        KeyCode::Right | KeyCode::Char('l') if app.tab == 1 => {
            app.log_hscroll = app.log_hscroll.saturating_add(8);
        }
        // ---- Detail field navigation (only while the Tunnels tab is open) ----
        KeyCode::Right | KeyCode::Char('l') if app.tab == 0 => {
            let n = app.detail_fields().len();
            if n > 0 {
                app.detail_cursor = (app.detail_cursor + 1) % n;
            }
        }
        KeyCode::Left | KeyCode::Char('h') if app.tab == 0 => {
            let n = app.detail_fields().len();
            if n > 0 {
                app.detail_cursor = (app.detail_cursor + n - 1) % n;
            }
        }
        KeyCode::Char('f') if app.tab == 1 => {
            app.log_follow = !app.log_follow;
            app.log_cleared = false;
            app.flash(if app.log_follow {
                "Log: following newest"
            } else {
                "Log: paused (End to follow again)"
            });
        }
        KeyCode::Char('t') if app.tab == 1 => {
            app.log_only_selected = !app.log_only_selected;
            app.log_cleared = false;
            app.flash(if app.log_only_selected {
                "Log: showing the selected tunnel only"
            } else {
                "Log: showing all entries"
            });
        }
        KeyCode::Char('/') if app.tab == 1 => app.mode = Mode::LogFilter(app.log_filter.clone()),
        KeyCode::Char('w') if app.tab == 1 => save_log(app),
        KeyCode::Char('c') if app.tab == 1 => {
            app.log = "(cleared — press r to reload from the journal)".into();
            app.log_scroll = 0;
            app.log_follow = false;
            app.log_cleared = true;
        }
        // Backup manager (works from either tab).
        KeyCode::Char('B') => {
            start_presentation(app, "Reading backups…".into(), || {
                Ok(Mode::Backup {
                    items: backend::list_backups(),
                    sel: 0,
                })
            });
        }
        KeyCode::Enter | KeyCode::Char('a') => toggle_active(app),
        KeyCode::Char('e') => request_editor(app, false),
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
        KeyCode::Char('K') => toggle_killswitch(app),
        KeyCode::Char('+') => request_editor(app, true),
        KeyCode::Char('i') => open_import_browser(app),
        KeyCode::Char('g') => generate_show(app),
        KeyCode::Char('c') => show_running(app),
        KeyCode::Char('p') => persist_live(app),
        KeyCode::Char('x') => export(app),
        KeyCode::Char('Q') => show_qr(app),
        KeyCode::Char('r') => {
            app.tick();
            app.flash("Refresh requested");
        }
        KeyCode::Char('u') => self_update(app, terminal)?,
        KeyCode::Char('D') => diagnose(app),
        KeyCode::Char('?') => app.mode = Mode::Help,
        KeyCode::Char('A') => app.mode = Mode::About,
        // Scrollable "WireGuard explained" concepts guide.
        KeyCode::Char('H') => {
            app.mode = Mode::Doc {
                title: "WireGuard explained".into(),
                body: include_str!("../docs/help.md"),
                scroll: 0,
            };
        }
        // Scrollable changelog ("what's new"). On the Log tab `w` saves the log
        // (handled above), so this only fires on the Tunnels tab.
        KeyCode::Char('w') => {
            app.mode = Mode::Doc {
                title: "What's new".into(),
                body: changelog_for_display(include_str!("../CHANGELOG.md")),
                scroll: 0,
            };
        }
        _ => {}
    }
    Ok(())
}

/// Run a connection diagnosis for the selected tunnel off the UI thread (it
/// pings/resolves and can take a few seconds), delivering the report via the
/// shared mailbox so the loop can pop it into a Message popup when it's ready.
fn diagnose(app: &mut App) {
    if app.diagnosing {
        app.flash("Diagnosis already in progress");
        return;
    }
    let Some(name) = app.selected_name() else {
        app.flash("Select a tunnel to diagnose");
        return;
    };
    app.diagnosing = true;
    app.flash(format!("Diagnosing {name}…"));
    let out = app.update_out.clone();
    std::thread::spawn(move || {
        let report = backend::diagnose_report(&name);
        out.lock().unwrap().push_back(UpdateMsg::Diagnosed(report));
    });
}

/// Download + verify + install the pending update with the TUI suspended.
///
/// Running `install.sh` (which may prompt for a sudo/su password) while ratatui
/// owns the raw-mode alternate screen corrupts the terminal and deadlocks on the
/// hidden prompt, so - exactly like `run_editor` - we drop back to the cooked
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

fn activation_failure_message(name: &str, raw: &str) -> String {
    format!(
        "Couldn't activate {name}:\n\n{}",
        doctor::friendly_error(raw)
    )
}

/// Keep the event loop alive while a helper may authenticate or wait for
/// wg-quick. Modal input and selection stay on the UI thread; results queue.
fn start_action(
    app: &mut App,
    message: String,
    work: impl FnOnce() -> Result<String, String> + Send + 'static,
) -> bool {
    if app.operation_pending {
        app.flash("Another operation is still running");
        return false;
    }
    app.operation_pending = true;
    app.flash(message);
    app.status_at = None;
    let out = app.update_out.clone();
    std::thread::spawn(move || {
        let result = work();
        out.lock()
            .unwrap()
            .push_back(UpdateMsg::ActionComplete(result));
    });
    true
}

fn start_selected_action(
    app: &mut App,
    message: String,
    work: impl FnOnce() -> Result<(String, String), String> + Send + 'static,
) {
    let generation = app.selection_generation;
    let out = app.update_out.clone();
    start_action(app, message, move || {
        let (message, name) = work()?;
        out.lock()
            .unwrap()
            .push_back(UpdateMsg::SelectWhenPresent(name, generation));
        Ok(message)
    });
}

fn start_presentation(
    app: &mut App,
    message: String,
    work: impl FnOnce() -> Result<Mode, String> + Send + 'static,
) {
    if app.operation_pending {
        app.flash("Another operation is still running");
        return;
    }
    app.operation_pending = true;
    app.flash(message);
    app.status_at = None;
    let out = app.update_out.clone();
    std::thread::spawn(move || {
        let result = work();
        out.lock().unwrap().push_back(UpdateMsg::Present(result));
    });
}

fn toggle_active(app: &mut App) {
    let Some(tunnel) = app.state.selected().and_then(|idx| app.tunnels.get(idx)) else {
        return;
    };
    let (name, active) = (tunnel.name.clone(), tunnel.active);
    start_action(
        app,
        format!(
            "{} {name}…",
            if active { "Deactivating" } else { "Activating" }
        ),
        move || {
            let result = if active {
                backend::deactivate(&name)
            } else {
                backend::activate(&name)
            };
            result.map_err(|error| activation_failure_message(&name, &error))?;
            if !active && doctor::clock_synced() == Some(false) {
                Ok(format!(
                    "Activated {name} — clock not NTP-synced; handshake may be rejected (press D to diagnose)"
                ))
            } else {
                Ok(format!(
                    "{} {name}",
                    if active { "Deactivated" } else { "Activated" }
                ))
            }
        },
    );
}

fn toggle_autostart(app: &mut App) {
    let Some(detail) = &app.detail else { return };
    let (name, want) = (detail.name.clone(), !detail.autostart);
    start_action(
        app,
        format!("Updating start-on-boot for {name}…"),
        move || {
            backend::set_autostart(&name, want)?;
            Ok(format!(
                "Start on boot {} for {name}",
                if want { "enabled" } else { "disabled" }
            ))
        },
    );
}

fn toggle_killswitch(app: &mut App) {
    let Some(detail) = &app.detail else { return };
    if !detail.active {
        app.flash("Kill switch needs an active tunnel");
        return;
    }
    let (name, want) = (detail.name.clone(), !detail.killswitch);
    start_action(
        app,
        format!("Updating kill switch for {name}…"),
        move || {
            backend::set_killswitch(&name, want)
                .map_err(|error| format!("Kill switch failed:\n\n{error}"))?;
            Ok(format!(
                "Kill switch {} for {name}",
                if want { "enabled" } else { "disabled" }
            ))
        },
    );
}

fn export(app: &mut App) {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let dest = std::path::Path::new(&home).join("wireguard-tunnels.zip");
    start_action(app, "Exporting tunnels…".into(), move || {
        let count =
            backend::export_zip(&dest).map_err(|error| format!("Export failed: {error}"))?;
        Ok(format!("Exported {count} tunnel(s) to {}", dest.display()))
    });
}

fn generate_show(app: &mut App) {
    start_presentation(app, "Generating keys…".into(), || {
        let (private, public) = backend::generate_keypair()?;
        let preshared = backend::generate_psk()?;
        Ok(Mode::Message(format!(
            "Private key:\n{private}\n\nPublic key:\n{public}\n\nPreshared key:\n{preshared}\n\nKeep private and preshared keys secret. Any key closes."
        )))
    });
}

fn show_running(app: &mut App) {
    let Some(detail) = &app.detail else { return };
    if !detail.active {
        app.flash("Tunnel is not active");
        return;
    }
    let name = detail.name.clone();
    start_presentation(
        app,
        format!("Reading running config of {name}…"),
        move || {
            let config = backend::running_config(&name)
                .map_err(|error| format!("showconf failed: {error}"))?;
            Ok(Mode::Message(format!(
                "Running config - {name}\n\n{}",
                config.trim_end()
            )))
        },
    );
}

fn persist_live(app: &mut App) {
    let Some(detail) = &app.detail else { return };
    if !detail.active {
        app.flash("Tunnel is not active");
        return;
    }
    let name = detail.name.clone();
    start_action(app, format!("Saving live state of {name}…"), move || {
        backend::persist_live(&name).map_err(|error| format!("Save-live failed: {error}"))?;
        Ok(format!("Saved live state of {name} to its .conf"))
    });
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
    import_files(app, &[path.to_path_buf()]);
}

fn import_files(app: &mut App, paths: &[PathBuf]) {
    let paths = paths.to_vec();
    start_selected_action(
        app,
        format!("Importing {} file(s)…", paths.len()),
        move || {
            let (mut count, mut skipped) = (0, 0);
            let mut first_error = None;
            let mut last = None;
            for path in paths {
                match import_one_file(&path) {
                    Ok(name) => {
                        count += 1;
                        last = Some(name);
                    }
                    Err(error) => {
                        skipped += 1;
                        first_error.get_or_insert(error);
                    }
                }
            }
            if count == 0 {
                return Err(format!(
                    "Nothing imported ({skipped} skipped): {}",
                    first_error.unwrap_or_default()
                ));
            }
            Ok((
                format!("Imported {count} tunnel(s), {skipped} skipped"),
                last.unwrap(),
            ))
        },
    );
}

fn import_one_file(path: &Path) -> Result<String, String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let content = if matches!(ext.as_str(), "png" | "jpg" | "jpeg") {
        backend::decode_qr(path)?
    } else {
        read_config_file(path)?
    };
    backend::validate_config(&content)?;
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("imported");
    let name = backend::unique_name(stem)?;
    backend::save_config(&name, &content)?;
    Ok(name)
}

fn show_qr(app: &mut App) {
    let Some(name) = app.selected_name() else {
        return;
    };
    start_presentation(app, format!("Preparing QR for {name}…"), move || {
        let config = backend::read_config(&name)?;
        Ok(Mode::Qr(qr_lines(&config)?))
    });
}

fn request_editor(app: &mut App, add_peer: bool) {
    let Some(name) = app.selected_name() else {
        return;
    };
    if app.operation_pending {
        app.flash("Another operation is still running");
        return;
    }
    app.operation_pending = true;
    app.flash(format!("Reading {name} for editor…"));
    app.status_at = None;
    let out = app.update_out.clone();
    std::thread::spawn(move || {
        let result = backend::read_config(&name).map(|original| {
            let initial = if add_peer {
                format!("{}\n\n[Peer]\n# PublicKey = \n# AllowedIPs = \n# Endpoint = \n# PersistentKeepalive = 25\n", original.trim_end())
            } else { original.clone() };
            EditorRequest { name, original, initial, new_kind: None }
        });
        out.lock()
            .unwrap()
            .push_back(UpdateMsg::EditorLoaded(result));
    });
}

fn edit_loaded_tunnel(
    app: &mut App,
    terminal: &mut ratatui::DefaultTerminal,
    request: EditorRequest,
) -> io::Result<()> {
    let EditorRequest {
        name,
        original,
        initial,
        new_kind,
    } = request;
    let edited = match run_editor(terminal, &initial) {
        Ok(Some(edited)) => edited,
        Ok(None) => {
            app.flash("Edit cancelled");
            return Ok(());
        }
        Err(error) => {
            app.mode = Mode::Message(format!("Editor failed: {error}"));
            return Ok(());
        }
    };
    if edited == original {
        app.flash("No changes");
        return Ok(());
    }
    if let Err(error) = backend::validate_config(&edited) {
        app.mode = Mode::Message(format!("Not saved - invalid config:\n{error}"));
        return Ok(());
    }
    if let Some(kind) = new_kind {
        start_selected_action(app, format!("Creating {name}…"), move || {
            if backend::tunnel_exists(&name)? {
                return Err(format!("A tunnel named '{name}' already exists."));
            }
            backend::save_config(&name, &edited)?;
            Ok((format!("Created {name} from {}", kind.label()), name))
        });
        return Ok(());
    }
    start_action(app, format!("Saving {name}…"), move || {
        let active = backend::try_list_tunnels()?
            .iter()
            .any(|t| t.name == name && t.active);
        backend::save_config(&name, &edited)?;
        if active {
            match backend::sync_running(&name) {
                Ok(()) => Ok(format!(
                    "Saved {name} (WireGuard settings applied live; reconnect for Address/DNS/MTU/routes)"
                )),
                Err(error) => Ok(format!(
                    "Saved {name}; live apply failed: {error}. Reconnect to apply changes"
                )),
            }
        } else {
            Ok(format!("Saved {name}"))
        }
    });
    Ok(())
}

/// Create a brand-new tunnel: a generated template, opened in $EDITOR.
fn create_tunnel(
    app: &mut App,
    _terminal: &mut ratatui::DefaultTerminal,
    name: &str,
    kind: create::TunnelTemplateKind,
) -> io::Result<()> {
    if app.operation_pending {
        app.flash("Another operation is still running");
        return Ok(());
    }
    app.operation_pending = true;
    app.flash("Generating tunnel keys…");
    app.status_at = None;
    let name = name.to_string();
    let out = app.update_out.clone();
    std::thread::spawn(move || {
        let result = backend::generate_keypair().map(|(private, _)| EditorRequest {
            name,
            original: String::new(),
            initial: create::generate_template(kind, &private),
            new_kind: Some(kind),
        });
        out.lock()
            .unwrap()
            .push_back(UpdateMsg::EditorLoaded(result));
    });
    Ok(())
}

fn rename_tunnel(app: &mut App, orig: &str, new: &str) {
    if let Err(error) = backend::validate_tunnel_name(new) {
        app.mode = Mode::Message(format!("Invalid tunnel name:\n{error}"));
        return;
    }
    if new == orig {
        return;
    }
    let (orig, new) = (orig.to_string(), new.to_string());
    start_selected_action(app, format!("Renaming {orig}…"), move || {
        if backend::tunnel_exists(&new)? {
            return Err(format!("A tunnel named '{new}' already exists."));
        }
        let config = backend::read_config(&orig)?;
        backend::rename_config(&orig, &new, &config)?;
        Ok((format!("Renamed {orig} -> {new}"), new))
    });
}

/// A private editor workspace with no predictable shared-directory or symlink
/// reuse. Cleanup includes editor-created swap/backup files and runs on errors.
struct EditorDraft {
    directory: PathBuf,
    path: PathBuf,
}

impl EditorDraft {
    fn new(initial: &str) -> io::Result<Self> {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let template = std::env::temp_dir().join("wg-tui-editor-XXXXXX");
        let mut name = std::ffi::CString::new(template.as_os_str().as_bytes())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?
            .into_bytes_with_nul();
        if unsafe { libc::mkdtemp(name.as_mut_ptr().cast()) }.is_null() {
            return Err(io::Error::last_os_error());
        }
        name.pop();
        let directory = PathBuf::from(std::ffi::OsString::from_vec(name));
        let draft = Self {
            path: directory.join("tunnel.conf"),
            directory,
        };
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&draft.path)?;
        file.write_all(initial.as_bytes())?;
        Ok(draft)
    }
}

impl Drop for EditorDraft {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

fn editor_argv(editor: &str) -> io::Result<Vec<String>> {
    let parts = shell_words::split(editor).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Invalid VISUAL/EDITOR quoting: {error}"),
        )
    })?;
    if parts.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "VISUAL/EDITOR is empty",
        ));
    }
    Ok(parts)
}

/// Resume the existing terminal without installing another panic hook. Calling
/// `ratatui::init()` for each editor/update would repeatedly wrap the old hook.
fn resume_terminal(terminal: &mut ratatui::DefaultTerminal) -> io::Result<()> {
    crossterm::terminal::enable_raw_mode()?;
    if let Err(error) = crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::EnterAlternateScreen
    ) {
        let _ = crossterm::terminal::disable_raw_mode();
        return Err(error);
    }
    terminal.autoresize()?;
    terminal.clear()
}

/// Suspend the TUI, run `$EDITOR` on a temp copy of `initial`, return the edited
/// text (None if the editor failed to launch). The temp file is mode 0600 and
/// removed afterwards.
fn run_editor(
    terminal: &mut ratatui::DefaultTerminal,
    initial: &str,
) -> io::Result<Option<String>> {
    let draft = EditorDraft::new(initial)?;
    let path = &draft.path;

    let editor = std::env::var("VISUAL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            std::env::var("EDITOR")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .unwrap_or_else(|| "nano".to_string());
    // Parse before suspending the terminal; malformed quoting is a useful
    // input error, not a failed shell launch. No shell expansion is performed.
    let parts = editor_argv(&editor)?;
    let (prog, args) = parts.split_first().unwrap();

    ratatui::restore();
    let status = Command::new(prog).args(args).arg(path).status();
    resume_terminal(terminal)?;

    match status {
        Ok(status) if status.success() => {
            read_config_file(path).map(Some).map_err(io::Error::other)
        }
        _ => Ok(None),
    }
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

    // Footer: status if set, else key hints. The footer shows the everyday keys
    // (with a compact fallback on narrow terminals) and points at `?` for the
    // full, sectioned key map. Every key always works - there is no mode.
    let hint = if app.tab == 1 {
        // The Log tab has its own keys; show them instead of the tunnel actions.
        let full = " j/k move  g/G top/bot  PgUp/Dn  h/l ←→  f follow  t tunnel  / filter  y copy  v select  Y copy-all  w save  c clear  r reload  Tab tunnels  ? help  q quit";
        let compact = " j/k move  f follow  / filter  y copy  v select  Y all  w save  Tab tunnels  ? help  q quit";
        if full.chars().count() as u16 <= chunks[2].width {
            full
        } else {
            compact
        }
    } else {
        let full = " Up/Dn move  Enter/a on/off  n new  i import  d remove  s on-boot  Q qr  h/l field  y copy  D diagnose  B backup  Tab log  H explained  A about  …  ? all keys";
        let compact = " Up/Dn move  Enter on/off  n new  i import  d remove  Q qr  y copy  D diagnose  Tab log  …  ? all keys";
        if full.chars().count() as u16 <= chunks[2].width {
            full
        } else {
            compact
        }
    };
    // A tunnel that's up but has no recent handshake is "active but not actually
    // connected" - surface a hint pointing at the diagnose key (Tunnels tab only).
    let stuck_handshake = app.tab == 0
        && app
            .detail
            .as_ref()
            .is_some_and(|d| d.active && d.handshake_age.is_none_or(|a| a > 180));
    // Priority: in-flight update status > transient status > update banner >
    // stuck-handshake hint > hints.
    let footer = if let Some(s) = &app.update_status {
        Line::from(vec![Span::styled(
            format!(" {s}"),
            Style::default().fg(Color::Green).bold(),
        )])
    } else if !app.status.is_empty() {
        Line::from(vec![Span::styled(
            format!(" {}", app.status),
            Style::default().fg(Color::Yellow),
        )])
    } else if let Some(info) = &app.update {
        Line::from(vec![Span::styled(
            format!(" update available v{} (press u)", info.latest),
            Style::default().fg(Color::Green).bold(),
        )])
    } else if stuck_handshake {
        Line::from(vec![Span::styled(
            " no handshake — press D to diagnose",
            Style::default().fg(Color::Red).bold(),
        )])
    } else {
        Line::from(vec![Span::styled(
            hint,
            Style::default().fg(Color::DarkGray),
        )])
    };
    f.render_widget(Paragraph::new(footer), chunks[2]);

    // Popups.
    match &app.mode {
        Mode::Help => render_help(f),
        Mode::About => render_about(f),
        Mode::Message(m) => render_message(f, "Notice", m),
        Mode::ConfirmDelete(name) => render_message(
            f,
            "Confirm delete",
            &format!("Delete tunnel '{name}'?\n\n[y] yes    [n] no"),
        ),
        Mode::InputNew(buf) => render_input(f, "New tunnel name", buf),
        Mode::NewPreset { name } => render_message(
            f,
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
        Mode::InputRename { buf, .. } => render_input(f, "Rename tunnel", buf),
        Mode::ImportBrowse {
            dir,
            entries,
            sel,
            marked,
        } => render_browse(f, dir, entries, *sel, marked),
        Mode::Qr(lines) => render_qr(f, lines),
        Mode::Backup { items, sel } => render_backup(f, items, *sel),
        Mode::ConfirmRestore(path) => render_message(
            f,
            "Confirm restore",
            &format!(
                "Restore all tunnels from\n  {}\n\nExisting tunnels with the same name are overwritten.\n\n[y] yes    [n] no",
                path.file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default()
            ),
        ),
        Mode::ConfirmBackupDelete(path) => render_message(
            f,
            "Delete backup",
            &format!(
                "Delete the backup\n  {}\n\n[y] yes    [n] no",
                path.file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default()
            ),
        ),
        Mode::LogFilter(buf) => render_input(f, "Filter log (substring)", buf),
        Mode::Doc {
            title,
            body,
            scroll,
        } => render_doc(f, title, body, *scroll),
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
    let h = entries.len().saturating_add(2).clamp(3, 22) as u16;
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

    // Copyable-field index; stays in lockstep with `App::detail_fields()` so the
    // highlighted row is exactly the one `y` will copy.
    let mut fi = 0usize;
    let mut push_field = |lines: &mut Vec<Line>, label: &str, value: &str| {
        let mut line = kv(label, value);
        if fi == app.detail_cursor {
            line = line.style(Style::default().bg(Color::Blue));
        }
        fi += 1;
        lines.push(line);
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
        if d.active {
            // Connection health from the most recent handshake.
            let (htxt, hc) = match d.handshake_age {
                Some(s) if s < 180 => (
                    format!("OK (last handshake {} ago)", fmt_ago(s)),
                    Color::Green,
                ),
                Some(s) => (
                    format!("stale (last handshake {} ago)", fmt_ago(s)),
                    Color::Yellow,
                ),
                None => ("waiting for handshake...".to_string(), Color::Yellow),
            };
            lines.push(Line::from(vec![
                Span::styled("  Connection    ", Style::default().fg(Color::DarkGray)),
                Span::styled(htxt, Style::default().fg(hc)),
            ]));
            lines.push(kv(
                "Speed",
                &format!(
                    "↓ {}/s   ↑ {}/s",
                    backend::fmt_bytes(app.rate_rx),
                    backend::fmt_bytes(app.rate_tx)
                ),
            ));
        }
        // Copyable interface fields (highlighted when focused). Non-empty ones
        // mirror `detail_fields()` exactly via `push_field`; empty ones render as
        // a plain dash and are not in the copyable index.
        if d.public_key.is_empty() {
            lines.push(kv("Public key", "-"));
        } else {
            push_field(&mut lines, "Public key", &d.public_key);
        }
        if d.listen_port.is_empty() {
            lines.push(kv("Listen port", "-"));
        } else {
            push_field(&mut lines, "Listen port", &d.listen_port);
        }
        if d.addresses.is_empty() {
            lines.push(kv("Addresses", "-"));
        } else {
            push_field(&mut lines, "Addresses", &d.addresses);
        }
        if d.dns.is_empty() {
            lines.push(kv("DNS", "-"));
        } else {
            push_field(&mut lines, "DNS", &d.dns);
        }
        lines.push(kv("Start on boot", if d.autostart { "Yes" } else { "No" }));
        lines.push(kv("Kill switch", if d.killswitch { "Yes" } else { "No" }));

        for (i, p) in d.peers.iter().enumerate() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("Peer {}", i + 1),
                Style::default().fg(Color::Cyan).bold(),
            )));
            if p.public_key.is_empty() {
                lines.push(kv("Public key", "-"));
            } else {
                push_field(&mut lines, "Public key", &p.public_key);
            }
            lines.push(kv("Preshared key", if p.preshared { "Yes" } else { "-" }));
            if p.allowed_ips.is_empty() {
                lines.push(kv("Allowed IPs", "-"));
            } else {
                push_field(&mut lines, "Allowed IPs", &p.allowed_ips);
            }
            if p.endpoint.is_empty() {
                lines.push(kv("Endpoint", "-"));
            } else {
                push_field(&mut lines, "Endpoint", &p.endpoint);
            }
            lines.push(kv("Keepalive", &dash(&p.keepalive)));
            lines.push(kv("Latest hs", &dash(&p.latest_handshake)));
            lines.push(kv("Transfer", &dash(&p.transfer)));
        }
    } else {
        lines.push(Line::from(Span::styled(
            "No tunnels yet",
            Style::default().fg(Color::Cyan).bold(),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(
            "Import a WireGuard .conf file or a QR-code image to begin:",
        ));
        lines.push(Line::from(""));
        lines.push(Line::from("  i   Import a .conf file or QR image"));
        lines.push(Line::from("  n   Create a new tunnel from scratch"));
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

/// Compact "seconds ago" → "12s" / "3m" / "2h".
fn fmt_ago(s: u64) -> String {
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m", s / 60)
    } else {
        format!("{}h", s / 3600)
    }
}

/// Write the full (unfiltered) log to a file in $HOME.
fn save_log(app: &mut App) {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let dest = std::path::Path::new(&home).join("wireguard-tui-log.txt");
    match backend::save_text_to(&dest, &app.log) {
        Ok(()) => app.flash(format!("Log saved to {}", dest.display())),
        Err(e) => app.flash(format!("Save failed: {e}")),
    }
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

/// The backup manager popup: one archive per row (date, size, tunnel count,
/// name), with the selection highlighted.
fn render_backup(f: &mut Frame, items: &[backend::BackupInfo], sel: usize) {
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
            Style::default().fg(Color::Cyan),
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
            let style = if i == sel {
                Style::default().fg(Color::Black).bg(Color::Cyan).bold()
            } else {
                Style::default().fg(Color::Gray)
            };
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
        .block(Block::default().borders(Borders::ALL).title(title))
        .scroll((offset, 0));
    f.render_widget(p, area);
}

/// Colour a log line by a crude severity guess so problems stand out.
fn log_line_style(line: &str) -> Style {
    let l = line.to_ascii_lowercase();
    if l.contains("error") || l.contains("fail") || l.contains("denied") || l.contains("refused") {
        Style::default().fg(Color::Red)
    } else if l.contains("warn") {
        Style::default().fg(Color::Yellow)
    } else if l.contains("=====") {
        Style::default().fg(Color::Cyan).bold()
    } else if l.contains("activat") || l.contains("enabled") || l.contains(" added") {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::Gray)
    }
}

/// Number of log lines after filters - used for cursor/scroll clamping in the
/// key handlers without building styled `Line`s.
fn filtered_log_count(app: &App) -> usize {
    let needle = app.log_filter.to_ascii_lowercase();
    let only = if app.log_only_selected {
        app.selected_name()
    } else {
        None
    };
    app.log
        .lines()
        .filter(|line| {
            (needle.is_empty() || line.to_ascii_lowercase().contains(&needle))
                && only.as_deref().is_none_or(|n| line.contains(n))
        })
        .count()
}

/// The plain (unstyled) filtered lines - used by the copy commands.
fn filtered_log_text_lines(app: &App) -> Vec<String> {
    let needle = app.log_filter.to_ascii_lowercase();
    let only = if app.log_only_selected {
        app.selected_name()
    } else {
        None
    };
    app.log
        .lines()
        .filter(|line| {
            (needle.is_empty() || line.to_ascii_lowercase().contains(&needle))
                && only.as_deref().is_none_or(|n| line.contains(n))
        })
        .map(str::to_string)
        .collect()
}

/// The log split into styled lines after the active filters (substring +
/// this-tunnel) are applied, with the current line and any visual selection
/// highlighted.
fn filtered_log_lines(app: &App) -> Vec<Line<'static>> {
    let needle = app.log_filter.to_ascii_lowercase();
    let only = if app.log_only_selected {
        app.selected_name()
    } else {
        None
    };

    // Visual-selection range (inclusive) over filtered-line indices.
    let sel = app.log_select_anchor.map(|a| {
        let c = app.log_cursor;
        (a.min(c), a.max(c))
    });

    app.log
        .lines()
        .filter(|line| {
            (needle.is_empty() || line.to_ascii_lowercase().contains(&needle))
                && only.as_deref().is_none_or(|n| line.contains(n))
        })
        .enumerate()
        .map(|(i, line)| {
            let base = log_line_style(line);
            let styled = if sel.is_some_and(|(lo, hi)| i >= lo && i <= hi) {
                // Selected range: blue background, readable foreground.
                base.bg(Color::Blue).fg(Color::White).bold()
            } else if i == app.log_cursor {
                // Current-line cursor.
                base.bg(Color::DarkGray).add_modifier(Modifier::BOLD)
            } else {
                base
            };
            Line::styled(line.to_string(), styled)
        })
        .collect()
}

fn render_log(f: &mut Frame, app: &mut App, area: Rect) {
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
        lines.push(Line::styled(
            why.to_string(),
            Style::default().fg(Color::DarkGray),
        ));
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
    } else if view_h > 0 && cursor >= app.log_scroll + view_h {
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
        .block(Block::default().borders(Borders::ALL).title(title))
        .scroll((app.log_scroll, app.log_hscroll));
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
    let lines = msg.lines().count().min(u16::MAX as usize) as u16;
    let width = msg
        .lines()
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(20)
        .min(u16::MAX as usize) as u16;
    let title_width = title.chars().count().min(u16::MAX as usize) as u16;
    let area = popup_area(
        f.area(),
        width.saturating_add(4).max(title_width.saturating_add(4)),
        lines.saturating_add(3),
    );
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
  TUNNELS TAB
  Up/Dn k/j   Move        Enter/a  On / off
  n  New       i  Import   d  Delete
  s  On-boot   Q  Show QR  h/l  Pick detail field
  y  Copy focused field    D  Diagnose connection
  H  WireGuard explained   w  What's new (changelog)

  TUNNELS - ADVANCED
  e  Edit raw config        g  Generate keys
  c  Show running config    p  Save live state
  K  Kill switch            + Add peer section
  R  Rename                 x  Export all

  LOG TAB  (press Tab)
  Up/Dn j/k   Move cursor    PgUp/PgDn  Page
  Home/g Top  End/G Bottom   h/l Left/Right
  f  Follow newest           t  This-tunnel only
  /  Filter                  c  Clear view   r  Reload
  y/Enter Copy line          v  Visual select
  Y  Copy whole filtered log w  Save to file

  BACKUP  (press B, from any tab)
  n  new       Enter/r  restore
  d  delete    x  export to home   Esc  close

  Tab tabs   r refresh   u update   A about   q quit";
    let area = popup_area(f.area(), 60, 30);
    f.render_widget(Clear, area);
    let title = format!(
        " wg-tui v{} keys (any key closes) ",
        env!("CARGO_PKG_VERSION")
    );
    let p = Paragraph::new(help)
        .block(Block::default().borders(Borders::ALL).title(title))
        .wrap(Wrap { trim: false });
    f.render_widget(p, area);
}

fn render_about(f: &mut Frame) {
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
         \x20 Press w for what's new (changelog) · H for concepts\n\
         \n\
         \x20 MIT License  \u{b7}  Built with Rust + ratatui",
        env!("CARGO_PKG_VERSION")
    );
    let area = popup_area(f.area(), 62, 15);
    f.render_widget(Clear, area);
    let p = Paragraph::new(about).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" About (press any key to close) "),
    );
    f.render_widget(p, area);
}

/// A near-full-screen scrollable document viewer (concepts guide, changelog).
/// `scroll` is the top visible line. Modelled on how the Log view scrolls.
fn doc_area(full: Rect) -> Rect {
    let width = full.width.saturating_sub(4).max(20);
    let height = full.height.saturating_sub(2).max(6);
    popup_area(full, width, height)
}

fn doc_max_scroll(body: &str, full: Rect) -> u16 {
    let area = doc_area(full);
    let lines = Paragraph::new(body)
        .wrap(Wrap { trim: false })
        .line_count(area.width.saturating_sub(2));
    lines
        .saturating_sub(area.height.saturating_sub(2) as usize)
        .min(u16::MAX as usize) as u16
}

fn render_doc(f: &mut Frame, title: &str, body: &str, scroll: u16) {
    let area = doc_area(f.area());
    f.render_widget(Clear, area);
    let title = format!(" {title}  (↑↓/jk PgUp/PgDn g/G scroll · Esc/q close) ");
    let p = Paragraph::new(body.to_string())
        .block(Block::default().borders(Borders::ALL).title(title))
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
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
    let step = 2i32; // half-block ─ doubles vertical resolution for 2:1 cells

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
        y += step;
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn editor_arguments_preserve_quoted_paths_and_do_not_expand_shell_code() {
        assert_eq!(
            editor_argv("'/tmp/editor with spaces' --wait --option 'two words'").unwrap(),
            vec!["/tmp/editor with spaces", "--wait", "--option", "two words"]
        );
        assert_eq!(
            editor_argv("editor '$HOME' '$(touch nope)' ';'").unwrap(),
            vec!["editor", "$HOME", "$(touch nope)", ";"]
        );
        assert!(editor_argv("editor 'unterminated").is_err());
        assert!(editor_argv("  ").is_err());
    }

    #[test]
    fn removed_selection_never_keeps_another_tunnels_detail() {
        let mut app = App {
            tunnels: vec![backend::Tunnel {
                name: "old".into(),
                active: true,
            }],
            ..App::default()
        };
        app.state.select(Some(0));
        app.detail = Some(backend::Detail {
            name: "old".into(),
            active: true,
            ..Default::default()
        });
        *app.poll_out.lock().unwrap() = Some(Snapshot {
            tunnels: Some(vec![backend::Tunnel {
                name: "new".into(),
                active: false,
            }]),
            detail: None,
            log: None,
            error: None,
        });
        app.apply_snapshot();
        assert_eq!(app.selected_name().as_deref(), Some("new"));
        assert!(app.detail.is_none());
        assert!(app.detail_fields().is_empty());
    }

    #[test]
    fn config_file_reads_are_bounded_and_never_wait_for_named_pipes() {
        use std::os::unix::ffi::OsStrExt as _;
        use std::os::unix::fs::DirBuilderExt as _;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("wg-ui-config-{}-{stamp}", std::process::id()));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .unwrap();
        let path = directory.join("example.conf");
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        std::fs::write(&path, "example config").unwrap();
        assert_eq!(read_config_file(&path).unwrap(), "example config");
        file.set_len(1024 * 1024 + 1).unwrap();
        assert!(read_config_file(&path).unwrap_err().contains("1 MiB"));
        drop(file);
        std::fs::remove_file(&path).unwrap();
        let cpath = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(cpath.as_ptr(), 0o600) }, 0);
        let start = std::time::Instant::now();
        assert!(
            read_config_file(&path)
                .unwrap_err()
                .contains("regular file")
        );
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn navigation_requests_detail_without_blocking_or_exposing_previous_keys() {
        let mut app = App {
            tunnels: vec![
                backend::Tunnel {
                    name: "first".into(),
                    active: false,
                },
                backend::Tunnel {
                    name: "second".into(),
                    active: false,
                },
            ],
            detail: Some(backend::Detail {
                name: "first".into(),
                public_key: "old key".into(),
                ..Default::default()
            }),
            ..App::default()
        };
        app.state.select(Some(0));
        app.move_selection(1);
        assert_eq!(app.selected_name().as_deref(), Some("second"));
        assert!(app.detail_fields().is_empty());
        assert_eq!(
            app.poll_in.lock().unwrap().selected.as_deref(),
            Some("second")
        );
        app.enter_log_tab();
        assert!(app.poll_in.lock().unwrap().want_log);
        assert_eq!(app.log, "(loading journal…)");
    }

    #[test]
    fn asynchronous_selection_applies_only_without_user_navigation() {
        let mut app = App {
            tunnels: vec![
                backend::Tunnel {
                    name: "first".into(),
                    active: false,
                },
                backend::Tunnel {
                    name: "second".into(),
                    active: false,
                },
            ],
            ..App::default()
        };
        app.state.select(Some(0));
        app.pending_selection = Some(("created".into(), app.selection_generation));
        app.move_selection(1);
        let snapshot = || Snapshot {
            tunnels: Some(vec![
                backend::Tunnel {
                    name: "first".into(),
                    active: false,
                },
                backend::Tunnel {
                    name: "second".into(),
                    active: false,
                },
                backend::Tunnel {
                    name: "created".into(),
                    active: false,
                },
            ]),
            detail: None,
            log: None,
            error: None,
        };
        *app.poll_out.lock().unwrap() = Some(snapshot());
        app.apply_snapshot();
        assert_eq!(app.selected_name().as_deref(), Some("second"));
        app.pending_selection = Some(("created".into(), app.selection_generation));
        *app.poll_out.lock().unwrap() = Some(snapshot());
        app.apply_snapshot();
        assert_eq!(app.selected_name().as_deref(), Some("created"));
        assert!(app.pending_selection.is_none());
    }

    #[test]
    fn document_end_reaches_wrapped_tail_in_narrow_terminal() {
        let body = format!("{}\nEND-MARKER", "word ".repeat(100));
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(24, 10)).unwrap();
        let max = doc_max_scroll(&body, terminal.get_frame().area());
        assert!(usize::from(max) > body.lines().count());
        terminal
            .draw(|frame| render_doc(frame, "Guide", &body, max))
            .unwrap();
        let visible = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(visible.contains("END-MARKER"));
    }

    #[test]
    fn editor_load_waits_for_modal_draft_before_opening() {
        let mut app = App {
            mode: Mode::InputRename {
                orig: "wg0".into(),
                buf: "unsaved".into(),
            },
            operation_pending: true,
            ..App::default()
        };
        app.update_out
            .lock()
            .unwrap()
            .push_back(UpdateMsg::EditorLoaded(Ok(EditorRequest {
                name: "original-selection".into(),
                original: "original text".into(),
                initial: "editor text".into(),
                new_kind: None,
            })));
        app.drain_update();
        assert!(app.ready_editor.is_none());
        assert!(matches!(&app.mode, Mode::InputRename { buf, .. } if buf == "unsaved"));
        app.mode = Mode::Normal;
        app.drain_update();
        assert!(!app.operation_pending);
        assert_eq!(
            app.ready_editor.as_ref().unwrap().name,
            "original-selection"
        );
    }

    #[test]
    fn helper_action_keeps_ui_state_and_serializes_until_result_is_consumed() {
        let mut app = App::default();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        assert!(start_action(&mut app, "Working".into(), move || {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Ok("Finished".into())
        }));
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(!start_action(&mut app, "Duplicate".into(), || panic!(
            "overlapping worker"
        )));
        app.mode = Mode::LogFilter("unsaved filter".into());
        release_tx.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        while app.update_out.lock().unwrap().is_empty() {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        app.drain_update();
        assert!(matches!(&app.mode, Mode::LogFilter(value) if value == "unsaved filter"));
        assert!(app.operation_pending);
        app.mode = Mode::Normal;
        app.drain_update();
        assert!(!app.operation_pending);
        assert_eq!(app.status, "Finished");
    }

    #[test]
    fn delayed_diagnosis_does_not_destroy_modal_input_or_drop_messages() {
        let mut app = App {
            mode: Mode::InputNew("unsaved-name".into()),
            ..App::default()
        };
        app.update_out
            .lock()
            .unwrap()
            .push_back(UpdateMsg::Diagnosed("report".into()));
        app.update_out
            .lock()
            .unwrap()
            .push_back(UpdateMsg::Available(update::UpdateInfo {
                current: "1.0.0".into(),
                latest: "2.0.0".into(),
            }));
        app.drain_update();
        assert!(matches!(&app.mode, Mode::InputNew(value) if value == "unsaved-name"));
        assert_eq!(app.update_out.lock().unwrap().len(), 2);
        app.mode = Mode::Normal;
        app.drain_update();
        assert!(matches!(&app.mode, Mode::Message(value) if value == "report"));
        assert_eq!(app.update_out.lock().unwrap().len(), 1);
        app.mode = Mode::Normal;
        app.drain_update();
        assert_eq!(app.update.as_ref().unwrap().latest, "2.0.0");
    }

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
            tunnels: None,
            detail: None,
            log: Some("rotated".into()),
            error: None,
        });
        app.apply_snapshot();
        assert_eq!(app.log, "first\nsecond\nlatest");
    }

    #[test]
    fn long_messages_do_not_overflow_popup_dimensions() {
        let backend = ratatui::backend::TestBackend::new(40, 8);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_message(frame, "Failure", &"x".repeat(65535)))
            .unwrap();
    }

    #[test]
    fn editor_drafts_are_exclusive_private_and_clean_backups_on_drop() {
        let first = EditorDraft::new("private key").unwrap();
        let second = EditorDraft::new("different key").unwrap();
        assert_ne!(first.directory, second.directory);
        assert_eq!(
            std::fs::metadata(&first.directory)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(&first.path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let directory = first.directory.clone();
        std::fs::write(directory.join("editor-backup"), "private key").unwrap();
        drop(first);
        assert!(!directory.exists());
        assert!(second.path.exists());
    }

    #[test]
    fn activation_failure_message_preserves_multiline_unknown_error() {
        let message = activation_failure_message(
            "Drite",
            "wg-quick up Drite:\n[#] ip link add Drite\nRTNETLINK failure",
        );
        assert!(message.starts_with("Couldn't activate Drite:"));
        assert!(message.contains("[#] ip link add Drite\nRTNETLINK failure"));
    }
}
