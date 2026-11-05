// This is the shared WireGuard backend (kept in step with the desktop client).
// Not every frontend exercises every helper, so unused ones are expected here.
#![allow(dead_code)]
//! Talks to WireGuard through the privileged `wg-helper` binary.
//!
//! Submodules:
//!   * [`helper`]      - running the helper/`wg`, typed [`HelperError`]
//!   * [`backup`]      - ZIP export, backups, restore with rollback
//!   * [`qr`]          - QR-code image import
//!   * [`health`]      - per-peer connection health classification
//!   * [`diagnostics`] - the connection checklist (`D`)
//!
//! This file holds the data model handed to the UI and the tunnel API.

pub mod backup;
pub mod diagnostics;
pub mod health;
pub mod helper;
pub mod qr;

use std::time::{SystemTime, UNIX_EPOCH};

pub use backup::{
    BackupInfo, create_backup, delete_backup, export_backup_to, export_zip, fmt_time, list_backups,
    restore_backup, save_text_to,
};
pub use diagnostics::diagnose_report;
pub use health::Health;
pub use helper::{HelperError, helper_path, init};
pub use qr::decode_qr;

use crate::{config, secrets, validation};
use helper::helper;

// ---------------------------------------------------------------------------
// Data model handed up to the UI layer.
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tunnel {
    pub name: String,
    pub active: bool,
}

#[derive(Default, Clone, Debug)]
pub struct Peer {
    pub public_key: String,
    pub preshared: bool,
    pub allowed_ips: String,
    pub endpoint: String,
    pub keepalive: String,
    pub latest_handshake: String,
    pub transfer: String,
    /// Live numbers for health classification (zero/None when not running).
    pub handshake_age: Option<u64>,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub keepalive_secs: Option<u64>,
}

impl Peer {
    pub fn sample(&self) -> health::PeerSample {
        health::PeerSample {
            handshake_age: self.handshake_age,
            rx: self.rx_bytes,
            tx: self.tx_bytes,
            keepalive: self.keepalive_secs,
        }
    }
}

/// The helper's view of a tunnel's firewall kill switch.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum KillSwitch {
    Enabled,
    Disabled,
    /// Turned on for this tunnel, but its firewall rules are gone or
    /// incomplete (e.g. after a firewall reload).
    Missing,
    /// Rules are present but no longer match the tunnel's runtime fwmark, or
    /// the interface is down.
    Mismatch,
    /// The status could not be read or was not understood. Never treated as off.
    #[default]
    Unknown,
}

impl KillSwitch {
    pub fn parse(output: &str) -> Self {
        match output.trim() {
            "enabled" => Self::Enabled,
            "disabled" => Self::Disabled,
            "missing" => Self::Missing,
            "mismatch" => Self::Mismatch,
            _ => Self::Unknown,
        }
    }

    /// Protection was requested but is not in force.
    pub fn is_broken(&self) -> bool {
        matches!(self, Self::Missing | Self::Mismatch)
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Enabled => "On",
            Self::Disabled => "Off",
            Self::Missing => "MISSING - firewall rules are gone",
            Self::Mismatch => "MISMATCH - rules no longer match the tunnel",
            Self::Unknown => "Unknown (status could not be read)",
        }
    }
}

