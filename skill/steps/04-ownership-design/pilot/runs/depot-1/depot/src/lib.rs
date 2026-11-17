//! A courier depot, simulated one step at a time.
//!
//! Zones are joined by roads. Parcels appear in a zone and have to reach
//! another. Couriers carry them: each step a courier delivers what has arrived,
//! passes parcels to a colleague who is going their way, picks up what fits,
//! and moves one zone. Couriers tire and rest, and retire after enough
//! deliveries; the depot hires a replacement at the next step. Parcels that
//! wait too long are given up.
//!
//! Everything is whole numbers, and the same arguments always give the same
//! run. Other code holds on to the objects made here (see `callers.py` and
//! `tests/callers.rs`): a `Courier`, a `Zone` or a `Parcel` that a caller
//! keeps goes on showing the state of that courier, zone or parcel, and a
//! caller may change a courier.
//!
//! # How the Python objects are held in Rust
//!
//! This is a port of `depot.py`; the classes and methods have the same names
//! and come in the same order. In Python every object is shared and can be
//! changed by anyone who holds it. Here each class is two structs:
//!
//! * `ParcelState`, `ZoneState`, `CourierState`, `DepotState` hold the
//!   attributes of the Python object. They are shared (`Rc`) and their
//!   changing attributes are in `Cell`/`RefCell`, so that every holder sees
//!   the same, current values.
//! * `Parcel`, `Zone`, `Courier`, `Depot` are what the Python code calls the
//!   object: the shared state together with `depot`, the depot it belongs to.
//!   The methods are on these. Cloning one gives another name for the same
//!   object, not a copy.
//!
//! So that the memory is given back when the depot and all kept objects are
//! gone, the states do not point at each other in a circle: a state names a
//! zone by its number (`origin`, `dest`, `zone`, `neighbours`), a parcel
//! knows its carrier weakly, and only the handles point back at the depot.
//! `parcel.dest()`, `courier.zone()` and so on turn these back into objects,
//! and `a.is(&b)` is Python's `a is b`.
//!
//! Where the Python loops over `list(xs)` or over a list it does not change
//! in the loop, the Rust loops over a snapshot (`bag()`, `waiting_list()`,
//! ...), which is the same thing.
//!
//! Weights, capacities and resting times are `i64`, as a caller may pass any
//! of them. Figures that are sums of weights (`load`, `congestion`,
//! `fatigue`) are `i128`, so they cannot overflow and agree with Python's
//! unbounded integers for every `i64` weight. `//` on them is `div_euclid`,
//! which like Python rounds down.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

/// A parcel that has waited longer than this is urgent.
const PATIENCE: u64 = 12;
/// A parcel that has waited longer than this is given up.
const EXPIRY: u64 = 40;
/// A courier this tired rests,
const FATIGUE_LIMIT: i128 = 25;
/// for this many steps.
const REST: i64 = 3;
/// Deliveries after which a courier retires, give or take.
const QUOTA: u64 = 30;
// `MASK` of the Python is not needed: a `u64` is 64 bits already, and the
// wrapping operations below are the `& MASK`.

/// `xs.remove(x)` of a Python list: takes out the first entry that is the
/// very same object, and fails if there is none.
fn remove_first<T>(list: &RefCell<Vec<Rc<T>>>, item: &Rc<T>) {
    let mut list = list.borrow_mut();
    let at = list
        .iter()
        .position(|entry| Rc::ptr_eq(entry, item))
        .expect("list.remove(x): x not in list");
    list.remove(at);
}

/// A linear congruential generator, so that a run can be repeated.
struct Rng {
    state: u64,
}

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng { state: seed }
    }

    fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state >> 33
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// The attributes of a [`Parcel`].
struct ParcelState {
    id: u64,
    origin: usize,
    dest: usize,
    weight: i64,
    born: u64,
    carrier: RefCell<Option<Weak<CourierState>>>,
    status: Cell<&'static str>,
    hops: Cell<u64>,
    closed_at: Cell<Option<u64>>,
}

