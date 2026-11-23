//! The host gate's log, as a gated run reads it.
//!
//! `ci/gate.sh` waits until the 1-minute load average has been under 6 at
//! 30 readings in a row, 10 s apart, then runs its command with a reading
//! every 10 s, and appends every line to one log. A run under the gate reads
//! that log twice:
//! - at its start ([`Gate::open`]), to check that the gate it runs under
//!   opened by that rule, is still open, and is the one that started it;
//! - at its end ([`Gate::finish`]), to wait for one more reading and take
//!   every reading from the gate's opening to that one as the run's.
//!
//! Every line starts with the clock time and the epoch seconds the gate
//! read them at; the gap rules use the epoch seconds, so a wait across
//! midnight is judged like any other.
//!
//! A log that fails a check makes the run's figures `unreliable`, with one
//! of the reasons of [`Unreliable`].

use std::path::{Path, PathBuf};
use std::time::Duration;

/// The one rule: this many readings in a row, each under [`LIMIT`].
pub const READINGS: usize = 30;
/// The 1-minute load average every reading must stay under.
pub const LIMIT: f64 = 6.0;
/// Seconds between two readings.
pub const EVERY: u64 = 10;

/// Why a log cannot vouch for a run's load.
#[derive(Debug, Clone, PartialEq)]
pub enum Unreliable {
    /// No log, or no run id given to the run: it was not run under the gate.
    NoGate(String),
    /// The gate did not open by the one rule.
    GateRule(String),
    /// The log's last block belongs to another run.
    NotThisRun(String),
    /// The log's last block has no open gate.
    GateNotOpen(String),
    /// A stretch of the run has no reading.
    LoadNotWatched(String),
}

impl Unreliable {
    /// The reason's name in the report, and what it says.
    pub fn reason(&self) -> (&'static str, &str) {
        match self {
            Unreliable::NoGate(s) => ("no_gate", s),
            Unreliable::GateRule(s) => ("gate_rule", s),
            Unreliable::NotThisRun(s) => ("not_this_run", s),
            Unreliable::GateNotOpen(s) => ("gate_not_open", s),
            Unreliable::LoadNotWatched(s) => ("load_not_watched", s),
        }
    }
}

/// One reading of the load average.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    /// The clock time the gate wrote, `HH:MM:SS`.
    pub time: String,
    /// The epoch seconds it wrote with it.
    pub epoch: u64,
    /// The 1-minute load average.
    pub one_minute: f64,
}

/// The readings taken while the run ran.
#[derive(Debug, Clone, PartialEq)]
pub struct Load {
    /// From the gate's opening to the reading after the run's end.
    pub readings: Vec<Reading>,
    /// Whether any of them is at [`LIMIT`] or over.
    pub at_or_over_limit: bool,
}

/// A gate a run is running under, checked at the run's start.
#[derive(Debug)]
pub struct Gate {
    path: PathBuf,
    /// The line where the run's block starts, its "wait:" header.
    header: usize,
}

