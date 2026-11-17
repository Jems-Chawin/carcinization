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
//! `tests/callers.rs`): a [`Courier`], a [`Zone`] or a [`Parcel`] that a
//! caller keeps goes on showing the state of that courier, zone or parcel,
//! and a caller may change a courier.
//!
//! # How the Python objects are represented
//!
//! A Python object that is shared and changed in place becomes two things:
//!
//! * a private `...State` struct with the attributes of the Python class
//!   (the ones that change are in a `Cell` or a `RefCell`), kept behind an
//!   `Rc`, and
//! * a public handle (`Parcel`, `Zone`, `Courier`) which is that `Rc` plus
//!   the depot the object belongs to: the `self.depot` of the Python. The
//!   methods of the Python class are the methods of the handle. Cloning a
//!   handle gives another reference to the same object, as assignment does
//!   in Python, and `is` in the Python is [`Rc::ptr_eq`] here.
//!
//! The lists inside the state structs hold the bare `Rc`s, not handles, so
//! that the depot is kept alive by whoever holds the `Depot` or one of the
//! handles, and by nothing else.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::fmt;
use std::rc::Rc;

/// A parcel that has waited longer than this is urgent.
pub const PATIENCE: u64 = 12;
/// A parcel that has waited longer than this is given up.
pub const EXPIRY: u64 = 40;
/// A courier this tired rests...
pub const FATIGUE_LIMIT: i64 = 25;
/// ...for this many steps.
pub const REST: i64 = 3;
/// Deliveries after which a courier retires, give or take.
pub const QUOTA: u64 = 30;
// `MASK` of the Python is not needed: `u64` arithmetic that wraps is the
// same as arithmetic `& MASK`.

/// `list.remove(item)`: takes out the first element that *is* `item`.
/// Like the Python, it is an error for the item not to be there.
fn remove_first<T>(list: &RefCell<Vec<Rc<T>>>, item: &Rc<T>) {
    let mut list = list.borrow_mut();
    let at = list
        .iter()
        .position(|other| Rc::ptr_eq(other, item))
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
}

/// The `status` of a parcel; it prints as the string the Python uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Waiting,
    Carried,
    Delivered,
    Expired,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Waiting => "waiting",
            Status::Carried => "carried",
            Status::Delivered => "delivered",
            Status::Expired => "expired",
        }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The attributes of a `Parcel` (all but `depot`, which is in the handle).
struct ParcelState {
    id: u64,
    origin: Rc<ZoneState>,
    dest: Rc<ZoneState>,
    weight: i64,
    born: u64,
    carrier: RefCell<Option<Rc<CourierState>>>,
    status: Cell<Status>,
    hops: Cell<u64>,
    closed_at: Cell<Option<u64>>,
}

/// Something to be carried from one zone to another.
pub struct Parcel {
    depot: Depot,
    state: Rc<ParcelState>,
}

impl Clone for Parcel {
    /// Another reference to the same parcel.
    fn clone(&self) -> Parcel {
        Parcel::wrap(&self.depot, &self.state)
    }
}

impl Parcel {
    /// `Parcel.__init__`
    fn new(depot: &Depot, pid: u64, origin: &Zone, dest: &Zone, weight: i64) -> Parcel {
        let state = ParcelState {
            id: pid,
            origin: Rc::clone(&origin.state),
            dest: Rc::clone(&dest.state),
            weight,
            born: depot.state.clock.get(),
            carrier: RefCell::new(None),
            status: Cell::new(Status::Waiting),
            hops: Cell::new(0),
            closed_at: Cell::new(None),
        };
        Parcel::wrap(depot, &Rc::new(state))
    }

    fn wrap(depot: &Depot, state: &Rc<ParcelState>) -> Parcel {
        Parcel {
            depot: depot.alias(),
            state: Rc::clone(state),
        }
    }

    pub fn id(&self) -> u64 {
        self.state.id
    }

    pub fn origin(&self) -> Zone {
        Zone::wrap(&self.depot, &self.state.origin)
    }

    pub fn dest(&self) -> Zone {
        Zone::wrap(&self.depot, &self.state.dest)
    }

    pub fn weight(&self) -> i64 {
        self.state.weight
    }

    pub fn born(&self) -> u64 {
        self.state.born
    }

