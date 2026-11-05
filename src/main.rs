//! A full-screen terminal UI for managing WireGuard tunnels - the same feature
//! set as the desktop client (list, live status, activate/deactivate, editor,
//! key generation, QR, export, start-on-boot), driven entirely by the keyboard.
//!
//! Layout of the crate:
//!   * `state`   - application state and how background results are applied
//!   * `input`   - key handling; `keymap` - keys, commands, help text
//!   * `actions` - background operations (verified against the real state)
//!   * `ops`     - operation state machine, verification, kill-switch policy
//!   * `ui`      - rendering; `theme` - colours and the ASCII fallback
//!   * `editor`  - external editor; `runtime` - event loop and poller
//!   * `backend` - the privileged helper and everything WireGuard-specific

mod actions;
mod backend;
mod cli;
mod clipboard;
mod config;
mod create;
mod doctor;
mod editor;
mod import_parse;
mod input;
mod keymap;
mod logging;
mod ops;
mod qr_isolate;
mod runtime;
mod safe_file;
mod secrets;
mod state;
#[cfg(test)]
mod test_support;
mod theme;
mod ui;
mod update;
mod validation;

use std::io;

fn main() -> io::Result<()> {
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new(qr_isolate::CHILD_ARG)) {
        qr_isolate::child_main();
    }
    // Handle --version/--help before touching the terminal, so they work in
    // scripts and don't accidentally launch the full-screen UI.
    let invocation = match cli::parse_args(std::env::args().skip(1)) {
        Ok(invocation) => invocation,
        Err(error) => {
            eprintln!("wg-tui: {error}");
            std::process::exit(2);
        }
    };
    let (theme, debug) = match invocation {
        cli::Invocation::Version => {
            println!("wg-tui {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        cli::Invocation::Help => {
            println!("{}", cli::usage());
            return Ok(());
        }
        cli::Invocation::Doctor => std::process::exit(cli::run_doctor()),
        cli::Invocation::Setup => std::process::exit(cli::run_setup()),
        cli::Invocation::Ui { theme, debug } => (theme, debug),
    };
    let theme = match cli::theme_from_env(theme.as_deref()) {
        Ok(theme) => theme,
        Err(error) => {
            eprintln!("wg-tui: {error}");
            std::process::exit(2);
        }
    };
    if let Some(target) = logging::target_from(debug, std::env::var_os("WG_TUI_LOG"))
        && let Err(error) = logging::init(target)
    {
        eprintln!("wg-tui: cannot open WG_TUI_LOG: {error}");
        std::process::exit(2);
    }
    logging::event(
        "start",
        &[
            ("version", env!("CARGO_PKG_VERSION")),
            ("theme", theme.name.label()),
            ("color", if theme.color { "yes" } else { "no" }),
        ],
    );
    backend::init();
    let mut terminal = ratatui::init();
    let res = runtime::run(&mut terminal, theme);
    ratatui::restore();
    res
}
