# Ownership design: <what is being ported>

Written before the port, and committed on its own before the first Rust
file. Half a page to a page.

## References in the original

| Holder | Refers to | Direction | Used to change it? |
|---|---|---|---|
| | | up / sideways / down | yes: how / no |

Mutable containers reachable from more than one place:

Are elements removed or reordered while something refers to them?
<no / yes: where>

Is anything compared by identity (the same object, not an equal one)?
<no / yes: where>

Does anything get read after it has been removed from its collection?
<no / yes: what is read>

## What code outside the port holds

| Object | Who holds it | What they do with it |
|---|---|---|
| | | only read / change it / expect to see changes made elsewhere / compare or key by identity |

## Hot path

- How it was identified: <profile command, input, share of time; or "by
  reading, because ...">
- Entry point:
- One iteration reads:
- One iteration writes:
- Needed during the same stretch of code:
- What one iteration has to create:
- **Allocation budget per iteration, after warm-up:** <number>, because <reason>.
- **Driver, the workload the budget is measured on:** input <what it is and
  why it is representative>; built by <constructor> at sizes <two or more,
  the larger near what production sees>; one iteration is <the call>;
  <n> iterations to warm up, because <reason>; <n> counted.

## Structure chosen

- Owner of each piece of data:
- How parts refer to each other: <plain index, because nothing is removed
  or reordered / generation-checked handle / stable id>
- Where the hot-path logic lives:
- How the hot path gets what it needs at once:
- What crosses the boundary to callers, as a copy or as a handle: <the list
  the binding step starts from>
- Behaviour that must stay exactly as it is (order of updates, ties, ...):

## Rejected

| Alternative | Why not |
|---|---|

## Changes during the port

<Date or commit: what turned out wrong in the above, and what replaced it.>
