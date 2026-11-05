//! Application state: the data the UI renders, the modal mode, and how
//! background results (poller snapshots, worker reports) are folded in.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::ListState;

use crate::backend::health::{self, Health, HealthTracker};
use crate::backend::{self, KillSwitch};
use crate::ops::{KillSwitchWatch, OpKind, Operation, Outcome, Settled};
use crate::theme::Theme;
use crate::{create, doctor, logging, update};

/// Background poll interval (live status refresh).
pub const TICK: Duration = Duration::from_millis(1500);
/// How long a footer status message stays before it auto-clears.
pub const STATUS_TTL: Duration = Duration::from_secs(4);

/// One entry in the import file browser.
pub struct FileEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
}

/// What the background poller is told to fetch (current selection + tab).
#[derive(Default)]
pub struct PollIn {
    pub selected: Option<String>,
    pub want_log: bool,
    /// Bumped to ask the poller for an immediate refresh.
    pub generation: u64,
}

/// A live snapshot computed off the UI thread by the poller.
pub struct Snapshot {
    /// `None` means the helper call failed - keep the existing list, just report.
    pub tunnels: Option<Vec<backend::Tunnel>>,
    pub detail: Option<(String, backend::Detail)>,
    pub log: Option<String>,
    pub error: Option<String>,
    pub clock_synced: Option<bool>,
    /// When the poller started gathering this snapshot.
    pub taken_at: Instant,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            tunnels: None,
            detail: None,
            log: None,
            error: None,
            clock_synced: None,
            taken_at: Instant::now(),
        }
    }
}

/// An editor buffer that has not been saved yet. Never discarded without the
/// user asking for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Draft {
    pub name: String,
    /// The saved text the draft started from (empty for a new tunnel).
    pub original: String,
    pub text: String,
    pub new_kind: Option<create::TunnelTemplateKind>,
}

impl Draft {
    pub fn into_request(self) -> EditorRequest {
        EditorRequest {
            name: self.name,
            original: self.original,
            initial: self.text,
            new_kind: self.new_kind,
            keep_on_cancel: true,
        }
    }
}

/// Why a draft is waiting in the recovery popup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DraftProblem {
    /// The text did not validate; Retry makes no sense, Re-edit does.
    Invalid,
    /// The helper refused or failed to save it; Retry may work.
    SaveFailed,
}

/// What the foreground is currently doing - a normal view or a modal popup.
#[derive(Default)]
pub enum Mode {
    #[default]
    Normal,
    /// The key map; scrollable.
    Help { scroll: u16 },
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
    NewPreset { name: String },
    /// Text entry to rename `orig`.
    InputRename { orig: String, buf: String },
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
    /// A draft that failed to validate or save: Retry / Re-edit / Discard.
    DraftRecovery {
        draft: Draft,
        problem: DraftProblem,
        error: String,
    },
    /// Second step before throwing a draft away.
    ConfirmDiscard {
        draft: Draft,
        problem: DraftProblem,
        error: String,
    },
    /// The config was saved but applying it to the running interface failed.
    SyncFailed {
        name: String,
        error: String,
        needs_reconnect: bool,
    },
    /// The kill switch is on for this tunnel but not in force.
    KillSwitchProblem { name: String, status: KillSwitch },
}

/// A finished background operation, as reported by its worker.
pub struct Report {
    pub outcome: Outcome,
    pub message: String,
    /// Show `message` as a popup instead of a footer flash.
    pub popup: bool,
    /// Switch to this mode (takes precedence over `message`).
    pub mode: Option<Mode>,
    /// Select this tunnel once it appears, unless the user navigated since
    /// the operation started (selection generation).
    pub select: Option<(String, u64)>,
    /// The worker brought this tunnel up (starts the health grace period).
    pub activated: Option<String>,
}

impl Report {
    pub fn ok(message: impl Into<String>) -> Self {
        Self {
            outcome: Outcome::Succeeded,
            message: message.into(),
            popup: false,
            mode: None,
            select: None,
            activated: None,
        }
    }

    pub fn failed(message: impl Into<String>) -> Self {
        Self {
            outcome: Outcome::Failed,
            popup: true,
            ..Self::ok(message)
        }
    }

