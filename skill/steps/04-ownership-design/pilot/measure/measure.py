"""Measures the reference port, the two calibration ports and every run under ../runs.

    python3 measure.py prepare          # put the wrapper, the suite and the declaration beside each port (then commit)
    python3 measure.py measure OUT.json # needs a clean, committed tree; OUT.json outside the repository or in target/

Each port is a directory holding `depot/`, the crate. `prepare` adds beside
it `py/` (the wrapper of this directory, unchanged), `suite.json` and
`structure.json`. Nothing inside `depot/` is touched.

For each port, `measure` records:
- its own checks: cargo fmt --check, clippy -D warnings, cargo test (which
  is tests/callers.rs with the golden texts);
- whether tests/callers.rs is still the task's file, byte for byte;
- carcinify diff against depot.py and callers.py on cases.jsonl;
- carcinify structure on `Depot::step` at two sizes: allocations, bytes and
  nanoseconds per step, and the S3, S4 and S7 lists.

Environment: CARCINIFY (the binary), CARCINIFY_PYTHON, CARCINIFY_MATURIN.
"""

import filecmp
import json
import os
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
PILOT = os.path.dirname(HERE)
RUNS = os.path.join(PILOT, "runs")

DECLARATION = {
    "schema": "carcinify/structure/declaration/2",
    "crate": "depot",
    "hot_paths": [
        {
            "entry": "Depot::step",
            "budget": {
                "allocations_per_iteration": 0,
                "bytes_per_iteration": 0,
                "reason": "set by the measurer, not by the port's author: zero, so that the tool prints what is above it",
            },
            "driver": {
                "input": "a depot built by Depot::new(zones, couriers, seed), nobody listening",
                "construct": "depot::Depot::new",
                "sizes": [
                    {"label": "8 zones, 16 couriers", "args": [8, 16, 7]},
                    {"label": "16 zones, 64 couriers", "args": [16, 64, 7]},
                ],
                "call": {"method": "step"},
                "warm_up": 300,
                "iterations": 300,
            },
            "same_algorithm": {"same": True, "statement": "declared by the measurer: the task is a port, not a new algorithm"},
        }
    ],
    "justifications": [],
}


def sh(args, cwd, env=None):
    p = subprocess.run(args, cwd=cwd, capture_output=True, text=True, env=env)
    return p.returncode, p.stdout + p.stderr


def ports():
    out = [("reference", os.path.join(HERE, "reference"))]
    for name in ("pos", "neg", "kept", "pos-alias"):
        out.append(("calibration-" + name, os.path.join(HERE, "calibration", name)))
    if os.path.isdir(RUNS):
        for d in sorted(os.listdir(RUNS)):
            if os.path.isdir(os.path.join(RUNS, d, "depot")):
                out.append((d, os.path.join(RUNS, d)))
    return out


def prepare():
    for name, port in ports():
        if os.path.isdir(os.path.join(port, "py")):
            shutil.rmtree(os.path.join(port, "py"))
        shutil.copytree(os.path.join(HERE, "py"), os.path.join(port, "py"), ignore=shutil.ignore_patterns("target"))
        rel = os.path.relpath(PILOT, port)
        with open(os.path.join(port, "suite.json"), "w") as f:
            json.dump(
                {
                    "schema": "carcinify/diff/suite/1",
                    "original": {"kind": "python", "path": [rel + "/measure/original", rel + "/task"], "module": "original"},
                    "candidate": {"kind": "pyo3", "crate": "py", "module": "depot_rs"},
                    "cases": [rel + "/measure/cases.jsonl"],
                },
                f,
                indent=2,
            )
            f.write("\n")
        with open(os.path.join(port, "structure.json"), "w") as f:
            json.dump(DECLARATION, f, indent=2)
            f.write("\n")
        # The wrapper's lock file must know the port's crate as it is.
        code, out = sh(["cargo", "generate-lockfile"], os.path.join(port, "py"))
        print(name, "prepared" if code == 0 else out)


def measure_one(name, port):
    crate = os.path.join(port, "depot")
    out = {"port": name}
    out["fmt"] = sh(["cargo", "fmt", "--check"], crate)[0] == 0
    out["clippy"] = sh(["cargo", "clippy", "--all-targets", "--", "-D", "warnings"], crate)[0] == 0
    out["own_tests"] = sh(["cargo", "test"], crate)[0] == 0
    theirs = os.path.join(crate, "tests", "callers.rs")
    out["callers_unchanged"] = not name.startswith("depot-") or (
        os.path.exists(theirs) and filecmp.cmp(theirs, os.path.join(PILOT, "task", "tests", "callers.rs"), shallow=False)
    )
    tmp = tempfile.mkdtemp(prefix="pilot-")
    report = os.path.join(tmp, "diff.json")
    code, text = sh(
        [os.environ["CARCINIFY"], "diff", os.path.join(port, "suite.json"), "--report", report,
         "--python", os.environ["CARCINIFY_PYTHON"], "--maturin", os.environ["CARCINIFY_MATURIN"]],
        port,
    )
    r = json.load(open(report)) if os.path.exists(report) else {}
    out["harness"] = {
        "exit": code,
        "verdict": r.get("verdict"),
        "summary": r.get("summary"),
        "commit": (r.get("git") or {}).get("commit"),
        "dirty": (r.get("git") or {}).get("dirty"),
        "reasons": r.get("reasons"),
    }
    if not r:
        out["harness"]["output"] = text[-1500:]
    report = os.path.join(tmp, "structure.json")
    code, text = sh([os.environ["CARCINIFY"], "structure", os.path.join(port, "structure.json"), "--report", report], port)
    out["structure_exit"] = code
    out["structure"] = json.load(open(report)) if os.path.exists(report) else None
    if out["structure"] is None:
        out["structure_output"] = text[-3000:]
    shutil.rmtree(tmp)
    return out


if __name__ == "__main__":
    if sys.argv[1:] == ["prepare"]:
        prepare()
    elif len(sys.argv) == 3 and sys.argv[1] == "measure":
        out_path = os.path.abspath(sys.argv[2])
        results = [measure_one(name, port) for name, port in ports()]
        with open(out_path, "w") as f:
            json.dump(results, f, indent=1)
            f.write("\n")
        for r in results:
            print(r["port"], r["harness"]["verdict"], r["harness"]["summary"], "structure exit", r["structure_exit"])
    else:
        sys.exit(__doc__)
