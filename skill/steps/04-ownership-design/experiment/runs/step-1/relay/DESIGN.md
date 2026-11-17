# Ownership design: `relay.py` (packet relay network) -> `relay` crate

Written before the port. Half a page to a page.

## References in the original

| Holder | Refers to | Direction | Used to change it? |
|---|---|---|---|
| `Hub.nodes` | every `Node` | down | yes: `inject` appends to `node.queue`, adds to `node.load`; `tick` calls `node.pump()` |
| `Hub.rng` | `Rng` | down | yes: `below` advances the state |
| `Node.hub` (**back-reference**, `Node(self, nid)`) | the `Hub` | **up** | yes: `hub.delivered[nid] +=`, `hub.latency +=`, `hub.dropped +=`, `hub.traffic[..] +=`; reads `hub.clock`, `len(hub.nodes)` |
| `Node.peers` (list of `Node` objects) | up to 3 other `Node`s | **sideways** | yes: `peer.queue.append(packet)`, `peer.load += size`; `pick` reads `peer.nid`, `peer.load` |
| `Node.queue` | `Packet`s | down | yes: `pop(0)`, `packet.ttl -= 1`; the same packet object is then moved to a peer's queue |
| `Node.label` | a `str` | down | no: read only by `state()` |

Mutable containers reachable from more than one place:

- each `Node` (its `queue` and `load`): from `Hub.nodes` and from the `peers`
  list of up to three other nodes;
- `Hub.delivered`, `Hub.traffic`, and the counters `dropped` / `latency`:
  from the hub and, through `node.hub`, from every node.

