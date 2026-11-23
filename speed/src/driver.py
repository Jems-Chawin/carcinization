"""carcinify speed's timing driver for Python.

Started by `carcinify speed run` as `python -s driver.py`, with
PYTHONHASHSEED and PYTHONSAFEPATH set and every other PYTHON* variable
removed, as carcinify diff starts its sides. It reads one plan as JSON on
stdin and writes its result as JSON to the file the plan names, so that
nothing the timed functions print can reach it.

Both sides run in this one process, the original's module and the port's
imported side by side. For each target, after one untimed call of each
side, the sides alternate in pairs of runs, original then port, then port
then original (ABBA). A run is `calls_per_run` calls, timed together, after
an untimed gc.collect(); a loop of pure computation (decide's) is timed
before and after each run. Started again for a target that did not fail,
with "cold" in the plan, it makes the cold thread instead: in new threads, a
cheap call into the port's module, then the port's first call there and its
next, each timed alone. The driver judges nothing: the figures are
carcinify speed's.

The arguments are decoded, and the outputs encoded, by carcinify diff's own
driver. Runs on Python 3.9 and later. Standard library only.
"""

import functools
import gc
import importlib
import importlib.machinery
import importlib.util
import inspect
import json
import keyword
import os
import platform
import sys
import sysconfig
import threading
import time
import traceback
import tracemalloc
import unicodedata


