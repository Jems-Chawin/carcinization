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
    modules = {name: import_side(plan["sides"][name]) for name in ("original", "port")}
    targets = []
    for target in plan["targets"]:
        name, req, n = target["name"], target["request"], target["calls_per_run"]
        out = {"name": name, "runs": [], "outputs": {}, "error": None, "refused": None}
        targets.append(out)
        # The function as carcinify diff finds it, by its rules: only the
        # module's own public functions, compiled ones in a wheel.
        fns = {}
        for side in modules:
            fn, found = harness.resolve(modules[side], req["fn"], plan["sides"][side]["init"])
            if fn is None:
                out["refused"] = {"side": side, "status": found["status"], "detail": found.get("detail", "")}
                break
            fns[side] = fn
        if out["refused"]:
            continue
        try:
            if req["encoding"] == "typed":
                args = [harness.dec(x) for x in req["args"]]
                kwargs = {k: harness.dec(x) for k, x in req["kwargs"].items()}
            else:
                args, kwargs = req["args"], req["kwargs"]
            # shortcut: every call gets the same objects, so a function that
            # changes its arguments sees the changes on later calls; copy
            # them per call if a port ever needs it.
            for side in modules:
                fns[side](*args, **kwargs)  # untimed: the runs are warm
            order = []
            for k in range(plan["runs"]):
                order += ["original", "port"] if k % 2 == 0 else ["port", "original"]
            kept_off = measure.calibrate()
            for i, side in enumerate(order):
                fn = fns[side]
                gc.collect()
                log("%s: gc.collect()" % name)
                start = time.perf_counter()
                for _ in range(n):
                    result = fn(*args, **kwargs)
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
