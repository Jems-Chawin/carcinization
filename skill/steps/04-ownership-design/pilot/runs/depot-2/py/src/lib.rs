//! The port under measurement as a Python module, so that `carcinify diff`
//! can set it beside `depot.py`. Written against the interface that
//! `task/tests/callers.rs` fixes, and copied beside each run unchanged.
//! `scenario` is the function of that file, word for word.

use depot::{Courier, Depot, Parcel, Zone};
use pyo3::prelude::*;
use std::sync::mpsc;

fn scenario(zones: usize, couriers: usize, seed: u64, steps: usize) -> String {
    let mut depot = Depot::new(zones, couriers, seed);
    let mut log: Vec<String> = Vec::new();
    let (notes, noted) = mpsc::channel::<String>();
    let (keep, kept) = mpsc::channel::<Parcel>();

    depot.on_delivered(move |parcel: &Parcel, courier: &Courier| {
        let line = format!(
            "d {} {} {} {}",
            parcel.id(),
            courier.id(),
            parcel.age(),
            courier.load()
        );
        notes.send(line).unwrap();
        if parcel.id().is_multiple_of(5) {
            courier.rest(2);
        }
        if parcel.id().is_multiple_of(4) {
            keep.send(parcel.clone()).unwrap();
        }
    });
    let watched: Courier = depot.courier(0).unwrap();
    let hub: Zone = depot.zone(0);
    let mut mine: Vec<Parcel> = Vec::new();
    for t in 0..steps {
        if t.is_multiple_of(7) {
            mine.push(depot.submit(t % zones, (t + 2) % zones, 2));
        }
        if t == steps / 2 {
            watched.set_capacity(3);
        }
        depot.step();
        log.extend(noted.try_iter());
        log.push(format!(
            "w {} {} {} z {} {}",
            watched.load(),
            watched.capacity(),
            u8::from(watched.retired()),
            hub.waiting(),
            hub.congestion()
        ));
    }
    for parcel in &mine {
        let carrier = parcel.carrier().map_or(-1, |c| c.id() as i64);
        log.push(format!(
            "m {} {} {} {}",
            parcel.id(),
            parcel.status(),
            parcel.age(),
            carrier
        ));
    }
    for parcel in kept.try_iter() {
        log.push(format!(
            "k {} {} {}",
            parcel.id(),
            parcel.status(),
            parcel.age()
        ));
    }
    log.push(format!("g {}", u8::from(depot.courier(0).is_none())));
    log.join("\n") + "\n" + &depot.report()
}

#[pymodule]
mod depot_rs {
    use super::*;

    /// `run(zones, couriers, seed, steps)` of `depot.py`.
    #[pyfunction]
    fn run(zones: usize, couriers: usize, seed: u64, steps: usize) -> String {
        depot::run(zones, couriers, seed, steps)
    }

    /// `scenario(zones, couriers, seed, steps)` of `callers.py`.
    #[pyfunction]
    fn scenario(zones: usize, couriers: usize, seed: u64, steps: usize) -> String {
        super::scenario(zones, couriers, seed, steps)
    }
}
