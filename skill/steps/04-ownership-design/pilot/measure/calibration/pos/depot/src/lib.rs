//! Calibration port POS: `depot.py` carried over object for object, in the
//! shape such a port takes once the borrow checker has been satisfied one
//! error at a time. Every part is its own `Rc<RefCell<..>>`, parts hold each
//! other as the Python objects do, and wherever a list is walked while
//! something in it may change, the list is cloned first. Written by hand to
//! try the pilot's rule on; it is not shown to any agent.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt::Write;
use std::rc::{Rc, Weak};

const PATIENCE: u64 = 12;
const EXPIRY: u64 = 40;
const FATIGUE_LIMIT: u64 = 25;
const REST: u64 = 3;
const QUOTA: u64 = 30;

type Listener = Box<dyn FnMut(&Parcel, &Courier)>;

struct ParcelData {
    depot: Weak<RefCell<DepotData>>,
    id: usize,
    dest: Zone,
    weight: u64,
    born: u64,
    carrier: Option<Courier>,
    status: &'static str,
    hops: u64,
    closed_at: Option<u64>,
}

struct ZoneData {
    depot: Weak<RefCell<DepotData>>,
    id: usize,
    neighbours: Vec<Zone>,
    waiting: Vec<Parcel>,
    couriers: Vec<Courier>,
    delivered: u64,
    congestion: u64,
}

struct CourierData {
    depot: Weak<RefCell<DepotData>>,
    id: usize,
    zone: Zone,
    capacity: u64,
    quota: u64,
    bag: Vec<Parcel>,
    fatigue: u64,
    resting: u64,
    delivered: u64,
    retired: bool,
}

struct DepotData {
    rng: u64,
    clock: u64,
    zones: Vec<Zone>,
    routes: Vec<Vec<usize>>,
    couriers: Vec<Courier>,
    parcels: BTreeMap<usize, Parcel>,
    listeners: Vec<Listener>,
    next_parcel: usize,
    next_courier: usize,
    vacancies: usize,
    arrivals: usize,
    delivered_count: u64,
    expired_count: u64,
    retired_count: u64,
    latency_total: u64,
    handovers: u64,
}

/// Something to be carried from one zone to another.
#[derive(Clone)]
pub struct Parcel(Rc<RefCell<ParcelData>>);

/// A place where parcels wait and couriers pass through.
#[derive(Clone)]
pub struct Zone(Rc<RefCell<ZoneData>>);

/// Carries parcels, one zone per step.
#[derive(Clone)]
pub struct Courier(Rc<RefCell<CourierData>>);

/// The whole simulation.
#[derive(Clone)]
pub struct Depot(Rc<RefCell<DepotData>>);

impl Parcel {
    fn depot(&self) -> Depot {
        Depot(self.0.borrow().depot.upgrade().expect("the depot is gone"))
    }

    pub fn id(&self) -> usize {
        self.0.borrow().id
    }

    pub fn status(&self) -> &'static str {
        self.0.borrow().status
    }

    pub fn carrier(&self) -> Option<Courier> {
        self.0.borrow().carrier.clone()
    }

    pub fn age(&self) -> u64 {
        let closed_at = self.0.borrow().closed_at;
        let end = match closed_at {
            Some(end) => end,
            None => self.depot().0.borrow().clock,
        };
        end - self.0.borrow().born
    }

    fn urgent(&self) -> bool {
        self.0.borrow().closed_at.is_none() && self.age() > PATIENCE
    }

    fn close(&self, status: &'static str) {
        let clock = self.depot().0.borrow().clock;
        let mut this = self.0.borrow_mut();
        this.status = status;
        this.closed_at = Some(clock);
        this.carrier = None;
    }
}

impl Zone {
    fn depot(&self) -> Depot {
        Depot(self.0.borrow().depot.upgrade().expect("the depot is gone"))
    }

    pub fn id(&self) -> usize {
        self.0.borrow().id
    }

    pub fn waiting(&self) -> usize {
        self.0.borrow().waiting.len()
    }