/// Something to be carried from one zone to another.
pub struct Parcel {
    depot: Depot,
    state: Rc<ParcelState>,
}

impl Clone for Parcel {
    /// Another name for the same parcel.
    fn clone(&self) -> Parcel {
        Parcel {
            depot: self.depot.alias(),
            state: Rc::clone(&self.state),
        }
    }
}

impl Parcel {
    fn new(depot: &Depot, pid: u64, origin: &Zone, dest: &Zone, weight: i64) -> Parcel {
        Parcel {
            depot: depot.alias(),
            state: Rc::new(ParcelState {
                id: pid,
                origin: origin.id(),
                dest: dest.id(),
                weight,
                born: depot.state.clock.get(),
                carrier: RefCell::new(None),
                status: Cell::new("waiting"),
                hops: Cell::new(0),
                closed_at: Cell::new(None),
            }),
        }
    }

    pub fn id(&self) -> u64 {
        self.state.id
    }

    fn origin(&self) -> Zone {
        self.depot.zone(self.state.origin)
    }

    fn dest(&self) -> Zone {
        self.depot.zone(self.state.dest)
    }

    fn weight(&self) -> i64 {
        self.state.weight
    }

    /// The courier who has it in the bag, if any.
    pub fn carrier(&self) -> Option<Courier> {
        let carrier = self.state.carrier.borrow();
        let state = carrier.as_ref()?.upgrade()?;
        Some(Courier {
            depot: self.depot.alias(),
            state,
        })
    }

    fn set_carrier(&self, carrier: Option<&Courier>) {
        *self.state.carrier.borrow_mut() = carrier.map(|courier| Rc::downgrade(&courier.state));
    }

    /// `"waiting"`, `"carried"`, `"delivered"` or `"expired"`.
    pub fn status(&self) -> &'static str {
        self.state.status.get()
    }

    fn hops(&self) -> u64 {
        self.state.hops.get()
    }

    /// `self.hops += 1`
    fn add_hop(&self) {
        self.state.hops.set(self.state.hops.get() + 1);
    }

    /// Steps since it appeared, up to the step it was closed.
    pub fn age(&self) -> u64 {
        let end = match self.state.closed_at.get() {
            None => self.depot.state.clock.get(),
            Some(closed_at) => closed_at,
        };
        end - self.state.born
    }

    fn urgent(&self) -> bool {
        self.state.closed_at.get().is_none() && self.age() > PATIENCE
    }

    fn close(&self, status: &'static str) {
        self.state.status.set(status);
        self.state.closed_at.set(Some(self.depot.state.clock.get()));
        self.set_carrier(None);
    }
}

/// The attributes of a [`Zone`].
struct ZoneState {
    id: usize,
    neighbours: RefCell<Vec<usize>>,
    waiting: RefCell<Vec<Rc<ParcelState>>>,
    couriers: RefCell<Vec<Rc<CourierState>>>,
    delivered: Cell<u64>,
    congestion: Cell<i128>,
}

/// A place where parcels wait and couriers pass through.
pub struct Zone {
    depot: Depot,
    state: Rc<ZoneState>,
}

impl Clone for Zone {
    /// Another name for the same zone.
    fn clone(&self) -> Zone {
        Zone {
            depot: self.depot.alias(),
            state: Rc::clone(&self.state),
        }
    }
}

impl Zone {
    fn new(depot: &Depot, zid: usize) -> Zone {
        Zone {
            depot: depot.alias(),
            state: Rc::new(ZoneState {
                id: zid,
                neighbours: RefCell::new(Vec::new()),
                waiting: RefCell::new(Vec::new()),
                couriers: RefCell::new(Vec::new()),
                delivered: Cell::new(0),
                congestion: Cell::new(0),
            }),
        }
    }

    /// `self is other`.
    fn is(&self, other: &Zone) -> bool {
        Rc::ptr_eq(&self.state, &other.state)
    }

    pub fn id(&self) -> usize {
        self.state.id
    }

