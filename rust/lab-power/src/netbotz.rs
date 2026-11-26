//! The APC `NetBotz` 250's switched outlet through its SSH CLI.
//!
//! The NMC runs no SSH exec channel (it closes without output), so this drives an interactive
//! session: `ssh -tt` with password authentication through askpass, a pinned `known_hosts` and
//! strict host-key checking, then `nboutlet` / `nboutlet -s On|Off` (current state) /
//! `nboutlet -ns On|Off` (normal state: the one after the `NetBotz` restarts) at the `apc>` prompt and
//! always `bye` at the end (the NMC allows few sessions). Outlet state is read from the table:
//!
//! ```text
//! Name                 Location             State                Normal State
//! Outlet               Unknown              Off                  Off
//! E000: Success
//! ```
//!
//! ssh's stderr is only classified (unreachable, auth failed, host key), never repeated: it can
//! carry host-key fingerprints.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::config::NetBotz;
use crate::secure::{self, ASKPASS_ENV};

/// The switched outlet as `nboutlet` shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Outlet {
    pub(crate) name: String,
    pub(crate) location: String,
    pub(crate) on: bool,
    pub(crate) normal_on: bool,
}

/// Where a session failed, for the error text.
#[derive(Clone, Copy)]
enum Phase {
    Login,
    Command,
}

/// One logged-in CLI session. Dropping it logs out.
pub(crate) struct Session {
    child: Child,
    stdin: Option<ChildStdin>,
    rx: Receiver<Vec<u8>>,
    stderr: Arc<Mutex<Vec<u8>>>,
    buf: Vec<u8>,
    timeout: Duration,
    logged_in: bool,
    host: String,
}

