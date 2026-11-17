//! The port that follows `DESIGN.md`.
//!
//! The decisions, in short (the note has the reasons and what was rejected):
//!
//! - The fleet owns everything. A unit holds no reference to the fleet;
//!   "unit `i`" is an index.
//! - A step is a function of the fleet, not a method of a unit, because it
//!   reads all units, writes two of them, and writes three fleet fields.
//! - What the hot loop reads for every pair (position, fuel) is kept apart
//!   from what it never reads (names), so nothing cold is touched or copied.
//! - Reading and writing are separated in time, not by copying: first find
//!   the neighbour with shared access and keep only its index and distance,
//!   then write through indices.

use crate::{Rng, Snapshot, WIDTH};

/// What the hot loop reads and writes. Three integers: copying one is free.
#[derive(Clone, Copy)]
struct Body {
    x: i64,
    y: i64,
    fuel: i64,
}

pub struct Fleet {
    bodies: Vec<Body>,
    /// Same order as `bodies`. Only `snapshot` reads them.
    names: Vec<String>,
    heat: Vec<i64>,
    transfers: i64,
    energy: f64,
}

/// The closest other unit that still has fuel, as (index, distance); the
/// lowest index wins a tie. Shared access to the bodies, nothing kept from it
/// but two integers.
fn nearest(bodies: &[Body], me: usize) -> Option<(usize, i64)> {
    let here = bodies[me];
    let mut found: Option<(usize, i64)> = None;
    for (index, other) in bodies.iter().enumerate() {
        if index == me || other.fuel <= 0 {
            continue;
        }
        let distance = (other.x - here.x).abs() + (other.y - here.y).abs();
        if found.is_none_or(|(_, best)| distance < best) {
            found = Some((index, distance));
        }
    }
    found
}

impl Fleet {
    pub fn new(count: usize, seed: u64) -> Fleet {
        let mut rng = Rng::new(seed);
        let mut bodies = Vec::with_capacity(count);
        let mut names = Vec::with_capacity(count);
        for uid in 0..count {
            let x = rng.below(WIDTH as u64) as i64;
            let y = rng.below(WIDTH as u64) as i64;
            let fuel = 20 + rng.below(60) as i64;
            bodies.push(Body { x, y, fuel });
            names.push(format!("tanker-{uid:03}"));
        }
        Fleet {
            bodies,
            names,
            heat: vec![0; (WIDTH * WIDTH) as usize],
            transfers: 0,
            energy: 0.0,
        }
    }

    /// The hot path. Allocates nothing: see `tests/allocations.rs`.
    pub fn tick(&mut self) {
        for me in 0..self.bodies.len() {
            self.step(me);
        }
    }

    fn step(&mut self, me: usize) {
        if self.bodies[me].fuel <= 0 {
            return;
        }
        // Read phase: the shared borrow of `bodies` ends with this statement.
        let Some((other, distance)) = nearest(&self.bodies, me) else {
            return;
        };
        // Write phase: two bodies by index, then three other fields. Each is
        // a separate borrow of a separate field, one after the other.
        let there = self.bodies[other];
        if distance <= 1 {
            if self.bodies[me].fuel > there.fuel {
                self.bodies[me].fuel -= 1;
                self.bodies[other].fuel += 1;
                self.transfers += 1;
            }
        } else {
            let body = &mut self.bodies[me];
            if there.x != body.x {
                body.x += if there.x > body.x { 1 } else { -1 };
            } else {
                body.y += if there.y > body.y { 1 } else { -1 };
            }
            body.fuel -= 1;
        }
        let body = self.bodies[me];
        self.heat[(body.y * WIDTH + body.x) as usize] += 1;
        self.energy += 0.1 * body.fuel as f64;
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            units: self
                .names
                .iter()
                .zip(&self.bodies)
                .map(|(n, b)| (n.clone(), b.x, b.y, b.fuel))
                .collect(),
            heat: self.heat.clone(),
            transfers: self.transfers,
            energy: self.energy,
        }
    }
}

pub fn simulate(count: usize, seed: u64, ticks: usize) -> Snapshot {
    let mut fleet = Fleet::new(count, seed);
    for _ in 0..ticks {
        fleet.tick();
    }
    fleet.snapshot()
}