    /// `self.neighbours`, as it is now.
    fn neighbours(&self) -> Vec<Zone> {
        let ids = self.state.neighbours.borrow().clone();
        ids.into_iter().map(|zid| self.depot.zone(zid)).collect()
    }

    /// `self.waiting`, as it is now.
    fn waiting_list(&self) -> Vec<Parcel> {
        let waiting = self.state.waiting.borrow();
        waiting.iter().map(|p| self.depot.parcel_of(p)).collect()
    }

    /// `len(self.waiting)`: how many parcels wait here.
    pub fn waiting(&self) -> usize {
        self.state.waiting.borrow().len()
    }

    /// `self.couriers`, as it is now.
    fn couriers(&self) -> Vec<Courier> {
        let couriers = self.state.couriers.borrow();
        couriers.iter().map(|c| self.depot.courier_of(c)).collect()
    }

    pub fn delivered(&self) -> u64 {
        self.state.delivered.get()
    }

    pub fn congestion(&self) -> i128 {
        self.state.congestion.get()
    }

    /// Puts a parcel at the end of the queue here.
    fn post(&self, parcel: &Parcel) {
        parcel.state.status.set("waiting");
        parcel.set_carrier(None);
        self.state
            .waiting
            .borrow_mut()
            .push(Rc::clone(&parcel.state));
    }

    /// Takes out of the queue the parcels the courier will carry: urgent
    /// ones first, then the others, each in order of arrival, as long as
    /// they fit.
    fn offer(&self, courier: &Courier) -> Vec<Parcel> {
        let mut room = i128::from(courier.capacity()) - courier.load();
        let mut taken = Vec::new();
        for urgent in [true, false] {
            for parcel in self.waiting_list() {
                if parcel.urgent() == urgent && i128::from(parcel.weight()) <= room {
                    room -= i128::from(parcel.weight());
                    taken.push(parcel);
                }
            }
        }
        for parcel in &taken {
            remove_first(&self.state.waiting, &parcel.state);
        }
        taken
    }

    /// The neighbour with the most congestion; the first of them if
    /// several have the same.
    fn busiest_neighbour(&self) -> Zone {
        let neighbours = self.neighbours();
        let mut best = neighbours[0].clone();
        for zone in &neighbours[1..] {
            if zone.congestion() > best.congestion() {
                best = zone.clone();
            }
        }
        best
    }

    /// End of a step: gives up parcels that waited too long and brings
    /// the congestion figure up to date.
    fn settle(&self) {
        for parcel in self.waiting_list() {
            if parcel.age() > EXPIRY {
                remove_first(&self.state.waiting, &parcel.state);
                self.depot.expire(&parcel);
            }
        }
        let weight: i128 = self
            .waiting_list()
            .iter()
            .map(|parcel| i128::from(parcel.weight()))
            .sum();
        self.state
            .congestion
            .set((self.congestion() * 3 + weight * 4).div_euclid(4));
    }
}

/// The attributes of a [`Courier`].
struct CourierState {
    id: u64,
    zone: Cell<usize>,
    capacity: Cell<i64>,
    quota: u64,
    bag: RefCell<Vec<Rc<ParcelState>>>,
    fatigue: Cell<i128>,
    resting: Cell<i64>,
    delivered: Cell<u64>,
    retired: Cell<bool>,
}

/// Carries parcels, one zone per step.
pub struct Courier {
    depot: Depot,
    state: Rc<CourierState>,
}

impl Clone for Courier {
    /// Another name for the same courier.
    fn clone(&self) -> Courier {
        Courier {
            depot: self.depot.alias(),
            state: Rc::clone(&self.state),
        }
    }
}

impl Courier {
    fn new(depot: &Depot, cid: u64, zone: &Zone, capacity: i64) -> Courier {
        Courier {
            depot: depot.alias(),
            state: Rc::new(CourierState {
                id: cid,
                zone: Cell::new(zone.id()),
                capacity: Cell::new(capacity),
                quota: QUOTA + cid % 7,
                bag: RefCell::new(Vec::new()),
                fatigue: Cell::new(0),
                resting: Cell::new(0),
                delivered: Cell::new(0),
                retired: Cell::new(false),
            }),
        }
    }

