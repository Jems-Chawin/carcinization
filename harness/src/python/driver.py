"""carcinify diff side driver for Python.

Started by the harness as `python -s driver.py`, with PYTHONHASHSEED and
PYTHONSAFEPATH set and every other PYTHON* variable removed from its
environment (see python/mod.rs); it checks that before anything else. It
imports one module, calls
the functions the harness names with the inputs the harness sends, and writes
each outcome back in the harness's value format. It decides nothing: it does
not compare, count or judge.

Protocol: one JSON object per line, requests on stdin, replies on stdout.
The function under test gets /dev/null as stdin and stderr as stdout, so
nothing it prints or reads can reach the protocol.

Runs on Python 3.9 and later. Standard library only.
"""

import hashlib
import importlib
import importlib.machinery
import inspect
import json
import os
import struct
import sys
import sysconfig
import threading
import traceback
import types

DRIVER = os.path.realpath(__file__)
# Python before 3.11 puts the script's directory first on sys.path (from 3.11
# PYTHONSAFEPATH keeps it off). The driver sits alone in its own directory,
# so nothing came from there; take it off, as -I did.
sys.path[:] = [p for p in sys.path if os.path.realpath(p or ".") != os.path.dirname(DRIVER)]
SHARED = None  # set in main() from the init request


class Unencodable(Exception):
    pass


def inside(path, entries):
    """True when `path` is one of `entries` or lies under one. Entries are
    real paths of files or directories."""
    return any(path == e or path.startswith(e + os.sep) for e in entries)


class Shared:
    """What the suite declares as shared, by path and by content.

    Shared is by content as well as by path: a byte-identical copy of a
    shared file is that shared file, wherever it sits (a side's own copy, or
    the copy maturin put in a wheel). The same test is used for every
    purpose (the open rule, where a function under test may live, and how a
    call is counted), so a copy cannot be exempt as shared for one of them
    and the side's own work for another.
    """

    def __init__(self, init):
        self.paths = [os.path.realpath(d) for d in init["shared_roots"]]
        self.hashes = set(init["shared_hashes"])
        self.known = {}

    def holds(self, path):
        """True when `path` (a real path) is a shared file or a copy of one."""
        found = self.known.get(path)
        if found is None:
            found = inside(path, self.paths)
            if not found and self.hashes:
                try:
                    with open(path, "rb") as f:
                        data = f.read()
                    found = bool(data) and hashlib.sha256(data).hexdigest() in self.hashes
                except OSError:
                    found = False
            self.known[path] = found
        return found


def source_of(path):
    """For a bytecode cache file, the source file it was compiled from."""
    head, name = os.path.split(path)
    if os.path.basename(head) == "__pycache__" and name.endswith(".pyc"):
        return os.path.join(os.path.dirname(head), name.split(".")[0] + ".py")
    return path


class Calls:
    """Counts the Python function calls made while a function under test
    runs, by where the called code lives: `own` (this side's files),
    `shared` (what the suite declares shared) or `other` (the standard
    library and installed packages).

    It says where the work was done, which the label on the function cannot:
    a compiled function that hands everything to bundled Python shows up
    here, and so do two sides that both forward to the same shared code.

    A profile function sees calls of Python functions in the threads it is
    set for. It does not see native code calling native code, and it never
    raises, so it cannot change what the function under test returns.
    """

    def __init__(self, init, shared):
        self.own = [os.path.realpath(d) for d in init["must_load_from"]]
        self.shared = shared
        self.kinds = {}
        self.counts = {"own": 0, "shared": 0, "other": 0}
        # One object, kept, so that "is the profile function still ours?"
        # can be answered by identity.
        self.installed = self.profile

    def profile(self, frame, event, arg):
        if event != "call":
            return
        try:
            name = frame.f_code.co_filename
            kind = self.kinds.get(name)
            if kind is None:
                path = os.path.realpath(name)
                # Shared first: a shared file may sit inside a side's directory.
                kind = "shared" if self.shared.holds(path) else "own" if inside(path, self.own) else "other"
                self.kinds[name] = kind
            self.counts[kind] += 1
        except Exception:
            pass

    def start(self):
        self.counts = {"own": 0, "shared": 0, "other": 0}
        threading.setprofile(self.installed)  # threads the function starts
        sys.setprofile(self.installed)

    def stop(self, active):
        """`active` is what sys.getprofile() returned when the call ended.
        If it is not this counter, the code under test set a profile
        function of its own or removed ours (a profiler, a coverage tool, or
        `sys.setprofile(None)`), and calls made after that were not seen. A
        partial count would read as a full one, so none is reported."""
        sys.setprofile(None)
        threading.setprofile(None)
        if active is not self.installed:
            return {
                "unavailable": "the code under test replaced or removed the profile function "
                "during the call, so not all of its Python calls were seen"
            }
        return self.counts