impl Session {
    /// Checks the private files, starts ssh and waits for the first prompt.
    pub(crate) fn open(cfg: &NetBotz, timeout: Duration) -> Result<Self, String> {
        let credentials = secure::path_from_env(&cfg.credentials_env, "NetBotz credentials file")?;
        let known_hosts = secure::path_from_env(&cfg.known_hosts_env, "pinned NetBotz known_hosts file")?;
        let user = secure::username(&credentials)?;
        secure::check_private(&known_hosts, "known_hosts file")?;
        let askpass: PathBuf =
            std::env::current_exe().map_err(|e| format!("cannot find the lab-power binary for askpass: {e}"))?;
        let mut cmd = Command::new(&cfg.ssh);
        cmd.args(ssh_args(cfg, &known_hosts, &user, timeout))
            .env("SSH_ASKPASS", &askpass)
            .env("SSH_ASKPASS_REQUIRE", "force")
            .env("DISPLAY", "lab-power")
            .env(ASKPASS_ENV, &credentials)
            .env_remove("SSH_AUTH_SOCK")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("cannot run {}: {}", cfg.ssh, e.kind()))?;
        let (tx, rx) = mpsc::channel();
        let mut stdout = child.stdout.take().ok_or("ssh has no stdout")?;
        thread::spawn(move || {
            let mut chunk = [0u8; 4096];
            while let Ok(n) = stdout.read(&mut chunk) {
                if n == 0 || tx.send(chunk.get(..n).unwrap_or_default().to_vec()).is_err() {
                    break;
                }
            }
        });
        let stderr = Arc::new(Mutex::new(Vec::new()));
        if let Some(mut pipe) = child.stderr.take() {
            let sink = Arc::clone(&stderr);
            thread::spawn(move || {
                let mut chunk = [0u8; 4096];
                while let Ok(n) = pipe.read(&mut chunk) {
                    if n == 0 {
                        break;
                    }
                    if let Ok(mut s) = sink.lock()
                        && s.len() < 65_536
                    {
                        s.extend_from_slice(chunk.get(..n).unwrap_or_default());
                    }
                }
            });
        }
        let stdin = child.stdin.take();
        let mut session = Self {
            child,
            stdin,
            rx,
            stderr,
            buf: Vec::new(),
            timeout,
            logged_in: false,
            host: cfg.host.clone(),
        };
        // connect + key exchange + password: allow more than one prompt's time.
        session.expect_prompt(timeout * 2 + Duration::from_secs(2), Phase::Login)?;
        session.logged_in = true;
        Ok(session)
    }

    fn expect_prompt(&mut self, wait: Duration, phase: Phase) -> Result<String, String> {
        let deadline = Instant::now() + wait;
        loop {
            if ends_with_prompt(&self.buf) {
                return Ok(String::from_utf8_lossy(&std::mem::take(&mut self.buf)).into_owned());
            }
            let left = deadline.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(left) {
                Ok(chunk) => {
                    self.buf.extend_from_slice(&chunk);
                    if self.buf.len() > 1 << 20 {
                        return Err(format!(
                            "NetBotz {}: unexpected CLI output (no prompt in 1 MiB)",
                            self.host
                        ));
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    return Err(match phase {
                        Phase::Login => format!(
                            "NetBotz {}: no CLI prompt within {}s of starting ssh (unreachable, or the login hung)",
                            self.host,
                            wait.as_secs()
                        ),
                        Phase::Command => {
                            format!("NetBotz {}: CLI prompt timed out after {}s", self.host, wait.as_secs())
                        }
                    });
                }
                Err(RecvTimeoutError::Disconnected) => return Err(self.closed_error(phase)),
            }
        }
    }

    fn closed_error(&mut self, phase: Phase) -> String {
        let status = wait_for(&mut self.child, Duration::from_secs(2));
        // the stderr thread may still be draining
        thread::sleep(Duration::from_millis(50));
        let text = self
            .stderr
            .lock()
            .map(|s| String::from_utf8_lossy(&s).into_owned())
            .unwrap_or_default();
        let host = &self.host;
        if let Some(why) = classify(&text) {
            return format!("NetBotz {host}: {why}");
        }
        match (phase, status) {
            (Phase::Login, Some(code)) => {
                format!("NetBotz {host}: ssh exited (status {code}) before the CLI prompt (session limit reached?)")
            }
            (Phase::Login, None) => format!("NetBotz {host}: ssh closed its output before the CLI prompt"),
            (Phase::Command, _) => format!("NetBotz {host}: the SSH session closed mid-command (inactivity timeout?)"),
        }
    }

    /// Runs one CLI command and returns its output without the echo and the prompt.
    pub(crate) fn command(&mut self, command: &str) -> Result<String, String> {
        let stdin = self.stdin.as_mut().ok_or("the SSH session is closed")?;
        stdin
            .write_all(format!("{command}\n").as_bytes())
            .and_then(|()| stdin.flush())
            .map_err(|_| self.closed_error(Phase::Command))?;
        let raw = self.expect_prompt(self.timeout, Phase::Command)?;
        Ok(strip_echo_and_prompt(&raw, command))
    }

    pub(crate) fn outlet(&mut self) -> Result<Outlet, String> {
        let out = self.command("nboutlet")?;
        parse_outlet(&out).map_err(|e| format!("NetBotz {}: {e}", self.host))
    }

    /// `nboutlet -s On|Off`, then reads the outlet back until it follows (or `wait` passes).
    pub(crate) fn set_outlet(&mut self, on: bool, wait: Duration) -> Result<Outlet, String> {
        self.set_and_follow("-s", "the outlet", |o| o.on, on, wait)
    }

    /// `nboutlet -ns On|Off` (the state after the `NetBotz` restarts), then reads the "Normal State"
    /// column back until it follows (or `wait` passes). The current state is not touched.
    pub(crate) fn set_normal(&mut self, on: bool, wait: Duration) -> Result<Outlet, String> {
        self.set_and_follow("-ns", "the normal state", |o| o.normal_on, on, wait)
    }

    fn set_and_follow(
        &mut self,
        flag: &str,
        what: &str,
        field: fn(&Outlet) -> bool,
        on: bool,
        wait: Duration,
    ) -> Result<Outlet, String> {
        let word = if on { "On" } else { "Off" };
        let out = self.command(&format!("nboutlet {flag} {word}"))?;
        status_code(&out).map_err(|e| format!("NetBotz {}: nboutlet {flag} {word}: {e}", self.host))?;
        let deadline = Instant::now() + wait;
        loop {
            let outlet = self.outlet()?;
            if field(&outlet) == on {
                return Ok(outlet);
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "NetBotz {}: nboutlet {flag} {word} was accepted but {what} still reads {}",
                    self.host,
                    on_off(field(&outlet))
                ));
            }
            thread::sleep(Duration::from_millis(500));
        }
    }

    /// Logs out (`bye`) and reaps ssh; kills it if it does not end within 5 s.
    pub(crate) fn close(&mut self) {
        if let Some(mut stdin) = self.stdin.take()
            && self.logged_in
        {
            let _ = stdin.write_all(b"bye\n").and_then(|()| stdin.flush());
        }
        if wait_for(&mut self.child, Duration::from_secs(5)).is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.close();
    }
}

