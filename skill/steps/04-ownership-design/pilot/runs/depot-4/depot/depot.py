"""A courier depot, simulated one step at a time.

Zones are joined by roads. Parcels appear in a zone and have to reach
another. Couriers carry them: each step a courier delivers what has arrived,
passes parcels to a colleague who is going their way, picks up what fits,
and moves one zone. Couriers tire and rest, and retire after enough
deliveries; the depot hires a replacement at the next step. Parcels that
wait too long are given up.

Everything is whole numbers, and the same arguments always give the same
run. Other code holds on to the objects made here (see `callers.py`): a
`Courier`, a `Zone` or a `Parcel` that a caller keeps goes on showing the
state of that courier, zone or parcel, and a caller may change a courier.
"""

PATIENCE = 12  # a parcel that has waited longer than this is urgent
EXPIRY = 40  # a parcel that has waited longer than this is given up
FATIGUE_LIMIT = 25  # a courier this tired rests
REST = 3  # for this many steps
QUOTA = 30  # deliveries after which a courier retires, give or take
MASK = (1 << 64) - 1


class Rng:
    """A linear congruential generator, so that a run can be repeated."""

    def __init__(self, seed):
        self.state = seed & MASK

    def next(self):
        self.state = (self.state * 6364136223846793005 + 1442695040888963407) & MASK
        return self.state >> 33

    def below(self, n):
        return self.next() % n


class Parcel:
    """Something to be carried from one zone to another."""

    def __init__(self, depot, pid, origin, dest, weight):
        self.depot = depot
        self.id = pid
        self.origin = origin
        self.dest = dest
        self.weight = weight
        self.born = depot.clock
        self.carrier = None
        self.status = "waiting"
        self.hops = 0
        self.closed_at = None

    def age(self):
        """Steps since it appeared, up to the step it was closed."""
        end = self.depot.clock if self.closed_at is None else self.closed_at
        return end - self.born

    def urgent(self):
        return self.closed_at is None and self.age() > PATIENCE

    def close(self, status):
        self.status = status
        self.closed_at = self.depot.clock
        self.carrier = None


class Zone:
    """A place where parcels wait and couriers pass through."""

    def __init__(self, depot, zid):
        self.depot = depot
        self.id = zid
        self.neighbours = []
        self.waiting = []
        self.couriers = []
        self.delivered = 0
        self.congestion = 0

    def post(self, parcel):
        """Puts a parcel at the end of the queue here."""
        parcel.status = "waiting"
        parcel.carrier = None
        self.waiting.append(parcel)

    def offer(self, courier):
        """Takes out of the queue the parcels the courier will carry: urgent
        ones first, then the others, each in order of arrival, as long as
        they fit."""
        room = courier.capacity - courier.load()
        taken = []
        for urgent in (True, False):
            for parcel in self.waiting:
                if parcel.urgent() == urgent and parcel.weight <= room:
                    taken.append(parcel)
                    room -= parcel.weight
        for parcel in taken:
            self.waiting.remove(parcel)
        return taken

    def busiest_neighbour(self):
        """The neighbour with the most congestion; the first of them if
        several have the same."""
        best = self.neighbours[0]
        for zone in self.neighbours[1:]:
            if zone.congestion > best.congestion:
                best = zone
        return best

    def settle(self):
        """End of a step: gives up parcels that waited too long and brings
        the congestion figure up to date."""
        for parcel in list(self.waiting):
            if parcel.age() > EXPIRY:
                self.waiting.remove(parcel)
                self.depot.expire(parcel)
        weight = sum(parcel.weight for parcel in self.waiting)
        self.congestion = (self.congestion * 3 + weight * 4) // 4


class Courier:
    """Carries parcels, one zone per step."""

    def __init__(self, depot, cid, zone, capacity):
        self.depot = depot
        self.id = cid
        self.zone = zone
        self.capacity = capacity
        self.quota = QUOTA + cid % 7
        self.bag = []
        self.fatigue = 0
        self.resting = 0
        self.delivered = 0
        self.retired = False

    def load(self):
        return sum(parcel.weight for parcel in self.bag)

    def heading(self):
        """The zone this courier is going to, or None with an empty bag."""
        return self.bag[0].dest if self.bag else None

    def rest(self, steps):
        """Callers use this to take a courier off the road for a while."""
        self.resting = max(self.resting, steps)

    def act(self):
        """One step of this courier."""
        if self.resting > 0:
            self.resting -= 1
            return
        self.drop_off()
        self.hand_over()
        self.pick_up()
        self.move()
        self.tire()

    def drop_off(self):
        for parcel in list(self.bag):
            if parcel.dest is self.zone:
                self.bag.remove(parcel)
                self.zone.delivered += 1
                self.delivered += 1
                self.depot.record_delivery(parcel, self)

    def hand_over(self):
        """A parcel that a colleague here is heading for, and this courier
        is not, goes to the colleague if it fits."""
        mine = self.heading()
        for peer in self.zone.couriers:
            if peer is self or peer.resting > 0:
                continue
            theirs = peer.heading()
            if theirs is None or theirs is mine:
                continue
            for parcel in list(self.bag):
                if parcel.dest is theirs and peer.load() + parcel.weight <= peer.capacity:
                    self.bag.remove(parcel)
                    peer.bag.append(parcel)
                    parcel.carrier = peer
                    parcel.hops += 1
                    self.depot.handovers += 1

    def pick_up(self):
        for parcel in self.zone.offer(self):
            self.bag.append(parcel)
            parcel.carrier = self
            parcel.status = "carried"

    def move(self):
        here = self.zone
        goal = self.heading()
        if goal is not None:
            there = self.depot.route(here, goal)
        else:
            there = here.busiest_neighbour()
            if there.congestion <= here.congestion:
                return
        here.couriers.remove(self)
        there.couriers.append(self)
        self.zone = there
        self.fatigue += 1 + self.load() // 8
        for parcel in self.bag:
            parcel.hops += 1

    def tire(self):
        if self.fatigue >= FATIGUE_LIMIT:
            self.fatigue = 0
            self.resting = REST
        if self.delivered >= self.quota:
            self.depot.retire(self)


