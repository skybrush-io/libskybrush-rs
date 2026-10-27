# Deliberate differences from the C library

Where the crate behaves differently from libskybrush 6.0.0 on purpose, the
difference is recorded here, with the reason. Each one was approved by the
maintainer; behaviour that is not listed here follows the C library.

Differences in the shape of the API (Rust types instead of C structs,
`Result`s instead of error codes, constructors instead of setup functions...)
are not listed; see `doc/conventions.md`.

## Colors

### RGBW conversion with a reference color that lacks a channel

`RgbwConversion::reference_color` (`sb_rgbw_conversion_use_reference_color`)
models a white LED that emits the reference color. When a component of the
reference is zero, the white LED emits none of that channel.

- **C:** the channel gets a multiplier of 255 and a divisor of 1/255. A color
  that has none of that channel cannot use the white LED at all, and white
  subtracts up to one unit from the channel.
- **Rust:** the channel does not limit the white level, and white subtracts
  nothing from it. A black reference leaves the white LED off.

Reason: the C behaviour is physically wrong, and it matters in practice:
colors of low temperatures (up to about 1900 K, e.g. 1000 K = (255, 67, 0))
have no blue. Results of the C library, compiled from the 6.0.0 tag:

| Reference | Color | C | Rust |
|---|---|---|---|
| (255, 200, 0) | (200, 200, 0) | (200, 200, 0, 0) | (0, 43, 0, 200) |
| (255, 200, 0) | (100, 100, 50) | (0, 21, 49, 100) | (0, 21, 50, 100) |
| (0, 0, 0) | (10, 20, 30) | (9, 19, 29, 255) | (10, 20, 30, 0) |
| 1000 K = (255, 67, 0) | (255, 67, 0) | (255, 67, 0, 0) | (0, 0, 0, 255) |

Conversions with references that have all three channels give the same
results as in C. The oracle has no reference without a channel, so it does
not test this either way; the unit tests in `src/colors.rs` do.

### Remembering the color temperature of a conversion

`sb_rgbw_conversion_use_color_temperature` returns early if the conversion was
already set up with the same temperature. A conversion set up with a reference
color records a temperature of 0, so setting it up with a temperature of 0
afterwards keeps the reference color.

In Rust, conversions are created by constructors, and
`RgbwConversion::temperature` is `None` unless the conversion was created from
a color temperature, so the caller can skip the setup without this mix-up.

### Converting NaN to a color component

Where the C library converts a float result to a color component, a NaN (e.g.
from a NaN ratio of `sb_rgb_color_linear_interpolation` or a NaN
temperature) is undefined behaviour in C. In Rust, the component is zero.

## Polynomials

### Extrema of polynomials of degree above 3

`sb_poly_get_extrema` is implemented for polynomials of degree at most 3. For
higher degrees, it returns `SB_SUCCESS` without writing its result, although
its documentation says that it returns `SB_EUNIMPLEMENTED`. `Poly::extrema`
returns `PolyError::UnsupportedDegree`, as documented; Rust has no way to
succeed without a result. Trajectories in show files have segments of degree at
most 3, so this does not affect them.

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
The crate uses `u64` milliseconds. (Not ported yet.)
