//! Running one side as a child process. Language-neutral.
//!
//! A side is any program that speaks the line protocol: an `init` request,
//! then `call` requests, then `exit`. If it dies or hangs on a case, that
//! case is recorded as failed and a new process is started for the next.

use serde_json::Value as J;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::Duration;

pub struct SideConfig {
    pub name: &'static str,
    pub program: PathBuf,
    pub args: Vec<String>,
    /// The `init` request.
    pub init: J,
    /// Everything the side writes to stdout or stderr ends up here.
    pub log: PathBuf,
}

struct Proc {
    child: Child,
    stdin: ChildStdin,
    replies: Receiver<String>,
}

pub struct Side {
    pub cfg: SideConfig,
    proc: Option<Proc>,
    /// The side's answer to `init`: interpreter and module location.
    pub hello: J,
    /// Files of the code the side loaded, as it reported them.
    pub files: BTreeSet<String>,
    /// For each function called: the file(s) the side said define it.
    pub defined_in: BTreeMap<String, BTreeSet<String>>,
    /// The module's own public functions: what a case could call.
    pub public_functions: BTreeSet<String>,
    /// The interpreter's installed-package directories (site-packages).
    pub package_dirs: Vec<std::path::PathBuf>,
    /// For each function called: Python function calls made while it ran,
    /// summed over its cases, as [own, shared, other]. Absent when counting
    /// was switched off.
    pub python_calls: BTreeMap<String, [u64; 3]>,
    /// For each function called: in how many cases the count could not be
    /// taken, and why (the reason of the first such case).
    pub python_calls_unavailable: BTreeMap<String, (u64, String)>,
    requests: u64,
    /// For each function called: `python` or `extension`.
    pub implementation: BTreeMap<String, String>,
    /// Files in the other side's source directories that this side's Python
    /// code opened. Any entry stops the run.
    pub opened_other: BTreeSet<String>,
    /// Processes this side started: the first few, and how many in all.
    pub processes: Vec<String>,
    pub process_count: u64,
    pub restarts: u32,
}

pub enum Reply {
    /// The side's answer to the request, as JSON.
    Line(String),
    /// Something other than the answer to this request arrived. The process
    /// has been stopped, since what it sends next cannot be trusted either.
    Protocol(String),
    Crashed(String),
    TimedOut,
}

impl Side {
    pub fn start(cfg: SideConfig, timeout: Duration) -> Result<Side, String> {
        let mut side = Side {
            cfg,
            proc: None,
            hello: J::Null,
            files: BTreeSet::new(),
            defined_in: BTreeMap::new(),
            public_functions: BTreeSet::new(),
            package_dirs: Vec::new(),
            python_calls: BTreeMap::new(),
            python_calls_unavailable: BTreeMap::new(),
            requests: 0,
            implementation: BTreeMap::new(),
            opened_other: BTreeSet::new(),
            processes: Vec::new(),
            process_count: 0,
            restarts: 0,
        };
        side.spawn(timeout)?;
        Ok(side)
    }

