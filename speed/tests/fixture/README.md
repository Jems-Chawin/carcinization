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
- `outside.jsonl` is a case file that no suite lists.