    /// `self is other`.
    fn is(&self, other: &Courier) -> bool {
        Rc::ptr_eq(&self.state, &other.state)
    }

    pub fn id(&self) -> u64 {
        self.state.id
    }

    /// The zone the courier is in.
    pub fn zone(&self) -> Zone {
        self.depot.zone(self.state.zone.get())
    }

    pub fn capacity(&self) -> i64 {
        self.state.capacity.get()
    }

    /// `courier.capacity = capacity`, which callers do.
    pub fn set_capacity(&self, capacity: i64) {
        self.state.capacity.set(capacity);
    }

    /// `self.bag`, as it is now.
    fn bag(&self) -> Vec<Parcel> {
        let bag = self.state.bag.borrow();
        bag.iter().map(|p| self.depot.parcel_of(p)).collect()
    }

    pub fn fatigue(&self) -> i128 {
        self.state.fatigue.get()
    }

    pub fn resting(&self) -> i64 {
        self.state.resting.get()
    }

    pub fn delivered(&self) -> u64 {
        self.state.delivered.get()
    }

    pub fn retired(&self) -> bool {
        self.state.retired.get()
    }

    pub fn load(&self) -> i128 {
        let bag = self.state.bag.borrow();
        bag.iter().map(|parcel| i128::from(parcel.weight)).sum()
    }

    /// The zone this courier is going to, or None with an empty bag.
    fn heading(&self) -> Option<Zone> {
        let first = self.state.bag.borrow().first().map(|parcel| parcel.dest);
        first.map(|zid| self.depot.zone(zid))
    }

    /// Callers use this to take a courier off the road for a while.
    pub fn rest(&self, steps: i64) {
        self.state.resting.set(self.resting().max(steps));
    }

    /// One step of this courier.
    fn act(&self) {
        if self.resting() > 0 {
            self.state.resting.set(self.resting() - 1);
            return;
        }
        self.drop_off();
        self.hand_over();
        self.pick_up();
        self.r#move();
        self.tire();
    }

    fn drop_off(&self) {
        for parcel in self.bag() {
            if parcel.dest().is(&self.zone()) {
                remove_first(&self.state.bag, &parcel.state);
                let zone = self.zone();
                zone.state.delivered.set(zone.delivered() + 1);
                self.state.delivered.set(self.delivered() + 1);
                self.depot.record_delivery(&parcel, self);
            }
        }
    }

    /// A parcel that a colleague here is heading for, and this courier
    /// is not, goes to the colleague if it fits.
    fn hand_over(&self) {
        let mine = self.heading();
        for peer in self.zone().couriers() {
            if peer.is(self) || peer.resting() > 0 {
                continue;
            }
            let Some(theirs) = peer.heading() else {
                continue;
            };
            if mine.as_ref().is_some_and(|mine| theirs.is(mine)) {
                continue;
            }
            for parcel in self.bag() {
                if parcel.dest().is(&theirs)
                    && peer.load() + i128::from(parcel.weight()) <= i128::from(peer.capacity())
                {
                    remove_first(&self.state.bag, &parcel.state);
                    peer.state.bag.borrow_mut().push(Rc::clone(&parcel.state));
                    parcel.set_carrier(Some(&peer));
                    parcel.add_hop();
                    let depot = &self.depot.state;
                    depot.handovers.set(depot.handovers.get() + 1);
                }
            }
        }
    }

    fn pick_up(&self) {
        for parcel in self.zone().offer(self) {
            self.state.bag.borrow_mut().push(Rc::clone(&parcel.state));
            parcel.set_carrier(Some(self));
            parcel.state.status.set("carried");
        }
    }

