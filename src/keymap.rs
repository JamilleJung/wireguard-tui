//! The normal-view key map: one table ([`KEYMAP`]) maps keys to commands and
//! generates the help popup, so every action is reachable and documented.

use crossterm::event::KeyCode;

/// An action in the normal (non-modal) view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Command {
    Quit,
    SwitchTab,
    Help,
    About,
    Explained,
    WhatsNew,
    Refresh,
    Update,
    Backup,
    Diagnose,
    MoveDown,
    MoveUp,
    NextField,
    PrevField,
    NextPeer,
    PrevPeer,
    DetailPageDown,
    DetailPageUp,
    Toggle,
    New,
    Import,
    Delete,
    Autostart,
    Qr,
    Copy,
    Edit,
    Generate,
    ShowRunning,
    PersistLive,
    KillSwitch,
    AddPeer,
    Rename,
    Export,
    ResumeDraft,
    LogDown,
    LogUp,
    LogPageDown,
    LogPageUp,
    LogTop,
    LogBottom,
    LogLeft,
    LogRight,
    LogFollow,
    LogOnlyTunnel,
    LogFilter,
    LogCopy,
    LogVisual,
    LogCopyAll,
    LogSave,
    LogClear,
    LogCancelSelection,
}

impl Command {
    /// Commands that start (or lead to) a helper operation; refused while
    /// another one is running.
    pub fn starts_operation(self) -> bool {
        matches!(
            self,
            Self::Toggle
                | Self::Edit
                | Self::New
                | Self::Delete
                | Self::Rename
                | Self::Autostart
                | Self::KillSwitch
                | Self::AddPeer
                | Self::Import
                | Self::PersistLive
                | Self::Export
                | Self::Update
                | Self::Backup
                | Self::Qr
                | Self::Generate
                | Self::ShowRunning
                | Self::ResumeDraft
        )
    }
}

/// Where a binding applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Tunnels,
    Log,
    Both,
}

pub struct Binding {
    pub section: &'static str,
    pub keys: &'static str,
    pub what: &'static str,
    #[cfg_attr(not(test), allow(dead_code))]
    pub scope: Scope,
    /// A representative key, used by tests to prove the binding works.
    #[cfg_attr(not(test), allow(dead_code))]
    pub key: KeyCode,
    pub command: Command,
}

const fn bind(
    section: &'static str,
    keys: &'static str,
    what: &'static str,
    scope: Scope,
    key: KeyCode,
    command: Command,
) -> Binding {
    Binding {
        section,
        keys,
        what,
        scope,
        key,
        command,
    }
}

use Command as C;
use KeyCode as K;
use Scope as S;

const TUNNELS: &str = "TUNNELS TAB";
const ADVANCED: &str = "TUNNELS - ADVANCED";
const LOG: &str = "LOG TAB  (press Tab)";
const GENERAL: &str = "EVERYWHERE";

