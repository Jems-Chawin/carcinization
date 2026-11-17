<!-- Saved by the experimenter. This sub-agent could not write REPORT.md
(the file write was refused for it), so this is its final message, word for
word. It is not part of what the run produced on disk. -->

The port is done and all three checks pass, but `REPORT.md` was not written: the harness refused the file write for a report from a subagent, and I did not work around it. Its content is in this message instead.

## Structure

The design note was written first, in `DESIGN.md`, and the port follows it.

- **Ownership:** `Sim` owns everything: a `Vec<Node>` of plain numbers and one `VecDeque<Packet>` per node.
- **References:** the Python back-reference `Node.hub` is gone, and peers are node indices (`[u32; 3]` plus a count) in peer order.
- **Logic:** `pump` and `pick` moved from the node to `Sim::pump(nid)` and a free `pick(&nodes, ..)`, because both need more than one node.
- **Packets:** `Packet` is a 24-byte `Copy` value that moves between queues.
- **Hot path:** it reads and writes by index, one after the other, so nothing is cloned, taken out of `self`, or wrapped in `Rc`/`RefCell`. No borrow error came up during the port.

Two things are represented differently from the Python, with identical results:

- **Label:** the string is not stored; `state()` computes its character sum from the node id.
- **Traffic matrix:** it is kept as one counter per edge inside each node, and `state()` forms the same weighted sum from those. The `count * count` matrix is zero everywhere else.

## Allocation budget

The budget set in `DESIGN.md` before the port is 0 allocations per tick after warm-up. `tests/alloc.rs` (a counting global allocator) measured 0 allocations in 100,000 ticks after 1,000 warm-up ticks, for networks of 2, 5, 17, 64 and 1000 nodes.

Of the copy-like calls the step lists (`clone`, `collect`, `to_vec`, `to_owned`, `to_string`, `mem::take`, `mem::replace`), `src/lib.rs` has one `collect`, in `Sim::new`. None is reachable from `tick`.

## Unsure about

- **Queue growth:** a queue allocates if it grows past its previous largest size (starting capacity 16). The test saw none, but nothing proves a bound on queue length for every seed and size, so "0 after warm-up" is measured, not guaranteed.
- **Integer range:** Python integers do not overflow; the port uses 64-bit counters with ordinary arithmetic. The largest values are the weighted sums in `state()`, roughly 1e14 for 64 nodes and 1e9 ticks. An overflow would panic in a debug build and wrap in a release build; the golden file has no case near that range.
- **Node count:** `Sim::new` asserts `count >= 2` and that `count` fits in `u32`. The Python would accept `count` 1 and fail on the first forward; the port panics at construction.
- **Speed:** I did not benchmark against the Python or build in release mode, so the speed gain is not measured.

## Command output

`cargo fmt --check`: no output, exit 0. `cargo clippy --all-targets -- -D warnings`: finished, no warnings. `cargo test`: 1 passed in `tests/alloc.rs` (`tick_allocates_nothing_after_warm_up`), 6 passed in `tests/golden.rs`.
