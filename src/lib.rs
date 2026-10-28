//! Parsing and playback of Skybrush drone show files.
//!
//! This crate is a Rust rewrite of the C library `libskybrush`, with feature
//! parity to version 6.0.1 of the C library where possible. It is meant to be
//! used from firmware, therefore it does not depend on the standard library;
//! it requires a global allocator only.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod basic_types;
pub mod colors;
mod crc32;
pub mod events;
pub mod formats;
pub mod gcs_light_control;
pub mod lights;
pub mod metadata;
pub mod motion;
pub mod palette;
mod parsing;
pub mod poly;
pub mod rth_plan;
pub mod trajectory;
pub mod yaw_control;
