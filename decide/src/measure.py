"""Runs one workload of a Python project under measurement.

Run by carcinify decide as

    python measure.py PHASE SPEC.json        (a token on standard input)

It loads the workload file named in the spec, calls the named function with
the given arguments, and prints one line that starts with the token and
carries the measurements as JSON. Standard library only.

The token is read from standard input before the workload is loaded, so it
is not in `sys.argv`, in the environment or on the command line. The
workload runs in this process all the same and is trusted not to forge a
result.

Phases, each run in a process of its own:

  plain         the workload called `repeat` times with nothing attached;
                wall time, CPU time of the process, of the main thread and
                of child processes for each call
  profile       one call under cProfile, timed by the wall clock
  boundary      one call with a hook on the candidate function alone: how
                often it is entered from outside, how much data goes in
                and comes out, and which compiled packages are called
                while it runs
  record        one call with a hook on the candidate alone, as in
                `boundary`: each call from outside it, its arguments
                written as a case line of carcinify diff when it is entered,
                before its body can change them (--record-calls)
  calls         Python 3.12 and later: one call under `sys.monitoring`,
                which reports calls of every callable, not only of built-in
                functions as the profiler does. Times the calls the
                project's code makes into compiled code while the candidate
                runs. Operators (`a @ b`) are not calls and are not seen.

Every phase runs a short loop of pure computation before and after the
workload and reports how much of the loop's time the process was kept off
the CPU. On a quiet machine that is about one percent; on a busy one the
workload's own figures cannot be trusted.
"""

import cProfile
import importlib.util
import inspect
import json
import os
import pstats
import statistics
import struct
import sys
import threading
import time


# The calibration loop runs until it has used this much CPU time.
CALIBRATION_CPU_SECONDS = 0.1


def calibrate():
    """The fraction of a pure computation's wall time this process was not
    on the CPU. The loop never waits, so anything above a percent or two
    was taken by other programs."""
    wall, cpu, x = time.perf_counter(), time.process_time(), 0
    while time.process_time() - cpu < CALIBRATION_CPU_SECONDS:
        for i in range(20000):
            x += i
    wall, cpu = time.perf_counter() - wall, time.process_time() - cpu
    return max(0.0, 1.0 - cpu / wall) if wall > 0 else 0.0


def load_average():
    try:
        return list(os.getloadavg())
    except (AttributeError, OSError):
        return None


def kept_off_the_cpu():
    """Linux: seconds the main thread was ready to run and was not running."""
    try:
        with open("/proc/self/schedstat") as f:
            return int(f.read().split()[1]) / 1e9
    except (OSError, ValueError, IndexError):
        return None


def load(spec):
    root = os.path.realpath(spec["project_root"])
    path = os.path.realpath(spec["workload_file"])
    for d in (os.path.dirname(path), root):
        if d not in sys.path:
            sys.path.insert(0, d)
    module_spec = importlib.util.spec_from_file_location("carcinify_decide_workload", path)
    module = importlib.util.module_from_spec(module_spec)
    module_spec.loader.exec_module(module)
    return getattr(module, spec["function"])


def category(filename, root):
    """Where a profiled function's code lives."""
    if filename == "~":  # how the profiler names code that has no Python source
        return "native"
    if filename.startswith("<"):  # <frozen importlib._bootstrap>, <string>
        return "other_python"
    real = os.path.realpath(filename)
    inside = real == root or real.startswith(root + os.sep)
    parts = real[len(root):].split(os.sep) if inside else []
    if inside and not any(p in ("site-packages", "dist-packages") or p.endswith(".egg") for p in parts):
        if not any(os.path.exists(os.path.join(root, *parts[1:i + 1], "pyvenv.cfg")) for i in range(len(parts))):
            return "own_python"
    return "other_python"


def plain(spec):
    fn = load(spec)
    runs = []
    before = calibrate()
    for _ in range(spec["repeat"]):
        children = os.times()
        off = kept_off_the_cpu()
        wall, cpu, main = time.perf_counter(), time.process_time(), time.thread_time()
        fn(*spec["args"])
        wall, cpu, main = time.perf_counter() - wall, time.process_time() - cpu, time.thread_time() - main
        after = os.times()
        off_after = kept_off_the_cpu()
        loop_after = calibrate()
        runs.append({
            "calibration_before": before,
            "calibration_after": loop_after,
            "seconds_ready_and_not_running": None if off is None or off_after is None else off_after - off,
            "wall_seconds": wall,
            "cpu_seconds": cpu,
            "main_thread_cpu_seconds": main,
            "child_process_cpu_seconds": (after.children_user + after.children_system) - (children.children_user + children.children_system),
        })
        before = loop_after
    try:
        import resource
        peak = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
        # Kilobytes on Linux, bytes on macOS.
        peak = peak if sys.platform == "darwin" else peak * 1024
    except ImportError:  # Windows
        peak = None
    return {"runs": runs, "peak_memory_bytes": peak}


