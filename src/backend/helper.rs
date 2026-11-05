//! Running the privileged `wg-helper` (and unprivileged `wg`) with bounded
//! output, a hard deadline and a typed error model.
//!
//! The helper runs as:
//!   * nothing             - when we are already root
//!   * `sudo -n wg-helper` - the normal case (NOPASSWD sudoers drop-in)
//!   * `pkexec wg-helper`  - fallback when sudo is not set up (prompts)

use std::fmt;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use crate::{logging, secrets};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Escalation {
    Direct,
    Sudo,
    Pkexec,
}

static ESC: OnceLock<Escalation> = OnceLock::new();
static HELPER: OnceLock<Option<String>> = OnceLock::new();

/// Default deadline for one helper call (wg-quick may wait on DNS/routes).
pub const HELPER_TIMEOUT: Duration = Duration::from_secs(90);

/// A failed helper invocation. `Display` keeps the user-facing wording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HelperError {
    /// No trusted helper binary (or the escalation tool) could be found.
    NotInstalled(String),
    /// Privilege escalation was refused, dismissed or needs configuration.
    AuthorizationRequired(String),
    /// The helper did not finish before the deadline; the outcome is unknown.
    Timeout,
    /// Another helper mutation holds the shared state lock.
    Busy(String),
    /// The helper ran and reported an error (stderr is already redacted).
    Failed { command: String, stderr: String },
    /// Spawning or talking to the process failed, or its output was invalid.
    Io(String),
}

impl HelperError {
    /// A timeout means the operation may or may not have taken effect.
    pub fn is_timeout(&self) -> bool {
        matches!(self, Self::Timeout)
    }

    /// Short machine-readable class for logs.
    pub fn class(&self) -> &'static str {
        match self {
            Self::NotInstalled(_) => "not_installed",
            Self::AuthorizationRequired(_) => "authorization_required",
            Self::Timeout => "timeout",
            Self::Busy(_) => "busy",
            Self::Failed { .. } => "failed",
            Self::Io(_) => "io",
        }
    }

    /// Whether the text mentions `needle` (case-insensitive), e.g. "reconnect".
    pub fn mentions(&self, needle: &str) -> bool {
        self.to_string()
            .to_ascii_lowercase()
            .contains(&needle.to_ascii_lowercase())
    }
}

impl fmt::Display for HelperError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotInstalled(message)
            | Self::AuthorizationRequired(message)
            | Self::Busy(message)
            | Self::Io(message) => f.write_str(message),
            Self::Timeout => f.write_str("Helper operation timed out."),
            Self::Failed { command, stderr } => write!(f, "{command}: {stderr}"),
        }
    }
}

impl std::error::Error for HelperError {}

impl From<HelperError> for String {
    fn from(error: HelperError) -> Self {
        error.to_string()
    }
}

/// Decide once how we gain privilege and where the helper lives.
pub fn init() {
    let esc = if unsafe { libc::geteuid() } == 0 {
        Escalation::Direct
    } else if helper_path().is_some_and(sudo_helper_available) {
        Escalation::Sudo
    } else {
        Escalation::Pkexec
    };
    let _ = ESC.set(esc);
}

fn sudo_helper_available(path: &str) -> bool {
    use std::os::unix::process::CommandExt;
    let mut command = Command::new("sudo");
    command
        .args(["-n", path, "list"])
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
        .spawn()
        .ok()
        .and_then(|child| helper_output_timeout(child, None, Duration::from_secs(2)).ok())
        .is_some_and(|out| out.status.success())
}

/// Whether to honour a `$WG_HELPER` override. Since the helper runs as root, an
/// attacker who can set this env var could otherwise get their own script run
/// with privilege (notably when the app itself is run as root). In **debug**
/// builds it is always honoured (dev convenience). In **release** builds it is
/// refused unless the operator opts in with `WG_ALLOW_UNSAFE_HELPER=1` *and* the
/// target is a safe file: an absolute path to a regular file owned by root and
/// not writable by group/other (so it can't be swapped under us).
fn wg_helper_override_allowed(p: &str) -> bool {
    if cfg!(debug_assertions) {
        return true;
    }
    if std::env::var("WG_ALLOW_UNSAFE_HELPER").as_deref() != Ok("1") {
        return false;
    }
    trusted_helper(std::path::Path::new(p))
}

