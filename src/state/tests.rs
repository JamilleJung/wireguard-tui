use super::*;
use crate::backend::{Detail, Peer, Tunnel};

fn tunnel(name: &str, active: bool) -> Tunnel {
    Tunnel {
        name: name.into(),
        active,
    }
}

#[test]
fn removed_selection_never_keeps_another_tunnels_detail() {
    let mut app = App {
        tunnels: vec![tunnel("old", true)],
        ..App::default()
    };
    app.state.select(Some(0));
    app.detail = Some(Detail {
        name: "old".into(),
        active: true,
        ..Default::default()
    });
    *app.poll_out.lock().unwrap() = Some(Snapshot {
        tunnels: Some(vec![tunnel("new", false)]),
        ..Snapshot::default()
    });
    app.apply_snapshot();
    assert_eq!(app.selected_name().as_deref(), Some("new"));
    assert!(app.detail.is_none());
    assert!(app.detail_fields().is_empty());
}

#[test]
fn navigation_requests_detail_without_blocking_or_exposing_previous_keys() {
    let mut app = App {
        tunnels: vec![tunnel("first", false), tunnel("second", false)],
        detail: Some(Detail {
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
        tunnels: vec![tunnel("first", false), tunnel("second", false)],
        ..App::default()
    };
    app.state.select(Some(0));
    app.pending_selection = Some(("created".into(), app.selection_generation));
    app.move_selection(1);
    let snapshot = || Snapshot {
        tunnels: Some(vec![
            tunnel("first", false),
            tunnel("second", false),
            tunnel("created", false),
        ]),
        ..Snapshot::default()
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
fn editor_load_waits_for_modal_draft_before_opening() {
    let mut app = App {
        mode: Mode::InputRename {
            orig: "wg0".into(),
            buf: "unsaved".into(),
        },
        ..App::default()
    };
    app.op.begin(OpKind::LoadEditor, Instant::now()).unwrap();
    app.update_out
        .lock()
        .unwrap()
        .push_back(UpdateMsg::EditorLoaded(Ok(EditorRequest {
            name: "original-selection".into(),
            original: "original text".into(),
            initial: "editor text".into(),
            new_kind: None,
            keep_on_cancel: false,
        })));
    app.drain_update();
    assert!(app.ready_editor.is_none());
    assert!(matches!(&app.mode, Mode::InputRename { buf, .. } if buf == "unsaved"));
    app.mode = Mode::Normal;
    app.drain_update();
    assert!(!app.op.is_busy());
    assert_eq!(
        app.ready_editor.as_ref().unwrap().name,
        "original-selection"
    );
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
fn finished_operation_is_reconciled_only_by_a_later_refresh() {
    let mut app = App {
        tunnels: vec![tunnel("wg0", false)],
        ..App::default()
    };
    app.state.select(Some(0));
    app.op.begin(OpKind::Connect, Instant::now()).unwrap();
    let stale = Snapshot {
        tunnels: Some(vec![tunnel("wg0", false)]),
        ..Snapshot::default()
    };
    app.update_out
        .lock()
        .unwrap()
        .push_back(UpdateMsg::Finished(Report::unknown("timed out")));
    app.drain_update();
    assert!(matches!(
        app.op,
        Operation::Finished {
            outcome: Outcome::Unknown,
            ..
        }
    ));
    assert!(matches!(&app.mode, Mode::Message(m) if m == "timed out"));
    let refresh = app.poll_in.lock().unwrap().generation;
    assert!(refresh > 0, "a finished operation asks for a re-query");
    // A snapshot gathered before the operation finished does not count.
    *app.poll_out.lock().unwrap() = Some(stale);
    app.apply_snapshot();
    assert!(matches!(app.op, Operation::Finished { .. }));
    *app.poll_out.lock().unwrap() = Some(Snapshot {
        tunnels: Some(vec![tunnel("wg0", true)]),
        ..Snapshot::default()
    });
    app.apply_snapshot();
    assert_eq!(app.op, Operation::Idle);
    assert!(app.tunnels[0].active);
}

fn detail_with_peers(count: usize) -> Detail {
    Detail {
        name: "wg0".into(),
        active: true,
        public_key: "interface key".into(),
        listen_port: "51820".into(),
        peers: (0..count)
            .map(|i| Peer {
                public_key: format!("peer key {i}"),
                allowed_ips: format!("10.0.{i}.0/24"),
                ..Peer::default()
            })
            .collect(),
        ..Detail::default()
    }
}

#[test]
fn focus_order_walks_interface_then_each_peer_and_wraps() {
    let mut app = App {
        detail: Some(detail_with_peers(2)),
        ..App::default()
    };
    let labels: Vec<_> = app.detail_fields().into_iter().map(|(l, _)| l).collect();
    assert_eq!(
        labels,
        [
            "Public key",
            "Listen port",
            "Peer 1 public key",
            "Peer 1 allowed IPs",
            "Peer 2 public key",
            "Peer 2 allowed IPs"
        ]
    );
    app.move_field(-1);
    assert_eq!(app.detail_cursor, 5);
    app.move_field(1);
    assert_eq!(app.detail_cursor, 0);
    assert!(app.detail_follow_focus);
}

#[test]
fn peer_navigation_jumps_between_peers_and_wraps() {
    let mut app = App {
        detail: Some(detail_with_peers(12)),
        ..App::default()
    };
    app.jump_peer(true);
    assert_eq!(app.detail_entries()[app.detail_cursor].2, Some(0));
    app.jump_peer(true);
    assert_eq!(app.detail_entries()[app.detail_cursor].2, Some(1));
    app.jump_peer(false);
    app.jump_peer(false);
    assert_eq!(app.detail_entries()[app.detail_cursor].2, Some(11));
    app.jump_peer(true);
    assert_eq!(app.detail_entries()[app.detail_cursor].2, Some(0));
    let mut empty = App::default();
    empty.jump_peer(true);
    assert_eq!(empty.detail_cursor, 0);
}

#[test]
fn health_is_tracked_per_peer_from_snapshots() {
    let mut app = App {
        tunnels: vec![tunnel("wg0", true)],
        ..App::default()
    };
    app.state.select(Some(0));
    let mut detail = detail_with_peers(2);
    detail.peers[0].handshake_age = Some(10);
    detail.peers[1].handshake_age = Some(4000);
    detail.killswitch = KillSwitch::Disabled;
    *app.poll_out.lock().unwrap() = Some(Snapshot {
        detail: Some(("wg0".into(), detail)),
        ..Snapshot::default()
    });
    app.apply_snapshot();
    assert_eq!(app.peer_health, [Health::Healthy, Health::Idle]);
    assert_eq!(app.tunnel_health(), Some(Health::Healthy));
}

#[test]
fn changelog_display_starts_at_first_release() {
    let src: &'static str = "# Changelog\n\nIntro\n\n## [Unreleased]\n\n## [1.2.3] - 2026\n- x\n";
    assert!(changelog_for_display(src).starts_with("## [1.2.3]"));
    assert_eq!(changelog_for_display("no headings"), "no headings");
}
