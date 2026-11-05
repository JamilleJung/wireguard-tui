//! Connection health per peer, derived from what WireGuard actually exposes.
//!
//! WireGuard only handshakes when there is something to send: an idle peer
//! without `PersistentKeepalive` can go hours without a handshake and still be
//! perfectly fine. A handshake older than 180 s (REJECT_AFTER_TIME) therefore
//! means "the session expired", not "the VPN is broken". Breakage shows up as
//! *sending without any answer*: tx grows while rx and the handshake do not.

use std::collections::HashMap;
use std::time::Instant;

/// WireGuard rejects a session older than this (seconds).
pub const REJECT_AFTER_TIME: u64 = 180;
/// WireGuard keeps retrying a handshake for this long after a send (seconds).
pub const REKEY_ATTEMPT_TIME: u64 = 90;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Health {
    /// Recent handshake: the session is valid.
    Healthy,
    /// Handshake attempts are in progress (just activated, or just started sending).
    Connecting,
    /// Up, no handshake yet, and nothing has been sent that would trigger one.
    NoTraffic,
    /// The last handshake is old because nothing needed to be sent. Normal.
    Idle,
    /// Traffic or keepalive should have renewed the handshake by now.
    HandshakeStale,
    /// Sending without any answer for longer than WireGuard retries.
    ConnectionFailed,
}

impl Health {
    pub fn label(self) -> &'static str {
        match self {
            Self::Healthy => "Healthy",
            Self::Connecting => "Connecting",
            Self::NoTraffic => "No traffic",
            Self::Idle => "Idle",
            Self::HandshakeStale => "Handshake stale",
            Self::ConnectionFailed => "Connection failed",
        }
    }

    pub fn explanation(self) -> &'static str {
        match self {
            Self::Healthy => "recent handshake",
            Self::Connecting => "handshake in progress",
            Self::NoTraffic => "no handshake yet - nothing has been sent",
            Self::Idle => "no recent traffic; WireGuard handshakes only when sending",
            Self::HandshakeStale => "handshake should have been renewed by now",
            Self::ConnectionFailed => "sending, but the peer is not answering",
        }
    }

    /// Whether this state indicates a real problem the user should look at.
    pub fn is_problem(self) -> bool {
        matches!(self, Self::HandshakeStale | Self::ConnectionFailed)
    }

    fn severity(self) -> u8 {
        match self {
            Self::Idle => 0,
            Self::NoTraffic => 1,
            Self::Healthy => 2,
            Self::Connecting => 3,
            Self::HandshakeStale => 4,
            Self::ConnectionFailed => 5,
        }
    }
}

/// Everything the classifier needs about one peer, in seconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HealthInput {
    /// Seconds since the latest handshake; `None` = never.
    pub handshake_age: Option<u64>,
    /// Bytes received / sent since the previous refresh.
    pub rx_delta: u64,
    pub tx_delta: u64,
    /// `PersistentKeepalive` interval, if set.
    pub keepalive: Option<u64>,
    /// Seconds since the interface was seen coming up; `None` = unknown
    /// (it was already up when the app started).
    pub since_activation: Option<u64>,
    /// Seconds we have been sending without any reply or new handshake.
    pub unanswered_for: Option<u64>,
}

/// Classify one peer. Pure: same input, same answer.
pub fn classify(input: HealthInput) -> Health {
    let sending = input.tx_delta > 0 || input.unanswered_for.is_some();
    if let Some(age) = input.handshake_age
        && age < REJECT_AFTER_TIME
    {
        return Health::Healthy;
    }
    if input
        .unanswered_for
        .is_some_and(|secs| secs >= REKEY_ATTEMPT_TIME)
    {
        return Health::ConnectionFailed;
    }
    let settled = input
        .since_activation
        .is_none_or(|secs| secs >= REKEY_ATTEMPT_TIME);
    match input.handshake_age {
        None => {
            if input.keepalive.is_some() && settled {
                // A keepalive sends regularly; no handshake at all means none
                // of those attempts were answered.
                Health::ConnectionFailed
            } else if sending || input.keepalive.is_some() {
                Health::Connecting
            } else {
                Health::NoTraffic
            }
        }
        Some(age) => {
            let keepalive_overdue = input
                .keepalive
                .is_some_and(|k| age >= REJECT_AFTER_TIME + k);
            if keepalive_overdue || sending {
                Health::HandshakeStale
            } else {
                Health::Idle
            }
        }
    }
}