    /// `move` in the Python; the word is taken in Rust, hence the `r#`.
    fn r#move(&self) {
        let here = self.zone();
        let there = match self.heading() {
            Some(goal) => self.depot.route(&here, &goal),
            None => {
                let there = here.busiest_neighbour();
                if there.congestion() <= here.congestion() {
                    return;
                }
                there
            }
        };
        remove_first(&here.state.couriers, &self.state);
        there
            .state
            .couriers
            .borrow_mut()
            .push(Rc::clone(&self.state));
        self.state.zone.set(there.id());
        self.state
            .fatigue
            .set(self.fatigue() + 1 + self.load().div_euclid(8));
        for parcel in self.bag() {
            parcel.add_hop();
        }
    }

    fn tire(&self) {
        if self.fatigue() >= FATIGUE_LIMIT {
            self.state.fatigue.set(0);
            self.state.resting.set(REST);
        }
        if self.delivered() >= self.state.quota {
            self.depot.retire(self);
        }
    }
}

/// What `on_delivered` is given: `listener(parcel, courier)`.
type Listener = Box<dyn FnMut(&Parcel, &Courier)>;

/// The attributes of a [`Depot`].
struct DepotState {
    rng: RefCell<Rng>,
    clock: Cell<u64>,
    zones: RefCell<Vec<Rc<ZoneState>>>,
    routes: RefCell<Vec<Vec<Option<usize>>>>,
    couriers: RefCell<Vec<Rc<CourierState>>>,
    parcels: RefCell<HashMap<u64, Rc<ParcelState>>>,
    listeners: RefCell<Vec<Listener>>,
    next_parcel: Cell<u64>,
    next_courier: Cell<u64>,
    vacancies: Cell<u64>,
    arrivals: usize,
    delivered_count: Cell<u64>,
    expired_count: Cell<u64>,
    retired_count: Cell<u64>,
    latency_total: Cell<u64>,
    handovers: Cell<u64>,
}

/// The whole simulation: zones, couriers, the parcels still open.
pub struct Depot {
    state: Rc<DepotState>,
}

impl Depot {
    /// `Depot(zones, couriers, seed)`. Panics with "at least three zones"
    /// where the Python raises `ValueError`.
    pub fn new(zones: usize, couriers: usize, seed: u64) -> Depot {
        if zones < 3 {
            panic!("at least three zones");
        }
        let depot = Depot {
            state: Rc::new(DepotState {
                rng: RefCell::new(Rng::new(seed)),
                clock: Cell::new(0),
                zones: RefCell::new(Vec::new()),
                routes: RefCell::new(Vec::new()),
                couriers: RefCell::new(Vec::new()),
                parcels: RefCell::new(HashMap::new()),
                listeners: RefCell::new(Vec::new()),
                next_parcel: Cell::new(0),
                next_courier: Cell::new(0),
                vacancies: Cell::new(0),
                arrivals: 1 + couriers / 3,
                delivered_count: Cell::new(0),
                expired_count: Cell::new(0),
                retired_count: Cell::new(0),
                latency_total: Cell::new(0),
                handovers: Cell::new(0),
            }),
        };
        for i in 0..zones {
            let zone = Zone::new(&depot, i);
            depot.state.zones.borrow_mut().push(zone.state);
        }
        for zone in depot.zones() {
            // `(zone.id - 1) % zones` is never negative in Python.
            for other in [
                (zone.id() + 1) % zones,
                (zone.id() + zones - 1) % zones,
                (zone.id() * 2 + 1) % zones,
            ] {
                let link = depot.zone(other);
                let mut neighbours = zone.state.neighbours.borrow_mut();
                if !link.is(&zone) && !neighbours.contains(&link.id()) {
                    neighbours.push(link.id());
                }
            }
        }
        let routes = depot
            .zones()
            .iter()
            .map(|zone| depot.first_steps(zone))
            .collect();
        *depot.state.routes.borrow_mut() = routes;
        for _ in 0..couriers {
            depot.hire();
        }
        depot
    }

