//! A courier depot, simulated one step at a time.
//!
//! Zones are joined by roads. Parcels appear in a zone and have to reach
//! another. Couriers carry them: each step a courier delivers what has
//! arrived, passes parcels to a colleague who is going their way, picks up
//! what fits, and moves one zone. Couriers tire and rest, and retire after
//! enough deliveries; the depot hires a replacement at the next step.
//! Parcels that wait too long are given up.
//!
//! Everything is whole numbers, and the same arguments always give the same
//! run. Other code holds on to the objects made here: a `Courier`, a `Zone`
//! or a `Parcel` that a caller keeps goes on showing the state of that
//! courier, zone or parcel, and a caller may change a courier.
//!
//! # How the Python objects are written here
//!
//! A Python object that several places refer to is, here, a small *handle*
//! (`Parcel`, `Zone`, `Courier`) that can be cloned freely; all clones show
//! and change the same state, as Python references do. `a == b` on handles
//! is Python's `a is b`.
//!
//! Each handle has two parts: `depot`, as in the Python, and `state`, the
//! attributes of the object (`ParcelState`, `ZoneState`, `CourierState`).
//! Attributes that change are in a `Cell` or a `RefCell`, which is what
//! lets them change while several handles exist.
//!
//! Python frees objects that refer to one another in a circle; Rust does
//! not. So inside the states the references that would close a circle
//! (parcel to zone, parcel to carrier, courier to zone, zone to neighbour)
//! are `Weak`, and the depot is not referred to at all; the handle supplies
//! it. A handle keeps its depot, and with it everything else, alive.
//!
//! # Numbers
//!
//! Python integers have no limit. Weights and capacities are `i64` here and
//! may be negative, as in the Python; sums made of them (a load, the
//! congestion of a zone, fatigue) are `i128` so that they cannot overflow.
//! `//` is `div_euclid`, which rounds down as Python does.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt;
use std::rc::{Rc, Weak};

/// A parcel that has waited longer than this is urgent.
pub const PATIENCE: u64 = 12;
/// A parcel that has waited longer than this is given up.
pub const EXPIRY: u64 = 40;
/// A courier this tired rests,
pub const FATIGUE_LIMIT: i128 = 25;
/// for this many steps.
pub const REST: u64 = 3;
/// Deliveries after which a courier retires, give or take.
pub const QUOTA: u64 = 30;
// MASK is not needed: the state is a `u64` and the arithmetic wraps.

/// `list.remove(item)`: takes out the first entry that is this very object.
fn remove<T>(list: &RefCell<Vec<Rc<T>>>, item: &Rc<T>) {
    let mut list = list.borrow_mut();
    let at = list
        .iter()
        .position(|entry| Rc::ptr_eq(entry, item))
        .expect("list.remove(x): x not in list");
    list.remove(at);
}

/// A linear congruential generator, so that a run can be repeated.
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng { state: seed }
    }

    // The name is the Python's; this is not an iterator.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state >> 33
    }

    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    pub fn state(&self) -> u64 {
        self.state
    }
}

/// The `status` of a parcel; printed, it is the Python string.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Waiting,
    Carried,
    Delivered,
    Expired,
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Status::Waiting => "waiting",
            Status::Carried => "carried",
            Status::Delivered => "delivered",
            Status::Expired => "expired",
        })
    }
}

struct ParcelState {
    id: u64,
    origin: Weak<ZoneState>,
    dest: Weak<ZoneState>,
    weight: i64,
    born: u64,
    carrier: RefCell<Option<Weak<CourierState>>>,
    status: Cell<Status>,
    hops: Cell<u64>,
    closed_at: Cell<Option<u64>>,
}

/// Something to be carried from one zone to another.
pub struct Parcel {
    depot: Depot,
    state: Rc<ParcelState>,
}