/// The most severe state among peers (an empty list is `NoTraffic`).
pub fn summarize(states: &[Health]) -> Health {
    states
        .iter()
        .copied()
        .max_by_key(|h| h.severity())
        .unwrap_or(Health::NoTraffic)
}

/// One live sample of a peer's counters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeerSample {
    pub handshake_age: Option<u64>,
    pub rx: u64,
    pub tx: u64,
    pub keepalive: Option<u64>,
}

#[derive(Clone, Copy, Debug)]
struct PeerTrack {
    rx: u64,
    tx: u64,
    /// Handshake time in whole seconds relative to the tracker's base instant.
    handshake_at: Option<i64>,
    unanswered_since: Option<Instant>,
}

/// Remembers the previous sample of each peer so deltas and "sending without
/// an answer" can be measured between refreshes.
#[derive(Default)]
pub struct HealthTracker {
    peers: HashMap<(String, String), PeerTrack>,
    activated: HashMap<String, Instant>,
    base: Option<Instant>,
}

impl HealthTracker {
    /// Record that `tunnel` was seen going from down to up at `now`.
    pub fn mark_activated(&mut self, tunnel: &str, now: Instant) {
        self.activated.insert(tunnel.to_string(), now);
        self.peers.retain(|(t, _), _| t != tunnel);
    }

    pub fn mark_deactivated(&mut self, tunnel: &str) {
        self.activated.remove(tunnel);
        self.peers.retain(|(t, _), _| t != tunnel);
    }

    /// Fold in a new sample and classify the peer.
    pub fn observe(
        &mut self,
        tunnel: &str,
        peer: &str,
        sample: PeerSample,
        now: Instant,
    ) -> Health {
        let key = (tunnel.to_string(), peer.to_string());
        let base = *self.base.get_or_insert(now);
        let elapsed = now.saturating_duration_since(base).as_secs() as i64;
        let handshake_at = sample
            .handshake_age
            .map(|age| elapsed - age.min(i64::MAX as u64 / 2) as i64);
        let previous = self.peers.get(&key).copied();
        let (rx_delta, tx_delta) = previous
            .map(|p| {
                (
                    sample.rx.saturating_sub(p.rx),
                    sample.tx.saturating_sub(p.tx),
                )
            })
            .unwrap_or((0, 0));
        let renewed = match (previous.and_then(|p| p.handshake_at), handshake_at) {
            (Some(before), Some(after)) => after > before + 1,
            (None, Some(_)) => previous.is_some(),
            _ => false,
        };
        let mut unanswered_since = previous.and_then(|p| p.unanswered_since);
        if rx_delta > 0 || renewed || sample.rx < previous.map_or(0, |p| p.rx) {
            unanswered_since = None;
        } else if tx_delta > 0 && unanswered_since.is_none() {
            unanswered_since = Some(now);
        }
        self.peers.insert(
            key,
            PeerTrack {
                rx: sample.rx,
                tx: sample.tx,
                handshake_at,
                unanswered_since,
            },
        );
        classify(HealthInput {
            handshake_age: sample.handshake_age,
            rx_delta,
            tx_delta,
            keepalive: sample.keepalive,
            since_activation: self
                .activated
                .get(tunnel)
                .map(|at| now.saturating_duration_since(*at).as_secs()),
            unanswered_for: unanswered_since.map(|at| now.saturating_duration_since(at).as_secs()),
        })
    }
}