    pub fn carrier(&self) -> Option<Courier> {
        let carrier = self.state.carrier.borrow();
        carrier
            .as_ref()
            .map(|state| Courier::wrap(&self.depot, state))
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
            None => self.depot.state.clock.get(),
            Some(closed_at) => closed_at,
        };
        end - self.state.born
    }

    pub fn urgent(&self) -> bool {
        self.state.closed_at.get().is_none() && self.age() > PATIENCE
    }

    fn close(&self, status: Status) {
        self.state.status.set(status);
        self.state.closed_at.set(Some(self.depot.state.clock.get()));
        *self.state.carrier.borrow_mut() = None;
    }

    /// `"%d:%d" % (p.id, p.hops)`, as the report prints a parcel.
    fn brief(&self) -> String {
        format!("{}:{}", self.id(), self.hops())
    }
}

/// The attributes of a `Zone` (all but `depot`, which is in the handle).
struct ZoneState {
    id: usize,
    neighbours: RefCell<Vec<Rc<ZoneState>>>,
    waiting: RefCell<Vec<Rc<ParcelState>>>,
    couriers: RefCell<Vec<Rc<CourierState>>>,
    delivered: Cell<u64>,
    congestion: Cell<i64>,
}

/// A place where parcels wait and couriers pass through.
pub struct Zone {
    depot: Depot,
    state: Rc<ZoneState>,
}

impl Clone for Zone {
    /// Another reference to the same zone.
    fn clone(&self) -> Zone {
        Zone::wrap(&self.depot, &self.state)
    }
}

impl Zone {
    /// `Zone.__init__`, without the handle: the depot makes its zones
    /// before it exists itself.
    fn new_state(zid: usize) -> Rc<ZoneState> {
        Rc::new(ZoneState {
            id: zid,
            neighbours: RefCell::new(Vec::new()),
            waiting: RefCell::new(Vec::new()),
            couriers: RefCell::new(Vec::new()),
            delivered: Cell::new(0),
            congestion: Cell::new(0),
        })
    }

    fn wrap(depot: &Depot, state: &Rc<ZoneState>) -> Zone {
        Zone {
            depot: depot.alias(),
            state: Rc::clone(state),
        }
    }

    /// `self is other`
    fn is(&self, other: &Zone) -> bool {
        Rc::ptr_eq(&self.state, &other.state)
    }

    pub fn id(&self) -> usize {
        self.state.id
    }

    /// `len(zone.waiting)`: how many parcels wait here.
    pub fn waiting(&self) -> usize {
        self.state.waiting.borrow().len()
    }

    pub fn delivered(&self) -> u64 {
        self.state.delivered.get()
    }

    pub fn congestion(&self) -> i64 {
        self.state.congestion.get()
    }

    /// `list(zone.neighbours)`
    pub fn neighbours(&self) -> Vec<Zone> {
        let neighbours = self.state.neighbours.borrow();
        neighbours
            .iter()
            .map(|state| Zone::wrap(&self.depot, state))
            .collect()
    }

    /// `list(zone.waiting)`
    pub fn waiting_parcels(&self) -> Vec<Parcel> {
        let waiting = self.state.waiting.borrow();
        waiting
            .iter()
            .map(|state| Parcel::wrap(&self.depot, state))
            .collect()
    }

    /// `list(zone.couriers)`
    pub fn couriers(&self) -> Vec<Courier> {
        let couriers = self.state.couriers.borrow();
        couriers
            .iter()
            .map(|state| Courier::wrap(&self.depot, state))
            .collect()
    }

    /// Puts a parcel at the end of the queue here.
    fn post(&self, parcel: &Parcel) {
        parcel.state.status.set(Status::Waiting);
        *parcel.state.carrier.borrow_mut() = None;
        self.state
            .waiting
            .borrow_mut()
            .push(Rc::clone(&parcel.state));
    }

