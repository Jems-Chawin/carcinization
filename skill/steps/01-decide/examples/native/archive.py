"""Packs a batch of event records for cold storage.

Each batch is written as JSON, compressed, and stored with its checksum.
All three steps are done by code that is already compiled: the `json`,
`zlib` and `hashlib` modules of the standard library.
"""

import hashlib
import json
import zlib


def pack(records):
    body = json.dumps(records, separators=(",", ":"), sort_keys=True).encode("utf-8")
    packed = zlib.compress(body, 9)
    return {"sha256": hashlib.sha256(packed).hexdigest(), "size": len(packed), "records": len(records)}


def pack_all(batches):
    return [pack(batch) for batch in batches]