/// Every normal-view key. The help popup is built from this table.
#[rustfmt::skip]
pub const KEYMAP: &[Binding] = &[
    bind(TUNNELS, "Up/Dn k/j", "Move between tunnels", S::Tunnels, K::Down, C::MoveDown),
    bind(TUNNELS, "Up/k", "Previous tunnel", S::Tunnels, K::Up, C::MoveUp),
    bind(TUNNELS, "Enter/a", "Connect / disconnect", S::Both, K::Char('a'), C::Toggle),
    bind(TUNNELS, "n", "New tunnel", S::Both, K::Char('n'), C::New),
    bind(TUNNELS, "i", "Import .conf or QR image", S::Both, K::Char('i'), C::Import),
    bind(TUNNELS, "d", "Delete tunnel", S::Both, K::Char('d'), C::Delete),
    bind(TUNNELS, "s", "Start on boot on/off", S::Both, K::Char('s'), C::Autostart),
    bind(TUNNELS, "Q", "Show QR code", S::Both, K::Char('Q'), C::Qr),
    bind(TUNNELS, "h/l Left/Right", "Focus previous / next detail field", S::Tunnels, K::Char('l'), C::NextField),
    bind(TUNNELS, "h/Left", "Focus previous detail field", S::Tunnels, K::Char('h'), C::PrevField),
    bind(TUNNELS, "] / [", "Jump to next / previous peer", S::Tunnels, K::Char(']'), C::NextPeer),
    bind(TUNNELS, "[", "Jump to previous peer", S::Tunnels, K::Char('['), C::PrevPeer),
    bind(TUNNELS, "PgDn/PgUp", "Scroll the detail pane", S::Tunnels, K::PageDown, C::DetailPageDown),
    bind(TUNNELS, "PgUp", "Scroll the detail pane up", S::Tunnels, K::PageUp, C::DetailPageUp),
    bind(TUNNELS, "y", "Copy focused field", S::Tunnels, K::Char('y'), C::Copy),
    bind(TUNNELS, "D", "Diagnose connection", S::Both, K::Char('D'), C::Diagnose),
    bind(ADVANCED, "e", "Edit raw config", S::Both, K::Char('e'), C::Edit),
    bind(ADVANCED, "+", "Add peer section", S::Both, K::Char('+'), C::AddPeer),
    bind(ADVANCED, "E", "Reopen a kept (unsaved) draft", S::Both, K::Char('E'), C::ResumeDraft),
    bind(ADVANCED, "g", "Generate keys", S::Tunnels, K::Char('g'), C::Generate),
    bind(ADVANCED, "c", "Show running config", S::Tunnels, K::Char('c'), C::ShowRunning),
    bind(ADVANCED, "p", "Save live state to .conf", S::Both, K::Char('p'), C::PersistLive),
    bind(ADVANCED, "K", "Kill switch on/off/repair", S::Both, K::Char('K'), C::KillSwitch),
    bind(ADVANCED, "R", "Rename", S::Both, K::Char('R'), C::Rename),
    bind(ADVANCED, "x", "Export all to ~/wireguard-tunnels.zip", S::Both, K::Char('x'), C::Export),
    bind(LOG, "Up/Dn j/k", "Move cursor", S::Log, K::Down, C::LogDown),
    bind(LOG, "Up/k", "Cursor up", S::Log, K::Up, C::LogUp),
    bind(LOG, "PgUp/PgDn", "Page", S::Log, K::PageDown, C::LogPageDown),
    bind(LOG, "PgUp", "Page up", S::Log, K::PageUp, C::LogPageUp),
    bind(LOG, "Home/g End/G", "Top / bottom", S::Log, K::Char('g'), C::LogTop),
    bind(LOG, "End/G", "Bottom", S::Log, K::Char('G'), C::LogBottom),
    bind(LOG, "h/l Left/Right", "Scroll sideways", S::Log, K::Char('h'), C::LogLeft),
    bind(LOG, "l/Right", "Scroll right", S::Log, K::Char('l'), C::LogRight),
    bind(LOG, "f", "Follow newest", S::Log, K::Char('f'), C::LogFollow),
    bind(LOG, "t", "This tunnel only", S::Log, K::Char('t'), C::LogOnlyTunnel),
    bind(LOG, "/", "Filter", S::Log, K::Char('/'), C::LogFilter),
    bind(LOG, "y/Enter", "Copy line or selection", S::Log, K::Char('y'), C::LogCopy),
    bind(LOG, "v", "Visual select (Esc cancels)", S::Log, K::Char('v'), C::LogVisual),
    bind(LOG, "Y", "Copy whole filtered log", S::Log, K::Char('Y'), C::LogCopyAll),
    bind(LOG, "w", "Save log to ~/wireguard-tui-log.txt", S::Log, K::Char('w'), C::LogSave),
    bind(LOG, "c", "Clear view", S::Log, K::Char('c'), C::LogClear),
    bind(GENERAL, "Tab", "Switch Tunnels / Log", S::Both, K::Tab, C::SwitchTab),
    bind(GENERAL, "B", "Backups (n new, Enter restore, d delete, x export)", S::Both, K::Char('B'), C::Backup),
    bind(GENERAL, "r", "Refresh / reload", S::Both, K::Char('r'), C::Refresh),
    bind(GENERAL, "H", "WireGuard explained", S::Both, K::Char('H'), C::Explained),
    bind(GENERAL, "w", "What's new (Tunnels tab)", S::Tunnels, K::Char('w'), C::WhatsNew),
    bind(GENERAL, "u", "Install available update", S::Both, K::Char('u'), C::Update),
    bind(GENERAL, "A", "About", S::Both, K::Char('A'), C::About),
    bind(GENERAL, "?", "This key map", S::Both, K::Char('?'), C::Help),
    bind(GENERAL, "q/Esc", "Quit (Ctrl+C anywhere)", S::Both, K::Char('q'), C::Quit),
];

