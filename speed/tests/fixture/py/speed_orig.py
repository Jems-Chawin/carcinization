"""The original of carcinify speed's fixture."""

import time


def nap(ms):
    time.sleep(ms / 1000)
    return ms


def wrong(n):
    return n