impl Parcel {
    fn new(depot: &Depot, pid: u64, origin: &Zone, dest: &Zone, weight: i64) -> Parcel {
        let state = ParcelState {
            id: pid,
            origin: Rc::downgrade(&origin.state),
            dest: Rc::downgrade(&dest.state),
            weight,
            born: depot.clock(),
            carrier: RefCell::new(None),
            status: Cell::new(Status::Waiting),
            hops: Cell::new(0),
            closed_at: Cell::new(None),
        };
        Parcel::of(depot, &Rc::new(state))
    }

    /// The handle for a parcel of this depot.
    fn of(depot: &Depot, state: &Rc<ParcelState>) -> Parcel {
        Parcel {
            depot: depot.share(),
            state: Rc::clone(state),
        }
    }

    pub fn id(&self) -> u64 {
        self.state.id
    }

    pub fn origin(&self) -> Zone {
        Zone::of_weak(&self.depot, &self.state.origin)
    }

    pub fn dest(&self) -> Zone {
        Zone::of_weak(&self.depot, &self.state.dest)
    }

    pub fn weight(&self) -> i64 {
        self.state.weight
    }

    pub fn born(&self) -> u64 {
        self.state.born
    }

    /// The courier that has it in the bag, if one has.
    pub fn carrier(&self) -> Option<Courier> {
        let carrier = self.state.carrier.borrow();
        let courier = carrier.as_ref()?.upgrade();
        // A parcel has a carrier only while it is in that courier's bag,
        // and such a courier is at work, so the depot has it.
        let courier = courier.expect("a carrier is a courier at work");
        Some(Courier::of(&self.depot, &courier))
    }

    fn set_carrier(&self, carrier: Option<&Courier>) {
        *self.state.carrier.borrow_mut() = carrier.map(|courier| Rc::downgrade(&courier.state));
    }

    pub fn status(&self) -> Status {
        self.state.status.get()
    }

    pub fn hops(&self) -> u64 {
        self.state.hops.get()
    }

    pub fn closed_at(&self) -> Option<u64> {
        self.state.closed_at.get()
    }

    /// Steps since it appeared, up to the step it was closed.
    pub fn age(&self) -> u64 {
        let end = match self.state.closed_at.get() {
            None => self.depot.clock(),
            Some(closed_at) => closed_at,
        };
        end - self.state.born
    }

    pub fn urgent(&self) -> bool {
        self.state.closed_at.get().is_none() && self.age() > PATIENCE
    }

    fn close(&self, status: Status) {
        self.state.status.set(status);
        self.state.closed_at.set(Some(self.depot.clock()));
        self.set_carrier(None);
    }
}

impl Clone for Parcel {
    fn clone(&self) -> Parcel {
        Parcel::of(&self.depot, &self.state)
    }
}

/// `a == b` is Python's `a is b`.
impl PartialEq for Parcel {
    fn eq(&self, other: &Parcel) -> bool {
        Rc::ptr_eq(&self.state, &other.state)
    }
}

struct ZoneState {
    id: usize,
    neighbours: RefCell<Vec<Weak<ZoneState>>>,
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

impl Zone {
    fn new(depot: &Depot, zid: usize) -> Zone {
        let state = ZoneState {
            id: zid,
            neighbours: RefCell::new(Vec::new()),
            waiting: RefCell::new(Vec::new()),
            couriers: RefCell::new(Vec::new()),
            delivered: Cell::new(0),
            congestion: Cell::new(0),
        };
        Zone::of(depot, &Rc::new(state))
    }

    /// The handle for a zone of this depot.
    fn of(depot: &Depot, state: &Rc<ZoneState>) -> Zone {
        Zone {
            depot: depot.share(),
            state: Rc::clone(state),
        }
    }

    fn of_weak(depot: &Depot, state: &Weak<ZoneState>) -> Zone {
        // The depot never lets go of a zone.
        Zone::of(depot, &state.upgrade().expect("the depot keeps its zones"))
    }

    pub fn id(&self) -> usize {
        self.state.id
    }

