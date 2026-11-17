//! Calibration port KEPT: the reference port with the five list copies the
//! Python itself makes put back as copies: `list(self.couriers)` in
//! `Depot.step`, `list(self.bag)` in `Courier.drop_off` and in
//! `Courier.hand_over`, `list(self.waiting)` in `Zone.settle`, and the new
//! list `Zone.offer` returns. Nothing else is changed: one owner, parts as
//! records referred to by number. Written to see how the pilot's rule
//! judges copies kept from the original; it is not shown to any agent.

use std::cell::RefCell;
use std::fmt::Write;
use std::rc::Rc;

const PATIENCE: u64 = 12;
const EXPIRY: u64 = 40;
const FATIGUE_LIMIT: u64 = 25;
const REST: u64 = 3;
const QUOTA: u64 = 30;
const OPEN: u64 = u64::MAX;
const NOBODY: usize = usize::MAX;

#[derive(Clone, Copy, PartialEq)]
enum Status {
    Waiting,
    Carried,
    Delivered,
    Expired,
}

struct ParcelRec {
    dest: usize,
    weight: u64,
    born: u64,
    closed_at: u64,
    carrier: usize,
    status: Status,
    hops: u64,
}

struct ZoneRec {
    neighbours: Vec<usize>,
    waiting: Vec<usize>,
    couriers: Vec<usize>,
    delivered: u64,
    congestion: u64,
}

struct CourierRec {
    zone: usize,
    capacity: u64,
    quota: u64,
    bag: Vec<usize>,
    fatigue: u64,
    resting: u64,
    delivered: u64,
    retired: bool,
}

struct World {
    rng: u64,
    clock: u64,
    zones: Vec<ZoneRec>,
    /// `routes[here * zones + goal]`: the zone to go to first.
    routes: Vec<usize>,
    /// Every courier ever hired, by number.
    couriers: Vec<CourierRec>,
    /// The numbers of those at work, in the order of hiring.
    at_work: Vec<usize>,
    /// Every parcel ever made, by number.
    parcels: Vec<ParcelRec>,
    open: usize,
    vacancies: usize,
    arrivals: usize,
    delivered_count: u64,
    expired_count: u64,
    retired_count: u64,
    latency_total: u64,
    handovers: u64,
}

fn load(bag: &[usize], parcels: &[ParcelRec]) -> u64 {
    bag.iter().map(|&p| parcels[p].weight).sum()
}

