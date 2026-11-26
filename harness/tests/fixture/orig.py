"""The "original" for the harness's own tests."""
import math
import functools
import os
import struct


def neg_zero():
    return -0.0


def nan():
    return float("nan")


def nan_payload():
    return struct.unpack(">d", bytes.fromhex("7ff8000000000000"))[0]


def inf():
    return math.inf


def big():
    return 2**64 + 1


def order():
    return {"a": 1, "b": 2}


def text():
    return "café"  # NFC


def crash():
    return 1


def hang():
    return 1


def raises(fail=True):
    if fail:
        raise ValueError("bad value")
    return 1


def raises_other_message(fail=True):
    if fail:
        raise ValueError("bad value")
    return 1


def raises_one_side():
    return 1


def echo(*args, **kwargs):
    return [list(args), kwargs]


def noisy(x):
    print("this goes to stdout and must not reach the protocol")
    return x


def read(path):
    with open(path, "rb") as f:
        data = f.read()
    with open(path, "wb") as f:  # a side that damages its input
        f.write(b"overwritten by the original")
    return data


def scratch(d):
    with open(os.path.join(d, "out.txt"), "w") as f:
        f.write("x")
    return [d, sorted(os.listdir(d))]


def unencodable():
    return 1


def tolerance():
    return {"amount": 396.03999999999996, "rate": 396.03999999999996}


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
        raise ValueError(1)  # str(e) is "1" on both sides
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


def check_sign(x):
    if x < 0:
        raise ValueError("negative %r from %r" % (x, object()))
    return x


# NumPy arrays, for the tests that run only with CARCINIFY_NUMPY_PYTHON.
# NumPy is imported inside each function: the other tests run without it.


def arrays():
    import numpy as np

    return (
        np.array([1.0, -0.0, float("nan"), float("inf")]),
        np.array(1.5),
        np.arange(6).reshape(2, 3),
        np.array([True, False]),
        np.zeros(3, dtype=np.float32),
        np.array([7, -7], dtype=np.int32),
        np.zeros((0, 2)),
        np.arange(6.0).reshape(2, 3),
        np.arange(4.0),
    )


def arrays_float32():
    import numpy as np

    return np.array([1.0, 2.0])


def arrays_shape():
    import numpy as np

    return np.zeros(4)


def arrays_last_bit():
    import numpy as np

    return (np.array([-0.5, 0.5]), np.array([0.1, 0.2]))


def arrays_float32_ulps():
    import numpy as np

    return np.array([1.0], dtype=np.float32)


def arrays_masked():
    import numpy as np

    return np.ma.array([1.0])


def arrays_scalar():
    import numpy as np

    return np.float64(1.0)


def arrays_bool_bytes():
    import numpy as np

    return np.array([1, 2], dtype=np.int8).view(np.bool_)


def arrays_list():
    return [1.0, 2.0]


def arrays_in(a, flag):
    a[0] = -1.0
    return (a, type(a).__name__, a.flags.writeable, flag)


class Opaque:
    pass


def opaque():
    return Opaque()


def numpy_not_loaded():
    import sys

    if "numpy" in sys.modules:
        raise AssertionError("numpy was imported")
    return True
