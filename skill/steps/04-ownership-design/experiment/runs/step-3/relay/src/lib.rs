//! Port of `relay.py`. The public items below are the interface the rest of
//! the system uses: keep their names and signatures. Everything else is
//! yours: fields, private functions, more modules.
//!
//! Ownership (see `DESIGN.md`): `Sim` owns all state as parallel vectors
//! indexed by node id. Nodes refer to each other by index, packets are small
//! `Copy` values stored in per-node ring buffers, and the logic that needs
//! the whole network (`pump`, `pick`) lives on `Sim`.

use std::collections::VecDeque;

const BURST: usize = 4;
const INJECT: usize = 3;
const MAX_PEERS: usize = 3;
/// Room reserved in every queue when the network is built, so that `tick`
/// does not have to grow a ring buffer later. `relay.py` runs of 300 000
/// ticks never show a queue longer than 7; a longer queue still works, it
/// grows once.
const QUEUE_RESERVE: usize = 32;

/// `Rng` of `relay.py`: a 64-bit LCG.
struct Rng {
    state: u64,
}

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.state >> 33) % n
    }
}

/// `Packet` of `relay.py`, stored by value in exactly one queue at a time.
#[derive(Clone, Copy)]
struct Packet {
    pid: u64,
    born: u64,
    dst: u32,
    /// 8..=15 at birth, dropped when it reaches 0.
    ttl: u8,
    /// 1..=9.
    size: u8,
}

/// The neighbours of one node, in the order `relay.py` builds them.
#[derive(Clone, Copy)]
struct Peers {
    ids: [u32; MAX_PEERS],
    len: u8,
}

impl Peers {
    fn as_slice(&self) -> &[u32] {
        &self.ids[..usize::from(self.len)]
    }
}

/// A relay network. The Rust counterpart of `Hub` in `relay.py`.
pub struct Sim {
    rng: Rng,
    count: usize,
    clock: u64,
    next_pid: u64,
    dropped: u64,
    latency: u64,
    // Per node, indexed by node id. Hot: read or written every tick.
    queues: Vec<VecDeque<Packet>>,
    loads: Vec<u64>,
    sent: Vec<u64>,
    delivered: Vec<u64>,
    peers: Vec<Peers>,
    /// `traffic[src * count + dst]`: packets forwarded from `src` to `dst`.
    traffic: Vec<u64>,
    // Cold: only `state` reads it.
    labels: Vec<String>,
}

impl Sim {
    /// `Hub(count, seed)`. `count` is at least 2.
    pub fn new(count: usize, seed: u64) -> Sim {
        assert!(count >= 2, "a network needs at least 2 nodes");
        let count32 = u32::try_from(count).expect("node count must fit in u32");
        let count64 = u64::from(count32);
        let peers = (0..count64)
            .map(|nid| {
                let mut found = Peers {
                    ids: [0; MAX_PEERS],
                    len: 0,
                };
                let candidates = [
                    (nid + 1) % count64,
                    (nid + count64 - 1) % count64,
                    (nid * 7 + 3) % count64,
                ];
                for other in candidates {
                    // Lossless: `other < count64 <= u32::MAX`.
                    let other = other as u32;
                    if u64::from(other) != nid && !found.as_slice().contains(&other) {
                        found.ids[usize::from(found.len)] = other;
                        found.len += 1;
                    }
                }
                found
            })
            .collect();
        Sim {
            rng: Rng { state: seed },
            count,
            clock: 0,
            next_pid: 1,
            dropped: 0,
            latency: 0,
            queues: (0..count)
                .map(|_| VecDeque::with_capacity(QUEUE_RESERVE))
                .collect(),
            loads: vec![0; count],
            sent: vec![0; count],
            delivered: vec![0; count],
            peers,
            traffic: vec![0; count * count],
            labels: (0..count).map(|nid| format!("relay-{nid:04}")).collect(),
        }
    }

    /// `Hub.tick()`: one tick of the network.
    pub fn tick(&mut self) {
        self.clock += 1;
        self.inject();
        for nid in 0..self.count {
            self.pump(nid);
        }
    }

    /// `Hub.inject()`: three new packets at random nodes.
    fn inject(&mut self) {
        let count = self.count as u64;
        for _ in 0..INJECT {
            let src = self.rng.below(count) as usize;
            let dst = self.rng.below(count) as u32;
            let ttl = 8 + self.rng.below(8) as u8;
            let size = 1 + self.rng.below(9) as u8;
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

    /// `Node.pick(packet)`: the destination if it is a neighbour of `nid`,
    /// otherwise the neighbour with the least load, the first such in peer
    /// order.
    fn pick(&self, nid: usize, dst: u32) -> usize {
        let peers = self.peers[nid].as_slice();
        let mut best = peers[0] as usize;
        if peers[0] == dst {
            return best;
        }
        let mut best_load = self.loads[best];
        for &peer in &peers[1..] {
            if peer == dst {
                return peer as usize;
            }
            let load = self.loads[peer as usize];
            if load < best_load {
                best = peer as usize;
                best_load = load;
            }
        }
        best
    }

    /// `Node.pump()` for node `nid`: forward up to `BURST` packets.
    fn pump(&mut self, nid: usize) {
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
            let peer = self.pick(nid, packet.dst);
            self.queues[peer].push_back(packet);
            self.loads[peer] += size;
            self.sent[nid] += 1;
            self.traffic[nid * self.count + peer] += 1;
        }
    }

    /// `Hub.state()`: the same integers in the same order.
    pub fn state(&self) -> Vec<i64> {
        let mut out = Vec::with_capacity(self.count * 6 + 4);
        for nid in 0..self.count {
            let queue = &self.queues[nid];
            out.push(int(self.loads[nid]));
            out.push(int(queue.len() as u64));
            out.push(int(self.sent[nid]));
            out.push(int(self.delivered[nid]));
            out.push(int(self.labels[nid]
                .chars()
                .map(|c| u64::from(u32::from(c)))
                .sum()));
            out.push(weighted(queue.iter().map(|p| p.pid)));
        }
        out.push(int(self.dropped));
        out.push(int(self.latency));
        out.push(int(self.clock));
        out.push(weighted(self.traffic.iter().copied()));
        out
    }
}

/// Python integers do not overflow; a value that does not fit the interface's
/// `i64` is reported instead of silently differing from `relay.py`.
fn int(value: u64) -> i64 {
    i64::try_from(value).expect("value exceeds i64, which relay.py would not wrap")
}

/// `sum((k + 1) * v for k, v in enumerate(values))`.
fn weighted(values: impl Iterator<Item = u64>) -> i64 {
    let total: i128 = values
        .zip(1i128..)
        .map(|(value, weight)| weight * i128::from(value))
        .sum();
    i64::try_from(total).expect("checksum exceeds i64, which relay.py would not wrap")
}

/// `run(count, seed, ticks)` of `relay.py`.
pub fn run(count: usize, seed: u64, ticks: usize) -> Vec<i64> {
    let mut sim = Sim::new(count, seed);
    for _ in 0..ticks {
        sim.tick();
    }
    sim.state()
}