    #[cfg(test)]
    pub fn unknown(message: impl Into<String>) -> Self {
        Self {
            outcome: Outcome::Unknown,
            popup: true,
            ..Self::ok(message)
        }
    }

    pub fn with_mode(outcome: Outcome, mode: Mode) -> Self {
        Self {
            outcome,
            mode: Some(mode),
            ..Self::ok(String::new())
        }
    }

    pub fn from_settled(settled: Settled) -> Self {
        Self {
            outcome: settled.outcome,
            popup: settled.popup,
            ..Self::ok(settled.message)
        }
    }

    pub fn from_result(result: Result<String, String>) -> Self {
        match result {
            Ok(message) => Self::ok(message),
            Err(error) => Self::failed(error),
        }
    }
}

/// A message from a background worker thread, drained each UI loop.
pub enum UpdateMsg {
    /// A newer release is available.
    Available(update::UpdateInfo),
    /// A finished connection diagnosis report, to show in a popup.
    Diagnosed(String),
    /// A foreground operation finished.
    Finished(Report),
    /// The editor's initial text is ready (or could not be loaded).
    EditorLoaded(Result<EditorRequest, String>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorRequest {
    pub name: String,
    pub original: String,
    pub initial: String,
    pub new_kind: Option<create::TunnelTemplateKind>,
    /// `initial` is an unsaved draft: cancelling the editor must keep it.
    pub keep_on_cancel: bool,
}

/// Work that needs the real terminal (suspending the UI), queued by input
/// handling and run by the main loop.
#[derive(Debug, PartialEq, Eq)]
pub enum TerminalTask {
    SelfUpdate,
}

#[derive(Default)]
pub struct App {
    pub tunnels: Vec<backend::Tunnel>,
    pub state: ListState,
    pub detail: Option<backend::Detail>,
    pub tab: usize, // 0 = Tunnels, 1 = Log
    pub log: String,
    pub log_scroll: u16,
    /// Auto-scroll the Log tab to the newest line on every refresh.
    pub log_follow: bool,
    /// Case-insensitive substring filter applied to the Log tab.
    pub log_filter: String,
    /// Show only Log lines that mention the selected tunnel.
    pub log_only_selected: bool,
    /// Highlighted "current line" in the Log view (index into the *filtered*
    /// lines). Drives scroll, copy-current-line, and visual selection.
    pub log_cursor: usize,
    /// Visual-selection anchor (Some when in visual mode). The selected range is
    /// [min(anchor,cursor) ..= max(anchor,cursor)].
    pub log_select_anchor: Option<usize>,
    /// Horizontal scroll for wide log lines (h/l or Left/Right).
    pub log_hscroll: u16,
    /// True after `c` clears the log; suppresses the poller's snapshot from
    /// overwriting the cleared placeholder until the next explicit log action.
    pub log_cleared: bool,
    /// Cursor line in the Detail view (index into a flat list of copyable fields).
    pub detail_cursor: usize,
    /// Top visible row of the Detail pane.
    pub detail_scroll: u16,
    /// Scroll the Detail pane to the focused field on the next draw.
    pub detail_follow_focus: bool,
    pub status: String,
    pub status_at: Option<Instant>,
    pub mode: Mode,
    pub quit: bool,
    /// Live throughput (bytes/sec) for the selected tunnel + the last sample it
    /// was derived from (name, rx, tx, when) so deltas reset on a new selection.
    pub rate_rx: u64,
    pub rate_tx: u64,
    pub prev_sample: Option<(String, u64, u64, Instant)>,
    // Shared with the background poller thread (off-UI-thread live refresh).
    pub poll_in: Arc<Mutex<PollIn>>,
    pub poll_out: Arc<Mutex<Option<Snapshot>>>,
    /// A pending newer release found by the detached startup check (if any).
    pub update: Option<update::UpdateInfo>,
    /// Short status line for an in-progress/just-finished self-update.
    pub update_status: Option<String>,
    /// Off-UI-thread mailbox for worker results.
    pub update_out: Arc<Mutex<VecDeque<UpdateMsg>>>,
    /// The foreground helper mutation (one at a time).
    pub op: Operation,
    /// The read-only diagnosis, which may run alongside an operation.
    pub diagnosis: Operation,
    pub ready_editor: Option<EditorRequest>,
    pub selection_generation: u64,
    pub pending_selection: Option<(String, u64)>,
    /// Per-peer health tracking across refreshes.
    pub health: HealthTracker,
    /// Health of each peer of `detail`, in the same order.
    pub peer_health: Vec<Health>,
    /// Whether the system clock is NTP-synced (from the poller).
    pub clock_synced: Option<bool>,
    pub killswitch_watch: KillSwitchWatch,
    /// A draft the user set aside with Esc (E reopens it).
    pub parked_draft: Option<(Draft, DraftProblem, String)>,
    pub theme: Theme,
    /// The last drawn frame size (for scroll limits in key handling).
    pub viewport: Rect,
    pub terminal_task: Option<TerminalTask>,
}

impl App {
    pub fn new(theme: Theme) -> Self {
        let mut app = Self {
            log_follow: true,
            theme,
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
    pub fn sync_poll_in(&self) {
        let mut pi = self.poll_in.lock().unwrap();
        pi.selected = self.selected_name();
        pi.want_log = self.tab == 1;
    }

    /// Ask the poller to refresh now (after a mutation or a timeout).
    pub fn request_refresh(&self) {
        let mut pi = self.poll_in.lock().unwrap();
        pi.generation = pi.generation.wrapping_add(1);
    }

    /// Apply the latest background snapshot to the UI state (non-blocking).
    pub fn apply_snapshot(&mut self) {
        let Some(snap) = self.poll_out.lock().unwrap().take() else {
            return;
        };
        let now = Instant::now();
        if let Some(err) = snap.error {
            self.flash(err);
        }
        self.clock_synced = snap.clock_synced.or(self.clock_synced);
        if let Some(tunnels) = snap.tunnels {
            self.track_activation(&tunnels, now);
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
                self.clear_detail();
            }
            // The re-queried state is now on screen: a finished operation
            // (including one that timed out) is reconciled.
            if let Operation::Finished { at, .. } = self.op
                && snap.taken_at >= at
            {
                self.op.reconciled();
            }
        }
        // Only apply detail that still matches the current selection.
        if let Some((name, d)) = snap.detail
            && Some(&name) == self.selected_name().as_ref()
        {
            let (rx, tx) = (d.rx_bytes, d.tx_bytes);
            self.peer_health = d
                .peers
                .iter()
                .map(|peer| {
                    self.health
                        .observe(&name, &peer.public_key, peer.sample(), now)
                })
                .collect();
            self.detail = Some(d);
            self.update_speed(&name, rx, tx);
            self.watch_killswitch(now);
        }
        if let Some(log) = snap.log
            && !self.log_cleared
            && self.log_select_anchor.is_none()
        {
            self.log = log;
        }
    }

    /// Start the health grace period for tunnels seen coming up.
    fn track_activation(&mut self, tunnels: &[backend::Tunnel], now: Instant) {
        if self.tunnels.is_empty() {
            return; // first list: we did not see these come up
        }
        for tunnel in tunnels {
            let was = self.tunnels.iter().find(|t| t.name == tunnel.name);
            match (was.map(|t| t.active), tunnel.active) {
                (Some(false) | None, true) => self.health.mark_activated(&tunnel.name, now),
                (Some(true), false) => self.health.mark_deactivated(&tunnel.name),
                _ => {}
            }
        }
    }

    /// Run `killswitch-repair` automatically, once per detected loss.
    fn watch_killswitch(&mut self, now: Instant) {
        let Some(detail) = &self.detail else { return };
        let can_start = !self.op.is_busy();
        let name = detail.name.clone();
        if self
            .killswitch_watch
            .observe(&name, detail.active, &detail.killswitch, can_start, now)
        {
            logging::event(
                "killswitch.auto_repair",
                &[("name", &name), ("status", detail.killswitch.label())],
            );
            crate::actions::repair_killswitch(self, name, true);
        }
    }

    fn clear_detail(&mut self) {
        self.detail = None;
        self.detail_cursor = 0;
        self.detail_scroll = 0;
        self.peer_health.clear();
        self.rate_rx = 0;
        self.rate_tx = 0;
        self.prev_sample = None;
    }

    /// Health summary of the selected tunnel (None when it is not up).
    pub fn tunnel_health(&self) -> Option<Health> {
        let detail = self.detail.as_ref().filter(|d| d.active)?;
        if detail.peers.is_empty() || self.peer_health.len() != detail.peers.len() {
            return None;
        }
        Some(health::summarize(&self.peer_health))
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

    pub fn selected_name(&self) -> Option<String> {
        self.state
            .selected()
            .and_then(|i| self.tunnels.get(i))
            .map(|t| t.name.clone())
    }

    pub fn load_detail(&mut self) {
        // Navigation must not wait for privileged helper reads. Clear stale
        // actions/keys immediately, then let the poller load this selection.
        self.clear_detail();
        self.sync_poll_in();
        self.request_refresh();
    }

    /// Switch to the Log tab and prime it so the pane is never blank on first
    /// view: request a background load, reset cursor/selection, re-enable follow, and
    /// flip `want_log` so the poller keeps it fresh.
    pub fn enter_log_tab(&mut self) {
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
        self.request_refresh();
    }

    /// Move the Log cursor by `delta` lines, clamped; auto-pause follow when the
    /// user scrolls up, re-enable it when the cursor reaches the last line.
    pub fn log_move_cursor(&mut self, delta: isize) {
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

    pub fn log_cursor_top(&mut self) {
        self.log_cursor = 0;
        self.log_follow = false;
    }

    pub fn log_cursor_bottom(&mut self) {
        let n = filtered_log_count(self);
        self.log_cursor = n.saturating_sub(1);
        self.log_follow = true;
    }

    /// Drain any pending message from the worker threads (non-blocking).
    pub fn drain_update(&mut self) {
        // Never discard a name/filter draft or confirmation when a delayed
        // result finishes. Keep messages queued until normal mode resumes.
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
            UpdateMsg::EditorLoaded(result) => {
                self.status.clear();
                self.status_at = None;
                let now = Instant::now();
                match result {
                    Ok(request) => {
                        self.op.complete(Outcome::Succeeded, now);
                        self.op.reconciled();
                        self.ready_editor = Some(request);
                    }
                    Err(error) => {
                        self.op.complete(Outcome::Failed, now);
                        self.op.reconciled();
                        self.mode = Mode::Message(error);
                    }
                }
            }
            UpdateMsg::Finished(report) => self.finish(report),
            UpdateMsg::Diagnosed(report) => {
                let now = Instant::now();
                self.diagnosis.complete(Outcome::Succeeded, now);
                self.diagnosis.reconciled();
                self.status.clear();
                self.status_at = None;
                self.mode = Mode::Message(report);
            }
        }
    }

    fn finish(&mut self, report: Report) {
        let now = Instant::now();
        let kind = self.op.running().map(|(kind, _)| kind);
        self.op.complete(report.outcome, now);
        logging::event(
            "operation.finished",
            &[
                ("kind", kind.map_or("none", OpKind::label)),
                ("outcome", report.outcome.label()),
            ],
        );
        if let Some((name, generation)) = report.select
            && generation == self.selection_generation
        {
            self.pending_selection = Some((name, generation));
        }
        if let Some(name) = &report.activated {
            self.health.mark_activated(name, now);
        }
        match report.mode {
            Some(mode) => {
                self.status.clear();
                self.status_at = None;
                self.mode = mode;
            }
            None if report.popup => {
                self.status.clear();
                self.status_at = None;
                self.mode = Mode::Message(report.message);
            }
            None => self.flash(report.message),
        }
        // Whatever the worker claimed, show the re-queried state next.
        self.request_refresh();
    }

    /// Copyable Detail fields in render order: (label, value, peer index).
    pub fn detail_entries(&self) -> Vec<(String, String, Option<usize>)> {
        let mut v = Vec::new();
        let Some(d) = &self.detail else { return v };
        for (label, value) in [
            ("Public key", &d.public_key),
            ("Listen port", &d.listen_port),
            ("Addresses", &d.addresses),
            ("DNS", &d.dns),
        ] {
            if !value.is_empty() {
                v.push((label.to_string(), value.clone(), None));
            }
        }
        for (i, p) in d.peers.iter().enumerate() {
            let pre = format!("Peer {} ", i + 1);
            for (label, value) in [
                ("public key", &p.public_key),
                ("allowed IPs", &p.allowed_ips),
                ("endpoint", &p.endpoint),
            ] {
                if !value.is_empty() {
                    v.push((format!("{pre}{label}"), value.clone(), Some(i)));
                }
            }
        }
        v
    }

    /// Flat list of copyable Detail fields in render order: (label, value).
    pub fn detail_fields(&self) -> Vec<(String, String)> {
        self.detail_entries()
            .into_iter()
            .map(|(label, value, _)| (label, value))
            .collect()
    }

    /// Move the focused Detail field by `delta`, wrapping.
    pub fn move_field(&mut self, delta: isize) {
        let n = self.detail_entries().len();
        if n > 0 {
            self.detail_cursor =
                (self.detail_cursor as isize + delta).rem_euclid(n as isize) as usize;
            self.detail_follow_focus = true;
        }
    }

    /// Jump the focus to the first field of the next (`forward`) or previous
    /// peer, wrapping around.
    pub fn jump_peer(&mut self, forward: bool) {
        let entries = self.detail_entries();
        let peers = self.detail.as_ref().map_or(0, |d| d.peers.len());
        if entries.is_empty() || peers == 0 {
            return;
        }
        let current = entries
            .get(self.detail_cursor.min(entries.len() - 1))
            .and_then(|(_, _, peer)| *peer);
        let target = match (current, forward) {
            (None, true) => 0,
            (None, false) => peers - 1,
            (Some(p), true) => (p + 1) % peers,
            (Some(p), false) => (p + peers - 1) % peers,
        };
        if let Some(index) = entries.iter().position(|(_, _, p)| *p == Some(target)) {
            self.detail_cursor = index;
            self.detail_follow_focus = true;
        }
    }

    /// Scroll the Detail pane by `delta` rows (render clamps the range).
    pub fn scroll_detail(&mut self, delta: i32) {
        self.detail_scroll = (self.detail_scroll as i32 + delta).clamp(0, u16::MAX as i32) as u16;
        self.detail_follow_focus = false;
    }

    /// Periodic refresh: live status, and the log if that tab is open. An
    /// explicit reload re-shows fresh log data, so it clears the cleared flag.
    pub fn tick(&mut self) {
        self.log_cleared = false;
        self.sync_poll_in();
        self.request_refresh();
    }

    pub fn move_selection(&mut self, delta: isize) {
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

    pub fn flash(&mut self, msg: impl Into<String>) {
        self.status = msg.into();
        self.status_at = Some(Instant::now());
    }

    /// Clear the footer status once it has been shown for `STATUS_TTL`.
    pub fn expire_status(&mut self) {
        if let Some(at) = self.status_at
            && at.elapsed() >= STATUS_TTL
        {
            self.status.clear();
            self.status_at = None;
        }
    }
}

fn log_line_matches(needle: &str, only: Option<&str>, line: &str) -> bool {
    (needle.is_empty() || line.to_ascii_lowercase().contains(needle))
        && only.is_none_or(|n| line.contains(n))
}

/// The log lines that pass the active filters (substring + this-tunnel).
pub fn filtered_log<'a>(app: &'a App) -> impl Iterator<Item = &'a str> + 'a {
    let needle = app.log_filter.to_ascii_lowercase();
    let only = if app.log_only_selected {
        app.selected_name()
    } else {
        None
    };
    app.log
        .lines()
        .filter(move |line| log_line_matches(&needle, only.as_deref(), line))
}

/// Number of log lines after filters - used for cursor/scroll clamping in the
/// key handlers without building styled `Line`s.
pub fn filtered_log_count(app: &App) -> usize {
    filtered_log(app).count()
}

/// The plain (unstyled) filtered lines - used by the copy commands.
pub fn filtered_log_text_lines(app: &App) -> Vec<String> {
    filtered_log(app).map(str::to_string).collect()
}

/// Trim the dev-facing Keep-a-Changelog header/intro and the (usually empty)
/// `[Unreleased]` section for the in-app "What's new" view — start at the first
/// `## [x.y.z]` heading. Returns a sub-slice, so the body stays `&'static str`.
pub fn changelog_for_display(src: &'static str) -> &'static str {
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

#[cfg(test)]
mod tests;