# A call into compiled code that takes less than this on average is counted
# with the Python code that makes it: `str.lower`, `list.append`, `len`. Its
# cost is mostly the call, and it goes away when the caller is ported. A
# longer one is work that is already compiled: compressing, hashing, sorting
# a large list, waiting for a socket.
SHORT_NATIVE_SECONDS = 20e-6


def standard_library(top):
    names = getattr(sys, "stdlib_module_names", None)
    if names is not None:
        return top in names
    if top in sys.builtin_module_names:
        return True
    import sysconfig
    lib = sysconfig.get_paths()["stdlib"]
    return (os.path.exists(os.path.join(lib, top + ".py")) or os.path.isdir(os.path.join(lib, top))
            or any(n.startswith(top + ".") for n in os.listdir(os.path.join(lib, "lib-dynload")) if os.path.isdir(os.path.join(lib, "lib-dynload"))))


def compiled_package(top):
    """Is this top-level name a package from outside the standard library
    with compiled modules loaded, as this interpreter has it?"""
    if not top or top == "builtins" or top.startswith("carcinify_decide") or standard_library(top):
        return False
    for name, m in list(sys.modules.items()):
        if m is not None and (name == top or name.startswith(top + ".")):
            f = getattr(m, "__file__", None)
            if f is None or f.endswith((".so", ".pyd", ".dylib")):
                return True
    return False


def compiled_imports(path):
    """Top-level modules the file imports that are compiled, or are
    packages with compiled modules loaded, as this interpreter has them."""
    import ast
    try:
        with open(path, encoding="utf-8") as f:
            tree = ast.parse(f.read(), path)
    except (OSError, SyntaxError):
        return []
    tops = set()
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            tops.update(a.name.split(".")[0] for a in node.names)
        elif isinstance(node, ast.ImportFrom) and node.module and not node.level:
            tops.add(node.module.split(".")[0])
    return sorted(t for t in tops if compiled_package(t))


_trees = {}


def compiled_packages_named(path, line):
    """Compiled packages whose imported names the function defined at this
    line of this file uses. A file may import a package for other functions;
    this looks at the one function."""
    import ast
    if path not in _trees:
        try:
            with open(path, encoding="utf-8") as f:
                _trees[path] = ast.parse(f.read(), path)
        except (OSError, SyntaxError):
            _trees[path] = None
    tree = _trees[path]
    if tree is None:
        return []
    bound = {}  # a name in the file -> the top-level package it was imported from
    function = None
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            for a in node.names:
                bound[(a.asname or a.name).split(".")[0]] = a.name.split(".")[0]
        elif isinstance(node, ast.ImportFrom) and node.module and not node.level:
            for a in node.names:
                bound[a.asname or a.name] = node.module.split(".")[0]
        elif isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.Lambda)) and node.lineno <= line and function is None:
            # The profiler gives the line of the `def`, or of the first
            # decorator.
            first = min([node.lineno] + [d.lineno for d in getattr(node, "decorator_list", [])])
            if line in (first, node.lineno):
                function = node
    if function is None:
        return []
    used = {n.id for n in ast.walk(function) if isinstance(n, ast.Name)}
    return sorted({bound[n] for n in used if n in bound and compiled_package(bound[n])})


