//! Port of `relay.py`. The public items below are the interface the rest of
//! the system uses: keep their names and signatures. Everything else is
//! yours: fields, private functions, more modules.
//!
//! Ownership is described in `DESIGN.md`: `Sim` owns everything in parallel
//! arrays indexed by node id; a node is an index, a peer is a node id, and a
//! packet is a small `Copy` value that lives in exactly one queue.

use std::collections::VecDeque;

#[cfg(test)]
mod alloc_test;

const BURST: usize = 4;
const INJECT: usize = 3;
const MAX_PEERS: usize = 3;

/// `Packet` of `relay.py`. Plain data, moved by value between queues.
#[derive(Clone, Copy)]
struct Packet {
    pid: u64,
    born: u64,
    dst: u32,
    ttl: u8,
    size: u8,
}

/// `Node.peers`: the ids of a node's neighbours, in the order the Python
/// built them.
#[derive(Clone, Copy)]
struct Links {
    ids: [u32; MAX_PEERS],
    len: u8,
}

impl Links {
    fn as_slice(&self) -> &[u32] {
        &self.ids[..usize::from(self.len)]
    }
}

/// A relay network. The Rust counterpart of `Hub` in `relay.py`.
pub struct Sim {
    rng: u64,
    clock: u64,
    next_pid: u64,
    dropped: u64,
    latency: u64,
    // Per node, read and written by every tick.
    queues: Vec<VecDeque<Packet>>,
    loads: Vec<u64>,
    sent: Vec<u64>,
    links: Vec<Links>,
    delivered: Vec<u64>,
    /// `count * count` counters, row = sender, column = receiver.
    traffic: Vec<u64>,
    // Per node, never touched by a tick.
    labels: Vec<String>,
}

/// `Node.pick`: the destination if it is a neighbour, otherwise the first
/// neighbour with the least load.
fn pick(links: Links, loads: &[u64], dst: u32) -> usize {
    let mut best = usize::MAX;
    let mut best_load = u64::MAX;
    for &peer in links.as_slice() {
        if peer == dst {
            return peer as usize;
        }
        let load = loads[peer as usize];
        if best == usize::MAX || load < best_load {
            best = peer as usize;
            best_load = load;
        }
    }
    best
}

impl Sim {
    /// `Hub(count, seed)`. `count` is at least 2.
    pub fn new(count: usize, seed: u64) -> Sim {
        assert!(count >= 2, "a network needs at least 2 nodes");
        assert!(u32::try_from(count).is_ok(), "too many nodes");
        let links = (0..count)
            .map(|nid| {
                let mut l = Links {
                    ids: [0; MAX_PEERS],
                    len: 0,
                };
                // u128 so that `nid * 7 + 3` cannot overflow for any usize.
                let chord = ((nid as u128 * 7 + 3) % count as u128) as usize;
                for other in [(nid + 1) % count, (nid + count - 1) % count, chord] {
                    let other = other as u32;
                    if other as usize != nid && !l.as_slice().contains(&other) {
                        l.ids[usize::from(l.len)] = other;
                        l.len += 1;
                    }
                }
                l
            })
            .collect();
        Sim {
            rng: seed,
            clock: 0,
            next_pid: 1,
            dropped: 0,
            latency: 0,
            queues: (0..count).map(|_| VecDeque::new()).collect(),
            loads: vec![0; count],
            sent: vec![0; count],
            links,
            delivered: vec![0; count],
            traffic: vec![0; count * count],
            labels: (0..count).map(|nid| format!("relay-{nid:04}")).collect(),
        }
    }

    /// `Rng.below`.
    fn below(&mut self, n: u64) -> u64 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.rng >> 33) % n
    }

    /// `Hub.inject`.
    fn inject(&mut self) {
        let count = self.queues.len() as u64;
        for _ in 0..INJECT {
            let src = self.below(count) as usize;
            let dst = self.below(count) as u32;
            let ttl = 8 + self.below(8) as u8;
            let size = 1 + self.below(9) as u8;
            self.queues[src].push_back(Packet {
                pid: self.next_pid,
                born: self.clock,
                dst,
                ttl,
                size,
            });
            self.loads[src] += u64::from(size);
            self.next_pid += 1;
        }
    }

    /// `Node.pump` for node `nid`.
    fn pump(&mut self, nid: usize) {
        let count = self.queues.len();
        let links = self.links[nid];
        for _ in 0..BURST {
            let Some(mut packet) = self.queues[nid].pop_front() else {
                break;
            };
            let size = u64::from(packet.size);
            self.loads[nid] -= size;
            if packet.dst as usize == nid {
                self.delivered[nid] += size;
                self.latency += self.clock - packet.born;
                continue;
            }
            packet.ttl -= 1;
            if packet.ttl == 0 {
                self.dropped += 1;
                continue;
            }
            let peer = pick(links, &self.loads, packet.dst);
            self.queues[peer].push_back(packet);
            self.loads[peer] += size;
            self.sent[nid] += 1;
            self.traffic[nid * count + peer] += 1;
        }
    }

    /// `Hub.tick()`: one tick of the network.
    pub fn tick(&mut self) {
        self.clock += 1;
        self.inject();
        for nid in 0..self.queues.len() {
            self.pump(nid);
        }
    }

    /// `Hub.state()`: the same integers in the same order.
    pub fn state(&self) -> Vec<i64> {
        let count = self.queues.len();
        let mut out = Vec::with_capacity(count * 6 + 4);
        for nid in 0..count {
            let queue = &self.queues[nid];
            out.push(self.loads[nid] as i64);
            out.push(queue.len() as i64);
            out.push(self.sent[nid] as i64);
            out.push(self.delivered[nid] as i64);
            out.push(self.labels[nid].chars().map(|c| c as i64).sum());
            out.push(
                queue
                    .iter()
                    .zip(1i64..)
                    .map(|(p, k)| k * p.pid as i64)
                    .sum(),
            );
        }
        out.push(self.dropped as i64);
        out.push(self.latency as i64);
        out.push(self.clock as i64);
        out.push(
            self.traffic
                .iter()
                .zip(1i64..)
                .map(|(&v, k)| k * v as i64)
                .sum(),
        );
        out
    }
}

/// `run(count, seed, ticks)` of `relay.py`.
pub fn run(count: usize, seed: u64, ticks: usize) -> Vec<i64> {
    let mut sim = Sim::new(count, seed);
    for _ in 0..ticks {
        sim.tick();
    }
    sim.state()
}
