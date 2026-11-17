"""A packet relay network.

Nodes sit on a ring with a few chords. Every tick the hub injects some
packets, then each node in turn forwards up to BURST packets from its queue
toward their destinations, choosing the least loaded neighbour.
"""

MASK = 2**64 - 1
BURST = 4
INJECT = 3


class Rng:
    def __init__(self, seed):
        self.state = seed & MASK

    def below(self, n):
        self.state = (self.state * 6364136223846793005 + 1442695040888963407) & MASK
        return (self.state >> 33) % n


class Packet:
    def __init__(self, pid, dst, ttl, size, born):
        self.pid = pid
        self.dst = dst
        self.ttl = ttl
        self.size = size
        self.born = born


class Node:
    def __init__(self, hub, nid):
        self.hub = hub
        self.nid = nid
        self.label = "relay-%04d" % nid
        self.queue = []
        self.peers = []
        self.load = 0
        self.sent = 0

    def pick(self, packet):
        """The neighbour to hand a packet to: its destination if that is a
        neighbour, otherwise the neighbour with the least load (the first
        such in peer order)."""
        best = None
        for peer in self.peers:
            if peer.nid == packet.dst:
                return peer
            if best is None or peer.load < best.load:
                best = peer
        return best

    def pump(self):
        budget = BURST
        while budget > 0 and self.queue:
            packet = self.queue.pop(0)
            self.load -= packet.size
            budget -= 1
            if packet.dst == self.nid:
                self.hub.delivered[self.nid] += packet.size
                self.hub.latency += self.hub.clock - packet.born
                continue
            packet.ttl -= 1
            if packet.ttl == 0:
                self.hub.dropped += 1
                continue
            peer = self.pick(packet)
            peer.queue.append(packet)
            peer.load += packet.size
            self.sent += 1
            self.hub.traffic[self.nid * len(self.hub.nodes) + peer.nid] += 1


class Hub:
    def __init__(self, count, seed):
        self.rng = Rng(seed)
        self.clock = 0
        self.next_pid = 1
        self.dropped = 0
        self.latency = 0
        self.nodes = [Node(self, nid) for nid in range(count)]
        self.delivered = [0] * count
        self.traffic = [0] * (count * count)
        for node in self.nodes:
            for other in ((node.nid + 1) % count, (node.nid - 1) % count, (node.nid * 7 + 3) % count):
                peer = self.nodes[other]
                if peer is not node and peer not in node.peers:
                    node.peers.append(peer)

    def inject(self):
        count = len(self.nodes)
        for _ in range(INJECT):
            src = self.rng.below(count)
            dst = self.rng.below(count)
            ttl = 8 + self.rng.below(8)
            size = 1 + self.rng.below(9)
            node = self.nodes[src]
            node.queue.append(Packet(self.next_pid, dst, ttl, size, self.clock))
            node.load += size
            self.next_pid += 1

    def tick(self):
        self.clock += 1
        self.inject()
        for node in self.nodes:
            node.pump()

    def state(self):
        out = []
        for node in self.nodes:
            out.append(node.load)
            out.append(len(node.queue))
            out.append(node.sent)
            out.append(self.delivered[node.nid])
            out.append(sum(ord(c) for c in node.label))
            out.append(sum((k + 1) * p.pid for k, p in enumerate(node.queue)))
        out.append(self.dropped)
        out.append(self.latency)
        out.append(self.clock)
        out.append(sum((k + 1) * v for k, v in enumerate(self.traffic)))
        return out


def run(count, seed, ticks):
    """Builds a network of `count` nodes (at least 2), runs it for `ticks`
    ticks and returns its state as a flat list of integers."""
    hub = Hub(count, seed)
    for _ in range(ticks):
        hub.tick()
    return hub.state()