def profile(spec):
    fn = load(spec)
    root = os.path.realpath(spec["project_root"])
    profiler = cProfile.Profile(time.perf_counter)
    wall = time.perf_counter()
    profiler.enable()
    try:
        fn(*spec["args"])
    finally:
        profiler.disable()
    wall = time.perf_counter() - wall
    me = os.path.realpath(__file__)
    stats = {}
    for key, value in pstats.Stats(profiler).stats.items():
        if key[0] != "~" and os.path.realpath(key[0]) == me:
            continue
        if key[2] == "<method 'disable' of '_lsprof.Profiler' objects>":
            continue
        stats[key] = value

    kind = {key: category(key[0], root) for key in stats}
    # Is a compiled function's average call long enough to be real work?
    heavy = {key: kind[key] == "native" and nc > 0 and tt / nc >= SHORT_NATIVE_SECONDS for key, (_cc, nc, tt, _ct, _callers) in stats.items()}
    callees = {}
    for key, (_cc, _nc, _tt, _ct, callers) in stats.items():
        for caller, edge in callers.items():
            if caller in stats:
                callees.setdefault(caller, []).append((key, edge))

    # Seconds by where the work is done. A short compiled call belongs to
    # the Python code that made it.
    totals = {"own_python": 0.0, "library_python": 0.0, "compiled": 0.0}
    for key, (_cc, _nc, tt, _ct, callers) in stats.items():
        if kind[key] == "own_python":
            totals["own_python"] += tt
        elif kind[key] == "other_python":
            totals["library_python"] += tt
        elif heavy[key]:
            totals["compiled"] += tt
        else:
            known = 0.0
            for caller, edge in callers.items():
                if caller in stats and kind[caller] != "native":
                    totals["own_python" if kind[caller] == "own_python" else "library_python"] += edge[2]
                    known += edge[2]
            totals["compiled"] += max(0.0, tt - known)

    # What a function and everything it calls is made of. The profiler
    # records who called whom and for how long, not whole call trees, so a
    # callee's time is shared out among its callers in proportion to the
    # time each call edge took: the usual approximation of call-graph
    # profilers.
    memo, open_ = {}, set()

    def made_of(key):
        if key in memo:
            return memo[key]
        if key in open_:  # recursion: counted where it was first entered
            return {"own_python": 0.0, "library_python": 0.0, "compiled": 0.0}
        open_.add(key)
        mine = "own_python" if kind[key] == "own_python" else "library_python"
        parts = {"own_python": 0.0, "library_python": 0.0, "compiled": 0.0}
        parts[mine] += stats[key][2]
        for callee, edge in callees.get(key, []):
            if kind[callee] == "native":
                parts["compiled" if heavy[callee] else mine] += edge[2]
            else:
                whole = stats[callee][3]
                share = edge[3] / whole if whole > 0 else 0.0
                for name, seconds in made_of(callee).items():
                    parts[name] += seconds * min(1.0, share)
        open_.discard(key)
        memo[key] = parts
        return parts

    own = []
    for key, (cc, nc, tt, ct, callers) in stats.items():
        if kind[key] != "own_python":
            continue
        filename, line, name = key
        named = sorted(((edge[3], callee[2], kind[callee], edge[1]) for callee, edge in callees.get(key, [])), reverse=True)
        own.append({
            "file": os.path.relpath(os.path.realpath(filename), root), "line": line, "name": name,
            "calls": nc, "primitive_calls": cc, "self_seconds": tt, "cumulative_seconds": ct,
            "callers": len([c for c in callers if c in stats]),
            "caller_names": sorted({c[2] for c in callers if c in stats})[:12],
            "made_of_seconds": made_of(key),
            "largest_callees": [{"name": n, "where": "compiled" if k == "native" else "own_python" if k == "own_python" else "library_python", "calls": c, "cumulative_seconds": t} for t, n, k, c in named[:8]],
        })
    own.sort(key=lambda f: -f["cumulative_seconds"])
    rest = sorted(((tt, key, nc) for key, (_cc, nc, tt, _ct, _callers) in stats.items() if kind[key] != "own_python"), reverse=True)
    candidate = spec.get("candidate")
    # Every file of the project the candidate reaches through calls, and
    # the compiled packages each imports: a helper in another file counts.
    reached = {}
    if candidate:
        want = os.path.realpath(os.path.join(root, candidate["file"]))
        todo = [k for k in stats if k[2] == candidate["function"] and k[0] != "~" and os.path.realpath(k[0]) == want and candidate.get("line") in (None, k[1])]
        seen = set(todo)
        while todo:
            key = todo.pop()
            if kind[key] == "own_python":
                named = compiled_packages_named(os.path.realpath(key[0]), key[1])
                if named:
                    reached["%s:%d %s" % (os.path.relpath(os.path.realpath(key[0]), root), key[1], key[2])] = named
            for callee, _edge in callees.get(key, []):
                if callee not in seen and kind[callee] != "native":
                    seen.add(callee)
                    todo.append(callee)
    return {
        "wall_seconds": wall,
        "profiled_seconds": sum(totals.values()),
        "seconds_by_where": totals,
        "short_compiled_call_seconds": SHORT_NATIVE_SECONDS,
        "own_python_functions": own[:200],
        "own_python_functions_total": len(own),
        "largest_outside_the_project": [
            {"name": key[2], "file": key[0], "where": "compiled" if kind[key] == "native" else "library_python", "long_calls": bool(heavy[key]), "calls": nc, "self_seconds": tt}
            for tt, key, nc in rest[:20]
        ],
        "compiled_modules_imported_by_the_candidate_file": compiled_imports(os.path.join(root, candidate["file"])) if candidate else [],
        "compiled_packages_named_in_functions_the_candidate_reaches": dict(sorted(reached.items())),
    }


class Sizer:
    """Counts the Python objects reachable from a value and their size as
    `sys.getsizeof` gives it. Stops at a limit; a value that reaches it is
    reported as at least that large."""

    LIMIT = 200_000

    def __init__(self, limit=None):
        self.modules = set()  # where the types of everything seen are defined
        self.limit = limit or self.LIMIT
        self.largest = {}  # a module -> the size of the largest object of its types seen
        self.slots = {}  # a type -> the names of its slots

    def slots_of(self, kind):
        names = self.slots.get(kind)
        if names is None:
            names = []
            for klass in kind.__mro__:
                own = klass.__dict__.get("__slots__", ())
                names.extend([own] if isinstance(own, str) else own)
            names = self.slots[kind] = tuple(n for n in names if n not in ("__dict__", "__weakref__"))
        return names

    def measure(self, value):
        seen, stack, objects, size = set(), [value], 0, 0
        while stack and objects < self.limit:
            v = stack.pop()
            if id(v) in seen:
                continue
            seen.add(id(v))
            objects += 1
            module = type(v).__module__
            self.modules.add(module)
            try:
                own = sys.getsizeof(v)
            except TypeError:
                own = 0
            size += own
            if module != "builtins" and own > self.largest.get(module, 0):
                self.largest[module] = own
            if isinstance(v, dict):
                stack.extend(v.keys())
                stack.extend(v.values())
            elif isinstance(v, (list, tuple, set, frozenset)):
                stack.extend(v)
            elif not isinstance(v, type) and not callable(v):
                if hasattr(v, "__dict__"):
                    stack.append(vars(v))
                # An object with `__slots__` keeps its attributes there.
                for name in self.slots_of(type(v)):
                    try:
                        stack.append(getattr(v, name))
                    except AttributeError:
                        pass
        return objects, size, objects >= self.limit


