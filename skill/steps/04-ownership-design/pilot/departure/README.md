# A departure from the protocol: depot-2 measured with one argument cast

Made after the runs were measured and reported, on the lead's decision.
`measure/` is as it was at the last commit before the runs; nothing in it
was changed for this.

`measure/py`, the wrapper every port is compared through, passes the
number of steps to `run` as `usize`. depot-2's `run` takes it as `u64`, so
the wrapper did not compile against it and the harness gave an error, not
a result. `depot-2/py` here is that wrapper with the one argument cast
(`steps as u64`), and `depot-2/suite.json` names the same original and the
same 32 cases.

    carcinify diff departure/depot-2/suite.json --report <somewhere outside the tree>

The result is in the pilot's `README.md`, in a row of its own marked as a
departure. It does not change the run's judgement by the rule, which does
not depend on the harness, or the count.
