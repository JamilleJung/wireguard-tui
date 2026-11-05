//! Opt-in structured logging for the frontend.
//!
//! Off by default. `WG_TUI_LOG=/path/to/file` appends `key=value` lines to a
//! private (0600) file; `--debug` writes them to stderr instead (redirect it,
//! e.g. `wg-tui --debug 2>wg-tui.log`, since the UI owns the terminal).
//!
//! Every value passes through [`secrets::redact_config`] and a key-shaped
//! token filter before it is written, so private and preshared keys never
//! reach a log line even if a caller passes config text by mistake.

use std::io::Write;
use std::sync::{Mutex, OnceLock};

use crate::{config, secrets};

type Sink = Box<dyn Write + Send>;

static SINK: OnceLock<Mutex<Option<Sink>>> = OnceLock::new();

fn sink() -> &'static Mutex<Option<Sink>> {
    SINK.get_or_init(|| Mutex::new(None))
}

/// Where log lines go, chosen at startup.
pub enum Target {
    File(std::path::PathBuf),
    Stderr,
}

/// Pick the target from `--debug` and `WG_TUI_LOG`.
pub fn target_from(debug_flag: bool, env_path: Option<std::ffi::OsString>) -> Option<Target> {
    match env_path.filter(|value| !value.is_empty()) {
        Some(path) => Some(Target::File(path.into())),
        None if debug_flag => Some(Target::Stderr),
        None => None,
    }
}

/// Install the log sink. Errors opening the file are returned for display.
pub fn init(target: Target) -> std::io::Result<()> {
    let writer: Sink = match target {
        Target::Stderr => Box::new(std::io::stderr()),
        Target::File(path) => {
            use std::os::unix::fs::OpenOptionsExt;
            Box::new(
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .mode(0o600)
                    .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                    .open(path)?,
            )
        }
    };
    *sink().lock().unwrap_or_else(|e| e.into_inner()) = Some(writer);
    Ok(())
}

#[cfg(test)]
pub fn init_writer(writer: Sink) {
    *sink().lock().unwrap_or_else(|e| e.into_inner()) = Some(writer);
}

/// Remove secrets from a value: config-style secret lines and any bare
/// WireGuard-key-shaped token.
pub fn redact(value: &str) -> String {
    let text = secrets::redact_config(value);
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < text.len() {
        let end = index + 44;
        if end <= text.len() && text.is_char_boundary(end) && config::is_wg_key(&text[index..end]) {
            out.push_str("<key>");
            index = end;
            continue;
        }
        let c = text[index..].chars().next().unwrap_or(' ');
        out.push(c);
        index += c.len_utf8();
    }
    out
}

fn quote(value: &str) -> String {
    let clean: String = redact(value)
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if !clean.is_empty()
        && clean
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:@+".contains(c))
    {
        clean
    } else {
        format!("\"{}\"", clean.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

/// Format one `key=value` line (no trailing newline).
pub fn format_line(unix_secs: u64, event: &str, fields: &[(&str, &str)]) -> String {
    let mut line = format!("ts={unix_secs} event={}", quote(event));
    for (key, value) in fields {
        line.push(' ');
        line.push_str(key);
        line.push('=');
        line.push_str(&quote(value));
    }
    line
}

/// Write one structured event if logging is enabled.
pub fn event(event: &str, fields: &[(&str, &str)]) {
    let Ok(mut guard) = sink().lock() else {
        return;
    };
    let Some(writer) = guard.as_mut() else {
        return;
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let _ = writeln!(writer, "{}", format_line(now, event, fields));
    let _ = writer.flush();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    const KEY: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopo=";

    #[derive(Clone, Default)]
    struct Shared(Arc<Mutex<Vec<u8>>>);

    impl Write for Shared {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn log_lines_never_contain_private_or_preshared_keys() {
        let captured = Shared::default();
        init_writer(Box::new(captured.clone()));
        let config =
            format!("[Interface]\nPrivateKey = {KEY}\n[Peer]\nPresharedKey={KEY}\nPublicKey = pub");
        event(
            "test.secret",
            &[
                ("config", &config),
                ("journal", &format!("PrivateKey: {KEY}")),
                ("bare", KEY),
                ("quoted", &format!("key '{KEY}' rejected")),
            ],
        );
        let text = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
        *sink().lock().unwrap() = None;
        assert!(text.contains("event=test.secret"), "{text}");
        assert!(!text.contains(KEY), "{text}");
        assert!(!text.contains("PrivateKey = A"), "{text}");
        // Other tests may log concurrently; this event must stay on one line.
        let ours: Vec<_> = text.lines().filter(|l| l.contains("test.secret")).collect();
        assert_eq!(ours.len(), 1, "{text}");
        assert!(ours[0].contains("bare=\"<key>\""), "{text}");
    }

    #[test]
    fn values_are_quoted_and_targets_are_opt_in() {
        assert_eq!(
            format_line(5, "helper.call", &[("verb", "up"), ("error", "a \"b\"\nc")]),
            "ts=5 event=helper.call verb=up error=\"a \\\"b\\\" c\""
        );
        assert!(target_from(false, None).is_none());
        assert!(matches!(target_from(true, None), Some(Target::Stderr)));
        assert!(matches!(
            target_from(true, Some("/tmp/x.log".into())),
            Some(Target::File(_))
        ));
        assert!(target_from(false, Some("".into())).is_none());
    }
}
