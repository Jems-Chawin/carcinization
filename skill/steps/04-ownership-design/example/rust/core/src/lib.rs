//! Two ports of `python/fleet.py`. Both give the same results as the
//! original, bit for bit. They differ in who owns what, and so in what the
//! hot path costs.
//!
//! - [`appeasement`] is the counter-example, kept on purpose: the Python
//!   structure carried over as it was, with each borrow-checker error
//!   answered by the smallest edit that made it go away.
//! - [`designed`] follows the design note in `DESIGN.md`.
//!
//! `tests/allocations.rs` puts a number on the difference.

pub mod appeasement;
pub mod designed;

/// Side of the square grid.
pub const WIDTH: i64 = 16;

/// What a simulation returns: per unit (name, x, y, fuel), the heat grid,
/// the number of transfers and the energy total.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub units: Vec<(String, i64, i64, i64)>,
    pub heat: Vec<i64>,
    pub transfers: i64,
    pub energy: f64,
}

/// The generator of `fleet.py`, so both languages draw the same numbers.
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
    fn the_two_ports_agree_at_every_tick() {
        for (count, seed) in [(0, 1), (1, 2), (2, 3), (24, 7), (60, 99)] {
            let mut a = appeasement::Fleet::new(count, seed);
            let mut b = designed::Fleet::new(count, seed);
            for tick in 0..150 {
                assert_eq!(
                    a.snapshot(),
                    b.snapshot(),
                    "count {count} seed {seed} tick {tick}"
                );
                a.tick();
                b.tick();
            }
        }
    }

    #[test]
    fn a_value_computed_by_the_python_original() {
        // fleet.simulate(24, 7, 200), from python/fleet.py.
        let s = designed::simulate(24, 7, 200);
        assert_eq!(s.transfers, 1985);
        assert_eq!(s.energy.to_bits(), 24083.400000000263f64.to_bits());
        assert_eq!(s.units[1], ("tanker-001".to_string(), 1, 9, 58));
        assert_eq!(s.heat.iter().sum::<i64>(), 4800);
    }
}
