"""The "original" for the harness's own tests."""
import math
import functools
import os
import struct


def neg_zero():
    return -0.0


def nan():
    return float("nan")


def nan_payload():
    return struct.unpack(">d", bytes.fromhex("7ff8000000000000"))[0]


def inf():
    return math.inf


def big():
    return 2**64 + 1


def order():
    return {"a": 1, "b": 2}


def text():
    return "café"  # NFC


def crash():
    return 1


def hang():
    return 1


def raises(fail=True):
    if fail:
        raise ValueError("bad value")
    return 1


def raises_other_message(fail=True):
    if fail:
        raise ValueError("bad value")
    return 1


def raises_one_side():
    return 1


def echo(*args, **kwargs):
    return [list(args), kwargs]


def noisy(x):
    print("this goes to stdout and must not reach the protocol")
    return x


def read(path):
    with open(path, "rb") as f:
        data = f.read()
    with open(path, "wb") as f:  # a side that damages its input
        f.write(b"overwritten by the original")
    return data


def scratch(d):
    with open(os.path.join(d, "out.txt"), "w") as f:
        f.write("x")
    return [d, sorted(os.listdir(d))]


def unencodable():
    return 1


def tolerance():
    return {"amount": 396.03999999999996, "rate": 396.03999999999996}


def decorated(x):
    return x


def _wrap(fn):
    @functools.wraps(fn)
    def inner(*args, **kwargs):
        return fn(*args, **kwargs)

    return inner


decorated = _wrap(decorated)


class Thing:
    pass


def _private():
    return 1


class StoreError(Exception):
    pass


def boom(fail=True):
    if fail:
        raise StoreError("corrupt segment")
    return 1


def args_differ(fail=True):
    if fail:
        raise ValueError(1)  # str(e) is "1" on both sides
    return 1
