//! Port of `relay.py`. The public items below are the interface the rest of
//! the system uses: keep their names and signatures. Everything else is
//! yours: fields, private functions, more modules.

use std::collections::VecDeque;

const BURST: usize = 4;
const INJECT: usize = 3;
/// A node has at most three neighbours: next, previous and one chord.
const MAX_PEERS: usize = 3;

struct Packet {
    pid: u64,
    born: u64,
    dst: u32,
    ttl: u8,
    size: u8,
}

/// The neighbours of one node, in the peer order of `relay.py`, with the
/// number of packets handed to each (the node's row of the traffic matrix).
struct Links {
    len: usize,
    peers: [u32; MAX_PEERS],
    traffic: [u64; MAX_PEERS],
}

/// A relay network. The Rust counterpart of `Hub` in `relay.py`.
///
/// Nodes are indices into parallel vectors rather than objects pointing at
/// each other. The `count * count` traffic matrix of the Python is kept
/// sparsely, per link, since only links can ever carry traffic.
pub struct Sim {
    rng: u64,
    clock: u64,
    next_pid: u64,
    dropped: u64,
    latency: u64,
    load: Vec<u64>,
    queues: Vec<VecDeque<Packet>>,
    /// One bit per node, set exactly when its queue is not empty, so that a
    /// tick visits only the nodes that have something to forward.
    busy: Vec<u64>,
    links: Vec<Links>,
    sent: Vec<u64>,
    delivered: Vec<u64>,
}

impl Sim {
    /// `Hub(count, seed)`. `count` is at least 2.
    pub fn new(count: usize, seed: u64) -> Sim {
        assert!(count >= 2, "a network needs at least 2 nodes");
        assert!(u32::try_from(count).is_ok(), "too many nodes");
        let links = (0..count)
            .map(|nid| {
                let mut links = Links {
                    len: 0,
                    peers: [0; MAX_PEERS],
                    traffic: [0; MAX_PEERS],
                };
                let chord = ((nid as u128 * 7 + 3) % count as u128) as usize;
                for other in [(nid + 1) % count, (nid + count - 1) % count, chord] {
                    let other = other as u32;
                    if other as usize != nid && !links.peers[..links.len].contains(&other) {
                        links.peers[links.len] = other;
                        links.len += 1;
                    }
                }
                links
            })
            .collect();
        Sim {
            rng: seed,
            clock: 0,
            next_pid: 1,
            dropped: 0,
            latency: 0,
            load: vec![0; count],
            queues: (0..count).map(|_| VecDeque::new()).collect(),
            busy: vec![0; count.div_ceil(64)],
            links,
            sent: vec![0; count],
            delivered: vec![0; count],
        }
    }

    /// `Hub.tick()`: one tick of the network.
    pub fn tick(&mut self) {
        self.clock += 1;
        self.inject();
        // Nodes pump in ascending order, as in the Python. A node with an
        // empty queue does nothing, so only busy ones are visited; the word
        // is read again after each pump because a pump can make a later
        // node busy within the same tick.
        for word in 0..self.busy.len() {
            let mut bits = self.busy[word];
            while bits != 0 {
                let bit = bits.trailing_zeros();
                self.pump(word * 64 + bit as usize);
                bits = self.busy[word] & (!0u64 << bit << 1);
            }
        }
    }

    /// `Hub.state()`: the same integers in the same order.
    pub fn state(&self) -> Vec<i64> {
        let count = self.queues.len();
        let mut out = Vec::with_capacity(count * 6 + 4);
        let mut traffic: u64 = 0;
        for nid in 0..count {
            let queue = &self.queues[nid];
            out.push(self.load[nid] as i64);
            out.push(queue.len() as i64);
            out.push(self.sent[nid] as i64);
            out.push(self.delivered[nid] as i64);
            out.push(label_sum(nid));
            out.push(
                queue
                    .iter()
                    .zip(1u64..)
                    .map(|(packet, k)| k * packet.pid)
                    .sum::<u64>() as i64,
            );
            let links = &self.links[nid];
            for (&peer, &sent) in links.peers[..links.len].iter().zip(&links.traffic) {
                traffic += (nid * count + peer as usize + 1) as u64 * sent;
            }
        }
        out.push(self.dropped as i64);
        out.push(self.latency as i64);
        out.push(self.clock as i64);
        out.push(traffic as i64);
        out
    }

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
            self.busy[src / 64] |= 1 << (src % 64);
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

    /// `Node.pump()` for node `nid`.
    fn pump(&mut self, nid: usize) {
        for _ in 0..BURST {
            let Some(mut packet) = self.queues[nid].pop_front() else {
                break;
            };
            if self.queues[nid].is_empty() {
                self.busy[nid / 64] &= !(1 << (nid % 64));
            }
            let size = u64::from(packet.size);
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
            let slot = self.pick(nid, packet.dst);
            let links = &mut self.links[nid];
            links.traffic[slot] += 1;
            let peer = links.peers[slot] as usize;
            self.queues[peer].push_back(packet);
            self.busy[peer / 64] |= 1 << (peer % 64);
            self.load[peer] += size;
            self.sent[nid] += 1;
        }
    }

    /// `Node.pick()`: which of `nid`'s links a packet for `dst` goes out on.
    /// The destination if it is a neighbour, otherwise the least loaded
    /// neighbour, the first such in peer order.
    fn pick(&self, nid: usize, dst: u32) -> usize {
        let links = &self.links[nid];
        let mut best = 0;
        let mut best_load = u64::MAX;
        for (slot, &peer) in links.peers[..links.len].iter().enumerate() {
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

/// The sum of the character codes of `"relay-%04d" % nid`.
fn label_sum(nid: usize) -> i64 {
    let mut sum: i64 = "relay-".bytes().map(i64::from).sum();
    let mut rest = nid;
    let mut digits = 0;
    while digits < 4 || rest > 0 {
        sum += i64::from(b'0') + (rest % 10) as i64;
        rest /= 10;
        digits += 1;
    }
    sum
}

/// `run(count, seed, ticks)` of `relay.py`.
pub fn run(count: usize, seed: u64, ticks: usize) -> Vec<i64> {
    let mut sim = Sim::new(count, seed);
    for _ in 0..ticks {
        sim.tick();
    }
    sim.state()
}