/// ssh's arguments: pinned host key, password auth only, no user config, agent or multiplexing.
fn ssh_args(cfg: &NetBotz, known_hosts: &std::path::Path, user: &str, timeout: Duration) -> Vec<String> {
    let kh = known_hosts.to_str().unwrap_or_default();
    let options = [
        format!("UserKnownHostsFile={kh}"),
        "GlobalKnownHostsFile=/dev/null".into(),
        "StrictHostKeyChecking=yes".into(),
        "UpdateHostKeys=no".into(),
        "CheckHostIP=no".into(),
        "PreferredAuthentications=password".into(),
        "PubkeyAuthentication=no".into(),
        "KbdInteractiveAuthentication=no".into(),
        "NumberOfPasswordPrompts=1".into(),
        format!("ConnectTimeout={}", timeout.as_secs().max(1)),
        "ServerAliveInterval=5".into(),
        "ServerAliveCountMax=3".into(),
        "ControlMaster=no".into(),
        "ControlPath=none".into(),
        "ForwardAgent=no".into(),
    ];
    let mut args: Vec<String> = vec!["-tt".into(), "-F".into(), "/dev/null".into()];
    let extra = cfg
        .host_key_algorithms
        .as_ref()
        .map(|a| format!("HostKeyAlgorithms={a}"));
    for option in options.into_iter().chain(extra) {
        args.push("-o".into());
        args.push(option);
    }
    args.extend([
        "-p".into(),
        cfg.port.to_string(),
        "-l".into(),
        user.to_owned(),
        "--".into(),
        cfg.host.clone(),
    ]);
    args
}

/// The exit status once the child ends within `limit` (-1 for a signal), else None.
fn wait_for(child: &mut Child, limit: Duration) -> Option<i32> {
    let deadline = Instant::now() + limit;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status.code().unwrap_or(-1)),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            _ => return None,
        }
    }
}

/// Maps ssh's own error text to a reason; None when it is not one we know.
fn classify(stderr: &str) -> Option<&'static str> {
    let s = stderr.to_ascii_lowercase();
    if s.contains("remote host identification has changed") {
        Some("HOST KEY CHANGED: the NetBotz key differs from the pinned known_hosts; not logging in")
    } else if s.contains("host key verification failed")
        || s.contains("host key is known")
        || s.contains("no matching host key")
    {
        Some("host key not verified against the pinned known_hosts; not logging in")
    } else if s.contains("permission denied")
        || s.contains("too many authentication failures")
        || s.contains("authentication failed")
    {
        Some("authentication failed (check the credentials file)")
    } else if [
        "connection timed out",
        "operation timed out",
        "no route to host",
        "connection refused",
        "network is unreachable",
        "could not resolve hostname",
        "name or service not known",
        "host is down",
    ]
    .iter()
    .any(|m| s.contains(m))
    {
        Some("unreachable over SSH")
    } else if s.contains("unable to negotiate") || s.contains("no matching") {
        Some(
            "SSH algorithm negotiation failed (the NMC offers only old algorithms: host_key_algorithms in targets.json)",
        )
    } else if s.contains("unsupported option") || s.contains("bad configuration option") || s.contains("usage: ssh") {
        Some("the local ssh rejected lab-power's options (an unusual OpenSSH build?)")
    } else if s.contains("connection closed") || s.contains("connection reset") || s.contains("broken pipe") {
        Some("the NetBotz closed the SSH connection (its session limit may be reached; try again shortly)")
    } else {
        None
    }
}

fn ends_with_prompt(buf: &[u8]) -> bool {
    let trimmed = buf.trim_ascii_end();
    let Some(before) = trimmed.strip_suffix(b"apc>") else {
        return false;
    };
    before.is_empty() || before.ends_with(b"\n") || before.ends_with(b"\r")
}

fn strip_echo_and_prompt(raw: &str, command: &str) -> String {
    let text = raw.replace('\r', "");
    let text = text.trim_end().strip_suffix("apc>").unwrap_or(&text);
    let mut lines: Vec<&str> = text.lines().collect();
    if let Some(i) = lines.iter().position(|l| l.trim() == command) {
        lines.drain(..=i);
    }
    lines.join("\n")
}

fn is_code_line(l: &str) -> bool {
    l.starts_with('E') && l.get(1..4).is_some_and(|d| d.bytes().all(|b| b.is_ascii_digit())) && l.get(4..5) == Some(":")
}

/// The `Exxx:` line: Ok for E000, else the CLI's code and message.
pub(crate) fn status_code(out: &str) -> Result<(), String> {
    let line = out
        .lines()
        .map(str::trim)
        .find(|l| is_code_line(l))
        .ok_or("unexpected CLI output (no Exxx status line)")?;
    if line.starts_with("E000:") {
        Ok(())
    } else {
        // the CLI's own error text (e.g. "E102: Parameter Error"): no secrets in it, keep it short
        Err(format!("CLI error {}", line.chars().take(80).collect::<String>()))
    }
}