#[derive(Default, Clone, Debug)]
pub struct Detail {
    pub name: String,
    pub active: bool,
    pub autostart: bool,
    pub killswitch: KillSwitch,
    pub public_key: String,
    pub listen_port: String,
    pub addresses: String,
    pub dns: String,
    pub peers: Vec<Peer>,
    /// Live interface totals (summed across peers), for computing throughput.
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    /// Seconds since the most recent handshake (None if inactive / never).
    pub handshake_age: Option<u64>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// List tunnels, surfacing a helper failure so the UI can tell "no tunnels"
/// apart from "couldn't reach the helper / permission denied".
pub fn saved_tunnel_names() -> Result<Vec<String>, HelperError> {
    let output = helper(&["list"], None)?;
    let mut seen = std::collections::HashSet::new();
    let mut names = Vec::new();
    for name in output.lines().filter(|name| !name.is_empty()) {
        validate_tunnel_name(name).map_err(HelperError::Io)?;
        if !seen.insert(name) {
            return Err(HelperError::Io(
                "wg-helper returned duplicate tunnel names.".into(),
            ));
        }
        names.push(name.to_string());
    }
    Ok(names)
}

/// Names of the interfaces that are currently up.
pub fn active_names() -> Result<std::collections::HashSet<String>, HelperError> {
    Ok(helper(&["active"], None)?
        .split_whitespace()
        .map(str::to_string)
        .collect())
}

pub fn try_list_tunnels() -> Result<Vec<Tunnel>, HelperError> {
    let names = saved_tunnel_names()?;
    let active = active_names()?;
    Ok(names
        .into_iter()
        .map(|name| Tunnel {
            active: active.contains(&name),
            name,
        })
        .collect())
}

pub fn tunnel_exists(name: &str) -> Result<bool, HelperError> {
    Ok(saved_tunnel_names()?
        .iter()
        .any(|existing| existing == name))
}

/// A collision-free tunnel name based on `base`: returns `base`, else
/// `base-2`, `base-3`, … (kept within the 15-char interface-name limit).
pub fn unique_name(base: &str) -> Result<String, String> {
    let existing: std::collections::HashSet<_> = saved_tunnel_names()?.into_iter().collect();
    unique_name_with(base, |name| existing.contains(name))
}

fn unique_name_with<F>(base: &str, exists: F) -> Result<String, String>
where
    F: Fn(&str) -> bool,
{
    let base = sanitize_name(base);
    if !exists(&base) {
        return Ok(base);
    }
    for n in 2..=10000 {
        let suffix = format!("-{n}");
        let keep = 15usize.saturating_sub(suffix.len());
        let candidate = format!("{}{}", base.chars().take(keep).collect::<String>(), suffix);
        if !exists(&candidate) {
            return Ok(candidate);
        }
    }
    Err("No unused tunnel name could be found; choose a different name.".into())
}

pub fn read_config(name: &str) -> Result<String, HelperError> {
    helper(&["read", name], None)
}

pub fn validate_tunnel_name(name: &str) -> Result<(), String> {
    validation::validate_tunnel_name(name)
}

pub fn save_config(name: &str, content: &str) -> Result<(), HelperError> {
    helper(&["save", name], Some(content)).map(|_| ())
}

pub fn rename_config(old: &str, new: &str, content: &str) -> Result<(), HelperError> {
    helper(&["rename", old, new], Some(content)).map(|_| ())
}

pub fn activate(name: &str) -> Result<(), HelperError> {
    helper(&["up", name], None).map(|_| ())
}

pub fn deactivate(name: &str) -> Result<(), HelperError> {
    helper(&["down", name], None).map(|_| ())
}

pub fn delete(name: &str) -> Result<(), HelperError> {
    helper(&["delete", name], None).map(|_| ())
}

/// Recent WireGuard-related log lines (this app's audit log + wg-quick units).
pub fn get_log() -> String {
    match helper(&["log"], None) {
        Ok(s) if !s.trim().is_empty() => secrets::redact_config(&s),
        Ok(_) => "(no recent log entries)".to_string(),
        Err(e) => format!("Could not read the log: {e}"),
    }
}

/// Build the full detail view for a tunnel by merging its on-disk config with
/// the live `wg show <name> dump` output.
pub fn get_detail(name: &str) -> Detail {
    let cfg = read_config(name).unwrap_or_default();
    let parsed = config::parse_config(&cfg);
    let dump = helper(&["dump", name], None).unwrap_or_default();
    let live = parse_dump(&dump);

    // Active: use the active interfaces list (fast, reliable), fall back to dump.
    let active = active_names()
        .map(|names| names.contains(name))
        .unwrap_or(!dump.trim().is_empty());

    // Interface public key: prefer the live value, else derive from privkey.
    let public_key = live
        .as_ref()
        .map(|l| l.iface_public.clone())
        .filter(|s| !s.is_empty())
        .or_else(|| parsed.private_key.as_deref().and_then(pubkey_of))
        .unwrap_or_default();

    let listen_port = live
        .as_ref()
        .map(|l| l.listen_port.clone())
        .filter(|s| !s.is_empty())
        .or_else(|| parsed.listen_port.clone())
        .unwrap_or_default();

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let peers = merge_peers(parsed.peers, live.as_ref(), now);
    let (rx_bytes, tx_bytes, handshake_age) = live_totals(live.as_ref(), now);

    Detail {
        name: name.to_string(),
        active,
        autostart: is_autostart(name),
        killswitch: killswitch_status(name),
        public_key,
        listen_port,
        addresses: parsed.address.unwrap_or_default(),
        dns: parsed.dns.unwrap_or_default(),
        peers,
        rx_bytes,
        tx_bytes,
        handshake_age,
    }
}

/// `wg pubkey` is pure crypto and needs no privilege.
fn pubkey_of(private_key: &str) -> Option<String> {
    if !config::is_wg_key(private_key) {
        return None;
    }
    let output = helper::wg_output("pubkey", Some(format!("{private_key}\n"))).ok()?;
    output
        .status
        .success()
        .then(|| key_output(&output.stdout))
        .flatten()
}

// ---------------------------------------------------------------------------
// Live status parsing
// ---------------------------------------------------------------------------

struct LivePeer {
    public_key: String,
    preshared: bool,
    endpoint: String,
    allowed_ips: String,
    keepalive: String,
    latest_handshake: u64,
    rx: u64,
    tx: u64,
}

struct LiveDump {
    iface_public: String,
    listen_port: String,
    peers: Vec<LivePeer>,
}

/// `wg show <iface> dump`:
///   line 1 (interface): private-key  public-key  listen-port  fwmark
///   line N (peer):      public-key  preshared-key  endpoint  allowed-ips
///                       latest-handshake  rx  tx  persistent-keepalive
fn parse_dump(text: &str) -> Option<LiveDump> {
    let mut lines = text.lines();
    let fields: Vec<_> = lines.next()?.split('\t').collect();
    if fields.len() != 4 || fields[2].parse::<u16>().is_err() {
        return None;
    }
    let mut dump = LiveDump {
        iface_public: if fields[1] == "(none)" {
            String::new()
        } else {
            fields[1].to_string()
        },
        listen_port: fields[2].to_string(),
        peers: Vec::new(),
    };
    for line in lines {
        let p: Vec<_> = line.split('\t').collect();
        if p.len() != 8 || !config::is_wg_key(p[0]) {
            continue;
        }
        let (Ok(latest_handshake), Ok(rx), Ok(tx)) = (p[4].parse(), p[5].parse(), p[6].parse())
        else {
            continue;
        };
        if p[7] != "off" && p[7].parse::<u16>().is_err() {
            continue;
        }
        dump.peers.push(LivePeer {
            public_key: p[0].to_string(),
            preshared: p[1] != "(none)",
            endpoint: if p[2] == "(none)" {
                String::new()
            } else {
                p[2].to_string()
            },
            allowed_ips: if p[3] == "(none)" {
                String::new()
            } else {
                p[3].to_string()
            },
            keepalive: p[7].to_string(),
            latest_handshake,
            rx,
            tx,
        });
    }
    Some(dump)
}

fn keepalive_secs(text: &str) -> Option<u64> {
    text.trim().parse::<u64>().ok().filter(|secs| *secs > 0)
}

fn handshake_age(epoch: u64, now: u64) -> Option<u64> {
    (epoch != 0 && epoch <= now).then(|| now - epoch)
}

fn live_peer(lp: &LivePeer, now: u64) -> Peer {
    Peer {
        public_key: lp.public_key.clone(),
        preshared: lp.preshared,
        allowed_ips: lp.allowed_ips.clone(),
        endpoint: lp.endpoint.clone(),
        keepalive: lp.keepalive.clone(),
        latest_handshake: fmt_handshake(lp.latest_handshake),
        transfer: fmt_transfer(lp.rx, lp.tx),
        handshake_age: handshake_age(lp.latest_handshake, now),
        rx_bytes: lp.rx,
        tx_bytes: lp.tx,
        keepalive_secs: keepalive_secs(&lp.keepalive),
    }
}

fn merge_peers(saved: Vec<config::ParsedPeer>, live: Option<&LiveDump>, now: u64) -> Vec<Peer> {
    let mut peers = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for p in saved {
        seen.insert(p.public_key.clone());
        let lp = live.and_then(|dump| dump.peers.iter().find(|v| v.public_key == p.public_key));
        match lp {
            Some(lp) => {
                let mut peer = live_peer(lp, now);
                if peer.endpoint.is_empty() {
                    peer.endpoint = p.endpoint;
                }
                peers.push(peer);
            }
            None => peers.push(Peer {
                keepalive_secs: keepalive_secs(&p.keepalive),
                public_key: p.public_key,
                preshared: !p.preshared_key.is_empty(),
                allowed_ips: p.allowed_ips,
                endpoint: p.endpoint,
                keepalive: p.keepalive,
                ..Peer::default()
            }),
        }
    }
    if let Some(dump) = live {
        for p in &dump.peers {
            if seen.insert(p.public_key.clone()) {
                peers.push(live_peer(p, now));
            }
        }
    }
    peers
}

fn live_totals(live: Option<&LiveDump>, now: u64) -> (u64, u64, Option<u64>) {
    let Some(live) = live else {
        return (0, 0, None);
    };
    let (rx, tx) = live.peers.iter().fold((0u64, 0u64), |(rx, tx), p| {
        (rx.saturating_add(p.rx), tx.saturating_add(p.tx))
    });
    let latest = live
        .peers
        .iter()
        .map(|p| p.latest_handshake)
        .max()
        .unwrap_or(0);
    (rx, tx, handshake_age(latest, now))
}

// ---------------------------------------------------------------------------
// Formatting helpers
// ---------------------------------------------------------------------------

fn fmt_handshake(epoch: u64) -> String {
    if epoch == 0 {
        return String::new();
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(epoch);
    if epoch > now {
        return "in the future (check system clock)".to_string();
    }
    let secs = now - epoch;
    match secs {
        0 => "now".to_string(),
        1 => "1 second ago".to_string(),
        s if s < 60 => format!("{s} seconds ago"),
        s if s < 120 => "1 minute ago".to_string(),
        s if s < 3600 => format!("{} minutes ago", s / 60),
        s if s < 7200 => "1 hour ago".to_string(),
        s if s < 86400 => format!("{} hours ago", s / 3600),
        s => format!("{} days ago", s / 86400),
    }
}

/// Compact "seconds ago" → "12s" / "3m" / "2h".
pub fn fmt_ago(s: u64) -> String {
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m", s / 60)
    } else {
        format!("{}h", s / 3600)
    }
}

pub fn fmt_bytes(b: u64) -> String {
    if b < 1024 {
        return format!("{b} B");
    }
    // Closed-form unit pick instead of an if/else ladder: the unit index is
    // floor(log2(b) / 10) — i.e. how many whole times 1024 divides b. The
    // high-bit position is `63 - leading_zeros` (one CPU instruction), and the
    // 1024-power thresholds are exact in f64, so this is byte-for-byte identical
    // to the ladder while collapsing five format! sites into one. Clamped to TiB.
    const UNITS: [&str; 4] = ["KiB", "MiB", "GiB", "TiB"];
    let exp = (((63 - b.leading_zeros()) / 10) as usize).min(4);
    let value = b as f64 / 1024f64.powi(exp as i32);
    format!("{value:.2} {}", UNITS[exp - 1])
}

/// Human-readable size, e.g. "12.3 KiB".
pub fn fmt_size(bytes: u64) -> String {
    fmt_bytes(bytes)
}

fn fmt_transfer(rx: u64, tx: u64) -> String {
    if rx == 0 && tx == 0 {
        return String::new();
    }
    format!("{} received, {} sent", fmt_bytes(rx), fmt_bytes(tx))
}

pub fn validate_config(text: &str) -> Result<(), String> {
    config::validate_basic_wireguard_config(text)
}

/// Generate a fresh WireGuard keypair via `wg genkey` / `wg pubkey` (no root).
pub fn generate_keypair() -> Result<(String, String), String> {
    let genkey = helper::wg_output("genkey", None)?;
    if !genkey.status.success() {
        return Err("wg genkey failed".into());
    }
    let private =
        key_output(&genkey.stdout).ok_or("wg genkey returned an invalid WireGuard key")?;
    let public = pubkey_of(&private).ok_or("wg pubkey failed")?;
    Ok((private, public))
}

/// Generate a fresh preshared key via `wg genpsk` (no root).
pub fn generate_psk() -> Result<String, String> {
    let out = helper::wg_output("genpsk", None)?;
    if !out.status.success() {
        return Err("wg genpsk failed".into());
    }
    key_output(&out.stdout).ok_or("wg genpsk returned an invalid WireGuard key".into())
}

fn key_output(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?.trim();
    config::is_wg_key(text).then(|| text.to_string())
}

/// Apply the saved config to a RUNNING tunnel without dropping sessions
/// (`wg syncconf`). Only wg-level fields take effect; Address/DNS/MTU/Table
/// changes still need a full reconnect. The helper refuses (mentioning
/// "reconnect") when the saved config changes the FwMark.
pub fn sync_running(name: &str) -> Result<(), HelperError> {
    helper(&["sync", name], None).map(|_| ())
}

/// The live running wg-level config (`wg showconf`).
pub fn running_config(name: &str) -> Result<String, HelperError> {
    helper(&["showconf", name], None)
}

/// Save the live running state back to the `.conf` (`wg-quick save`).
pub fn persist_live(name: &str) -> Result<(), HelperError> {
    helper(&["persist", name], None).map(|_| ())
}

/// All tunnel configs as (filename, contents), for export.
pub fn read_all_configs() -> Result<Vec<(String, String)>, String> {
    backup::collect_configs(saved_tunnel_names()?, |name| {
        read_config(name).map_err(String::from)
    })
}

/// Whether `wg-quick@<name>` is enabled to start on boot.
pub fn autostart_status(name: &str) -> Result<bool, HelperError> {
    helper(&["is-enabled", name], None).map(|s| s.trim() == "enabled")
}

pub fn is_autostart(name: &str) -> bool {
    autostart_status(name).unwrap_or(false)
}

/// Enable/disable starting the tunnel on boot.
pub fn set_autostart(name: &str, on: bool) -> Result<(), HelperError> {
    helper(&[if on { "enable" } else { "disable" }, name], None).map(|_| ())
}

/// The helper's kill switch status for this tunnel. A failed query is
/// `Unknown`, never "off".
pub fn killswitch_status(name: &str) -> KillSwitch {
    helper(&["killswitch-status", name], None)
        .map(|s| KillSwitch::parse(&s))
        .unwrap_or(KillSwitch::Unknown)
}

/// Enable/disable the helper-installed firewall kill switch. This is intentionally
/// not persistent: it uses standard firewall rules and no daemon.
pub fn set_killswitch(name: &str, on: bool) -> Result<(), HelperError> {
    helper(
        &[
            if on {
                "killswitch-enable"
            } else {
                "killswitch-disable"
            },
            name,
        ],
        None,
    )
    .map(|_| ())
}

/// Reinstall the kill switch for `name` from the live interface.
pub fn repair_killswitch(name: &str) -> Result<(), HelperError> {
    helper(&["killswitch-repair", name], None).map(|_| ())
}

/// True if the config contains directives that `wg-quick` runs as **root** on
/// activation (`PostUp`/`PreUp`/`PostDown`/`PreDown`). Used to warn the user
/// before they save/activate a config from an untrusted source.
pub fn config_runs_scripts(text: &str) -> bool {
    secrets::config_runs_scripts(text)
}

/// Make a safe tunnel/interface name from an imported file name. The result
/// satisfies the helper's rule (starts with an alphanumeric, then
/// alphanumeric/_/-/., max 15 chars): truncate first, then trim the ends so a
/// cut can't re-introduce a trailing dot or a non-alphanumeric leading char.
pub fn sanitize_name(file_stem: &str) -> String {
    validation::sanitize_import_name(file_stem)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{fake_helper, scratch_dir};
    use std::time::Duration;

    // A syntactically valid 44-char base64 WireGuard key (43 chars + '=').
    const KEY: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopo=";

    #[test]
    fn offline_backup_and_name_checks_survive_a_failed_active_query() {
        let dir = scratch_dir("backend-offline");
        let program = fake_helper(
            &dir,
            &format!(
                "case \"$1\" in\nlist) printf 'wg0\\n';;\nactive) echo 'active query failed' >&2; exit 2;;\nread) printf '[Interface]\\nPrivateKey = {KEY}\\n';;\n*) exit 2;;\nesac"
            ),
        );
        helper::with_fake_helper(&program, Duration::from_secs(5), || {
            assert!(try_list_tunnels().is_err());
            assert_eq!(unique_name("wg0").unwrap(), "wg0-2");
            assert_eq!(read_all_configs().unwrap().len(), 1);
            let dest = dir.join("offline-backup.zip");
            assert_eq!(export_zip(&dest).unwrap(), 1);
            assert!(dest.is_file());
        });
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn killswitch_status_never_reads_failures_or_unknown_text_as_off() {
        assert_eq!(KillSwitch::parse("enabled\n"), KillSwitch::Enabled);
        assert_eq!(KillSwitch::parse("disabled"), KillSwitch::Disabled);
        assert_eq!(KillSwitch::parse("missing"), KillSwitch::Missing);
        assert_eq!(KillSwitch::parse("mismatch"), KillSwitch::Mismatch);
        assert_eq!(KillSwitch::parse("something new"), KillSwitch::Unknown);
        assert_eq!(KillSwitch::parse(""), KillSwitch::Unknown);
        assert!(KillSwitch::Missing.is_broken() && KillSwitch::Mismatch.is_broken());
        assert!(!KillSwitch::Unknown.is_broken());
        let dir = scratch_dir("backend-killswitch");
        let program = fake_helper(
            &dir,
            "case \"$2\" in\nlost) echo missing;;\nmoved) echo mismatch;;\n*) echo 'denied' >&2; exit 2;;\nesac",
        );
        helper::with_fake_helper(&program, Duration::from_secs(5), || {
            assert_eq!(killswitch_status("lost"), KillSwitch::Missing);
            assert_eq!(killswitch_status("moved"), KillSwitch::Mismatch);
            assert_eq!(killswitch_status("other"), KillSwitch::Unknown);
        });
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn generated_keys_must_be_valid_utf8_and_canonical_wireguard_keys() {
        assert_eq!(
            key_output(format!("{KEY}\n").as_bytes()),
            Some(KEY.to_string())
        );
        assert!(key_output(b"invalid").is_none());
        assert!(key_output(&[0xff]).is_none());
        assert!(key_output(b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopq=").is_none());
    }

    #[test]
    fn live_dump_keeps_unsaved_peers_and_actual_runtime_settings() {
        let other = "A".repeat(43) + "=";
        let dump = parse_dump(&format!("private\t{KEY}\t51820\toff\n{KEY}\t(none)\t(none)\t10.0.0.0/24\t100\t20\t30\toff\n{other}\t{KEY}\t[::1]:5\t::/0\t200\t40\t50\t25\n")).unwrap();
        let saved = config::ParsedPeer {
            public_key: KEY.into(),
            allowed_ips: "old".into(),
            keepalive: "old".into(),
            endpoint: "saved:1".into(),
            ..Default::default()
        };
        let peers = merge_peers(vec![saved], Some(&dump), 300);
        assert_eq!(peers.len(), 2);
        assert_eq!(peers[0].allowed_ips, "10.0.0.0/24");
        assert_eq!(peers[0].keepalive, "off");
        assert_eq!(peers[0].endpoint, "saved:1");
        assert_eq!(peers[0].handshake_age, Some(200));
        assert_eq!((peers[0].rx_bytes, peers[0].tx_bytes), (20, 30));
        assert_eq!(peers[0].keepalive_secs, None);
        assert!(peers[1].preshared);
        assert_eq!(peers[1].keepalive_secs, Some(25));
        assert_eq!(peers[1].handshake_age, Some(100));
        assert_eq!(live_totals(Some(&dump), 300), (60, 80, Some(100)));
        assert_eq!(live_totals(Some(&dump), 150).2, None);
        let mut dump = dump;
        dump.peers[0].rx = u64::MAX;
        assert_eq!(live_totals(Some(&dump), 300).0, u64::MAX);
        assert!(parse_dump("invalid\tdump\t0").is_none());
        let offline = merge_peers(
            vec![config::ParsedPeer {
                public_key: KEY.into(),
                keepalive: "25".into(),
                ..Default::default()
            }],
            None,
            300,
        );
        assert_eq!(offline[0].keepalive_secs, Some(25));
        assert_eq!(offline[0].handshake_age, None);
    }

    // The previous if/else ladder, kept here as the behavioral oracle so the
    // closed-form `fmt_bytes` is proven byte-for-byte identical to it.
    fn fmt_bytes_ladder(b: u64) -> String {
        const KIB: f64 = 1024.0;
        let b = b as f64;
        if b < KIB {
            format!("{b:.0} B")
        } else if b < KIB * KIB {
            format!("{:.2} KiB", b / KIB)
        } else if b < KIB * KIB * KIB {
            format!("{:.2} MiB", b / (KIB * KIB))
        } else if b < KIB * KIB * KIB * KIB {
            format!("{:.2} GiB", b / (KIB * KIB * KIB))
        } else {
            format!("{:.2} TiB", b / (KIB * KIB * KIB * KIB))
        }
    }

    #[test]
    fn fmt_bytes_matches_ladder() {
        // Every unit boundary (and ±1 around it), plus 0 and u64::MAX.
        let mut cases = vec![0u64, 1, 1023, 1024, 1025, u64::MAX];
        for p in [10u32, 20, 30, 40, 50, 60] {
            let t = 1u64 << p;
            cases.extend([t - 1, t, t + 1]);
        }
        // A dense-ish multiplicative sweep across the whole range.
        let mut v = 1u64;
        while v < u64::MAX / 3 {
            cases.push(v);
            cases.push(v * 3);
            v = v.saturating_mul(2);
        }
        for b in cases {
            assert_eq!(fmt_bytes(b), fmt_bytes_ladder(b), "mismatch at b={b}");
        }
    }

    #[test]
    fn sanitize_name_rules() {
        assert_eq!(sanitize_name("home"), "home");
        assert_eq!(sanitize_name("home server"), "home_server");
        assert_eq!(sanitize_name("name.conf"), "name");
        assert_eq!(sanitize_name("@#$"), "tunnel"); // all symbols
        assert_eq!(sanitize_name("___abc"), "abc"); // leading non-alnum stripped
        assert_eq!(sanitize_name("a.b.c."), "a.b.c"); // trailing dot trimmed
        assert_eq!(sanitize_name(""), "tunnel");
        let long = sanitize_name("averylongtunnelname1234567");
        assert!(long.chars().count() <= 15);
        // Result is always helper-valid: starts with an alphanumeric.
        for input in ["...x", "___y", "@@@9abc", "valid-name"] {
            let n = sanitize_name(input);
            assert!(
                n.chars().next().unwrap().is_ascii_alphanumeric(),
                "{input} -> {n}"
            );
        }
    }

    #[test]
    fn unique_name_deduplicates() {
        let name = unique_name_with("vpn config", |candidate| {
            matches!(candidate, "vpn_config" | "vpn_config-2")
        });
        assert_eq!(name.unwrap(), "vpn_config-3");
        assert!(unique_name_with("wg0", |_| true).is_err());
    }

    #[test]
    fn validate_config_allows_full_and_interface_only_configs() {
        let cfg = format!(
            "[Interface]\nPrivateKey = {KEY}\nAddress = 10.0.0.2/24, fd00::2/64\n\
             DNS = 1.1.1.1\n\n[Peer]\nPublicKey = {KEY}\nAllowedIPs = 0.0.0.0/0, ::/0\n\
             Endpoint = vpn.example.com:51820\nPersistentKeepalive = 25\n"
        );
        assert!(validate_config(&cfg).is_ok());
        assert!(validate_config(&format!("[Interface]\nPrivateKey = {KEY}\n")).is_ok());
    }

    #[test]
    fn validate_rejects_missing_private_key() {
        assert!(validate_config("not a config").is_err());
        assert!(validate_config(&format!(
            "[Interface]\nAddress = 10.0.0.2/24\n[Peer]\nPublicKey = {KEY}\nAllowedIPs = 0.0.0.0/0\n"
        ))
        .is_err());
    }
}
