//! Pure logic behind background operations: the operation state machine,
//! verifying an action against the state actually observed afterwards, and
//! the kill-switch repair policy.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::backend::{HelperError, KillSwitch};

/// What a background operation is doing (for logs and the busy message).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpKind {
    Connect,
    Disconnect,
    Reconnect,
    Save,
    Sync,
    Create,
    Rename,
    Delete,
    Import,
    Autostart,
    KillSwitch,
    KillSwitchRepair,
    Backup,
    Restore,
    Export,
    PersistLive,
    Read,
    LoadEditor,
    Diagnose,
}

impl OpKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Connect => "connect",
            Self::Disconnect => "disconnect",
            Self::Reconnect => "reconnect",
            Self::Save => "save",
            Self::Sync => "sync",
            Self::Create => "create",
            Self::Rename => "rename",
            Self::Delete => "delete",
            Self::Import => "import",
            Self::Autostart => "autostart",
            Self::KillSwitch => "killswitch",
            Self::KillSwitchRepair => "killswitch-repair",
            Self::Backup => "backup",
            Self::Restore => "restore",
            Self::Export => "export",
            Self::PersistLive => "persist",
            Self::Read => "read",
            Self::LoadEditor => "load-editor",
            Self::Diagnose => "diagnose",
        }
    }
}

/// How an operation ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Succeeded,
    Failed,
    /// The helper timed out and re-checking could not tell what happened.
    Unknown,
}

impl Outcome {
    pub fn label(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Unknown => "unknown",
        }
    }
}