    /// `zone.neighbours`, as it is now.
    pub fn neighbours(&self) -> Vec<Zone> {
        let neighbours = self.state.neighbours.borrow();
        neighbours
            .iter()
            .map(|zone| Zone::of_weak(&self.depot, zone))
            .collect()
    }

    /// `len(zone.waiting)`: how many parcels wait here.
    pub fn waiting(&self) -> usize {
        self.state.waiting.borrow().len()
    }

    /// `zone.waiting`, as it is now.
    pub fn waiting_parcels(&self) -> Vec<Parcel> {
        let waiting = self.state.waiting.borrow();
        waiting
            .iter()
            .map(|parcel| Parcel::of(&self.depot, parcel))
            .collect()
    }

    /// `zone.couriers`, as it is now.
    pub fn couriers(&self) -> Vec<Courier> {
        let couriers = self.state.couriers.borrow();
        couriers
            .iter()
            .map(|courier| Courier::of(&self.depot, courier))
            .collect()
    }

    pub fn delivered(&self) -> u64 {
        self.state.delivered.get()
    }

    pub fn congestion(&self) -> i128 {
        self.state.congestion.get()
    }

    /// Puts a parcel at the end of the queue here.
    fn post(&self, parcel: &Parcel) {
        parcel.state.status.set(Status::Waiting);
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
            for parcel in self.waiting_parcels() {
                if parcel.urgent() == urgent && i128::from(parcel.weight()) <= room {
                    room -= i128::from(parcel.weight());
                    taken.push(parcel);
                }
            }
        }
        for parcel in &taken {
            remove(&self.state.waiting, &parcel.state);
        }
        taken
    }

    /// The neighbour with the most congestion; the first of them if
    /// several have the same.
    fn busiest_neighbour(&self) -> Zone {
        let mut neighbours = self.neighbours().into_iter();
        let mut best = neighbours.next().expect("a zone has a neighbour");
        for zone in neighbours {
            if zone.congestion() > best.congestion() {
                best = zone;
            }
        }
        best
    }

    /// End of a step: gives up parcels that waited too long and brings
    /// the congestion figure up to date.
    fn settle(&self) {
        for parcel in self.waiting_parcels() {
            if parcel.age() > EXPIRY {
                remove(&self.state.waiting, &parcel.state);
                self.depot.expire(&parcel);
            }
        }
        let weight: i128 = self
            .waiting_parcels()
            .iter()
            .map(|parcel| i128::from(parcel.weight()))
            .sum();
        let congestion = (self.congestion() * 3 + weight * 4).div_euclid(4);
        self.state.congestion.set(congestion);
    }
}

impl Clone for Zone {
    fn clone(&self) -> Zone {
        Zone::of(&self.depot, &self.state)
    }
}

/// `a == b` is Python's `a is b`.
impl PartialEq for Zone {
    fn eq(&self, other: &Zone) -> bool {
        Rc::ptr_eq(&self.state, &other.state)
    }
}

struct CourierState {
    id: u64,
    zone: RefCell<Weak<ZoneState>>,
    capacity: Cell<i64>,
    quota: u64,
    bag: RefCell<Vec<Rc<ParcelState>>>,
    fatigue: Cell<i128>,
    resting: Cell<u64>,
    delivered: Cell<u64>,
    retired: Cell<bool>,
}

/// Carries parcels, one zone per step.
pub struct Courier {
    depot: Depot,
    state: Rc<CourierState>,
}

impl Courier {
    fn new(depot: &Depot, cid: u64, zone: &Zone, capacity: i64) -> Courier {
        let state = CourierState {
            id: cid,
            zone: RefCell::new(Rc::downgrade(&zone.state)),
            capacity: Cell::new(capacity),
            quota: QUOTA + cid % 7,
            bag: RefCell::new(Vec::new()),
            fatigue: Cell::new(0),
            resting: Cell::new(0),
            delivered: Cell::new(0),
            retired: Cell::new(false),
        };
        Courier::of(depot, &Rc::new(state))
    }

