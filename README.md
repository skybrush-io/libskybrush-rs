# libskybrush-rs

Parsing and playback of Skybrush drone show files in Rust.

This is a rewrite of the C library
[libskybrush](https://github.com/skybrush-io/libskybrush), with feature parity
to version 6.0.0 of the C library where possible. The API is idiomatic Rust and
does not follow the conventions of the C library.

The crate is `no_std` and requires a global allocator only, so it can be used
in firmware, e.g., in loadable ArduPilot modules.

## License

Copyright 2026 CollMot Robotics Ltd.

libskybrush-rs is free software: you can redistribute it and/or modify it under
the terms of the GNU General Public License as published by the Free Software
Foundation, either version 3 of the License, or (at your option) any later
version.

libskybrush-rs is distributed in the hope that it will be useful, but WITHOUT
ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS
FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.

You should have received a copy of the GNU General Public License along with
this program. If not, see <https://www.gnu.org/licenses/>.
