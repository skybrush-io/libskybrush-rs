# Deviations from the Rust API guidelines

The public API follows the [Rust API guidelines](https://rust-lang.github.io/api-guidelines/checklist.html)
except where noted here, with the reasons. The deviations that users notice
directly are also explained in the design notes of the crate documentation
(`src/lib.rs`).

- **C-SEND-SYNC.** Scenes share their components through `Rc`, so
  `ScreenplayScene`, `Screenplay` and `ShowController` are neither `Send` nor
  `Sync`: a module runs on a single thread, and `Arc` would cost atomic
  operations for nothing. Trajectory segments cache their polynomials in a
  `OnceCell`, so they and the trajectory players are `Send` but not `Sync`.
  The data read from a show file is `Send` and `Sync`. `tests/auto_traits.rs`
  checks all of this at compile time.
- **C-NEWTYPE (units).** Times, distances and angles are `u32` milliseconds,
  `f32` seconds, millimeters and degrees, with the unit in the name
  (`duration_msec`, `origin_sec`), not newtypes. This keeps ports from C
  mechanical and the arithmetic comparable with the oracle;
  `core::time::Duration` would not hold the signed origins of scenes either.
- **C-COLLECT.** `EventList`, `TimeAxis` and `Screenplay` do not implement
  `FromIterator` and `Extend`, which cannot report a failed allocation (see
  [Allocation](conventions.md#allocation)). They have methods that return a `Result`.
- **C-CUSTOM-TYPE (sentinel values).** Missing values are `None`, except
  where infinity is the mathematical answer (`travel_time_for_distance` at
  zero speed, an acceleration limit of infinity), and in the public fields of
  `RthPlanEntry` (`max_acceleration`, `landing_velocity`), which mirror the
  entries of the file format and the C struct.
- **C-STRUCT-PRIVATE.** Plain data types have public fields and are not
  `#[non_exhaustive]`: the vectors, `Event`, `RthPlanEntry`, `TimeSegment`,
  `YawSetpoint`, the statistics of trajectories and `ControlOutputTime`.
  Users build some of them with struct literals, as in C; a new field is a
  breaking change, which is accepted.
- **C-NEWTYPE-HIDE.** `EventType` and `BinaryBlockType` expose their `u8`:
  they are open-ended codes, and show files may contain codes that the crate
  does not know.
- **C-BITFLAG.** Sets of flags come from the private macro `impl_flags!`
  (see [Sets of flags](conventions.md#sets-of-flags)) instead of the `bitflags` crate,
  which would be a public dependency. The method names are those of
  `bitflags`, so switching later would hardly change the API.
- **Names (C-CONV, C-ITER, C-GETTER).** Names follow the C library, by the
  rules of [Naming](conventions.md#naming), so that ports stay mechanical: e.g.
  `num_points` and `num_entries` instead of `len`, or `has_any_flag` next to
  the methods of the flags themselves.
- **C-SERDE.** Not implemented; an optional `serde` feature can be added when
  a tool needs it.
- **C-EXAMPLE.** The crate and its main types have examples, not every item.
- **C-PERMISSIVE.** The crate is licensed under GPL-3.0-or-later, like the C
  library and ArduPilot, rather than MIT or Apache-2.0.