impl World {
    fn below(&mut self, n: usize) -> usize {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.rng >> 33) % n as u64) as usize
    }

    fn age(&self, parcel: usize) -> u64 {
        let p = &self.parcels[parcel];
        let end = if p.closed_at == OPEN {
            self.clock
        } else {
            p.closed_at
        };
        end - p.born
    }

    fn heading(&self, courier: usize) -> Option<usize> {
        self.couriers[courier]
            .bag
            .first()
            .map(|&p| self.parcels[p].dest)
    }

    fn hire(&mut self) {
        let zone = self.below(self.zones.len());
        let capacity = 4 + self.below(5) as u64;
        let id = self.couriers.len();
        self.couriers.push(CourierRec {
            zone,
            capacity,
            quota: QUOTA + id as u64 % 7,
            bag: Vec::new(),
            fatigue: 0,
            resting: 0,
            delivered: 0,
            retired: false,
        });
        self.at_work.push(id);
        self.zones[zone].couriers.push(id);
    }

    fn submit(&mut self, origin: usize, dest: usize, weight: u64) -> usize {
        let id = self.parcels.len();
        self.parcels.push(ParcelRec {
            dest,
            weight,
            born: self.clock,
            closed_at: OPEN,
            carrier: NOBODY,
            status: Status::Waiting,
            hops: 0,
        });
        self.open += 1;
        self.zones[origin].waiting.push(id);
        id
    }

    fn begin_step(&mut self) {
        self.clock += 1;
        for _ in 0..self.vacancies {
            self.hire();
        }
        self.vacancies = 0;
        let count = self.zones.len();
        for _ in 0..self.arrivals {
            let origin = self.below(count);
            let mut dest = self.below(count - 1);
            if dest >= origin {
                dest += 1;
            }
            let weight = 1 + self.below(4) as u64;
            self.submit(origin, dest, weight);
        }
    }

    /// False if the courier rests this step.
    fn start_act(&mut self, courier: usize) -> bool {
        let c = &mut self.couriers[courier];
        if c.resting > 0 {
            c.resting -= 1;
            return false;
        }
        true
    }

    /// Delivers `parcel` if it is for the zone the courier stands in.
    fn deliver(&mut self, courier: usize, parcel: usize) -> bool {
        let c = &mut self.couriers[courier];
        if self.parcels[parcel].dest != c.zone {
            return false;
        }
        if let Some(at) = c.bag.iter().position(|&p| p == parcel) {
            c.bag.remove(at);
        }
        c.delivered += 1;
        self.zones[c.zone].delivered += 1;
        let p = &mut self.parcels[parcel];
        p.status = Status::Delivered;
        p.closed_at = self.clock;
        p.carrier = NOBODY;
        self.open -= 1;
        self.delivered_count += 1;
        self.latency_total += self.clock - p.born;
        true
    }

    fn hand_over(&mut self, courier: usize) {
        let mine = self.heading(courier);
        let zone = self.couriers[courier].zone;
        for k in 0..self.zones[zone].couriers.len() {
            let peer = self.zones[zone].couriers[k];
            if peer == courier || self.couriers[peer].resting > 0 {
                continue;
            }
            let Some(theirs) = self.heading(peer) else {
                continue;
            };
            if Some(theirs) == mine {
                continue;
            }
            let Ok([me, them]) = self.couriers.get_disjoint_mut([courier, peer]) else {
                continue;
            };
            // `for parcel in list(self.bag)`
            let bag = me.bag.clone();
            for parcel in bag {
                let p = &self.parcels[parcel];
                let (dest, weight) = (p.dest, p.weight);
                if dest == theirs && load(&them.bag, &self.parcels) + weight <= them.capacity {
                    if let Some(at) = me.bag.iter().position(|&x| x == parcel) {
                        me.bag.remove(at);
                    }
                    them.bag.push(parcel);
                    let p = &mut self.parcels[parcel];
                    p.carrier = peer;
                    p.hops += 1;
                    self.handovers += 1;
                }
            }
        }
    }

    fn pick_up(&mut self, courier: usize) {
        let World {
            couriers,
            zones,
            parcels,
            clock,
            ..
        } = self;
        let c = &mut couriers[courier];
        let waiting = &mut zones[c.zone].waiting;
        // A caller may have set the capacity below what is carried.
        let mut room = c.capacity as i64 - load(&c.bag, parcels) as i64;
        // `taken = []`: a new list at every call, as `Zone.offer` returns one.
        let mut taken: Vec<usize> = Vec::new();
        for urgent in [true, false] {
            for &parcel in waiting.iter() {
                let p = &parcels[parcel];
                if (*clock - p.born > PATIENCE) == urgent && p.weight as i64 <= room {
                    taken.push(parcel);
                    room -= p.weight as i64;
                }
            }
        }
        if taken.is_empty() {
            return;
        }
        waiting.retain(|parcel| !taken.contains(parcel));
        for &parcel in taken.iter() {
            c.bag.push(parcel);
            parcels[parcel].carrier = courier;
            parcels[parcel].status = Status::Carried;
        }
    }

    fn busiest_neighbour(&self, zone: usize) -> usize {
        let mut best = self.zones[zone].neighbours[0];
        for &other in &self.zones[zone].neighbours[1..] {
            if self.zones[other].congestion > self.zones[best].congestion {
                best = other;
            }
        }
        best
    }

    fn travel(&mut self, courier: usize) {
        let here = self.couriers[courier].zone;
        let there = match self.heading(courier) {
            Some(goal) => self.routes[here * self.zones.len() + goal],
            None => {
                let there = self.busiest_neighbour(here);
                if self.zones[there].congestion <= self.zones[here].congestion {
                    return;
                }
                there
            }
        };
        let present = &mut self.zones[here].couriers;
        if let Some(at) = present.iter().position(|&c| c == courier) {
            present.remove(at);
        }
        self.zones[there].couriers.push(courier);
        let c = &mut self.couriers[courier];
        c.zone = there;
        c.fatigue += 1 + load(&c.bag, &self.parcels) / 8;
        for &parcel in &c.bag {
            self.parcels[parcel].hops += 1;
        }
    }

    /// True if the courier retired.
    fn tire(&mut self, courier: usize) -> bool {
        let c = &mut self.couriers[courier];
        if c.fatigue >= FATIGUE_LIMIT {
            c.fatigue = 0;
            c.resting = REST;
        }
        if c.delivered < c.quota {
            return false;
        }
        c.retired = true;
        if let Some(at) = self.at_work.iter().position(|&x| x == courier) {
            self.at_work.remove(at);
        }
        let zone = &mut self.zones[c.zone];
        if let Some(at) = zone.couriers.iter().position(|&x| x == courier) {
            zone.couriers.remove(at);
        }
        for &parcel in &c.bag {
            let p = &mut self.parcels[parcel];
            p.status = Status::Waiting;
            p.carrier = NOBODY;
            zone.waiting.push(parcel);
        }
        c.bag.clear();
        self.retired_count += 1;
        self.vacancies += 1;
        true
    }

    /// The rest of a courier's step after the deliveries. True if retired.
    fn finish_act(&mut self, courier: usize) -> bool {
        self.hand_over(courier);
        self.pick_up(courier);
        self.travel(courier);
        self.tire(courier)
    }

    fn settle(&mut self) {
        for zone in &mut self.zones {
            // `for parcel in list(self.waiting)`
            let waiting = zone.waiting.clone();
            for parcel in waiting {
                let p = &mut self.parcels[parcel];
                if self.clock - p.born > EXPIRY {
                    if let Some(at) = zone.waiting.iter().position(|&x| x == parcel) {
                        zone.waiting.remove(at);
                    }
                    p.status = Status::Expired;
                    p.closed_at = self.clock;
                    p.carrier = NOBODY;
                    self.open -= 1;
                    self.expired_count += 1;
                }
            }
            let weight = load(&zone.waiting, &self.parcels);
            zone.congestion = (zone.congestion * 3 + weight * 4) / 4;
        }
    }

    fn first_steps(zones: &[ZoneRec], start: usize, first: &mut [usize]) {
        first[start] = start;
        let mut frontier = Vec::new();
        for &zone in &zones[start].neighbours {
            if first[zone] == NOBODY {
                first[zone] = zone;
                frontier.push(zone);
            }
        }
        while !frontier.is_empty() {
            let mut reached = Vec::new();
            for &zone in &frontier {
                for &link in &zones[zone].neighbours {
                    if first[link] == NOBODY {
                        first[link] = first[zone];
                        reached.push(link);
                    }
                }
            }
            frontier = reached;
        }
    }

    fn report(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(
            out,
            "depot {} {} {} {} {} {} {} {} {} {}",
            self.clock,
            self.rng,
            self.parcels.len(),
            self.couriers.len(),
            self.open,
            self.delivered_count,
            self.expired_count,
            self.retired_count,
            self.latency_total,
            self.handovers
        );
        let bag = |out: &mut String, parcels: &[usize]| {
            for (i, &p) in parcels.iter().enumerate() {
                let comma = if i == 0 { "" } else { "," };
                let _ = write!(out, "{comma}{p}:{}", self.parcels[p].hops);
            }
        };
        for (id, zone) in self.zones.iter().enumerate() {
            let _ = write!(out, "zone {id} {} {} w ", zone.delivered, zone.congestion);
            bag(&mut out, &zone.waiting);
            out.push_str(" c ");
            for (i, c) in zone.couriers.iter().enumerate() {
                let comma = if i == 0 { "" } else { "," };
                let _ = write!(out, "{comma}{c}");
            }
            out.push('\n');
        }
        for &id in &self.at_work {
            let c = &self.couriers[id];
            let _ = write!(
                out,
                "courier {id} {} {} {} {} {} b ",
                c.zone, c.capacity, c.fatigue, c.resting, c.delivered
            );
            bag(&mut out, &c.bag);
            out.push('\n');
        }
        out
    }
}

