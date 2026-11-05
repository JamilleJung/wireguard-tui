//! Connection diagnostics: a short checklist for the selected tunnel.
//!
//! It samples the tunnel twice a couple of seconds apart, so the same health
//! classification the UI uses can see whether traffic is flowing. A stale
//! handshake on an idle tunnel is reported as normal, and the system clock is
//! only named as a *possible* cause when handshakes are actually failing.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::health::{self, Health, HealthTracker};
use super::{Detail, fmt_ago};

fn diag_line(s: &mut String, mark: &str, label: &str, detail: &str) {
    s.push_str(&format!("  {mark} {label:<12} {detail}\n"));
}

pub(crate) fn endpoint_host(ep: &str) -> String {
    if let Some(rest) = ep.strip_prefix('[')
        && let Some((h, _)) = rest.split_once(']')
    {
        return h.to_string();
    }
    ep.rsplit_once(':')
        .map(|(h, _)| h.to_string())
        .unwrap_or_else(|| ep.to_string())
}

fn ping_host(host: &str) -> Option<bool> {
    let st = Command::new("ping")
        .args(["-c", "1", "-W", "2", host])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok()?;
    Some(st.success())
}

fn dns_ok() -> bool {
    use std::net::ToSocketAddrs;
    ("one.one.one.one", 443)
        .to_socket_addrs()
        .map(|mut a| a.next().is_some())
        .unwrap_or(false)
}

/// What the checklist found, gathered before formatting.
pub struct Findings {
    pub name: String,
    pub clock_synced: Option<bool>,
    pub active: bool,
    /// (peer label, health, seconds since its last handshake)
    pub peers: Vec<(String, Health, Option<u64>)>,
    /// (endpoint host, ping answered)
    pub endpoint: Option<(String, Option<bool>)>,
    pub dns_ok: bool,
}

/// Run a connection-health checklist for `name` and return a formatted report.
/// Safe on a background thread (pings / resolves / shells out).
pub fn diagnose_report(name: &str) -> String {
    let clock_synced = crate::doctor::clock_synced();
    let first = super::get_detail(name);
    if !first.active {
        return format_report(&Findings {
            name: name.to_string(),
            clock_synced,
            active: false,
            peers: Vec::new(),
            endpoint: None,
            dns_ok: true,
        });
    }
    let mut tracker = HealthTracker::default();
    let start = Instant::now();
    observe(&mut tracker, &first, start);
    std::thread::sleep(Duration::from_secs(2));
    let second = super::get_detail(name);
    let peers = observe(&mut tracker, &second, start + Duration::from_secs(2));
    let endpoint = second
        .peers
        .iter()
        .map(|p| p.endpoint.clone())
        .find(|e| !e.is_empty())
        .map(|ep| {
            let host = endpoint_host(&ep);
            let reachable = ping_host(&host);
            (host, reachable)
        });
    format_report(&Findings {
        name: name.to_string(),
        clock_synced,
        active: second.active,
        peers,
        endpoint,
        dns_ok: dns_ok(),
    })
}

fn observe(
    tracker: &mut HealthTracker,
    detail: &Detail,
    now: Instant,
) -> Vec<(String, Health, Option<u64>)> {
    detail
        .peers
        .iter()
        .enumerate()
        .map(|(i, peer)| {
            let health = tracker.observe(&detail.name, &peer.public_key, peer.sample(), now);
            (format!("Peer {}", i + 1), health, peer.handshake_age)
        })
        .collect()
}