    /// Takes out of the queue the parcels the courier will carry: urgent
    /// ones first, then the others, each in order of arrival, as long as
    /// they fit.
    fn offer(&self, courier: &Courier) -> Vec<Parcel> {
        let mut room = courier.capacity() - courier.load();
        let mut taken = Vec::new();
        for urgent in [true, false] {
            for parcel in self.waiting_parcels() {
                if parcel.urgent() == urgent && parcel.weight() <= room {
                    room -= parcel.weight();
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
        let mut best = &neighbours[0];
        for zone in &neighbours[1..] {
            if zone.congestion() > best.congestion() {
                best = zone;
            }
        }
        best.clone()
    }

    /// End of a step: gives up parcels that waited too long and brings
    /// the congestion figure up to date.
    fn settle(&self) {
        for parcel in self.waiting_parcels() {
            if parcel.age() > EXPIRY {
                remove_first(&self.state.waiting, &parcel.state);
                self.depot.expire(&parcel);
            }
        }
        let weight: i64 = self
            .waiting_parcels()
            .iter()
            .map(|parcel| parcel.weight())
            .sum();
        // `//` rounds down; `div_euclid` by a positive number does too.
        self.state
            .congestion
            .set((self.congestion() * 3 + weight * 4).div_euclid(4));
    }
}

/// The attributes of a `Courier` (all but `depot`, which is in the handle).
struct CourierState {
    id: u64,
    zone: RefCell<Rc<ZoneState>>,
    capacity: Cell<i64>,
    quota: u64,
    bag: RefCell<Vec<Rc<ParcelState>>>,
    fatigue: Cell<i64>,
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
    /// Another reference to the same courier.
    fn clone(&self) -> Courier {
        Courier::wrap(&self.depot, &self.state)
    }
}

impl Courier {
    /// `Courier.__init__`
    fn new(depot: &Depot, cid: u64, zone: &Zone, capacity: i64) -> Courier {
        let state = CourierState {
            id: cid,
            zone: RefCell::new(Rc::clone(&zone.state)),
            capacity: Cell::new(capacity),
            quota: QUOTA + cid % 7,
            bag: RefCell::new(Vec::new()),
            fatigue: Cell::new(0),
            resting: Cell::new(0),
            delivered: Cell::new(0),
            retired: Cell::new(false),
        };
        Courier::wrap(depot, &Rc::new(state))
    }

    fn wrap(depot: &Depot, state: &Rc<CourierState>) -> Courier {
        Courier {
            depot: depot.alias(),
            state: Rc::clone(state),
        }
    }

    /// `self is other`
    fn is(&self, other: &Courier) -> bool {
        Rc::ptr_eq(&self.state, &other.state)
    }

    pub fn id(&self) -> u64 {
        self.state.id
    }

    pub fn zone(&self) -> Zone {
        Zone::wrap(&self.depot, &self.state.zone.borrow())
    }

    pub fn capacity(&self) -> i64 {
        self.state.capacity.get()
    }

    /// `courier.capacity = capacity`: callers change this.
    pub fn set_capacity(&self, capacity: i64) {
        self.state.capacity.set(capacity);
    }

    pub fn quota(&self) -> u64 {
        self.state.quota
    }

    /// `list(courier.bag)`
    pub fn bag(&self) -> Vec<Parcel> {
        let bag = self.state.bag.borrow();
        bag.iter()
            .map(|state| Parcel::wrap(&self.depot, state))
            .collect()
    }

    pub fn fatigue(&self) -> i64 {
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

    pub fn load(&self) -> i64 {
        let bag = self.state.bag.borrow();
        bag.iter().map(|parcel| parcel.weight).sum()
    }

    /// The zone this courier is going to, or `None` with an empty bag.
    pub fn heading(&self) -> Option<Zone> {
        let bag = self.state.bag.borrow();
        bag.first()
            .map(|parcel| Zone::wrap(&self.depot, &parcel.dest))
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
        self.move_();
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
            // `if theirs is None or theirs is mine: continue`
            let Some(theirs) = peer.heading() else {
                continue;
            };
            if mine.as_ref().is_some_and(|mine| theirs.is(mine)) {
                continue;
            }
            for parcel in self.bag() {
                if parcel.dest().is(&theirs) && peer.load() + parcel.weight() <= peer.capacity() {
                    remove_first(&self.state.bag, &parcel.state);
                    peer.state.bag.borrow_mut().push(Rc::clone(&parcel.state));
                    *parcel.state.carrier.borrow_mut() = Some(Rc::clone(&peer.state));
                    parcel.state.hops.set(parcel.hops() + 1);
                    let depot = &self.depot.state;
                    depot.handovers.set(depot.handovers.get() + 1);
                }
            }
        }
    }

    fn pick_up(&self) {
        for parcel in self.zone().offer(self) {
            self.state.bag.borrow_mut().push(Rc::clone(&parcel.state));
            *parcel.state.carrier.borrow_mut() = Some(Rc::clone(&self.state));
            parcel.state.status.set(Status::Carried);
        }
    }

    /// `Courier.move` (`move` is a reserved word in Rust).
    fn move_(&self) {
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
        *self.state.zone.borrow_mut() = Rc::clone(&there.state);
        // `//` rounds down; `div_euclid` by a positive number does too.
        self.state
            .fatigue
            .set(self.fatigue() + 1 + self.load().div_euclid(8));
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

/// What `on_delivered` keeps: `listener(parcel, courier)`.
type Listener = Box<dyn FnMut(&Parcel, &Courier)>;

/// The attributes of a `Depot`.
struct DepotState {
    rng: RefCell<Rng>,
    clock: Cell<u64>,
    zones: Vec<Rc<ZoneState>>,
    routes: RefCell<Vec<Vec<Option<usize>>>>,
    couriers: RefCell<Vec<Rc<CourierState>>>,
    parcels: RefCell<BTreeMap<u64, Rc<ParcelState>>>,
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

impl Drop for DepotState {
    /// Python's collector frees objects that refer to each other; `Rc`
    /// does not. This runs when the `Depot` and every handle are gone, so
    /// nobody can see it: it undoes the references that go in circles
    /// (zone to zone, zone to courier and back, courier to parcel and back).
    fn drop(&mut self) {
        for zone in &self.zones {
            zone.neighbours.borrow_mut().clear();
            zone.waiting.borrow_mut().clear();
            zone.couriers.borrow_mut().clear();
        }
        for courier in self.couriers.borrow().iter() {
            courier.bag.borrow_mut().clear();
        }
    }
}

/// `ValueError("at least three zones")`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValueError(pub &'static str);

impl fmt::Display for ValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for ValueError {}

/// The whole simulation: zones, couriers, the parcels still open.
pub struct Depot {
    state: Rc<DepotState>,
}

impl Depot {
    /// `Depot.__init__`. Panics where the Python raises `ValueError`;
    /// [`Depot::try_new`] returns the error instead.
    pub fn new(zones: usize, couriers: usize, seed: u64) -> Depot {
        match Depot::try_new(zones, couriers, seed) {
            Ok(depot) => depot,
            Err(error) => panic!("{error}"),
        }
    }

    /// `Depot.__init__`, with the `ValueError` as an `Err`.
    pub fn try_new(zones: usize, couriers: usize, seed: u64) -> Result<Depot, ValueError> {
        if zones < 3 {
            return Err(ValueError("at least three zones"));
        }
        let zone_states: Vec<Rc<ZoneState>> = (0..zones).map(Zone::new_state).collect();
        for zone in &zone_states {
            // `(zone.id - 1) % zones` is never negative in Python.
            for other in [
                (zone.id + 1) % zones,
                (zone.id + zones - 1) % zones,
                (zone.id * 2 + 1) % zones,
            ] {
                let link = &zone_states[other];
                let mut neighbours = zone.neighbours.borrow_mut();
                if !Rc::ptr_eq(link, zone) && !neighbours.iter().any(|n| Rc::ptr_eq(n, link)) {
                    neighbours.push(Rc::clone(link));
                }
            }
        }
        let depot = Depot {
            state: Rc::new(DepotState {
                rng: RefCell::new(Rng::new(seed)),
                clock: Cell::new(0),
                zones: zone_states,
                routes: RefCell::new(Vec::new()),
                couriers: RefCell::new(Vec::new()),
                parcels: RefCell::new(BTreeMap::new()),
                listeners: RefCell::new(Vec::new()),
                next_parcel: Cell::new(0),
                next_courier: Cell::new(0),
                vacancies: Cell::new(0),
                arrivals: 1 + (couriers / 3) as u64,
                delivered_count: Cell::new(0),
                expired_count: Cell::new(0),
                retired_count: Cell::new(0),
                latency_total: Cell::new(0),
                handovers: Cell::new(0),
            }),
        };
        let routes = (0..zones)
            .map(|zid| depot.first_steps(&depot.zone(zid)))
            .collect();
        *depot.state.routes.borrow_mut() = routes;
        for _ in 0..couriers {
            depot.hire();
        }
        Ok(depot)
    }

    /// The `self.depot` that a parcel, a zone or a courier keeps: another
    /// reference to this same depot. Not for callers, who have one `Depot`.
    fn alias(&self) -> Depot {
        Depot {
            state: Rc::clone(&self.state),
        }
    }

    /// For every zone, the neighbour of `start` to go to first on a
    /// shortest way there. Roads are one-way where the express link is.
    fn first_steps(&self, start: &Zone) -> Vec<Option<usize>> {
        let mut first: Vec<Option<usize>> = vec![None; self.state.zones.len()];
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
        let step = self.state.routes.borrow()[here.id()][goal.id()]
            .expect("list indices must be integers, not NoneType");
        self.zone(step)
    }

    fn hire(&self) -> Courier {
        let count = self.state.zones.len() as u64;
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

    /// The courier with this number, or `None` if there is none at work.
    pub fn courier(&self, cid: u64) -> Option<Courier> {
        self.couriers().into_iter().find(|c| c.id() == cid)
    }

    pub fn zone(&self, zid: usize) -> Zone {
        Zone::wrap(self, &self.state.zones[zid])
    }

    /// `list(depot.zones)`
    pub fn zones(&self) -> Vec<Zone> {
        (0..self.state.zones.len())
            .map(|zid| self.zone(zid))
            .collect()
    }

    /// `list(depot.couriers)`
    pub fn couriers(&self) -> Vec<Courier> {
        let couriers = self.state.couriers.borrow();
        couriers
            .iter()
            .map(|state| Courier::wrap(self, state))
            .collect()
    }

    pub fn clock(&self) -> u64 {
        self.state.clock.get()
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
    pub fn on_delivered<F>(&mut self, listener: F)
    where
        F: FnMut(&Parcel, &Courier) + 'static,
    {
        self.state.listeners.borrow_mut().push(Box::new(listener));
    }

    fn record_delivery(&self, parcel: &Parcel, courier: &Courier) {
        parcel.close(Status::Delivered);
        self.forget(parcel);
        let state = &self.state;
        state.delivered_count.set(state.delivered_count.get() + 1);
        state
            .latency_total
            .set(state.latency_total.get() + parcel.age());
        // A listener has the parcel and the courier but no way to the
        // depot, so the list cannot change under this loop.
        for listener in state.listeners.borrow_mut().iter_mut() {
            listener(parcel, courier);
        }
    }

    fn expire(&self, parcel: &Parcel) {
        parcel.close(Status::Expired);
        self.forget(parcel);
        self.state
            .expired_count
            .set(self.state.expired_count.get() + 1);
    }

    /// `del self.parcels[parcel.id]`
    fn forget(&self, parcel: &Parcel) {
        self.state
            .parcels
            .borrow_mut()
            .remove(&parcel.id())
            .expect("KeyError: no such parcel");
    }

    /// The courier leaves; what was in the bag waits where they stood.
    fn retire(&self, courier: &Courier) {
        courier.state.retired.set(true);
        remove_first(&self.state.couriers, &courier.state);
        let zone = courier.zone();
        remove_first(&zone.state.couriers, &courier.state);
        for parcel in courier.bag() {
            zone.post(&parcel);
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
        let count = self.state.zones.len() as u64;
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
        fn briefs(parcels: &[Parcel]) -> String {
            let briefs: Vec<String> = parcels.iter().map(Parcel::brief).collect();
            briefs.join(",")
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
            let couriers: Vec<String> = zone
                .couriers()
                .iter()
                .map(|courier| courier.id().to_string())
                .collect();
            lines.push(format!(
                "zone {} {} {} w {} c {}",
                zone.id(),
                zone.delivered(),
                zone.congestion(),
                briefs(&zone.waiting_parcels()),
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
                briefs(&courier.bag()),
            ));
        }
        lines.join("\n") + "\n"
    }
}

/// A run with nobody watching: the state after `steps` steps.
pub fn run(zones: usize, couriers: usize, seed: u64, steps: u64) -> String {
    let mut depot = Depot::new(zones, couriers, seed);
    for _ in 0..steps {
        depot.step();
    }
    depot.report()
}
