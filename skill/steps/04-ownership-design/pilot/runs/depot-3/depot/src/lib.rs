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
//! # How the Python objects are written here
//!
//! This is a port of `depot.py`, in the same order, class by class and method
//! by method. In Python every object is a reference that many others share
//! and change. Here each class is two types:
//!
//! * `XState` holds the attributes. The ones that change are in a `Cell`
//!   (plain values) or a `RefCell` (lists), so that they can be changed
//!   through a shared reference, as in Python.
//! * `X` (`Parcel`, `Zone`, `Courier`, `Depot`) is a handle: a counted
//!   reference (`Rc`) to the state. Cloning a handle is Python's "another
//!   name for the same object"; `==` on handles is Python's `is`. The methods
//!   of the Python class are the methods of the handle.
//!
//! Python frees objects that refer to each other in a circle; `Rc` does not.
//! So the references go one way only: the depot owns its zones, couriers and
//! open parcels, a zone owns its queue and the couriers standing in it, a
//! courier owns its bag. Every reference back up or sideways (`parcel.depot`,
//! `parcel.dest`, `parcel.carrier`, `courier.zone`, `zone.neighbours`, ...)
//! is a `Weak` one, which does not keep its target alive. A handle given out
//! to a caller carries, on top of that, a strong reference to the depot
//! (`_keep`): as in Python, an object that a caller keeps keeps the whole
//! depot it belongs to, so the weak references inside always find their
//! target.
//!
//! No `RefCell` borrow is held while a delivery listener runs, so a listener
//! may read and change any object it holds.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::{Rc, Weak};

/// A parcel that has waited longer than this is urgent.
const PATIENCE: u64 = 12;
/// A parcel that has waited longer than this is given up.
const EXPIRY: u64 = 40;
/// A courier this tired rests...
const FATIGUE_LIMIT: i64 = 25;
/// ...for this many steps.
const REST: i64 = 3;
/// Deliveries after which a courier retires, give or take.
const QUOTA: u64 = 30;
// `MASK` of the original is not needed: the state is a `u64` and the
// arithmetic wraps.

/// Python's `list.remove(item)`: takes out the first element that is `item`.
fn remove<T: PartialEq>(list: &RefCell<Vec<T>>, item: &T) {
    let mut list = list.borrow_mut();
    let at = list
        .iter()
        .position(|other| other == item)
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

/// Something to be carried from one zone to another.
#[derive(Clone)]
pub struct Parcel {
    state: Rc<ParcelState>,
    /// Set in the handles given to callers; see the top of the file.
    _keep: Option<Rc<DepotState>>,
}

struct ParcelState {
    depot: Weak<DepotState>,
    id: u64,
    origin: Weak<ZoneState>,
    dest: Weak<ZoneState>,
    weight: i64,
    born: u64,
    carrier: RefCell<Option<Weak<CourierState>>>,
    status: Cell<&'static str>,
    hops: Cell<u64>,
    closed_at: Cell<Option<u64>>,
}

/// Python's `is`.
impl PartialEq for Parcel {
    fn eq(&self, other: &Parcel) -> bool {
        Rc::ptr_eq(&self.state, &other.state)
    }
}

impl Parcel {
    fn new(depot: &Depot, pid: u64, origin: &Zone, dest: &Zone, weight: i64) -> Parcel {
        Parcel {
            state: Rc::new(ParcelState {
                depot: Rc::downgrade(&depot.state),
                id: pid,
                origin: Rc::downgrade(&origin.state),
                dest: Rc::downgrade(&dest.state),
                weight,
                born: depot.state.clock.get(),
                carrier: RefCell::new(None),
                status: Cell::new("waiting"),
                hops: Cell::new(0),
                closed_at: Cell::new(None),
            }),
            _keep: None,
        }
    }

    /// The same parcel, as a handle to give to a caller.
    fn kept(&self) -> Parcel {
        Parcel {
            state: self.state.clone(),
            _keep: self.state.depot.upgrade(),
        }
    }

    /// `self.depot`
    fn depot(&self) -> Depot {
        Depot::link(&self.state.depot)
    }

    /// `self.id`
    pub fn id(&self) -> u64 {
        self.state.id
    }

    /// `self.origin`
    fn origin(&self) -> Zone {
        Zone::link(&self.state.origin)
    }

    /// `self.dest`
    fn dest(&self) -> Zone {
        Zone::link(&self.state.dest)
    }

    /// `self.weight`
    fn weight(&self) -> i64 {
        self.state.weight
    }

    /// `self.carrier`: the courier who has it in the bag, if any.
    pub fn carrier(&self) -> Option<Courier> {
        let carrier = self.state.carrier.borrow();
        carrier
            .as_ref()
            .map(|courier| Courier::link(courier).kept())
    }

    /// `self.carrier = courier`
    fn set_carrier(&self, courier: Option<&Courier>) {
        *self.state.carrier.borrow_mut() = courier.map(|courier| Rc::downgrade(&courier.state));
    }

    /// `self.status`: "waiting", "carried", "delivered" or "expired".
    pub fn status(&self) -> &'static str {
        self.state.status.get()
    }

    /// `self.hops += 1`
    fn add_hop(&self) {
        self.state.hops.set(self.state.hops.get() + 1);
    }

    /// Steps since it appeared, up to the step it was closed.
    pub fn age(&self) -> u64 {
        let end = match self.state.closed_at.get() {
            None => self.depot().state.clock.get(),
            Some(closed_at) => closed_at,
        };
        end - self.state.born
    }

    fn urgent(&self) -> bool {
        self.state.closed_at.get().is_none() && self.age() > PATIENCE
    }

    fn close(&self, status: &'static str) {
        self.state.status.set(status);
        self.state
            .closed_at
            .set(Some(self.depot().state.clock.get()));
        self.set_carrier(None);
    }
}