    pub fn congestion(&self) -> u64 {
        self.0.borrow().congestion
    }

    fn post(&self, parcel: &Parcel) {
        {
            let mut p = parcel.0.borrow_mut();
            p.status = "waiting";
            p.carrier = None;
        }
        self.0.borrow_mut().waiting.push(parcel.clone());
    }

    fn offer(&self, courier: &Courier) -> Vec<Parcel> {
        let mut room = courier.capacity() as i64 - courier.load() as i64;
        let mut taken: Vec<Parcel> = Vec::new();
        let waiting = self.0.borrow().waiting.clone();
        for urgent in [true, false] {
            for parcel in &waiting {
                let weight = parcel.0.borrow().weight as i64;
                if parcel.urgent() == urgent && weight <= room {
                    taken.push(parcel.clone());
                    room -= weight;
                }
            }
        }
        for parcel in &taken {
            let mut this = self.0.borrow_mut();
            if let Some(at) = this
                .waiting
                .iter()
                .position(|p| Rc::ptr_eq(&p.0, &parcel.0))
            {
                this.waiting.remove(at);
            }
        }
        taken
    }

    fn busiest_neighbour(&self) -> Zone {
        let neighbours = self.0.borrow().neighbours.clone();
        let mut best = neighbours[0].clone();
        for zone in &neighbours[1..] {
            if zone.congestion() > best.congestion() {
                best = zone.clone();
            }
        }
        best
    }

    fn settle(&self) {
        let waiting = self.0.borrow().waiting.clone();
        for parcel in &waiting {
            if parcel.age() > EXPIRY {
                {
                    let mut this = self.0.borrow_mut();
                    if let Some(at) = this
                        .waiting
                        .iter()
                        .position(|p| Rc::ptr_eq(&p.0, &parcel.0))
                    {
                        this.waiting.remove(at);
                    }
                }
                self.depot().expire(parcel);
            }
        }
        let mut this = self.0.borrow_mut();
        let weight: u64 = this.waiting.iter().map(|p| p.0.borrow().weight).sum();
        this.congestion = (this.congestion * 3 + weight * 4) / 4;
    }
}

impl Courier {
    fn depot(&self) -> Depot {
        Depot(self.0.borrow().depot.upgrade().expect("the depot is gone"))
    }

    pub fn id(&self) -> usize {
        self.0.borrow().id
    }

    pub fn capacity(&self) -> u64 {
        self.0.borrow().capacity
    }

    pub fn set_capacity(&self, capacity: u64) {
        self.0.borrow_mut().capacity = capacity;
    }

    pub fn retired(&self) -> bool {
        self.0.borrow().retired
    }

    pub fn load(&self) -> u64 {
        self.0
            .borrow()
            .bag
            .iter()
            .map(|p| p.0.borrow().weight)
            .sum()
    }

    fn heading(&self) -> Option<Zone> {
        self.0
            .borrow()
            .bag
            .first()
            .map(|p| p.0.borrow().dest.clone())
    }

    pub fn rest(&self, steps: u64) {
        let mut this = self.0.borrow_mut();
        this.resting = this.resting.max(steps);
    }

    fn act(&self) {
        if self.0.borrow().resting > 0 {
            self.0.borrow_mut().resting -= 1;
            return;
        }
        self.drop_off();
        self.hand_over();
        self.pick_up();
        self.travel();
        self.tire();
    }

    fn remove_from_bag(&self, parcel: &Parcel) {
        let mut this = self.0.borrow_mut();
        if let Some(at) = this.bag.iter().position(|p| Rc::ptr_eq(&p.0, &parcel.0)) {
            this.bag.remove(at);
        }
    }

    fn drop_off(&self) {
        let bag = self.0.borrow().bag.clone();
        for parcel in &bag {
            let dest = parcel.0.borrow().dest.clone();
            let zone = self.0.borrow().zone.clone();
            if Rc::ptr_eq(&dest.0, &zone.0) {
                self.remove_from_bag(parcel);
                zone.0.borrow_mut().delivered += 1;
                self.0.borrow_mut().delivered += 1;
                self.depot().record_delivery(parcel, self);
            }
        }
    }

