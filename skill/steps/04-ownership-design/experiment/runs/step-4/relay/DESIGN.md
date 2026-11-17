# Ownership design: `relay.py` (packet relay network) -> `Sim`

Written before the port. Half a page to a page.

## References in the original

| Holder | Refers to | Direction | Used to change it? |
|---|---|---|---|
| `Hub.nodes` | every `Node` | down | yes: `inject` appends to `node.queue`, adds to `node.load`; `tick` calls `node.pump()` |
| `Node.hub` | the `Hub` that contains it (`Node(self, nid)`) | **up** | yes: `pump` writes `hub.delivered[nid]`, `hub.latency`, `hub.dropped`, `hub.traffic[...]`; reads `hub.clock`, `len(hub.nodes)` |
| `Node.peers` | 1 to 3 other `Node` objects | **sideways** | yes: `pump` does `peer.queue.append(packet)` and `peer.load += size`; `pick` reads `peer.nid`, `peer.load` |
| `Node.queue` | `Packet` objects | down | yes: `packet.ttl -= 1`; the same packet object moves from queue to queue |
| `Hub.rng` | `Rng` | down | yes: `below` advances the state (only from `inject`) |
| `Hub.delivered`, `Hub.traffic` | lists of ints | down | yes, but only through `Node.hub` |

Mutable containers reachable from more than one place:

- each `Node.queue` and `Node.load`: from `Hub.nodes[i]` and from the `peers`
  list of up to three other nodes (in-degree is not bounded by 3: the chord
  `(7n+3) % count` and the dedup make it irregular);
- `Hub.delivered`, `Hub.traffic`, `Hub.latency`, `Hub.dropped`: from the hub
  and from every node through `self.hub`.

No lists are handed over uncopied, no callbacks, no object sits in two
containers except nodes (in `Hub.nodes` and in `peers` lists). A packet is in
exactly one queue at a time. No loop changes the list it iterates:
`for node in self.nodes` never changes `nodes`, but `pump` on node `i`
changes *other elements* (`peer.queue`, `peer.load`), and a later node in the
same tick sees those changes.

## Hot path

- Entry point: `Hub.tick` -> `inject` (3 packets) -> `Node.pump` for every node in index order.
- One iteration (one `pump` of node `i`) reads: its queue head, its `nid`,
  its peers' ids and loads, `hub.clock`, node count.
- One iteration writes: own `queue` (pop front), own `load`, own `sent`;
  one peer's `queue` (push back) and `load`; `delivered[i]`, `latency`,
  `dropped`, `traffic[i*count + peer]`.
- Needed during the same stretch of code: nothing that overlaps. The packet
  is fully removed from queue `i` before anything else is touched; `pick`
  only reads peer loads and yields an index; then the peer's queue and load
  are written. Node `i` is never its own peer, but that is not even needed:
  the accesses are one after the other, not held together.
- What one iteration has to create: nothing. `inject` creates 3 packets per
  tick, which are small plain values placed into queue storage that already
  exists; a packet that is forwarded moves, it is not rebuilt.
- **Allocation budget per iteration, after warm-up:** 0, because a tick only
  moves fixed-size packets between queues and bumps counters. The single
  permitted exception is a queue growing past its own previous high-water
  mark (amortised ring-buffer growth, the counterpart of Python's list
  growth); once the queues have reached their working sizes there are none.
  `state()` is off the hot path and allocates exactly its result `Vec` (1).

## Structure chosen

- Owner of each piece of data: `Sim` owns everything. `nodes: Vec<Node>`
  (plain data: `load`, `sent`, `delivered`, peer ids, per-edge traffic
  counters), `queues: Vec<VecDeque<Packet>>` (one per node, parallel to
  `nodes`), `rng`, `clock`, `next_pid`, `dropped`, `latency`. `Packet` is a
  small `Copy` struct (`pid`, `born`, `dst`, `ttl`, `size`) stored by value
  in the queues.
- How parts refer to each other: a peer is a node index (`u32`) in a fixed
  `[u32; 3]` plus a length, in peer order. `Node.hub` disappears; `nid` is
  the position in `nodes`.
- Where the hot-path logic lives: `pump` and `pick` become `Sim::pump(i)`
  and a `pick` that takes the node slice, since both need more than one node.