/// A place where parcels wait and couriers pass through.
#[derive(Clone)]
pub struct Zone {
    state: Rc<ZoneState>,
    /// Set in the handles given to callers; see the top of the file.
    _keep: Option<Rc<DepotState>>,
}

struct ZoneState {
    depot: Weak<DepotState>,
    id: usize,
    neighbours: RefCell<Vec<Weak<ZoneState>>>,
    waiting: RefCell<Vec<Parcel>>,
    couriers: RefCell<Vec<Courier>>,
    delivered: Cell<u64>,
    congestion: Cell<i64>,
}

/// Python's `is`.
impl PartialEq for Zone {
    fn eq(&self, other: &Zone) -> bool {
        Rc::ptr_eq(&self.state, &other.state)
    }
}

impl Zone {
    fn new(depot: &Depot, zid: usize) -> Zone {
        Zone {
            state: Rc::new(ZoneState {
                depot: Rc::downgrade(&depot.state),
                id: zid,
                neighbours: RefCell::new(Vec::new()),
                waiting: RefCell::new(Vec::new()),
                couriers: RefCell::new(Vec::new()),
                delivered: Cell::new(0),
                congestion: Cell::new(0),
            }),
            _keep: None,
        }
    }

    /// The zone a weak reference points to.
    fn link(state: &Weak<ZoneState>) -> Zone {
        Zone {
            state: state.upgrade().expect("a zone lives as long as its depot"),
            _keep: None,
        }
    }

    /// The same zone, as a handle to give to a caller.
    fn kept(&self) -> Zone {
        Zone {
            state: self.state.clone(),
            _keep: self.state.depot.upgrade(),
        }
    }

    /// `self.depot`
    fn depot(&self) -> Depot {
        Depot::link(&self.state.depot)
    }

    /// `self.id`
    pub fn id(&self) -> usize {
        self.state.id
    }

    /// `list(self.neighbours)`
    fn neighbours(&self) -> Vec<Zone> {
        self.state
            .neighbours
            .borrow()
            .iter()
            .map(Zone::link)
            .collect()
    }

    /// `len(self.waiting)`: how many parcels are in the queue here.
    pub fn waiting(&self) -> usize {
        self.state.waiting.borrow().len()
    }

    /// `list(self.couriers)`
    fn couriers(&self) -> Vec<Courier> {
        self.state.couriers.borrow().clone()
    }

    /// `self.congestion`
    pub fn congestion(&self) -> i64 {
        self.state.congestion.get()
    }

    /// Puts a parcel at the end of the queue here.
    fn post(&self, parcel: &Parcel) {
        parcel.state.status.set("waiting");
        parcel.set_carrier(None);
        self.state.waiting.borrow_mut().push(parcel.clone());
    }

