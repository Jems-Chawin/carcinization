"""The original, as the harness calls it: `run` of depot.py and `scenario`
of callers.py under one module name."""

from callers import scenario  # noqa: F401
from depot import run  # noqa: F401
