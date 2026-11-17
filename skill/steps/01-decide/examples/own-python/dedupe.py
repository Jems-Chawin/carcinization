"""Finds customer records that are probably the same person.

Two records match when their fingerprints are equal. `find_duplicates`
compares every record with every other one. `find_duplicates_grouped` gives
the same pairs by grouping records by fingerprint first.
"""


def fingerprint(record):
    """Letters and digits of the name and the postcode, lower-cased, with
    repeated letters collapsed: "Anna  Meyer" and "ana meyer" agree."""
    out = []
    last = ""
    for ch in record["name"] + "|" + record["postcode"]:
        if ch.isalnum() or ch == "|":
            ch = ch.lower()
            if ch != last:
                out.append(ch)
            last = ch
    return "".join(out)


def same_person(a, b):
    return fingerprint(a) == fingerprint(b)


def find_duplicates(records):
    """Every pair of records that match, as (earlier id, later id)."""
    pairs = []
    for i, a in enumerate(records):
        for b in records[i + 1:]:
            if same_person(a, b):
                pairs.append((a["id"], b["id"]))
    return pairs


def find_duplicates_grouped(records):
    """The same pairs, in the same order, with one fingerprint per record."""
    groups = {}
    for position, record in enumerate(records):
        groups.setdefault(fingerprint(record), []).append((position, record["id"]))
    pairs = []
    for members in groups.values():
        for i, (position, first) in enumerate(members):
            for _, second in members[i + 1:]:
                pairs.append((position, first, second))
    pairs.sort()
    return [(first, second) for _, first, second in pairs]


def find_duplicates_in_upload(data):
    """The whole check on an uploaded file: the file's bytes in, the pairs
    of ids out. Reading the file is part of it."""
    records = []
    for line in data.decode("utf-8").splitlines()[1:]:
        ident, name, postcode = line.split(",")
        records.append({"id": int(ident), "name": name, "postcode": postcode})
    return find_duplicates_grouped(records)
