//! Command-line entry points that do not open the full-screen UI: argument
//! parsing, `wg-tui doctor` and `wg-tui setup`.

use std::io::Write as _;

use crate::doctor;
use crate::theme::{THEME_NAMES, Theme};

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
pub enum Invocation {
    Version,
    Help,
    Doctor,
    Setup,
    Ui { theme: Option<String>, debug: bool },
}

/// Parse the arguments after the program name.
pub fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Invocation, String> {
    let mut theme = None;
    let mut debug = false;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-V" | "--version" => return Ok(Invocation::Version),
            "-h" | "--help" => return Ok(Invocation::Help),
            "doctor" => return Ok(Invocation::Doctor),
            "setup" => return Ok(Invocation::Setup),
            "--debug" => debug = true,
            "--theme" => {
                theme = Some(
                    args.next()
                        .ok_or_else(|| format!("--theme needs a value ({THEME_NAMES})"))?,
                )
            }
            other => match other.strip_prefix("--theme=") {
                Some(value) => theme = Some(value.to_string()),
                None => return Err(format!("unknown argument '{other}' (try --help)")),
            },
        }
    }
    Ok(Invocation::Ui { theme, debug })
}

pub fn usage() -> String {
    format!(
        "wg-tui {} - a terminal UI for managing WireGuard tunnels\n\n\
         Usage: wg-tui                 launch the interactive UI\n       \
         wg-tui doctor            check your system and print a checklist\n       \
         wg-tui setup             guided fix for anything missing\n       \
         wg-tui --theme NAME      UI theme: {THEME_NAMES}\n       \
         wg-tui --debug           write a redacted debug log to stderr\n       \
         wg-tui --version         print the version\n       \
         wg-tui --help            show this help\n\n\
         Environment: WG_TUI_THEME=NAME, NO_COLOR=1 (no colours),\n\
         \x20            WG_TUI_LOG=/path/file (redacted debug log).\n\n\
         Inside the app, press '?' for the full key map.",
        env!("CARGO_PKG_VERSION")
    )
}

/// Resolve the theme from the flag and the environment.
pub fn theme_from_env(flag: Option<&str>) -> Result<Theme, String> {
    let env_theme = std::env::var("WG_TUI_THEME").ok();
    let no_color = std::env::var("NO_COLOR").ok();
    Theme::resolve(flag, env_theme.as_deref(), no_color.as_deref())
}

/// `wg-tui doctor` - print a system checklist; exit 0 (ok) / 1 (warnings) /
/// 2 (critical missing). Read-only.
pub fn run_doctor() -> i32 {
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
pub fn run_setup() -> i32 {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Invocation, String> {
        parse_args(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn arguments_select_commands_theme_and_debug() {
        assert_eq!(
            parse(&[]).unwrap(),
            Invocation::Ui {
                theme: None,
                debug: false
            }
        );
        assert_eq!(parse(&["--version"]).unwrap(), Invocation::Version);
        assert_eq!(parse(&["-h"]).unwrap(), Invocation::Help);
        assert_eq!(parse(&["doctor"]).unwrap(), Invocation::Doctor);
        assert_eq!(
            parse(&["--theme", "ascii", "--debug"]).unwrap(),
            Invocation::Ui {
                theme: Some("ascii".into()),
                debug: true
            }
        );
        assert_eq!(
            parse(&["--theme=high-contrast"]).unwrap(),
            Invocation::Ui {
                theme: Some("high-contrast".into()),
                debug: false
            }
        );
        assert!(parse(&["--theme"]).is_err());
        assert!(
            parse(&["--bogus"])
                .unwrap_err()
                .contains("unknown argument")
        );
        assert!(usage().contains("--theme NAME"));
    }
}
