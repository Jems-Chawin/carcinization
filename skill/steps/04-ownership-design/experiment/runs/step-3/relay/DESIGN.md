# Ownership design: `relay.py` (packet relay network) -> crate `relay`

Written before the port. The comparison harness is `tests/golden.rs`.

## References in the original

| Holder | Refers to | Direction | Used to change it? |
|---|---|---|---|
| `Node.hub` | the `Hub` that contains it (`Node(self, nid)`) | **up** | yes: `delivered[nid] +=`, `latency +=`, `dropped += 1`, `traffic[..] += 1`; also reads `clock`, `len(nodes)` |
| `Node.peers` | up to 3 other `Node`s of the same hub | **sideways** | yes: `peer.queue.append(packet)`, `peer.load += size`; `pick` reads `peer.nid`, `peer.load` |
| `Hub.nodes` | every `Node` | down | yes: `inject` appends to `queue`, adds to `load`; `tick` calls `pump` |
| `Node.queue` | `Packet`s | down | yes: `pop(0)`, `append`; `packet.ttl -= 1` |
| local `packet` in `pump` | a `Packet` just popped | down | yes: `ttl -= 1`, then handed to one peer queue |
| `Hub.rng` | `Rng` | down | yes: `below` advances `state` |
| `Hub.delivered`, `Hub.traffic` | lists of ints | down | yes, but only through `Node.hub` |

Mutable containers reachable from more than one place:

- Each node's `queue` and `load`: from `Hub.nodes` and from the `peers` list
  of each of its neighbours.
- `Hub.delivered`, `Hub.traffic`, `Hub.dropped`, `Hub.latency`: from the hub
  and from every node through `Node.hub`.

Not shared: a `Packet` is in exactly one queue at a time (it is moved, never
aliased). `Node.label` is written once and read only by `state()`.

Iteration that changes what it iterates: `for node in self.nodes: node.pump()`
never adds or removes nodes, but each `pump` changes **other** elements
(peer queue and load). Nodes later in the list see, in the same tick, the
packets and loads earlier nodes produced, and may forward such a packet
again in that tick. `self.queue` cannot grow during its own `pump` (a node is
never its own peer).

## Hot path

- Entry point: `Hub.tick` -> `inject` (3 packets) -> `Node.pump` for every
  node in index order -> `Node.pick` per forwarded packet.
- One iteration reads: `clock`, rng state, each node's queue head (up to 4
  packets), the node's peer ids, the peers' loads.
- One iteration writes: rng state, `clock`, `next_pid`; the node's queue and
  load; one peer's queue and load; `sent[nid]`, `delivered[nid]`,
  `traffic[nid*count+peer]`, `dropped`, `latency`; the packet's `ttl`.
- Needed during the same stretch of code: nothing that overlaps. The packet
  is popped (own queue released), own load is reduced, then peers' loads are
  read to get an index, then one peer's queue and load are written. Own node
  and peer are always different nodes and are used one after the other. The
  hub counters are plain integers.
- What one iteration has to create: 3 packets, as values. Nothing else; a
  forwarded packet is moved, and the per-node state already exists.
- **Allocation budget per iteration, after warm-up:** 0, because a tick only
  moves small `Copy` packets between queues that already exist. The only
  heap event possible in a tick is a queue's ring buffer growing when that
  queue exceeds its previous longest length; that is amortised growth to a
  high-water mark, not per-tick work, and "warm-up" means the queues have
  reached it. (Queue lengths are small in practice: the goldens show 0 or 1
  at the end of every run.) The test asserts exactly 0 allocations and 0
  reallocations over a long window after warm-up.

## Structure chosen

- Owner of each piece of data: `Sim` owns everything, as parallel vectors
  indexed by node id: `queues: Vec<VecDeque<Packet>>`, `loads`, `sent`,
  `delivered`, `peers` (fixed `[u32; 3]` plus a length per node), `traffic`
  (`count*count`), `labels` (cold, only for `state()`), plus the scalar
  counters and the rng state. There is no `Node` type holding anything.
- How parts refer to each other: by node index (`u32`). `Node.hub`
  disappears; `Node.peers` becomes a small inline array of indices.
  `Packet` is a 24-byte `Copy` struct (`pid`, `born`, `dst`, `ttl`, `size`)
  stored by value in the ring buffers.
- Where the hot-path logic lives: on `Sim`. `node.pump()` becomes
  `self.pump(i)`, `node.pick(packet)` becomes `self.pick(i, dst)` returning
  a peer index; `inject` is a `Sim` method.
