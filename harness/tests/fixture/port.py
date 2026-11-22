"""The "port" for the harness's own tests: wrong in one small way per function."""
import decimal
import functools
import os
import struct
import time


def neg_zero():
    return 0.0


def nan():
    return 0.0


def nan_payload():
    return struct.unpack(">d", bytes.fromhex("7ff8000000000001"))[0]


def inf():
    return 1.7976931348623157e308


def big():
    return (2**64 + 1) % 2**64  # what wrapping to u64 would give


def order():
    return {"b": 2, "a": 1}


def text():
    return "café"  # NFD


def crash():
    os._exit(3)


def hang():
    time.sleep(600)


def raises(fail=True):
    if fail:
        raise ValueError("bad value")
    return 1


def raises_other_message(fail=True):
    if fail:
        raise ValueError("a different message")
    return 1


def raises_one_side():
    raise ValueError("only the port raises")


def echo(*args, **kwargs):
    return [list(args), kwargs]


def noisy(x):
    os.write(1, b"raw write to fd 1\n")
    return x


def read(path):
    with open(path, "rb") as f:
        return f.read()


def scratch(d):
    with open(os.path.join(d, "out.txt"), "w") as f:
        f.write("x")
    return [d, sorted(os.listdir(d))]


def unencodable():
    return decimal.Decimal("1")


def tolerance():
    return {"amount": 396.04, "rate": 396.03999999999996}


def decorated(x):
    return x


def _wrap(fn):
    @functools.wraps(fn)
    def inner(*args, **kwargs):
        return fn(*args, **kwargs)

    return inner


decorated = _wrap(decorated)


class Thing:
    pass


def _private():
    return 1


class StoreError(Exception):
    pass


def boom(fail=True):
    if fail:
        raise StoreError("corrupt segment")
    return 1


def args_differ(fail=True):
    if fail:
        raise ValueError("1")  # str(e) is "1" on both sides
    return 1


def set_order(words):
    return list(set(words))


def first_missing(labels, sizes):
    total = 1
    for label in set(labels):  # a missing size is named in set order
        total *= sizes[label]
    return total


def isolation():
    """Raises if anything the harness keeps out of a side got in."""
    import importlib.util
    import sys

    markers = ("pythonpath_marker", "user_marker", "userbase_marker", "cwd_marker")
    found = [m for m in markers if importlib.util.find_spec(m) is not None]
    found += ["driver directory %r" % p for p in sys.path if os.path.isfile(os.path.join(p or ".", "driver.py"))]
    if found:
        raise AssertionError(found)
    return True


def child_environment(expected):
    """Raises unless a process this side starts sees exactly `expected` as
    its PYTHON* variables."""
    import json
    import subprocess
    import sys

    code = "import json, os; print(json.dumps(sorted([k, v] for k, v in os.environ.items() if k.startswith('PYTHON'))))"
    seen = json.loads(subprocess.run([sys.executable, "-c", code], capture_output=True, text=True, check=True).stdout)
    if seen != expected:
        raise AssertionError(seen)
    return True


def read_text(path):
    with open(path) as f:
        return f.read()


_UNSET = object()


def with_defaults(x, y=_UNSET, z=None, big=10**30):
    return [x, y is _UNSET, z, big]