    /// Takes out of the queue the parcels the courier will carry: urgent
    /// ones first, then the others, each in order of arrival, as long as
    /// they fit.
    fn offer(&self, courier: &Courier) -> Vec<Parcel> {
        let mut room = courier.capacity() - courier.load();
        let mut taken = Vec::new();
        for urgent in [true, false] {
            for parcel in self.state.waiting.borrow().iter() {
                if parcel.urgent() == urgent && parcel.weight() <= room {
                    taken.push(parcel.clone());
                    room -= parcel.weight();
                }
            }
        }
        for parcel in &taken {
            remove(&self.state.waiting, parcel);
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
        let waiting = self.state.waiting.borrow().clone();
        for parcel in &waiting {
            if parcel.age() > EXPIRY {
                remove(&self.state.waiting, parcel);
                self.depot().expire(parcel);
            }
        }
        let weight: i64 = self
            .state
            .waiting
            .borrow()
            .iter()
            .map(|parcel| parcel.weight())
            .sum();
        // `//` rounds down, also below zero: `div_euclid`, not `/`.
        self.state
            .congestion
            .set((self.congestion() * 3 + weight * 4).div_euclid(4));
    }
}

/// Carries parcels, one zone per step.
#[derive(Clone)]
pub struct Courier {
    state: Rc<CourierState>,
    /// Set in the handles given to callers; see the top of the file.
    _keep: Option<Rc<DepotState>>,
}

struct CourierState {
    depot: Weak<DepotState>,
    id: u64,
    zone: RefCell<Weak<ZoneState>>,
    capacity: Cell<i64>,
    quota: u64,
    bag: RefCell<Vec<Parcel>>,
    fatigue: Cell<i64>,
    resting: Cell<i64>,
    delivered: Cell<u64>,
    retired: Cell<bool>,
}

/// Python's `is`.
impl PartialEq for Courier {
    fn eq(&self, other: &Courier) -> bool {
        Rc::ptr_eq(&self.state, &other.state)
    }
}

impl Courier {
    fn new(depot: &Depot, cid: u64, zone: &Zone, capacity: i64) -> Courier {
        Courier {
            state: Rc::new(CourierState {
                depot: Rc::downgrade(&depot.state),
                id: cid,
                zone: RefCell::new(Rc::downgrade(&zone.state)),
                capacity: Cell::new(capacity),
                quota: QUOTA + cid % 7,
                bag: RefCell::new(Vec::new()),
                fatigue: Cell::new(0),
                resting: Cell::new(0),
                delivered: Cell::new(0),
                retired: Cell::new(false),
            }),
            _keep: None,
        }
    }

    /// The courier a weak reference points to.
    fn link(state: &Weak<CourierState>) -> Courier {
        Courier {
            state: state.upgrade().expect("a carrier is a courier at work"),
            _keep: None,
        }
    }

    /// The same courier, as a handle to give to a caller.
    fn kept(&self) -> Courier {
        Courier {
            state: self.state.clone(),
            _keep: self.state.depot.upgrade(),
        }
    }

    /// `self.depot`
    fn depot(&self) -> Depot {
        Depot::link(&self.state.depot)
    }

    /// `self.id`
    pub fn id(&self) -> u64 {
        self.state.id
    }

    /// `self.zone`
    fn zone(&self) -> Zone {
        Zone::link(&self.state.zone.borrow())
    }

    /// `self.capacity`
    pub fn capacity(&self) -> i64 {
        self.state.capacity.get()
    }

    /// `self.capacity = capacity`: callers change this.
    pub fn set_capacity(&self, capacity: i64) {
        self.state.capacity.set(capacity);
    }

    /// `list(self.bag)`
    fn bag(&self) -> Vec<Parcel> {
        self.state.bag.borrow().clone()
    }

    /// `self.resting`
    fn resting(&self) -> i64 {
        self.state.resting.get()
    }

    /// `self.retired`
    pub fn retired(&self) -> bool {
        self.state.retired.get()
    }

    pub fn load(&self) -> i64 {
        self.state
            .bag
            .borrow()
            .iter()
            .map(|parcel| parcel.weight())
            .sum()
    }

    /// The zone this courier is going to, or None with an empty bag.
    fn heading(&self) -> Option<Zone> {
        self.state.bag.borrow().first().map(|parcel| parcel.dest())
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
        self.move_on();
        self.tire();
    }

    fn drop_off(&self) {
        for parcel in &self.bag() {
            if parcel.dest() == self.zone() {
                remove(&self.state.bag, parcel);
                let zone = self.zone();
                zone.state.delivered.set(zone.state.delivered.get() + 1);
                self.state.delivered.set(self.state.delivered.get() + 1);
                self.depot().record_delivery(parcel, self);
            }
        }
    }

