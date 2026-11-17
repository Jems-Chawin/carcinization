// A check on the experiment itself, not a port. This is the hot path of
// `relay.py` written in Rust the way the Python is written: the node's
// method is given the hub it belongs to. If this compiled, the program
// would hold no trap and the experiment could show nothing.
// `rustc --edition 2024 --crate-type lib literal.rs` must fail with borrow
// errors; `errors.txt` beside this file is what it printed.

pub struct Packet {
    pub dst: usize,
    pub ttl: u32,
    pub size: i64,
    pub born: i64,
}

pub struct Node {
    pub nid: usize,
    pub queue: Vec<Packet>,
    pub peers: Vec<usize>,
    pub load: i64,
    pub sent: i64,
}

pub struct Hub {
    pub nodes: Vec<Node>,
    pub delivered: Vec<i64>,
    pub traffic: Vec<i64>,
    pub dropped: i64,
    pub latency: i64,
    pub clock: i64,
}

impl Node {
    fn pick<'a>(&self, hub: &'a mut Hub, packet: &Packet) -> &'a mut Node {
        let mut best = self.peers[0];
        for &peer in &self.peers {
            if peer == packet.dst {
                return &mut hub.nodes[peer];
            }
            if hub.nodes[peer].load < hub.nodes[best].load {
                best = peer;
            }
        }
        &mut hub.nodes[best]
    }

    fn pump(&mut self, hub: &mut Hub) {
        let mut budget = 4;
        while budget > 0 && !self.queue.is_empty() {
            let mut packet = self.queue.remove(0);
            self.load -= packet.size;
            budget -= 1;
            if packet.dst == self.nid {
                hub.delivered[self.nid] += packet.size;
                hub.latency += hub.clock - packet.born;
                continue;
            }
            packet.ttl -= 1;
            if packet.ttl == 0 {
                hub.dropped += 1;
                continue;
            }
            let peer = self.pick(hub, &packet);
            let size = packet.size;
            peer.queue.push(packet);
            peer.load += size;
            self.sent += 1;
            hub.traffic[self.nid * hub.nodes.len() + peer.nid] += 1;
        }
    }
}

impl Hub {
    pub fn tick(&mut self) {
        self.clock += 1;
        for node in &mut self.nodes {
            node.pump(self);
        }
    }
}
