//! THE COUNTER-EXAMPLE. Do not copy this module; it is here to be tested
//! against.
//!
//! In `board.py` a task holds a reference to the task that blocks it. Here
//! that reference became an index into the board's list of tasks, which is
//! the usual advice and is right for a list that only grows. This list
//! shrinks. When a finished task is removed, every task after it moves up
//! one place, and each stored index now names a different task, or none.
//!
//! Nothing reports it. The borrow checker has no opinion about an integer,
//! the index is in bounds often enough not to panic, and the results are
//! simply wrong: see the tests in `lib.rs`.

use crate::{Rng, Snapshot};

struct Task {
    tid: i64,
    work: i64,
    /// Where the blocker was in `Board::tasks` when this task was added.
    blocker: Option<usize>,
}

pub struct Board {
    rng: Rng,
    tasks: Vec<Task>,
    done: Vec<(i64, i64)>,
    next_tid: i64,
    clock: i64,
}

impl Board {
    pub fn new(seed: u64) -> Board {
        Board {
            rng: Rng::new(seed),
            tasks: Vec::new(),
            done: Vec::new(),
            next_tid: 1,
            clock: 0,
        }
    }

    fn add(&mut self) {
        let mut blocker = None;
        if !self.tasks.is_empty() && self.rng.below(3) != 0 {
            blocker = Some(self.rng.below(self.tasks.len() as u64) as usize);
        }
        let work = 1 + self.rng.below(4) as i64;
        self.tasks.push(Task {
            tid: self.next_tid,
            work,
            blocker,
        });
        self.next_tid += 1;
    }

    pub fn tick(&mut self) {
        self.clock += 1;
        for _ in 0..self.rng.below(3) {
            self.add();
        }
        for i in 0..self.tasks.len() {
            // "Is my blocker still on the board?" asked of an index: all it
            // can say is whether the list is still that long.
            if self.tasks[i].blocker.is_some_and(|b| b < self.tasks.len()) {
                continue;
            }
            self.tasks[i].work -= 1;
        }
        let clock = self.clock;
        for task in self.tasks.iter().filter(|t| t.work <= 0) {
            self.done.push((task.tid, clock));
        }
        // Everything after a removed task moves up; no stored index follows.
        self.tasks.retain(|t| t.work > 0);
    }

    pub fn snapshot(&self) -> Snapshot {
        let blocker_tid = |t: &Task| t.blocker.and_then(|b| self.tasks.get(b)).map(|b| b.tid);
        Snapshot {
            done: self.done.clone(),
            open: self
                .tasks
                .iter()
                .map(|t| (t.tid, t.work, blocker_tid(t)))
                .collect(),
        }
    }
}

pub fn run(seed: u64, ticks: usize) -> Snapshot {
    let mut board = Board::new(seed);
    for _ in 0..ticks {
        board.tick();
    }
    board.snapshot()
}