    /// A parcel that a colleague here is heading for, and this courier
    /// is not, goes to the colleague if it fits.
    fn hand_over(&self) {
        let mine = self.heading();
        for peer in &self.zone().couriers() {
            if peer == self || peer.resting() > 0 {
                continue;
            }
            let theirs = peer.heading();
            if theirs.is_none() || theirs == mine {
                continue;
            }
            for parcel in &self.bag() {
                if Some(parcel.dest()) == theirs && peer.load() + parcel.weight() <= peer.capacity()
                {
                    remove(&self.state.bag, parcel);
                    peer.state.bag.borrow_mut().push(parcel.clone());
                    parcel.set_carrier(Some(peer));
                    parcel.add_hop();
                    let depot = self.depot();
                    depot.state.handovers.set(depot.state.handovers.get() + 1);
                }
            }
        }
    }

    fn pick_up(&self) {
        for parcel in self.zone().offer(self) {
            self.state.bag.borrow_mut().push(parcel.clone());
            parcel.set_carrier(Some(self));
            parcel.state.status.set("carried");
        }
    }

    /// `move` in the original; that word is taken in Rust.
    fn move_on(&self) {
        let here = self.zone();
        let goal = self.heading();
        let there = match goal {
            Some(goal) => self.depot().route(&here, &goal),
            None => {
                let there = here.busiest_neighbour();
                if there.congestion() <= here.congestion() {
                    return;
                }
                there
            }
        };
        remove(&here.state.couriers, self);
        there.state.couriers.borrow_mut().push(self.clone());
        *self.state.zone.borrow_mut() = Rc::downgrade(&there.state);
        // `//` rounds down, also below zero: `div_euclid`, not `/`.
        self.state
            .fatigue
            .set(self.state.fatigue.get() + 1 + self.load().div_euclid(8));
        for parcel in &self.bag() {
            parcel.add_hop();
        }
    }

    fn tire(&self) {
        if self.state.fatigue.get() >= FATIGUE_LIMIT {
            self.state.fatigue.set(0);
            self.state.resting.set(REST);
        }
        if self.state.delivered.get() >= self.state.quota {
            self.depot().retire(self);
        }
    }
}

/// `listener(parcel, courier)`, called at each delivery.
type Listener = Box<dyn FnMut(&Parcel, &Courier)>;

/// The whole simulation: zones, couriers, the parcels still open.
pub struct Depot {
    state: Rc<DepotState>,
}

struct DepotState {
    rng: RefCell<Rng>,
    clock: Cell<u64>,
    zones: RefCell<Vec<Zone>>,
    routes: RefCell<Vec<Vec<Option<usize>>>>,
    couriers: RefCell<Vec<Courier>>,
    parcels: RefCell<BTreeMap<u64, Parcel>>,
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

impl Depot {
    /// Panics with "at least three zones" where the original raises
    /// `ValueError`.
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
                parcels: RefCell::new(BTreeMap::new()),
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
        let all: Vec<Zone> = (0..zones).map(|i| Zone::new(&depot, i)).collect();
        *depot.state.zones.borrow_mut() = all.clone();
        for zone in &all {
            for other in [
                (zone.id() + 1) % zones,
                (zone.id() + zones - 1) % zones,
                (zone.id() * 2 + 1) % zones,
            ] {
                let link = &all[other];
                if link != zone && !zone.neighbours().contains(link) {
                    zone.state
                        .neighbours
                        .borrow_mut()
                        .push(Rc::downgrade(&link.state));
                }
            }
        }
        let routes = all.iter().map(|zone| depot.first_steps(zone)).collect();
        *depot.state.routes.borrow_mut() = routes;
        for _ in 0..couriers {
            depot.hire();
        }
        depot
    }

    /// The depot a weak reference points to.
    fn link(state: &Weak<DepotState>) -> Depot {
        Depot {
            state: state.upgrade().expect("the depot outlives what is in it"),
        }
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
        let first = self.state.routes.borrow()[here.id()][goal.id()];
        self.state.zones.borrow()[first.expect("every zone can be reached")].clone()
    }

    fn hire(&self) -> Courier {
        let count = self.state.zones.borrow().len() as u64;
        let zone =
            self.state.zones.borrow()[self.state.rng.borrow_mut().below(count) as usize].clone();
        let capacity = 4 + self.state.rng.borrow_mut().below(5) as i64;
        let courier = Courier::new(self, self.state.next_courier.get(), &zone, capacity);
        self.state
            .next_courier
            .set(self.state.next_courier.get() + 1);
        self.state.couriers.borrow_mut().push(courier.clone());
        zone.state.couriers.borrow_mut().push(courier.clone());
        courier
    }

