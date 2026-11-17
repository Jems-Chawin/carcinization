# Structure report: notes for Python sources

Python-specific help for part 5 of `STEP.md`.

## Timing from the Python caller

Time the call the way the application makes it, original and port side by
side, on the same input, in the same interpreter:

```sh
python -m timeit -v -n 20 -r 7 -s "import ledger, data; rows = data.load()" "ledger.total(rows)"
python -m timeit -v -n 20 -r 7 -s "import ledger_rs, data; rows = data.load()" "ledger_rs.total(rows)"
```

- `-r 7` repeats and `-v` prints the time of every repeat; without `-v`
  only the best is shown and the spread is lost. Keep the full output with
  the figure, and give the best and the worst of the seven.
- Build the extension in release mode (`maturin build --release`, which is
  what the harness does). A debug build can be slower than the Python.
- Include converting the arguments. If the application passes a list of
  dicts, time passing a list of dicts; that conversion is part of what the
  user waits for, and for small inputs it can be most of it.
- Do not time through `carcinify diff` unless it is run with
  `--no-call-counts`: its counting of Python calls slows the Python side by
  up to about nine times and would be booked as a gain of the port.

## A same-algorithm baseline

If the port replaced a scan with a lookup, a list with a set, or repeated
string building with one join, make that change in the Python and time it
too. The table then has three rows: the original, the original with the
better algorithm, the port. The second to the third is what the language
gave.