    fn hand_over(&self) {
        let mine = self.heading();
        let zone = self.0.borrow().zone.clone();
        let peers = zone.0.borrow().couriers.clone();
        for peer in &peers {
            if Rc::ptr_eq(&peer.0, &self.0) || peer.0.borrow().resting > 0 {
                continue;
            }
            let Some(theirs) = peer.heading() else {
                continue;
            };
            if mine.as_ref().is_some_and(|m| Rc::ptr_eq(&m.0, &theirs.0)) {
                continue;
            }
            let bag = self.0.borrow().bag.clone();
            for parcel in &bag {
                let dest = parcel.0.borrow().dest.clone();
                let weight = parcel.0.borrow().weight;
                if Rc::ptr_eq(&dest.0, &theirs.0) && peer.load() + weight <= peer.capacity() {
                    self.remove_from_bag(parcel);
                    peer.0.borrow_mut().bag.push(parcel.clone());
                    {
                        let mut p = parcel.0.borrow_mut();
                        p.carrier = Some(peer.clone());
                        p.hops += 1;
                    }
                    self.depot().0.borrow_mut().handovers += 1;
                }
            }
        }
    }

    fn pick_up(&self) {
        let zone = self.0.borrow().zone.clone();
        for parcel in zone.offer(self) {
            self.0.borrow_mut().bag.push(parcel.clone());
            let mut p = parcel.0.borrow_mut();
            p.carrier = Some(self.clone());
            p.status = "carried";
        }
    }

    fn travel(&self) {
        let here = self.0.borrow().zone.clone();
        let there = match self.heading() {
            Some(goal) => self.depot().route(&here, &goal),
            None => {
                let there = here.busiest_neighbour();
                if there.congestion() <= here.congestion() {
                    return;
                }
                there
            }
        };
        {
            let mut h = here.0.borrow_mut();
            if let Some(at) = h.couriers.iter().position(|c| Rc::ptr_eq(&c.0, &self.0)) {
                h.couriers.remove(at);
            }
        }
        there.0.borrow_mut().couriers.push(self.clone());
        let load = self.load();
        {
            let mut this = self.0.borrow_mut();
            this.zone = there;
            this.fatigue += 1 + load / 8;
        }
        let bag = self.0.borrow().bag.clone();
        for parcel in &bag {
            parcel.0.borrow_mut().hops += 1;
        }
    }

    fn tire(&self) {
        {
            let mut this = self.0.borrow_mut();
            if this.fatigue >= FATIGUE_LIMIT {
                this.fatigue = 0;
                this.resting = REST;
            }
        }
        let done = self.0.borrow().delivered >= self.0.borrow().quota;
        if done {
            self.depot().retire(self);
        }
    }
}

impl Depot {
    /// Panics with fewer than three zones.
    pub fn new(zones: usize, couriers: usize, seed: u64) -> Depot {
        assert!(zones >= 3, "at least three zones");
        let depot = Depot(Rc::new(RefCell::new(DepotData {
            rng: seed,
            clock: 0,
            zones: Vec::new(),
            routes: Vec::new(),
            couriers: Vec::new(),
            parcels: BTreeMap::new(),
            listeners: Vec::new(),
            next_parcel: 0,
            next_courier: 0,
            vacancies: 0,
            arrivals: 1 + couriers / 3,
            delivered_count: 0,
            expired_count: 0,
            retired_count: 0,
            latency_total: 0,
            handovers: 0,
        })));
        let places: Vec<Zone> = (0..zones)
            .map(|id| {
                Zone(Rc::new(RefCell::new(ZoneData {
                    depot: Rc::downgrade(&depot.0),
                    id,
                    neighbours: Vec::new(),
                    waiting: Vec::new(),
                    couriers: Vec::new(),
                    delivered: 0,
                    congestion: 0,
                })))
            })
            .collect();
        for zone in &places {
            let id = zone.id();
            for other in [
                (id + 1) % zones,
                (id + zones - 1) % zones,
                (id * 2 + 1) % zones,
            ] {
                let link = places[other].clone();
                let known = zone
                    .0
                    .borrow()
                    .neighbours
                    .iter()
                    .any(|n| Rc::ptr_eq(&n.0, &link.0));
                if !Rc::ptr_eq(&link.0, &zone.0) && !known {
                    zone.0.borrow_mut().neighbours.push(link);
                }
            }
        }
        depot.0.borrow_mut().zones = places.clone();
        let routes: Vec<Vec<usize>> = places.iter().map(|z| depot.first_steps(z)).collect();
        depot.0.borrow_mut().routes = routes;
        for _ in 0..couriers {
            depot.hire();
        }
        depot
    }

