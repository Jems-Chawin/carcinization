//! Two ports of `python/board.py`, which adds tasks, removes finished ones,
//! and lets a task hold a reference to the task that blocks it.
//!
//! - [`plain_index`] is the counter-example, kept on purpose: the reference
//!   became an index into the task list. It compiles, it never panics, and
//!   it is wrong as soon as a task is removed.
//! - [`checked`] replaces the reference with a handle that knows when the
//!   thing it named has gone. It gives the results of the original.
//!
//! The tests below show both facts.

pub mod checked;
pub mod plain_index;

/// What a run returns: (tid, tick finished) for every finished task, and
/// (tid, work left, blocker's tid) for every task still on the board.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub done: Vec<(i64, i64)>,
    pub open: Vec<(i64, i64, Option<i64>)>,
}

/// The generator of `board.py`, so both languages draw the same numbers.
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Rng {
        Rng(seed)
    }

    pub(crate) fn below(&mut self, n: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) % n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_checked_port_gives_the_values_of_the_python_original() {
        // board.run(7, 200) and board.run(1, 40), from python/board.py.
        let s = checked::run(7, 200);
        assert_eq!(s.done.len(), 220);
        assert_eq!(&s.done[..3], &[(2, 2), (1, 4), (3, 5)]);
        assert_eq!(
            &s.open[..3],
            &[
                (221, 2, Some(217)),
                (222, 4, Some(220)),
                (223, 3, Some(218))
            ]
        );
        let s = checked::run(1, 40);
        assert_eq!(s.done.len(), 42);
        assert_eq!(
            &s.open[..3],
            &[(43, 1, Some(42)), (44, 2, Some(40)), (45, 2, Some(41))]
        );
    }

    #[test]
    fn the_plain_index_goes_stale_without_any_error() {
        // The two ports agree for as long as nothing has been removed...
        let seed = 7;
        let first_removal = (1..)
            .find(|t| !checked::run(seed, *t).done.is_empty())
            .unwrap();
        for ticks in 0..first_removal {
            assert_eq!(
                plain_index::run(seed, ticks),
                checked::run(seed, ticks),
                "tick {ticks}"
            );
        }
        // ...and part company soon after, with no panic and no error: the
        // index still points into the list, at some other task.
        let apart =
            (first_removal..500).find(|t| plain_index::run(seed, *t) != checked::run(seed, *t));
        let apart = apart.expect("the plain index should have gone wrong within 500 ticks");
        println!(
            "seed {seed}: first removal at tick {first_removal}, results differ from tick {apart}"
        );

        // The same happens for other seeds; it is not one unlucky case.
        let wrong = (0..20u64)
            .filter(|s| plain_index::run(*s, 300) != checked::run(*s, 300))
            .count();
        println!("of 20 seeds, the plain index gives wrong results after 300 ticks for {wrong}");
        assert!(wrong >= 15, "only {wrong} of 20 seeds went wrong");
    }

    #[test]
    fn a_handle_to_a_removed_task_is_never_taken_for_a_new_one() {
        // Slots are reused, so a slot number alone would point at whatever
        // moved in. The generation tells the two apart.
        let mut board = checked::Board::new(3);
        let mut reused = false;
        for _ in 0..400 {
            board.tick();
            reused |= board.slots_reused() > 0;
            board.assert_no_blocker_resolves_to_another_task();
        }
        assert!(reused, "the test never exercised a reused slot");
    }
}
