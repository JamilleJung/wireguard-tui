use super::*;
use crate::state::Draft;

#[test]
fn mutations_are_refused_while_an_operation_runs() {
    let mut app = App::default();
    app.op
        .begin(crate::ops::OpKind::Connect, std::time::Instant::now())
        .unwrap();
    handle_key(&mut app, K::Char('n'), KeyModifiers::NONE);
    assert!(matches!(app.mode, Mode::Normal));
    assert!(app.status.contains("still running"));
    // Read-only navigation still works.
    handle_key(&mut app, K::Char('?'), KeyModifiers::NONE);
    assert!(matches!(app.mode, Mode::Help { .. }));
}

#[test]
fn help_popup_scrolls_and_closes_by_keyboard() {
    let mut app = App {
        viewport: ratatui::layout::Rect::new(0, 0, 40, 12),
        ..App::default()
    };
    handle_key(&mut app, K::Char('?'), KeyModifiers::NONE);
    handle_key(&mut app, K::Char('j'), KeyModifiers::NONE);
    handle_key(&mut app, K::PageDown, KeyModifiers::NONE);
    let Mode::Help { scroll } = app.mode else {
        panic!("help closed")
    };
    assert!(scroll > 1);
    handle_key(&mut app, K::Char('G'), KeyModifiers::NONE);
    let Mode::Help { scroll: end } = app.mode else {
        panic!("help closed")
    };
    assert_eq!(end, ui::popups::help_max_scroll(app.viewport));
    handle_key(&mut app, K::Esc, KeyModifiers::NONE);
    assert!(matches!(app.mode, Mode::Normal));
}

fn recovery(problem: DraftProblem) -> App {
    App {
        mode: Mode::DraftRecovery {
            draft: Draft {
                name: "wg0".into(),
                original: "saved".into(),
                text: "draft text".into(),
                new_kind: None,
            },
            problem,
            error: "Not saved".into(),
        },
        ..App::default()
    }
}

#[test]
fn draft_recovery_offers_reedit_discard_and_keep_by_keyboard() {
    let mut app = recovery(DraftProblem::SaveFailed);
    handle_key(&mut app, K::Char('e'), KeyModifiers::NONE);
    let request = app.ready_editor.take().unwrap();
    assert_eq!(request.initial, "draft text");
    assert!(request.keep_on_cancel);

    // Discard needs confirmation; anything but y returns to the draft.
    let mut app = recovery(DraftProblem::Invalid);
    handle_key(&mut app, K::Char('d'), KeyModifiers::NONE);
    assert!(matches!(app.mode, Mode::ConfirmDiscard { .. }));
    handle_key(&mut app, K::Char('n'), KeyModifiers::NONE);
    assert!(matches!(&app.mode, Mode::DraftRecovery { draft, .. } if draft.text == "draft text"));
    handle_key(&mut app, K::Char('d'), KeyModifiers::NONE);
    handle_key(&mut app, K::Char('y'), KeyModifiers::NONE);
    assert!(matches!(app.mode, Mode::Normal));
    assert!(app.status.contains("discarded"));

    // Esc keeps it for later; E brings it back.
    let mut app = recovery(DraftProblem::SaveFailed);
    handle_key(&mut app, K::Esc, KeyModifiers::NONE);
    assert!(matches!(app.mode, Mode::Normal));
    assert!(app.parked_draft.is_some());
    handle_key(&mut app, K::Char('E'), KeyModifiers::NONE);
    assert!(
        matches!(&app.mode, Mode::DraftRecovery { draft, problem: DraftProblem::SaveFailed, .. } if draft.text == "draft text")
    );

    // Retry is offered only for save failures, and never while busy.
    let mut app = recovery(DraftProblem::Invalid);
    handle_key(&mut app, K::Char('r'), KeyModifiers::NONE);
    assert!(matches!(app.mode, Mode::DraftRecovery { .. }));
    let mut app = recovery(DraftProblem::SaveFailed);
    app.op
        .begin(crate::ops::OpKind::Connect, std::time::Instant::now())
        .unwrap();
    handle_key(&mut app, K::Char('r'), KeyModifiers::NONE);
    assert!(matches!(&app.mode, Mode::DraftRecovery { draft, .. } if draft.text == "draft text"));
}

#[test]
fn sync_failure_popup_can_be_kept_as_is() {
    let mut app = App {
        mode: Mode::SyncFailed {
            name: "wg0".into(),
            error: "boom".into(),
            needs_reconnect: false,
        },
        ..App::default()
    };
    handle_key(&mut app, K::Char('x'), KeyModifiers::NONE);
    assert!(matches!(app.mode, Mode::SyncFailed { .. }));
    handle_key(&mut app, K::Char('k'), KeyModifiers::NONE);
    assert!(matches!(app.mode, Mode::Normal));
    assert!(app.status.contains("differs from its saved config"));
}

#[test]
fn detail_and_peer_keys_move_focus() {
    let mut app = App {
        detail: Some(crate::backend::Detail {
            name: "wg0".into(),
            public_key: "k".into(),
            peers: vec![
                crate::backend::Peer {
                    public_key: "p1".into(),
                    ..Default::default()
                },
                crate::backend::Peer {
                    public_key: "p2".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }),
        ..App::default()
    };
    handle_key(&mut app, K::Char(']'), KeyModifiers::NONE);
    assert_eq!(app.detail_cursor, 1);
    handle_key(&mut app, K::Char(']'), KeyModifiers::NONE);
    assert_eq!(app.detail_cursor, 2);
    handle_key(&mut app, K::Char('h'), KeyModifiers::NONE);
    assert_eq!(app.detail_cursor, 1);
    handle_key(&mut app, K::PageDown, KeyModifiers::NONE);
    assert!(app.detail_scroll > 0);
    assert!(!app.detail_follow_focus);
}
