//! Converting an entry of an RTH plan to a trajectory that can be flown.

use core::{error, fmt};

use libm::{ceilf, fabsf};

use super::{RthPlanEntry, RthPlanEntryFlags};
use crate::basic_types::Vector3;
use crate::trajectory::{Trajectory, TrajectoryBuilder, TrajectoryBuilderError, TrajectoryError};

/// The largest scale of a trajectory.
const MAX_SCALE: u8 = 127;

/// Why an RTH plan entry could not be converted to a trajectory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RthTrajectoryError {
    /// A coordinate is too large for any scale of a trajectory.
    ScaleOverflow,
    /// A duration is negative or NaN.
    InvalidDuration,
    /// A duration does not fit into 32-bit milliseconds.
    DurationOverflow,
    /// The trajectory could not be built.
    Builder(TrajectoryBuilderError),
    /// The trajectory that was built could not be read.
    Trajectory(TrajectoryError),
}

impl fmt::Display for RthTrajectoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ScaleOverflow => f.write_str("a coordinate is too large for a trajectory"),
            Self::InvalidDuration => f.write_str("a duration is negative or NaN"),
            Self::DurationOverflow => f.write_str("a duration is too long"),
            Self::Builder(err) => write!(f, "{err}"),
            Self::Trajectory(err) => write!(f, "{err}"),
        }
    }
}

impl error::Error for RthTrajectoryError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Builder(err) => Some(err),
            Self::Trajectory(err) => Some(err),
            _ => None,
        }
    }
}

impl From<TrajectoryBuilderError> for RthTrajectoryError {
    fn from(err: TrajectoryBuilderError) -> Self {
        Self::Builder(err)
    }
}

impl From<TrajectoryError> for RthTrajectoryError {
    fn from(err: TrajectoryError) -> Self {
        Self::Trajectory(err)
    }
}

impl Trajectory<'static> {
    /// The trajectory of the action of an RTH plan entry, flown from the
    /// given position, which is where the plan expects the drone to be.
    ///
    /// The trajectory holds the start position until the time of the entry
    /// and its delay, rises along the neck, moves to the target, holds the
    /// position for the delay after the action, and descends to the landing
    /// altitude if the entry has a landing velocity. The drone is expected to
    /// land when the trajectory ends.
    ///
    /// # Errors
    ///
    /// Returns an error if a coordinate or a duration is out of range, or
    /// memory could not be allocated.
    pub fn from_rth_plan_entry(
        entry: &RthPlanEntry,
        start: Vector3,
    ) -> Result<Self, RthTrajectoryError> {
        let max_acceleration =
            if entry.max_acceleration <= 0.0 || !entry.max_acceleration.is_finite() {
                f32::INFINITY
            } else {
                entry.max_acceleration
            };

        // The smallest scale that holds every coordinate of the action
        let mut scale = 1;
        update_scale(&mut scale, &[start.x, start.y, start.z])?;
        if entry.flags.contains(RthPlanEntryFlags::HAS_NECK) {
            update_scale(&mut scale, &[start.z + entry.pre_neck])?;
        }
        if entry.flags.contains(RthPlanEntryFlags::HAS_TARGET_XY) {
            update_scale(
                &mut scale,
                &[entry.landing_target.x, entry.landing_target.y],
            )?;
        }
        if entry.flags.contains(RthPlanEntryFlags::HAS_TARGET_Z) {
            update_scale(&mut scale, &[entry.arrival_altitude])?;
        }
        update_scale(&mut scale, &[entry.landing_altitude])?;

        let start_time_sec = if entry.time_sec < 0.0 {
            0.0
        } else {
            entry.time_sec
        };
        let start = start.with_yaw(0.0);

        let mut builder = TrajectoryBuilder::new(scale)?;
        builder.set_start_position(start)?;

        // Hold the start position until the time of the entry and its delay
        let pre_delay_sec = if entry.pre_delay_sec > 0.0 {
            entry.pre_delay_sec
        } else {
            0.0
        };
        builder.hold_position_for(sec_to_msec(start_time_sec + pre_delay_sec)?)?;

        let mut target = start;

        // The neck; the flag is not checked, as in the C library
        if entry.pre_neck != 0.0 || entry.pre_neck_duration_sec != 0.0 {
            target.z += entry.pre_neck;
            let duration_msec = sec_to_msec(entry.pre_neck_duration_sec)?.max(1);
            builder.move_to_in_time(target, duration_msec, max_acceleration)?;
        }

        // The horizontal and vertical movement
        if entry.flags.contains(RthPlanEntryFlags::HAS_TARGET_XY) {
            target.x = entry.landing_target.x;
            target.y = entry.landing_target.y;
        }
        if entry.flags.contains(RthPlanEntryFlags::HAS_TARGET_Z) {
            target.z = entry.arrival_altitude;
        }
        if entry
            .flags
            .intersects(RthPlanEntryFlags::HAS_TARGET_XY | RthPlanEntryFlags::HAS_TARGET_Z)
        {
            let duration_msec = sec_to_msec(entry.duration_sec)?.max(1);
            builder.move_to_in_time(target, duration_msec, max_acceleration)?;
        }

        builder.hold_position_for(sec_to_msec(entry.post_delay_sec)?)?;

        // The descent to the landing altitude
        if entry.landing_velocity > 0.0 && entry.landing_velocity.is_finite() {
            let current_altitude = target.z;
            target.z = entry.landing_altitude;
            let duration_sec = fabsf(current_altitude - target.z) / entry.landing_velocity;
            let duration_msec = sec_to_msec(duration_sec)?.max(1);
            builder.move_to_in_time(target, duration_msec, max_acceleration)?;
        }

        Ok(builder.build()?)
    }
}

/// Increases the scale of a trajectory if needed, so that it holds the given
/// coordinates.
fn update_scale(scale: &mut u8, coordinates: &[f32]) -> Result<(), RthTrajectoryError> {
    let max = f32::from(*scale) * f32::from(i16::MAX);
    // Like in the C library, a NaN as the first coordinate wins
    let mut magnitudes = coordinates.iter().map(|&coordinate| fabsf(coordinate));
    let first = magnitudes.next().unwrap_or(0.0);
    let max_coordinate = magnitudes.fold(
        first,
        |max, magnitude| {
            if magnitude > max { magnitude } else { max }
        },
    );
    if max_coordinate > max {
        let new_scale = ceilf(max_coordinate / f32::from(i16::MAX));
        if new_scale > f32::from(MAX_SCALE) {
            return Err(RthTrajectoryError::ScaleOverflow);
        }
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "an integer between 1 and 127"
        )]
        let new_scale = new_scale as u8;
        *scale = new_scale;
    }
    Ok(())
}

/// Converts a duration in seconds to milliseconds, truncated.
fn sec_to_msec(duration_sec: f32) -> Result<u32, RthTrajectoryError> {
    // 2^32 is a float, unlike u32::MAX
    const LIMIT_MSEC: f32 = 4_294_967_296.0;
    if duration_sec < 0.0 || duration_sec.is_nan() {
        return Err(RthTrajectoryError::InvalidDuration);
    }
    let duration_msec = duration_sec * 1000.0;
    if duration_msec >= LIMIT_MSEC {
        return Err(RthTrajectoryError::DurationOverflow);
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a non-negative float below 2^32"
    )]
    Ok(duration_msec as u32)
}
