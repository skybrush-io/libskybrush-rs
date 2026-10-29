//! Building trajectories from lines, Bézier curves and holds.

use alloc::borrow::Cow;
use alloc::collections::TryReserveError;
use alloc::vec::Vec;
use core::{error, fmt};

use libm::{floorf, hypotf};

use super::{HEADER_LENGTH, Trajectory, TrajectoryError, msec_to_sec};
use crate::basic_types::Vector3WithYaw;
use crate::motion::{cubic_bezier_from_velocity_constraints, travel_velocity_for_distance};
use crate::poly::MAX_POLY_COEFFS;

/// The longest duration of a segment that the builder writes, in
/// milliseconds.
const MAX_DURATION_MSEC: u32 = 60_000;

/// The bits of a linear segment along the X axis in the header of a segment;
/// shifted left by 2 for Y and by 4 for Z.
const LINEAR: u8 = 0x01;

/// The bits of a cubic Bézier segment along the X axis in the header of a
/// segment; shifted left by 2 for Y and by 4 for Z.
const BEZIER: u8 = 0x02;

/// Why a trajectory could not be built or modified.
///
/// # Examples
///
/// ```
/// use skybrush::basic_types::Vector3WithYaw;
/// use skybrush::trajectory::TrajectoryBuilder;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// // Coordinates are stored in units of 1 mm
/// let mut builder = TrajectoryBuilder::new(1)?;
/// builder.set_start_position(Vector3WithYaw::default())?;
/// let target = Vector3WithYaw { x: 3000.0, z: 2000.0, ..Vector3WithYaw::default() };
/// builder.append_line(target, 4000)?;
/// builder.hold_position_for(1000)?;
/// let trajectory = builder.build()?;
/// assert_eq!(trajectory.total_duration_msec(), 5000);
/// assert_eq!(trajectory.end_position(), target);
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrajectoryBuilderError {
    /// The scale is zero or larger than 127.
    InvalidScale,
    /// The start position cannot be set after a segment was added.
    StartPositionAlreadyUsed,
    /// The duration of a segment is zero.
    ZeroDuration,
    /// The duration of a segment is longer than 60 seconds.
    DurationTooLong,
    /// A coordinate cannot be represented with the scale of the trajectory,
    /// or it is NaN.
    CoordinateOutOfRange,
    /// Memory could not be allocated.
    OutOfMemory,
}

impl fmt::Display for TrajectoryBuilderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidScale => "the scale must be between 1 and 127",
            Self::StartPositionAlreadyUsed => {
                "the start position cannot be set after a segment was added"
            }
            Self::ZeroDuration => "the duration of the segment is zero",
            Self::DurationTooLong => "the duration of the segment is longer than 60 seconds",
            Self::CoordinateOutOfRange => "a coordinate is out of the range of the scale",
            Self::OutOfMemory => "out of memory",
        })
    }
}

impl error::Error for TrajectoryBuilderError {}

impl From<TryReserveError> for TrajectoryBuilderError {
    fn from(_: TryReserveError) -> Self {
        Self::OutOfMemory
    }
}

/// Builds a trajectory segment by segment.
///
/// Every operation is all-or-nothing: an operation that fails leaves the
/// builder as it was.
#[derive(Debug)]
pub struct TrajectoryBuilder {
    bytes: Vec<u8>,
    last_position: Vector3WithYaw,
    scale: f32,
}

impl TrajectoryBuilder {
    /// A builder of an empty trajectory that starts at the origin, with
    /// coordinates in multiples of `scale` millimeters.
    ///
    /// # Errors
    ///
    /// Returns an error if the scale is zero or larger than 127, or memory
    /// could not be allocated.
    pub fn new(scale: u8) -> Result<Self, TrajectoryBuilderError> {
        if scale == 0 || scale > 127 {
            return Err(TrajectoryBuilderError::InvalidScale);
        }
        let mut bytes = Vec::new();
        bytes.try_reserve(HEADER_LENGTH)?;
        bytes.push(scale);
        bytes.resize(HEADER_LENGTH, 0);
        Ok(Self {
            bytes,
            last_position: Vector3WithYaw::default(),
            scale: f32::from(scale),
        })
    }