    fn spawn(&mut self, timeout: Duration) -> Result<(), String> {
        let name = self.cfg.name;
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.cfg.log)
            .map_err(|e| format!("{name}: cannot open log: {e}"))?;
        let mut child = Command::new(&self.cfg.program)
            .args(&self.cfg.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(log))
            .spawn()
            .map_err(|e| format!("{name}: cannot start {}: {e}", self.cfg.program.display()))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let (tx, replies) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        self.proc = Some(Proc {
            child,
            stdin,
            replies,
        });
        let init = self.cfg.init.to_string();
        let hello = match self.exchange(&init, timeout) {
            Reply::Line(l) => serde_json::from_str::<J>(&l)
                .map_err(|e| format!("{name}: bad reply to init: {e}"))?,
            Reply::Crashed(d) => return Err(format!("{name}: died while starting: {d}")),
            Reply::Protocol(d) => return Err(format!("{name}: {d}")),
            Reply::TimedOut => {
                return Err(format!(
                    "{name}: no answer to init within {}s",
                    timeout.as_secs()
                ));
            }
        };
        if hello.get("ok").and_then(J::as_bool) != Some(true) {
            let why = hello
                .get("error")
                .and_then(J::as_str)
                .unwrap_or("no reason given");
            self.kill();
            return Err(format!("{name}: could not start: {why}"));
        }
        self.absorb(&hello);
        self.hello = hello;
        Ok(())
    }

    /// Takes from a reply what every reply may carry: loaded files, the
    /// module's functions, and what the audit hook saw.
    pub fn absorb(&mut self, reply: &J) {
        let strings = |from: &J, key: &str| {
            let list = from.get(key).and_then(J::as_array);
            list.into_iter()
                .flatten()
                .filter_map(J::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        };
        self.files.extend(strings(reply, "files"));
        self.public_functions
            .extend(strings(reply, "public_functions"));
        for dir in strings(reply, "package_dirs") {
            if !self
                .package_dirs
                .iter()
                .any(|d| d.as_os_str() == dir.as_str())
            {
                self.package_dirs.push(dir.into());
            }
        }
        if let Some(watch) = reply.get("watch") {
            self.opened_other
                .extend(strings(watch, "opened_other_side"));
            let room = 20usize.saturating_sub(self.processes.len());
            self.processes
                .extend(strings(watch, "processes").into_iter().take(room));
            self.process_count += watch.get("process_count").and_then(J::as_u64).unwrap_or(0);
        }
    }

    fn kill(&mut self) -> String {
        match self.proc.take() {
            Some(mut p) => {
                let _ = p.child.kill();
                p.child
                    .wait()
                    .map_or_else(|e| e.to_string(), |s| s.to_string())
            }
            None => "not running".to_string(),
        }
    }

    fn log_since(&self, offset: u64) -> String {
        let mut text = Vec::new();
        if let Ok(mut f) = std::fs::File::open(&self.cfg.log) {
            let len = f.metadata().map_or(0, |m| m.len());
            let from = offset.max(len.saturating_sub(4000));
            if f.seek(SeekFrom::Start(from)).is_ok() {
                let _ = f.read_to_end(&mut text);
            }
        }
        String::from_utf8_lossy(&text).trim().to_string()
    }

    fn exchange(&mut self, request: &str, timeout: Duration) -> Reply {
        let log_start = std::fs::metadata(&self.cfg.log).map_or(0, |m| m.len());
        // Each request gets an id nobody could have known beforehand, and
        // only a line carrying it back is its answer.
        self.requests += 1;
        let seed = format!(
            "{:p} {} {:?} {}",
            self,
            std::process::id(),
            std::time::SystemTime::now(),
            self.requests
        );
        let id = crate::provenance::sha256_bytes(seed.as_bytes())[..24].to_string();
        let request = format!(
            "{{\"id\":\"{id}\",{}",
            request.trim_start().trim_start_matches('{')
        );
        let Some(p) = self.proc.as_mut() else {
            return Reply::Crashed("not running".to_string());
        };
        // A line already waiting answers no request: something else wrote it.
        if let Ok(stray) = p.replies.try_recv() {
            self.kill();
            return Reply::Protocol(format!(
                "a line arrived on the protocol channel that answers no request, so the side was stopped: {}",
                stray.chars().take(200).collect::<String>()
            ));
        }
        // A failed write means the process is gone; the read below reports it.
        let _ = writeln!(p.stdin, "{request}").and_then(|()| p.stdin.flush());
        match p.replies.recv_timeout(timeout) {
            Ok(line) => {
                let answers = serde_json::from_str::<J>(&line)
                    .ok()
                    .is_some_and(|j| j.get("id").and_then(J::as_str) == Some(id.as_str()));
                if answers {
                    return Reply::Line(line);
                }
                self.kill();
                Reply::Protocol(format!(
                    "a line arrived that is not the answer to this request (it does not carry the request's id), so the side was stopped. Something other than the driver wrote to the protocol channel: {}",
                    line.chars().take(200).collect::<String>()
                ))
            }
            Err(RecvTimeoutError::Timeout) => {
                self.kill();
                Reply::TimedOut
            }
            Err(RecvTimeoutError::Disconnected) => {
                let status = match self.proc.take() {
                    Some(mut p) => p
                        .child
                        .wait()
                        .map_or_else(|e| e.to_string(), |s| s.to_string()),
                    None => "unknown".to_string(),
                };
                Reply::Crashed(format!(
                    "process ended ({status}) without answering. Its last output:\n{}",
                    self.log_since(log_start)
                ))
            }
        }
    }

    /// Sends one `call`. `Err` means the side could not be brought back
    /// after an earlier crash, which ends the whole run.
    pub fn call(&mut self, request: &str, timeout: Duration) -> Result<Reply, String> {
        if self.proc.is_none() {
            self.restarts += 1;
            self.spawn(timeout)?;
        }
        Ok(self.exchange(request, timeout))
    }

    /// Asks the side which files it loaded and lets it exit.
    pub fn finish(&mut self) {
        if self.proc.is_some() {
            if let Reply::Line(l) = self.exchange(r#"{"op":"exit"}"#, Duration::from_secs(30))
                && let Ok(j) = serde_json::from_str::<J>(&l)
            {
                self.absorb(&j);
            }
            if let Some(mut p) = self.proc.take() {
                drop(p.stdin);
                let _ = p.child.wait();
            }
        }
    }
}

impl Side {
    /// True for a file in the interpreter's installed-package directories:
    /// third-party code, wherever the virtualenv happens to be.
    pub fn is_installed_package(&self, file: &std::path::Path) -> bool {
        self.package_dirs.iter().any(|d| file.starts_with(d))
    }
}

impl Drop for Side {
    fn drop(&mut self) {
        self.kill();
    }
}
