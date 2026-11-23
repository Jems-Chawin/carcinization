"""A port of speed_orig, slow on purpose: nap sleeps ten times as long."""

import os
import signal
import threading
import time
from os.path import basename  # not this module's own code


# Each thread's workspace, made on its first nap in 50 ms, as a real port's
# buffers take time to set up: what carcinify speed's cold thread measures.
_thread = threading.local()


def nap(ms):
    if not getattr(_thread, "workspace", False):
        time.sleep(0.05)
        _thread.workspace = True
    if ms:
        time.sleep(10 * ms / 1000)
    return ms


def wrong(n):
    return n + 1


def pop_first(xs):
    return xs.pop(0)


def hang():
    time.sleep(60)


def square(n):
    return n * n


def crash(n):
    # SIGKILL, not os.abort(): the interpreter dies without a result, and
    # no system writes a crash report for it.
    os.kill(os.getpid(), signal.SIGKILL)


def first(xs):
    return xs.pop(0)


def size(s):
    return len(s)


def zero():
    return 0


# Set on the importing thread only: a new thread finds no table.
_imported = threading.local()
_imported.table = {"a": 1}
_imported.factor = 2


def lookup(key):
    return _imported.table[key]


def main_only(n):
    if threading.current_thread() is not threading.main_thread():
        raise RuntimeError("called off the main thread")
    return n


def doubled(n):
    # A new thread has no factor of its own: it gets the default.
    return getattr(_imported, "factor", 1) * n


def lazy(n):
    import lazy_helper  # a sibling module, imported only when called

    return lazy_helper.same(n)


def fill(n):
    return sum(range(n))


def fails(n):
    raise ValueError("the port fails on %d" % n)


def temp(n):
    pair, named = (n, n), {"n": n}
    del pair, named
    return n


def blob(n):
    return bytes(n)


_log = []


def log_push(n):
    _log.append(n)
    return n


def options(**kwargs):
    return len(kwargs)


def pair(n):
    return (n + 1, n + 2)


def half(n):
    return n * 1.5
