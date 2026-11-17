//! Port of `relay.py`. The public items below are the interface the rest of
//! the system uses: keep their names and signatures. Everything else is
//! yours: fields, private functions, more modules.
//!
//! Ownership is described in `DESIGN.md`: `Sim` owns all nodes and queues,
//! peers are indices, and the logic that needs more than one node lives on
//! `Sim`.

use std::collections::VecDeque;

const BURST: usize = 4;
const INJECT: usize = 3;
const MAX_PEERS: usize = 3;
/// Starting capacity of each queue, so that ordinary runs never grow one.
const QUEUE_CAPACITY: usize = 16;

/// `Rng` of `relay.py`: a 64-bit linear congruential generator.
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

/// A packet, held by value in exactly one queue at a time.
#[derive(Clone, Copy)]
struct Packet {
    pid: u64,
    born: u64,
    dst: u32,
    ttl: u16,
    size: u16,
}

/// The per-node numbers. A node's id is its index in `Sim::nodes`; its queue
/// is `Sim::queues` at the same index.
struct Node {
    load: i64,
    sent: u64,
    delivered: u64,
    /// Neighbour ids in peer order; the first `peer_count` are valid.
    peers: [u32; MAX_PEERS],
    peer_count: usize,
    /// Packets forwarded to each peer: the non-zero entries of the Python
    /// `traffic` matrix row for this node.
    traffic: [u64; MAX_PEERS],
}

/// A relay network. The Rust counterpart of `Hub` in `relay.py`.
pub struct Sim {
    rng: Rng,
    clock: u64,
    next_pid: u64,
    dropped: u64,
    latency: u64,
    nodes: Vec<Node>,
    queues: Vec<VecDeque<Packet>>,
}

/// `Node.pick`: the slot, in `nodes[from].peers`, of the neighbour to hand a
/// packet for `dst` to. The destination if it is a neighbour, otherwise the
/// neighbour with the least load, the first such in peer order.
fn pick(nodes: &[Node], from: usize, dst: u32) -> usize {
    let node = &nodes[from];
    let mut best = 0;
    let mut best_load = i64::MAX;
    for (slot, &peer) in node.peers[..node.peer_count].iter().enumerate() {
        if peer == dst {
            return slot;
        }
        let load = nodes[peer as usize].load;
        if slot == 0 || load < best_load {
            best = slot;
            best_load = load;
        }
    }
    best
}

/// Sum of the character codes of `"relay-%04d" % nid`.
fn label_sum(nid: usize) -> i64 {
    let mut sum: i64 = b"relay-".iter().map(|&c| i64::from(c)).sum();
    let mut rest = nid;
    let mut digits = 0;
    while digits < 4 || rest > 0 {
        sum += i64::from(b'0') + (rest % 10) as i64;
        rest /= 10;
        digits += 1;
    }
    sum
}

impl Sim {
    /// `Hub(count, seed)`. `count` is at least 2.
    pub fn new(count: usize, seed: u64) -> Sim {
        assert!(count >= 2, "a network needs at least 2 nodes");
        assert!(u32::try_from(count).is_ok(), "too many nodes");
        let mut nodes = Vec::with_capacity(count);
        for nid in 0..count {
            let mut peers = [0u32; MAX_PEERS];
            let mut peer_count = 0;
            // Widened so that `nid * 7 + 3` cannot overflow on 32-bit targets.
            let (n, c) = (nid as u64, count as u64);
            for other in [(n + 1) % c, (n + c - 1) % c, (n * 7 + 3) % c] {
                let other = other as u32;
                if other as usize != nid && !peers[..peer_count].contains(&other) {
                    peers[peer_count] = other;
                    peer_count += 1;
                }
            }
            nodes.push(Node {
                load: 0,
                sent: 0,
                delivered: 0,
                peers,
                peer_count,
                traffic: [0; MAX_PEERS],
            });
        }
        let queues = (0..count)
            .map(|_| VecDeque::with_capacity(QUEUE_CAPACITY))
            .collect();
        Sim {
            rng: Rng { state: seed },
            clock: 0,
            next_pid: 1,
            dropped: 0,
            latency: 0,
            nodes,
            queues,
        }
    }

    /// `Hub.inject()`.
    fn inject(&mut self) {
        let count = self.nodes.len() as u64;
        for _ in 0..INJECT {
            let src = self.rng.below(count) as usize;
            let dst = self.rng.below(count) as u32;
            let ttl = 8 + self.rng.below(8) as u16;
            let size = 1 + self.rng.below(9) as u16;
            self.queues[src].push_back(Packet {
                pid: self.next_pid,
                born: self.clock,
                dst,
                ttl,
                size,
            });
            self.nodes[src].load += i64::from(size);
            self.next_pid += 1;
        }
    }

    /// `Node.pump()` for node `nid`.
    fn pump(&mut self, nid: usize) {
        for _ in 0..BURST {
            let Some(mut packet) = self.queues[nid].pop_front() else {
                break;
            };
            let size = i64::from(packet.size);
            self.nodes[nid].load -= size;
            if packet.dst as usize == nid {
                self.nodes[nid].delivered += u64::from(packet.size);
                self.latency += self.clock - packet.born;
                continue;
            }
            packet.ttl -= 1;
            if packet.ttl == 0 {
                self.dropped += 1;
                continue;
            }
            let slot = pick(&self.nodes, nid, packet.dst);
            let node = &mut self.nodes[nid];
            node.sent += 1;
            node.traffic[slot] += 1;
            let peer = node.peers[slot] as usize;
            self.queues[peer].push_back(packet);
            self.nodes[peer].load += size;
        }
    }

    /// `Hub.tick()`: one tick of the network.
    pub fn tick(&mut self) {
        self.clock += 1;
        self.inject();
        for nid in 0..self.nodes.len() {
            self.pump(nid);
        }
    }

    /// `Hub.state()`: the same integers in the same order.
    pub fn state(&self) -> Vec<i64> {
        let count = self.nodes.len();
        let mut out = Vec::with_capacity(count * 6 + 4);
        let mut traffic_sum: i64 = 0;
        for (nid, (node, queue)) in self.nodes.iter().zip(&self.queues).enumerate() {
            out.push(node.load);
            out.push(queue.len() as i64);
            out.push(node.sent as i64);
            out.push(node.delivered as i64);
            out.push(label_sum(nid));
            out.push(
                queue
                    .iter()
                    .zip(1i64..)
                    .map(|(packet, k)| k * packet.pid as i64)
                    .sum(),
            );
            // The Python matrix entry for (nid, peer) is at nid * count + peer.
            for (&peer, &sent) in node.peers[..node.peer_count].iter().zip(&node.traffic) {
                traffic_sum += (nid * count + peer as usize + 1) as i64 * sent as i64;
            }
        }
        out.push(self.dropped as i64);
        out.push(self.latency as i64);
        out.push(self.clock as i64);
        out.push(traffic_sum);
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
