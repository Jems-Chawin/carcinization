//! Port of `relay.py`. The public items below are the interface the rest of
//! the system uses: keep their names and signatures. Everything else is
//! yours: fields, private functions, more modules.
//!
//! Layout: nodes are plain indices and their fields live in parallel vectors
//! owned by `Sim`, so a tick touches no reference counts and allocates only
//! when a queue outgrows its buffer. Two things differ from the Python in
//! representation but not in result:
//!
//! * `traffic` is kept per (node, peer slot) instead of as a `count * count`
//!   matrix; a node has at most three peers, so that is all the matrix ever
//!   holds. `state` weights each counter by its matrix position.
//! * a bitset records which queues are non-empty, so a tick visits only the
//!   nodes that have something to pump (an empty node's `pump` does nothing).

use std::collections::VecDeque;

const BURST: usize = 4;
const INJECT: usize = 3;
const MAX_PEERS: usize = 3;

struct Packet {
    pid: u64,
    born: i64,
    dst: u32,
    ttl: u8,
    size: u8,
}

/// The neighbours of one node, in the order the Python appends them.
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
    rng: u64,
    clock: i64,
    next_pid: u64,
    dropped: i64,
    latency: i64,
    queues: Vec<VecDeque<Packet>>,
    peers: Vec<Peers>,
    load: Vec<i64>,
    sent: Vec<i64>,
    delivered: Vec<i64>,
    /// `traffic[nid * MAX_PEERS + slot]`: packets `nid` handed to its peer
    /// number `slot`.
    traffic: Vec<i64>,
    /// Bit `nid` is set exactly when `queues[nid]` is non-empty.
    busy: Vec<u64>,
}

impl Sim {
    /// `Hub(count, seed)`. `count` is at least 2.
    pub fn new(count: usize, seed: u64) -> Sim {
        assert!(count >= 2, "a network needs at least 2 nodes");
        assert!(u32::try_from(count).is_ok(), "too many nodes");
        let peers = (0..count)
            .map(|nid| {
                let mut found = Peers {
                    ids: [0; MAX_PEERS],
                    len: 0,
                };
                // u128: `nid * 7 + 3` must not wrap, Python's does not.
                let chord = ((nid as u128 * 7 + 3) % count as u128) as usize;
                for other in [(nid + 1) % count, (nid + count - 1) % count, chord] {
                    let other = other as u32;
                    if other as usize != nid && !found.as_slice().contains(&other) {
                        found.ids[usize::from(found.len)] = other;
                        found.len += 1;
                    }
                }
                found
            })
            .collect();
        Sim {
            rng: seed,
            clock: 0,
            next_pid: 1,
            dropped: 0,
            latency: 0,
            queues: (0..count).map(|_| VecDeque::new()).collect(),
            peers,
            load: vec![0; count],
            sent: vec![0; count],
            delivered: vec![0; count],
            traffic: vec![0; count * MAX_PEERS],
            busy: vec![0; count.div_ceil(64)],
        }
    }

    /// `Hub.tick()`: one tick of the network.
    pub fn tick(&mut self) {
        self.clock += 1;
        self.inject();
        // Visit the busy nodes in ascending order. A pump can make a later
        // node busy, and that node must then be pumped in this same tick, so
        // the word is read again after every pump rather than once.
        for word in 0..self.busy.len() {
            let mut todo = !0u64;
            loop {
                let bits = self.busy[word] & todo;
                if bits == 0 {
                    break;
                }
                let bit = bits.trailing_zeros();
                self.pump(word * 64 + bit as usize);
                // Keep only the bits above `bit`.
                todo = (!1u64).checked_shl(bit).unwrap_or(0);
            }
        }
    }

    /// `Hub.state()`: the same integers in the same order.
    pub fn state(&self) -> Vec<i64> {
        let count = self.queues.len();
        let mut out = Vec::with_capacity(count * 6 + 4);
        for nid in 0..count {
            let queue = &self.queues[nid];
            out.push(self.load[nid]);
            out.push(queue.len() as i64);
            out.push(self.sent[nid]);
            out.push(self.delivered[nid]);
            let label = format!("relay-{nid:04}");
            out.push(label.bytes().map(i64::from).sum());
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
        out.push(self.clock);
        let mut weighted = 0i64;
        for nid in 0..count {
            for (slot, &peer) in self.peers[nid].as_slice().iter().enumerate() {
                let position = (nid * count + peer as usize) as i64;
                weighted += (position + 1) * self.traffic[nid * MAX_PEERS + slot];
            }
        }
        out.push(weighted);
        out
    }

    /// `Rng.below(n)`.
    fn below(&mut self, n: u64) -> u64 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.rng >> 33) % n
    }

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
            self.load[src] += i64::from(size);
            self.busy[src / 64] |= 1 << (src % 64);
            self.next_pid += 1;
        }
    }

    /// `Node.pump()` for node `nid`, whose queue is non-empty.
    fn pump(&mut self, nid: usize) {
        let peers = self.peers[nid];
        for _ in 0..BURST {
            let Some(mut packet) = self.queues[nid].pop_front() else {
                break;
            };
            let size = i64::from(packet.size);
            self.load[nid] -= size;
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
            let slot = self.pick(peers.as_slice(), packet.dst);
            let peer = peers.ids[slot] as usize;
            self.queues[peer].push_back(packet);
            self.load[peer] += size;
            self.busy[peer / 64] |= 1 << (peer % 64);
            self.sent[nid] += 1;
            self.traffic[nid * MAX_PEERS + slot] += 1;
        }
        if self.queues[nid].is_empty() {
            self.busy[nid / 64] &= !(1 << (nid % 64));
        }
    }

    /// `Node.pick()`: the slot of the peer to hand a packet to. The
    /// destination if it is a peer, otherwise the first least loaded peer.
    fn pick(&self, peers: &[u32], dst: u32) -> usize {
        let mut best = 0;
        let mut best_load = i64::MAX;
        for (slot, &peer) in peers.iter().enumerate() {
            if peer == dst {
                return slot;
            }
            let load = self.load[peer as usize];
            if load < best_load {
                best = slot;
                best_load = load;
            }
        }
        best
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
