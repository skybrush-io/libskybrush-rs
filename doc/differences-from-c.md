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
file. In Rust, borrowed data is immutable (see `CLAUDE.md`):

- `Trajectory::clear` makes the trajectory empty and stops borrowing the show
  file. Its scale becomes 1, as for any empty trajectory; in C, a cleared view
  has a scale of 0.
- `Trajectory::cut_at` narrows the borrowed bytes if it cuts at the boundary of
  a segment, and copies the bytes up to the cut otherwise.

The API traces record how many bytes clearing a trajectory changed in the
show file (`cleared_view.bytes_changed_in_show_file`). The API runner does not
compare it; see `NOT_APPLICABLE` in `tests/api_traces.rs`.

## Light programs

### Durations do not wrap around

Light program durations are in frames of 20 ms, with 32-bit frame counts, so a
wait can last about 8.6 × 10^10 ms. The C library computes milliseconds in an
`unsigned long`, which has 32 bits on the flight controller and wraps there.
The crate uses `u64` milliseconds (`LightPlayer::seek` and
`LightPlayer::color_at` take `u64` timestamps). Fixing this in C needs
a change of the public API (`unsigned long` appears in the signatures of
`lights.h`), so it is left for the next major version of the C library.

## Show controller

### The players follow the components of the current scene

In C, the show controller creates the players of a scene when the scene
becomes the current one, and keeps them until another scene does; replacing
the trajectory, the light program, the yaw control or the events of the
current scene has no effect until then. In Rust, the controller checks the
components of the current scene at every update and creates a new player for
a component that was replaced. Players whose component did not change keep
their state, so that modifying the screenplay, e.g. to pause the show by
adding segments to the time axis, does not fire the events of the scene
again. See `tests/show_controller.rs`.
