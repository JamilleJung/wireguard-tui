use std::io::Write as _;
use std::process::Command;

/// Copy `text` to the system clipboard, trying the most reliable backend first.
///
/// Order: wl-copy (Wayland) → xclip → xsel → OSC 52 terminal escape. Returns the
/// name of the backend that succeeded (for status toasts); always falls back to
/// OSC 52 so something is attempted even with no clipboard tool installed.
pub fn copy_text(text: &str) -> &'static str {
    if text.is_empty() {
        return "noop";
    }
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();

    // Native helpers (best: they reach the real system clipboard, not just the
    // terminal). Prefer wl-copy under Wayland.
    let candidates: &[(&str, &[&str])] = if wayland {
        &[
            ("wl-copy", &[]),
            ("xclip", &["-selection", "clipboard"]),
            ("xsel", &["--clipboard", "--input"]),
        ]
    } else {
        &[
            ("xclip", &["-selection", "clipboard"]),
            ("xsel", &["--clipboard", "--input"]),
            ("wl-copy", &[]),
        ]
    };

    for (prog, args) in candidates {
        if which(prog) && pipe_to(prog, args, text) {
            return prog;
        }
    }

    osc52(text);
    "osc52"
}

/// True if `prog` is on PATH (cheap, no shell).
fn which(prog: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|p| p.join(prog).is_file()))
}

/// Try one clipboard tool without letting a stalled reader hang the TUI.
fn pipe_to(prog: &str, args: &[&str], text: &str) -> bool {
    let mut command = Command::new(prog);
    command.args(args);
    pipe_to_command(command, text, std::time::Duration::from_secs(2))
}

/// Bound both stdin delivery and child exit: a clipboard helper can stall
/// before reading a large config, or keep running after EOF. Reap the child in
/// a worker and terminate its process group if it misses the deadline.
fn pipe_to_command(
    mut command: std::process::Command,
    text: &str,
    timeout: std::time::Duration,
) -> bool {
    use std::io::Write as _;
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    use std::sync::mpsc;

    let Ok(mut child) = command
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let pid = child.id();
    let Some(mut stdin) = child.stdin.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return false;
    };
    let input = text.to_string();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
        let status = child.wait();
        let written = matches!(writer.join(), Ok(Ok(())));
        let _ = tx.send(status.is_ok_and(|status| status.success()) && written);
    });
    match rx.recv_timeout(timeout) {
        Ok(success) => success,
        Err(_) => {
            // The spawned child starts a separate process group. This also
            // closes inherited stdin in descendants so the writer can finish.
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
            let _ = rx.recv_timeout(std::time::Duration::from_millis(200));
            false
        }
    }
}

/// OSC 52 fallback: write the clipboard escape straight to the tty. Does not
/// draw, so it won't disturb the ratatui frame.
fn osc52(text: &str) {
    let seq = format!("\x1b]52;c;{}\x07", base64(text.as_bytes()));
    let mut out = std::io::stdout();
    let _ = out.write_all(seq.as_bytes());
    let _ = out.flush();
}

/// Minimal standard-alphabet base64 (for the OSC 52 clipboard escape).
fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let b0 = c[0] as u32;
        let b1 = *c.get(1).unwrap_or(&0) as u32;
        let b2 = *c.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(if c.len() > 1 {
            T[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if c.len() > 2 {
            T[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Normalize single-field copy payloads.
///
/// Terminal rendering can wrap or indent display values. Single values such as
/// public keys, endpoints, addresses, and AllowedIPs should copy cleanly without
/// leading spaces, trailing spaces, or accidental newlines. Raw configs/logs do
/// not use this helper because their newlines are meaningful.
pub fn normalize_single_field_copy_value(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_helper_times_out_while_stdin_is_blocked() {
        let mut command = std::process::Command::new("sh");
        command.args(["-c", "sleep 30"]);
        let start = std::time::Instant::now();
        assert!(!pipe_to_command(
            command,
            &"x".repeat(262144),
            std::time::Duration::from_millis(100)
        ));
        assert!(start.elapsed() < std::time::Duration::from_secs(3));
    }

    #[test]
    fn clipboard_helper_requires_successful_delivery_and_exit() {
        let mut command = std::process::Command::new("sh");
        command.args(["-c", "cat >/dev/null"]);
        assert!(pipe_to_command(
            command,
            &"x".repeat(262144),
            std::time::Duration::from_secs(3)
        ));
        let mut command = std::process::Command::new("sh");
        command.args(["-c", "exit 7"]);
        assert!(!pipe_to_command(
            command,
            "secret config",
            std::time::Duration::from_secs(3)
        ));
    }

    #[test]
    fn trims_accidental_outer_whitespace() {
        assert_eq!(normalize_single_field_copy_value(" abc "), "abc");
        assert_eq!(normalize_single_field_copy_value("\nabc\n"), "abc");
        assert_eq!(normalize_single_field_copy_value("abc\n"), "abc");
        assert_eq!(normalize_single_field_copy_value("  abc\n  "), "abc");
        assert_eq!(normalize_single_field_copy_value("abc\r\n"), "abc");
    }

    #[test]
    fn joins_display_wrapped_fields() {
        assert_eq!(
            normalize_single_field_copy_value("  one\n  two  \n"),
            "one two"
        );
    }
}