/// Format the checklist. Pure, so the wording rules are unit-tested.
pub fn format_report(f: &Findings) -> String {
    let mut s = format!("Connection check for {}\n\n", f.name);
    let summary = health::summarize(&f.peers.iter().map(|(_, h, _)| *h).collect::<Vec<_>>());
    let failing = f.active && !f.peers.is_empty() && summary.is_problem();
    match f.clock_synced {
        Some(true) => diag_line(&mut s, "[OK]", "Clock", "system clock is NTP-synced"),
        Some(false) if failing => {
            diag_line(&mut s, "[!]", "Clock", "NOT NTP-synced");
            s.push_str(
                "       -> a wrong clock is one possible cause of the failing handshake (the server's anti-replay check); enable NTP/chrony\n",
            );
        }
        Some(false) => {
            diag_line(&mut s, "[!]", "Clock", "NOT NTP-synced");
            s.push_str("       -> not causing a problem right now; enabling NTP/chrony avoids future handshake rejections\n");
        }
        None => diag_line(&mut s, "[?]", "Clock", "could not read clock-sync status"),
    }
    if !f.active {
        diag_line(
            &mut s,
            "[!]",
            "Tunnel",
            "not active — activate it first, then re-run",
        );
        return s;
    }
    diag_line(&mut s, "[OK]", "Tunnel", &format!("{} is up", f.name));
    let newest = f.peers.iter().filter_map(|(_, _, age)| *age).min();
    let ago = newest.map(fmt_ago).unwrap_or_else(|| "never".into());
    match summary {
        _ if f.peers.is_empty() => {
            diag_line(&mut s, "[?]", "Handshake", "this tunnel has no peers")
        }
        Health::Healthy => diag_line(
            &mut s,
            "[OK]",
            "Handshake",
            &format!("last handshake {ago} ago"),
        ),
        Health::Idle => diag_line(
            &mut s,
            "[OK]",
            "Handshake",
            &format!(
                "idle — last handshake {ago} ago; WireGuard only handshakes when there is traffic"
            ),
        ),
        Health::NoTraffic => diag_line(
            &mut s,
            "[?]",
            "Handshake",
            "none yet — nothing has been sent; send some traffic (e.g. ping an address behind the tunnel) to test",
        ),
        Health::Connecting => diag_line(
            &mut s,
            "[..]",
            "Handshake",
            "in progress — re-run in a few seconds",
        ),
        Health::HandshakeStale | Health::ConnectionFailed => {
            diag_line(
                &mut s,
                "[X]",
                "Handshake",
                &format!(
                    "{} (last {ago}) — up but NOT actually connected",
                    summary.label()
                ),
            );
            let mut causes = String::from(
                "       -> possible causes: endpoint unreachable or UDP blocked; the server has not accepted this peer's key/preshared key",
            );
            if f.clock_synced == Some(false) {
                causes.push_str("; the clock is not NTP-synced");
            }
            s.push_str(&causes);
            s.push('\n');
        }
    }
    if f.peers.len() > 1 {
        for (label, health, _) in &f.peers {
            s.push_str(&format!("       {label}: {}\n", health.label()));
        }
    }
    match &f.endpoint {
        Some((host, Some(true))) => {
            diag_line(&mut s, "[OK]", "Endpoint", &format!("{host} is reachable"))
        }
        Some((host, Some(false))) => {
            diag_line(
                &mut s,
                "[!]",
                "Endpoint",
                &format!("{host} did not answer ping"),
            );
            s.push_str(
                "       -> server may block ICMP (handshake can still work) or the host is down\n",
            );
        }
        Some((_, None)) => diag_line(&mut s, "[?]", "Endpoint", "could not test the endpoint"),
        None => diag_line(&mut s, "[?]", "Endpoint", "no endpoint set in this config"),
    }
    if f.dns_ok {
        diag_line(&mut s, "[OK]", "DNS", "name resolution works");
    } else {
        diag_line(
            &mut s,
            "[!]",
            "DNS",
            "DNS lookup failed — check the tunnel's DNS = line / resolvconf",
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn findings(health: Health, clock: Option<bool>) -> Findings {
        Findings {
            name: "wg0".into(),
            clock_synced: clock,
            active: true,
            peers: vec![("Peer 1".into(), health, Some(900))],
            endpoint: Some(("vpn.example.com".into(), Some(true))),
            dns_ok: true,
        }
    }

    #[test]
    fn idle_tunnel_with_unsynced_clock_is_not_blamed_on_ntp() {
        let report = format_report(&findings(Health::Idle, Some(false)));
        assert!(report.contains("idle"), "{report}");
        assert!(!report.contains("NOT actually connected"), "{report}");
        assert!(!report.contains("possible cause"), "{report}");
        assert!(report.contains("not causing a problem"), "{report}");
    }

    #[test]
    fn failing_handshake_names_the_clock_only_as_a_possible_cause() {
        let report = format_report(&findings(Health::ConnectionFailed, Some(false)));
        assert!(report.contains("NOT actually connected"), "{report}");
        assert!(report.contains("one possible cause"), "{report}");
        assert!(!report.contains("likely"), "{report}");
        let synced = format_report(&findings(Health::ConnectionFailed, Some(true)));
        assert!(!synced.contains("NTP-synced;"), "{synced}");
        assert!(!synced.contains("clock is not"), "{synced}");
    }

    #[test]
    fn multi_peer_reports_each_peer() {
        let mut f = findings(Health::Healthy, Some(true));
        f.peers
            .push(("Peer 2".into(), Health::ConnectionFailed, None));
        let report = format_report(&f);
        assert!(report.contains("Peer 1: Healthy"), "{report}");
        assert!(report.contains("Peer 2: Connection failed"), "{report}");
    }

    #[test]
    fn endpoint_hosts_strip_ports_and_brackets() {
        assert_eq!(endpoint_host("[::1]:51820"), "::1");
        assert_eq!(endpoint_host("vpn.example.com:51820"), "vpn.example.com");
        assert_eq!(endpoint_host("host"), "host");
    }
}
