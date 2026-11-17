# Deciding: notes for Python sources

Python-specific help for `STEP.md`.

## The workload file

```python
# bench/workload.py
import importer

def run(count):
    """One import of `count` records."""
    return importer.import_file(make_records(count))
```

The tool loads the file and calls the function with the arguments from the
declaration. Data built when the file is loaded is not timed; data built in
the function is. Say in a comment which you chose and why.

Run the tool with the project's own interpreter (`--python
.venv/bin/python`) so that the libraries are the ones in use.

## Reading the shares

- **Waiting** is wall time the process did not spend on the CPU:
  `time.sleep`, sockets, files, locks, a subprocess, a GPU. With `asyncio`
  the waiting shows the same way.
- **Compiled code** is the interpreter's built-ins and C extensions when a
  call takes long enough to be real work: `zlib.compress`, `sorted` on a
  large list, `re` on a large string, a database driver.
- A function that uses NumPy or a package like it will be counted as the
  project's own Python for work the package does. That holds for operators
  (`a @ b`, `x * 2 + y`), which no instrument sees, and for calls such as
  `np.dot(a, b)` and `np.add(a, b)`: these are objects of NumPy's own
  types, not built-in functions, and `cProfile` reports only built-in
  functions. On Python 3.12 and later the tool times such calls with
  `sys.monitoring`; on older Pythons it cannot.
- For such a candidate the report gives bounds, not a share. To get a
  lower bound, write a stand-in workload:

  ```python
  class Shape:
      """Stands in for an array: the product costs nothing."""
      def __init__(self, a):
          self.first = a[:1, :1].copy()
      def __matmul__(self, other):
          return self.first

  def run_with_stand_ins(n, times):
      a, b = make_matrices(n)
      return mat.products(Shape(a), Shape(b), times)
  ```

  For the call form, replace the function for the length of the run
  (`np.dot = lambda a, b: ready_made`) and put it back. Keep every line of
  the candidate's own Python running.

  Replace every array the candidate touches, wherever it comes from: its
  arguments, module-level constants, attributes of objects it is given. The
  tool refuses a stand-in workload in which an object of the package larger
  than 64 KiB still reaches the candidate. A stand-in may hand back a small
  array (`a[:1, :1].copy()`), never a view of a large one.
- If the project imports NumPy anywhere and the candidate is plain Python,
  the tool still asks you to say so (`candidate.uses_no_compiled_package`)
  before a port for speed passes. Read the candidate first: an array can
  arrive in an attribute, a default argument, or the result of a call.
- The other way is a line profiler such as `line_profiler`, installed in a
  scratch virtual environment with a copy of the project's dependencies,
  not in the project's own environment. A line that is one array operation
  is the package's time.
- Building large strings and arithmetic on big integers are compiled work
  too, reached by operators, and are shown as the project's own Python.
  Nothing flags them.
- The profile covers the main thread. Work in a thread pool or a process
  pool appears as waiting, with the children's CPU time given beside it.

## What crosses the boundary well and badly

- Well: numbers, `bytes`, `str`, and buffers (`bytes`, `memoryview`,
  arrays) handed over whole.
- Badly: a `list` of `dict`s of small objects, each of which has to be
  looked at and converted. If the candidate takes one, the port will want
  to take the raw input (the file, the bytes) and build its own structure,
  or return a handle to data kept on the Rust side.
- A function entered millions of times for a fraction of a microsecond is
  not a candidate, however hot. The loop around it is.

## Free-threaded Python

On a free-threaded build, CPU time can exceed wall time for the main
thread's own work if threads run in parallel. The tool flags more CPU than
wall and profiles the main thread only.
