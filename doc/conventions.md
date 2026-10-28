# Conventions

How the crate is organised and the rules its code follows. The background
(why the crate exists, the module runtime of the flight controller and the
test oracle) is in `CLAUDE.md`; this document does not repeat it.

## Module layout

There is one public module per public header of the C library, with the same
name. Helpers that are not public in C are private modules. Within a layer,
submodules follow the source files of the C library where that is natural.

| Module | C counterpart | Contents |
|---|---|---|
| `parsing` (private) | `src/parsing.c`, parts of `src/utils.c` | Little-endian reads and varints |
| `crc32` (private) | `src/crc32.c` | CRC32 of blocks |
| `basic_types` | `basic_types.h` | Small value types shared by the layers (vectors, bounding boxes, intervals) |
| `colors` | `colors.h` | RGB colors and color helpers |
| `palette` | `palette.h` | Color palettes |
| `formats::binary` | `formats/binary.h` | Reading `.skyb` files: header, blocks, checksums |
| `formats::tlv` | `formats/tlv.h` | The TLV parser |
| `poly` | `poly.h` | Polynomials |
| `motion` | `motion.h` | Motion helpers |
| `trajectory` | `trajectory.h`, `stats.h`, `src/trajectory/` | Trajectories, segments, the player, the builder, statistics |
| `lights` | `lights.h`, `src/lights/` | Light programs and the light player |
| `gcs_light_control` | `gcs_light_control.h` | Light control from the ground station |
| `events` | `events.h` | Event lists and the event player |
| `yaw_control` | `yaw_control.h` | Yaw control curves and the yaw player |
| `rth_plan` | `rth_plan.h` | Return-to-home plans |
| `metadata` | `metadata.h` | Show metadata |
| `time_axis` | `time_axis.h` | Time axes |
| `screenplay` | `screenplay.h`, `src/screenplay/` | Scenes and screenplays |
| `control` | `control.h` | The show controller |

`sb_bezier_cut_at` from `utils.h` is a private helper of `poly`, since only
cutting trajectories needs it. Where the other public helpers of `utils.h`
(scaling, expanding bounding boxes) end up is decided with the layer that
needs them.

The following parts of the C library have no counterpart, because Rust
provides them:

| C | Replaced by |
|---|---|
| `buffer.h` | Slices, `Vec<u8>` and `Cow<[u8]>` |
| `memory.h` | `alloc` |
| `refcount.h` | `Rc` |
| `error.h` | The error enums of the layers (see below) |
| `decls.h`, `skybrush.h` | `lib.rs` |

`src/tools/takeoff_land_info.c` is a command line tool, not part of the
library, and is not ported.

## Errors

Each layer has its own error enum, named after the layer: `ParseError`,
`TrajectoryError`, `LightProgramError` and so on. A layer that calls another
one wraps its errors in a variant and converts them with `From`, so `?` works
across layers:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrajectoryError {
    /// The block ended in the middle of a segment.
    Truncated,
    /// The trajectory has no segments.
    Empty,
    /// The block could not be parsed.
    Parse(ParseError),
}
```

- Errors are `Copy` and hold no heap data: an error raised during playback
  must not allocate.
- Every variant corresponds to exactly one error name of the C library
  (`EPARSE`, `EINVAL`...), so that the test harness can map it. A variant may
  be finer than the C code (several variants may map to `EPARSE`), never
  coarser.
- The enums are not `#[non_exhaustive]`. The mapping in the test harness is
  an exhaustive `match`, so adding a variant fails to compile until the
  variant is mapped.
- Every enum implements `core::fmt::Display` and `core::error::Error`.

## Ownership: share, do not copy

The flight controller is short on memory, so data is shared wherever possible
and copied only when there is no other way.

- **The show file is borrowed.** The caller owns the bytes of the show file;
  everything parsed from it (trajectories, light programs, event lists...) is
  a view into those bytes with a lifetime `'a`, not a copy. Parsing computes
  offsets and validates the data; it does not decode the data into owned
  structures if it can be read from the bytes when needed.
- **Prefer a narrower view to a modified copy.** Where the C library modifies
  data in place (e.g. cutting a trajectory), first look for a representation
  that leaves the bytes alone: a sub-slice, a view with a new end time. These
  are decided per operation at the review of the layer.
- **Own only what really changes.** When an operation produces data that is
  not in the show file (a landing segment, a built trajectory), the result
  owns that data. `Cow` is used where a value is borrowed in the common case
  and owned only sometimes.
- **`Rc` only for real sharing**, e.g. a trajectory used by several scenes.
  `Arc` is not needed: a module runs on a single thread.
- **No cloning of bulk data.** Small value types (colors, vectors, times) are
  `Copy`. Types that hold or view bulk data do not implement `Clone` unless
  cloning is cheap (a view or an `Rc`).

Cutting a trajectory at the boundary of a segment narrows the borrowed
bytes. Other modifications (a cut within a segment, a landing) copy the bytes
that are kept, like in the C library (`Cow`). A possible alternative that saves that memory, at the cost of a more complex
player: keep borrowing the unmodified segments, and store the few rewritten or
appended segments at the end (a cut rewrites one, a landing appends one) in a
small inline buffer. See the documentation of `Trajectory`.

Whether the show controller borrows the show file (`ShowController<'a>`) or
holds it in an `Rc<[u8]>` is an API question for the review of the screenplay
and show controller layers.

## Allocation

The heap of a module may not grow arbitrarily while the drone is armed.

