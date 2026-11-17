"""Writes cases/roundtrip.jsonl and data/ledger.jsonl for this example.

Deterministic: the same seed gives the same files on any Python from 3.9
(checked with 3.9, 3.12 and 3.13), and both files are committed, so the
harness hashes exactly what was reviewed. Run it again only
to change the cases:

    python3 gen_cases.py

Floats are written as bit patterns (the harness's typed encoding), so no
JSON number parser stands between this script and either side.
"""

import json
import os
import random
import struct
import sys
import unicodedata

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "python"))
import ledger  # noqa: E402  (the original writes the data file)

SEED = 20261114


def f(x):
    return {"t": "float", "bits": struct.pack(">d", x).hex()}


def i(x):
    return {"t": "int", "v": str(x)}


def s(x):
    return {"t": "str", "v": x}


def record(id_, name, amount):
    return {"t": "map", "v": [[s("id"), id_], [s("name"), name], [s("amount"), amount]]}


def case(case_id, records, **extra):
    args = [{"t": "list", "v": records}, {"t": "scratch_dir"}]
    return dict({"id": case_id, "fn": "roundtrip", "encoding": "typed", "args": args}, **extra)


def from_bits(b):
    return struct.unpack(">d", b.to_bytes(8, "big"))[0]


NAMES = [
    "freight",
    "",
    unicodedata.normalize("NFC", "café"),
    unicodedata.normalize("NFD", "café"),
    "日本語 \U0001F980",
    'quote " backslash \\ slash /',
    "tab\tnewline\nreturn\rnul\x00",
    "  line separator",
    "x" * 300,
]

EDGE = [
    ("the-value-from-a-real-port", 396.03999999999996),
    ("negative-zero", -0.0),
    ("zero", 0.0),
    ("smallest-subnormal", 5e-324),
    ("largest-subnormal", from_bits(0x000FFFFFFFFFFFFF)),
    ("smallest-normal", 2.2250738585072014e-308),
    ("largest-float", 1.7976931348623157e308),
    ("most-negative-float", -1.7976931348623157e308),
    ("one-third", 1 / 3),
    ("point-one-plus-point-two", 0.1 + 0.2),
    ("two-to-the-53-plus-two", 9007199254740994.0),
    ("whole-number", 5.0),
    ("exponent-form", 1e22),
]


def main():
    rng = random.Random(SEED)
    cases = []
    for name, value in EDGE:
        cases.append(case("edge-" + name, [record(i(1), s(name), f(value))]))
    cases.append(case("edge-no-records", []))
    cases.append(case("edge-largest-id", [record(i(2**63 - 1), s("max"), f(1.5))]))
    cases.append(case("edge-every-name", [record(i(n), s(name), f(float(n))) for n, name in enumerate(NAMES)]))

    # Inputs the original rejects. The port must reject them the same way.
    bad = [
        ("nan", record(i(1), s("x"), f(float("nan")))),
        ("infinity", record(i(1), s("x"), f(float("inf")))),
        ("negative-infinity", record(i(1), s("x"), f(float("-inf")))),
        ("integer-amount", record(i(1), s("x"), i(5))),
        ("id-beyond-64-bits", record(i(2**70), s("x"), f(1.0))),
        ("id-two-to-the-63", record(i(2**63), s("x"), f(1.0))),
        ("negative-id", record(i(-1), s("x"), f(1.0))),
        ("boolean-id", record({"t": "bool", "v": True}, s("x"), f(1.0))),
        ("bytes-name", record(i(1), {"t": "bytes", "hex": "78"}, f(1.0))),
    ]
    for name, rec in bad:
        cases.append(case("reject-" + name, [rec], expect_raise=True))
    # The first bad record decides which error is raised.
    cases.append(case("reject-first-error-wins", [bad[0][1], bad[4][1]], expect_raise=True))
    cases.append(case("reject-good-then-bad", [record(i(1), s("ok"), f(1.0)), bad[6][1]], expect_raise=True))

    def amount():
        kind = rng.randrange(4)
        if kind == 0:  # prices: two decimals, then some arithmetic on them
            return rng.randrange(1, 10**8) / 100 * rng.choice([1.0, 1.1, 3.0, 0.07])
        if kind == 1:  # any finite double at all
            while True:
                x = from_bits(rng.getrandbits(64))
                if x == x and x not in (float("inf"), float("-inf")):
                    return x
        if kind == 2:  # sums of small fractions
            # Added one at a time, not with sum(): from Python 3.12 sum()
            # corrects its rounding, and the cases would depend on the version.
            total = 0
            for _ in range(rng.randrange(1, 6)):
                total = total + rng.randrange(1, 1000) / 1000
            return total
        return rng.uniform(-1e6, 1e6)

    for n in range(40):
        recs = [record(i(rng.randrange(0, 2**63)), s(rng.choice(NAMES)), f(amount())) for _ in range(25)]
        cases.append(case("random-%02d" % n, recs))

    with open(os.path.join(HERE, "cases", "roundtrip.jsonl"), "w", encoding="utf-8") as out:
        for c in cases:
            out.write(json.dumps(c) + "\n")

    # The data file: what the original would have left on disk in production.
    ledger.save(
        [
            {"id": 1001, "name": "freight", "amount": 1250.0},
            {"id": 1002, "name": "café supplies", "amount": 87.15},
            {"id": 1003, "name": "port fees", "amount": 396.03999999999996},
            {"id": 1004, "name": "refund", "amount": -0.0},
            {"id": 1005, "name": "insurance", "amount": 0.1 + 0.2},
            {"id": 9223372036854775807, "name": "carry-over", "amount": 1e22},
        ],
        os.path.join(HERE, "data", "ledger.jsonl"),
    )
    print("%d cases written" % len(cases))


if __name__ == "__main__":
    main()