def addressed(value):
    """The bytes an object gives access to, not only those it owns: a view
    of an array owns a hundred bytes and addresses the whole array."""
    try:
        size = getattr(value, "nbytes", None)
        if isinstance(size, int):
            return size
        try:
            return memoryview(value).nbytes
        except TypeError:
            return sys.getsizeof(value)
    except Exception:
        return 0


class Reach:
    """Looks at everything a value leads to, whatever kind of thing holds
    it: `gc.get_referents` gives what a list, a dictionary, an instance, a
    slot, a deque, a closure cell or a bound method refers to. Nearest
    things first, objects of types from outside the interpreter before the
    interpreter's own, and at most `limit` objects for each value given.
    Plain numbers, strings and bytes are not counted: they are of no
    package and lead nowhere, and a list of a million of them must not use
    up the limit before the one array in it is seen.

    It does not go into modules, classes or code, nor from a function into
    its globals: those lead to the whole program. From a function it goes
    into what the function closes over and its default arguments."""

    def __init__(self, limit=20_000):
        import collections
        import gc
        import types
        self.limit = limit
        self.modules = set()   # where the types of everything reached are defined
        self.largest = {}      # such a module -> the most bytes one object of its types addresses
        self.total = {}        # such a module -> the bytes all its objects address, each counted once
        self.cut_short = 0     # how many walks stopped at the limit
        self._counted = set()
        self._deque = collections.deque
        self._referents = gc.get_referents
        self._function = types.FunctionType
        self._stop = (type, types.ModuleType, types.FrameType, types.CodeType, types.BuiltinFunctionType)
        self._plain = frozenset((int, float, complex, bool, str, bytes, type(None)))

    def measure(self, value):
        seen, queue, count = set(), self._deque([value]), 0
        while queue:
            if count >= self.limit:
                self.cut_short += 1
                break
            v = queue.popleft()
            if id(v) in seen:
                continue
            seen.add(id(v))
            count += 1
            kind = type(v)
            module = kind.__module__
            if module != "builtins":
                self.modules.add(module)
                size = addressed(v)
                if size > self.largest.get(module, 0):
                    self.largest[module] = size
                if id(v) not in self._counted:
                    self._counted.add(id(v))
                    self.total[module] = self.total.get(module, 0) + size
            if isinstance(v, self._stop):
                continue
            if kind is self._function:
                found = list(v.__closure__ or ()) + list(v.__defaults__ or ()) + list((v.__kwdefaults__ or {}).values())
            else:
                try:
                    found = self._referents(v)
                except Exception:
                    found = ()
            for x in found:
                kind = type(x)
                if kind in self._plain or isinstance(x, self._stop):
                    continue
                if kind.__module__ != "builtins":
                    queue.appendleft(x)
                else:
                    queue.append(x)


def describe(value):
    name = type(value).__name__
    if isinstance(value, (list, tuple, set, frozenset, dict, str, bytes, bytearray)):
        inner = ""
        if isinstance(value, (list, tuple)) and value:
            inner = " of " + type(value[0]).__name__
        return "%s%s, length %d" % (name, inner, len(value))
    return name