/// Map a normal-view key to a command. `tab` is 0 (Tunnels) or 1 (Log).
pub fn command_for(tab: usize, selecting: bool, code: KeyCode) -> Option<Command> {
    let log = tab == 1;
    Some(match code {
        K::Esc if log && selecting => C::LogCancelSelection,
        K::Char('q') | K::Esc => C::Quit,
        K::Char('y') if log => C::LogCopy,
        K::Char('y') => C::Copy,
        K::Char('v') if log => C::LogVisual,
        K::Char('Y') if log => C::LogCopyAll,
        K::Tab | K::BackTab => C::SwitchTab,
        K::Enter if log => C::LogCopy,
        K::Down | K::Char('j') if log => C::LogDown,
        K::Down | K::Char('j') => C::MoveDown,
        K::Up | K::Char('k') if log => C::LogUp,
        K::Up | K::Char('k') => C::MoveUp,
        K::PageDown if log => C::LogPageDown,
        K::PageUp if log => C::LogPageUp,
        K::PageDown => C::DetailPageDown,
        K::PageUp => C::DetailPageUp,
        K::Home | K::Char('g') if log => C::LogTop,
        K::End | K::Char('G') if log => C::LogBottom,
        K::Left | K::Char('h') if log => C::LogLeft,
        K::Right | K::Char('l') if log => C::LogRight,
        K::Right | K::Char('l') => C::NextField,
        K::Left | K::Char('h') => C::PrevField,
        K::Char(']') if !log => C::NextPeer,
        K::Char('[') if !log => C::PrevPeer,
        K::Char('f') if log => C::LogFollow,
        K::Char('t') if log => C::LogOnlyTunnel,
        K::Char('/') if log => C::LogFilter,
        K::Char('w') if log => C::LogSave,
        K::Char('c') if log => C::LogClear,
        K::Char('B') => C::Backup,
        K::Enter | K::Char('a') => C::Toggle,
        K::Char('e') => C::Edit,
        K::Char('E') => C::ResumeDraft,
        K::Char('n') => C::New,
        K::Char('d') => C::Delete,
        K::Char('R') => C::Rename,
        K::Char('s') => C::Autostart,
        K::Char('K') => C::KillSwitch,
        K::Char('+') => C::AddPeer,
        K::Char('i') => C::Import,
        K::Char('g') => C::Generate,
        K::Char('c') => C::ShowRunning,
        K::Char('p') => C::PersistLive,
        K::Char('x') => C::Export,
        K::Char('Q') => C::Qr,
        K::Char('r') => C::Refresh,
        K::Char('u') => C::Update,
        K::Char('D') => C::Diagnose,
        K::Char('?') => C::Help,
        K::Char('A') => C::About,
        K::Char('H') => C::Explained,
        K::Char('w') => C::WhatsNew,
        _ => return None,
    })
}

/// The help popup text, built from [`KEYMAP`] (one line per entry,
/// skipping the secondary half of paired entries).
pub fn help_text() -> String {
    let mut out = String::new();
    let mut section = "";
    for binding in KEYMAP {
        if is_secondary(binding) {
            continue;
        }
        if binding.section != section {
            if !section.is_empty() {
                out.push('\n');
            }
            section = binding.section;
            out.push_str(&format!("  {section}\n"));
        }
        out.push_str(&format!("  {:<15} {}\n", binding.keys, binding.what));
    }
    out.push_str(
        "\n  POPUPS\n  Esc closes; y/n confirm; arrows/jk scroll lists\n  Draft kept: e re-edit, r retry, d discard, Esc keep for later",
    );
    out
}

