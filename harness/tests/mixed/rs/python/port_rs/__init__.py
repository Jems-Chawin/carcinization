"""The port: `scale` and `check` are compiled, `label` is still Python."""

from ._native import check, scale


def label(x):
    return "item-" + str(x)