def boundary(spec):
    fn = load(spec)
    root = os.path.realpath(spec["project_root"])
    want_file = os.path.realpath(os.path.join(root, spec["candidate"]["file"]))
    want_name = spec["candidate"]["function"]
    sizer = Sizer()
    # A second, smaller walk over what the candidate and the project's
    # functions it calls hold in their local variables when they return, and
    # in the globals they name: an array kept in a module constant, in a
    # closure or in an attribute never crosses the candidate's boundary.
    looker = Reach()
    # And the same kind of walk over what goes in and comes out.
    crossing = Reach()
    looked = {}  # code -> how many of its returns were looked at
    own_code = {}
    module_type = type(sys)

    def look(frame, code):
        seen = looked.get(code, 0)
        if seen >= 20:
            return
        looked[code] = seen + 1
        try:
            looker.measure(list(frame.f_locals.values()))
            if seen == 0:
                for name in code.co_names:
                    if name in frame.f_globals:
                        value = frame.f_globals[name]
                        if isinstance(value, module_type):
                            # `weights.W`: the attribute is named too.
                            looker.measure([getattr(value, n) for n in code.co_names if n in getattr(value, "__dict__", {})])
                        else:
                            looker.measure(value)
        except Exception:  # an object whose attributes cannot be read
            pass

    state = {"depth": 0, "entered": 0, "inner": 0, "samples": [], "truncated": 0, "types": set(), "c_calls": {},
             "since": None, "inside_wall": 0.0, "inside_cpu": 0.0}
    every = [1]
    known = {}

    def hook(frame, event, arg):
        if event != "call" and event != "return":
            if event == "c_call" and state["depth"]:
                # A built-in function or method called while the candidate
                # runs: note where it comes from.
                module = getattr(arg, "__module__", None) or type(getattr(arg, "__self__", None)).__module__
                state["c_calls"][module] = state["c_calls"].get(module, 0) + 1
            return
        code = frame.f_code
        wanted = known.get(code)
        if wanted is None:
            # Decided once for each piece of code, not on every call.
            wanted = known[code] = code.co_name == want_name and os.path.realpath(code.co_filename) == want_file
        if not wanted:
            if event == "return" and state["depth"]:
                own = own_code.get(code)
                if own is None:
                    own = own_code[code] = category(code.co_filename, root) == "own_python"
                if own:
                    look(frame, code)
            return
        if event == "return":
            look(frame, code)
        if event == "call":
            state["depth"] += 1
            if state["depth"] > 1:
                state["inner"] += 1
                return
            state["entered"] += 1
            # Wall time, and CPU time of every thread of the process, from
            # here to the matching return.
            state["since"] = (time.perf_counter(), time.process_time())
            n = state["entered"]
            # The first fifty, then fewer and fewer: at most a few hundred.
            if n > 50 and n % every[0]:
                return
            if n > 50 and len(state["samples"]) % 50 == 0:
                every[0] *= 2
            names = code.co_varnames[:code.co_argcount + code.co_kwonlyargcount]
            args = {k: frame.f_locals[k] for k in names if k in frame.f_locals}
            objects, size, cut = sizer.measure(list(args.values()))
            crossing.measure(list(args.values()))
            state["truncated"] += cut
            state["samples"].append({"call": n, "in_objects": objects - 1, "in_bytes": size,
                                     "arguments": {k: describe(v) for k, v in args.items()}})
            state["types"].update(type(v).__module__.split(".")[0] for v in args.values())
        else:
            state["depth"] -= 1
            if state["depth"] == 0 and state["since"] is not None:
                state["inside_wall"] += time.perf_counter() - state["since"][0]
                state["inside_cpu"] += time.process_time() - state["since"][1]
                state["since"] = None
            if state["depth"] == 0 and state["samples"] and state["samples"][-1]["call"] == state["entered"] and "out_objects" not in state["samples"][-1]:
                objects, size, cut = sizer.measure(arg)
                crossing.measure(arg)
                state["truncated"] += cut
                state["samples"][-1].update({"out_objects": objects, "out_bytes": size, "returns": describe(arg)})
                state["types"].add(type(arg).__module__.split(".")[0])

    sys.setprofile(hook)
    try:
        fn(*spec["args"])
    finally:
        sys.setprofile(None)
    samples = [s for s in state["samples"] if "out_objects" in s]
    by_top = {}
    for module, count in state["c_calls"].items():
        top = module.split(".")[0] if isinstance(module, str) else ""
        if compiled_package(top):
            by_top[top] = by_top.get(top, 0) + count

    largest, total = {}, {}
    for walk in (crossing, looker):
        for found, into in ((walk.largest, largest), (walk.total, total)):
            for module, size in found.items():
                top = module.split(".")[0] if isinstance(module, str) else ""
                if compiled_package(top):
                    into[top] = max(into.get(top, 0), size)

    def spread(key):
        values = [s[key] for s in samples]
        return {"median": statistics.median(values), "largest": max(values)} if values else None

    return {
        "entered_from_outside": state["entered"],
        "calls_from_inside_itself": state["inner"],
        # With the hook on, so the Python part is slower than in a plain
        # run; used only to see whether other threads are at work.
        "inside_the_candidate": {"wall_seconds": state["inside_wall"], "cpu_seconds_of_all_threads": state["inside_cpu"]},
        "calls_sampled": len(samples),
        "objects_in_per_call": spread("in_objects"),
        "bytes_in_per_call": spread("in_bytes"),
        "objects_out_per_call": spread("out_objects"),
        "bytes_out_per_call": spread("out_bytes"),
        "samples_cut_at_the_limit": state["truncated"],
        "object_limit_per_value": Sizer.LIMIT,
        "modules_of_the_types_crossing": sorted(t for t in state["types"] if t != "builtins" and not standard_library(t) and not t.startswith("carcinify_decide")),
        # Every object reached from the arguments and results, not only the
        # outermost: an array held in an attribute of an argument counts.
        "compiled_packages_of_the_objects_crossing": sorted(t for t in {m.split(".")[0] for m in crossing.modules if isinstance(m, str)} if compiled_package(t)),
        "compiled_packages_called_while_it_runs": dict(sorted(by_top.items())),
        # Local variables at return, and the globals named, of the candidate
        # and of the project's functions it calls (the first twenty returns
        # of each).
        # For each such package, among all of these: the most bytes one
        # object addresses (`nbytes`, or the size of its buffer, or
        # `sys.getsizeof`; a view of an array counts the data it shows), and
        # the bytes all its objects address, each object once.
        "largest_object_of_each_compiled_package_bytes": largest,
        "total_bytes_of_each_compiled_package_seen": total,
        "objects_looked_at_per_value": looker.limit,
        "walks_cut_short": crossing.cut_short + looker.cut_short,
        "compiled_packages_of_the_locals_and_globals": sorted(t for t in {m.split(".")[0] for m in looker.modules if isinstance(m, str)} if compiled_package(t)),
        "first_call": samples[0] if samples else None,
        "largest_call": max(samples, key=lambda s: s["in_objects"] + s["out_objects"]) if samples else None,
    }