type Listener = Box<dyn FnMut(&Parcel, &Courier)>;

/// The whole simulation.
pub struct Depot {
    world: Rc<RefCell<World>>,
    listeners: Vec<Listener>,
}

/// A courier, as a caller holds one.
#[derive(Clone)]
pub struct Courier {
    world: Rc<RefCell<World>>,
    id: usize,
}

/// A zone, as a caller holds one.
#[derive(Clone)]
pub struct Zone {
    world: Rc<RefCell<World>>,
    id: usize,
}

/// A parcel, as a caller holds one.
#[derive(Clone)]
pub struct Parcel {
    world: Rc<RefCell<World>>,
    id: usize,
}

impl Depot {
    /// Panics with fewer than three zones.
    pub fn new(zones: usize, couriers: usize, seed: u64) -> Depot {
        assert!(zones >= 3, "at least three zones");
        let mut places: Vec<ZoneRec> = Vec::with_capacity(zones);
        for id in 0..zones {
            let mut neighbours = Vec::new();
            for other in [
                (id + 1) % zones,
                (id + zones - 1) % zones,
                (id * 2 + 1) % zones,
            ] {
                if other != id && !neighbours.contains(&other) {
                    neighbours.push(other);
                }
            }
            places.push(ZoneRec {
                neighbours,
                waiting: Vec::new(),
                couriers: Vec::new(),
                delivered: 0,
                congestion: 0,
            });
        }
        let mut routes = vec![NOBODY; zones * zones];
        for start in 0..zones {
            World::first_steps(
                &places,
                start,
                &mut routes[start * zones..(start + 1) * zones],
            );
        }
        let mut world = World {
            rng: seed,
            clock: 0,
            zones: places,
            routes,
            couriers: Vec::new(),
            at_work: Vec::new(),
            parcels: Vec::new(),
            open: 0,
            vacancies: 0,
            arrivals: 1 + couriers / 3,
            delivered_count: 0,
            expired_count: 0,
            retired_count: 0,
            latency_total: 0,
            handovers: 0,
        };
        for _ in 0..couriers {
            world.hire();
        }
        Depot {
            world: Rc::new(RefCell::new(world)),
            listeners: Vec::new(),
        }
    }