A `Packet` is only ever in one queue at a time (popped, then appended
elsewhere or discarded), so it has one holder despite being a Python object.
No container is changed while it is being iterated: `tick` iterates
`hub.nodes`, which never changes length; `pump` changes *other elements*
(peers' queues and loads), and later nodes in the same tick see those changes.

## Hot path

- Entry point: `Hub.tick` -> `inject` (3 packets) -> `Node.pump` for each node
  in index order -> `Node.pick` for each forwarded packet (<= 4 per node).
- One iteration reads: `clock`, node count, the rng; per node its own queue
  head, its `nid`, its peer ids; per forwarded packet the `load` of each of
  its <= 3 peers.
- One iteration writes: rng state, `clock`, `next_pid`; queues and loads of
  the injected-into nodes; per node its own queue and load, `sent`, one
  peer's queue and load, and the hub's `delivered`, `latency`, `dropped`,
  `traffic`.
- Needed during the same stretch of code: while handling one packet of node
  `i`, the code pops from queue `i`, reads the loads of `i`'s peers, then
  pushes to the queue of one peer `j` and bumps hub counters. `j != i` always
  (a node is never its own peer). Nothing needs queue `i` and queue `j`
  borrowed *simultaneously*: the packet is a small value that is fully out of
  queue `i` before queue `j` is touched. `pick` needs only the peer id list
  and the load array, never the queues.
- What one iteration has to create: nothing lasting. Three packets come into
  being per tick, but they are 24-byte plain values written into queue
  storage that already exists; packets are moved between queues, never
  copied for keeps. `state()` builds a `Vec` but is not on the hot path.
- **Allocation budget per iteration, after warm-up:** 0, because a tick only
  moves fixed-size packet values between queues that already own their
  storage, and updates integers in arrays sized at construction. "Warm-up"
  means: each queue's ring buffer has reached its high-water capacity. A
  queue may reallocate only when its length exceeds every length it has had
  before (amortised doubling), which is growth of the state, not per-tick
  work. I cannot prove a bound on queue length from the code (service rate
  4/node/tick against 3 injected/tick with ttl <= 15 keeps them short in
  practice), so the queues are not presized to a "maximum"; the test warms up
  and then asserts exactly 0 over a long window.

## Structure chosen

- Owner of each piece of data: `Sim` owns everything, as parallel arrays
  indexed by node id: `queues: Vec<VecDeque<Packet>>`, `loads: Vec<u64>`,
  `sent: Vec<u64>`, `links: Vec<Links>` (peer ids), `labels: Vec<String>`,
  plus `delivered`, `traffic`, the scalar counters and the rng state. There
  is no `Node` struct; a node is an index.
- How parts refer to each other: a peer is a `u32` node id stored inline in
  `Links { ids: [u32; 3], len: u8 }` (`Copy`, no heap). The `Node.hub`
  back-reference disappears: the code that needed it is a method of `Sim`.
  `Packet { pid, born, dst, ttl, size }` is `Copy` plain data.
- Where the hot-path logic lives: `Node.pump` becomes `Sim::pump(&mut self,
  i)`; `Node.pick` becomes a free function `pick(links, &loads, dst) ->
  usize` taking exactly the two things it reads; `inject` is a `Sim` method.
- How the hot path gets what it needs at once: reading and writing are
  separated in time and only small values are kept across. `pop_front` on
  `queues[i]` yields a `Packet` by value (borrow ends); `links[i]` is copied
  out (13 bytes, `Copy`); `pick` borrows `loads` shared and returns an index;
  then `queues[j].push_back`, `loads[j] +=` and the counters are written one
  after the other. No two elements of one collection are borrowed at once, so
  no `split_at_mut`, no `Rc`/`RefCell`, no `mem::take`, no clones.
  Data is split by use: `pick` walks only `loads` (dense `u64`s) and the
  inline peer ids; `labels` (the only heap-owning per-node data besides the
  queues) is in its own array that the tick never touches.
- Behaviour that must stay exactly as it is:
  - rng: 64-bit wrapping LCG, result `(state >> 33) % n`; call order per
    injected packet is src, dst, ttl, size;
  - `clock` is incremented before `inject`; `born` is the new clock;
  - nodes are pumped in index order, in place: a packet forwarded to a
    higher-numbered node can be forwarded again in the same tick, and loads
    seen by `pick` include changes made earlier in the tick;
  - `pump` takes at most `BURST` packets, FIFO; delivered and dropped
    packets still use up budget; `load` is reduced before anything else;
  - delivery is checked before ttl; ttl is decremented then tested `== 0`;
  - `pick`: scanning peers in order, return the destination at once if met
    (even if an earlier peer was less loaded), else the first peer with the
    strictly smallest load;
  - peer order: `(n+1)%c`, `(n-1)%c` (Python modulo, so `c-1` for node 0),
    `(7n+3)%c`, skipping self and duplicates;
  - `label` is `"relay-%04d"`; `state()` sums its character codes;
  - integers: Python's are unbounded. The rng is masked to 64 bits by the
    source. Everything else (`latency`, `traffic`, `next_pid`, loads) is
    `u64`: at millions of ticks these stay far below 2^63, and `state()`
    converts to `i64`. The weighted sums in `state()` are the first thing
    that would leave `i64` on an absurdly long run; no golden case reaches
    that and I do not handle it.

## Rejected

| Alternative | Why not |
|---|---|
| `Rc<RefCell<Node>>` peers plus `Weak<RefCell<Hub>>` back-reference (the Python's shape) | Nothing here has several owners; it would add refcount and borrow-flag traffic to every forwarded packet and runtime borrow panics in place of compile-time checks. |
| `struct Node { queue, load, sent, peers, label }` in a `Vec<Node>` with `Sim::pump(i)` | Works with indices, but `pick` would stride over whole nodes (queue header, label `String`) to read one `load` each; parallel arrays keep what `pick` reads dense and make the disjoint-field borrows obvious. |
| `Vec<Packet>` queues with `remove(0)` (literal `list.pop(0)`) | O(queue length) shift per pop; `VecDeque` is the same FIFO in O(1). |
| One shared packet slab with intrusive per-node linked lists and a free list | One allocation instead of `count`, but worse locality when scanning a queue in `state()`, more code, and no fewer allocations after warm-up (both are 0). |
| `Vec<Vec<u32>>` peers | A heap block per node for at most 3 ids; an inline `[u32; 3]` is `Copy` and can be held across the writes for free. |
| Presizing every queue to a fixed large capacity to make "0 allocations" hold from tick 1 | No derivable bound on queue length, so the number would be a guess; the warm-up definition above is honest about it. |

## Changes during the port

2026-11-15, after the first measurement. The structure above went in
unchanged and there were no borrow errors. What was wrong was the first
allocation test, not the design: it used a fixed warm-up of 20 000 ticks and
asserted 0 allocations over the next 200 000. It failed for one of five
networks (count=17, seed=99: 3 allocations in 200 000 ticks). That is the
case the budget paragraph already names (a queue reaching a new high-water
length), but a fixed tick count is not the note's definition of warm-up, so
the test was checking something the note never promised. The budget is not
raised. The test was replaced by one inside the crate (`src/alloc_test.rs`)
that checks the stated budget exactly: on every measured tick, the number of
allocations equals the number of queues whose capacity grew in that tick,
so any allocation that is not a high-water growth fails it; and such ticks
must be fewer than 1 in 10 000. Measured: 0, 0, 3, 0, 0 allocations in
200 000 ticks for the five networks.

A consequence worth knowing: because queue lengths have no derivable bound,
"zero allocations forever after N ticks" cannot be promised for any N; a
rare long excursion of one queue costs one reallocation (the queue's
capacity then stays, `VecDeque` never shrinks).