class Depot:
    """The whole simulation: zones, couriers, the parcels still open."""

    def __init__(self, zones, couriers, seed):
        if zones < 3:
            raise ValueError("at least three zones")
        self.rng = Rng(seed)
        self.clock = 0
        self.zones = [Zone(self, i) for i in range(zones)]
        for zone in self.zones:
            for other in ((zone.id + 1) % zones, (zone.id - 1) % zones, (zone.id * 2 + 1) % zones):
                link = self.zones[other]
                if link is not zone and link not in zone.neighbours:
                    zone.neighbours.append(link)
        self.routes = [self.first_steps(zone) for zone in self.zones]
        self.couriers = []
        self.parcels = {}
        self.listeners = []
        self.next_parcel = 0
        self.next_courier = 0
        self.vacancies = 0
        self.arrivals = 1 + couriers // 3
        self.delivered_count = 0
        self.expired_count = 0
        self.retired_count = 0
        self.latency_total = 0
        self.handovers = 0
        for _ in range(couriers):
            self.hire()

    def first_steps(self, start):
        """For every zone, the neighbour of `start` to go to first on a
        shortest way there. Roads are one-way where the express link is."""
        first = [None] * len(self.zones)
        first[start.id] = start.id
        frontier = []
        for zone in start.neighbours:
            if first[zone.id] is None:
                first[zone.id] = zone.id
                frontier.append(zone)
        while frontier:
            reached = []
            for zone in frontier:
                for link in zone.neighbours:
                    if first[link.id] is None:
                        first[link.id] = first[zone.id]
                        reached.append(link)
            frontier = reached
        return first

    def route(self, here, goal):
        return self.zones[self.routes[here.id][goal.id]]

    def hire(self):
        zone = self.zones[self.rng.below(len(self.zones))]
        capacity = 4 + self.rng.below(5)
        courier = Courier(self, self.next_courier, zone, capacity)
        self.next_courier += 1
        self.couriers.append(courier)
        zone.couriers.append(courier)
        return courier

    def courier(self, cid):
        """The courier with this number, or None if there is none at work."""
        for courier in self.couriers:
            if courier.id == cid:
                return courier
        return None

    def zone(self, zid):
        return self.zones[zid]

    def submit(self, origin, dest, weight):
        """A new parcel, waiting in zone `origin`, for zone `dest`."""
        parcel = Parcel(self, self.next_parcel, self.zones[origin], self.zones[dest], weight)
        self.next_parcel += 1
        self.parcels[parcel.id] = parcel
        parcel.origin.post(parcel)
        return parcel

    def on_delivered(self, listener):
        """`listener(parcel, courier)` is called at each delivery."""
        self.listeners.append(listener)

    def record_delivery(self, parcel, courier):
        parcel.close("delivered")
        del self.parcels[parcel.id]
        self.delivered_count += 1
        self.latency_total += parcel.age()
        for listener in self.listeners:
            listener(parcel, courier)

    def expire(self, parcel):
        parcel.close("expired")
        del self.parcels[parcel.id]
        self.expired_count += 1

    def retire(self, courier):
        """The courier leaves; what was in the bag waits where they stood."""
        courier.retired = True
        self.couriers.remove(courier)
        courier.zone.couriers.remove(courier)
        for parcel in courier.bag:
            courier.zone.post(parcel)
        courier.bag = []
        self.retired_count += 1
        self.vacancies += 1

    def step(self):
        self.clock += 1
        for _ in range(self.vacancies):
            self.hire()
        self.vacancies = 0
        count = len(self.zones)
        for _ in range(self.arrivals):
            origin = self.rng.below(count)
            dest = self.rng.below(count - 1)
            if dest >= origin:
                dest += 1
            self.submit(origin, dest, 1 + self.rng.below(4))
        for courier in list(self.couriers):
            courier.act()
        for zone in self.zones:
            zone.settle()

    def report(self):
        """The whole state as text, one line per thing."""
        lines = [
            "depot %d %d %d %d %d %d %d %d %d %d"
            % (
                self.clock,
                self.rng.state,
                self.next_parcel,
                self.next_courier,
                len(self.parcels),
                self.delivered_count,
                self.expired_count,
                self.retired_count,
                self.latency_total,
                self.handovers,
            )
        ]
        for zone in self.zones:
            lines.append(
                "zone %d %d %d w %s c %s"
                % (
                    zone.id,
                    zone.delivered,
                    zone.congestion,
                    ",".join("%d:%d" % (p.id, p.hops) for p in zone.waiting),
                    ",".join(str(c.id) for c in zone.couriers),
                )
            )
        for courier in self.couriers:
            lines.append(
                "courier %d %d %d %d %d %d b %s"
                % (
                    courier.id,
                    courier.zone.id,
                    courier.capacity,
                    courier.fatigue,
                    courier.resting,
                    courier.delivered,
                    ",".join("%d:%d" % (p.id, p.hops) for p in courier.bag),
                )
            )
        return "\n".join(lines) + "\n"


def run(zones, couriers, seed, steps):
    """A run with nobody watching: the state after `steps` steps."""
    depot = Depot(zones, couriers, seed)
    for _ in range(steps):
        depot.step()
    return depot.report()
