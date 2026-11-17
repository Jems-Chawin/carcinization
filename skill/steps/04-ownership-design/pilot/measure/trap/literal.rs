//! The trap check: the busiest part of `depot.py` written in Rust line for
//! line, each class a struct that owns what the Python object holds and each
//! method taking what the Python method reaches. It is not meant to compile.
//! `errors.txt` is what rustc 1.100.0 says about it.
//!
//! Left out because they add nothing to the question: hiring, routes,
//! urgency, fatigue and the report.

pub struct Parcel {
    pub id: usize,
    pub dest: usize,
    pub weight: u64,
    pub hops: u64,
    pub carrier: Option<usize>,
}

pub struct Zone {
    pub id: usize,
    pub waiting: Vec<Parcel>,
    pub couriers: Vec<Courier>,
    pub delivered: u64,
}

pub struct Courier {
    pub id: usize,
    pub capacity: u64,
    pub bag: Vec<Parcel>,
    pub delivered: u64,
    pub quota: u64,
}

pub struct Depot {
    pub zones: Vec<Zone>,
    pub listeners: Vec<Box<dyn FnMut(&Parcel, &Courier)>>,
    pub delivered_count: u64,
    pub handovers: u64,
    pub vacancies: u64,
}

impl Zone {
    /// `Zone.offer`
    pub fn offer(&mut self, courier: &Courier) -> Vec<Parcel> {
        let mut room = courier.capacity - courier.load();
        let mut taken = Vec::new();
        for parcel in &self.waiting {
            if parcel.weight <= room {
                room -= parcel.weight;
                // `taken.append(parcel)` then `self.waiting.remove(parcel)`
                let at = self.waiting.iter().position(|p| p.id == parcel.id).unwrap();
                taken.push(self.waiting.remove(at));
            }
        }
        taken
    }
}

impl Courier {
    pub fn load(&self) -> u64 {
        self.bag.iter().map(|p| p.weight).sum()
    }

    /// `Courier.act`: the courier reaches its zone and the depot.
    pub fn act(&mut self, zone: &mut Zone, depot: &mut Depot) {
        // drop_off
        for parcel in &self.bag {
            if parcel.dest == zone.id {
                let at = self.bag.iter().position(|p| p.id == parcel.id).unwrap();
                let parcel = self.bag.remove(at);
                zone.delivered += 1;
                self.delivered += 1;
                depot.record_delivery(&parcel, self);
            }
        }
        // hand_over
        for peer in &mut zone.couriers {
            for parcel in &self.bag {
                if peer.load() + parcel.weight <= peer.capacity {
                    let at = self.bag.iter().position(|p| p.id == parcel.id).unwrap();
                    peer.bag.push(self.bag.remove(at));
                    depot.handovers += 1;
                }
            }
        }
        // pick_up
        for mut parcel in zone.offer(self) {
            parcel.carrier = Some(self.id);
            self.bag.push(parcel);
        }
        // tire
        if self.delivered >= self.quota {
            depot.retire(self, zone);
        }
    }
}

impl Depot {
    /// `Depot.record_delivery`
    pub fn record_delivery(&mut self, parcel: &Parcel, courier: &Courier) {
        self.delivered_count += 1;
        for listener in &mut self.listeners {
            listener(parcel, courier);
        }
    }

    /// `Depot.retire`
    pub fn retire(&mut self, courier: &mut Courier, zone: &mut Zone) {
        let at = zone.couriers.iter().position(|c| c.id == courier.id).unwrap();
        zone.couriers.remove(at);
        zone.waiting.append(&mut courier.bag);
        self.vacancies += 1;
    }

    /// `Depot.step`
    pub fn step(&mut self) {
        for zone in &mut self.zones {
            for courier in &mut zone.couriers {
                courier.act(zone, self);
            }
        }
    }
}
