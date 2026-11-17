"""How the rest of the system uses `depot.py` today.

This code is not being ported. It is here to show what callers do with the
objects: they keep them, read them later, and change a courier.
`tests/callers.rs` is the same code written against the Rust port.
"""

from depot import Depot


def scenario(zones, couriers, seed, steps):
    depot = Depot(zones, couriers, seed)
    log = []
    kept = []

    def note(parcel, courier):
        log.append("d %d %d %d %d" % (parcel.id, courier.id, parcel.age(), courier.load()))
        if parcel.id % 5 == 0:
            courier.rest(2)
        if parcel.id % 4 == 0:
            kept.append(parcel)

    depot.on_delivered(note)
    watched = depot.courier(0)
    hub = depot.zone(0)
    mine = []
    for t in range(steps):
        if t % 7 == 0:
            mine.append(depot.submit(t % zones, (t + 2) % zones, 2))
        if t == steps // 2:
            watched.capacity = 3
        depot.step()
        log.append(
            "w %d %d %d z %d %d"
            % (watched.load(), watched.capacity, int(watched.retired), len(hub.waiting), hub.congestion)
        )
    for parcel in mine:
        carrier = -1 if parcel.carrier is None else parcel.carrier.id
        log.append("m %d %s %d %d" % (parcel.id, parcel.status, parcel.age(), carrier))
    for parcel in kept:
        log.append("k %d %s %d" % (parcel.id, parcel.status, parcel.age()))
    gone = depot.courier(0) is None
    log.append("g %d" % int(gone))
    return "\n".join(log) + "\n" + depot.report()