/// Resolve an allowed `$WG_HELPER` override, this application's trusted
/// installed or adjacent helper, or a development helper in debug builds.
pub fn helper_path() -> Option<&'static str> {
    HELPER
        .get_or_init(|| {
            if let Ok(path) = std::env::var("WG_HELPER")
                && wg_helper_override_allowed(&path)
            {
                return Some(path);
            }
            for path in [
                "/usr/local/lib/wireguard-tui/wg-helper",
                "/usr/lib/wireguard-tui/wg-helper",
            ] {
                if trusted_helper(std::path::Path::new(path)) {
                    return Some(path.to_string());
                }
            }
            if let Some(path) = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join("wg-helper")))
                && (trusted_helper(&path) || (cfg!(debug_assertions) && path.is_file()))
            {
                return Some(path.to_string_lossy().into_owned());
            }
            // Development fallbacks are never baked into release helper selection:
            // an old build directory must not become a root executable later.
            if cfg!(debug_assertions) {
                for rel in ["target/debug/wg-helper", "target/release/wg-helper"] {
                    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
                    if path.is_file() {
                        return Some(path.to_string_lossy().into_owned());
                    }
                }
            }
            None
        })
        .as_deref()
}

pub fn trusted_helper(path: &std::path::Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    if !path.is_absolute() {
        return false;
    }
    let Ok(canonical) = path.canonicalize() else {
        return false;
    };
    let Ok(meta) = canonical.metadata() else {
        return false;
    };
    if !meta.is_file() || meta.uid() != 0 || meta.mode() & 0o022 != 0 || meta.mode() & 0o111 == 0 {
        return false;
    }
    // A root-owned file in a user-writable directory can still be replaced.
    // Verify both the spelling's parents and the resolved target's parents.
    [path, canonical.as_path()].into_iter().all(|file| {
        file.ancestors().skip(1).all(|dir| {
            dir.metadata()
                .is_ok_and(|meta| meta.is_dir() && meta.uid() == 0 && meta.mode() & 0o022 == 0)
        })
    })
}

/// Turn a failed spawn into an actionable message. The common dead-end is a
/// server over SSH where the user isn't a sudoer and `pkexec` (polkit) isn't
/// installed - "No such file or directory" then refers to the missing `pkexec`,
/// not the helper. Tell the user how to get out of it.
fn spawn_error(esc: Escalation, helper: &str, e: &std::io::Error) -> HelperError {
    if e.kind() == std::io::ErrorKind::NotFound {
        return HelperError::NotInstalled(match esc {
            Escalation::Direct => {
                format!("helper not found at {helper} - run ./install.sh (or set it up) first")
            }
            Escalation::Sudo => "'sudo' not found on PATH".to_string(),
            Escalation::Pkexec => "can't gain root: no passwordless sudo, and pkexec (polkit) \
                 isn't installed. Re-run ./install.sh (it now sets up sudo for you), or start \
                 this as root ('su -' then run it again)."
                .to_string(),
        });
    }
    HelperError::Io(format!("spawn failed: {e}"))
}

/// Where and how to run the helper. Production code uses the process-wide
/// selection; tests point it at a fake script.
#[derive(Clone, Debug)]
pub struct Invocation {
    pub program: String,
    pub escalation: Escalation,
    pub timeout: Duration,
}

#[cfg(test)]
thread_local! {
    /// Test-only override so backend functions can run against a fake helper.
    pub static TEST_INVOCATION: std::cell::RefCell<Option<Invocation>> =
        const { std::cell::RefCell::new(None) };
}

fn current_invocation() -> Result<Invocation, HelperError> {
    #[cfg(test)]
    if let Some(invocation) = TEST_INVOCATION.with(|slot| slot.borrow().clone()) {
        return Ok(invocation);
    }
    let program = helper_path().ok_or_else(|| {
        HelperError::NotInstalled(
            "No trusted executable wg-helper found; run ./install.sh to install it.".into(),
        )
    })?;
    Ok(Invocation {
        program: program.to_string(),
        escalation: *ESC.get().unwrap_or(&Escalation::Pkexec),
        timeout: HELPER_TIMEOUT,
    })
}

