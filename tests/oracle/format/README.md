# Golden traces of `.skyb` show files

This folder contains a set of show files and their _golden traces_: what the
reference implementation (`libskybrush`) computes from each show file. While
the [conformance test cases](../conformance/README.md) specify how individual
blocks must be parsed, the golden traces specify how a whole show must be
played back. Every implementation is expected to reproduce them within the
tolerances given in `tolerances.json`.

- `<name>.skyb` is a show file.
- `<name>.json` is its golden trace; see `trace.schema.json` for the meaning
  and the units of each field.
- `tolerances.json` specifies how closely a trace must match a golden trace.

## Contents of a trace

Each trace samples the show every 100 milliseconds, from the start of the show
until one second after the end of its longest component:

- the position, velocity and acceleration of the trajectory;
- the yaw and the yaw rate of the yaw control curve;
- the color of the light program;
- the RTH plan entry that applies at each sample;
- the output of the show controller (position, velocity, yaw, yaw rate,
  color, and whether the show has ended) when the show file is played as a
  screenplay with a single scene, along with the events fired by the show
  controller.

It also records the static contents of the show: the takeoff and landing
statistics of the trajectory, the events, the GCS light control setup and the
metadata. Components that are not present in the show file are omitted from
the trace, and non-finite floating-point values are represented as `null`.

## Comparing traces

The unit of each value is determined from the suffix of its key: `_mm`
(millimeters), `_mm_s` (millimeters per second), `_mm_s2` (millimeters per
second squared), `_deg` (degrees), `_deg_s` (degrees per second) and `_sec`
(seconds). The columns of RTH plan entries are named in `entry_fields`. A
value matches if it differs from the golden value by at most
`abs + rel * |golden value|`, using the tolerance of its unit in
`tolerances.json`. Integers without a unit (colors, timestamps in
milliseconds, flags, counts and the fields of events) must match exactly. The
`generator` field is informative and is not compared.

## Updating the traces

The traces are generated and checked by `libskybrush`: its `traces` test
regenerates them with the current version of the library and compares them
with the golden traces, and its `update_traces` build target replaces the
golden traces with the output of the current version. Update the traces only
when a change in the behaviour of the library is intentional, and review the
differences before committing them.