/// Entries whose keys are already shown on the line before (e.g. "Up/k").
pub(crate) fn is_secondary(binding: &Binding) -> bool {
    matches!(
        binding.command,
        C::MoveUp
            | C::PrevField
            | C::PrevPeer
            | C::DetailPageUp
            | C::LogUp
            | C::LogPageUp
            | C::LogBottom
            | C::LogRight
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    use Command as C;
    use KeyCode as K;

    /// Every command, listed by an exhaustive match so a new command cannot be
    /// added without deciding how it is reached.
    fn all_commands() -> Vec<Command> {
        let all = [
            C::Quit,
            C::SwitchTab,
            C::Help,
            C::About,
            C::Explained,
            C::WhatsNew,
            C::Refresh,
            C::Update,
            C::Backup,
            C::Diagnose,
            C::MoveDown,
            C::MoveUp,
            C::NextField,
            C::PrevField,
            C::NextPeer,
            C::PrevPeer,
            C::DetailPageDown,
            C::DetailPageUp,
            C::Toggle,
            C::New,
            C::Import,
            C::Delete,
            C::Autostart,
            C::Qr,
            C::Copy,
            C::Edit,
            C::Generate,
            C::ShowRunning,
            C::PersistLive,
            C::KillSwitch,
            C::AddPeer,
            C::Rename,
            C::Export,
            C::ResumeDraft,
            C::LogDown,
            C::LogUp,
            C::LogPageDown,
            C::LogPageUp,
            C::LogTop,
            C::LogBottom,
            C::LogLeft,
            C::LogRight,
            C::LogFollow,
            C::LogOnlyTunnel,
            C::LogFilter,
            C::LogCopy,
            C::LogVisual,
            C::LogCopyAll,
            C::LogSave,
            C::LogClear,
            C::LogCancelSelection,
        ];
        for command in all {
            match command {
                C::Quit
                | C::SwitchTab
                | C::Help
                | C::About
                | C::Explained
                | C::WhatsNew
                | C::Refresh
                | C::Update
                | C::Backup
                | C::Diagnose
                | C::MoveDown
                | C::MoveUp
                | C::NextField
                | C::PrevField
                | C::NextPeer
                | C::PrevPeer
                | C::DetailPageDown
                | C::DetailPageUp
                | C::Toggle
                | C::New
                | C::Import
                | C::Delete
                | C::Autostart
                | C::Qr
                | C::Copy
                | C::Edit
                | C::Generate
                | C::ShowRunning
                | C::PersistLive
                | C::KillSwitch
                | C::AddPeer
                | C::Rename
                | C::Export
                | C::ResumeDraft
                | C::LogDown
                | C::LogUp
                | C::LogPageDown
                | C::LogPageUp
                | C::LogTop
                | C::LogBottom
                | C::LogLeft
                | C::LogRight
                | C::LogFollow
                | C::LogOnlyTunnel
                | C::LogFilter
                | C::LogCopy
                | C::LogVisual
                | C::LogCopyAll
                | C::LogSave
                | C::LogClear
                | C::LogCancelSelection => {}
            }
        }
        all.to_vec()
    }

    #[test]
    fn every_keymap_entry_is_handled_by_its_key_in_its_tab() {
        for binding in KEYMAP {
            let tabs: &[usize] = match binding.scope {
                Scope::Tunnels => &[0],
                Scope::Log => &[1],
                Scope::Both => &[0, 1],
            };
            for &tab in tabs {
                assert_eq!(
                    command_for(tab, false, binding.key),
                    Some(binding.command),
                    "{} in tab {tab}",
                    binding.keys
                );
            }
        }
    }

    #[test]
    fn every_command_is_reachable_by_keyboard_and_documented() {
        let documented: HashSet<_> = KEYMAP.iter().map(|b| b.command).collect();
        let help = help_text();
        for command in all_commands() {
            if command == C::LogCancelSelection {
                // Esc during a visual selection; documented with `v`.
                assert_eq!(command_for(1, true, K::Esc), Some(command));
                assert!(help.contains("Esc cancels"));
                continue;
            }
            assert!(documented.contains(&command), "{command:?} has no key");
        }
        for binding in KEYMAP.iter().filter(|b| !is_secondary(b)) {
            assert!(
                help.contains(binding.what),
                "{} missing from help",
                binding.what
            );
        }
    }

    #[test]
    fn log_keys_override_tunnel_keys_only_on_the_log_tab() {
        assert_eq!(command_for(0, false, K::Char('c')), Some(C::ShowRunning));
        assert_eq!(command_for(1, false, K::Char('c')), Some(C::LogClear));
        assert_eq!(command_for(0, false, K::Enter), Some(C::Toggle));
        assert_eq!(command_for(1, false, K::Enter), Some(C::LogCopy));
        assert_eq!(command_for(1, false, K::Esc), Some(C::Quit));
        assert_eq!(command_for(0, false, K::Char('w')), Some(C::WhatsNew));
        assert_eq!(command_for(1, false, K::Char('w')), Some(C::LogSave));
        // Tunnel actions still work from the Log tab.
        assert_eq!(command_for(1, false, K::Char('a')), Some(C::Toggle));
        assert_eq!(command_for(0, false, K::F(5)), None);
    }
}
