//! The port that follows `DESIGN.md`: a reference to something that can be
//! removed becomes a handle that is checked, not a bare index.
//!
//! - Tasks live in slots. A slot is reused after its task is removed, so
//!   the board does not grow with every task there has ever been.
//! - A [`Handle`] is a slot number and the slot's generation at the time.
//!   Removing a task raises the slot's generation, so every handle to the
//!   old task stops matching, for good. "Is my blocker still on the board?"
//!   is one comparison, and it cannot be answered wrongly by whatever task
//!   moved into the slot later.
//! - The order of the Python list (oldest first, survivors keep their
//!   order) is kept in `order`, apart from where the tasks are stored.
//! - Python can still read `blocker.tid` after the blocker has left the
//!   board, because the reference keeps the object alive. A handle keeps
//!   nothing alive, so the one thing that is read later, the tid, is copied
//!   into the task when the reference is made.

use crate::{Rng, Snapshot};

/// Names one task for as long as it is on the board, and nothing afterwards.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Handle {
    slot: u32,
    generation: u32,
}

struct Task {
    tid: i64,
    work: i64,
    /// The blocking task, and its tid (see the module comment).
    blocker: Option<(Handle, i64)>,
}

struct Slot {
    generation: u32,
    task: Option<Task>,
}

pub struct Board {
    rng: Rng,
    slots: Vec<Slot>,
    free: Vec<u32>,
    /// Handles of the tasks on the board, in the order of the Python list.
    order: Vec<Handle>,
    done: Vec<(i64, i64)>,
    next_tid: i64,
    clock: i64,
    reused: u64,
}

impl Board {
    pub fn new(seed: u64) -> Board {
        Board {
            rng: Rng::new(seed),
            slots: Vec::new(),
            free: Vec::new(),
            order: Vec::new(),
            done: Vec::new(),
            next_tid: 1,
            clock: 0,
            reused: 0,
        }
    }

    /// The task a handle names, if it is still on the board.
    fn get(&self, handle: Handle) -> Option<&Task> {
        let slot = &self.slots[handle.slot as usize];
        if slot.generation == handle.generation {
            slot.task.as_ref()
        } else {
            None
        }
    }

    fn insert(&mut self, task: Task) -> Handle {
        let slot = match self.free.pop() {
            Some(slot) => {
                self.reused += 1;
                slot
            }
            None => {
                self.slots.push(Slot {
                    generation: 0,
                    task: None,
                });
                (self.slots.len() - 1) as u32
            }
        };
        self.slots[slot as usize].task = Some(task);
        Handle {
            slot,
            generation: self.slots[slot as usize].generation,
        }
    }

    fn remove(&mut self, handle: Handle) -> Option<Task> {
        let slot = &mut self.slots[handle.slot as usize];
        if slot.generation != handle.generation {
            return None;
        }
        let task = slot.task.take()?;
        // From here on no handle made for this task matches the slot.
        slot.generation += 1;
        self.free.push(handle.slot);
        Some(task)
    }

    fn add(&mut self) {
        let mut blocker = None;
        if !self.order.is_empty() && self.rng.below(3) != 0 {
            let handle = self.order[self.rng.below(self.order.len() as u64) as usize];
            blocker = self.get(handle).map(|task| (handle, task.tid));
        }
        let work = 1 + self.rng.below(4) as i64;
        let handle = self.insert(Task {
            tid: self.next_tid,
            work,
            blocker,
        });
        self.order.push(handle);
        self.next_tid += 1;
    }

    pub fn tick(&mut self) {
        self.clock += 1;
        for _ in 0..self.rng.below(3) {
            self.add();
        }
        for i in 0..self.order.len() {
            let handle = self.order[i];
            let blocked = self
                .get(handle)
                .and_then(|t| t.blocker)
                .is_some_and(|(b, _)| self.get(b).is_some());
            if blocked {
                continue;
            }
            if let Some(task) = self.slots[handle.slot as usize].task.as_mut() {
                task.work -= 1;
            }
        }
        for i in 0..self.order.len() {
            let handle = self.order[i];
            if self.get(handle).is_some_and(|t| t.work <= 0)
                && let Some(task) = self.remove(handle)
            {
                self.done.push((task.tid, self.clock));
            }
        }
        // Survivors keep their order, as in the Python list.
        let slots = &self.slots;
        self.order
            .retain(|h| slots[h.slot as usize].generation == h.generation);
    }

    pub fn snapshot(&self) -> Snapshot {
        let open = self.order.iter().filter_map(|h| self.get(*h));
        Snapshot {
            done: self.done.clone(),
            open: open
                .map(|t| (t.tid, t.work, t.blocker.map(|(_, tid)| tid)))
                .collect(),
        }
    }

    /// How many times a slot has been given to a new task. For the tests.
    pub fn slots_reused(&self) -> u64 {
        self.reused
    }

    /// For the tests: wherever a blocker handle still resolves, it resolves
    /// to the task it was made for.
    pub fn assert_no_blocker_resolves_to_another_task(&self) {
        for task in self.order.iter().filter_map(|h| self.get(*h)) {
            if let Some((handle, tid)) = task.blocker
                && let Some(found) = self.get(handle)
            {
                assert_eq!(
                    found.tid, tid,
                    "task {} found a stranger in its blocker's slot",
                    task.tid
                );
            }
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
