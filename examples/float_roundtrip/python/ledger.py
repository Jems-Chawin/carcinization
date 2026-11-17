"""A small ledger store: the Python original in this example.

Records are dicts {"id": int, "name": str, "amount": float}, stored one JSON
object per line.
"""

import json
import math
import os

MAX_ID = 2**63 - 1


def _check(record):
    if type(record["id"]) is not int or not 0 <= record["id"] <= MAX_ID:
        raise ValueError("id must be an integer from 0 to 2**63 - 1")
    if type(record["name"]) is not str:
        raise ValueError("name must be a string")
    if type(record["amount"]) is not float or not math.isfinite(record["amount"]):
        raise ValueError("amount must be a finite float")


def save(records, path):
    """Writes the records to `path`. Nothing is written if any record is bad."""
    for record in records:
        _check(record)
    with open(path, "w", encoding="utf-8") as f:
        for r in records:
            f.write(json.dumps({"id": r["id"], "name": r["name"], "amount": r["amount"]}))
            f.write("\n")


def load(path):
    """Reads the records back from `path`."""
    records = []
    with open(path, encoding="utf-8") as f:
        for line in f:
            r = json.loads(line)
            records.append({"id": r["id"], "name": r["name"], "amount": r["amount"]})
    return records


def roundtrip(records, directory):
    """Saves the records into `directory` and loads them again."""
    path = os.path.join(directory, "ledger.jsonl")
    save(records, path)
    return load(path)


def first_amount(records):
    """The amount of the first record. An empty list raises IndexError."""
    return records[0]["amount"]
