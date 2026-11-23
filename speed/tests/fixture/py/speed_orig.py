"""The original of carcinify speed's fixture."""

import functools
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