class Watch:
    """What the code under test did that the harness wants to know about,
    seen through a Python audit hook (which cannot be removed once added).

    - Opening a file in the other side's source directories. That is how a
      "port" ends up running the original: importing it, exec-ing its
      source, runpy. The harness stops the run when it sees one.
    - Starting a process. Recorded only; a port may have good reason to.

    The hook sees what Python does. Native code that opens a file itself
    raises no audit event.
    """

    PROCESS_EVENTS = ("subprocess.Popen", "os.system", "os.exec", "os.posix_spawn", "os.spawn", "os.fork", "os.forkpty")

    def __init__(self, other_roots, shared):
        self.other_roots = [os.path.realpath(d) for d in other_roots]
        self.shared = [os.path.realpath(d) for d in shared]
        self.active = False  # only while the side's own code is running
        self.opened = []
        self.processes = []
        self.process_count = 0
        sys.addaudithook(self.hook)

    def hook(self, event, args):
        if not self.active:
            return
        try:
            if event == "open":
                path = args[0]
                if isinstance(path, bytes):
                    path = os.fsdecode(path)
                if isinstance(path, str):
                    path = os.path.realpath(path)
                    if (
                        inside(path, self.other_roots)
                        and not inside(source_of(path), self.shared)
                        and path not in self.opened
                    ):
                        self.opened.append(path)
            elif event in self.PROCESS_EVENTS:
                self.process_count += 1
                if len(self.processes) < 20:
                    self.processes.append("%s %s" % (event, " ".join(repr(a)[:200] for a in args[:2])))
        except Exception:
            pass  # an error here would surface inside the code under test

    def drain(self):
        """What was seen since the last call, to put in a reply."""
        out = {"opened_other_side": self.opened, "processes": self.processes, "process_count": self.process_count}
        self.opened, self.processes, self.process_count = [], [], 0
        return out


# NumPy dtypes the format has, by kind and size, with their names on the wire.
ARRAY_DTYPES = {("f", 8): "<f8", ("f", 4): "<f4", ("i", 8): "<i8", ("i", 4): "<i4", ("b", 1): "|b1"}


def enc_array(np, v):
    """An exact numpy.ndarray -> wire value: dtype, shape, and the elements'
    bytes in C order, little-endian. Layout and byte order are not values."""
    dt = v.dtype
    wire = ARRAY_DTYPES.get((dt.kind, dt.itemsize))
    if wire is None or dt.fields is not None or dt.subdtype is not None:
        raise Unencodable("numpy.ndarray of dtype %r" % dt.str)
    if wire == "|b1":
        data = v.tobytes(order="C")
        if data.translate(None, b"\x00\x01"):  # a view can make them
            raise Unencodable("numpy.ndarray of dtype bool holding byte values other than 0 and 1")
    else:
        data = v.astype(dt.newbyteorder("<"), copy=False).tobytes(order="C")
    return {"t": "ndarray", "dtype": wire, "shape": list(v.shape), "hex": data.hex()}


def enc(v):
    """Python value -> wire value. Exact types only: a subclass (numpy.float64,
    an IntEnum, an OrderedDict) is reported, never silently narrowed."""
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
    # Only a side that has imported NumPy can return an array, so NumPy is
    # looked up, never imported here.
    np = sys.modules.get("numpy")
    if np is not None and t is np.ndarray:
        return enc_array(np, v)
    raise Unencodable("%s.%s" % (t.__module__, t.__qualname__))


def dec(j):
    """Wire value -> Python value, for typed inputs."""
    t = j["t"]
    if t == "none":
        return None
    if t == "bool":
        return j["v"]
    if t == "int":
        return int(j["v"])
    if t == "float":
        return struct.unpack(">d", bytes.fromhex(j["bits"]))[0]
    if t == "str":
        return j["v"] if "v" in j else "".join(map(chr, j["cp"]))
    if t == "bytes":
        return bytes.fromhex(j["hex"])
    if t == "list":
        return [dec(x) for x in j["v"]]
    if t == "tuple":
        return tuple(dec(x) for x in j["v"])
    if t == "map":
        return {dec(k): dec(x) for k, x in j["v"]}
    if t == "set":
        return {dec(x) for x in j["v"]}
    if t == "ndarray":
        try:
            import numpy as np
        except ImportError:
            raise ValueError("an ndarray input needs NumPy, which this side's interpreter cannot import") from None
        data = np.frombuffer(bytes.fromhex(j["hex"]), dtype=np.dtype(j["dtype"]))
        return data.reshape(j["shape"]).copy()  # writable, and this side's own
    if t == "_bytes_file":  # written by the harness for a file_bytes input
        with open(j["path"], "rb") as f:
            return f.read()
    raise ValueError("unknown input type %r" % (t,))


