# Ownership design: `relay.py` (Hub / Node / Packet / Rng) -> `relay::Sim`

Written before the port. Half a page to a page.

## References in the original

| Holder | Refers to | Direction | Used to change it? |
|---|---|---|---|
| `Node.hub` | the `Hub` that contains it | **up** | yes: `delivered[nid] +=`, `latency +=`, `dropped +=`, `traffic[..] +=`; also reads `clock`, `len(nodes)` |
| `Node.peers` | up to 3 other `Node`s | **sideways** | yes: `peer.queue.append(packet)`, `peer.load += size`; also reads `peer.nid`, `peer.load` |
| `Hub.nodes` | every `Node` | down | yes: `inject` appends to `node.queue`, adds to `node.load`; `tick` calls `pump` |
| `Node.queue` | `Packet`s | down | yes: `packet.ttl -= 1`; the packet object is moved (not copied) from one queue to a peer's |
| `Hub.rng` | `Rng` | down | yes: `below` advances the state |
| `Node.label` | `str` | down | no: built once, read only by `state()` |

Mutable containers reachable from more than one place: every `Node`
(its `queue` and `load`) is reachable from `Hub.nodes` and from the `peers`
list of each neighbour; `Hub.delivered`, `Hub.traffic` and the scalar
counters are reachable from the hub and from every node through `node.hub`.
A `Packet` is only ever in one queue at a time (popped before it is
appended elsewhere), so packets have a single holder already.

## Hot path

- Entry point: `Hub.tick` -> `inject` (3 packets) -> `Node.pump` for each node in index order.
- One iteration reads: rng state, clock, the node's queue head, the node's peer ids, the `load` of each of its peers, the packet's fields.
- One iteration writes: rng state, `clock`, `next_pid`; the node's own queue (pop front), `load`, `sent`; one peer's queue (push back) and `load`; `delivered[nid]`, `latency`, `dropped`, `traffic[nid*count+peer]`.
- Needed during the same stretch of code: in `pump`, node `i` is being changed (queue, load, sent) while a *different* node `p` is read (load, in `pick`) and then changed (queue, load), and hub counters are changed. No list is appended to while it is iterated: `peers` never contains the node itself, so a node never pushes onto its own queue; `Hub.nodes` never changes length. Nothing has to be held across a write except the popped packet (a small value) and the chosen peer's index.
- What one iteration has to create: nothing lasting except the 3 injected packets, which are small plain values stored in queue storage that already exists. `pick` and `pump` create nothing.
- **Allocation budget per iteration (one `tick`), after warm-up:** 0, because a tick only moves fixed-size packet values between existing queues and adds to existing counters. The one legitimate allocating event is a queue growing past the largest length it has ever had; that is amortised doubling, it happens O(log max-queue-length) times per node over a whole run, not per tick, and it is what "warm-up" means here. Queues start with capacity 16 (allocated in `Sim::new`, off the hot path) so that typical runs never grow at all. The allocation test asserts exactly 0 over a long window after warm-up.

## Structure chosen