    /// A builder that appends to an existing trajectory. Its bytes are copied
    /// if they are borrowed.
    ///
    /// `last_position` is the end position of the trajectory, if known; it is
    /// calculated otherwise.
    ///
    /// # Errors
    ///
    /// Returns an error if memory could not be allocated.
    pub fn from_trajectory(
        trajectory: Trajectory<'_>,
        last_position: Option<Vector3WithYaw>,
    ) -> Result<Self, TrajectoryBuilderError> {
        let last_position = last_position.unwrap_or_else(|| trajectory.end_position());
        let scale = trajectory.scale();
        Ok(Self {
            bytes: try_into_owned(trajectory.bytes)?,
            last_position,
            scale,
        })
    }

    /// The last position of the trajectory being built.
    #[must_use]
    pub const fn last_position(&self) -> Vector3WithYaw {
        self.last_position
    }

    /// Sets the start position of the trajectory. Trajectories have no yaw,
    /// so the yaw is ignored.
    ///
    /// # Errors
    ///
    /// Returns an error if a segment was already added, or a coordinate is
    /// out of range.
    pub fn set_start_position(
        &mut self,
        start: Vector3WithYaw,
    ) -> Result<(), TrajectoryBuilderError> {
        if self.bytes.len() != HEADER_LENGTH {
            return Err(TrajectoryBuilderError::StartPositionAlreadyUsed);
        }
        let coordinates = [
            scale_coordinate(start.x, self.scale)?,
            scale_coordinate(start.y, self.scale)?,
            scale_coordinate(start.z, self.scale)?,
            // The retired start yaw
            0,
        ];
        let header = self.bytes.get_mut(1..HEADER_LENGTH).into_iter().flatten();
        let encoded = coordinates.into_iter().flat_map(i16::to_le_bytes);
        for (slot, byte) in header.zip(encoded) {
            *slot = byte;
        }
        self.last_position = Vector3WithYaw { yaw: 0.0, ..start };
        Ok(())
    }

    /// Appends a cubic Bézier curve with the given inner control points to
    /// the given target.
    ///
    /// # Errors
    ///
    /// Returns an error if the duration is zero or longer than 60 seconds, a
    /// coordinate is out of range, or memory could not be allocated.
    pub fn append_cubic_bezier(
        &mut self,
        control1: Vector3WithYaw,
        control2: Vector3WithYaw,
        target: Vector3WithYaw,
        duration_msec: u32,
    ) -> Result<(), TrajectoryBuilderError> {
        let segment = EncodedSegment::cubic_bezier(
            self.last_position,
            [control1, control2, target],
            duration_msec,
            self.scale,
        )?;
        self.append(&segment, target)
    }

    /// Appends a straight line to the given target. Lines longer than 60
    /// seconds are split in halves until they are short enough.
    ///
    /// # Errors
    ///
    /// Returns an error if the duration is zero, a coordinate is out of
    /// range, or memory could not be allocated.
    pub fn append_line(
        &mut self,
        target: Vector3WithYaw,
        duration_msec: u32,
    ) -> Result<(), TrajectoryBuilderError> {
        self.all_or_nothing(|builder| builder.append_line_unchecked(target, duration_msec))
    }

    /// Holds the last position for the given duration, in segments of at
    /// most 60 seconds. Does nothing if the duration is zero.
    ///
    /// # Errors
    ///
    /// Returns an error if memory could not be allocated.
    pub fn hold_position_for(&mut self, duration_msec: u32) -> Result<(), TrajectoryBuilderError> {
        self.all_or_nothing(|builder| {
            let mut remaining = duration_msec;
            while remaining > 0 {
                let current = remaining.min(MAX_DURATION_MSEC);
                remaining -= current;
                builder.append_line_unchecked(builder.last_position, current)?;
            }
            Ok(())
        })
    }

    /// Moves to the given target in the given time: accelerating smoothly to
    /// a travel velocity, moving with that velocity, and decelerating
    /// smoothly to a stop. The acceleration is at most `max_acceleration` if
    /// the duration allows it.
    ///
    /// An acceleration that is not positive and finite means moving along a
    /// straight line with constant velocity.
    ///
    /// # Errors
    ///
    /// Returns an error if the duration is zero, a coordinate is out of
    /// range, or memory could not be allocated.
    pub fn move_to_in_time(
        &mut self,
        target: Vector3WithYaw,
        duration_msec: u32,
        max_acceleration: f32,
    ) -> Result<(), TrajectoryBuilderError> {
        self.all_or_nothing(|builder| {
            builder.move_to_in_time_unchecked(target, duration_msec, max_acceleration)
        })
    }