    fn below(&self, n: usize) -> usize {
        let mut this = self.0.borrow_mut();
        this.rng = this
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((this.rng >> 33) % n as u64) as usize
    }

    fn first_steps(&self, start: &Zone) -> Vec<usize> {
        let count = self.0.borrow().zones.len();
        let mut first: Vec<Option<usize>> = vec![None; count];
        first[start.id()] = Some(start.id());
        let mut frontier: Vec<Zone> = Vec::new();
        for zone in start.0.borrow().neighbours.iter() {
            if first[zone.id()].is_none() {
                first[zone.id()] = Some(zone.id());
                frontier.push(zone.clone());
            }
        }
        while !frontier.is_empty() {
            let mut reached: Vec<Zone> = Vec::new();
            for zone in &frontier {
                for link in zone.0.borrow().neighbours.iter() {
                    if first[link.id()].is_none() {
                        first[link.id()] = first[zone.id()];
                        reached.push(link.clone());
                    }
                }
            }
            frontier = reached;
        }
        first
            .into_iter()
            .map(|f| f.expect("a zone out of reach"))
            .collect()
    }

    fn route(&self, here: &Zone, goal: &Zone) -> Zone {
        let this = self.0.borrow();
        this.zones[this.routes[here.id()][goal.id()]].clone()
    }

    fn hire(&self) -> Courier {
        let count = self.0.borrow().zones.len();
        let at = self.below(count);
        let zone = self.0.borrow().zones[at].clone();
        let capacity = 4 + self.below(5) as u64;
        let id = self.0.borrow().next_courier;
        let courier = Courier(Rc::new(RefCell::new(CourierData {
            depot: Rc::downgrade(&self.0),
            id,
            zone: zone.clone(),
            capacity,
            quota: QUOTA + id as u64 % 7,
            bag: Vec::new(),
            fatigue: 0,
            resting: 0,
            delivered: 0,
            retired: false,
        })));
        {
            let mut this = self.0.borrow_mut();
            this.next_courier += 1;
            this.couriers.push(courier.clone());
        }
        zone.0.borrow_mut().couriers.push(courier.clone());
        courier
    }

    /// The courier with this number, if at work.
    pub fn courier(&self, id: usize) -> Option<Courier> {
        self.0
            .borrow()
            .couriers
            .iter()
            .find(|c| c.id() == id)
            .cloned()
    }

    pub fn zone(&self, id: usize) -> Zone {
        self.0.borrow().zones[id].clone()
    }

    pub fn submit(&mut self, origin: usize, dest: usize, weight: u64) -> Parcel {
        let origin = self.zone(origin);
        let dest = self.zone(dest);
        let (id, born) = {
            let mut this = self.0.borrow_mut();
            this.next_parcel += 1;
            (this.next_parcel - 1, this.clock)
        };
        let parcel = Parcel(Rc::new(RefCell::new(ParcelData {
            depot: Rc::downgrade(&self.0),
            id,
            dest,
            weight,
            born,
            carrier: None,
            status: "waiting",
            hops: 0,
            closed_at: None,
        })));
        self.0.borrow_mut().parcels.insert(id, parcel.clone());
        origin.post(&parcel);
        parcel
    }

    pub fn on_delivered(&mut self, listener: impl FnMut(&Parcel, &Courier) + 'static) {
        self.0.borrow_mut().listeners.push(Box::new(listener));
    }

