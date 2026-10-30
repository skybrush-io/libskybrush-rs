//! Parsing and playback of [Skybrush] drone show files.
//!
//! This crate is a Rust rewrite of the C library [libskybrush], with feature
//! parity to version 6.0.4 of the C library where possible. It is meant to be
//! used from firmware, therefore it does not depend on the standard library;
//! it requires a global allocator only.
//!
//! # Overview
//!
//! A show file (`.skyb`) is a list of blocks, read with
//! [`BinaryFile`](formats::binary::BinaryFile). Each block holds a component
//! of the show for one drone:
//!
//! - its [trajectory](trajectory::Trajectory),
//! - its [light program](lights::LightProgram),
//! - its [yaw control](yaw_control::YawControl),
//! - its [events](events::EventList), such as pyro triggers,
//! - the collective [return-to-home plan](rth_plan::RthPlan),
//! - the [setup of light control](gcs_light_control::GcsLightControlSetup)
//!   from the ground station, and the [metadata](metadata::ShowMetadata) of
//!   the show.
//!
//! Each component has a player that evaluates it at a time instant. A
//! [screenplay](screenplay::Screenplay) places scenes of these components on
//! the show clock, each with a [time axis](time_axis::TimeAxis) that may slow
//! down, stop or speed up the time of the scene, and the
//! [show controller](control::ShowController) tells what the drone should do
//! at a time instant of the screenplay.
//!
//! Components read from a show file in memory borrow their bytes from it
//! where they are not modified, so loading a show copies little. Playback
//! does not allocate, and malformed input is reported as an error rather than
//! a panic.
//!
//! # Example
//!
//! ```
//! use skybrush::control::ShowController;
//! use skybrush::formats::binary::BinaryFile;
//! use skybrush::screenplay::Screenplay;
//! # use skybrush::basic_types::Vector3WithYaw;
//! # use skybrush::trajectory::TrajectoryBuilder;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # // A show file with a single trajectory block: a climb to 5 m in 5 s
//! # let mut builder = TrajectoryBuilder::new(1)?;
//! # builder.set_start_position(Vector3WithYaw::default())?;
//! # let top = Vector3WithYaw { z: 5000.0, ..Vector3WithYaw::default() };
//! # builder.move_to_in_time(top, 5000, 2000.0)?;
//! # let body = builder.build()?.bytes().to_vec();
//! # let mut bytes = b"skyb\x01\x01".to_vec();
//! # bytes.extend_from_slice(&u16::try_from(body.len())?.to_le_bytes());
//! # bytes.extend_from_slice(&body);
//! // `bytes` holds the contents of a show file
//! let file = BinaryFile::new(&bytes)?;
//! let screenplay = Screenplay::from_binary_file(&file)?;
//! let mut controller = ShowController::new(screenplay);
//!
//! // What the drone should do 2.5 s into the show
//! controller.update_time_msec(2500);
//! let output = controller.current_output();
//! let position = output.position().ok_or("the show has no trajectory")?;
//! assert!(position.z > 0.0 && position.z < 5000.0);
//!
//! // Fire the events that are due by then
//! while let Some(event) = controller.next_event() {
//!     println!("{event:?}");
//! }
//! # Ok(())
//! # }
//! ```
//!
//! Positions are in millimeters, times in milliseconds or seconds as the
//! names of the functions tell, and angles in degrees.
//!
//! # Design notes
//!
//! A few choices follow from the firmware that the crate is written for, and
//! differ from what is usual in Rust crates:
//!
//! - **Threads.** A flight controller module runs on a single thread, so
//!   scenes share their components through [`Rc`](alloc::rc::Rc): a
//!   [`Screenplay`](screenplay::Screenplay) and the
//!   [`ShowController`](control::ShowController) are neither `Send` nor
//!   `Sync`, and stay on the thread that created them. The components read
//!   from a show file are `Send` and `Sync`. Trajectory players are `Send`
//!   but not `Sync`, since trajectory segments cache their polynomials in a
//!   [`OnceCell`](core::cell::OnceCell).
//! - **Units.** Times, distances and angles are plain numbers (`u32`
//!   milliseconds, `f32` seconds, millimeters and degrees) with the unit in
//!   the name, as in the C library, rather than types such as
//!   [`Duration`](core::time::Duration).
//! - **Allocation.** Memory is allocated when a show is loaded, and a failed
//!   allocation is reported as an error instead of aborting. Therefore the
//!   collections do not implement `FromIterator` and `Extend`, which cannot
//!   report it; they have methods that return a `Result` instead.
//! - **Missing values** are `None`, except where infinity is the
//!   mathematical answer (e.g. the travel time at zero speed, or an
//!   acceleration without a limit) and in the fields of
//!   [`RthPlanEntry`](rth_plan::RthPlanEntry), which follow the file format.
//!
//! [Skybrush]: https://skybrush.io
//! [libskybrush]: https://github.com/skybrush-io/libskybrush

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod basic_types;
pub mod colors;
pub mod control;
mod crc32;
pub mod events;
mod flags;
pub mod formats;
pub mod gcs_light_control;
pub mod lights;
pub mod metadata;
pub mod motion;
pub mod palette;
mod parsing;
pub mod poly;
pub mod rth_plan;
pub mod screenplay;
pub mod time_axis;
pub mod trajectory;
pub mod yaw_control;
