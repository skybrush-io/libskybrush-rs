#!/usr/bin/env python3
"""Seeds the corpora of the fuzz targets from the test oracle.

Usage: fuzz/seed.py

The show files of the format traces and the show files of the conformance
cases become seeds of the show_file target, and the bodies of their blocks
become seeds of the target of their block type. The corpora are in
fuzz/corpus/<target>/, which is not under version control; seeds that are
already there are kept.
"""

import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ORACLE = ROOT / "tests" / "oracle"
CORPUS = ROOT / "fuzz" / "corpus"

# The fuzz target of each block type
TARGETS = {
    1: "trajectory",
    2: "light_program",
    4: "rth_plan",
    5: "yaw_control",
    6: "event_list",
    7: "tlv",
    8: "tlv",
}


def blocks(data):
    """The (type, body) pairs of the blocks of a show file, as far as they
    can be read."""
    if data[:4] != b"skyb" or len(data) < 5:
        return
    offset = 5
    if data[4] == 2:
        if len(data) < 6:
            return
        offset = 6 + (4 if data[5] & 0x01 else 0)
    while offset + 3 <= len(data):
        block_type = data[offset]
        length = int.from_bytes(data[offset + 1 : offset + 3], "little")
        if block_type == 0:
            return
        body = data[offset + 3 : offset + 3 + length]
        yield block_type, body
        offset += 3 + length


def save(target, data):
    directory = CORPUS / target
    directory.mkdir(parents=True, exist_ok=True)
    (directory / hashlib.sha1(data).hexdigest()).write_bytes(data)


def main():
    shows = [path.read_bytes() for path in sorted((ORACLE / "format").glob("*.skyb"))]
    cases = json.loads((ORACLE / "conformance" / "cases.json").read_text())["cases"]
    shows += [bytes.fromhex(case["bytes"]) for case in cases]

    for show in shows:
        save("show_file", show)
        for block_type, body in blocks(show):
            target = TARGETS.get(block_type)
            if target:
                save(target, body)

    for directory in sorted(CORPUS.iterdir()):
        print(f"{directory.name}: {len(list(directory.iterdir()))} seeds")


if __name__ == "__main__":
    main()
