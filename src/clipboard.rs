use std::io::Write as _;
use std::process::{Command, Stdio};

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
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|p| p.join(prog).is_file())
    })
}

/// Spawn `prog args`, write `text` to its stdin, wait. True on clean exit.
fn pipe_to(prog: &str, args: &[&str], text: &str) -> bool {
    let mut child = match Command::new(prog)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    if let Some(mut stdin) = child.stdin.take() {
        if stdin.write_all(text.as_bytes()).is_err() {
            return false;
        }
        // Drop stdin to signal EOF (wl-copy/xclip block until then).
    }
    child.wait().map(|s| s.success()).unwrap_or(false)
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
    use super::normalize_single_field_copy_value;

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
