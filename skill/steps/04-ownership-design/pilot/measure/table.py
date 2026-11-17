"""Prints the table of a measurement and applies the rule of PROTOCOL.md.

    python3 table.py OUT.json [depot-2,depot-3]

The second argument names runs found to be contaminated (PROTOCOL.md);
they are counted as not measurable.

The rule is here as code so that it is applied the same way to every run.
"""

import json
import sys

# The rule's numbers. They are in PROTOCOL.md, which was committed before
# any run, and must not be changed afterwards.
ARRIVALS_AT_THE_LARGER_SIZE = 22  # parcels that arrive per step with 64 couriers: 1 + 64 // 3
ALLOCATIONS_BEYOND_ARRIVALS = 16.0  # A: at least this many per step, beyond one per arriving parcel, and
COPIES_ON_THE_HOT_PATH = 14  # A and C: at least this many S3 constructs
TAKEN_OUT_OF_SELF = 1  # B: at least this many S4 hits on the hot path
SHARED_OWNERSHIP = 8  # C: with the S3 constructs, at least this many S7 hits


def judge(r):
    """(measurable, shows the pattern, which parts of the rule, A alone)"""
    s = r.get("structure")
    if not s or not s.get("hot_paths") or not s["hot_paths"][0].get("measured"):
        return False, False, [], False
    h = s["hot_paths"][0]
    larger = h["measured"][-1]["allocations_per_iteration"]
    s3 = h["copying_constructs"]
    s4 = len(h["fields_taken_out_of_self"])
    s7 = len(s["shared_ownership"])
    why = []
    cost = larger - ARRIVALS_AT_THE_LARGER_SIZE >= ALLOCATIONS_BEYOND_ARRIVALS
    if cost and s3 >= COPIES_ON_THE_HOT_PATH:
        why.append("A")
    if s4 >= TAKEN_OUT_OF_SELF:
        why.append("B")
    if s3 >= COPIES_ON_THE_HOT_PATH and s7 >= SHARED_OWNERSHIP:
        why.append("C")
    return True, bool(why), why, cost and not why


def main():
    results = json.load(open(sys.argv[1]))
    contaminated = sys.argv[2].split(",") if len(sys.argv) > 2 else []
    print("| Port | Harness | Alloc/step, small | large | Bytes/step, small | large | ns/step, small | large | S3 on the hot path | S4 | S7 | Lines | fmt, clippy, own tests | Callers file unchanged | Shows the pattern |")
    print("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|")
    shown = unmeasurable = 0
    for r in results:
        h = r["harness"]
        summary = h.get("summary") or {}
        harness = "%s, %s of %s" % (h.get("verdict"), summary.get("identical", "-"), summary.get("cases", "-"))
        measurable, shows, why, alone = judge(r)
        if r["port"] in contaminated:
            measurable, shows, why, alone = False, False, [], False
        yes = lambda b: "yes" if b else "no"
        checks = "%s/%s/%s" % (yes(r["fmt"]), yes(r["clippy"]), yes(r["own_tests"]))
        if measurable:
            s = r["structure"]
            hp = s["hot_paths"][0]
            m = hp["measured"]
            lines = sum(f["other_lines"] for f in s["size"]["files"].values())
            cells = [
                "%.2f" % m[0]["allocations_per_iteration"], "%.2f" % m[-1]["allocations_per_iteration"],
                "%.0f" % m[0]["bytes_per_iteration"], "%.0f" % m[-1]["bytes_per_iteration"],
                "%.0f" % m[0]["nanoseconds_per_iteration"], "%.0f" % m[-1]["nanoseconds_per_iteration"],
                str(hp["copying_constructs"]), str(len(hp["fields_taken_out_of_self"])), str(len(s["shared_ownership"])), str(lines),
            ]
            verdict = ("yes: " + "+".join(why)) if shows else "no"
            if alone:
                verdict = "no: A alone (allocates on the step path with few copying constructs)"
            if h.get("verdict") != "pass" and shows:
                verdict += " (and fails the harness)"
        else:
            cells = ["-"] * 10
            verdict = "contaminated: not measurable" if r["port"] in contaminated else "not measurable"
        if r["port"].startswith("depot-"):
            shown += shows
            unmeasurable += not measurable
        print("| " + " | ".join([r["port"], harness] + cells + [checks, yes(r["callers_unchanged"]), verdict]) + " |")
    runs = sum(1 for r in results if r["port"].startswith("depot-"))
    print()
    print("runs: %d; show the pattern: %d; not measurable: %d" % (runs, shown, unmeasurable))
    if runs:
        if unmeasurable >= 2:
            print("outcome by the rule: inconclusive, two or more runs could not be measured")
        elif shown == 0:
            print("outcome by the rule: no run shows the pattern")
        elif shown == 1:
            print("outcome by the rule: inconclusive, one run shows the pattern")
        else:
            print("outcome by the rule: the pattern was observed in %d of %d runs" % (shown, runs))


if __name__ == "__main__":
    main()