def loaded_files():
    """Files of every imported module outside the standard library: the code
    that actually ran. The harness hashes them."""
    paths = sysconfig.get_paths()
    std = [os.path.realpath(paths[k]) + os.sep for k in ("stdlib", "platstdlib")]
    out = set()
    for m in list(sys.modules.values()):
        f = getattr(m, "__file__", None)
        if not f:
            continue
        f = os.path.realpath(f)
        if f == DRIVER:
            continue
        if "site-packages" not in f and any(f.startswith(s) for s in std):
            continue
        out.add(f)
    return sorted(out)


def type_name(t):
    return "%s.%s" % (t.__module__, t.__qualname__)


def resolve(module, name, init):
    """Finds the function a case names. Returns (function, file that defines
    it), or (None, reply) when the name is not one the harness will call.

    Only the module's own public functions qualify. Looking the name up with
    getattr would also find what every module object inherits (__setattr__,
    __init__, ...), and calling those compares nothing.
    """

    def refuse(why):
        return None, {"status": "refused", "detail": "%r %s" % (name, why)}

    def foreign(why):
        return None, {"status": "foreign", "detail": "%r %s" % (name, why)}

    if name.startswith("_"):
        return refuse("starts with an underscore; only public functions can be cases")
    namespace = vars(module)
    if name not in namespace:
        return None, {"status": "no_such_function"}
    obj = namespace[name]
    if type(obj) not in (types.FunctionType, types.BuiltinFunctionType):
        return refuse("is a %s, not a function" % type_name(type(obj)))
    try:
        inner = inspect.unwrap(obj)  # past functools.wraps decorators
    except ValueError:
        inner = obj
    if type(inner) is types.FunctionType:
        origin = inner.__code__.co_filename
    elif type(inner) is types.BuiltinFunctionType and isinstance(inner.__self__, types.ModuleType):
        origin = getattr(inner.__self__, "__file__", None)
    else:
        origin = None
    origin = os.path.realpath(origin) if origin else ""

    def under(entries):
        return inside(origin, [os.path.realpath(e) for e in entries])

    if origin and SHARED.holds(origin):
        return None, {
            "status": "shared_function",
            "detail": "%r is defined in %r, which the suite declares as shared. Both sides would run "
            "the same code, so a function under test cannot live there" % (name, origin),
        }
    if not under(init["must_load_from"]):
        return refuse(
            "is defined in %r, which is outside %r: it is in the module's namespace but is not this side's own code"
            % (origin, init["must_load_from"])
        )
    suffixes = tuple(importlib.machinery.EXTENSION_SUFFIXES)
    compiled = type(inner) is types.BuiltinFunctionType and origin.endswith(suffixes)
    if init["require_extension"] and not compiled and name not in init["python_functions"]:
        return foreign(
            "is a Python function (%r) in a wheel built from a Rust crate, not a compiled one. If that is "
            "intended, list it under \"python_functions\" for this side in the suite; the report will then "
            "show it as python" % origin
        )
    return obj, (origin, "extension" if compiled else "python")


def public_functions(module, init):
    """Names a case could call on this module: its own public functions."""
    declared = dict(init, require_extension=False)  # list Python ones too
    return sorted(n for n in vars(module) if not n.startswith("_") and resolve(module, n, declared)[0] is not None)


def lineage(cls, init):
    """The classes an `except` clause could name to catch this one: the
    method resolution order without `object`. A class the side defines
    itself is given by class name alone (its module differs between the
    sides by construction), any other by full name."""
    return [
        "own:" + c.__qualname__ if own_class(c, init) else type_name(c)
        for c in cls.__mro__
        if c is not object
    ]


def own_class(cls, init):
    """True when the class is defined by this side's own code, so the module
    part of its name says where the port lives, not what the error is."""
    owner = sys.modules.get(cls.__module__)
    origin = getattr(owner, "__file__", None)
    if not origin:
        return False
    origin = os.path.realpath(origin)
    return any(origin.startswith(os.path.realpath(d) + os.sep) for d in init["must_load_from"])