- How the hot path gets what it needs at once: reading and writing are
  separated in time and done by index. Pop the packet by value from
  `queues[i]` (borrow ends), update scalars, call `pick` with shared access
  to `nodes` returning a peer slot (a small integer), then write
  `queues[peer]` and `nodes[peer].load`. `queues` and `nodes` are separate
  fields, so nothing has to be cloned, taken out or split.
- Data split by use: the label is never stored. It is only observable as the
  sum of its character codes in `state()`, which is computed from `nid`
  arithmetically there ("relay-" + at least four decimal digits). The
  `count*count` traffic matrix is only ever non-zero at `(i, peer of i)`,
  so the hot path keeps one counter per edge inside the node (`[u64; 3]`),
  and `state()` forms the same weighted sum `(i*count + peer + 1) * v` from
  them. Zero entries add nothing to the sum, so the result is identical.
- Behaviour that must stay exactly as it is:
  - nodes pump in index order, and a packet forwarded to a higher-numbered
    node is seen (and possibly forwarded again) in the same tick;
  - queues are FIFO; at most `BURST` = 4 packets per node per tick,
    delivered and dropped ones count against the burst;
  - order inside `pump`: `load -= size` first, then delivery check, then
    `ttl -= 1`, drop at 0, then pick;
  - `pick`: destination wins if it is a peer, as soon as it is met in peer
    order; otherwise least load, first such in peer order (strict `<`), with
    loads as they are at that moment;
  - peer order: `(n+1)%c`, `(n-1)%c`, `(7n+3)%c`, skipping self and repeats
    (`(n-1) % c` is non-negative in Python: `(n + c - 1) % c`);
  - RNG: 64-bit LCG with wrapping arithmetic, `(state >> 33) % n`, four
    draws per packet in the order src, dst, ttl, size;
  - integers: Python's do not overflow. Counters are 64-bit; the largest
    quantities are the weighted sums in `state()`, which stay far below
    2^63 for any run that is feasible (about 1e14 for 64 nodes and 1e9
    ticks). Not masked or wrapped on purpose: an overflow should fail
    loudly in a debug build, not return a wrong number.

## Rejected

| Alternative | Why not |
|---|---|
| `Rc<RefCell<Node>>` peers plus `Weak<RefCell<Hub>>` back-reference | Keeps the Python shape; nothing here has more than one owner. Adds refcount and borrow-flag work on every hop. |
| `Node::pump(&mut self, hub: &mut Hub)` | The node is inside the hub: cannot be borrowed with it. Would force `mem::take` of the queue or of the node per pump. |
| `split_at_mut` / `get_disjoint_mut` to hold node `i` and its peer together | Not needed: the two are never needed in the same stretch, indices one after the other do it. |
| `Vec<Packet>` queue with `remove(0)` as in Python's `pop(0)` | O(queue length) per pop; `VecDeque` gives the same FIFO order in O(1). |
| One shared packet arena with intrusive per-node lists | More code and an index per packet for no fewer allocations after warm-up. |
| `Box<Packet>` / packets by reference | A packet is 24 bytes and has one holder at a time; moving it by value is cheaper than a pointer chase. |
| Storing `label: String` in `Node` | Heap data in the hot struct that the hot path never reads; only its character sum is observable. |
| Dense `count*count` traffic matrix on the hot path | A random write into a large table per hop; per-edge counters sit in the node already in cache. |
| `peers: Vec<u32>` per node | A heap indirection per pick for at most three ids. |

## Changes during the port

2026-11-15: nothing in the above turned out wrong; no borrow error came up
during the port. One addition: each queue is created with capacity 16, so
warm-up growth is rare. The budget was not changed.

Measured afterwards (`tests/alloc.rs`, counting global allocator): 0
allocations in 100,000 ticks after 1,000 warm-up ticks, for five network
sizes from 2 to 1000 nodes. Budget 0: met.

Copy-like calls in `src/lib.rs` (`clone`, `collect`, `to_vec`, `to_owned`,
`to_string`, `mem::take`, `mem::replace`): one `collect`, in `Sim::new`,
building the vector of queues once. None reachable from `tick`.
