# Task: port `depot.py` to Rust

`depot.py` in this directory is one module of a larger system written in
Python. We are moving it to Rust. Port it, in the crate in this directory.
`src/lib.rs` is empty; add modules and files as you see fit.

## Requirements

- **The callers stay.** `callers.py` shows how the rest of the system uses
  the module: it keeps the objects, reads them later and changes them.
  `tests/callers.rs` is that code written against the Rust port, and it is
  the interface the port has to offer. It must compile and pass as it is;
  do not change it or the files in `tests/golden/`.
- **Keep the structure of the original.** The port will be reviewed side by
  side with `depot.py` by people who know the Python well and Rust less.
  A reviewer should find each class and each method of the original in the
  Rust, doing the same job.
- **The same results.** For any arguments the port must give exactly what
  the Python gives, not only for the ones in the tests.
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