def calls(spec):
    """Times the calls the project's code makes into compiled code while
    the candidate runs, with `sys.monitoring`."""
    mon = getattr(sys, "monitoring", None)
    if mon is None:
        return {"available": False, "why": "sys.monitoring needs Python 3.12 or later"}
    import threading
    import types
    fn = load(spec)
    root = os.path.realpath(spec["project_root"])
    want_file = os.path.realpath(os.path.join(root, spec["candidate"]["file"]))
    want_name, want_line = spec["candidate"]["function"], spec["candidate"].get("line")
    main_thread = threading.get_ident()
    tool, ev, clock = mon.PROFILER_ID, mon.events, time.perf_counter
    heap_type = 1 << 9
    is_candidate, is_own, kinds = {}, {}, {}
    depth = [0]
    stack = []  # open calls into compiled code: [kind, code, offset, start, python seconds, python depth, python start]
    found = {}

    def classify(c):
        """(package, does the profiler see it) for a callable that is
        compiled code, None for one that is Python."""
        t = type(c)
        if t is types.FunctionType or t is types.MethodType:
            return None
        seen = False
        if t is types.BuiltinFunctionType:
            seen, module = True, c.__module__ or type(c.__self__).__module__
        elif t is types.MethodDescriptorType:
            seen, module = True, c.__objclass__.__module__
        elif t is types.WrapperDescriptorType:
            module = c.__objclass__.__module__
        elif t is types.MethodWrapperType:
            module = type(c.__self__).__module__
        elif isinstance(c, type):
            if c.__flags__ & heap_type:  # a class written in Python, or one that cannot be told from it
                return None
            module = c.__module__
        else:
            if isinstance(getattr(t, "__call__", None), types.FunctionType):
                return None  # an object whose __call__ is Python
            module = getattr(c, "__module__", None)
            if not isinstance(module, str):
                module = t.__module__
        top = module.split(".")[0] if isinstance(module, str) else ""
        return (top or "?", seen)

    def kind_of(c):
        t = type(c)
        if t is types.FunctionType:
            return None
        key = (type(c.__self__), c.__name__) if t is types.BuiltinFunctionType and not isinstance(c.__self__, types.ModuleType) else c if t in (types.BuiltinFunctionType, types.MethodDescriptorType, types.WrapperDescriptorType) or isinstance(c, type) else t
        try:
            return kinds[key]
        except KeyError:
            kinds[key] = k = classify(c)
            return k
        except TypeError:  # not hashable
            return classify(c)

    def on_call(code, offset, callable_, arg0):
        if not depth[0] or threading.get_ident() != main_thread:
            return
        k = kind_of(callable_)
        if k is None:
            return
        own = is_own.get(code)
        if own is None:
            own = is_own[code] = category(code.co_filename, root) == "own_python"
        if own:
            stack.append([k, code, offset, clock(), 0.0, 0, 0.0])

    def on_c_end(code, offset, callable_, arg0):
        now = clock()
        if not stack or stack[-1][1] is not code or stack[-1][2] != offset or threading.get_ident() != main_thread:
            return
        k, _code, _offset, start, python, _depth, _start = stack.pop()
        whole = now - start
        alone = max(0.0, whole - python)
        f = found.setdefault(k, {"calls": 0, "seconds": 0.0, "long_calls": 0, "long_seconds": 0.0, "seconds_with_the_python_beneath": 0.0})
        f["calls"] += 1
        f["seconds"] += alone
        f["seconds_with_the_python_beneath"] += whole
        if alone >= SHORT_NATIVE_SECONDS:
            f["long_calls"] += 1
            f["long_seconds"] += alone

    def enter(code, offset):
        if threading.get_ident() != main_thread:
            return
        c = is_candidate.get(code)
        if c is None:
            c = is_candidate[code] = code.co_name == want_name and os.path.realpath(code.co_filename) == want_file and want_line in (None, code.co_firstlineno)
        if c:
            depth[0] += 1
        if stack:
            # Python running beneath a compiled call (a callback, a key
            # function, a generator being consumed): not compiled work.
            e = stack[-1]
            if e[5] == 0:
                e[6] = clock()
            e[5] += 1

    def leave(code, offset, value):
        if threading.get_ident() != main_thread:
            return
        if is_candidate.get(code) and depth[0]:
            depth[0] -= 1
        if stack and stack[-1][5]:
            e = stack[-1]
            e[5] -= 1
            if e[5] == 0:
                e[4] += clock() - e[6]

    mon.use_tool_id(tool, "carcinify decide")
    mon.register_callback(tool, ev.CALL, on_call)
    mon.register_callback(tool, ev.C_RETURN, on_c_end)
    mon.register_callback(tool, ev.C_RAISE, on_c_end)
    mon.register_callback(tool, ev.PY_START, enter)
    mon.register_callback(tool, ev.PY_RESUME, enter)
    mon.register_callback(tool, ev.PY_RETURN, leave)
    mon.register_callback(tool, ev.PY_YIELD, leave)
    mon.register_callback(tool, ev.PY_UNWIND, leave)
    wall = clock()
    mon.set_events(tool, ev.CALL | ev.C_RETURN | ev.C_RAISE | ev.PY_START | ev.PY_RESUME | ev.PY_RETURN | ev.PY_YIELD | ev.PY_UNWIND)
    try:
        fn(*spec["args"])
    finally:
        mon.set_events(tool, 0)
        mon.free_tool_id(tool)
    wall = clock() - wall
    packages = {}
    for (top, seen), f in sorted(found.items()):
        entry = packages.setdefault(top, {"standard_library": top == "builtins" or standard_library(top)})
        entry["seen_by_the_profiler" if seen else "not_seen_by_the_profiler"] = f
    return {
        "available": True,
        "wall_seconds": wall,
        "short_compiled_call_seconds": SHORT_NATIVE_SECONDS,
        "what": "Calls made by the project's own code, while the candidate runs, to callables that are compiled code. `seconds` leaves out Python that ran beneath the call. `not_seen_by_the_profiler` are callables that are not built-in functions (NumPy's `dot` and `add`, a C type called to make an object): cProfile counts their time as the Python function that calls them.",
        "packages": packages,
    }