/// Whether to mention the clock as a *possible* cause. Only when handshakes
/// are actually failing and the clock is known to be unsynchronised.
pub fn clock_hint(health: Health, clock_synced: Option<bool>) -> bool {
    health.is_problem() && clock_synced == Some(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn input() -> HealthInput {
        HealthInput::default()
    }

    #[test]
    fn old_handshake_without_traffic_is_idle_not_broken() {
        let idle = classify(HealthInput {
            handshake_age: Some(3600),
            ..input()
        });
        assert_eq!(idle, Health::Idle);
        assert!(!idle.is_problem());
        assert_eq!(
            classify(HealthInput {
                handshake_age: Some(181),
                ..input()
            }),
            Health::Idle
        );
    }

    #[test]
    fn recent_handshake_is_healthy_regardless_of_counters() {
        for tx_delta in [0, 10_000] {
            assert_eq!(
                classify(HealthInput {
                    handshake_age: Some(5),
                    tx_delta,
                    unanswered_for: Some(500),
                    ..input()
                }),
                Health::Healthy
            );
        }
    }

    #[test]
    fn never_handshaked_depends_on_traffic_keepalive_and_time() {
        assert_eq!(classify(input()), Health::NoTraffic);
        assert_eq!(
            classify(HealthInput {
                tx_delta: 148,
                since_activation: Some(3),
                unanswered_for: Some(0),
                ..input()
            }),
            Health::Connecting
        );
        assert_eq!(
            classify(HealthInput {
                keepalive: Some(25),
                since_activation: Some(10),
                ..input()
            }),
            Health::Connecting
        );
        assert_eq!(
            classify(HealthInput {
                keepalive: Some(25),
                since_activation: Some(120),
                ..input()
            }),
            Health::ConnectionFailed
        );
        assert_eq!(
            classify(HealthInput {
                tx_delta: 148,
                unanswered_for: Some(REKEY_ATTEMPT_TIME),
                ..input()
            }),
            Health::ConnectionFailed
        );
    }

    #[test]
    fn expired_session_with_traffic_or_keepalive_is_stale_then_failed() {
        assert_eq!(
            classify(HealthInput {
                handshake_age: Some(400),
                tx_delta: 96,
                unanswered_for: Some(2),
                ..input()
            }),
            Health::HandshakeStale
        );
        assert_eq!(
            classify(HealthInput {
                handshake_age: Some(400),
                unanswered_for: Some(95),
                ..input()
            }),
            Health::ConnectionFailed
        );
        assert_eq!(
            classify(HealthInput {
                handshake_age: Some(REJECT_AFTER_TIME + 25),
                keepalive: Some(25),
                ..input()
            }),
            Health::HandshakeStale
        );
        assert_eq!(
            classify(HealthInput {
                handshake_age: Some(REJECT_AFTER_TIME + 10),
                keepalive: Some(25),
                ..input()
            }),
            Health::Idle
        );
    }

    #[test]
    fn summary_picks_the_most_severe_peer() {
        assert_eq!(
            summarize(&[Health::Healthy, Health::Idle, Health::ConnectionFailed]),
            Health::ConnectionFailed
        );
        assert_eq!(summarize(&[Health::Idle, Health::Healthy]), Health::Healthy);
        assert_eq!(summarize(&[]), Health::NoTraffic);
    }

    #[test]
    fn tracker_measures_unanswered_sending_per_peer() {
        let mut tracker = HealthTracker::default();
        let start = Instant::now();
        let sample = |tx, rx, age| PeerSample {
            handshake_age: age,
            rx,
            tx,
            keepalive: None,
        };
        // Peer A: old handshake, starts sending, never answered.
        assert_eq!(
            tracker.observe("wg0", "A", sample(100, 50, Some(600)), start),
            Health::Idle
        );
        let later = start + Duration::from_secs(2);
        assert_eq!(
            tracker.observe("wg0", "A", sample(200, 50, Some(602)), later),
            Health::HandshakeStale
        );
        let much_later = start + Duration::from_secs(2 + REKEY_ATTEMPT_TIME);
        assert_eq!(
            tracker.observe("wg0", "A", sample(300, 50, Some(692)), much_later),
            Health::ConnectionFailed
        );
        // Peer B on the same tunnel is unaffected and idle.
        assert_eq!(
            tracker.observe("wg0", "B", sample(10, 10, Some(900)), much_later),
            Health::Idle
        );
        // An answer (fresh handshake) clears the failure.
        let answered = much_later + Duration::from_secs(2);
        assert_eq!(
            tracker.observe("wg0", "A", sample(400, 148, Some(1)), answered),
            Health::Healthy
        );
    }

    #[test]
    fn tracker_uses_activation_time_for_the_connecting_grace() {
        let mut tracker = HealthTracker::default();
        let start = Instant::now();
        tracker.mark_activated("wg0", start);
        let keepalive = PeerSample {
            handshake_age: None,
            rx: 0,
            tx: 0,
            keepalive: Some(25),
        };
        assert_eq!(
            tracker.observe("wg0", "A", keepalive, start + Duration::from_secs(5)),
            Health::Connecting
        );
        assert_eq!(
            tracker.observe("wg0", "A", keepalive, start + Duration::from_secs(100)),
            Health::ConnectionFailed
        );
    }

    #[test]
    fn clock_is_only_a_possible_cause_when_handshakes_fail() {
        assert!(!clock_hint(Health::Idle, Some(false)));
        assert!(!clock_hint(Health::NoTraffic, Some(false)));
        assert!(!clock_hint(Health::ConnectionFailed, Some(true)));
        assert!(!clock_hint(Health::ConnectionFailed, None));
        assert!(clock_hint(Health::ConnectionFailed, Some(false)));
        assert!(clock_hint(Health::HandshakeStale, Some(false)));
    }
}