    /// Another name for the same depot, for the objects that belong to it.
    fn alias(&self) -> Depot {
        Depot {
            state: Rc::clone(&self.state),
        }
    }

    /// The `Parcel` object for a parcel held in one of the lists.
    fn parcel_of(&self, state: &Rc<ParcelState>) -> Parcel {
        Parcel {
            depot: self.alias(),
            state: Rc::clone(state),
        }
    }

    /// The `Courier` object for a courier held in one of the lists.
    fn courier_of(&self, state: &Rc<CourierState>) -> Courier {
        Courier {
            depot: self.alias(),
            state: Rc::clone(state),
        }
    }

    /// `self.zones`.
    fn zones(&self) -> Vec<Zone> {
        (0..self.state.zones.borrow().len())
            .map(|zid| self.zone(zid))
            .collect()
    }

    /// `self.couriers`, as it is now.
    fn couriers(&self) -> Vec<Courier> {
        let couriers = self.state.couriers.borrow();
        couriers.iter().map(|c| self.courier_of(c)).collect()
    }

    /// For every zone, the neighbour of `start` to go to first on a
    /// shortest way there. Roads are one-way where the express link is.
    fn first_steps(&self, start: &Zone) -> Vec<Option<usize>> {
        let mut first: Vec<Option<usize>> = vec![None; self.state.zones.borrow().len()];
        first[start.id()] = Some(start.id());
        let mut frontier = Vec::new();
        for zone in start.neighbours() {
            if first[zone.id()].is_none() {
                first[zone.id()] = Some(zone.id());
                frontier.push(zone);
            }
        }
        while !frontier.is_empty() {
            let mut reached = Vec::new();
            for zone in &frontier {
                for link in zone.neighbours() {
                    if first[link.id()].is_none() {
                        first[link.id()] = first[zone.id()];
                        reached.push(link);
                    }
                }
            }
            frontier = reached;
        }
        first
    }

    fn route(&self, here: &Zone, goal: &Zone) -> Zone {
        let first = self.state.routes.borrow()[here.id()][goal.id()];
        self.zone(first.expect("every zone can be reached from every other"))
    }

    fn hire(&self) -> Courier {
        let count = self.state.zones.borrow().len() as u64;
        let zone = self.zone(self.state.rng.borrow_mut().below(count) as usize);
        let capacity = 4 + self.state.rng.borrow_mut().below(5) as i64;
        let courier = Courier::new(self, self.state.next_courier.get(), &zone, capacity);
        self.state
            .next_courier
            .set(self.state.next_courier.get() + 1);
        self.state
            .couriers
            .borrow_mut()
            .push(Rc::clone(&courier.state));
        zone.state
            .couriers
            .borrow_mut()
            .push(Rc::clone(&courier.state));
        courier
    }

    /// The courier with this number, or None if there is none at work.
    pub fn courier(&self, cid: u64) -> Option<Courier> {
        self.couriers()
            .into_iter()
            .find(|courier| courier.id() == cid)
    }

    /// The zone with this number. Panics, where the Python raises
    /// `IndexError`, if there is none.
    pub fn zone(&self, zid: usize) -> Zone {
        Zone {
            depot: self.alias(),
            state: Rc::clone(&self.state.zones.borrow()[zid]),
        }
    }

    /// A new parcel, waiting in zone `origin`, for zone `dest`.
    pub fn submit(&mut self, origin: usize, dest: usize, weight: i64) -> Parcel {
        let parcel = Parcel::new(
            self,
            self.state.next_parcel.get(),
            &self.zone(origin),
            &self.zone(dest),
            weight,
        );
        self.state.next_parcel.set(self.state.next_parcel.get() + 1);
        self.state
            .parcels
            .borrow_mut()
            .insert(parcel.id(), Rc::clone(&parcel.state));
        parcel.origin().post(&parcel);
        parcel
    }

    /// `listener(parcel, courier)` is called at each delivery.
    pub fn on_delivered(&mut self, listener: impl FnMut(&Parcel, &Courier) + 'static) {
        self.state.listeners.borrow_mut().push(Box::new(listener));
    }

