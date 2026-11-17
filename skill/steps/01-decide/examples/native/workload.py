"""The workload measured for the decision: one hour of events."""

import archive


def events(batches, per_batch):
    return [
        [{"id": b * per_batch + i, "kind": ("view", "click", "buy")[i % 3], "value": i * 0.37, "tags": ["a%d" % (i % 7), "b%d" % (i % 11)]}
         for i in range(per_batch)]
        for b in range(batches)
    ]


# Made when the file is loaded, so that building the test data is not part
# of what is measured. In production the events arrive from a queue.
HOUR = events(24, 4000)


def run(batches):
    """The first `batches` batches of the hour, 4,000 records each."""
    return archive.pack_all(HOUR[:batches])
