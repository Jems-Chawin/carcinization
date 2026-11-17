"""The workloads measured for the decision: one import of customer records."""

import dedupe

FIRST = ["Anna", "Ben", "Chloe", "David", "Emma", "Farid", "Greta", "Hugo", "Ines", "Jonas", "Kira", "Liam"]
LAST = ["Meyer", "Novak", "Okafor", "Petrov", "Quinn", "Rossi", "Schmidt", "Tanaka", "Ueda", "Varga"]


def records(count):
    """`count` records; about one in twenty is a respelling of another."""
    out = []
    for i in range(count):
        if i % 20 == 19:
            earlier = out[i - 7]
            out.append({"id": i, "name": earlier["name"].upper().replace(" ", "  "), "postcode": earlier["postcode"]})
        else:
            out.append({"id": i, "name": "%s %s %d" % (FIRST[i % len(FIRST)], LAST[(i // 3) % len(LAST)], i % 89), "postcode": "%05d" % (i * 37 % 9973)})
    return out


def run(count):
    """The import as it is today: every record against every other."""
    return len(dedupe.find_duplicates(records(count)))


def run_grouped(count):
    """The same import with the records grouped by fingerprint first."""
    return len(dedupe.find_duplicates_grouped(records(count)))
