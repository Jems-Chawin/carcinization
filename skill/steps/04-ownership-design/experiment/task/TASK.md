# Task: port `relay.py` to Rust

`relay.py` in this directory simulates a packet relay network. It is too
slow. `Hub.tick` is the hot path: in production it is called millions of
times on a network that stays alive for the whole run. The reason for the
port is speed.

Port it to Rust in the crate in this directory.

## Requirements

- Keep the public interface in `src/lib.rs` exactly as it is: `Sim::new`,
  `Sim::tick`, `Sim::state` and `run`, with those signatures. Everything
  else is yours to design: fields, private functions, more modules or files.
- The results must be exactly those of the Python. `tests/golden.rs` holds
  outputs computed by `relay.py`; do not change that file. It must pass.
- The standard library only: no dependencies in `Cargo.toml`.
- These must pass when you finish:
  `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test`.
- Work only inside this directory. Do not read other directories on this
  machine and do not use the network; everything you need is here.

## When you are done

Write `REPORT.md` in this directory: a few lines on how you structured the
port, anything you were unsure about, and the output of the three commands
above. Your final message should say the same in brief.
