"""The original for the mixed-wheel tests."""


def scale(x):
    return x * 3


def label(x):
    return "item-%d" % x


class StoreError(ValueError):
    pass


def check(n):
    if n < 0:
        raise StoreError("negative", n)
    return n