class Unencodable(Exception):
    pass


def enc(v):
    """A value in carcinify diff's value format; the same as `enc` in the
    harness's driver.py. Exact types only: a subclass is reported."""
    t = type(v)
    if v is None:
        return {"t": "none"}
    if t is bool:
        return {"t": "bool", "v": v}
    if t is int:
        return {"t": "int", "v": str(v)}
    if t is float:
        return {"t": "float", "bits": struct.pack(">d", v).hex()}
    if t is str:
        try:
            v.encode("utf-8")
        except UnicodeEncodeError:  # lone surrogates
            return {"t": "str", "cp": [ord(c) for c in v]}
        return {"t": "str", "v": v}
    if t is bytes:
        return {"t": "bytes", "hex": v.hex()}
    if t is list:
        return {"t": "list", "v": [enc(x) for x in v]}
    if t is tuple:
        return {"t": "tuple", "v": [enc(x) for x in v]}
    if t is dict:
        return {"t": "map", "v": [[enc(k), enc(x)] for k, x in v.items()]}
    if t is set or t is frozenset:
        return {"t": "set", "v": [enc(x) for x in v]}
    raise Unencodable("%s.%s" % (t.__module__, t.__qualname__))


NO_DEFAULT = object()


def at_default(value, default):
    """The value is the parameter's default: the object itself, or one the
    value format writes the same. As `at_default` in the harness's
    driver.py, which leaves such a parameter out when it compares."""
    if default is NO_DEFAULT:
        return False
    if value is default:
        return True
    try:
        return enc(value) == enc(default)
    except (Unencodable, RecursionError):
        return False


def defaults_of(frame, code):
    """The defaults of the function the frame runs, by parameter name, when
    the function can be found by its name in its module; none otherwise."""
    fn = frame.f_globals.get(code.co_name)
    if getattr(fn, "__code__", None) is not code:
        return {}
    positional = code.co_varnames[:code.co_argcount]
    found = dict(zip(positional[len(positional) - len(fn.__defaults__ or ()):], fn.__defaults__ or ()))
    found.update(fn.__kwdefaults__ or {})
    return found


