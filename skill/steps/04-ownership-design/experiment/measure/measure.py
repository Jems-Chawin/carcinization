"""Measures every run under ../runs and writes the results to a file you name.

    python3 measure.py prepare          # add the PyO3 wrapper and a suite beside each run (then commit)
    python3 measure.py measure OUT.json # everything below; needs a clean, committed tree for the harness

OUT.json must be outside the repository or in an ignored directory such as
target/. ../results.json is the record of the original measurement and is
not written to: times differ on every run, so it could never be reproduced
exactly, and writing it would leave the tree dirty.

For each run:
- the run's own checks: cargo fmt --check, clippy -D warnings, cargo test
  (which includes tests/golden.rs);
- hot_path.rs, in a temporary copy of the crate so the run stays as the
  sub-agent left it: allocations, bytes and nanoseconds per tick (release);
- count.py on its source;
- rustify-diff against relay.py, through the PyO3 wrapper;
- for the arm given the step: the allocation budget its DESIGN.md states.

Environment: RUSTIFY_DIFF (the harness binary), RUSTIFY_PYTHON,
RUSTIFY_MATURIN.
"""

import json
import os
import re
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
EXPERIMENT = os.path.dirname(HERE)
RUNS = os.path.join(EXPERIMENT, "runs")


def sh(args, cwd, env=None):
    p = subprocess.run(args, cwd=cwd, capture_output=True, text=True, env=env)
    return p.returncode, p.stdout + p.stderr


def run_names():
    return sorted(d for d in os.listdir(RUNS) if os.path.isdir(os.path.join(RUNS, d, "relay")))


def prepare():
    for name in run_names():
        run = os.path.join(RUNS, name)
        if os.path.isdir(os.path.join(run, "py")):
            shutil.rmtree(os.path.join(run, "py"))
        shutil.copytree(os.path.join(HERE, "py"), os.path.join(run, "py"), ignore=shutil.ignore_patterns("target"))
        with open(os.path.join(run, "suite.json"), "w") as f:
            json.dump(
                {
                    "schema": "rustify-diff/suite/1",
                    "original": {"kind": "python", "path": ["../../task"], "module": "relay"},
                    "candidate": {"kind": "pyo3", "crate": "py", "module": "relay_rs"},
                    "cases": ["../../measure/cases.jsonl"],
                },
                f,
                indent=2,
            )
            f.write("\n")
        # The wrapper's lock file must know the run's crate as it is.
        code, out = sh(["cargo", "generate-lockfile"], os.path.join(run, "py"))
        print(name, "prepared" if code == 0 else out)


def stated_budget(run):
    path = os.path.join(run, "relay", "DESIGN.md")
    if not os.path.exists(path):
        return None
    for line in open(path, encoding="utf-8"):
        if "budget" in line.lower():
            return line.strip()[:300]
    return "DESIGN.md has no line naming a budget"


def measure_one(name):
    run = os.path.join(RUNS, name)
    crate = os.path.join(run, "relay")
    out = {"run": name, "arm": name.rsplit("-", 1)[0]}
    out["fmt"] = sh(["cargo", "fmt", "--check"], crate)[0] == 0
    out["clippy"] = sh(["cargo", "clippy", "--all-targets", "--", "-D", "warnings"], crate)[0] == 0
    out["own_tests"] = sh(["cargo", "test"], crate)[0] == 0
    with tempfile.TemporaryDirectory() as tmp:
        copy = os.path.join(tmp, "relay")
        shutil.copytree(crate, copy, ignore=shutil.ignore_patterns("target"))
        shutil.copy(os.path.join(HERE, "hot_path.rs"), os.path.join(copy, "tests", "hot_path.rs"))
        code, text = sh(["cargo", "test", "--release", "--test", "hot_path", "--", "--nocapture"], copy)
        m = re.search(r"MEASURED (\{.*\})", text)
        out["hot_path"] = json.loads(m.group(1)) if m else None
        if not m:
            out["hot_path_error"] = text[-1500:]
    code, text = sh([sys.executable, os.path.join(HERE, "count.py"), os.path.join(crate, "src")], HERE)
    out["counts"] = json.loads(text) if code == 0 else None
    out["stated_budget"] = stated_budget(run)
    report = os.path.join(tempfile.gettempdir(), "rustify-t002-%s.json" % name)
    code, text = sh(
        [
            os.environ["RUSTIFY_DIFF"],
            os.path.join(run, "suite.json"),
            "--report",
            report,
            "--python",
            os.environ["RUSTIFY_PYTHON"],
            "--maturin",
            os.environ["RUSTIFY_MATURIN"],
        ],
        run,
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
    return out


if __name__ == "__main__":
    if sys.argv[1:] == ["prepare"]:
        prepare()
    elif len(sys.argv) == 3 and sys.argv[1] == "measure":
        out_path = os.path.abspath(sys.argv[2])
        if os.path.exists(os.path.join(EXPERIMENT, "results.json")) and os.path.samefile(
            os.path.dirname(out_path) or ".", EXPERIMENT
        ):
            sys.exit("write the results somewhere else: this directory holds the record of the original measurement")
        results = [measure_one(n) for n in run_names()]
        with open(out_path, "w") as f:
            json.dump(results, f, indent=1)
            f.write("\n")
        for r in results:
            print(json.dumps({k: r[k] for k in ("run", "hot_path", "harness")})[:400])
    else:
        sys.exit(__doc__)