    /// The trajectory that was built. Consumes the builder; its bytes become
    /// the bytes of the trajectory without copying.
    ///
    /// # Errors
    ///
    /// Returns an error if the builder was created from an empty trajectory
    /// without a header.
    pub fn build(self) -> Result<Trajectory<'static>, TrajectoryError> {
        Trajectory::from_bytes(self.bytes)
    }

    /// Runs an operation that may append several segments, and restores the
    /// builder if it fails.
    fn all_or_nothing(
        &mut self,
        operation: impl FnOnce(&mut Self) -> Result<(), TrajectoryBuilderError>,
    ) -> Result<(), TrajectoryBuilderError> {
        let (length, last_position) = (self.bytes.len(), self.last_position);
        let result = operation(self);
        if result.is_err() {
            self.bytes.truncate(length);
            self.last_position = last_position;
        }
        result
    }

    /// Appends an encoded segment that ends at the given target.
    fn append(
        &mut self,
        segment: &EncodedSegment,
        target: Vector3WithYaw,
    ) -> Result<(), TrajectoryBuilderError> {
        self.bytes.try_reserve(segment.as_bytes().len())?;
        self.bytes.extend_from_slice(segment.as_bytes());
        // Trajectories have no yaw
        self.last_position = Vector3WithYaw { yaw: 0.0, ..target };
        Ok(())
    }

    /// Appends a line; the caller restores the builder if this fails.
    fn append_line_unchecked(
        &mut self,
        target: Vector3WithYaw,
        duration_msec: u32,
    ) -> Result<(), TrajectoryBuilderError> {
        if duration_msec > MAX_DURATION_MSEC {
            // Split the line in halves; both are shorter than the line and
            // not empty, so the recursion ends
            let last = self.last_position;
            // As in C, which overflows where f32::midpoint would not
            let midpoint = (last + target) / 2.0;
            let half = duration_msec / 2;
            self.append_line_unchecked(midpoint, half)?;
            return self.append_line_unchecked(target, duration_msec - half);
        }
        let segment = EncodedSegment::line(self.last_position, target, duration_msec, self.scale)?;
        self.append(&segment, target)
    }

    /// Moves to a target in a given time; the caller restores the builder if
    /// this fails.
    fn move_to_in_time_unchecked(
        &mut self,
        target: Vector3WithYaw,
        duration_msec: u32,
        max_acceleration: f32,
    ) -> Result<(), TrajectoryBuilderError> {
        if duration_msec == 0 {
            return Err(TrajectoryBuilderError::ZeroDuration);
        }
        if !max_acceleration.is_finite() || max_acceleration <= 0.0 || duration_msec < 3 {
            // Constant velocity along a straight line; also when the
            // duration is too short for three segments
            return self.append_line_unchecked(target, duration_msec);
        }

        // The travel velocity and the durations of the phases
        let start = self.last_position;
        let distance = hypotf(
            hypotf(target.x - start.x, target.y - start.y),
            target.z - start.z,
        );
        let duration_sec = msec_to_sec(duration_msec);
        let mut travel_speed =
            travel_velocity_for_distance(distance, duration_sec, max_acceleration);
        let mut cruise_sec = duration_sec - 2.0 * travel_speed / max_acceleration;
        if cruise_sec < 0.0 {
            // The acceleration is not enough; accelerate uniformly to the
            // travel velocity and decelerate right away
            cruise_sec = 0.0;
            travel_speed = 2.0 * distance / duration_sec;
        }
        let mut accel_sec = (duration_sec - cruise_sec) / 2.0;
        let mut accel_msec = float_to_msec(accel_sec * 1000.0);
        if accel_msec < 1 {
            accel_msec = 1;
            accel_sec = msec_to_sec(accel_msec);
        }

        let rest = Vector3WithYaw {
            yaw: (target.yaw - start.yaw) / duration_sec,
            ..Vector3WithYaw::default()
        };
        let travel_velocity =
            (travel_speed * (target.xyz() - start.xyz()) / distance).with_yaw(rest.yaw);
        // How far the drone gets while accelerating or decelerating; the yaw
        // changes uniformly, so its offset is twice as large
        let offset =
            (travel_velocity.xyz() * accel_sec / 2.0).with_yaw(travel_velocity.yaw * accel_sec);

        // Accelerate from rest to the travel velocity
        let cruise_start = start + offset;
        self.append_smooth_curve(
            (start, rest),
            (cruise_start, travel_velocity),
            accel_sec,
            accel_msec,
        )?;

        // Move with the travel velocity, if there is time left for it
        let cruise_end = target - offset;
        if let Some(cruise_msec) = duration_msec.checked_sub(accel_msec.saturating_mul(2))
            && cruise_msec > 0
        {
            self.append_line_unchecked(cruise_end, cruise_msec)?;
        }

        // Decelerate from the travel velocity to rest
        self.append_smooth_curve(
            (cruise_end, travel_velocity),
            (target, rest),
            accel_sec,
            accel_msec,
        )
    }

    /// Appends a cubic Bézier curve between two points with the given
    /// velocities.
    fn append_smooth_curve(
        &mut self,
        (start, start_velocity): (Vector3WithYaw, Vector3WithYaw),
        (end, end_velocity): (Vector3WithYaw, Vector3WithYaw),
        duration_sec: f32,
        duration_msec: u32,
    ) -> Result<(), TrajectoryBuilderError> {
        let (control1, control2) = cubic_bezier_from_velocity_constraints(
            start,
            start_velocity,
            end,
            end_velocity,
            duration_sec,
        )
        .ok_or(TrajectoryBuilderError::ZeroDuration)?;
        let segment = EncodedSegment::cubic_bezier(
            self.last_position,
            [control1, control2, end],
            duration_msec,
            self.scale,
        )?;
        self.append(&segment, end)
    }
}

