"""The workload for the check that runs inside the upload request.

The uploaded file is built when this module is loaded, which is not timed:
in production the bytes arrive with the request. The measured function
starts from those bytes, so reading the file is inside what is measured,
as it would be inside a port that takes the bytes.
"""

import dedupe
import workload

SIZE = 200000
UPLOAD = ("id,name,postcode\n" + "\n".join("%d,%s,%s" % (r["id"], r["name"], r["postcode"]) for r in workload.records(SIZE))).encode("utf-8")


def run():
    """One upload of the largest customer file seen so far."""
    return len(dedupe.find_duplicates_in_upload(UPLOAD))
