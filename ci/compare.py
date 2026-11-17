"""Sets a run's results.tsv beside ci/expected.tsv.

    python compare.py ci/expected.tsv <out>/results.tsv

Prints every difference and exits 1 if there is one, 0 if there is none.

What is compared, row by row:

  check    exit status 0
  suite    exit status 0, and the numbers of tests passed, failed and
           ignored equal to the expected ones: a test that stops running
           is a difference
  example  the exit status and the first group of facts (verdict, counts)
           equal to the expected ones
  tree     no change in the checkout after the run

What is not compared, and why:

  - the last column of an example row: values judged by time (the flags
    carcinify decide raised from a time, nanoseconds, how many runs were
    needed on a busy machine). Its flags that come from a count or a
    declaration are in the first group, by kind, and are compared. They are printed in every run and legitimately
    differ between machines.
  - the times of suites and examples.
  - the dependency check's live examples against the file: crates.io
    changes. Each is compared with its replay from the answers just
    fetched, which must give the same outcomes.

A row in the results that the file does not have is a difference. A row
of the file that the run did not produce is a difference unless the file
marks it `optional` (it needs the network or NumPy), or the run was made
with SUITES_ONLY=1 and the row is not a check or a suite.
"""

import sys


def rows(path):
    out = []
    with open(path, encoding="utf-8") as f:
        for line in f:
            line = line.rstrip("\n")
            if line and not line.startswith("#"):
                out.append(line.split("\t"))
    return out


def main():
    expected_path, results_path = sys.argv[1], sys.argv[2]
    expected = {}
    for row in rows(expected_path):
        row += [""] * (5 - len(row))
        expected[(row[0], row[1])] = {"facts": row[2], "exit": row[3], "optional": row[4] == "optional"}
    problems, seen, compared = [], set(), 0
    suites_only = False
    live = {}
    for row in rows(results_path):
        kind = row[0]
        if kind == "note":
            suites_only = suites_only or "suites only" in row[1]
            continue
        if kind == "stopped":
            problems.append("the run was stopped: " + " ".join(row[1:]))
            continue
        if kind == "tree":
            seen.add(("tree", ""))
            compared += 1
            if not row[1].startswith("0 change"):
                problems.append("the run changed the checkout: " + row[1])
            continue
        row += [""] * (4 - len(row))
        key, facts, status = (kind, row[1]), row[2], row[3]
        seen.add(key)
        name = "%s %s" % key
        if kind == "example" and row[1].startswith("deps ") and row[1].endswith(("-live", "-replay")):
            # Compared with each other below, not with the file.
            live[row[1]] = (facts, status)
            continue
        if kind in ("check", "suite") and status != "exit 0":
            problems.append("%s failed: %s, %s" % (name, facts or "no counts", status))
        want = expected.get(key)
        if want is None:
            problems.append("%s is not in the expected results: add it to ci/expected.tsv" % name)
            continue
        compared += 1
        if facts != want["facts"]:
            problems.append("%s: expected \"%s\", got \"%s\"" % (name, want["facts"], facts))
        if status != want["exit"]:
            problems.append("%s: expected %s, got %s" % (name, want["exit"], status))
    for name, (facts, status) in sorted(live.items()):
        if not name.endswith("-live"):
            continue
        replay = live.get(name[: -len("live")] + "replay")
        compared += 1
        if replay is None:
            problems.append("example %s has no replay beside it" % name)
        elif (facts.replace(" live ", " "), status) != (replay[0].replace(" replay ", " "), replay[1]):
            problems.append("example %s and its replay differ: \"%s, %s\" against \"%s, %s\"" % (name, facts, status, replay[0], replay[1]))
        if not facts.startswith("complete "):
            problems.append("example %s: \"%s\"" % (name, facts))
    for key, want in sorted(expected.items()):
        if key in seen or want["optional"]:
            continue
        if suites_only and key[0] not in ("check", "suite"):
            continue
        if suites_only and key == ("check", "release-build"):
            continue
        problems.append("%s %s is expected and was not run" % key)
    if problems:
        print("FAILED: %d difference(s) from ci/expected.tsv" % len(problems))
        for p in problems:
            print("  " + p)
        return 1
    print("as expected: %d rows compared with ci/expected.tsv%s" % (compared, ", suites only" if suites_only else ""))
    return 0


if __name__ == "__main__":
    sys.exit(main())
