"""Prints the results table and the decision-rule quantities.

    python3 table.py              # from ../results.json, the original measurement
    python3 table.py OUT.json     # from a new measurement
"""

import json
import os
import statistics

HERE = os.path.dirname(os.path.abspath(__file__))
import sys

results = json.load(open(sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(HERE), "results.json")))

print("| Run | Equivalent (harness) | Alloc/tick | Bytes/tick | ns/tick | Hot path: clone | collect | to_vec etc. | take/replace | Rc/RefCell/Arc/Mutex | containers made | Lines | fmt, clippy, own tests |")
print("|---|---|---|---|---|---|---|---|---|---|---|---|---|")
arms = {}
for r in results:
    h, hp, c = r["harness"], r["hot_path"] or {}, (r["counts"] or {}).get("hot", {})
    s = h.get("summary") or {}
    ok = h.get("verdict") == "pass" and not h.get("dirty")
    eq = "%s, %s of %s" % (h.get("verdict"), s.get("identical"), s.get("cases"))
    checks = "/".join("yes" if r[k] else "NO" for k in ("fmt", "clippy", "own_tests"))
    print("| %s | %s | %s | %s | %s | %s | %s | %s | %s | %s | %s | %s | %s |" % (
        r["run"], eq, hp.get("allocations_per_tick"), hp.get("bytes_per_tick"), hp.get("ns_per_tick"),
        c.get("clone"), c.get("collect"), c.get("to_vec_owned_string"), c.get("mem_take_replace"),
        c.get("rc_refcell_arc_mutex"), c.get("vec_new_in_body"), (r["counts"] or {}).get("lines"), checks))
    a = arms.setdefault(r["arm"], {"low": 0, "failed": 0, "ns": []})
    if not ok or not hp:
        a["failed"] += 1
    else:
        a["ns"].append(hp["ns_per_tick"])
        if hp["allocations_per_tick"] < 1:
            a["low"] += 1
print()
for arm, a in sorted(arms.items()):
    print("%s: low-allocation %d, failed %d, median ns/tick %s" % (arm, a["low"], a["failed"], statistics.median(a["ns"]) if a["ns"] else None))
print()
for r in results:
    if r.get("stated_budget"):
        print("%s budget line: %s" % (r["run"], r["stated_budget"]))
