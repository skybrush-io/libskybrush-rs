# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

The first release: a rewrite of the C library libskybrush 6.0.2 in Rust,
with the fixes made to the C library since then.

### Added

- Reading show files: trajectories, light programs, yaw control, events,
  return-to-home plans, the setup of light control from the ground station,
  and the metadata of the show.
- Players of the components, the statistics of trajectories, the trajectory
  builder, cuts and landings, and the trajectories of RTH plan entries.
- Time axes, screenplays and the show controller.

<!-- Links to the tags; cargo-release updates them (see release.toml) -->
[Unreleased]: https://github.com/skybrush-io/libskybrush-rs/commits/main