/// Idle → Running → Finished → (next refresh reconciles) → Idle.
///
/// Only one mutation runs at a time: `begin` refuses while `Running`. A
/// finished operation stays visible until the next live refresh has been
/// applied, so the UI reflects the re-queried state rather than the worker's
/// own claim.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Operation {
    #[default]
    Idle,
    Running {
        kind: OpKind,
        started: Instant,
    },
    Finished {
        kind: OpKind,
        outcome: Outcome,
        at: Instant,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub struct Busy(pub OpKind);

impl Operation {
    pub fn begin(&mut self, kind: OpKind, now: Instant) -> Result<(), Busy> {
        if let Self::Running { kind: running, .. } = self {
            return Err(Busy(*running));
        }
        *self = Self::Running { kind, started: now };
        Ok(())
    }

    /// Record the result. Ignored unless an operation is running.
    pub fn complete(&mut self, outcome: Outcome, now: Instant) {
        if let Self::Running { kind, .. } = *self {
            *self = Self::Finished {
                kind,
                outcome,
                at: now,
            };
        }
    }

    /// A refresh has been applied after the operation finished.
    pub fn reconciled(&mut self) {
        if matches!(self, Self::Finished { .. }) {
            *self = Self::Idle;
        }
    }

    pub fn is_busy(&self) -> bool {
        matches!(self, Self::Running { .. })
    }

    pub fn running(&self) -> Option<(OpKind, Duration)> {
        match self {
            Self::Running { kind, started } => Some((*kind, started.elapsed())),
            _ => None,
        }
    }
}

/// The user-facing result of checking an action against reality.
#[derive(Debug, PartialEq, Eq)]
pub struct Settled {
    pub outcome: Outcome,
    pub message: String,
    /// Show as a popup (something needs attention) rather than a footer flash.
    pub popup: bool,
}

/// Texts for one verified action, e.g. connecting `wg0`.
pub struct Expectation {
    /// "Activated wg0"
    pub done: String,
    /// "wg0 is up"
    pub state: String,
    /// "wg0 is not up"
    pub not_state: String,
    /// "Couldn't activate wg0" - prefix for a failure popup.
    pub failed: String,
}

/// Combine what the helper reported with a fresh observation of the state.
/// `observed` is `Ok(true)` when the expected state holds.
pub fn settle(
    result: &Result<(), HelperError>,
    observed: Result<bool, HelperError>,
    expect: &Expectation,
    explain: impl Fn(&HelperError) -> String,
) -> Settled {
    let ok = |message: String| Settled {
        outcome: Outcome::Succeeded,
        message,
        popup: false,
    };
    match (result, observed) {
        (Ok(()), Ok(true)) => ok(expect.done.clone()),
        (Ok(()), Ok(false)) => Settled {
            outcome: Outcome::Failed,
            message: format!(
                "{}:\n\nThe helper reported success, but re-checking shows {}.",
                expect.failed, expect.not_state
            ),
            popup: true,
        },
        (Ok(()), Err(check)) => ok(format!("{} (could not re-check: {check})", expect.done)),
        (Err(error), Ok(true)) if error.is_timeout() => ok(format!(
            "{} (the helper timed out, but re-checking shows {})",
            expect.done, expect.state
        )),
        (Err(error), Ok(false)) if error.is_timeout() => Settled {
            outcome: Outcome::Failed,
            message: format!(
                "{}:\n\nThe helper timed out. Re-checking shows {}.",
                expect.failed, expect.not_state
            ),
            popup: true,
        },
        (Err(error), Err(check)) if error.is_timeout() => Settled {
            outcome: Outcome::Unknown,
            message: format!(
                "{}:\n\nThe helper timed out and the current state could not be re-checked ({check}). It may or may not have taken effect; the view refreshes automatically.",
                expect.failed
            ),
            popup: true,
        },
        (Err(error), Ok(true)) => Settled {
            outcome: Outcome::Succeeded,
            message: format!(
                "{} - but the helper also reported an error:\n\n{}",
                expect.done,
                explain(error)
            ),
            popup: true,
        },
        (Err(error), _) => Settled {
            outcome: Outcome::Failed,
            message: format!("{}:\n\n{}", expect.failed, explain(error)),
            popup: true,
        },
    }
}

/// Decides when to run `killswitch-repair` automatically: once per detected
/// loss, and never more often than an exponentially growing interval when
/// losses keep recurring. A failed repair is not retried automatically.
#[derive(Default)]
pub struct KillSwitchWatch {
    tunnels: HashMap<String, WatchState>,
}

#[derive(Clone, Copy, Debug)]
struct WatchState {
    /// The current loss has already had its automatic attempt.
    attempted: bool,
    /// Automatic attempts so far (drives the backoff).
    attempts: u32,
    last_attempt: Option<Instant>,
}

pub const REPAIR_BACKOFF: Duration = Duration::from_secs(15);
pub const REPAIR_BACKOFF_MAX: Duration = Duration::from_secs(15 * 60);
/// Healthy for this long after a repair resets the backoff.
pub const REPAIR_RESET_AFTER: Duration = Duration::from_secs(30 * 60);

impl KillSwitchWatch {
    fn backoff(attempts: u32) -> Duration {
        REPAIR_BACKOFF
            .saturating_mul(1u32 << attempts.saturating_sub(1).min(16))
            .min(REPAIR_BACKOFF_MAX)
    }

    /// Feed one observation; returns true when a repair should start now.
    /// `can_start` is false while another operation runs, in which case the
    /// attempt is simply deferred to a later observation.
    pub fn observe(
        &mut self,
        name: &str,
        active: bool,
        status: &KillSwitch,
        can_start: bool,
        now: Instant,
    ) -> bool {
        let state = self.tunnels.entry(name.to_string()).or_insert(WatchState {
            attempted: false,
            attempts: 0,
            last_attempt: None,
        });
        if !active || !status.is_broken() {
            if matches!(status, KillSwitch::Enabled | KillSwitch::Disabled) || !active {
                state.attempted = false;
                if state
                    .last_attempt
                    .is_some_and(|at| now.saturating_duration_since(at) >= REPAIR_RESET_AFTER)
                {
                    state.attempts = 0;
                }
            }
            return false;
        }
        if state.attempted || !can_start {
            return false;
        }
        if let Some(at) = state.last_attempt
            && now.saturating_duration_since(at) < Self::backoff(state.attempts)
        {
            return false;
        }
        state.attempted = true;
        state.attempts = state.attempts.saturating_add(1);
        state.last_attempt = Some(now);
        true
    }

    /// Whether the current loss already had its automatic attempt.
    pub fn attempted(&self, name: &str) -> bool {
        self.tunnels.get(name).is_some_and(|s| s.attempted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expect() -> Expectation {
        Expectation {
            done: "Activated wg0".into(),
            state: "wg0 is up".into(),
            not_state: "wg0 is not up".into(),
            failed: "Couldn't activate wg0".into(),
        }
    }

    fn failed() -> HelperError {
        HelperError::Failed {
            command: "wg-helper up wg0".into(),
            stderr: "RTNETLINK answers: File exists".into(),
        }
    }

    #[test]
    fn operations_never_overlap_and_reconcile_after_finishing() {
        let now = Instant::now();
        let mut op = Operation::default();
        assert!(!op.is_busy());
        op.begin(OpKind::Connect, now).unwrap();
        assert!(op.is_busy());
        assert_eq!(op.begin(OpKind::Delete, now), Err(Busy(OpKind::Connect)));
        op.complete(Outcome::Unknown, now);
        assert!(matches!(
            op,
            Operation::Finished {
                kind: OpKind::Connect,
                outcome: Outcome::Unknown,
                ..
            }
        ));
        assert!(!op.is_busy());
        op.reconciled();
        assert_eq!(op, Operation::Idle);
        // Completing when nothing runs is ignored.
        op.complete(Outcome::Succeeded, now);
        assert_eq!(op, Operation::Idle);
        // A finished (not yet reconciled) operation does not block the next.
        op.begin(OpKind::Save, now).unwrap();
        op.complete(Outcome::Failed, now);
        op.begin(OpKind::Sync, now).unwrap();
        assert!(op.is_busy());
    }

    #[test]
    fn verified_success_and_reported_success_that_did_not_happen() {
        let done = settle(&Ok(()), Ok(true), &expect(), |e| e.to_string());
        assert_eq!(done.outcome, Outcome::Succeeded);
        assert_eq!(done.message, "Activated wg0");
        assert!(!done.popup);
        let lie = settle(&Ok(()), Ok(false), &expect(), |e| e.to_string());
        assert_eq!(lie.outcome, Outcome::Failed);
        assert!(lie.message.contains("re-checking shows wg0 is not up"));
        assert!(lie.popup);
    }

    #[test]
    fn timeouts_are_reconciled_from_the_observed_state() {
        let applied = settle(&Err(HelperError::Timeout), Ok(true), &expect(), |e| {
            e.to_string()
        });
        assert_eq!(applied.outcome, Outcome::Succeeded);
        assert!(
            applied
                .message
                .contains("timed out, but re-checking shows wg0 is up")
        );
        let not_applied = settle(&Err(HelperError::Timeout), Ok(false), &expect(), |e| {
            e.to_string()
        });
        assert_eq!(not_applied.outcome, Outcome::Failed);
        assert!(not_applied.message.contains("wg0 is not up"));
        let unknown = settle(
            &Err(HelperError::Timeout),
            Err(HelperError::Timeout),
            &expect(),
            |e| e.to_string(),
        );
        assert_eq!(unknown.outcome, Outcome::Unknown);
        assert!(unknown.message.contains("may or may not"));
    }

    #[test]
    fn errors_are_explained_and_contradictions_reported() {
        let error = settle(&Err(failed()), Ok(false), &expect(), |e| {
            format!("friendly: {e}")
        });
        assert_eq!(error.outcome, Outcome::Failed);
        assert!(
            error
                .message
                .starts_with("Couldn't activate wg0:\n\nfriendly:")
        );
        let contradiction = settle(&Err(failed()), Ok(true), &expect(), |e| e.to_string());
        assert_eq!(contradiction.outcome, Outcome::Succeeded);
        assert!(contradiction.popup);
        assert!(contradiction.message.contains("also reported an error"));
    }

    #[test]
    fn kill_switch_repair_runs_once_per_loss_with_backoff() {
        let mut watch = KillSwitchWatch::default();
        let start = Instant::now();
        let at = |secs| start + Duration::from_secs(secs);
        // Healthy: nothing to do.
        assert!(!watch.observe("wg0", true, &KillSwitch::Enabled, true, at(0)));
        // Loss detected: one attempt, then no more for the same loss.
        assert!(watch.observe("wg0", true, &KillSwitch::Missing, true, at(1)));
        for secs in 2..200 {
            assert!(!watch.observe("wg0", true, &KillSwitch::Missing, true, at(secs)));
        }
        // Repaired, then lost again long after the backoff window: new attempt.
        assert!(!watch.observe("wg0", true, &KillSwitch::Enabled, true, at(200)));
        assert!(watch.observe("wg0", true, &KillSwitch::Mismatch, true, at(201)));
        // Second loss right after a recent attempt: deferred by the backoff.
        let mut watch = KillSwitchWatch::default();
        assert!(watch.observe("wg0", true, &KillSwitch::Missing, true, at(0)));
        assert!(!watch.observe("wg0", true, &KillSwitch::Enabled, true, at(1)));
        assert!(!watch.observe("wg0", true, &KillSwitch::Missing, true, at(2)));
        assert!(watch.observe(
            "wg0",
            true,
            &KillSwitch::Missing,
            true,
            at(REPAIR_BACKOFF.as_secs())
        ));
        // Third loss: the window has doubled.
        assert!(!watch.observe("wg0", true, &KillSwitch::Enabled, true, at(20)));
        let second = REPAIR_BACKOFF.as_secs();
        assert!(!watch.observe(
            "wg0",
            true,
            &KillSwitch::Missing,
            true,
            at(second + REPAIR_BACKOFF.as_secs())
        ));
        assert!(watch.observe(
            "wg0",
            true,
            &KillSwitch::Missing,
            true,
            at(second + 2 * REPAIR_BACKOFF.as_secs())
        ));
    }

    #[test]
    fn kill_switch_repair_waits_for_idle_and_ignores_inactive_or_unknown() {
        let mut watch = KillSwitchWatch::default();
        let now = Instant::now();
        assert!(!watch.observe("wg0", false, &KillSwitch::Mismatch, true, now));
        assert!(!watch.observe("wg0", true, &KillSwitch::Unknown, true, now));
        assert!(!watch.observe("wg0", true, &KillSwitch::Missing, false, now));
        assert!(!watch.attempted("wg0"));
        assert!(watch.observe("wg0", true, &KillSwitch::Missing, true, now));
        assert!(watch.attempted("wg0"));
        // Unknown does not end the loss episode (no spinning on flaky reads).
        assert!(!watch.observe("wg0", true, &KillSwitch::Unknown, true, now));
        assert!(!watch.observe("wg0", true, &KillSwitch::Missing, true, now));
        // Other tunnels are tracked separately.
        assert!(watch.observe("wg1", true, &KillSwitch::Missing, true, now));
    }
}
