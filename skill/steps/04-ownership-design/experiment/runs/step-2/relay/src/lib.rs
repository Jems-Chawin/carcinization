//! Port of `relay.py`. The public items below are the interface the rest of
//! the system uses: keep their names and signatures. Everything else is
//! yours: fields, private functions, more modules.
//!
//! Ownership is described in `DESIGN.md`: `Sim` owns everything, a node is an
//! index into parallel columns, and peers are indices.

use std::collections::VecDeque;

const BURST: usize = 4;
const INJECT: usize = 3;
/// Initial capacity of each node's queue, so that ordinary runs never grow one.
const QUEUE_CAPACITY: usize = 16;

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

/// A packet in flight. Plain integers; owned by the queue it is in.
#[derive(Clone, Copy)]
struct Packet {
    pid: u64,
    born: u64,
    dst: u32,
    ttl: u8,
    size: u8,
}

/// The neighbours of one node, as node indices in the Python's peer order.
#[derive(Clone, Copy)]
struct Peers {
    ids: [u32; 3],
    len: u8,
}

impl Peers {
    fn as_slice(&self) -> &[u32] {
        &self.ids[..usize::from(self.len)]
    }
}

/// `Node.pick`: the destination if it is a neighbour, otherwise the first
/// least loaded neighbour in peer order.
fn pick(peers: &Peers, load: &[u64], dst: u32) -> u32 {
    let ids = peers.as_slice();
    let mut best = ids[0];
    for &peer in ids {
        if peer == dst {
            return peer;
        }
        if load[peer as usize] < load[best as usize] {
            best = peer;
        }
    }
    best
}

/// A relay network. The Rust counterpart of `Hub` in `relay.py`.
pub struct Sim {
    rng: Rng,
    clock: u64,
    next_pid: u64,
    dropped: u64,
    latency: u64,
    count: usize,
    // Per-node columns, indexed by node id. `load` is what `pick` scans.
    load: Vec<u64>,
    sent: Vec<u64>,
    queues: Vec<VecDeque<Packet>>,
    peers: Vec<Peers>,
    delivered: Vec<u64>,
    /// `count * count` counters: `traffic[from * count + to]`.
    traffic: Vec<u64>,
    /// Cold: read only by `state`.
    labels: Vec<String>,
}

impl Sim {
    /// `Hub(count, seed)`. `count` is at least 2.
    pub fn new(count: usize, seed: u64) -> Sim {
        assert!(count >= 2, "count must be at least 2");
        assert!(u32::try_from(count).is_ok(), "count must fit in 32 bits");
        let mut peers = Vec::with_capacity(count);
        for nid in 0..count as u64 {
            let n = count as u64;
            let mut p = Peers {
                ids: [0; 3],
                len: 0,
            };
            for other in [(nid + 1) % n, (nid + n - 1) % n, (nid * 7 + 3) % n] {
                let other = other as u32;
                if u64::from(other) != nid && !p.as_slice().contains(&other) {
                    p.ids[usize::from(p.len)] = other;
                    p.len += 1;
                }
            }
            peers.push(p);
        }
        Sim {
            rng: Rng { state: seed },
            clock: 0,
            next_pid: 1,
            dropped: 0,
            latency: 0,
            count,
            load: vec![0; count],
            sent: vec![0; count],
            queues: (0..count)
                .map(|_| VecDeque::with_capacity(QUEUE_CAPACITY))
                .collect(),
            peers,
            delivered: vec![0; count],
            traffic: vec![0; count * count],
            labels: (0..count).map(|nid| format!("relay-{nid:04}")).collect(),
        }
    }

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
            self.load[src] += u64::from(size);
            self.next_pid += 1;
        }
    }

    /// `Node.pump` for node `i`.
    fn pump(&mut self, i: usize) {
        for _ in 0..BURST {
            let Some(mut packet) = self.queues[i].pop_front() else {
                break;
            };
            let size = u64::from(packet.size);
            self.load[i] -= size;
            if packet.dst as usize == i {
                self.delivered[i] += size;
                self.latency += self.clock - packet.born;
                continue;
            }
            packet.ttl -= 1;
            if packet.ttl == 0 {
                self.dropped += 1;
                continue;
            }
            let peer = pick(&self.peers[i], &self.load, packet.dst) as usize;
            self.queues[peer].push_back(packet);
            self.load[peer] += size;
            self.sent[i] += 1;
            self.traffic[i * self.count + peer] += 1;
        }
    }

    /// `Hub.tick()`: one tick of the network.
    pub fn tick(&mut self) {
        self.clock += 1;
        self.inject();
        for i in 0..self.count {
            self.pump(i);
        }
    }

    /// `Hub.state()`: the same integers in the same order.
    pub fn state(&self) -> Vec<i64> {
        let mut out = Vec::with_capacity(self.count * 6 + 4);
        for i in 0..self.count {
            let queue = &self.queues[i];
            out.push(to_i64(self.load[i].into()));
            out.push(to_i64(queue.len() as i128));
            out.push(to_i64(self.sent[i].into()));
            out.push(to_i64(self.delivered[i].into()));
            out.push(to_i64(
                self.labels[i]
                    .chars()
                    .map(|c| i128::from(u32::from(c)))
                    .sum(),
            ));
            out.push(to_i64(weighted_sum(queue.iter().map(|p| p.pid))));
        }
        out.push(to_i64(self.dropped.into()));
        out.push(to_i64(self.latency.into()));
        out.push(to_i64(self.clock.into()));
        out.push(to_i64(weighted_sum(self.traffic.iter().copied())));
        out
    }
}

/// `sum((k + 1) * v for k, v in enumerate(values))`.
fn weighted_sum(values: impl Iterator<Item = u64>) -> i128 {
    values
        .enumerate()
        .map(|(k, v)| (k as i128 + 1) * i128::from(v))
        .sum()
}

/// Python integers do not overflow; say so loudly instead of wrapping.
fn to_i64(value: i128) -> i64 {
    i64::try_from(value).expect("state value exceeds i64")
}

/// `run(count, seed, ticks)` of `relay.py`.
pub fn run(count: usize, seed: u64, ticks: usize) -> Vec<i64> {
    let mut sim = Sim::new(count, seed);
    for _ in 0..ticks {
        sim.tick();
    }
    sim.state()
}
