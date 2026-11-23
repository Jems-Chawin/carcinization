"""The original of carcinify speed's fixture."""

import time


def nap(ms):
    # No sleep at all for 0: time.sleep(0) takes about 1 ms on some Linux.
    if ms:
        time.sleep(ms / 1000)
    return ms


def wrong(n):
    return n
