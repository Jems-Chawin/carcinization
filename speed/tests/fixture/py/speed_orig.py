"""The original of carcinify speed's fixture."""

import functools
import threading
import time
from os.path import basename  # not this module's own code


def _spin(seconds):
    # Waits on the clock without sleeping: on GitHub's macOS runners a sleep
    # ended tens of ms late, and the tests assert on how long nap takes.
    if seconds < 0:
        raise ValueError("sleep length must be non-negative")
    end = time.perf_counter() + seconds
    while time.perf_counter() < end:
        pass


def nap(ms):
    _spin(ms / 1000)
    return ms


def doze(ms):
    # Sleeps: for a test that needs only time to pass, with no CPU spent.
    time.sleep(ms / 1000)
    return ms


def wrong(n):
    return n


def pop_first(xs):
    return xs.pop(0)


def hang():
    time.sleep(60)


@functools.lru_cache(maxsize=None)
def square(n):
    return n * n


def crash(n):
    return n


def first(xs):
    return xs[0]


def size(s):
    return len(s)


def zero():
    return 0


TABLE = {"a": 1}


def lookup(key):
    return TABLE[key]


def main_only(n):
    if threading.current_thread() is not threading.main_thread():
        raise RuntimeError("called off the main thread")
    return n


def doubled(n):
    return 2 * n


def lazy(n):
    return n


def fill(n):
    return sum(range(n))


def fails(n):
    return n


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