def at_default(value, default):
    """The value is the parameter's default: the object itself, or one the
    value format writes the same (an int decoded from a case is a new
    object)."""
    if default is inspect.Parameter.empty:
        return False
    if value is default:
        return True
    try:
        return enc(value) == enc(default)
    except (Unencodable, RecursionError):
        return False


def bind(module, req, init):
    """A call's arguments as the function receives them, by parameter name
    with defaults filled in, so that one call written two ways (by position
    and by keyword) is one input. Nothing is run."""
    fn, found = resolve(module, req["fn"], init)
    if fn is None:
        return found
    try:
        if req["encoding"] == "typed":
            args = [dec(x) for x in req["args"]]
            kwargs = {k: dec(x) for k, x in req["kwargs"].items()}
        else:
            args, kwargs = req["args"], req["kwargs"]
    except Exception:
        return {"status": "bad_input", "detail": traceback.format_exc()}
    try:
        signature = inspect.signature(fn)
        bound = signature.bind(*args, **kwargs)
    except TypeError as e:
        return {"status": "bad_call", "detail": "the arguments do not fit the function's signature: %s" % e}
    except ValueError:  # no signature: compared as written
        arguments = {"args": tuple(args), "kwargs": kwargs}
    else:
        # A parameter at its default is left out, written or not, so that a
        # default the value format cannot hold (a sentinel object) need not
        # be encoded. decide's recorder leaves it out the same way.
        params = signature.parameters
        arguments = {
            name: value
            for name, value in bound.arguments.items()
            if not at_default(value, params[name].default)
        }
    try:
        return {"status": "bound", "value": enc(arguments)}
    except Unencodable as e:
        return {"status": "unencodable", "detail": "a default value is a %s" % e}


def call(module, req, init, watch, calls):
    fn, found = resolve(module, req["fn"], init)
    if fn is None:
        return found
    resp = invoke(fn, req, init, watch, calls)
    resp["defined_in"], resp["implementation"] = found
    return resp


def invoke(fn, req, init, watch, calls):
    try:
        if req["encoding"] == "typed":
            args = [dec(x) for x in req["args"]]
            kwargs = {k: dec(x) for k, x in req["kwargs"].items()}
        else:
            args, kwargs = req["args"], req["kwargs"]
    except Exception:
        return {"status": "bad_input", "detail": traceback.format_exc()}
    # Arguments that do not fit the signature would make both sides raise the
    # same TypeError before either body runs. That is a broken case, not
    # agreement, so it is reported as such and never compared.
    try:
        inspect.signature(fn).bind(*args, **kwargs)
    except TypeError as e:
        return {"status": "bad_call", "detail": "the arguments do not fit the function's signature: %s" % e}
    except ValueError:
        pass  # no signature available; the call itself will tell
    counted = None
    watch.active = True
    if calls:
        calls.start()
    try:
        result = fn(*args, **kwargs)
    except BaseException as e:  # pyo3's PanicException is a BaseException
        active = sys.getprofile()
        sys.setprofile(None)  # first, so that stop() itself is not counted
        if calls:
            counted = calls.stop(active)
        watch.active = False
        try:
            message = str(e)
        except Exception:
            message = "<str() of the exception failed>"
        tb = traceback.format_exc().encode("utf-8", "backslashreplace").decode("utf-8")
        try:
            args = enc(e.args)
        except (Unencodable, RecursionError, AttributeError):
            args = None  # the harness says so rather than guessing
        return {
            "status": "raised",
            "type": type_name(type(e)),
            "class_name": type(e).__qualname__,
            "own_class": own_class(type(e), init),
            "lineage": lineage(type(e), init),
            "message": enc(message),
            "args": args,
            "traceback": tb[-4000:],
            "python_calls": counted,
        }
    active = sys.getprofile()
    sys.setprofile(None)  # first, so that stop() itself is not counted
    if calls:
        counted = calls.stop(active)
    watch.active = False
    try:
        return {"status": "returned", "value": enc(result), "python_calls": counted}
    except Unencodable as e:
        return {"status": "unencodable", "detail": "the result holds a %s, which the value format does not cover" % e}
    except RecursionError:
        return {"status": "unencodable", "detail": "the result is nested too deeply"}


