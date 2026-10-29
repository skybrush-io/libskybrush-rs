# Fuzzing

The fuzz targets check that the crate neither panics nor hangs on malformed
input. They need [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) and a
nightly toolchain:

    cargo install cargo-fuzz
    fuzz/seed.py
    cargo +nightly fuzz run show_file -- -timeout=10

`fuzz/seed.py` seeds the corpora in `fuzz/corpus/` from the show files and the
conformance cases of the test oracle. The targets:

| Target | Input |
|---|---|
| `show_file` | A whole show file: every component, and the show controller, also after replacing its screenplay |
| `trajectory` | The body of a trajectory block: playback, statistics, cuts, landings and the builder |
| `light_program` | The body of a light program block |
| `yaw_control` | The body of a yaw control block |
| `event_list` | The body of an event list block, and the modifications of event lists |
| `rth_plan` | The body of an RTH plan block, and the trajectories of its entries |
| `tlv` | The body of a GCS light control setup or show metadata block |

Show files of version 2 are also parsed as version 1 files with the same
blocks, so that mutations of the blocks are not rejected for a wrong checksum.

Light programs are executed one command at a time, so the targets seek at
most ten minutes into them; a program of short waits takes long to seek far
ahead, as in the C library.

## On GitHub Actions

The `Fuzz` workflow (`.github/workflows/fuzz.yml`) runs only when started by
hand, from the Actions tab or with

    gh workflow run fuzz.yml -f seconds=3600

It builds the fuzz targets once, with the crate compiled a single time, and
then runs each target in its own job for the given number of seconds. The
input that made a target fail and the grown corpus are uploaded as
artifacts.
