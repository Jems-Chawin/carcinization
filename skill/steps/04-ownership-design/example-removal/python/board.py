"""A task board: the Python original of the second example.

Tasks are added and, when finished, removed. A task may hold a reference to
another task that blocks it, and waits for as long as that task is still on
the board. Whether it is still there is asked with `in`, which for objects
means "is this very object in the list".
"""

MASK = 2**64 - 1


class Rng:
    """A linear congruential generator, so both languages draw the same numbers."""

    def __init__(self, seed):
        self.state = seed & MASK

    def below(self, n):
        self.state = (self.state * 6364136223846793005 + 1442695040888963407) & MASK
        return (self.state >> 33) % n


class Task:
    def __init__(self, tid, work, blocker):
        self.tid = tid
        self.work = work
        self.blocker = blocker  # another Task, or None


class Board:
    def __init__(self, seed):
        self.rng = Rng(seed)
        self.tasks = []
        self.done = []
        self.next_tid = 1
        self.clock = 0

    def add(self):
        blocker = None
        if self.tasks and self.rng.below(3) != 0:
            blocker = self.tasks[self.rng.below(len(self.tasks))]
        self.tasks.append(Task(self.next_tid, 1 + self.rng.below(4), blocker))
        self.next_tid += 1

    def tick(self):
        self.clock += 1
        for _ in range(self.rng.below(3)):
            self.add()
        for task in self.tasks:
            if task.blocker is not None and task.blocker in self.tasks:
                continue  # its blocker is still on the board
            task.work -= 1
        for task in [t for t in self.tasks if t.work <= 0]:
            self.tasks.remove(task)  # everything after it moves up one place
            self.done.append([task.tid, self.clock])

    def snapshot(self):
        return {
            "done": self.done,
            # A finished blocker is off the board, but the task still holds
            # the object, so its tid can still be read.
            "open": [[t.tid, t.work, None if t.blocker is None else t.blocker.tid] for t in self.tasks],
        }


def run(seed, ticks):
    """Runs a board for `ticks` ticks and returns what was done and what is open."""
    board = Board(seed)
    for _ in range(ticks):
        board.tick()
    return board.snapshot()
