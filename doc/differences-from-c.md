# Deliberate differences from the C library

Where the crate behaves differently from libskybrush 6.0.1 on purpose, the
difference is recorded here, with the reason. Each one was approved by the
maintainer; behaviour that is not listed here follows the C library.

Differences in the shape of the API (Rust types instead of C structs,
`Result`s instead of error codes, constructors instead of setup functions...)
are not listed; see `doc/conventions.md`.

## Show files

### The show file is never modified

In C, a trajectory or light program loaded from a show file in memory is a
view into the show file, and clearing or cutting it writes through to the show
file. In Rust, borrowed data is immutable; operations that would modify it
produce owned data instead (see `CLAUDE.md`).

The API traces record how many bytes clearing a trajectory changed in the
show file (`cleared_view.bytes_changed_in_show_file`). The API runner does not
compare it; see `NOT_APPLICABLE` in `tests/api_traces.rs`.

## Light programs

### Durations do not wrap around

Light program durations are in frames of 20 ms, with 32-bit frame counts, so a
wait can last about 8.6 × 10^10 ms. The C library computes milliseconds in an
`unsigned long`, which has 32 bits on the flight controller and wraps there.
The crate uses `u64` milliseconds. (Not ported yet.) Fixing this in C needs
a change of the public API (`unsigned long` appears in the signatures of
`lights.h`), so it is left for the next major version of the C library.
