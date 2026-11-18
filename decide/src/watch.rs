//! Watches the machine while a measured process runs.
//!
//! The measuring script times a loop of pure computation before and after
//! each run. Load that begins and ends inside a run falls between those
//! two readings. So for as long as the measured process runs, a second
//! Python process does nothing but compute, and at the end says how much
//! CPU time it got in how much wall time. It never waits, so what it did
//! not get was taken by other programs, and the workload beside it was
//! treated the same way.
//!
//! It has to compute without pause. A first version worked in short pieces
//! with pauses between, in a thread of this program, and read the same with
//! three busy programs for every CPU as with none: a thread that sleeps
//! most of the time is put ahead of the others when it wakes.
//!
//! The cost: one CPU is kept busy for the length of every measured run.
//! With fewer than four CPUs that would take too much from the workload
//! itself, and no watch is kept.

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};

const LOOP: &str = "\
import select, sys, time
wall, cpu, x = time.perf_counter(), time.process_time(), 0
while not select.select([sys.stdin], [], [], 0)[0]:
    for i in range(200000):
        x += i
print(time.process_time() - cpu, time.perf_counter() - wall)
";

/// With fewer CPUs than this the watching process would itself take a
/// large part of the machine from the workload.
pub const MIN_CPUS_TO_WATCH: usize = 4;

/// The CPUs this process may use: fewer than the machine has under a CPU
/// set or a quota, as in a container. 0 when not known.
pub fn cpus() -> usize {
    std::thread::available_parallelism().map_or(0, |n| n.get())
}

pub struct Watch(Option<Child>);

pub struct Reading {
    pub seconds: f64,
    /// The fraction of its time the watching process was not on the CPU.
    pub kept_off_the_cpu: f64,
}

impl Watch {
    pub fn start(python: &Path) -> Watch {
        if cpus() < MIN_CPUS_TO_WATCH || cfg!(windows) {
            return Watch(None);
        }
        Watch(
            Command::new(python)
                .args(["-I", "-c", LOOP])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .ok(),
        )
    }

    /// Stops the watching process and reads what it saw. `None` when no
    /// watch was kept or it could not be read.
    pub fn finish(self) -> Option<Reading> {
        let mut child = self.0?;
        // A line on its standard input is the signal to stop.
        let told = child
            .stdin
            .take()
            .is_some_and(|mut stdin| stdin.write_all(b"\n").is_ok());
        if !told {
            let _ = child.kill();
        }
        let mut text = String::new();
        if let Some(mut out) = child.stdout.take() {
            let _ = out.read_to_string(&mut text);
        }
        let _ = child.wait();
        let mut numbers = text.split_whitespace().map(str::parse::<f64>);
        let (cpu, wall) = (numbers.next()?.ok()?, numbers.next()?.ok()?);
        (wall > 0.05).then(|| Reading {
            seconds: wall,
            kept_off_the_cpu: (1.0 - cpu / wall).clamp(0.0, 1.0),
        })
    }
}