def arguments(frame, code, defaults):
    """A call's arguments from its frame at entry: the named parameters by
    position, keyword-only ones by keyword, then `*args` and `**kwargs`. A
    parameter at its default is left out, and the named ones after it go
    by keyword, so that a default the value format cannot hold (a sentinel
    object) is never written."""
    names, values = code.co_varnames, frame.f_locals
    positional, kwonly = code.co_argcount, code.co_kwonlyargcount
    varargs = code.co_flags & inspect.CO_VARARGS
    rest = values[names[positional + kwonly]] if varargs else ()
    args, kwargs, by_keyword = [], {}, False
    for i, n in enumerate(names[:positional]):
        # Not when the parameter cannot be named, or arguments follow it
        # by position.
        if not rest and i >= code.co_posonlyargcount and at_default(values[n], defaults.get(n, NO_DEFAULT)):
            by_keyword = True
        elif by_keyword:
            kwargs[n] = enc(values[n])
        else:
            args.append(enc(values[n]))
    for n in names[positional:positional + kwonly]:
        if not at_default(values[n], defaults.get(n, NO_DEFAULT)):
            kwargs[n] = enc(values[n])
    i = positional + kwonly
    if varargs:
        args += [enc(x) for x in rest]
        i += 1
    if code.co_flags & inspect.CO_VARKEYWORDS:
        kwargs.update((k, enc(x)) for k, x in values[names[i]].items())
    return {"encoding": "typed", "args": args, "kwargs": kwargs}


def record(spec):
    """Each call of the candidate from outside it, as a case line. The hook
    sees the candidate's code however it was reached, `from m import f`
    included, in every thread the workload starts, and encodes the
    arguments as the call is entered."""
    fn = load(spec)
    root = os.path.realpath(spec["project_root"])
    want_file = os.path.realpath(os.path.join(root, spec["candidate"]["file"]))
    want_name = spec["candidate"]["function"]
    known, seen, lines, state, defaults = {}, set(), [], {"calls": 0}, {}
    depth = {}  # per thread: a call in one thread is not inside another's

    def hook(frame, event, arg):
        if event != "call" and event != "return":
            return
        code = frame.f_code
        wanted = known.get(code)
        if wanted is None:
            wanted = known[code] = code.co_name == want_name and os.path.realpath(code.co_filename) == want_file
        if not wanted:
            return
        thread = threading.get_ident()
        if event == "return":
            depth[thread] -= 1
            return
        depth[thread] = depth.get(thread, 0) + 1
        if depth[thread] > 1:
            return  # a call from inside itself is not an input of the workload
        state["calls"] += 1
        try:
            if code not in defaults:
                defaults[code] = defaults_of(frame, code)
            line = dict({"fn": want_name}, **arguments(frame, code, defaults[code]))
        except Unencodable as e:
            line = {"fn": want_name, "unencodable": str(e)}
        except RecursionError:
            line = {"fn": want_name, "unencodable": "a value nested too deeply"}
        key = json.dumps(line)
        if key not in seen:
            seen.add(key)
            lines.append(line)

    # 3.12 and later also reach threads already running; before that, the
    # threads the workload starts from now on, and this one.
    every = getattr(threading, "setprofile_all_threads", None)
    if every:
        every(hook)
    else:
        threading.setprofile(hook)
        sys.setprofile(hook)
    try:
        fn(*spec["args"])
    finally:
        if every:
            every(None)
        else:
            threading.setprofile(None)
            sys.setprofile(None)
    return {
        "calls": state["calls"],
        "lines": [json.dumps(dict({"id": "call-%d" % n}, **line)) for n, line in enumerate(lines, 1)],
    }


def main():
    phase, spec_path = sys.argv[1], sys.argv[2]
    token = sys.stdin.readline().strip()
    with open(spec_path, encoding="utf-8") as f:
        spec = json.load(f)
    # What the workload sees as its own command line.
    sys.argv = [spec["workload_file"]]
    phases = {"plain": plain, "profile": profile, "boundary": boundary, "calls": calls, "record": record}
    if phase not in phases:
        raise SystemExit("unknown phase " + phase)
    load_before, loop_before = load_average(), calibrate()
    wall, cpu = time.perf_counter(), time.process_time()
    result = phases[phase](spec)
    # Of the whole phase and of every thread: how many CPUs the measured
    # process used on average.
    result["process"] = {"wall_seconds": time.perf_counter() - wall, "cpu_seconds": time.process_time() - cpu}
    result["machine"] = {
        "calibration_before": loop_before,
        "calibration_after": calibrate(),
        "load_average_before": load_before,
        "load_average_after": load_average(),
        "logical_cpus": os.cpu_count(),
    }
    # Every compiled package from outside the standard library that is
    # loaded now, whether or not anything was seen to use it.
    # A module with neither a file nor an import spec was made up at run
    # time by another module (Cython's `cython_runtime`): it is nothing the
    # project loaded by name.
    def real(top):
        module = sys.modules.get(top)
        return getattr(module, "__file__", None) is not None or getattr(module, "__spec__", None) is not None

    result["compiled_packages_loaded"] = sorted(t for t in {name.split(".")[0] for name in list(sys.modules)} if real(t) and compiled_package(t))
    result["python"] = "%d.%d.%d" % sys.version_info[:3]
    result["implementation"] = sys.implementation.name
    sys.stdout.flush()
    print("\n%s %s" % (token, json.dumps(result)))


if __name__ == "__main__":
    main()
