# Conformance test cases of the `.skyb` format

This folder contains language-neutral test cases that specify how parsers of
the Skybrush binary show file format (`.skyb`) must behave on valid, edge-case
and malformed inputs. Every implementation of the format is expected to pass
all of them.

- `cases.json` contains the test cases. Do not edit it by hand.
- `cases.schema.json` is the JSON schema of `cases.json`.
- `../../../etc/scripts/generate-skyb-conformance-cases.mjs` is the source of
  truth of the test cases. It describes each case with small helper functions
  so that block lengths and checksums are calculated automatically.

To regenerate `cases.json` after changing the generator, run `npm run build`
(or `npm run build:skyb-conformance` to regenerate the test cases only).

## Structure of a test case

Each test case has a stable `id`, a human-readable `description`, references
to the pages of the specification that it is based on (`spec`), the contents
of the show file encoded in hexadecimal (`bytes`), and the expected results
(`expect`). The expected results are derived from the specification, _not_
from the output of any particular implementation.

`expect.file` is the result of opening the file, i.e. validating the header,
the checksum and the header of the first block:

- `SUCCESS`: the file can be opened.
- `EPARSE`: the signature is invalid, the version is not supported, or the
  file ends in the middle of the header.
- `ECORRUPTED`: the checksum does not match the contents of the file.
- `EREAD`: the file ends in the middle of the header of the first block.

When the file can be opened, each block type is loaded from the file, and the
result of loading it must match `expect.<block type>.result`:

- `SUCCESS`: the block was loaded.
- `ENOENT`: the file contains no block of this type. **Block types that are
  not listed in `expect` must be reported as missing.**
- `EREAD`: the file ends before the end of a block that had to be read or
  skipped while looking for the block.
- `ECORRUPTED`: the body of the block is malformed.

When a block is loaded successfully, the other properties of
`expect.<block type>` describe its contents: positions of the trajectory at
given time instants, colors of the light program at given time instants and
the time when it ends, yaw angles, the complete list of events, the points of
the RTH plan, and so on. Properties that are absent are not checked. See
`cases.schema.json` for the units and the exact meaning of each property.

Floating-point values must match within the tolerances given at the top of
`cases.json`. Colors, timestamps and raw values must match exactly.

Implementations are encouraged to run each case both on in-memory buffers and
on files read from disk, as both code paths must behave identically.

## Users

`libskybrush` runs these test cases as part of its unit tests; see
`test/conformance/generate_tests.py` and `test/unit/test_conformance.c` in
that repository.