impl Gate {
    /// Checks the log's last block at the start of a run whose run id is
    /// `run_id` (`GATE_RUN_ID`, set by the gate).
    pub fn open(path: &Path, run_id: Option<&str>) -> Result<Gate, Unreliable> {
        let Some(run_id) = run_id else {
            return Err(Unreliable::NoGate(
                "no GATE_RUN_ID: the run was not started by ci/gate.sh".into(),
            ));
        };
        let text = std::fs::read_to_string(path)
            .map_err(|e| Unreliable::NoGate(format!("no gate log at {}: {e}", path.display())))?;
        let lines: Vec<&str> = text.lines().collect();
        let Some(header) = lines.iter().rposition(|l| rule(l).is_some()) else {
            return Err(Unreliable::NoGate(format!(
                "{} has no gate's \"wait:\" line",
                path.display()
            )));
        };
        let (readings, limit, every, id) = rule(lines[header]).unwrap();
        if id != Some(run_id) {
            return Err(Unreliable::NotThisRun(format!(
                "the log's last block has run id {}, the run has {run_id}",
                id.unwrap_or("none")
            )));
        }
        let block = &lines[header + 1..];
        if block.iter().any(|l| is(l, "end, exit ")) {
            return Err(Unreliable::GateNotOpen(
                "the log's last block has ended: its gate is not this run's".into(),
            ));
        }
        let opened: Vec<usize> = (0..block.len())
            .filter(|&i| is(block[i], "gate open; "))
            .collect();
        let [open] = opened[..] else {
            return Err(Unreliable::GateNotOpen(format!(
                "the log's last block has {} \"gate open\" lines, not one",
                opened.len()
            )));
        };
        if (readings, limit, every) != (READINGS, LIMIT, EVERY) {
            return Err(Unreliable::GateRule(format!(
                "the gate waited for {readings} readings under {limit}, {every} s apart, not the one rule: {READINGS} under {LIMIT}, {EVERY} s apart"
            )));
        }
        let wait: Vec<Reading> = block[..open].iter().filter_map(|l| reading(l)).collect();
        if wait.len() < READINGS {
            return Err(Unreliable::GateRule(format!(
                "the gate opened after {} readings, not {READINGS}",
                wait.len()
            )));
        }
        let last = &wait[wait.len() - READINGS..];
        if let Some(r) = last.iter().find(|r| r.one_minute >= LIMIT) {
            return Err(Unreliable::GateRule(format!(
                "the gate opened with a reading of {} at {}",
                r.one_minute, r.time
            )));
        }
        if let Some((a, b)) = gap(last.iter().map(|r| (r.time.as_str(), r.epoch))) {
            return Err(Unreliable::GateRule(format!(
                "no reading between {a} and {b}, before the gate opened"
            )));
        }
        Ok(Gate {
            path: path.to_path_buf(),
            header,
        })
    }

    /// At the run's end: waits up to `patience` for one more reading, then
    /// gives the run's readings. The gated command passes two intervals.
    pub fn finish(self, patience: Duration) -> Result<Load, Unreliable> {
        let since_open = |text: &str| -> ((String, u64), Vec<Reading>) {
            let mut block = text
                .lines()
                .skip(self.header + 1)
                .skip_while(|l| !is(l, "gate open; "));
            let open = block
                .next()
                .and_then(stamp)
                .map(|(t, e, _)| (t.to_string(), e))
                .unwrap_or_default();
            (open, block.filter_map(reading).collect())
        };
        let read = || std::fs::read_to_string(&self.path).unwrap_or_default();
        let before = since_open(&read()).1.len();
        let start = std::time::Instant::now();
        let (open, mut readings) = loop {
            let (open, readings) = since_open(&read());
            if readings.len() > before {
                break (open, readings);
            }
            if start.elapsed() >= patience {
                return Err(Unreliable::LoadNotWatched(format!(
                    "no reading was appended to the gate's log within {} s of the run's end",
                    patience.as_secs_f64()
                )));
            }
            std::thread::sleep(Duration::from_millis(50).min(patience));
        };
        readings.truncate(before + 1);
        let times = std::iter::once((open.0.as_str(), open.1))
            .chain(readings.iter().map(|r| (r.time.as_str(), r.epoch)));
        if let Some((a, b)) = gap(times) {
            return Err(Unreliable::LoadNotWatched(format!(
                "no reading between {a} and {b}, while the run ran"
            )));
        }
        let at_or_over_limit = readings.iter().any(|r| r.one_minute >= LIMIT);
        Ok(Load {
            readings,
            at_or_over_limit,
        })
    }
}

/// The rule and run id of a block's header, `<date> <time> <epoch> wait:
/// 30 readings under 6, 10 s apart, run <id>, then: <command>`.
fn rule(line: &str) -> Option<(usize, f64, u64, Option<&str>)> {
    let (_, _, rest) = stamp(line.get(11..)?)?;
    let rest = rest.strip_prefix("wait: ")?;
    let (readings, rest) = rest.split_once(" readings under ")?;
    let (limit, rest) = rest.split_once(", ")?;
    let (every, rest) = rest.split_once(" s apart")?;
    let id = rest
        .strip_prefix(", run ")
        .and_then(|r| r.split_once(", then: "))
        .map(|(id, _)| id);
    Some((
        readings.parse().ok()?,
        limit.parse().ok()?,
        every.parse().ok()?,
        id,
    ))
}