/// The bytes of a `Cow`, copied into a new vector if they are borrowed.
pub(super) fn try_into_owned(bytes: Cow<'_, [u8]>) -> Result<Vec<u8>, TryReserveError> {
    match bytes {
        Cow::Owned(bytes) => Ok(bytes),
        Cow::Borrowed(bytes) => {
            let mut result = Vec::new();
            result.try_reserve_exact(bytes.len())?;
            result.extend_from_slice(bytes);
            Ok(result)
        }
    }
}

/// A coordinate in multiples of the scale, rounded down, as in the C library.
pub(super) fn scale_coordinate(coordinate: f32, scale: f32) -> Result<i16, TrajectoryBuilderError> {
    let scaled = floorf(coordinate / scale);
    // Written so that NaN is rejected as well
    if scaled >= f32::from(i16::MIN) && scaled <= f32::from(i16::MAX) {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "an integer in the range of i16"
        )]
        Ok(scaled as i16)
    } else {
        Err(TrajectoryBuilderError::CoordinateOutOfRange)
    }
}

/// Milliseconds from a non-negative float, truncated as in the C library.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "truncated as in the C library; out of range values saturate"
)]
pub(super) fn float_to_msec(msec: f32) -> u32 {
    msec as u32
}

/// The longest encoded segment: a header byte, the duration, and seven
/// control points along each of the three axes.
const MAX_SEGMENT_LENGTH: usize = 3 + 2 * 3 * (MAX_POLY_COEFFS - 1);

/// An encoded segment of a trajectory.
#[derive(Clone, Copy, Debug)]
pub(super) struct EncodedSegment {
    bytes: [u8; MAX_SEGMENT_LENGTH],
    len: usize,
}

impl EncodedSegment {
    /// A segment with the given header byte and duration, and no control
    /// points yet.
    pub(super) fn new(header: u8, duration_msec: u16) -> Self {
        let mut result = Self {
            bytes: [0; MAX_SEGMENT_LENGTH],
            len: 1,
        };
        result.bytes[0] = header;
        result.push(duration_msec.to_le_bytes());
        result
    }

