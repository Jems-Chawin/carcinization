# Fixture for carcinify speed

- `py/speed_orig.py`, the original: `nap(ms)` sleeps `ms` milliseconds.
- The port is slow on purpose, so the expected ratio is known: its `nap`
  sleeps ten times as long, so original ÷ port is about 0.1.
  - `py-port/speed_port_py.py` is a Python port, for the tests that need
    no build (`suite-python.json`);
  - `rs/` is the same port in Rust behind PyO3, built by maturin
    (`suite.json`).
- `wrong(n)` returns another value in the Python port, to be caught.
- `basename` is in both modules' namespace, imported from `os.path`: not
  their own code, so carcinify diff refuses it as a case, and speed too.
- `pop_first(xs)` changes its argument: on one list reused, its fourth
  call would raise.
- `hang()` takes 60 s a call, far longer than the time limit its test gives.
- `square(n)` is cached in the original (`functools.lru_cache`): every
  timed call would be a cache hit, so carcinify diff refuses it, and speed
  too.
- `first(xs)` returns the list's first item; the Python port pops it,
  which the original does not.
- The ports' `nap` makes a workspace on a thread's first call, in 5 ms;
  the PyO3 port's `workspaces()` counts them. carcinify speed's cold
  thread measures it.
- `zero()` has no required parameter, so a call with no arguments runs it.
- The Python port's `lookup` finds its table on the importing thread
  only, so it raises on a new thread where the original does not.
  `main_only` raises off the main thread on both sides. The port's
  `doubled` finds its factor on the importing thread only, so it
  answers otherwise on a new thread.
- The Python port's `lazy` imports its sibling `lazy_helper` only when
  called.
- The Python port's `fails` raises ValueError where the original
  returns.
- The PyO3 port's `fill(n)` makes n boxes in one vector, all freed: n + 1
  allocations and 16n bytes. Its first call also makes a 4096-byte
  workspace and keeps it. Built with the feature `count_allocations`, the
  crate counts its allocations and exports `_speed_allocations()`.
- `temp(n)` makes a Python tuple and a dict, drops both, and returns n.
- `options(**kwargs)` counts its keyword arguments; the case
  options-odd-key gives one whose key, a-b, cannot be written as a name.
- The PyO3 port's `blob(n)` returns an object that owns n bytes of Rust
  memory and keeps nothing; its `log_push(n)` keeps every n in a Vec that
  doubles as it fills.
- `crash(n)` returns n in the original; the Python port kills its own
  interpreter (SIGKILL), which leaves no crash report behind.
- `callers/` holds caller targets: `callers.py` calls `library.py`, which
  reaches `nap` or `wrong` through its own module, where carcinify speed
  puts each side's function. `two_naps` reaches the case nap-2;
  `a_nap_no_case_has` reaches nap(3), which no case has;
  `naps_that_change` reaches nap(2) and then nap(0);
  `a_nap_set_by_the_environment` reaches nap(2), or the nap
  SPEED_FIXTURE_NAP names; `a_set_of_letters` reaches size-letters, a set
  of strings whose order follows the hash seed, or the letters
  SPEED_FIXTURE_LETTERS names; `a_wrong` reaches wrong-1.
- `outside.jsonl` is a case file that no suite lists.