    /// The courier with this number, if at work.
    pub fn courier(&self, id: usize) -> Option<Courier> {
        let world = self.world.borrow();
        let at_work = world.couriers.get(id).is_some_and(|c| !c.retired);
        at_work.then(|| Courier {
            world: self.world.clone(),
            id,
        })
    }

    pub fn zone(&self, id: usize) -> Zone {
        assert!(id < self.world.borrow().zones.len(), "no such zone");
        Zone {
            world: self.world.clone(),
            id,
        }
    }

    pub fn submit(&mut self, origin: usize, dest: usize, weight: u64) -> Parcel {
        let id = self.world.borrow_mut().submit(origin, dest, weight);
        Parcel {
            world: self.world.clone(),
            id,
        }
    }

    pub fn on_delivered(&mut self, listener: impl FnMut(&Parcel, &Courier) + 'static) {
        self.listeners.push(Box::new(listener));
    }

    pub fn step(&mut self) {
        self.world.borrow_mut().begin_step();
        // `for courier in list(self.couriers)`
        let couriers = self.world.borrow().at_work.clone();
        for courier in couriers {
            if !self.world.borrow_mut().start_act(courier) {
                continue;
            }
            // `for parcel in list(self.bag)`
            let bag = self.world.borrow().couriers[courier].bag.clone();
            for parcel in bag {
                // The borrow ends with this statement, before any listener runs.
                let delivered = self.world.borrow_mut().deliver(courier, parcel);
                if delivered && !self.listeners.is_empty() {
                    let parcel = Parcel {
                        world: self.world.clone(),
                        id: parcel,
                    };
                    let courier = Courier {
                        world: self.world.clone(),
                        id: courier,
                    };
                    for listener in &mut self.listeners {
                        listener(&parcel, &courier);
                    }
                }
            }
            self.world.borrow_mut().finish_act(courier);
        }
        self.world.borrow_mut().settle();
    }

    pub fn report(&self) -> String {
        self.world.borrow().report()
    }
}

impl Courier {
    pub fn id(&self) -> usize {
        self.id
    }

    pub fn load(&self) -> u64 {
        let world = self.world.borrow();
        load(&world.couriers[self.id].bag, &world.parcels)
    }

    pub fn capacity(&self) -> u64 {
        self.world.borrow().couriers[self.id].capacity
    }

    pub fn set_capacity(&self, capacity: u64) {
        self.world.borrow_mut().couriers[self.id].capacity = capacity;
    }

    pub fn rest(&self, steps: u64) {
        let mut world = self.world.borrow_mut();
        let c = &mut world.couriers[self.id];
        c.resting = c.resting.max(steps);
    }

    pub fn retired(&self) -> bool {
        self.world.borrow().couriers[self.id].retired
    }
}

impl Zone {
    pub fn id(&self) -> usize {
        self.id
    }

    pub fn waiting(&self) -> usize {
        self.world.borrow().zones[self.id].waiting.len()
    }

    pub fn congestion(&self) -> u64 {
        self.world.borrow().zones[self.id].congestion
    }
}

impl Parcel {
    pub fn id(&self) -> usize {
        self.id
    }

    pub fn status(&self) -> &'static str {
        match self.world.borrow().parcels[self.id].status {
            Status::Waiting => "waiting",
            Status::Carried => "carried",
            Status::Delivered => "delivered",
            Status::Expired => "expired",
        }
    }

    pub fn age(&self) -> u64 {
        self.world.borrow().age(self.id)
    }

    pub fn carrier(&self) -> Option<Courier> {
        let carrier = self.world.borrow().parcels[self.id].carrier;
        (carrier != NOBODY).then(|| Courier {
            world: self.world.clone(),
            id: carrier,
        })
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