    /// The handle for a courier of this depot.
    fn of(depot: &Depot, state: &Rc<CourierState>) -> Courier {
        Courier {
            depot: depot.share(),
            state: Rc::clone(state),
        }
    }

    pub fn id(&self) -> u64 {
        self.state.id
    }

    pub fn zone(&self) -> Zone {
        Zone::of_weak(&self.depot, &self.state.zone.borrow())
    }

    pub fn capacity(&self) -> i64 {
        self.state.capacity.get()
    }

    /// `courier.capacity = capacity`
    pub fn set_capacity(&self, capacity: i64) {
        self.state.capacity.set(capacity);
    }

    pub fn quota(&self) -> u64 {
        self.state.quota
    }

    /// `courier.bag`, as it is now.
    pub fn bag(&self) -> Vec<Parcel> {
        let bag = self.state.bag.borrow();
        bag.iter()
            .map(|parcel| Parcel::of(&self.depot, parcel))
            .collect()
    }

    pub fn fatigue(&self) -> i128 {
        self.state.fatigue.get()
    }

    pub fn resting(&self) -> u64 {
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
    pub fn heading(&self) -> Option<Zone> {
        let bag = self.state.bag.borrow();
        let first = bag.first()?;
        Some(Zone::of_weak(&self.depot, &first.dest))
    }

    /// Callers use this to take a courier off the road for a while.
    pub fn rest(&self, steps: u64) {
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
        self.move_on();
        self.tire();
    }

    fn drop_off(&self) {
        for parcel in self.bag() {
            if parcel.dest() == self.zone() {
                remove(&self.state.bag, &parcel.state);
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
            if peer == *self || peer.resting() > 0 {
                continue;
            }
            let theirs = peer.heading();
            if theirs.is_none() || theirs == mine {
                continue;
            }
            for parcel in self.bag() {
                if Some(parcel.dest()) == theirs
                    && peer.load() + i128::from(parcel.weight()) <= i128::from(peer.capacity())
                {
                    remove(&self.state.bag, &parcel.state);
                    peer.state.bag.borrow_mut().push(Rc::clone(&parcel.state));
                    parcel.set_carrier(Some(&peer));
                    parcel.state.hops.set(parcel.hops() + 1);
                    self.depot.add_handover();
                }
            }
        }
    }

    fn pick_up(&self) {
        for parcel in self.zone().offer(self) {
            self.state.bag.borrow_mut().push(Rc::clone(&parcel.state));
            parcel.set_carrier(Some(self));
            parcel.state.status.set(Status::Carried);
        }
    }

    /// `move` in the Python; that is a reserved word in Rust.
    fn move_on(&self) {
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
        remove(&here.state.couriers, &self.state);
        there
            .state
            .couriers
            .borrow_mut()
            .push(Rc::clone(&self.state));
        *self.state.zone.borrow_mut() = Rc::downgrade(&there.state);
        let fatigue = self.fatigue() + 1 + self.load().div_euclid(8);
        self.state.fatigue.set(fatigue);
        for parcel in self.bag() {
            parcel.state.hops.set(parcel.hops() + 1);
        }
    }

    fn tire(&self) {
        if self.fatigue() >= FATIGUE_LIMIT {
            self.state.fatigue.set(0);
            self.state.resting.set(REST);
        }
        if self.delivered() >= self.quota() {
            self.depot.retire(self);
        }
    }
}

impl Clone for Courier {
    fn clone(&self) -> Courier {
        Courier::of(&self.depot, &self.state)
    }
}

/// `a == b` is Python's `a is b`.
impl PartialEq for Courier {
    fn eq(&self, other: &Courier) -> bool {
        Rc::ptr_eq(&self.state, &other.state)
    }
}

type Listener = Box<dyn FnMut(&Parcel, &Courier)>;

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
    arrivals: u64,
    delivered_count: Cell<u64>,
    expired_count: Cell<u64>,
    retired_count: Cell<u64>,
    latency_total: Cell<u64>,
    handovers: Cell<u64>,
}

/// The whole simulation: zones, couriers, the parcels still open.
///
/// The caller has the one `Depot`; it cannot be cloned. The methods that a
/// caller drives it with (`submit`, `step`, `on_delivered`, `hire`) take
/// `&mut self`, so a listener cannot call them in the middle of a step.
pub struct Depot {
    state: Rc<DepotState>,
}

impl Depot {
    /// Panics with "at least three zones" where the Python raises
    /// `ValueError`.
    pub fn new(zones: usize, couriers: usize, seed: u64) -> Depot {
        if zones < 3 {
            panic!("at least three zones");
        }
        let mut depot = Depot {
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
                arrivals: 1 + couriers as u64 / 3,
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
            // (zone.id - 1) % zones is written with `+ zones` because the
            // numbers here have no sign.
            for other in [
                (zone.id() + 1) % zones,
                (zone.id() + zones - 1) % zones,
                (zone.id() * 2 + 1) % zones,
            ] {
                let link = depot.zone(other);
                if link != zone && !zone.neighbours().contains(&link) {
                    let mut neighbours = zone.state.neighbours.borrow_mut();
                    neighbours.push(Rc::downgrade(&link.state));
                }
            }
        }
        for zone in depot.zones() {
            let first = depot.first_steps(&zone);
            depot.state.routes.borrow_mut().push(first);
        }
        for _ in 0..couriers {
            depot.hire();
        }
        depot
    }