    fn record_delivery(&self, parcel: &Parcel, courier: &Courier) {
        parcel.close("delivered");
        let age = parcel.age();
        {
            let mut this = self.0.borrow_mut();
            this.parcels.remove(&parcel.id());
            this.delivered_count += 1;
            this.latency_total += age;
        }
        // A listener reads the depot through what it is given, so the
        // depot cannot stay borrowed while one runs.
        let mut listeners = std::mem::take(&mut self.0.borrow_mut().listeners);
        for listener in &mut listeners {
            listener(parcel, courier);
        }
        self.0.borrow_mut().listeners = listeners;
    }

    fn expire(&self, parcel: &Parcel) {
        parcel.close("expired");
        let mut this = self.0.borrow_mut();
        this.parcels.remove(&parcel.id());
        this.expired_count += 1;
    }

    fn retire(&self, courier: &Courier) {
        courier.0.borrow_mut().retired = true;
        {
            let mut this = self.0.borrow_mut();
            if let Some(at) = this
                .couriers
                .iter()
                .position(|c| Rc::ptr_eq(&c.0, &courier.0))
            {
                this.couriers.remove(at);
            }
        }
        let zone = courier.0.borrow().zone.clone();
        {
            let mut z = zone.0.borrow_mut();
            if let Some(at) = z.couriers.iter().position(|c| Rc::ptr_eq(&c.0, &courier.0)) {
                z.couriers.remove(at);
            }
        }
        let bag = courier.0.borrow().bag.clone();
        for parcel in &bag {
            zone.post(parcel);
        }
        courier.0.borrow_mut().bag = Vec::new();
        let mut this = self.0.borrow_mut();
        this.retired_count += 1;
        this.vacancies += 1;
    }

    pub fn step(&mut self) {
        self.0.borrow_mut().clock += 1;
        let vacancies = self.0.borrow().vacancies;
        for _ in 0..vacancies {
            self.hire();
        }
        self.0.borrow_mut().vacancies = 0;
        let count = self.0.borrow().zones.len();
        let arrivals = self.0.borrow().arrivals;
        for _ in 0..arrivals {
            let origin = self.below(count);
            let mut dest = self.below(count - 1);
            if dest >= origin {
                dest += 1;
            }
            let weight = 1 + self.below(4) as u64;
            self.submit(origin, dest, weight);
        }
        let couriers = self.0.borrow().couriers.clone();
        for courier in &couriers {
            courier.act();
        }
        let zones = self.0.borrow().zones.clone();
        for zone in &zones {
            zone.settle();
        }
    }

    pub fn report(&self) -> String {
        let this = self.0.borrow();
        let mut out = String::new();
        let _ = writeln!(
            out,
            "depot {} {} {} {} {} {} {} {} {} {}",
            this.clock,
            this.rng,
            this.next_parcel,
            this.next_courier,
            this.parcels.len(),
            this.delivered_count,
            this.expired_count,
            this.retired_count,
            this.latency_total,
            this.handovers
        );
        let bag = |parcels: &[Parcel]| {
            parcels
                .iter()
                .map(|p| format!("{}:{}", p.id(), p.0.borrow().hops))
                .collect::<Vec<String>>()
                .join(",")
        };
        for zone in &this.zones {
            let z = zone.0.borrow();
            let couriers = z
                .couriers
                .iter()
                .map(|c| c.id().to_string())
                .collect::<Vec<String>>()
                .join(",");
            let _ = writeln!(
                out,
                "zone {} {} {} w {} c {}",
                z.id,
                z.delivered,
                z.congestion,
                bag(&z.waiting),
                couriers
            );
        }
        for courier in &this.couriers {
            let c = courier.0.borrow();
            let _ = writeln!(
                out,
                "courier {} {} {} {} {} {} b {}",
                c.id,
                c.zone.id(),
                c.capacity,
                c.fatigue,
                c.resting,
                c.delivered,
                bag(&c.bag)
            );
        }
        out
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