/// Run the helper with a verb (+ optional names) and optional stdin payload.
pub fn helper(args: &[&str], stdin: Option<&str>) -> Result<String, HelperError> {
    let invocation = current_invocation()?;
    run(&invocation, args, stdin)
}

/// Classify a non-zero helper exit. The helper prints a plain message; the
/// escalation tools have their own conventions.
fn classify_failure(
    escalation: Escalation,
    code: Option<i32>,
    command: String,
    stderr: String,
) -> HelperError {
    let lower = stderr.to_ascii_lowercase();
    let message = format!("{command}: {stderr}");
    if lower.contains("another helper operation is busy") {
        HelperError::Busy(message)
    } else if (escalation == Escalation::Pkexec && matches!(code, Some(126 | 127)))
        || (escalation == Escalation::Sudo && lower.contains("a password is required"))
        || lower.contains("not authorized")
    {
        HelperError::AuthorizationRequired(message)
    } else {
        HelperError::Failed { command, stderr }
    }
}

pub fn run(
    invocation: &Invocation,
    args: &[&str],
    stdin: Option<&str>,
) -> Result<String, HelperError> {
    let started = Instant::now();
    let verb = args.first().copied().unwrap_or("");
    let result = run_inner(invocation, args, stdin);
    let elapsed = started.elapsed().as_millis().to_string();
    let name = args.get(1).copied().unwrap_or("");
    match &result {
        Ok(_) => logging::event(
            "helper.call",
            &[
                ("verb", verb),
                ("name", name),
                ("outcome", "ok"),
                ("elapsed_ms", &elapsed),
            ],
        ),
        Err(error) => logging::event(
            "helper.call",
            &[
                ("verb", verb),
                ("name", name),
                ("outcome", error.class()),
                ("elapsed_ms", &elapsed),
                ("error", &error.to_string()),
            ],
        ),
    }
    result
}

fn run_inner(
    invocation: &Invocation,
    args: &[&str],
    stdin: Option<&str>,
) -> Result<String, HelperError> {
    let esc = invocation.escalation;
    let helper = invocation.program.as_str();

    // Polling must not open authentication dialogs every refresh. pkcheck
    // without --allow-user-interaction only accepts an existing authorization;
    // explicit mutating actions still use pkexec's normal authentication flow.
    if esc == Escalation::Pkexec
        && read_only_helper_verb(args.first().copied().unwrap_or(""))
        && !polkit_authorized(helper)
    {
        return Err(HelperError::AuthorizationRequired("Helper authorization is required; run ./install.sh to configure the scoped helper grant.".into()));
    }

    let mut cmd = match esc {
        Escalation::Direct => {
            let mut c = Command::new(helper);
            c.args(args);
            c
        }
        Escalation::Sudo => {
            let mut c = Command::new("sudo");
            c.arg("-n").arg(helper).args(args);
            c
        }
        Escalation::Pkexec => {
            let mut c = Command::new("pkexec");
            c.arg("--disable-internal-agent").arg(helper).args(args);
            c
        }
    };

    use std::os::unix::process::CommandExt;
    cmd.process_group(0);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if stdin.is_some() {
        cmd.stdin(Stdio::piped());
    }

    let child = cmd.spawn().map_err(|e| spawn_error(esc, helper, &e))?;
    let out = helper_output_timeout(child, stdin.map(str::to_owned), invocation.timeout)?;
    if !out.status.success() {
        let err = secrets::redact_config(&String::from_utf8_lossy(&out.stderr));
        return Err(classify_failure(
            esc,
            out.status.code(),
            format!("{} {}", helper, args.join(" ")),
            err.trim().to_string(),
        ));
    }
    String::from_utf8(out.stdout)
        .map_err(|_| HelperError::Io("wg-helper returned invalid UTF-8.".to_string()))
}

const MAX_HELPER_OUTPUT: usize = 4 * 1024 * 1024;

fn nonblocking(pipe: &impl std::os::fd::AsRawFd) -> Result<(), HelperError> {
    let fd = pipe.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        Err(HelperError::Io(std::io::Error::last_os_error().to_string()))
    } else {
        Ok(())
    }
}