    /// Another reference to this depot, for the handles of its objects.
    fn share(&self) -> Depot {
        Depot {
            state: Rc::clone(&self.state),
        }
    }

    pub fn clock(&self) -> u64 {
        self.state.clock.get()
    }

    /// `depot.zones`
    pub fn zones(&self) -> Vec<Zone> {
        let zones = self.state.zones.borrow();
        zones.iter().map(|zone| Zone::of(self, zone)).collect()
    }

    /// `depot.couriers`, as it is now.
    pub fn couriers(&self) -> Vec<Courier> {
        let couriers = self.state.couriers.borrow();
        couriers
            .iter()
            .map(|courier| Courier::of(self, courier))
            .collect()
    }

    /// For every zone, the neighbour of `start` to go to first on a
    /// shortest way there. Roads are one-way where the express link is.
    fn first_steps(&self, start: &Zone) -> Vec<Option<usize>> {
        let mut first = vec![None; self.state.zones.borrow().len()];
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
        // Never None: every zone has a road to the next one, so every zone
        // can be reached from every other.
        let first = self.state.routes.borrow()[here.id()][goal.id()];
        self.zone(first.expect("every zone can be reached"))
    }

    pub fn hire(&mut self) -> Courier {
        let count = self.state.zones.borrow().len() as u64;
        let zone = self.zone(self.below(count) as usize);
        let capacity = 4 + self.below(5) as i64;
        let courier = Courier::new(self, self.state.next_courier.get(), &zone, capacity);
        self.state
            .next_courier
            .set(self.state.next_courier.get() + 1);
        let mut couriers = self.state.couriers.borrow_mut();
        couriers.push(Rc::clone(&courier.state));
        drop(couriers);
        let mut couriers = zone.state.couriers.borrow_mut();
        couriers.push(Rc::clone(&courier.state));
        drop(couriers);
        courier
    }

    /// `self.rng.below(n)`
    fn below(&self, n: u64) -> u64 {
        self.state.rng.borrow_mut().below(n)
    }

    /// The courier with this number, or None if there is none at work.
    pub fn courier(&self, cid: u64) -> Option<Courier> {
        self.couriers()
            .into_iter()
            .find(|courier| courier.id() == cid)
    }

    /// Panics if there is no such zone, where the Python raises
    /// `IndexError`.
    pub fn zone(&self, zid: usize) -> Zone {
        Zone::of(self, &self.state.zones.borrow()[zid])
    }

