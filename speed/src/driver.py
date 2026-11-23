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
before and after each run. The driver judges nothing: the figures are
carcinify speed's.

The arguments are decoded, and the outputs encoded, by carcinify diff's own
driver. Runs on Python 3.9 and later. Standard library only.
"""

import gc
import importlib
import importlib.util
import json
import os
import platform
import sys
import time
import traceback


def load_file(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def import_side(side):
    """The side's module, from the side's own directories and nowhere else."""
    sys.path[:0] = side["sys_path"]
    try:
        module = importlib.import_module(side["module"])
    finally:
        del sys.path[: len(side["sys_path"])]
    where = os.path.realpath(getattr(module, "__file__", None) or "")
    roots = [os.path.realpath(r) for r in side["roots"]]
    if not any(where.startswith(r + os.sep) for r in roots):
        raise RuntimeError("%s was loaded from %r, outside %r" % (side["module"], where, roots))
    return module


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


def main():
    plan = json.load(sys.stdin)
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
    # Recording the caller targets needs the original alone.
    names = ("original",) if plan.get("record") else ("original", "port")
    modules = {name: import_side(plan["sides"][name]) for name in names}
    targets = []
    for target in plan["targets"]:
        name, n, caller = target["name"], target["calls_per_run"], target.get("caller")
        req = target.get("request")
        out = {"name": name, "runs": [], "outputs": {}, "error": None, "refused": None, "arguments_changed_by_call": None}
        targets.append(out)
        # The function as carcinify diff finds it, by its rules: only the
        # module's own public functions, compiled ones in a wheel.
        fns = {}
        for side in modules:
            fn, found = harness.resolve(modules[side], caller["fn"] if caller else req["fn"], plan["sides"][side]["init"])
            if fn is None:
                out["refused"] = {"side": side, "status": found["status"], "detail": found.get("detail", "")}
                break
            fns[side] = fn
        if out["refused"]:
            continue
        if caller:
            # Two untimed calls of the caller with the original in place,
            # each recorded by decide's recorder.
            try:
                spec = caller_spec(caller, fns["original"])
                measure.load(spec)  # the caller's directory on sys.path, for its imports
                put_in_place(caller, fns["original"])
                out["recorded"] = [measure.record(spec), measure.record(spec)]
            except BaseException:
                out["error"] = traceback.format_exc()[-4000:]
            continue
        try:

            def fresh():
                """A new copy of the case's arguments, decoded again."""
                if req["encoding"] == "typed":
                    return [harness.dec(x) for x in req["args"]], {k: harness.dec(x) for k, x in req["kwargs"].items()}
                return json.loads(json.dumps(req["args"])), json.loads(json.dumps(req["kwargs"]))

            def encoded(args, kwargs):
                try:
                    return harness.enc((args, kwargs))
                except Exception:  # no longer a value the format holds
                    return None

            # One untimed call on each side, so that the runs are warm, on
            # its own copy: does the call change its arguments?
            changed = False
            for side in modules:
                args, kwargs = fresh()
                before = encoded(args, kwargs)
                fns[side](*args, **kwargs)
                changed = changed or before is None or encoded(args, kwargs) != before
            out["arguments_changed_by_call"] = changed
            if changed:
                log("%s: the call changes its arguments: each call gets a fresh copy, made before its run" % name)
            args, kwargs = fresh()
            order = []
            for k in range(plan["runs"]):
                order += ["original", "port"] if k % 2 == 0 else ["port", "original"]
            kept_off = measure.calibrate()
            for i, side in enumerate(order):
                fn = fns[side]
                # The same loop on both sides; the copies are made untimed.
                calls = [fresh() for _ in range(n)] if changed else [(args, kwargs)] * n
                gc.collect()
                log("%s: gc.collect()" % name)
                start = time.perf_counter()
                for a, k in calls:
                    result = fn(*a, **k)
                seconds = (time.perf_counter() - start) / n
                log("%s: run %d %s, %d calls, timed" % (name, i + 1, side, n))
                after = measure.calibrate()
                out["runs"].append(
                    {"side": side, "seconds_per_call": seconds, "calls": n, "kept_off_the_cpu": max(kept_off, after)}
                )
                kept_off = after
                if side not in out["outputs"]:
                    try:
                        out["outputs"][side] = {"status": "returned", "value": harness.enc(result)}
                    except harness.Unencodable as e:
                        out["outputs"][side] = {"status": "unencodable", "detail": str(e)}
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
    }
    with open(plan["result"], "w") as f:
        json.dump(result, f)


if __name__ == "__main__":
    main()