/// Whether `line` is a stamped line whose text starts with `what`.
fn is(line: &str, what: &str) -> bool {
    stamp(line).is_some_and(|(_, _, rest)| rest.starts_with(what))
}

/// A reading, `HH:MM:SS <epoch> load <1 min> <5 min> <15 min>`, with or
/// without the wait's count after it.
fn reading(line: &str) -> Option<Reading> {
    let (time, epoch, rest) = stamp(line)?;
    let one_minute = rest
        .strip_prefix("load ")?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    Some(Reading {
        time: time.to_string(),
        epoch,
        one_minute,
    })
}

/// A line's leading `HH:MM:SS <epoch seconds>`, and the text after them.
fn stamp(line: &str) -> Option<(&str, u64, &str)> {
    let time = line.get(..8)?;
    let clock = time.bytes().enumerate().all(|(i, b)| {
        if i == 2 || i == 5 {
            b == b':'
        } else {
            b.is_ascii_digit()
        }
    });
    let (epoch, rest) = line[8..].strip_prefix(' ')?.split_once(' ')?;
    clock.then_some((time, epoch.parse().ok()?, rest))
}

/// The first two consecutive stamps more than two intervals apart, or out
/// of order, by their epoch seconds.
fn gap<'a>(stamps: impl Iterator<Item = (&'a str, u64)>) -> Option<(&'a str, &'a str)> {
    let stamps: Vec<(&str, u64)> = stamps.collect();
    stamps.windows(2).find_map(|w| {
        let ((a, ea), (b, eb)) = (w[0], w[1]);
        (eb < ea || eb - ea > 2 * EVERY).then_some((a, b))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const ID: &str = "4f2a9c1e7b3d5a60";

    /// A temporary log file, removed when dropped.
    struct Log(PathBuf);

    impl Log {
        fn new(text: &str) -> Log {
            static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("carcinify-gate-{}-{n}.log", std::process::id()));
            std::fs::write(&path, text).unwrap();
            Log(path)
        }
    }

    fn append(path: &Path, line: &str) {
        let mut f = std::fs::OpenOptions::new().append(true).open(path).unwrap();
        writeln!(f, "{line}").unwrap();
    }

    impl Drop for Log {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    /// The epoch seconds of 2026-11-23 10:00:00 +0700.
    const TEN: u64 = 1_795_402_800;

    /// The stamp `HH:MM:SS <epoch>` of `seconds` after 10:00:00.
    fn at(seconds: u64) -> String {
        let t = (36_000 + seconds) % 86_400;
        format!(
            "{:02}:{:02}:{:02} {}",
            t / 3600,
            t / 60 % 60,
            t % 60,
            TEN + seconds
        )
    }

    /// The reading `seconds` after 10:00:00.
    fn r(seconds: u64, one_minute: f64) -> Reading {
        let (time, epoch) = at(seconds)
            .split_once(' ')
            .map(|(t, e)| (t.to_string(), e.parse().unwrap()))
            .unwrap();
        Reading {
            time,
            epoch,
            one_minute,
        }
    }

    /// A block as `ci/gate.sh` writes it: the header, the wait's readings
    /// 10 s apart (each with its count in a row), and "gate open". Returns
    /// the text and the time of the gate's opening.
    fn block(start: u64, id: &str, rule: (usize, &str, u64), wait: &[f64]) -> (String, u64) {
        let timed: Vec<(u64, f64)> = wait
            .iter()
            .enumerate()
            .map(|(i, l)| (start + 10 * i as u64, *l))
            .collect();
        block_at(start, id, rule, &timed)
    }

    /// The same, with each reading's time given.
    fn block_at(
        start: u64,
        id: &str,
        rule: (usize, &str, u64),
        wait: &[(u64, f64)],
    ) -> (String, u64) {
        let (readings, limit, every) = rule;
        let mut text = format!(
            "2026-11-23 {} wait: {readings} readings under {limit}, {every} s apart, run {id}, then: ./run.sh\n",
            at(start)
        );
        let mut in_a_row = 0;
        let mut t = start;
        for &(time, load) in wait {
            t = time;
            in_a_row = if load < 6.0 { in_a_row + 1 } else { 0 };
            text.push_str(&format!(
                "{} load {load:.2} 5.00 5.00 ({in_a_row} under {limit} in a row)\n",
                at(t)
            ));
        }
        text.push_str(&format!("{} gate open; start: ./run.sh\n", at(t)));
        (text, t)
    }

    /// A block opened by the one rule: 30 readings at 5.
    fn opened(start: u64, id: &str) -> (String, u64) {
        block(start, id, (30, "6", 10), &[5.0; 30])
    }

    fn reading(t: u64, load: f64) -> String {
        format!("{} load {load:.2} 5.00 5.00", at(t))
    }

    /// Opens the gate on `log`, has another thread append `later` 50 ms on,
    /// and finishes with a patience of 5 s.
    fn run(log: &Log, later: String) -> Result<Load, Unreliable> {
        let gate = Gate::open(&log.0, Some(ID))?;
        let path = log.0.clone();
        let writer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            append(&path, &later);
        });
        let load = gate.finish(Duration::from_secs(5));
        writer.join().unwrap();
        load
    }

    fn reason(r: Result<impl std::fmt::Debug, Unreliable>) -> &'static str {
        match r {
            Err(Unreliable::NoGate(_)) => "no_gate",
            Err(Unreliable::GateRule(_)) => "gate_rule",
            Err(Unreliable::NotThisRun(_)) => "not_this_run",
            Err(Unreliable::GateNotOpen(_)) => "gate_not_open",
            Err(Unreliable::LoadNotWatched(_)) => "load_not_watched",
            Ok(_) => "ok",
        }
    }

    #[test]
    fn a_run_under_a_quiet_open_gate_gets_its_readings_up_to_the_one_after_its_end() {
        let (text, t) = opened(0, ID);
        let log = Log::new(&text);
        let load = run(&log, reading(t + 10, 5.5)).unwrap();
        assert_eq!(load.readings, vec![r(t + 10, 5.5)]);
        assert!(!load.at_or_over_limit);
    }

    #[test]
    fn a_reading_at_6_or_more_while_the_run_ran_flags_its_load() {
        let (text, t) = opened(0, ID);
        let log = Log::new(&(text + &reading(t + 10, 6.0) + "\n"));
        let load = run(&log, reading(t + 20, 5.0)).unwrap();
        assert_eq!(load.readings.len(), 2);
        assert!(load.at_or_over_limit);
    }

    #[test]
    fn a_run_without_a_log_or_without_a_run_id_was_not_gated() {
        let missing = std::env::temp_dir().join("carcinify-gate-no-such-log.log");
        assert_eq!(reason(Gate::open(&missing, Some(ID))), "no_gate");
        let (text, _) = opened(0, ID);
        let log = Log::new(&text);
        assert_eq!(reason(Gate::open(&log.0, None)), "no_gate");
    }

    #[test]
    fn a_log_written_under_a_test_rule_did_not_gate_by_the_one_rule() {
        for rule in [(1, "6", 10), (30, "8", 10), (30, "6", 1)] {
            let (text, _) = block(0, ID, rule, &[5.0; 30]);
            let log = Log::new(&text);
            assert_eq!(
                reason(Gate::open(&log.0, Some(ID))),
                "gate_rule",
                "{rule:?}"
            );
        }
    }

    #[test]
    fn the_last_30_readings_before_the_gate_opened_must_each_be_under_6_without_a_gap() {
        let mut high = [5.0; 30];
        high[12] = 6.0;
        let (text, _) = block(0, ID, (30, "6", 10), &high);
        assert_eq!(
            reason(Gate::open(&Log::new(&text).0, Some(ID))),
            "gate_rule"
        );
        let (text, _) = block(0, ID, (30, "6", 10), &[5.0; 29]);
        assert_eq!(
            reason(Gate::open(&Log::new(&text).0, Some(ID))),
            "gate_rule"
        );
        // A gap of three intervals between two of the 30 readings.
        let gapped: Vec<(u64, f64)> = (0..30)
            .map(|i| (10 * i + if i < 15 { 0 } else { 20 }, 5.0))
            .collect();
        let (text, _) = block_at(0, ID, (30, "6", 10), &gapped);
        assert_eq!(
            reason(Gate::open(&Log::new(&text).0, Some(ID))),
            "gate_rule"
        );
    }

    #[test]
    fn a_log_whose_last_block_has_another_run_id_is_not_this_run() {
        let (text, _) = opened(0, "0000000000000000");
        assert_eq!(
            reason(Gate::open(&Log::new(&text).0, Some(ID))),
            "not_this_run"
        );
    }

    #[test]
    fn a_log_whose_last_block_has_ended_or_never_opened_has_no_open_gate() {
        let (text, t) = opened(0, ID);
        let ended = format!(
            "{text}{}\n{} end, exit 0; load 5.00 5.00 5.00\n",
            reading(t + 10, 5.0),
            at(t + 15)
        );
        assert_eq!(
            reason(Gate::open(&Log::new(&ended).0, Some(ID))),
            "gate_not_open"
        );
        let (text, _) = opened(0, ID);
        let waiting = text
            .lines()
            .filter(|l| !l.contains("gate open"))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        assert_eq!(
            reason(Gate::open(&Log::new(&waiting).0, Some(ID))),
            "gate_not_open"
        );
        let (text, t) = opened(0, ID);
        let twice = format!("{text}{} gate open; start: ./run.sh\n", at(t));
        assert_eq!(
            reason(Gate::open(&Log::new(&twice).0, Some(ID))),
            "gate_not_open"
        );
    }

    #[test]
    fn a_run_whose_final_reading_never_comes_is_not_watched() {
        let (text, _) = opened(0, ID);
        let log = Log::new(&text);
        let gate = Gate::open(&log.0, Some(ID)).unwrap();
        assert_eq!(
            reason(gate.finish(Duration::from_millis(200))),
            "load_not_watched"
        );
    }

    #[test]
    fn a_gap_of_three_intervals_while_the_run_ran_is_not_watched() {
        let (text, t) = opened(0, ID);
        let log = Log::new(&(text + &reading(t + 10, 5.0) + "\n"));
        assert_eq!(reason(run(&log, reading(t + 40, 5.0))), "load_not_watched");
    }

    #[test]
    fn a_reading_between_the_gates_opening_and_the_runs_start_counts_as_the_runs() {
        let (text, t) = opened(0, ID);
        let log = Log::new(&(text + &reading(t + 10, 5.1) + "\n"));
        let load = run(&log, reading(t + 20, 5.2)).unwrap();
        let loads: Vec<f64> = load.readings.iter().map(|r| r.one_minute).collect();
        assert_eq!(loads, vec![5.1, 5.2]);
    }

    #[test]
    fn a_log_with_an_earlier_block_is_judged_by_its_last_block() {
        let (old, t) = block(0, "0000000000000000", (1, "6", 1), &[9.0]);
        let old = format!(
            "{old}{}\n{} end, exit 1; load 9.00 9.00 9.00\n",
            reading(t + 1, 9.5),
            at(t + 2)
        );
        let (new, t) = opened(1000, ID);
        let log = Log::new(&(old + &new));
        let load = run(&log, reading(t + 10, 5.0)).unwrap();
        assert_eq!(load.readings.len(), 1);
    }

    /// The Linux gate log of 2026-11-23 13:15, up to its opening, with the
    /// command replaced, and with the run id and the epoch seconds that
    /// `ci/gate.sh` now writes (from its clock times, at +0700). Its wait
    /// began at 8.94; its last 30 readings span 291 s, 10 to 11 s apart.
    #[test]
    fn a_copy_of_a_real_gate_log_opened_by_the_one_rule() {
        let text = include_str!("../fixtures/real-gate-log.txt").replace("RUNID", ID);
        let log = Log::new(&text);
        let load = run(&log, "13:21:41 1795414901 load 6.24 6.00 6.26".to_string()).unwrap();
        let expected = Reading {
            time: "13:21:41".into(),
            epoch: 1_795_414_901,
            one_minute: 6.24,
        };
        assert_eq!(load.readings, vec![expected]);
        assert!(load.at_or_over_limit);
    }

    #[test]
    fn a_wait_and_a_run_across_midnight_are_judged_by_their_epoch_seconds() {
        // 30 readings from 23:57:30 to 00:02:20, the gate opening at
        // 00:02:20 and the run's reading at 00:02:30.
        let (text, t) = opened(86_400 - 36_000 - 150, ID);
        let log = Log::new(&text);
        let load = run(&log, reading(t + 10, 5.0)).unwrap();
        assert_eq!(load.readings, vec![r(t + 10, 5.0)]);
        assert!(
            text.contains("23:59:50") && text.contains("00:00:00"),
            "{text}"
        );
        // A day and ten seconds later, the clock reads ten seconds on.
        let (text, t) = opened(0, ID);
        let log = Log::new(&(text + &reading(t + 10, 5.0) + "\n"));
        assert_eq!(
            reason(run(&log, reading(t + 86_410, 5.0))),
            "load_not_watched"
        );
    }

    /// `ci/gate.sh` with `LOG` set and the test-only settings cleared.
    fn gate_script(log: &Path) -> std::process::Command {
        let mut c = std::process::Command::new("bash");
        c.arg(concat!(env!("CARGO_MANIFEST_DIR"), "/../ci/gate.sh"))
            .env("LOG", log)
            .env_remove("GATE_EVERY")
            .env_remove("GATE_READINGS")
            .env_remove("GATE_LIMIT")
            .env_remove("GATE_RUN_ID");
        c
    }

    #[test]
    fn the_gate_script_states_the_one_rule_and_a_run_id_in_its_header() {
        let log = Log::new("");
        // In a process group of its own, so that its `sleep` goes with it.
        let mut gate =
            std::os::unix::process::CommandExt::process_group(&mut gate_script(&log.0), 0)
                .arg("true")
                .spawn()
                .unwrap();
        // The header comes before the first reading; the wait itself would
        // take five minutes, so the script is stopped once it is there.
        let start = std::time::Instant::now();
        let header = loop {
            let text = std::fs::read_to_string(&log.0).unwrap();
            if let Some(line) = text.lines().next() {
                break line.to_string();
            }
            assert!(start.elapsed() < Duration::from_secs(10), "no header");
            std::thread::sleep(Duration::from_millis(20));
        };
        // Bash's own `kill`: a slim Linux image may have no kill program.
        let group = format!("kill -KILL -- -{}", gate.id());
        let killed = std::process::Command::new("bash")
            .args(["-c", &group])
            .status();
        assert!(killed.unwrap().success());
        gate.wait().unwrap();
        let (readings, limit, every, id) = rule(&header).expect(&header);
        assert_eq!(
            (readings, limit, every),
            (READINGS, LIMIT, EVERY),
            "{header}"
        );
        let id = id.expect(&header);
        assert!(
            id.len() == 16 && id.bytes().all(|b| b.is_ascii_hexdigit()),
            "{header}"
        );
    }

    #[test]
    fn the_gate_script_gives_its_command_the_run_id_and_a_test_rule_is_judged_so() {
        let log = Log::new("");
        let snapshot = Log::new("");
        let given = Log::new("");
        let status = gate_script(&log.0)
            .env("GATE_EVERY", "1")
            .env("GATE_READINGS", "2")
            .env("GATE_LIMIT", "1000")
            .env("SNAPSHOT", &snapshot.0)
            .env("GIVEN", &given.0)
            .args([
                "sh",
                "-c",
                r#"cp "$LOG" "$SNAPSHOT"; printf %s "$GATE_RUN_ID" > "$GIVEN""#,
            ])
            .status()
            .unwrap();
        assert!(status.success());
        let id = std::fs::read_to_string(&given.0).unwrap();
        let header = std::fs::read_to_string(&log.0)
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .to_string();
        assert_eq!(
            rule(&header).and_then(|r| r.3),
            Some(id.as_str()),
            "{header}"
        );
        // While its command ran, the gate was open, under a test rule.
        assert_eq!(reason(Gate::open(&snapshot.0, Some(&id))), "gate_rule");
        assert_eq!(reason(Gate::open(&snapshot.0, Some(ID))), "not_this_run");
        // Afterwards its block has ended.
        assert_eq!(reason(Gate::open(&log.0, Some(&id))), "gate_not_open");
    }
}
