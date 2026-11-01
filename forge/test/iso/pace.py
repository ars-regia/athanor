"""The pace of the console and the monitor: a factor on every wait, one for a real run."""

import math
import os

VARIABLE = "ATHANOR_CONSOLE_PACE"


def parse(text: str) -> float:
    """`text` as a pace: a finite number above zero, or a ValueError naming the variable."""
    try:
        value = float(text)
    except ValueError:
        value = math.nan
    if not (math.isfinite(value) and value > 0):
        raise ValueError(f"{VARIABLE} must be a finite number above 0, got {text!r}")
    return value


def from_environment() -> float:
    return parse(os.environ.get(VARIABLE, "1"))
