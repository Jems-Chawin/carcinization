//! The Rust port of `depot.py`. Everything is in `depot.rs`, in the order
//! of the Python file, so that the two can be read side by side.

mod depot;

pub use depot::{
    Courier, Depot, EXPIRY, FATIGUE_LIMIT, PATIENCE, Parcel, QUOTA, REST, Rng, Status, Zone, run,
};
