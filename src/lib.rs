//! Parsing and playback of Skybrush drone show files.
//!
//! This crate is a Rust rewrite of the C library `libskybrush`, with feature
//! parity to version 6.0.0 of the C library where possible. It is meant to be
//! used from firmware, therefore it does not depend on the standard library;
//! it requires a global allocator only.

#![no_std]

extern crate alloc;

pub mod colors;
mod crc32;
pub mod formats;
#[allow(
    dead_code,
    reason = "used by the parsers of the blocks, not ported yet"
)]
mod parsing;