    /// A new parcel, waiting in zone `origin`, for zone `dest`.
    pub fn submit(&mut self, origin: usize, dest: usize, weight: i64) -> Parcel {
        let origin = self.zone(origin);
        let dest = self.zone(dest);
        let parcel = Parcel::new(self, self.state.next_parcel.get(), &origin, &dest, weight);
        self.state.next_parcel.set(self.state.next_parcel.get() + 1);
        let mut parcels = self.state.parcels.borrow_mut();
        parcels.insert(parcel.id(), Rc::clone(&parcel.state));
        drop(parcels);
        parcel.origin().post(&parcel);
        parcel
    }

    /// `listener(parcel, courier)` is called at each delivery.
    pub fn on_delivered(&mut self, listener: impl FnMut(&Parcel, &Courier) + 'static) {
        self.state.listeners.borrow_mut().push(Box::new(listener));
    }

    fn record_delivery(&self, parcel: &Parcel, courier: &Courier) {
        parcel.close(Status::Delivered);
        self.state.parcels.borrow_mut().remove(&parcel.id());
        self.state
            .delivered_count
            .set(self.state.delivered_count.get() + 1);
        self.state
            .latency_total
            .set(self.state.latency_total.get() + parcel.age());
        // A listener has no way to the depot's `&mut self` methods, so the
        // list cannot be asked for again while it is borrowed here.
        for listener in self.state.listeners.borrow_mut().iter_mut() {
            listener(parcel, courier);
        }
    }

    fn expire(&self, parcel: &Parcel) {
        parcel.close(Status::Expired);
        self.state.parcels.borrow_mut().remove(&parcel.id());
        self.state
            .expired_count
            .set(self.state.expired_count.get() + 1);
    }

    /// The courier leaves; what was in the bag waits where they stood.
    fn retire(&self, courier: &Courier) {
        courier.state.retired.set(true);
        remove(&self.state.couriers, &courier.state);
        remove(&courier.zone().state.couriers, &courier.state);
        for parcel in courier.bag() {
            courier.zone().post(&parcel);
        }
        courier.state.bag.borrow_mut().clear();
        self.state
            .retired_count
            .set(self.state.retired_count.get() + 1);
        self.state.vacancies.set(self.state.vacancies.get() + 1);
    }

    /// `self.depot.handovers += 1`
    fn add_handover(&self) {
        self.state.handovers.set(self.state.handovers.get() + 1);
    }

    pub fn step(&mut self) {
        self.state.clock.set(self.clock() + 1);
        for _ in 0..self.state.vacancies.get() {
            self.hire();
        }
        self.state.vacancies.set(0);
        let count = self.state.zones.borrow().len() as u64;
        for _ in 0..self.state.arrivals {
            let origin = self.below(count);
            let mut dest = self.below(count - 1);
            if dest >= origin {
                dest += 1;
            }
            let weight = 1 + self.below(4) as i64;
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
        let mut lines = vec![format!(
            "depot {} {} {} {} {} {} {} {} {} {}",
            self.clock(),
            self.state.rng.borrow().state(),
            self.state.next_parcel.get(),
            self.state.next_courier.get(),
            self.state.parcels.borrow().len(),
            self.state.delivered_count.get(),
            self.state.expired_count.get(),
            self.state.retired_count.get(),
            self.state.latency_total.get(),
            self.state.handovers.get(),
        )];
        let parcels = |parcels: Vec<Parcel>| {
            let texts: Vec<String> = parcels
                .iter()
                .map(|p| format!("{}:{}", p.id(), p.hops()))
                .collect();
            texts.join(",")
        };
        for zone in self.zones() {
            let couriers: Vec<String> =
                zone.couriers().iter().map(|c| c.id().to_string()).collect();
            lines.push(format!(
                "zone {} {} {} w {} c {}",
                zone.id(),
                zone.delivered(),
                zone.congestion(),
                parcels(zone.waiting_parcels()),
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
                parcels(courier.bag()),
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
