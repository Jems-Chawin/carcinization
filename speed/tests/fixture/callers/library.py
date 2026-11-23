"""A library that calls `nap` and `wrong` through its own module, as
numpy's einsum_path calls _greedy_path. carcinify speed puts each side's
function in their place."""


def nap(ms):
    raise RuntimeError("carcinify speed did not put a side's nap in place")


def wrong(n):
    raise RuntimeError("carcinify speed did not put a side's wrong in place")


def naps(times):
    return [nap(ms) for ms in times]


def call_wrong(n):
    return wrong(n)


# How often naps_that_change has been called, kept here: the file of
# callers is loaded afresh for each recording.
calls = [0]
