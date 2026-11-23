"""The original of carcinify speed's fixture."""

import functools
import threading
import time
from os.path import basename  # not this module's own code


def nap(ms):
    # No sleep at all for 0: time.sleep(0) takes about 1 ms on some Linux.
    if ms:
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
