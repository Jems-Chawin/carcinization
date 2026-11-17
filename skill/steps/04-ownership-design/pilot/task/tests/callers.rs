//! The callers of the module, written against the Rust port. This is
//! `callers.py` and must not be changed: it is the interface the port has
//! to offer. The expected texts in `golden/` were printed by the Python.

use depot::{Courier, Depot, Parcel, Zone};
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

#[test]
fn a_run_with_nobody_watching() {
    assert_eq!(
        depot::run(8, 16, 7, 300),
        include_str!("golden/run-8-16-7-300.txt")
    );
    assert_eq!(
        depot::run(8, 3, 5, 500),
        include_str!("golden/run-8-3-5-500.txt")
    );
    assert_eq!(
        depot::run(3, 1, 1, 50),
        include_str!("golden/run-3-1-1-50.txt")
    );
}

#[test]
fn callers_keep_objects_and_change_a_courier() {
    assert_eq!(
        scenario(8, 16, 7, 300),
        include_str!("golden/scenario-8-16-7-300.txt")
    );
    assert_eq!(
        scenario(5, 2, 3, 400),
        include_str!("golden/scenario-5-2-3-400.txt")
    );
}