- **Loading allocates; playback does not.** Allocation is allowed while a show
  is being loaded or prepared. Playback (time updates, position, color and yaw
  queries, event iteration) must not allocate. Scratch space needed by
  playback is allocated when the player is created.
- **Allocation failure is an error, not an abort.** An allocation that fails
  through an infallible API (`Vec::push` beyond capacity, `Box::new`,
  `Rc::new`) parks the module until reboot. Collections reserve with
  `try_reserve` and then fill within the reserved capacity; the failure is
  reported as an error of the layer. `Box::try_new` and `Rc::try_new` are not
  stable, so boxes and `Rc`s are kept few and small, and are created at load
  time only.
- **Checked by a test.** The test harness installs a counting
  `#[global_allocator]` (with thread-local counters, since tests run in
  parallel) and asserts that playback calls allocate nothing.

## Panics and arithmetic

A panic parks the module until reboot, which stops the show in flight.

- `unwrap`, `expect`, `panic!` and slice indexing are denied by clippy (see
  `Cargo.toml`). Use `get`, `split_at_checked`, `?` and the like, and report
  errors. Unit tests (`#[cfg(test)]`) may use them (see `clippy.toml`).
- Where a lint is overridden in library code, use `#[expect(..., reason =
  "...")]` with the reason why the code cannot panic or truncate.
- Malformed input must not panic in any build. Arithmetic on values that come
  from the input uses `checked_*` or `saturating_*` (or `wrapping_*` where
  wrapping is intended), not the plain operators, since integer overflow
  panics in builds with overflow checks and division by zero always panics.
- Float-to-integer `as` casts saturate in Rust and are fine; integer-to-integer
  `as` casts that may truncate are replaced by `try_from` unless truncation is
  intended.
- The parsers are fuzzed with `cargo-fuzz`; the fuzzers build with overflow
  checks so that unchecked arithmetic is found.

## Naming

Code that uses the C library should be portable mechanically, so names follow
the C library: types drop `sb_` and `_t` (`sb_trajectory_player_t` →
`TrajectoryPlayer`), and functions drop `sb_<type>_` and become methods or
associated functions of the type.

A type whose C name would be misleading in Rust gets a better name. Each such
type is listed here, so that the mapping stays mechanical:

| C | Rust | Why |
|---|---|---|
| `sb_binary_file_parser_t` | `BinaryFile` | A validated view of a show file; reading the blocks is an iterator, not state of the parser |

On top of that, the following rules apply, and only these:

| C | Rust |
|---|---|
| `sb_T_make(&t, ...)` | `T::new(...)` |
| `sb_T_make_X(&t, ...)` | `T::X(...)` |
| `sb_T_init(&t, ...)` | `T::new(...)` (or `T::X(...)` for `init_X`) |
| `sb_T_destroy(&t)` | `Drop`, or nothing |
| `sb_T_update_from_X(&t, ...)` | `T::from_X(...)` |
| `sb_T_get_X(&t)` | `t.X()` |
| `sb_T_use_X(&t, ...)`, setting up a configuration | `T::X(...)` |
| `sb_T_size(&t)` | `t.len()` |
| output arguments and an error code | the return value: `Result`, `Option`, or the value |

Setters (`set_X`), predicates (`is_X`, `has_X`) and everything else keep their
C names, except for the functions listed here, whose names by the rules would
not be idiomatic Rust:

| C | Rust | Why |
|---|---|---|
| `sb_trajectory_update_from_builder(&t, &builder)` | `builder.build()` | A builder finishes with `build()`, which consumes it |
| `sb_event_list_get_ptr(&t, i)`, `sb_event_list_get_ptr_const(&t, i)` | `t.get_mut(i)`, `t.get(i)` | The names of slices and `Vec` |
| `sb_rth_plan_entry_clear(&e, plan, t)` | `RthPlanEntry::new(plan, t)` | It sets every field, so it creates an entry rather than clearing one |

Fields keep their C names too. A C name that is a Rust keyword is prefixed
with the name of the type: the `type` of `sb_event_t` is `Event::event_type`.

## Paths

Code names an item with at most one `::`: import the module or the item,
then write `error::Error` or `Error`, not `core::error::Error`. A file that
imports `core::fmt` and writes `fmt::Display` writes `error::Error` too,
not `core::error::Error`. Paths into the crate's own modules
(`crate::poly::Poly`) are imported the same way. Where idiomatic Rust
writes a longer path (e.g. in a macro or a `#[derive]`), that wins.

## Floating point and toolchain

As in `CLAUDE.md`: `f32` with math functions from `libm`, no `mul_add`, and no
contortions to be bit-exact with C. The minimum supported Rust version is
1.98.1.

## Tests

- The oracle lives in `tests/oracle`, copied from the sibling repositories at
  pinned commits by `scripts/vendor-oracle.sh` (see `tests/oracle/SOURCES`).
  It is never edited by hand; `scripts/vendor-oracle.sh --check` verifies it.
- The runners for the conformance cases, the format traces and the API traces
  are in `tests/`. Each runner has a list of known failures in
  `tests/known_failures/`. A failure that is not on the list fails the test,
  and so does a listed case that passes, so the list shrinks as the port
  progresses and nothing regresses silently. `UPDATE_KNOWN_FAILURES=1 cargo
  test` rewrites the lists from the current results; review the diff.
- The comparison of traces (`tests/common/compare.rs`) is a port of
  `compare_traces.py` of libskybrush and must agree with it. Its tests are in
  `tests/comparator.rs`, which also has an ignored test that compares two
  trace files, for cross-checking with the Python script.
- The test harness may use `std` and dev-dependencies such as `serde_json`;
  the library may not.