    /// A straight line from `start` to `target`.
    #[expect(
        clippy::float_cmp,
        reason = "an axis is left out only if it stays exactly where it is"
    )]
    fn line(
        start: Vector3WithYaw,
        target: Vector3WithYaw,
        duration_msec: u32,
        scale: f32,
    ) -> Result<Self, TrajectoryBuilderError> {
        let mut result = Self::new(0, check_duration(duration_msec)?);
        let axes = [
            (start.x, target.x),
            (start.y, target.y),
            (start.z, target.z),
        ];
        for (axis, (start, target)) in axes.into_iter().enumerate() {
            if start != target {
                result.bytes[0] |= LINEAR << (2 * axis);
                result.push_coordinate(target, scale)?;
            }
        }
        Ok(result)
    }

    /// A cubic Bézier curve from `start` along the given control points.
    #[expect(
        clippy::float_cmp,
        reason = "an axis is left out only if it stays exactly where it is"
    )]
    pub(super) fn cubic_bezier(
        start: Vector3WithYaw,
        points: [Vector3WithYaw; 3],
        duration_msec: u32,
        scale: f32,
    ) -> Result<Self, TrajectoryBuilderError> {
        let mut result = Self::new(0, check_duration(duration_msec)?);
        let [control1, control2, target] = points;
        let axes = [
            [start.x, control1.x, control2.x, target.x],
            [start.y, control1.y, control2.y, target.y],
            [start.z, control1.z, control2.z, target.z],
        ];
        for (axis, [start, control1, control2, target]) in axes.into_iter().enumerate() {
            // Written so that a NaN coordinate is written, and rejected
            if start != control1 || control1 != control2 || control2 != target {
                result.bytes[0] |= BEZIER << (2 * axis);
                for coordinate in [control1, control2, target] {
                    result.push_coordinate(coordinate, scale)?;
                }
            }
        }
        Ok(result)
    }

    /// Appends a coordinate, in multiples of the scale.
    pub(super) fn push_coordinate(
        &mut self,
        coordinate: f32,
        scale: f32,
    ) -> Result<(), TrajectoryBuilderError> {
        self.push(scale_coordinate(coordinate, scale)?.to_le_bytes());
        Ok(())
    }

    /// Appends two bytes. The buffer is large enough for any segment.
    fn push(&mut self, bytes: [u8; 2]) {
        if let Some(slot) = self.bytes.get_mut(self.len..self.len + 2) {
            slot.copy_from_slice(&bytes);
            self.len += 2;
        }
    }

    pub(super) fn as_bytes(&self) -> &[u8] {
        self.bytes.get(..self.len).unwrap_or(&self.bytes)
    }
}

/// The duration of a new segment, if it is valid.
fn check_duration(duration_msec: u32) -> Result<u16, TrajectoryBuilderError> {
    if duration_msec == 0 {
        Err(TrajectoryBuilderError::ZeroDuration)
    } else if duration_msec > MAX_DURATION_MSEC {
        Err(TrajectoryBuilderError::DurationTooLong)
    } else {
        u16::try_from(duration_msec).map_err(|_| TrajectoryBuilderError::DurationTooLong)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(x: f32, y: f32, z: f32) -> Vector3WithYaw {
        Vector3WithYaw { x, y, z, yaw: 0.0 }
    }

    #[test]
    fn a_failed_start_position_changes_nothing() {
        let mut builder = TrajectoryBuilder::new(1).unwrap();
        builder.set_start_position(point(1.0, 2.0, 3.0)).unwrap();
        let bytes = builder.bytes.clone();
        // The X coordinate is valid, the Y coordinate is not
        assert_eq!(
            builder.set_start_position(point(5.0, f32::NAN, 0.0)),
            Err(TrajectoryBuilderError::CoordinateOutOfRange)
        );
        assert_eq!(builder.bytes, bytes);
        assert_eq!(builder.last_position(), point(1.0, 2.0, 3.0));
    }

    #[test]
    fn a_failed_split_line_changes_nothing() {
        let mut builder = TrajectoryBuilder::new(1).unwrap();
        builder.set_start_position(point(0.0, 0.0, 0.0)).unwrap();
        // The first half of the line is in range, the second half is not
        assert_eq!(
            builder.append_line(point(60_000.0, 0.0, 0.0), 120_000),
            Err(TrajectoryBuilderError::CoordinateOutOfRange)
        );
        assert_eq!(builder.bytes.len(), HEADER_LENGTH);
        assert_eq!(builder.last_position(), point(0.0, 0.0, 0.0));
    }

    #[test]
    fn coordinates_are_rounded_down() {
        assert_eq!(scale_coordinate(25.0, 10.0), Ok(2));
        assert_eq!(scale_coordinate(-25.0, 10.0), Ok(-3));
        assert_eq!(scale_coordinate(327_670.0, 10.0), Ok(i16::MAX));
        assert_eq!(
            scale_coordinate(327_680.0, 10.0),
            Err(TrajectoryBuilderError::CoordinateOutOfRange)
        );
        assert_eq!(
            scale_coordinate(f32::INFINITY, 1.0),
            Err(TrajectoryBuilderError::CoordinateOutOfRange)
        );
    }
}