    fn record_delivery(&self, parcel: &Parcel, courier: &Courier) {
        parcel.close("delivered");
        self.forget(parcel);
        let state = &self.state;
        state.delivered_count.set(state.delivered_count.get() + 1);
        state
            .latency_total
            .set(state.latency_total.get() + parcel.age());
        for listener in state.listeners.borrow_mut().iter_mut() {
            listener(parcel, courier);
        }
    }

    /// `del self.parcels[parcel.id]`
    fn forget(&self, parcel: &Parcel) {
        self.state
            .parcels
            .borrow_mut()
            .remove(&parcel.id())
            .expect("KeyError: the parcel is not open");
    }

    fn expire(&self, parcel: &Parcel) {
        parcel.close("expired");
        self.forget(parcel);
        self.state
            .expired_count
            .set(self.state.expired_count.get() + 1);
    }

    /// The courier leaves; what was in the bag waits where they stood.
    fn retire(&self, courier: &Courier) {
        courier.state.retired.set(true);
        remove_first(&self.state.couriers, &courier.state);
        remove_first(&courier.zone().state.couriers, &courier.state);
        for parcel in courier.bag() {
            courier.zone().post(&parcel);
        }
        courier.state.bag.borrow_mut().clear();
        self.state
            .retired_count
            .set(self.state.retired_count.get() + 1);
        self.state.vacancies.set(self.state.vacancies.get() + 1);
    }

    pub fn step(&mut self) {
        self.state.clock.set(self.state.clock.get() + 1);
        for _ in 0..self.state.vacancies.get() {
            self.hire();
        }
        self.state.vacancies.set(0);
        let count = self.state.zones.borrow().len() as u64;
        for _ in 0..self.state.arrivals {
            let origin = self.state.rng.borrow_mut().below(count);
            let mut dest = self.state.rng.borrow_mut().below(count - 1);
            if dest >= origin {
                dest += 1;
            }
            let weight = 1 + self.state.rng.borrow_mut().below(4) as i64;
            self.submit(origin as usize, dest as usize, weight);
        }
        for courier in self.couriers() {
            courier.act();
        }
        for zone in self.zones() {
            zone.settle();
        }
    }

    /// The whole state as text, one line per thing.
    pub fn report(&self) -> String {
        fn parcels(list: &[Parcel]) -> String {
            let each: Vec<String> = list
                .iter()
                .map(|p| format!("{}:{}", p.id(), p.hops()))
                .collect();
            each.join(",")
        }
        let state = &self.state;
        let mut lines = vec![format!(
            "depot {} {} {} {} {} {} {} {} {} {}",
            state.clock.get(),
            state.rng.borrow().state,
            state.next_parcel.get(),
            state.next_courier.get(),
            state.parcels.borrow().len(),
            state.delivered_count.get(),
            state.expired_count.get(),
            state.retired_count.get(),
            state.latency_total.get(),
            state.handovers.get(),
        )];
        for zone in self.zones() {
            let couriers: Vec<String> =
                zone.couriers().iter().map(|c| c.id().to_string()).collect();
            lines.push(format!(
                "zone {} {} {} w {} c {}",
                zone.id(),
                zone.delivered(),
                zone.congestion(),
                parcels(&zone.waiting_list()),
                couriers.join(","),
            ));
        }
        for courier in self.couriers() {
            lines.push(format!(
                "courier {} {} {} {} {} {} b {}",
                courier.id(),
                courier.zone().id(),
                courier.capacity(),
                courier.fatigue(),
                courier.resting(),
                courier.delivered(),
                parcels(&courier.bag()),
            ));
        }
        lines.join("\n") + "\n"
    }
}

/// A run with nobody watching: the state after `steps` steps.
pub fn run(zones: usize, couriers: usize, seed: u64, steps: usize) -> String {
    let mut depot = Depot::new(zones, couriers, seed);
    for _ in 0..steps {
        depot.step();
    }
    depot.report()
}
