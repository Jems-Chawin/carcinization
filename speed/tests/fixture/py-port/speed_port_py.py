"""A port of speed_orig, slow on purpose: nap sleeps ten times as long."""

import os
import signal
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
    # SIGKILL, not os.abort(): the interpreter dies without a result, and
    # no system writes a crash report for it.
    os.kill(os.getpid(), signal.SIGKILL)


def first(xs):
    return xs.pop(0)


def size(s):
    return len(s)
