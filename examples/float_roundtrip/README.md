# Worked example: the float that came back different

`python/ledger.py` is the original: it writes records
`{"id", "name", "amount"}` one JSON object per line and reads them back.
`rust/` is its port: `core` is the store in plain Rust, `py` exposes it to
Python through PyO3 0.30 as `ledger_rs`.

The port has a bug found in a real port. `serde_json`, as it comes,
reads some floats one bit off. The Cargo feature `float-roundtrip` turns on
`serde_json/float_roundtrip`, which fixes it. The two suites differ in that
feature and nothing else.

From the repository root, with maturin 1.16 in `.venv`
(`uv venv && uv pip install maturin==1.16.0`):

```sh
cargo build --release
run() { target/release/carcinify diff examples/float_roundtrip/suite-$1.json \
    --report target/carcinify-diff/$1.json \
    --python .venv/bin/python --maturin .venv/bin/maturin; }

run buggy   # exit 1
run panic   # exit 1: a Rust panic, as Python sees it
run fixed   # exit 0
```

The buggy run starts:

```
DIFFERENT data-file-ledger (cases/real-data.jsonl:1)
  at $[2]["amount"]: value
    original   float 396.03999999999996  bits 0x4078c0a3d70a3d70
    candidate  float 396.04  bits 0x4078c0a3d70a3d71
    in the record  "id": 1003, "name": "port fees"
```

`$[2]["amount"]` is the `amount` of the third record that `load` returned,
which is line 3 of `data/ledger.jsonl`.

## The cases

- `cases/real-data.jsonl`: `load` on `data/ledger.jsonl`, a file the original
  wrote.
- `cases/roundtrip.jsonl`: 67 cases for `roundtrip`, written by
  `gen_cases.py` from a fixed seed. Edge values (`-0.0`, subnormals, the
  largest float, an id of 2**63 - 1, names in NFC and NFD), 40 random batches
  of 25 records, and 11 inputs the original rejects (NaN, infinity, an id of
  2**70, a negative id), where the port must raise the same `ValueError` with
  the same message.

## The panic case

`first_amount` returns the amount of the first record. The Python raises
`IndexError` for an empty list. The port indexes without checking, on
purpose, so an empty list is a Rust panic. `suite-panic.json` has two cases:
one record, which is identical, and none, which is reported as

```
DIFFERENT no-records (cases/panic.jsonl:2)
  at $: exception_type
    original   raised builtins.IndexError: "list index out of range"
    candidate  raised pyo3_runtime.PanicException: "index out of bounds: the len is 0 but the index is 0"
    original is builtins.IndexError < builtins.LookupError < builtins.Exception < builtins.BaseException; candidate is pyo3_runtime.PanicException < builtins.BaseException
```

`PanicException` derives from `BaseException`, not `Exception`, so a
caller's `except Exception:` does not catch it. The interpreter survives:
the side is not restarted.

## Not covered

- `save` is not called by any case; it runs only inside `roundtrip`. (In the
  buggy and fixed suites `first_amount` is not called either; it has
  `suite-panic.json`.) The
  harness lists it under `public_functions_not_called` for both sides.

- Names holding lone surrogates. The original stores them; the port cannot
  take them as a Rust `String`. No case exercises this, so the fixed suite
  passing says nothing about it.
- The bytes of the file on disk. The two write the same numbers differently
  (`1e+22` and `1e22`) and each reads the other's. Only what `load` returns is
  compared.
- A file with an integer where a float should be (`"amount": 5`). The
  original would return an int, the port a float. Neither writes such a file.

## Checks for the Rust side

```sh
cd examples/float_roundtrip/rust
cargo fmt --check && cargo clippy --all-targets -- -D warnings
cargo test                              # asserts the bug is present by default
cargo test --features float-roundtrip   # asserts the fix holds
```