    /// The courier with this number, or None if there is none at work.
    pub fn courier(&self, cid: u64) -> Option<Courier> {
        for courier in self.state.couriers.borrow().iter() {
            if courier.id() == cid {
                return Some(courier.kept());
            }
        }
        None
    }

    /// Panics if there is no such zone, where the original raises
    /// `IndexError`.
    pub fn zone(&self, zid: usize) -> Zone {
        self.state.zones.borrow()[zid].kept()
    }

    /// A new parcel, waiting in zone `origin`, for zone `dest`.
    pub fn submit(&mut self, origin: usize, dest: usize, weight: i64) -> Parcel {
        let origin = self.state.zones.borrow()[origin].clone();
        let dest = self.state.zones.borrow()[dest].clone();
        let parcel = Parcel::new(self, self.state.next_parcel.get(), &origin, &dest, weight);
        self.state.next_parcel.set(self.state.next_parcel.get() + 1);
        self.state
            .parcels
            .borrow_mut()
            .insert(parcel.id(), parcel.clone());
        parcel.origin().post(&parcel);
        parcel.kept()
    }

    /// `listener(parcel, courier)` is called at each delivery.
    pub fn on_delivered(&mut self, listener: impl FnMut(&Parcel, &Courier) + 'static) {
        self.state.listeners.borrow_mut().push(Box::new(listener));
    }

    fn record_delivery(&self, parcel: &Parcel, courier: &Courier) {
        parcel.close("delivered");
        self.state
            .parcels
            .borrow_mut()
            .remove(&parcel.id())
            .expect("a delivered parcel was open");
        self.state
            .delivered_count
            .set(self.state.delivered_count.get() + 1);
        self.state
            .latency_total
            .set(self.state.latency_total.get() + parcel.age());
        let (parcel, courier) = (parcel.kept(), courier.kept());
        for listener in self.state.listeners.borrow_mut().iter_mut() {
            listener(&parcel, &courier);
        }
    }

    fn expire(&self, parcel: &Parcel) {
        parcel.close("expired");
        self.state
            .parcels
            .borrow_mut()
            .remove(&parcel.id())
            .expect("an expired parcel was open");
        self.state
            .expired_count
            .set(self.state.expired_count.get() + 1);
    }

    /// The courier leaves; what was in the bag waits where they stood.
    fn retire(&self, courier: &Courier) {
        courier.state.retired.set(true);
        remove(&self.state.couriers, courier);
        remove(&courier.zone().state.couriers, courier);
        for parcel in &courier.bag() {
            courier.zone().post(parcel);
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
        let couriers = self.state.couriers.borrow().clone();
        for courier in &couriers {
            courier.act();
        }
        let zones = self.state.zones.borrow().clone();
        for zone in &zones {
            zone.settle();
        }
    }

    /// The whole state as text, one line per thing.
    pub fn report(&self) -> String {
        fn bag(parcels: &[Parcel]) -> String {
            let each: Vec<String> = parcels
                .iter()
                .map(|p| format!("{}:{}", p.id(), p.state.hops.get()))
                .collect();
            each.join(",")
        }
        let mut lines = vec![format!(
            "depot {} {} {} {} {} {} {} {} {} {}",
            self.state.clock.get(),
            self.state.rng.borrow().state,
            self.state.next_parcel.get(),
            self.state.next_courier.get(),
            self.state.parcels.borrow().len(),
            self.state.delivered_count.get(),
            self.state.expired_count.get(),
            self.state.retired_count.get(),
            self.state.latency_total.get(),
            self.state.handovers.get(),
        )];
        for zone in self.state.zones.borrow().iter() {
            let couriers: Vec<String> = zone
                .state
                .couriers
                .borrow()
                .iter()
                .map(|c| c.id().to_string())
                .collect();
            lines.push(format!(
                "zone {} {} {} w {} c {}",
                zone.id(),
                zone.state.delivered.get(),
                zone.congestion(),
                bag(&zone.state.waiting.borrow()),
                couriers.join(","),
            ));
        }
        for courier in self.state.couriers.borrow().iter() {
            lines.push(format!(
                "courier {} {} {} {} {} {} b {}",
                courier.id(),
                courier.zone().id(),
                courier.capacity(),
                courier.state.fatigue.get(),
                courier.resting(),
                courier.state.delivered.get(),
                bag(&courier.state.bag.borrow()),
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