def isolation_error(init):
    """What is not as the harness started this interpreter, or None."""
    wrong = []
    if sys.flags.ignore_environment:
        wrong.append("it ignores the environment (-E or -I), so the hash seed is not in effect")
    seed = os.environ.get("PYTHONHASHSEED")
    if seed != str(init["hash_seed"]):
        wrong.append("PYTHONHASHSEED is %r, not %r" % (seed, str(init["hash_seed"])))
    others = sorted(k for k in os.environ if k.startswith("PYTHON") and k not in ("PYTHONHASHSEED", "PYTHONSAFEPATH"))
    if others:
        wrong.append("PYTHON* variables that the harness removed are set: %s" % ", ".join(others))
    if not sys.flags.no_user_site:
        wrong.append("the user site-packages directory is on")
    if sys.version_info >= (3, 11) and not sys.flags.safe_path:
        wrong.append("the script's directory may be on sys.path (safe_path is off)")
    return "; ".join(wrong) or None


def main():
    # Keep the protocol on private descriptors, then point the usual ones away.
    proto_in = os.fdopen(os.dup(0), "r", encoding="utf-8", newline="\n")
    proto_out = os.fdopen(os.dup(1), "w", encoding="utf-8", newline="\n")
    sys.stdout.flush()
    null = os.open(os.devnull, os.O_RDONLY)
    os.dup2(null, 0)
    os.close(null)
    os.dup2(2, 1)
    if hasattr(sys, "set_int_max_str_digits"):
        sys.set_int_max_str_digits(0)  # integers of any size

    answering = [None]  # the id of the request being answered

    def reply(obj):
        # Every reply carries the id of its request. The harness makes the
        # ids up afresh for each request and refuses a line without the
        # right one, so a line written to this pipe by anything else is not
        # taken for an answer.
        obj["id"] = answering[0]
        proto_out.write(json.dumps(obj))
        proto_out.write("\n")
        proto_out.flush()

    init = json.loads(proto_in.readline())
    answering[0] = init.get("id")
    wrong = isolation_error(init)
    if wrong:
        reply({"ok": False, "error": "the interpreter does not run as the harness started it: " + wrong})
        return
    # Read at start-up and done with: a process the code under test starts
    # inherits the seed and no other PYTHON* variable.
    os.environ.pop("PYTHONSAFEPATH", None)
    sys.path[:0] = init["sys_path"]
    watch = Watch(init["other_roots"], init["shared_roots"])
    global SHARED
    SHARED = Shared(init)
    calls = Calls(init, SHARED) if init["count_calls"] else None
    watch.active = True
    try:
        module = importlib.import_module(init["module"])
    except BaseException:
        watch.active = False
        reply({"ok": False, "error": "import of %r failed:\n%s" % (init["module"], traceback.format_exc())})
        return
    watch.active = False
    origin = os.path.realpath(getattr(module, "__file__", None) or "")
    roots = [os.path.realpath(d) + os.sep for d in init["must_load_from"]]
    if not any(origin.startswith(r) for r in roots):
        reply({
            "ok": False,
            "error": "module %r was loaded from %r, which is outside %r: another copy is shadowing the one under test"
            % (init["module"], origin, init["must_load_from"]),
        })
        return
    # The list of functions allowed to be Python is an allowance, so it has
    # to be exact: a name that is not there, or that is compiled after all,
    # would let a function change kind later without the run noticing.
    for name in init["python_functions"]:
        obj = vars(module).get(name)
        if type(obj) is types.BuiltinFunctionType:
            why = "is a compiled function. Take it off the list"
        elif type(obj) is not types.FunctionType:
            why = "is not a function of the module. Take it off the list"
        else:
            continue
        reply({"ok": False, "error": "\"python_functions\" lists %r, which %s." % (name, why)})
        return
    reply({
        "ok": True,
        "python": {
            "version": "%d.%d.%d" % sys.version_info[:3],
            "implementation": sys.implementation.name,
            "build": sys.version,
            "executable": os.path.realpath(sys.executable),
        },
        "module_file": origin,
        # Where this interpreter keeps installed packages. Files there are
        # third-party code, even when the virtualenv is inside the repository.
        "package_dirs": sorted({os.path.realpath(sysconfig.get_paths()[k]) for k in ("purelib", "platlib")}),
        "files": loaded_files(),
        "public_functions": public_functions(module, init),
        "watch": watch.drain(),
    })

    for line in proto_in:
        req = json.loads(line)
        answering[0] = req.get("id")
        if req["op"] == "exit":
            reply({
                "files": loaded_files(),
                "public_functions": public_functions(module, init),
                "watch": watch.drain(),
            })
            return
        if req["op"] == "bind":
            reply(bind(module, req, init))
            continue
        resp = call(module, req, init, watch, calls)
        resp["watch"] = watch.drain()
        sys.stdout.flush()
        sys.stderr.flush()
        reply(resp)


if __name__ == "__main__":
    main()
