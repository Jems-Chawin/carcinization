//! THE COUNTER-EXAMPLE. Do not copy this module; it is here to be measured.
//!
//! This is `fleet.py` carried over class for class. In Python a unit holds
//! its fleet and reaches everything through it. Written the same way in
//! Rust, `unit.step(&mut fleet)` cannot compile, because the unit is inside
//! the fleet it would borrow. Each error that followed was answered with the
//! smallest edit that made it go away, and each of those edits is marked
//! `APPEASEMENT` below with the error it silenced.
//!
//! Every one of them is needed for this structure to compile, which is why
//! no lint objects to any of them. The results are identical to the
//! original. The cost is in `tests/allocations.rs`.

use crate::{Rng, Snapshot, WIDTH};

#[derive(Clone)]
struct Unit {
    uid: usize,
    name: String,
    x: i64,
    y: i64,
    fuel: i64,
}

pub struct Fleet {
    units: Vec<Unit>,
    heat: Vec<i64>,
    transfers: i64,
    energy: f64,
}

impl Unit {
    /// The closest other unit that still has fuel; the lowest uid wins a tie.
    fn nearest(&self, others: &[Unit]) -> Option<(Unit, i64)> {
        let mut found: Option<(Unit, i64)> = None;
        for other in others {
            if other.uid == self.uid || other.fuel <= 0 {
                continue;
            }
            let distance = (other.x - self.x).abs() + (other.y - self.y).abs();
            if found.as_ref().is_none_or(|(_, best)| distance < *best) {
                // APPEASEMENT: returning `&Unit` tied the result to `others`
                // ("borrowed value does not live long enough" at the call),
                // so the unit is cloned out, name and all.
                found = Some((other.clone(), distance));
            }
        }
        found
    }

    /// One step. Returns the uid of a unit to credit with one fuel, since
    /// this unit cannot reach the other one to do it itself.
    fn step(
        &mut self,
        others: &[Unit],
        heat: &mut [i64],
        transfers: &mut i64,
        energy: &mut f64,
    ) -> Option<usize> {
        if self.fuel <= 0 {
            return None;
        }
        let (other, distance) = self.nearest(others)?;
        let mut credit = None;
        if distance <= 1 {
            if self.fuel > other.fuel {
                self.fuel -= 1;
                credit = Some(other.uid);
                *transfers += 1;
            }
        } else {
            if other.x != self.x {
                self.x += if other.x > self.x { 1 } else { -1 };
            } else {
                self.y += if other.y > self.y { 1 } else { -1 };
            }
            self.fuel -= 1;
        }
        heat[(self.y * WIDTH + self.x) as usize] += 1;
        *energy += 0.1 * self.fuel as f64;
        credit
    }
}

impl Fleet {
    pub fn new(count: usize, seed: u64) -> Fleet {
        let mut rng = Rng::new(seed);
        let mut units = Vec::new();
        for uid in 0..count {
            let x = rng.below(WIDTH as u64) as i64;
            let y = rng.below(WIDTH as u64) as i64;
            let fuel = 20 + rng.below(60) as i64;
            units.push(Unit {
                uid,
                name: format!("tanker-{uid:03}"),
                x,
                y,
                fuel,
            });
        }
        Fleet {
            units,
            heat: vec![0; (WIDTH * WIDTH) as usize],
            transfers: 0,
            energy: 0.0,
        }
    }

    pub fn tick(&mut self) {
        // APPEASEMENT: `for unit in &mut self.units` held `self` borrowed for
        // the whole loop, so the ids are collected first.
        let uids: Vec<usize> = self.units.iter().map(|u| u.uid).collect();
        for uid in uids {
            // APPEASEMENT: "cannot borrow `self.units` as immutable because it
            // is also borrowed as mutable". The unit is cloned out, stepped,
            // and written back.
            let mut unit = self.units[uid].clone();
            // APPEASEMENT: the same error for the neighbours the step reads,
            // so it gets a copy of every unit, on every step of every unit.
            let others = self.units.clone();
            // APPEASEMENT: "cannot borrow `self.heat` as mutable": the grid is
            // taken out of the fleet and put back afterwards.
            let mut heat = std::mem::take(&mut self.heat);
            let credit = unit.step(&others, &mut heat, &mut self.transfers, &mut self.energy);
            self.heat = heat;
            self.units[uid] = unit;
            if let Some(other) = credit {
                self.units[other].fuel += 1;
            }
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            units: self
                .units
                .iter()
                .map(|u| (u.name.clone(), u.x, u.y, u.fuel))
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