/// Parses `nboutlet` output: exactly one outlet row under the Name/Location/State/Normal State header.
pub(crate) fn parse_outlet(out: &str) -> Result<Outlet, String> {
    status_code(out)?;
    let lines: Vec<&str> = out.lines().collect();
    let (hi, header) = lines
        .iter()
        .enumerate()
        .find(|(_, l)| l.contains("Name") && l.contains("Location") && l.contains("Normal State"))
        .ok_or("unexpected nboutlet output (no Name/Location/State/Normal State header)")?;
    let col = |word: &str, from: usize| header.get(from..).and_then(|h| h.find(word)).map(|i| i + from);
    let name_c = col("Name", 0).ok_or("unexpected nboutlet header")?;
    let loc_c = col("Location", name_c).ok_or("unexpected nboutlet header")?;
    let state_c = col("State", loc_c).ok_or("unexpected nboutlet header")?;
    let normal_c = col("Normal State", loc_c).ok_or("unexpected nboutlet header")?;
    if !(name_c < loc_c && loc_c < state_c && state_c < normal_c) {
        return Err("unexpected nboutlet header (column order)".to_owned());
    }
    let rows: Vec<&str> = lines
        .iter()
        .skip(hi + 1)
        .copied()
        .take_while(|l| !is_code_line(l.trim()))
        .filter(|l| !l.trim().is_empty() && !l.trim().chars().all(|c| c == '-'))
        .collect();
    let [row] = rows.as_slice() else {
        return Err(format!(
            "unexpected nboutlet output (expected one outlet row, found {})",
            rows.len()
        ));
    };
    if !row.is_ascii() {
        return Err("unexpected nboutlet output (non-ASCII row)".to_owned());
    }
    let field = |a: usize, b: Option<usize>| -> String {
        match b {
            Some(b) => row.get(a.min(row.len())..b.min(row.len())),
            None => row.get(a.min(row.len())..),
        }
        .unwrap_or_default()
        .trim()
        .to_owned()
    };
    let parse_state = |s: &str| match s.to_ascii_lowercase().as_str() {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err(format!(
            "unexpected nboutlet output (state {:?})",
            s.chars().take(20).collect::<String>()
        )),
    };
    Ok(Outlet {
        name: field(name_c, Some(loc_c)),
        location: field(loc_c, Some(state_c)),
        on: parse_state(&field(state_c, Some(normal_c)))?,
        normal_on: parse_state(&field(normal_c, None))?,
    })
}

pub(crate) fn on_off(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REAL: &str = include_str!("../tests/fixtures/nboutlet-off.txt");

    #[test]
    fn real_transcript() {
        assert!(ends_with_prompt(REAL.as_bytes()));
        let out = strip_echo_and_prompt(REAL, "nboutlet");
        let o = parse_outlet(&out).unwrap();
        assert_eq!(
            o,
            Outlet {
                name: "Outlet".into(),
                location: "Unknown".into(),
                on: false,
                normal_on: false
            }
        );
    }

    #[test]
    fn prompts() {
        assert!(ends_with_prompt(b"x\r\napc>"));
        assert!(ends_with_prompt(b"apc> "));
        assert!(!ends_with_prompt(b"xapc>"));
        assert!(!ends_with_prompt(b"apc> nboutlet"));
    }

    #[test]
    fn odd_output_refused() {
        let on = REAL.replace("Off                  Off", "On                   Off");
        assert!(parse_outlet(&strip_echo_and_prompt(&on, "nboutlet")).unwrap().on);
        for bad in [
            "E000: Success".to_owned(),
            "garbage".to_owned(),
            REAL.replace("E000: Success", "E102: Parameter Error"),
            REAL.replace("Off                  Off", "Maybe                Off"),
            REAL.replace(
                "Outlet               Unknown",
                "Outlet               Unknown\r\nOutlet 2             Unknown              On                   On\r",
            ),
        ] {
            assert!(parse_outlet(&strip_echo_and_prompt(&bad, "nboutlet")).is_err(), "{bad}");
        }
        assert!(status_code("E000: Success").is_ok());
        assert!(status_code("E102: Parameter Error").unwrap_err().contains("E102"));
        assert!(status_code("nothing").is_err());
    }

    #[test]
    fn ssh_errors_classified_without_echo() {
        assert!(
            classify("@@@ WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED! @@@ SHA256:abc")
                .unwrap()
                .contains("CHANGED")
        );
        assert!(
            classify("Host key verification failed.")
                .unwrap()
                .contains("not verified")
        );
        assert!(
            classify("apc@h: Permission denied (password).")
                .unwrap()
                .contains("authentication")
        );
        assert!(
            classify("ssh: connect to host h port 22: Connection timed out")
                .unwrap()
                .contains("unreachable")
        );
        assert!(
            classify("ssh: connect to host h port 22: Connection refused")
                .unwrap()
                .contains("unreachable")
        );
        assert!(classify("something else").is_none());
    }
}
