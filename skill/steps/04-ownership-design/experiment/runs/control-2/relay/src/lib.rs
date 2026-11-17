//! Port of `relay.py`. The public items below are the interface the rest of
//! the system uses: keep their names and signatures. Everything else is
//! yours: fields, private functions, more modules.

use std::collections::VecDeque;

const BURST: usize = 4;
const INJECT: usize = 3;
/// A node has at most three distinct neighbours: next, previous and a chord.
const MAX_PEERS: usize = 3;

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

struct Packet {
    pid: u64,
    born: u64,
    dst: u32,
    ttl: u8,
    size: u8,
}

struct Node {
    queue: VecDeque<Packet>,
    /// Neighbour ids in the order `relay.py` builds them; `npeers` are valid.
    peers: [u32; MAX_PEERS],
    npeers: u8,
    sent: u64,
}

/// A relay network. The Rust counterpart of `Hub` in `relay.py`.
pub struct Sim {
    rng: Rng,
    clock: u64,
    next_pid: u64,
    dropped: u64,
    latency: u64,
    nodes: Vec<Node>,
    /// `Node.load`, kept apart from the nodes so that choosing a neighbour
    /// reads one small dense array.
    load: Vec<i64>,
    delivered: Vec<u64>,
    /// Packets forwarded per (node, peer slot): `traffic[nid * MAX_PEERS + slot]`.
    /// The Python keeps a `count * count` matrix, but only these cells can
    /// ever be non-zero.
    traffic: Vec<u64>,
}

impl Sim {
    /// `Hub(count, seed)`. `count` is at least 2.
    pub fn new(count: usize, seed: u64) -> Sim {
        assert!(count >= 2, "a network needs at least 2 nodes");
        assert!(u32::try_from(count).is_ok(), "node ids must fit in 32 bits");
        let nodes = (0..count)
            .map(|nid| {
                let mut peers = [0u32; MAX_PEERS];
                let mut npeers = 0;
                let chord = ((nid as u128 * 7 + 3) % count as u128) as usize;
                for other in [(nid + 1) % count, (nid + count - 1) % count, chord] {
                    let other = other as u32;
                    if other as usize != nid && !peers[..npeers].contains(&other) {
                        peers[npeers] = other;
                        npeers += 1;
                    }
                }
                Node {
                    queue: VecDeque::new(),
                    peers,
                    npeers: npeers as u8,
                    sent: 0,
                }
            })
            .collect();
        Sim {
            rng: Rng { state: seed },
            clock: 0,
            next_pid: 1,
            dropped: 0,
            latency: 0,
            nodes,
            load: vec![0; count],
            delivered: vec![0; count],
            traffic: vec![0; count * MAX_PEERS],
        }
    }

    /// `Hub.tick()`: one tick of the network.
    pub fn tick(&mut self) {
        self.clock += 1;
        self.inject();
        for nid in 0..self.nodes.len() {
            if !self.nodes[nid].queue.is_empty() {
                self.pump(nid);
            }
        }
    }

    /// `Hub.state()`: the same integers in the same order.
    pub fn state(&self) -> Vec<i64> {
        let count = self.nodes.len();
        let mut out = Vec::with_capacity(count * 6 + 4);
        for (nid, node) in self.nodes.iter().enumerate() {
            out.push(self.load[nid]);
            out.push(node.queue.len() as i64);
            out.push(node.sent as i64);
            out.push(self.delivered[nid] as i64);
            out.push(label_sum(nid));
            let pids: u64 = node
                .queue
                .iter()
                .enumerate()
                .map(|(k, p)| (k as u64 + 1) * p.pid)
                .sum();
            out.push(pids as i64);
        }
        out.push(self.dropped as i64);
        out.push(self.latency as i64);
        out.push(self.clock as i64);
        let mut traffic = 0u64;
        for (nid, node) in self.nodes.iter().enumerate() {
            for slot in 0..node.npeers as usize {
                let cell = nid * count + node.peers[slot] as usize;
                traffic += (cell as u64 + 1) * self.traffic[nid * MAX_PEERS + slot];
            }
        }
        out.push(traffic as i64);
        out
    }

    fn inject(&mut self) {
        let count = self.nodes.len() as u64;
        for _ in 0..INJECT {
            let src = self.rng.below(count) as usize;
            let dst = self.rng.below(count) as u32;
            let ttl = 8 + self.rng.below(8) as u8;
            let size = 1 + self.rng.below(9) as u8;
            self.nodes[src].queue.push_back(Packet {
                pid: self.next_pid,
                born: self.clock,
                dst,
                ttl,
                size,
            });
            self.load[src] += i64::from(size);
            self.next_pid += 1;
        }
    }

    /// `Node.pump()` for node `nid`.
    fn pump(&mut self, nid: usize) {
        let peers = self.nodes[nid].peers;
        let npeers = self.nodes[nid].npeers as usize;
        let peers = &peers[..npeers];
        for _ in 0..BURST {
            let Some(mut packet) = self.nodes[nid].queue.pop_front() else {
                break;
            };
            let size = i64::from(packet.size);
            self.load[nid] -= size;
            if packet.dst as usize == nid {
                self.delivered[nid] += size as u64;
                self.latency += self.clock - packet.born;
                continue;
            }
            packet.ttl -= 1;
            if packet.ttl == 0 {
                self.dropped += 1;
                continue;
            }
            let slot = self.pick(peers, packet.dst);
            let peer = peers[slot] as usize;
            self.nodes[peer].queue.push_back(packet);
            self.load[peer] += size;
            self.nodes[nid].sent += 1;
            self.traffic[nid * MAX_PEERS + slot] += 1;
        }
    }

    /// `Node.pick()`: the slot of the peer to hand a packet for `dst` to. That
    /// is `dst` itself if it is a peer, otherwise the first least loaded peer.
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

/// The sum of the character codes of `"relay-%04d" % nid`.
fn label_sum(nid: usize) -> i64 {
    let mut sum: i64 = "relay-".bytes().map(i64::from).sum();
    let mut rest = nid;
    let mut digits = 0;
    while rest > 0 || digits < 4 {
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