def load_file(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def import_side(side):
    """The side's module, from the side's own directories and nowhere else.
    They stay on sys.path, as in the process carcinify diff gives each side,
    so that what the module imports when called is found there: speed build
    has refused any top-level name the two sides would find in different
    files (see `mixed`)."""
    sys.path[:0] = side["sys_path"]
    module = importlib.import_module(side["module"])
    where = os.path.realpath(getattr(module, "__file__", None) or "")
    roots = [os.path.realpath(r) for r in side["roots"]]
    if not any(where.startswith(r + os.sep) for r in roots):
        raise RuntimeError("%s was loaded from %r, outside %r" % (side["module"], where, roots))
    return module


def origin(spec):
    """Where a module spec loads from: its file, a namespace package's
    directories, or how the interpreter has it built in."""
    if spec.has_location:
        return os.path.realpath(spec.origin)
    if spec.origin is None and spec.submodule_search_locations is not None:
        # Once each: the shared entries are on the one process's path twice.
        return tuple(dict.fromkeys(os.path.realpath(p) for p in spec.submodule_search_locations))
    return spec.origin


def found(name, path):
    """Where `import name` finds a top-level module with `path` as
    sys.path, as Python's import does: the interpreter's finders in order,
    the path finder along `path`."""
    for finder in sys.meta_path:
        find = getattr(finder, "find_spec", None)
        spec = find and find(name, path if finder is importlib.machinery.PathFinder else None)
        if spec is not None:
            return origin(spec)
    return None


def mixed(sides, interpreters):
    """RUST-177: the top-level names this process gives a side otherwise
    than the side's own path would, its directories, the shared entries and
    then the interpreter's (`interpreters`), as Python itself resolves them.
    Asked after both sides are imported and called, so that what a call
    imports is in sys.modules. The names asked: every one the sides'
    directories and the shared entries provide, and every module in
    sys.modules from them. Each finding is tagged with its rule: (a) the
    two sides' paths give two modules, which speed build refuses only for a
    name both sides import alone (RUST-180); (b) the module this process
    holds is neither side's."""
    paths = {side: [os.path.realpath(p) for p in sides[side]["sys_path"]] + interpreters for side in sides}
    dirs = {d for side in sides for d in paths[side][: len(sides[side]["sys_path"])]}

    def from_sides(where):
        files = where if isinstance(where, tuple) else (where,)
        return any(isinstance(f, str) and f.startswith(d + os.sep) for f in files for d in dirs)

    names = set()
    for d in dirs:
        for entry in os.listdir(d) if os.path.isdir(d) else ():
            name = entry if os.path.isdir(os.path.join(d, entry)) else inspect.getmodulename(entry)
            if name and name.isidentifier():
                names.add(name)
    for name, module in list(sys.modules.items()):
        spec = getattr(module, "__spec__", None)
        if "." not in name and spec is not None and from_sides(origin(spec)):
            names.add(name)

    def show(where):
        return "the namespace package in " + ", ".join(where) if isinstance(where, tuple) else str(where)

    refused = []
    for name in sorted(names):
        o, p = found(name, paths["original"]), found(name, paths["port"])
        spec = getattr(sys.modules.get(name), "__spec__", None)
        held = origin(spec) if spec is not None else None
        # Two namespace directories mix only once imported: that is the
        # module held, below. A directory of data is never imported.
        if o is not None and p is not None and o != p and not (isinstance(o, tuple) and isinstance(p, tuple)):
            refused.append({"rule": "a", "name": name, "text": "%s (%s and %s)" % (name, show(o), show(p))})
        elif held is not None and (o is not None or p is not None) and held not in (o, p):
            text = "%s (the original's own path gives %s, the port's %s, but this process holds %s)"
            refused.append({"rule": "b", "name": name, "text": text % (name, show(o), show(p), show(held))})
    return refused


def held():
    """Every top-level module this process holds, with where it came from
    (RUST-180)."""
    return {
        name: origin(module.__spec__)
        for name, module in list(sys.modules.items())
        if "." not in name and getattr(module, "__spec__", None) is not None
    }


def put_in_place(caller, fn):
    """Puts a side's function where the caller's library calls it from."""
    module, attribute = caller["replace"].rsplit(".", 1)
    setattr(importlib.import_module(module), attribute, fn)


def caller_spec(caller, original):
    """decide's recorder's spec for the caller: its file and function, and
    the original's function as the one whose calls are recorded."""
    code = getattr(original, "__code__", None)
    if code is None:
        raise RuntimeError(
            "the original's %s is compiled: decide's recorder sees calls of Python functions only" % caller["fn"]
        )
    path = caller["path"]
    return {
        "project_root": os.path.dirname(path),
        "workload_file": path,
        "function": caller["function"],
        "candidate": {"file": code.co_filename, "function": code.co_name},
        "args": [],
    }


def case_args(req, harness):
    """A new copy of the case's arguments, decoded again."""
    if req["encoding"] == "typed":
        return [harness.dec(x) for x in req["args"]], {k: harness.dec(x) for k, x in req["kwargs"].items()}
    return json.loads(json.dumps(req["args"])), json.loads(json.dumps(req["kwargs"]))


def timed_case(req, fns, out, n, name, harness, log):
    """A direct target's timed run for a side, after one untimed call of
    each side, so that the runs are warm, each on its own copy of the case's
    arguments: whether each side's call changed them is reported, and if
    either did, every timed call gets a fresh copy, made before the run."""

    def fresh():
        return case_args(req, harness)

    def encoded(args, kwargs):
        try:
            return harness.enc((args, kwargs))
        except Exception:  # no longer a value the format holds
            return None

    by_side = {}
    for side in fns:
        args, kwargs = fresh()
        before = encoded(args, kwargs)
        fns[side](*args, **kwargs)
        by_side[side] = before is None or encoded(args, kwargs) != before
    out["arguments_changed_by_call"] = by_side
    changed = any(by_side.values())
    if changed:
        log("%s: the call changes its arguments: each call gets a fresh copy, made before its run" % name)
    args, kwargs = fresh()

    def run(side, n=n):
        fn = fns[side]
        # The same loop on both sides; the copies are made untimed.
        calls = [fresh() for _ in range(n)] if changed else [(args, kwargs)] * n
        gc.collect()
        log("%s: gc.collect()" % name)
        start = time.perf_counter()
        for a, k in calls:
            result = fn(*a, **k)
        return (time.perf_counter() - start) / n, result

    return run


def timed_caller(caller, fns, out, n, name, measure, log):
    """A caller target's timed run for a side, with the side's function put
    in place before it, after one untimed call of the caller with each
    side's: the original's recorded again, so that what is timed can be held
    to what speed build checked."""
    spec = caller_spec(caller, fns["original"])
    call = measure.load(spec)
    put_in_place(caller, fns["original"])
    out["recorded"] = measure.record(spec)
    put_in_place(caller, fns["port"])
    call()

    def run(side, n=n):
        put_in_place(caller, fns[side])
        gc.collect()
        log("%s: gc.collect()" % name)
        start = time.perf_counter()
        for _ in range(n):
            result = call()
        return (time.perf_counter() - start) / n, result

    return run


def written_out(fn, args, kwargs):
    """A call of `fn` made as a caller writes it, the keyword names
    written out and the values bound, so that the call leaves the
    interpreter nothing of its own (RUST-174). Keys that cannot be written
    as names fall back to functools.partial; the second value says whether
    it did. That is a key that is not an identifier, a keyword, __debug__
    (which cannot be assigned), or one Python would read as another name,
    since source identifiers are read as NFKC (RUST-179)."""
    def writable(k):
        return k.isidentifier() and not keyword.iskeyword(k) and k != "__debug__" and unicodedata.normalize("NFKC", k) == k

    if not all(writable(k) for k in kwargs):
        return functools.partial(fn, *args, **kwargs), True
    scope = {"f": fn}
    names = []
    for i, value in enumerate(args):
        scope["a%d" % i] = value
        names.append("a%d" % i)
    for i, (key, value) in enumerate(kwargs.items()):
        scope["k%d" % i] = value
        names.append("%s=k%d" % (key, i))
    return eval("lambda: f(%s)" % ", ".join(names), scope), False


def count(out, ready, counts, note=None):
    """Design §4, in speed build: the port's Rust allocations, by the
    counting allocator of a wheel built with count_allocations, over the
    target's first call in this process, apart, then over a warm call, each
    with its result still held, as by its caller. Then, by tracemalloc, the
    Python objects a warm call leaves to its caller, and its peak: every
    block the call touches, with no help from free lists.
    `ready()` gives the next call, its arguments made. `note`, if any, is
    what the Python figure holds beside the call's own."""
    if counts is None:
        out["counts"] = {"not_counted": "the port's module, built with count_allocations, exports no _speed_allocations"}
        return
    keys = ("allocations", "frees", "bytes", "live_bytes")

    def counted(call):
        # Read with the result dropped (RUST-175): Rust memory the result
        # owns is the result's, not a workspace the port keeps.
        gc.collect()
        before = counts()
        result = call()
        held = counts()
        del result
        gc.collect()
        after = counts()
        c = {k: b - a for k, a, b in zip(keys, before, after)}
        c["result_bytes"] = held[3] - after[3]
        return c

    def traced(read):
        # From emptied free lists (gc.collect() empties them), so that
        # every block the call takes is a new one, traced, its result's
        # among them (RUST-178). A second collection, with the result
        # held, empties them again: the call's freed temporaries, waiting
        # there, are released, and only the result and what it holds stay.
        call = ready()
        gc.collect()
        tracemalloc.start()
        result = call()
        gc.collect()
        seen = read()
        tracemalloc.stop()
        del result
        return seen

    first = counted(ready())
    warm = counted(ready())
    # Growth over 16 more warm calls, each result dropped: one warm call
    # can miss a Vec that doubles only now and then (RUST-175).
    gc.collect()
    start = counts()[3]
    for _ in range(16):
        result = ready()()
        del result
    gc.collect()
    growth = counts()[3] - start
    # Two more warm calls: a snapshot, and what reads it, would be in the
    # peak; the snapshot's own blocks are left out by their file.
    peak = traced(lambda: tracemalloc.get_traced_memory()[1])
    left = traced(lambda: tracemalloc.take_snapshot().filter_traces([tracemalloc.Filter(False, tracemalloc.__file__)]).traces)
    out["counts"] = {
        "first": first,
        "warm": warm,
        "growth": growth,
        "python": {
            "objects_left_to_the_caller": {"blocks": len(left), "bytes": sum(t.size for t in left)},
            "peak_bytes": peak,
        },
    }
    if note:
        out["counts"]["python"]["note"] = note


def encoded_output(harness, result):
    """A call's output as carcinify diff's driver encodes it."""
    try:
        return {"status": "returned", "value": harness.enc(result)}
    except harness.Unencodable as e:
        return {"status": "unencodable", "detail": str(e)}


def cold_thread(out, run, module, fn, threads, name, harness, log):
    """Design §2: in each of `threads` new threads, a cheap call into the
    port's module, untimed, then the port's first call there (cold) and its
    next (warm), each timed alone by `run`. The thread's start is not
    timed. The cheap call is the module's _speed_noop if it exports one,
    else the compared function with no arguments, when its signature shows
    that such a call raises TypeError while the arguments are parsed, before
    the body runs. When
    the port raises on a new thread, the original's first call is made once
    on another: if it does not raise, the port's traceback is kept, for
    carcinify speed to fail the target. Each output on a new thread is kept
    encoded, for carcinify speed to compare with the original's."""
    noop = getattr(module, "_speed_noop", None)
    cold = out["cold_thread"] = {
        "cheap_call": "_speed_noop" if noop else "the compared function with no arguments",
        "threads": [],
        "not_measured": None,
        "raised_in_new_thread": None,
        "outputs": [],
    }
    raised = []

    def cheap():
        if noop:
            noop()
            return None
        # Decided before any call: a TypeError the body raised would look
        # the same, after the body had warmed the thread.
        try:
            signature = inspect.signature(fn)
        except (TypeError, ValueError):
            return "the compared function shows no signature, so nothing says a call with no arguments would stop before its body: export _speed_noop from the port's module"
        try:
            signature.bind()
        except TypeError:
            pass  # a required parameter
        else:
            return "the compared function %s%s can be called with no arguments, so such a call would run its body and warm the thread: export _speed_noop from the port's module" % (
                getattr(fn, "__name__", "?"),
                signature,
            )
        try:
            fn()
        except TypeError:
            return None
        except Exception as e:
            return "the compared function with no arguments raised %s, not TypeError, so its body may have run and warmed the thread: export _speed_noop from the port's module" % type(e).__name__
        return "the compared function with no arguments ran, so it would warm the thread: export _speed_noop from the port's module"

    def calls(timed):
        """The port's first and next calls on this thread: their outputs,
        for the output check, and their times if `timed`."""
        try:
            first, first_result = run("port", 1)
            after, after_result = run("port", 1)
            if timed:
                cold["threads"].append({"cold": first, "warm": after})
            cold["outputs"].append([encoded_output(harness, first_result), encoded_output(harness, after_result)])
        except BaseException:  # pyo3's PanicException is a BaseException
            raised.append(traceback.format_exc()[-4000:])

    def thread():
        try:
            cold["not_measured"] = cheap()
        except BaseException:
            cold["not_measured"] = "the cheap call raised:\n" + traceback.format_exc()[-2000:]
            return
        if cold["not_measured"] is None:
            calls(timed=True)

    def original():
        try:
            run("original", 1)
        except BaseException:
            raised.append(None)

    def on_new_thread(target):
        t = threading.Thread(target=target)
        t.start()
        t.join()

    for k in range(threads):
        log("%s: cold thread %d starts" % (name, k + 1))
        on_new_thread(thread)
        if cold["not_measured"] or raised:
            cold["threads"], cold["outputs"] = [], []
            break
    if cold["not_measured"] and not raised:
        # No cheap call, so nothing is timed; but what a caller sees on a
        # new thread, a raise or another output, is still checked.
        log("%s: one new thread, untimed, for the checks alone" % name)
        on_new_thread(lambda: calls(timed=False))
    if raised:
        on_new_thread(original)
        if len(raised) > 1:
            cold["not_measured"] = cold["not_measured"] or (
                "the port raised on a new thread, and the original raised there too:\n" + raised[0]
            )
        else:
            cold["raised_in_new_thread"] = raised[0]
    log("%s: cold thread, %d threads" % (name, len(cold["threads"])))


def abba(runs):
    """The sides' order over `runs` pairs: original then port, then port
    then original."""
    order = []
    for k in range(runs):
        order += ["original", "port"] if k % 2 == 0 else ["port", "original"]
    return order


def time_sides(out, run, plan, n, name, measure, harness, log):
    """The sides in pairs of runs, original then port, then port then
    original, `run(side)` timing each; decide's loop of pure computation
    timed before and after each run; the output of each side's first timed
    call encoded."""
    kept_off = measure.calibrate()
    for i, side in enumerate(abba(plan["runs"])):
        seconds, result = run(side)
        log("%s: run %d %s, %d calls, timed" % (name, i + 1, side, n))
        after = measure.calibrate()
        out["runs"].append({"side": side, "seconds_per_call": seconds, "calls": n, "kept_off_the_cpu": max(kept_off, after)})
        kept_off = after
        if side not in out["outputs"]:
            out["outputs"][side] = encoded_output(harness, result)


def main():
    plan = json.load(sys.stdin)
    interpreters = list(sys.path)
    harness = load_file(plan["harness_driver"], "carcinify_diff_driver")
    measure = load_file(plan["measure"], "carcinify_decide_measure")
    harness.SHARED = harness.Shared({"shared_roots": plan["shared_roots"], "shared_hashes": plan["shared_hashes"]})
    debug = open(plan["debug_log"], "a") if plan.get("debug_log") else None

    def log(line):
        if debug:
            debug.write(line + "\n")
            debug.flush()

    seed = os.environ.get("PYTHONHASHSEED")
    log("hash seed %s (PYTHONHASHSEED=%s, hash randomization %s)" % (seed, seed, sys.flags.hash_randomization))
    # With "lone" in the plan, one side alone, on its own path (RUST-180).
    lone = plan.get("lone")
    modules = {name: import_side(plan["sides"][name]) for name in ([lone] if lone else ("original", "port"))}
    targets = []
    for target in plan["targets"]:
        name, n, caller = target["name"], target["calls_per_run"], target.get("caller")
        req = target.get("request")
        out = {"name": name, "runs": [], "outputs": {}, "error": None, "refused": None, "arguments_changed_by_call": None, "cold_thread": None}
        targets.append(out)
        # The function as carcinify diff finds it, by its rules: only the
        # module's own public functions, compiled ones in a wheel.
        fns = {}
        for side in modules:
            fn, resolved = harness.resolve(modules[side], caller["fn"] if caller else req["fn"], plan["sides"][side]["init"])
            if fn is None:
                out["refused"] = {"side": side, "status": resolved["status"], "detail": resolved.get("detail", "")}
                break
            fns[side] = fn
        if out["refused"]:
            continue
        try:
            if caller and plan.get("record"):
                # Two untimed calls of the caller with the original in
                # place, each recorded by decide's recorder.
                spec = caller_spec(caller, fns["original"])
                measure.load(spec)  # the caller's directory on sys.path, for its imports
                put_in_place(caller, fns["original"])
                out["recorded"] = [measure.record(spec), measure.record(spec)]
            elif plan.get("record"):
                # One untimed call of each side, so that speed build sees
                # what the call imports. The side is written down first, so
                # that speed build can name it if the call never returns.
                for side in fns:
                    with open(plan["calling"], "w") as f:
                        f.write(side)
                    args, kwargs = case_args(req, harness)
                    fns[side](*args, **kwargs)
            elif plan.get("count"):
                # The counted wheel's port alone is called, and nothing
                # before: its first call here is its first.
                if caller:
                    spec = caller_spec(caller, fns["original"])
                    call = measure.load(spec)
                    put_in_place(caller, fns["port"])

                    def ready():
                        return call

                    note = None
                else:

                    def ready():
                        return written_out(fns["port"], *case_args(req, harness))[0]

                    note = None
                    if written_out(fns["port"], *case_args(req, harness))[1]:
                        note = "the case's keyword names cannot all be written as names, so the call goes through functools.partial, as a caller's call would not: the figures may include blocks partial takes for the call"
                count(out, ready, getattr(modules["port"], "_speed_allocations", None), note)
            else:
                if caller:
                    run = timed_caller(caller, fns, out, n, name, measure, log)
                else:
                    run = timed_case(req, fns, out, n, name, harness, log)
                if plan.get("cold"):
                    # Warmed as the timing driver's runs warmed its process
                    # (RUST-172): the same runs, in the same order, untimed.
                    for i, side in enumerate(abba(plan["runs"])):
                        run(side)
                        log("%s: run %d %s, %d calls, untimed" % (name, i + 1, side, n))
                    cold_thread(out, run, modules["port"], fns["port"], plan["runs"], name, harness, log)
                else:
                    time_sides(out, run, plan, n, name, measure, harness, log)
        except BaseException:  # pyo3's PanicException is a BaseException
            out["error"] = traceback.format_exc()[-4000:]
    gil = getattr(sys, "_is_gil_enabled", lambda: True)()
    numpy = sys.modules.get("numpy")
    result = {
        "python": {
            "version": platform.python_version(),
            "implementation": platform.python_implementation(),
            "gil": gil,
            "numpy": getattr(numpy, "__version__", None),
        },
        "hash_seed": seed,
        "targets": targets,
        # What was imported, for carcinify speed to hold to the commit, and
        # where this interpreter keeps installed packages, which it leaves
        # out as carcinify diff does.
        "imported": sorted({os.path.realpath(f) for f in (getattr(m, "__file__", None) for m in list(sys.modules.values())) if f}),
        "package_dirs": sorted({os.path.realpath(sysconfig.get_paths()[k]) for k in ("purelib", "platlib")}),
    }
    if plan.get("record"):
        result["held"] = held()
        if lone:
            # Where the one process's path, the port's entries first, as
            # import_side leaves them, would find each module this side holds.
            shared = [os.path.realpath(p) for s in ("port", "original") for p in plan["sides"][s]["sys_path"]] + interpreters
            result["shared_gives"] = {name: found(name, shared) for name in result["held"]}
        else:
            result["mixed"] = mixed(plan["sides"], interpreters)
    with open(plan["result"], "w") as f:
        json.dump(result, f)


if __name__ == "__main__":
    main()