fn drain_helper_pipe(
    pipe: &mut Option<impl std::io::Read>,
    bytes: &mut Vec<u8>,
) -> Result<(), HelperError> {
    let mut chunk = [0; 8192];
    for _ in 0..16 {
        let Some(reader) = pipe else {
            return Ok(());
        };
        match reader.read(&mut chunk) {
            Ok(0) => {
                *pipe = None;
                return Ok(());
            }
            Ok(n) if bytes.len() + n <= MAX_HELPER_OUTPUT => bytes.extend_from_slice(&chunk[..n]),
            Ok(_) => {
                return Err(HelperError::Io(
                    "Helper output exceeds the 4 MiB safety limit.".into(),
                ));
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(HelperError::Io(e.to_string())),
        }
    }
    Ok(())
}

pub fn helper_output_timeout(
    mut child: std::process::Child,
    input: Option<String>,
    timeout: Duration,
) -> Result<std::process::Output, HelperError> {
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let mut stdin = child.stdin.take();
    let input = input.unwrap_or_default().into_bytes();
    let mut offset = 0;
    let mut failed_input = false;
    let mut out = Vec::new();
    let mut err = Vec::new();
    let mut status = None;
    let started = Instant::now();
    let unavailable = |what: &str| HelperError::Io(format!("Helper {what} is unavailable."));
    let result = (|| {
        nonblocking(stdout.as_ref().ok_or_else(|| unavailable("stdout"))?)?;
        nonblocking(stderr.as_ref().ok_or_else(|| unavailable("stderr"))?)?;
        if let Some(pipe) = &stdin {
            nonblocking(pipe)?;
        }
        loop {
            drain_helper_pipe(&mut stdout, &mut out)?;
            drain_helper_pipe(&mut stderr, &mut err)?;
            if offset == input.len() {
                stdin = None;
            }
            if let Some(pipe) = &mut stdin {
                match pipe.write(&input[offset..]) {
                    Ok(0) => {
                        failed_input = true;
                        stdin = None;
                    }
                    Ok(n) => offset += n,
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                        ) => {}
                    Err(_) => {
                        failed_input = true;
                        stdin = None;
                    }
                }
            }
            // Keep the leader unreaped while any pipe can delay completion:
            // its PID then remains reserved until group cleanup.
            if status.is_none() && stdout.is_none() && stderr.is_none() && stdin.is_none() {
                status = child
                    .try_wait()
                    .map_err(|e| HelperError::Io(e.to_string()))?;
            }
            if status.is_some() && stdout.is_none() && stderr.is_none() {
                break;
            }
            if started.elapsed() >= timeout {
                return Err(HelperError::Timeout);
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        let status = status.ok_or_else(|| HelperError::Io("Missing helper exit status.".into()))?;
        if status.success() && (failed_input || offset != input.len()) {
            return Err(HelperError::Io(
                "Could not send the complete config to wg-helper.".into(),
            ));
        }
        Ok(std::process::Output {
            status,
            stdout: out,
            stderr: err,
        })
    })();
    if result.is_err() && status.is_none() {
        // Each helper command starts in its own group. Close our pipes and
        // terminate what we can; do not wait forever on an escaped descendant
        // or a privileged process that the UI user cannot signal.
        stdin.take();
        stdout.take();
        stderr.take();
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGKILL);
        }
        let _ = child.kill();
        let deadline = Instant::now();
        while deadline.elapsed() < Duration::from_millis(200) {
            if child.try_wait().ok().flatten().is_some() {
                return result;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
    result
}

/// Verbs that never mutate state; they must not trigger interactive auth.
pub fn read_only_helper_verb(verb: &str) -> bool {
    matches!(
        verb,
        "list"
            | "active"
            | "read"
            | "dump"
            | "is-enabled"
            | "showconf"
            | "log"
            | "killswitch-status"
    )
}

fn polkit_subject(stat: &str, pid: u32, uid: u32) -> Option<String> {
    // /proc stat field 2 (comm) can itself contain spaces and ')'. Field22 is
    // starttime; use pid,starttime,uid as required by pkcheck to avoid PID reuse.
    let start = stat.rsplit_once(')')?.1.split_whitespace().nth(19)?;
    if start.is_empty() || !start.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(format!("{pid},{start},{uid}"))
}

fn polkit_authorized(helper: &str) -> bool {
    let Ok(stat) = std::fs::read_to_string("/proc/self/stat") else {
        return false;
    };
    let Some(subject) = polkit_subject(&stat, std::process::id(), unsafe { libc::geteuid() })
    else {
        return false;
    };
    let mut cmd = Command::new("pkcheck");
    cmd.args([
        "--action-id",
        "org.freedesktop.policykit.exec",
        "--process",
        &subject,
        "--detail",
        "program",
        helper,
        "--detail",
        "user",
        "root",
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null());
    let Ok(mut child) = cmd.spawn() else {
        return false;
    };
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if start.elapsed() < Duration::from_secs(3) => {
                std::thread::sleep(Duration::from_millis(10))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

/// Run unprivileged `wg <verb>` (key generation) with a short deadline.
pub fn wg_output(verb: &str, input: Option<String>) -> Result<std::process::Output, String> {
    use std::os::unix::process::CommandExt;
    let executable = if unsafe { libc::geteuid() } == 0 && !cfg!(debug_assertions) {
        ["/usr/sbin/wg", "/usr/bin/wg", "/sbin/wg", "/bin/wg"]
            .into_iter()
            .find(|path| trusted_helper(std::path::Path::new(path)))
            .ok_or("No trusted wg executable found.")?
    } else {
        "wg"
    };
    let mut command = Command::new(executable);
    command
        .arg(verb)
        .process_group(0)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = command.spawn().map_err(|e| format!("wg {verb}: {e}"))?;
    helper_output_timeout(child, input, Duration::from_secs(10)).map_err(String::from)
}

/// Run `f` with backend helper calls on this thread routed to a fake helper.
#[cfg(test)]
pub fn with_fake_helper<T>(
    program: &std::path::Path,
    timeout: Duration,
    f: impl FnOnce() -> T,
) -> T {
    TEST_INVOCATION.with(|slot| {
        *slot.borrow_mut() = Some(Invocation {
            program: program.to_string_lossy().into_owned(),
            escalation: Escalation::Direct,
            timeout,
        })
    });
    let result = f();
    TEST_INVOCATION.with(|slot| *slot.borrow_mut() = None);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{fake_helper, scratch_dir};

    fn direct(program: &std::path::Path, timeout: Duration) -> Invocation {
        Invocation {
            program: program.to_string_lossy().into_owned(),
            escalation: Escalation::Direct,
            timeout,
        }
    }

    #[test]
    fn helper_capture_drains_both_streams_and_caps_memory() {
        let child = Command::new("sh")
            .args([
                "-c",
                "head -c 262144 /dev/zero; head -c 262144 /dev/zero >&2",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let out = helper_output_timeout(child, None, HELPER_TIMEOUT).unwrap();
        assert!(out.status.success());
        assert_eq!((out.stdout.len(), out.stderr.len()), (262144, 262144));
        let child = Command::new("sh")
            .args(["-c", "head -c 4194305 /dev/zero"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        assert!(
            helper_output_timeout(child, None, HELPER_TIMEOUT)
                .unwrap_err()
                .to_string()
                .contains("4 MiB")
        );
    }

    #[test]
    fn helper_escaped_pipe_fixture() {
        if std::env::var_os("WG_TEST_ESCAPE_PIPE").is_none() {
            return;
        }
        // Fork after exec: doing this in pre_exec would also inherit Rust's
        // exec-error pipe and make Command::spawn itself wait for the sleeper.
        unsafe {
            let pid = libc::fork();
            assert!(pid >= 0);
            if pid == 0 {
                libc::setsid();
                libc::sleep(2);
                libc::_exit(0);
            }
        }
    }

    #[test]
    fn helper_deadlines_cover_blocked_stdin_and_escaped_output_pipes() {
        use std::os::unix::process::CommandExt;
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "sleep 30"]);
        cmd.process_group(0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let started = Instant::now();
        let error = helper_output_timeout(
            cmd.spawn().unwrap(),
            Some("x".repeat(1024 * 1024)),
            Duration::from_millis(100),
        )
        .unwrap_err();
        assert!(error.is_timeout());
        assert!(error.to_string().contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(2));
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args([
            "--exact",
            "backend::helper::tests::helper_escaped_pipe_fixture",
            "--nocapture",
        ])
        .env("WG_TEST_ESCAPE_PIPE", "1");
        cmd.process_group(0)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let started = Instant::now();
        assert!(
            helper_output_timeout(cmd.spawn().unwrap(), None, Duration::from_millis(100))
                .unwrap_err()
                .is_timeout()
        );
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn helper_invocation_times_out_with_a_typed_unknown_outcome() {
        let dir = scratch_dir("helper-timeout");
        let program = fake_helper(&dir, "sleep 30");
        let started = Instant::now();
        let error = run(
            &direct(&program, Duration::from_millis(150)),
            &["up", "wg0"],
            None,
        )
        .unwrap_err();
        assert_eq!(error, HelperError::Timeout);
        assert!(started.elapsed() < Duration::from_secs(3));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn helper_failures_are_classified_and_keep_their_wording() {
        let dir = scratch_dir("helper-classify");
        let program = fake_helper(
            &dir,
            "case \"$1\" in\n\
             busy) echo 'wg-helper: another helper operation is busy; retry when it completes' >&2; exit 2;;\n\
             denied) echo 'wg-helper: write /etc/wireguard/wg0.conf: Permission denied (os error 13)' >&2; exit 2;;\n\
             secret) echo 'PrivateKey = SECRETVALUE' >&2; exit 2;;\n\
             ok) printf 'fine';;\n\
             esac",
        );
        let invocation = direct(&program, Duration::from_secs(5));
        let busy = run(&invocation, &["busy"], None).unwrap_err();
        assert!(matches!(busy, HelperError::Busy(_)));
        assert!(
            busy.to_string()
                .ends_with("another helper operation is busy; retry when it completes")
        );
        let denied = run(&invocation, &["denied", "wg0"], None).unwrap_err();
        assert!(matches!(denied, HelperError::Failed { .. }));
        assert_eq!(
            denied.to_string(),
            format!(
                "{} denied wg0: wg-helper: write /etc/wireguard/wg0.conf: Permission denied (os error 13)",
                program.display()
            )
        );
        let secret = run(&invocation, &["secret"], None).unwrap_err();
        assert!(!secret.to_string().contains("SECRETVALUE"));
        assert_eq!(run(&invocation, &["ok"], None).unwrap(), "fine");
        let missing = run(
            &direct(&dir.join("absent"), Duration::from_secs(1)),
            &["list"],
            None,
        )
        .unwrap_err();
        assert!(matches!(missing, HelperError::NotInstalled(_)));
        assert!(matches!(
            classify_failure(
                Escalation::Pkexec,
                Some(126),
                "pkexec".into(),
                "Not authorized".into()
            ),
            HelperError::AuthorizationRequired(_)
        ));
        assert!(matches!(
            classify_failure(
                Escalation::Sudo,
                Some(1),
                "sudo".into(),
                "sudo: a password is required".into()
            ),
            HelperError::AuthorizationRequired(_)
        ));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn helper_discovery_requires_a_trusted_executable() {
        assert!(!trusted_helper(std::path::Path::new("relative-helper")));
        let dir = scratch_dir("helper-trust");
        let helper = dir.join("wg-helper");
        std::fs::write(&helper, b"untrusted file").unwrap();
        assert!(!trusted_helper(&helper));
        std::fs::remove_dir_all(dir).unwrap();
        assert!(trusted_helper(std::path::Path::new("/bin/sh")));
    }

    #[test]
    fn background_reads_never_request_interactive_authentication() {
        for verb in [
            "list",
            "active",
            "read",
            "dump",
            "is-enabled",
            "showconf",
            "log",
            "killswitch-status",
        ] {
            assert!(read_only_helper_verb(verb));
        }
        for verb in [
            "save",
            "up",
            "down",
            "delete",
            "rename",
            "persist",
            "sync",
            "killswitch-enable",
            "killswitch-disable",
            "killswitch-repair",
        ] {
            assert!(!read_only_helper_verb(verb));
        }
        let fields = (3..=22)
            .map(|field| if field == 22 { "123456" } else { "0" })
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(
            polkit_subject(&format!("42 (name with ) spaces) {fields}"), 42, 1000),
            Some("42,123456,1000".into())
        );
        assert_eq!(polkit_subject("bad proc text", 42, 1000), None);
    }
}
