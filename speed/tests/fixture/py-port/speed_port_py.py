"""A port of speed_orig, slow on purpose: nap sleeps ten times as long."""

import os
import time
from os.path import basename  # not this module's own code


def nap(ms):
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
    os.abort()
