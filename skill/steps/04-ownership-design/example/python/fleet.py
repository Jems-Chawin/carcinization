"""A fleet of tankers on a grid: the Python original of this example.

Each unit keeps a reference back to the fleet it belongs to, and uses it to
look at the other units, to hand fuel to one of them, and to write into the
fleet's shared tallies. That is ordinary Python, and it is the shape that
does not carry over to Rust as it stands.
"""

MASK = 2**64 - 1
WIDTH = 16


class Rng:
    """A linear congruential generator, so both languages draw the same numbers."""

    def __init__(self, seed):
        self.state = seed & MASK

    def below(self, n):
        self.state = (self.state * 6364136223846793005 + 1442695040888963407) & MASK
        return (self.state >> 33) % n


class Unit:
    def __init__(self, fleet, uid, name, x, y, fuel):
        self.fleet = fleet  # back-reference to the owner
        self.uid = uid
        self.name = name
        self.x = x
        self.y = y
        self.fuel = fuel

    def nearest(self):
        """The closest other unit that still has fuel; the lowest uid wins a tie."""
        best = None
        found = None
        for other in self.fleet.units:  # reads every other unit through the fleet
            if other is self or other.fuel <= 0:
                continue
            distance = abs(other.x - self.x) + abs(other.y - self.y)
            if best is None or distance < best:
                best = distance
                found = other
        return found, best

    def step(self):
        if self.fuel <= 0:
            return
        other, distance = self.nearest()
        if other is None:
            return
        if distance <= 1:
            if self.fuel > other.fuel:
                self.fuel -= 1
                other.fuel += 1  # writes to another unit
                self.fleet.transfers += 1  # writes to the fleet
        else:
            if other.x != self.x:
                self.x += 1 if other.x > self.x else -1
            else:
                self.y += 1 if other.y > self.y else -1
            self.fuel -= 1
        self.fleet.heat[self.y * WIDTH + self.x] += 1  # writes to a shared list
        self.fleet.energy += 0.1 * self.fuel


class Fleet:
    def __init__(self, count, seed):
        rng = Rng(seed)
        self.units = []
        self.heat = [0] * (WIDTH * WIDTH)
        self.transfers = 0
        self.energy = 0.0
        for uid in range(count):
            x = rng.below(WIDTH)
            y = rng.below(WIDTH)
            fuel = 20 + rng.below(60)
            self.units.append(Unit(self, uid, "tanker-%03d" % uid, x, y, fuel))

    def tick(self):
        """The hot path: every unit takes one step, in order, seeing the
        moves the units before it have already made."""
        for unit in self.units:
            unit.step()

    def snapshot(self):
        return {
            "units": [[u.name, u.x, u.y, u.fuel] for u in self.units],
            "heat": list(self.heat),
            "transfers": self.transfers,
            "energy": self.energy,
        }


def simulate(count, seed, ticks):
    """Builds a fleet, runs it for `ticks` ticks and returns its state."""
    fleet = Fleet(count, seed)
    for _ in range(ticks):
        fleet.tick()
    return fleet.snapshot()
