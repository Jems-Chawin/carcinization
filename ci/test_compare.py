"""Tests of ci/compare.py's rule for a machine that was too busy.

    python3 ci/test_compare.py
"""

import os
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
EXPECTED = (
    "suite\tdecide\t3 passed, 0 failed, 1 ignored\texit 0\n"
    "example\tdecide waiting-decide\tconsistent mostly_waiting\texit 0\n"
)
BUSY = (
    "thread 'b' (1) panicked at decide/tests/cli.rs:219:9:\n"
    "the machine is too busy to run this test: six runs in a row were unreliable\n"
)
WRONG = "thread 'b' (1) panicked at decide/tests/cli.rs:300:5:\nassertion failed\n"


def compare(results, log, busy_allowed):
    with tempfile.TemporaryDirectory() as d:
        for name, text in (("expected.tsv", EXPECTED), ("results.tsv", results), ("suite-decide.log", log)):
            with open(os.path.join(d, name), "w", encoding="utf-8") as f:
                f.write(text)
        env = dict(os.environ, CARCINIFY_TOO_BUSY_IS_NOT_A_FAILURE="1" if busy_allowed else "")
        env.pop("GITHUB_ACTIONS", None)
        run = subprocess.run(
            [sys.executable, os.path.join(HERE, "compare.py"), os.path.join(d, "expected.tsv"), os.path.join(d, "results.tsv")],
            capture_output=True, text=True, env=env,
        )
        return run.returncode, run.stdout


def results(counts, status, example="consistent mostly_waiting\texit 0"):
    return "suite\tdecide\t%s\t%s\nexample\tdecide waiting-decide\t%s\n" % (counts, status, example)


ONE_FAILED = results("2 passed, 1 failed, 1 ignored", "exit 101")
LOG = "test a ... ok\ntest b ... FAILED\ntest c ... ok\n"


class TooBusy(unittest.TestCase):
    def test_a_test_that_gave_up_as_too_busy_fails_the_run_unless_allowed(self):
        code, out = compare(ONE_FAILED, LOG + BUSY, busy_allowed=False)
        self.assertEqual(code, 1, out)

    def test_allowed_it_is_named_and_the_run_passes(self):
        code, out = compare(ONE_FAILED, LOG + BUSY, busy_allowed=True)
        self.assertEqual(code, 0, out)
        self.assertIn("not judged, the machine was too busy: suite decide: b", out)

    def test_a_test_that_failed_for_another_reason_still_fails(self):
        code, out = compare(ONE_FAILED, LOG + WRONG, busy_allowed=True)
        self.assertEqual(code, 1, out)

    def test_a_suite_that_ended_with_another_exit_status_still_fails(self):
        # RUST-109: counts as expected and one busy test, but cargo was killed.
        code, out = compare(results("2 passed, 1 failed, 1 ignored", "exit 137"), LOG + BUSY, busy_allowed=True)
        self.assertEqual(code, 1, out)

    def test_a_different_number_of_tests_still_fails(self):
        code, out = compare(results("1 passed, 1 failed, 1 ignored", "exit 101"), LOG + BUSY, busy_allowed=True)
        self.assertEqual(code, 1, out)

    def test_no_test_judged_at_all_still_fails(self):
        log = "test a ... FAILED\ntest b ... FAILED\ntest c ... FAILED\n" + "".join(BUSY.replace("'b'", "'%s'" % n) for n in "abc")
        code, out = compare(results("0 passed, 3 failed, 1 ignored", "exit 101"), log, busy_allowed=True)
        self.assertEqual(code, 1, out)

    def test_an_example_answered_unreliable_is_not_judged(self):
        r = results("3 passed, 0 failed, 1 ignored", "exit 0", "unreliable mostly_waiting\texit 1")
        self.assertEqual(compare(r, "", busy_allowed=False)[0], 1)
        code, out = compare(r, "", busy_allowed=True)
        self.assertEqual(code, 0, out)
        self.assertIn("example decide waiting-decide", out)

    def test_an_example_with_another_verdict_still_fails(self):
        r = results("3 passed, 0 failed, 1 ignored", "exit 0", "contradicted mostly_waiting\texit 1")
        self.assertEqual(compare(r, "", busy_allowed=True)[0], 1)



class Tree(unittest.TestCase):
    OK = results("3 passed, 0 failed, 1 ignored", "exit 0")

    def test_a_tree_clean_before_and_unchanged_after_passes(self):
        code, out = compare(self.OK + "tree\t0 change(s) before the run, 0 after; HEAD unchanged\n", "", busy_allowed=False)
        self.assertEqual(code, 0, out)

    def test_a_tree_dirty_before_the_run_fails_though_clean_after(self):
        # Committed while the run went on: clean at the end, not what ran.
        code, out = compare(self.OK + "tree\t1 change(s) before the run, 0 after; HEAD unchanged\n", "", busy_allowed=False)
        self.assertEqual(code, 1, out)
        self.assertIn("1 change(s) before the run", out)

    def test_a_commit_made_during_the_run_fails_though_the_tree_is_clean_at_both_ends(self):
        row = "tree\t0 change(s) before the run, 0 after; HEAD aaaa at the start, bbbb at the end\n"
        code, out = compare(self.OK + row, "", busy_allowed=False)
        self.assertEqual(code, 1, out)
        self.assertIn("HEAD aaaa at the start, bbbb at the end", out)

    def test_a_tree_changed_by_the_run_fails(self):
        code, out = compare(self.OK + "tree\t0 change(s) before the run, 2 after; HEAD unchanged\n", "", busy_allowed=False)
        self.assertEqual(code, 1, out)


if __name__ == "__main__":
    unittest.main()
