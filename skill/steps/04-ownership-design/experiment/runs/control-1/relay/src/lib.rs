//! Port of `relay.py`. The public items below are the interface the rest of
//! the system uses: keep their names and signatures. Everything else is
//! yours: fields, private functions, more modules.
//!
//! Layout: the Python object graph (hub <-> nodes <-> peers) is flattened
//! into index-addressed arrays owned by `Sim`. Peers are node indices in a
//! fixed three-slot array per node, loads live in their own dense vector
//! because `pick` reads the loads of neighbours, and each queue is a
//! `VecDeque` so that taking the head is O(1) (`list.pop(0)` is O(n)).

use std::collections::VecDeque;

const BURST: usize = 4;
const INJECT: usize = 3;
/// A node has at most three distinct neighbours: next, previous and a chord.
const MAX_PEERS: usize = 3;

struct Rng {
    state: u64,
}

impl Rng {
    #[inline]
    fn below(&mut self, n: u64) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.state >> 33) % n
    }
}

/// 24 bytes. `dst` fits `u32` because `Sim::new` rejects larger networks,
/// `ttl` is below 16 and `size` is below 10.
struct Packet {
    pid: u64,
    born: u64,
    dst: u32,
    ttl: u8,
    size: u8,
}

/// The neighbours of one node, in the order `Hub.__init__` adds them.
#[derive(Clone, Copy)]
struct Peers {
    ids: [u32; MAX_PEERS],
    len: u8,
}

impl Peers {
    #[inline]
    fn as_slice(&self) -> &[u32] {
        &self.ids[..self.len as usize]
    }
}

/// A relay network. The Rust counterpart of `Hub` in `relay.py`.
pub struct Sim {
    rng: Rng,
    count: usize,
    clock: u64,
    next_pid: u64,
    dropped: i64,
    latency: i64,
    // Per node, indexed by node id.
    queues: Vec<VecDeque<Packet>>,
    peers: Vec<Peers>,
    loads: Vec<i64>,
    sent: Vec<i64>,
    delivered: Vec<i64>,
    /// `count * count` counters, row = sender, column = receiver.
    traffic: Vec<i64>,
}

impl Sim {
    /// `Hub(count, seed)`. `count` is at least 2.
    pub fn new(count: usize, seed: u64) -> Sim {
        assert!(
            u32::try_from(count).is_ok(),
            "node count {count} does not fit in 32 bits"
        );
        let mut peers = Vec::with_capacity(count);
        for nid in 0..count {
            let mut entry = Peers {
                ids: [0; MAX_PEERS],
                len: 0,
            };
            // `(nid - 1) % count` is non-negative in Python.
            let others = [
                (nid + 1) % count,
                (nid + count - 1) % count,
                ((nid as u64 * 7 + 3) % count as u64) as usize,
            ];
            for other in others {
                let other = other as u32;
                if other as usize != nid && !entry.as_slice().contains(&other) {
                    entry.ids[entry.len as usize] = other;
                    entry.len += 1;
                }
            }
            peers.push(entry);
        }
        Sim {
            rng: Rng { state: seed },
            count,
            clock: 0,
            next_pid: 1,
            dropped: 0,
            latency: 0,
            queues: (0..count).map(|_| VecDeque::new()).collect(),
            peers,
            loads: vec![0; count],
            sent: vec![0; count],
            delivered: vec![0; count],
            traffic: vec![0; count * count],
        }
    }

    /// `Hub.tick()`: one tick of the network.
    pub fn tick(&mut self) {
        self.clock += 1;
        self.inject();
        for nid in 0..self.count {
            if !self.queues[nid].is_empty() {
                self.pump(nid);
            }
        }
    }

    #[inline]
    fn enqueue(&mut self, nid: usize, packet: Packet) {
        self.loads[nid] += i64::from(packet.size);
        self.queues[nid].push_back(packet);
    }

    /// `Hub.state()`: the same integers in the same order.
    pub fn state(&self) -> Vec<i64> {
        let mut out = Vec::with_capacity(self.count * 6 + 4);
        for nid in 0..self.count {
            let queue = &self.queues[nid];
            out.push(self.loads[nid]);
            out.push(queue.len() as i64);
            out.push(self.sent[nid]);
            out.push(self.delivered[nid]);
            out.push(label_sum(nid));
            out.push(
                queue
                    .iter()
                    .enumerate()
                    .map(|(k, p)| (k as i64 + 1) * p.pid as i64)
                    .sum(),
            );
        }
        out.push(self.dropped);
        out.push(self.latency);
        out.push(self.clock as i64);
        out.push(
            self.traffic
                .iter()
                .enumerate()
                .map(|(k, &v)| (k as i64 + 1) * v)
                .sum(),
        );
        out
    }

    fn inject(&mut self) {
        let count = self.count as u64;
        for _ in 0..INJECT {
            let src = self.rng.below(count) as usize;
            let dst = self.rng.below(count) as u32;
            let ttl = 8 + self.rng.below(8) as u8;
            let size = 1 + self.rng.below(9) as u8;
            let packet = Packet {
                pid: self.next_pid,
                born: self.clock,
                dst,
                ttl,
                size,
            };
            self.enqueue(src, packet);
            self.next_pid += 1;
        }
    }

    /// `Node.pump()` for node `nid`.
    fn pump(&mut self, nid: usize) {
        let peers = self.peers[nid];
        let row = nid * self.count;
        for _ in 0..BURST {
            let Some(mut packet) = self.queues[nid].pop_front() else {
                break;
            };
            let size = i64::from(packet.size);
            self.loads[nid] -= size;
            if packet.dst as usize == nid {
                self.delivered[nid] += size;
                self.latency += (self.clock - packet.born) as i64;
                continue;
            }
            packet.ttl -= 1;
            if packet.ttl == 0 {
                self.dropped += 1;
                continue;
            }
            let peer = pick(peers.as_slice(), &self.loads, packet.dst);
            self.enqueue(peer, packet);
            self.sent[nid] += 1;
            self.traffic[row + peer] += 1;
        }
    }
}

/// `Node.pick()`: the destination if it is a neighbour, otherwise the
/// neighbour with the least load, the first such in peer order.
///
/// As in the Python, the scan stops at the destination, so it must not be
/// reordered into "find the minimum, then look for the destination".
#[inline]
fn pick(peers: &[u32], loads: &[i64], dst: u32) -> usize {
    let mut best = usize::MAX;
    let mut best_load = i64::MAX;
    for &peer in peers {
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

/// The sum of the code points of `"relay-%04d" % nid`, without building it.
fn label_sum(nid: usize) -> i64 {
    const PREFIX: i64 =
        b'r' as i64 + b'e' as i64 + b'l' as i64 + b'a' as i64 + b'y' as i64 + b'-' as i64;
    let mut digits = 0;
    let mut width = 0;
    let mut rest = nid;
    while rest > 0 {
        digits += (rest % 10) as i64;
        rest /= 10;
        width += 1;
    }
    // Zero padding to four digits; each pad contributes '0' like any digit.
    PREFIX + digits + i64::from(b'0') * width.max(4)
}

/// `run(count, seed, ticks)` of `relay.py`.
pub fn run(count: usize, seed: u64, ticks: usize) -> Vec<i64> {
    let mut sim = Sim::new(count, seed);
    for _ in 0..ticks {
        sim.tick();
    }
    sim.state()
}