- How the hot path gets what it needs at once: reading and writing are
  separated in time, with only `Copy` values held across: pop a packet (by
  value), update own load, call `pick` (shared borrow of `peers` and
  `loads`, returns an index), then index the peer's queue and load to write.
  Loads are kept in their own dense vector, apart from the queues, because
  `pick` reads three of them per packet and nothing else of those nodes.
  `VecDeque` replaces `list.pop(0)` (O(1) instead of O(n)) with the same
  FIFO order.
- Behaviour that must stay exactly as it is:
  - Nodes pumped in index order, in place; later nodes see earlier nodes'
    forwards within the same tick.
  - `inject` draws `src, dst, ttl, size` from the rng in that order, 3 times,
    before any pump; `clock` is incremented first, so `born == clock`.
  - In `pump`: load reduced before anything else; delivery check before the
    ttl decrement; drop when ttl reaches 0 after decrement; `pick` sees own
    load already reduced and peers' loads as of now.
  - `pick`: scan peers in order; a peer equal to `dst` wins at once;
    otherwise the least load, first such in peer order (strict `<`).
  - Peer order and de-duplication: `(n+1)%c`, `(n-1)%c` (non-negative),
    `(7n+3)%c`, skipping self and repeats. Every node has at least one peer
    since `count >= 2`, so `pick` always finds one.
  - Rng: 64-bit LCG with wrapping arithmetic, `(state >> 33) % n`.
  - Integer range: Python ints do not overflow. Counters are `u64`/`i64`
    and cannot realistically wrap. The weighted checksums in `state()`
    (`(k+1)*v` over `traffic`, `(k+1)*pid` over a queue) are the only values
    that could approach `i64` on an extreme run; they are summed in `i128`
    and converted with a checked conversion, so an out-of-range result
    panics instead of silently differing from Python. `state()` is not hot.

## Rejected

| Alternative | Why not |
|---|---|
| `Rc<RefCell<Node>>` peers and `Weak<RefCell<Hub>>` back-reference (the Python shape) | Nothing has several owners; costs a refcount and borrow flag check per access on the hot path and can panic on re-entrant borrow. |
| `Vec<Node>` with `pump(&mut self, hub: &mut Hub)` | The node is inside the hub's `nodes`, so it needs `mem::take` of the node list or `split_at_mut` gymnastics every tick; the logic needs the container, so it belongs to the container. |
| Array of `Node` structs (queue, load, sent, peers together), indexed | Works for borrowing, but `pick` would pull a whole node's cache line to read one integer; a dense `loads` vector is cheaper. Kept as the fallback if it measured worse. |
| `Vec<Packet>` queue with `remove(0)` like `list.pop(0)` | O(queue length) per pop; `VecDeque` gives the same order in O(1). |
| `Box<Packet>` / packets in a slab with intrusive lists | Packets are 24 bytes and `Copy`; boxing allocates per packet (budget would be 3 per tick), a slab adds indirection for nothing. |
| Collecting the peers' loads or the popped packets into a temporary `Vec` per pump | Allocates on the hot path; breaks the budget and the in-place update order gains nothing from it. |
| Storing `label` in the per-node hot data | It owns heap memory and the hot loop never reads it; kept in a separate cold vector. |

## Changes during the port

2026-11-15, first run of the allocation test (`tests/alloc.rs`): 3 allocations
in 200 000 ticks for count 17 after a 20 000-tick warm-up; the other sizes
were at 0. Cause: the queues started empty (`VecDeque::new()`), so each of
the 17 ring buffers grew 0 -> 4 -> 8 on its own the first time that queue
held a 5th packet, which for some nodes is tens of thousands of ticks in.
The ownership structure was right; what was missing from the note was
**when the queue buffers get their capacity**. Measured with `relay.py`
over 300 000 ticks (counts 2, 4, 5, 17) and fewer for 64 and 500: the
longest queue ever seen is 7, and the record grows about one step per
tenfold more ticks.

Replacement: `Sim::new` reserves `QUEUE_RESERVE = 32` packets per queue
(768 bytes per node, paid once at construction, next to a `count*count`
traffic table). The budget stays **0 per tick** and was not raised; the
test now asserts exactly 0 over 200 000 ticks after only a 1 000-tick
warm-up, for counts 2, 5, 17, 64 and 500.

What is not guaranteed: nothing in `relay.py` bounds a queue's length, so
a queue that one day exceeds 32 packets grows its buffer once (one
reallocation, then never again at that size). That is a one-off cost of an
unbounded queue, not per-tick work; a hard cap would change behaviour.

Counts in the functions reachable from `tick` (`inject`, `pump`, `pick`,
`Rng::below`): `clone` 0, `collect` 0, `to_vec` 0, `to_owned` 0,
`to_string` 0, `mem::take` 0, `mem::replace` 0. The three `collect`s in the
crate are in `Sim::new` (peers, queues, labels). `Packet` is `Copy`; it is
moved out of one queue and into another, never duplicated.