- Owner of each piece of data: `Sim` owns everything. Nodes stop being objects and become an index `0..count` into parallel columns owned by `Sim`: `load: Vec<u64>`, `sent: Vec<u64>`, `queues: Vec<VecDeque<Packet>>`, `peers: Vec<Peers>`, plus `delivered`, `traffic`, the rng and the scalar counters. `Packet` is a `Copy` struct of integers (pid, born, dst, ttl, size), owned by whichever queue it is in. Labels are cold data (`labels: Vec<String>`), in their own column, touched only by `state()`.
- How parts refer to each other: by node index (`u32`). `Peers` is a fixed `[u32; 3]` plus a length, in the Python's peer order. `Node.hub` disappears; `nid` is the index itself.
- Where the hot-path logic lives: `pump` becomes `Sim::pump(&mut self, i)`, because it needs the whole network. `pick` becomes a free function taking exactly what it reads: the node's `Peers`, the `load` slice, and the destination; it returns an index.
- How the hot path gets what it needs at once: reads and writes are separated in time and by field. Pop the packet from `queues[i]` by value (borrow ends), update `load[i]`; `pick` reads `peers[i]` and `load` with shared access and returns a `u32` (borrow ends); then `queues[p].push_back(packet)` and `load[p] += size`. Two nodes are never borrowed mutably together: indices are used one after the other. The columns are separate fields, all accessed inside `pump` itself, so the compiler sees them as disjoint. The dense `load` column also keeps `pick`'s reads in one small array.
- Behaviour that must stay exactly as it is:
  - order within a tick: `clock += 1`, then inject (rng draws in order src, dst, ttl, size), then nodes pumped in index order, so a later node sees packets forwarded by earlier nodes in the same tick, and a packet can hop several nodes in one tick;
  - `pump`: at most `BURST` pops; `load[i]` is reduced *before* anything else; delivery is checked before the ttl decrement; ttl reaching 0 drops; `sent`/`traffic` counted only on a forward;
  - `pick`: destination wins if it is a neighbour (first match in peer order, even if a less loaded peer was seen earlier); otherwise strictly-less comparison, so the first least-loaded peer in peer order wins ties;
  - peer order and de-duplication: `(nid+1)%count`, `(nid-1)%count` (non-negative modulo), `(nid*7+3)%count`, skipping self and repeats;
  - the rng is 64-bit wrapping (`& MASK`), `below(n) = (state >> 33) % n`;
  - `label` is `"relay-%04d"`, wider than 4 digits when `nid >= 10000`;
  - integer range: Python integers do not overflow. Hot-path counters are `u64` (cannot realistically overflow: they grow by small amounts per tick). The weighted sums in `state()` are computed in `i128` and converted to `i64` with a checked conversion that panics rather than wrapping silently; the public signature fixes `i64`.

## Rejected

| Alternative | Why not |
|---|---|
| `Rc<RefCell<Node>>` for nodes, `Weak` back to the hub | Keeps the Python's shape; nothing here really has several owners. Adds refcount traffic and runtime borrow checks on every hop, and pointer-chasing in `pick`. |
| `struct Node { queue, peers, load, sent, label }` in one `Vec<Node>` with `split_at_mut` / `get_disjoint_mut` for node and peer | Works, but needs two simultaneous mutable borrows that the logic does not require (sequential indexing is enough), and puts the `String` label and the queue header between the `load` values that `pick` scans. |
| `Node::pump(&mut self, hub: &mut Hub)` | The node is inside the hub; the method cannot have both. Logic that needs the container belongs to the container. |
| Collecting forwards into a per-tick `Vec` and applying them after the loop | Allocates or needs a scratch buffer, and changes behaviour: later nodes must see earlier nodes' forwards within the same tick, and `pick` must see up-to-date loads. |
| `Vec<Packet>` queue with `remove(0)` as in Python | O(n) per pop; `VecDeque` gives the same FIFO order in O(1). |
| One shared packet arena with intrusive per-node linked lists | Also zero-allocation, but more code and worse locality than a ring buffer per node for 24-byte `Copy` packets; no measured need. |
| `Box<Packet>` / packets by reference | A packet has one holder at a time and is 24 bytes; moving the value is cheaper than an allocation per packet. |
| Storing only the label's character sum | Would be enough for `state()`, but drops data the original keeps; a cold `Vec<String>` costs nothing on the hot path. |

## Changes during the port

2026-11-15: nothing in the above turned out wrong. The port compiled without a borrow error and without any workaround. Measured afterwards: 0 allocations in 100,000 ticks after a 1,000-tick warm-up, for counts 2, 5, 17, 64 and 500 (`tests/alloc.rs`), which is the budget. In `src/lib.rs` there is no `clone`, `to_vec`, `to_owned`, `to_string`, `mem::take` or `mem::replace`; the two `collect` calls are in `Sim::new` (building the queues and the labels), not reachable from `tick`.
