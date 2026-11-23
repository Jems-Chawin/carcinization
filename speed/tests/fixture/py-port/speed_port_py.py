"""A port of speed_orig, slow on purpose: nap sleeps ten times as long."""

import time


def nap(ms):
    if ms:
        time.sleep(10 * ms / 1000)
    return ms


def wrong(n):
    return n + 1
