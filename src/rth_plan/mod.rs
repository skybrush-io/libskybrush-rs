//! Collective return-to-home (RTH) plans: what each drone should do if the
//! show is interrupted at a given time instant.
//!
//! A plan is kept in its encoded form, as in the body of an RTH plan block,
//! and its entries are decoded when the plan is evaluated. A plan read from a
//! show file in memory borrows its bytes from the show file.
//!
//! An entry that cannot be decoded (a varint above 32 bits, an entry time
//! beyond 32 bits, a duration above 2^24 seconds, a missing point) does not
//! make the plan invalid: the plan can be evaluated before that entry, and
//! evaluations that reach it fail.

mod trajectory;

use crate::flags::impl_flags;
use alloc::borrow::Cow;
use alloc::vec::Vec;
use core::{error, fmt};

pub use trajectory::RthTrajectoryError;

use crate::basic_types::Vector2;
use crate::formats::binary::{BinaryBlockType, BinaryFile, BinaryFileError};
use crate::parsing::{VarintError, decode_varuint32, read_i16, read_u16};

/// The length of the header of an RTH plan block: flags, scale, acceleration,
/// landing velocity, default landing altitude and the number of points.
const HEADER_LENGTH: usize = 10;

/// The length of a point: two 16-bit coordinates.
const POINT_LENGTH: usize = 4;

/// The longest duration of an action, in seconds: the largest integer that a
/// float holds without loss of precision.
const MAX_DURATION_SEC: u32 = 16_777_216;

/// The parameters of an RTH plan entry that are set.
///
/// # Examples
///
/// ```
/// use skybrush::basic_types::Vector3;
/// use skybrush::rth_plan::{RthPlan, RthPlanEntryFlags};
/// use skybrush::trajectory::Trajectory;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// // The body of an RTH plan block: two landing points, and two entries
/// let body = [
///     0x00, 0x0a, 0xd0, 0x07, 0xe8, 0x03, 0x00, 0x00, 0x02, 0x00, 0x2c, 0x01, 0x90, 0x01,
///     0x70, 0xfe, 0xd4, 0xfe, 0x02, 0x00, 0x20, 0x00, 0x00, 0x22, 0x0f, 0x00, 0x32, 0x05,
/// ];
/// let plan = RthPlan::from_buffer(&body)?;
///
/// // If the show is interrupted at 10 s: wait until 15 s, then fly to
/// // (3 m, 4 m) in 50 s
/// let entry = plan.evaluate_at(10.0)?;
/// assert_eq!(entry.time_sec, 15.0);
/// assert!(entry.flags.contains(RthPlanEntryFlags::HAS_TARGET_XY));
/// assert_eq!((entry.landing_target.x, entry.landing_target.y), (3000.0, 4000.0));
///
/// // The trajectory of the action, from the position of the drone
/// let start = Vector3 { x: 0.0, y: 0.0, z: 5000.0 };
/// let trajectory = Trajectory::from_rth_plan_entry(&entry, start)?;
/// let end = trajectory.end_position();
/// assert!((end.x - 3000.0).abs() < 1.0 && (end.y - 4000.0).abs() < 1.0);
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct RthPlanEntryFlags(u8);

impl RthPlanEntryFlags {
    /// No parameters.
    pub const NONE: Self = Self(0);
    /// A vertical neck before the action.
    pub const HAS_NECK: Self = Self(1 << 0);
    /// A horizontal landing target.
    pub const HAS_TARGET_XY: Self = Self(1 << 1);
    /// An arrival altitude above the landing target.
    pub const HAS_TARGET_Z: Self = Self(1 << 2);
    /// A landing altitude.
    pub const HAS_LANDING_ALTITUDE: Self = Self(1 << 3);
    /// A delay before the action.
    pub const HAS_PRE_DELAY: Self = Self(1 << 4);
    /// A delay after the action.
    pub const HAS_POST_DELAY: Self = Self(1 << 5);
    /// All parameters.
    pub const ALL: Self = Self(0x3f);
}

impl_flags!(RthPlanEntryFlags);

/// What to do when the return-to-home action is triggered at a time instant.
/// Coordinates are in millimeters, times in seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RthPlanEntry {
    /// The time when the action starts.
    pub time_sec: f32,
    /// The duration of the main part of the action, without the delays, the
    /// neck and the landing.
    pub duration_sec: f32,
    /// The parameters that are set. The landing altitude, the delays and the
    /// neck always count as set, since they have defaults.
    pub flags: RthPlanEntryFlags,
    /// The horizontal landing target, if [`RthPlanEntryFlags::HAS_TARGET_XY`]
    /// is set.
    pub landing_target: Vector2,
    /// The arrival altitude above the target, if
    /// [`RthPlanEntryFlags::HAS_TARGET_Z`] is set.
    pub arrival_altitude: f32,
    /// The delay before the action.
    pub pre_delay_sec: f32,
    /// The delay after the action.
    pub post_delay_sec: f32,
    /// The height of the vertical neck before the action.
    pub pre_neck: f32,
    /// The duration of the neck.
    pub pre_neck_duration_sec: f32,
    /// The largest acceleration during the action, on a best-effort basis.
    /// Infinity, zero, a negative value or NaN mean no limit.
    pub max_acceleration: f32,
    /// The velocity of the descent from the arrival altitude to the landing
    /// altitude. Zero, a negative value, infinity or NaN mean no descent.
    pub landing_velocity: f32,
    /// The landing altitude.
    pub landing_altitude: f32,
}

impl RthPlanEntry {
    /// An entry at the given time that lands in place, with the defaults of
    /// the given plan, or without limits and at zero altitude if there is no
    /// plan.
    #[must_use]
    pub fn new(plan: Option<&RthPlan<'_>>, time_sec: f32) -> Self {
        Self {
            time_sec,
            duration_sec: 0.0,
            // These are valid even at their defaults
            flags: RthPlanEntryFlags::HAS_LANDING_ALTITUDE
                | RthPlanEntryFlags::HAS_PRE_DELAY
                | RthPlanEntryFlags::HAS_POST_DELAY
                | RthPlanEntryFlags::HAS_NECK,
            landing_target: Vector2 { x: 0.0, y: 0.0 },
            arrival_altitude: 0.0,
            pre_delay_sec: 0.0,
            post_delay_sec: 0.0,
            pre_neck: 0.0,
            pre_neck_duration_sec: 0.0,
            max_acceleration: plan.map_or(f32::INFINITY, RthPlan::default_acceleration_limit),
            landing_velocity: plan.map_or(f32::NAN, RthPlan::default_landing_velocity),
            landing_altitude: plan.map_or(0.0, RthPlan::default_landing_altitude),
        }
    }
}

/// Why an RTH plan could not be read or evaluated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RthPlanError {
    /// The body of the RTH plan block is malformed: its header, its points
    /// or its entries do not fit in it.
    Corrupted,
    /// The show file could not be read.
    File(BinaryFileError),
    /// An entry that was needed holds a varint that does not fit into 32
    /// bits.
    VarintOverflow,
    /// The time of an entry that was needed does not fit into 32 bits.
    TimeOverflow,
    /// An entry that was needed has a duration above 2^24 seconds.
    DurationTooLong,
    /// An entry that was needed refers to a point that does not exist.
    MissingPoint,
}

impl fmt::Display for RthPlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Corrupted => f.write_str("the RTH plan is malformed"),
            Self::File(_) => f.write_str("the show file could not be read"),
            Self::VarintOverflow => f.write_str("an entry holds a varint above 32 bits"),
            Self::TimeOverflow => f.write_str("the time of an entry does not fit into 32 bits"),
            Self::DurationTooLong => f.write_str("an entry has a duration above 2^24 seconds"),
            Self::MissingPoint => f.write_str("an entry refers to a point that does not exist"),
        }
    }
}

impl error::Error for RthPlanError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::File(err) => Some(err),
            _ => None,
        }
    }
}

impl From<BinaryFileError> for RthPlanError {
    fn from(err: BinaryFileError) -> Self {
        Self::File(err)
    }
}

/// A collective return-to-home plan: a list of points, and a list of entries
/// that tell what to do from each time instant on.
#[derive(Clone, Debug)]
pub struct RthPlan<'a> {
    /// The header, the points and the entries, as in the body of an RTH plan
    /// block.
    bytes: Cow<'a, [u8]>,
    /// The scale of the coordinates: each unit is this many millimeters.
    scale: f32,
    /// The length of the header in `bytes`.
    header_length: usize,
    num_points: usize,
    max_acceleration: f32,
    landing_velocity: f32,
    landing_altitude: f32,
}

impl Default for RthPlan<'static> {
    fn default() -> Self {
        Self::new()
    }
}

impl RthPlan<'static> {
    /// An empty plan, which lands in place at any time.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            bytes: Cow::Borrowed(&[]),
            scale: 1.0,
            header_length: 0,
            num_points: 0,
            max_acceleration: f32::INFINITY,
            landing_velocity: f32::NAN,
            landing_altitude: 0.0,
        }
    }

    /// A plan that owns the given body of an RTH plan block.
    ///
    /// # Errors
    ///
    /// Returns [`RthPlanError::Corrupted`] if the body is malformed.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, RthPlanError> {
        Self::from_cow(Cow::Owned(bytes))
    }
}

impl<'a> RthPlan<'a> {
    /// A plan that borrows the given body of an RTH plan block.
    ///
    /// # Errors
    ///
    /// Returns [`RthPlanError::Corrupted`] if the body is malformed.
    pub fn from_buffer(bytes: &'a [u8]) -> Result<Self, RthPlanError> {
        Self::from_cow(Cow::Borrowed(bytes))
    }

    /// The RTH plan of a show file, borrowing its bytes from the show file,
    /// or `None` if the show file has no RTH plan.
    ///
    /// # Errors
    ///
    /// Returns an error if the RTH plan block is truncated or malformed.
    pub fn from_binary_file(file: &BinaryFile<'a>) -> Result<Option<Self>, RthPlanError> {
        file.find_first_block_by_type(BinaryBlockType::RTH_PLAN)?
            .map(|block| Self::from_buffer(block.body))
            .transpose()
    }

    fn from_cow(bytes: Cow<'a, [u8]>) -> Result<Self, RthPlanError> {
        validate(&bytes)?;
        // The first byte holds flags without a meaning yet; the highest bit
        // of the scale is reserved
        let scale = f32::from(bytes.get(1).map_or(0, |&scale| scale & 0x7f));
        let u16_at = |offset| read_u16(&bytes, offset).unwrap_or_default();
        let max_acceleration = match u16_at(2) {
            // Zero means no limit
            0 => f32::INFINITY,
            value => f32::from(value),
        };
        let landing_velocity = f32::from(u16_at(4));
        let landing_altitude = f32::from(read_i16(&bytes, 6).unwrap_or_default()) * scale;
        let num_points = usize::from(u16_at(8));
        Ok(Self {
            bytes,
            scale,
            header_length: HEADER_LENGTH,
            num_points,
            max_acceleration,
            landing_velocity,
            landing_altitude,
        })
    }

    /// The bytes of the plan, as in the body of an RTH plan block.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The largest acceleration during the actions, unless an entry says
    /// otherwise; infinity if there is no limit.
    #[must_use]
    pub fn default_acceleration_limit(&self) -> f32 {
        if self.max_acceleration > 0.0 && self.max_acceleration.is_finite() {
            self.max_acceleration
        } else {
            f32::INFINITY
        }
    }

    /// The landing altitude of the actions, unless an entry says otherwise.
    #[must_use]
    pub const fn default_landing_altitude(&self) -> f32 {
        self.landing_altitude
    }

    /// The velocity of the descent to the landing altitude, or NaN if the
    /// actions do not descend.
    #[must_use]
    pub fn default_landing_velocity(&self) -> f32 {
        if self.has_default_landing_velocity() {
            self.landing_velocity
        } else {
            f32::NAN
        }
    }

    /// Whether the actions descend to the landing altitude.
    #[must_use]
    pub fn has_default_landing_velocity(&self) -> bool {
        self.landing_velocity.is_finite() && self.landing_velocity > 0.0
    }

    /// Sets the largest acceleration during the actions; zero, negative
    /// values, infinity and NaN mean no limit.
    pub fn set_default_acceleration_limit(&mut self, max_acceleration: f32) {
        self.max_acceleration = if max_acceleration > 0.0 && max_acceleration.is_finite() {
            max_acceleration
        } else {
            f32::INFINITY
        };
    }

    /// Sets the landing altitude of the actions; infinity and NaN become zero.
    pub fn set_default_landing_altitude(&mut self, landing_altitude: f32) {
        self.landing_altitude = if landing_altitude.is_finite() {
            landing_altitude
        } else {
            0.0
        };
    }

    /// Sets the velocity of the descent to the landing altitude; zero,
    /// negative values, infinity and NaN mean no descent.
    pub fn set_default_landing_velocity(&mut self, landing_velocity: f32) {
        self.landing_velocity = if landing_velocity.is_finite() && landing_velocity > 0.0 {
            landing_velocity
        } else {
            f32::NAN
        };
    }

    /// Makes the actions not descend to the landing altitude.
    pub fn clear_default_landing_velocity(&mut self) {
        self.set_default_landing_velocity(f32::NAN);
    }

    /// The number of points.
    #[must_use]
    pub const fn num_points(&self) -> usize {
        self.num_points
    }

    /// The point at the given index, in millimeters, or `None` if there is
    /// no such point.
    #[must_use]
    pub fn point(&self, index: usize) -> Option<Vector2> {
        if index >= self.num_points {
            return None;
        }
        let offset = self.offset_of_point(index)?;
        Some(Vector2 {
            x: self.coordinate_at(offset)?,
            y: self.coordinate_at(offset + 2)?,
        })
    }

    /// The number of entries.
    #[must_use]
    pub fn num_entries(&self) -> usize {
        self.offset_of_point(self.num_points)
            .and_then(|offset| read_u16(&self.bytes, offset))
            .map_or(0, usize::from)
    }

    /// Whether the plan has no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.num_entries() == 0
    }

    /// What to do if the return-to-home action is triggered at the given
    /// time, in seconds: the first entry that is not earlier than the given
    /// time. Before the start of the show, and after the last entry, the
    /// drone lands in place.
    ///
    /// # Errors
    ///
    /// Returns an error if an entry up to the one that is found cannot be
    /// decoded.
    #[expect(
        clippy::cast_precision_loss,
        reason = "entry times are compared and returned as floats, as in the C library"
    )]
    pub fn evaluate_at(&self, time_sec: f32) -> Result<RthPlanEntry, RthPlanError> {
        let default = RthPlanEntry::new(Some(self), time_sec);
        if time_sec < 0.0 {
            return Ok(default);
        }

        let Some(first_entry) = self
            .offset_of_point(self.num_points)
            .map(|offset| offset + 2)
        else {
            return Ok(default);
        };
        let mut reader = Reader {
            plan: self,
            offset: first_entry,
        };
        let mut entry = default;
        let mut entry_time_sec: u32 = 0;
        for _ in 0..self.num_entries() {
            let flags = reader.byte()?;
            entry_time_sec = entry_time_sec
                .checked_add(reader.varuint()?)
                .ok_or(RthPlanError::TimeOverflow)?;
            entry.time_sec = entry_time_sec as f32;

            // An entry without flags is the same as the previous one
            if flags != 0 {
                entry = reader.entry(flags, entry_time_sec as f32)?;
            }

            if entry.time_sec >= time_sec {
                return Ok(entry);
            }
        }

        // After the last entry, land in place
        Ok(default)
    }

    fn offset_of_point(&self, index: usize) -> Option<usize> {
        index
            .checked_mul(POINT_LENGTH)?
            .checked_add(self.header_length)
    }

    fn coordinate_at(&self, offset: usize) -> Option<f32> {
        read_i16(&self.bytes, offset).map(|value| f32::from(value) * self.scale)
    }
}

/// Decodes the entries of a plan.
struct Reader<'p, 'a> {
    plan: &'p RthPlan<'a>,
    offset: usize,
}

impl Reader<'_, '_> {
    /// Decodes the parameters of an entry with the given flags, starting
    /// from the defaults of the plan.
    fn entry(&mut self, flags: u8, time_sec: f32) -> Result<RthPlanEntry, RthPlanError> {
        let mut entry = RthPlanEntry::new(Some(self.plan), time_sec);
        let flags = RthPlanEntryFlags::from_bits_retain(flags);
        if flags.contains(RthPlanEntryFlags::HAS_NECK) {
            entry.pre_neck = self.coordinate()?;
            entry.pre_neck_duration_sec = self.duration()?;
        }
        if flags.contains(RthPlanEntryFlags::HAS_TARGET_XY) {
            let index = usize::try_from(self.varuint()?).map_err(|_| RthPlanError::MissingPoint)?;
            entry.landing_target = self.plan.point(index).ok_or(RthPlanError::MissingPoint)?;
        }
        if flags.contains(RthPlanEntryFlags::HAS_TARGET_Z) {
            entry.arrival_altitude = self.coordinate()?;
        }
        if flags.intersects(RthPlanEntryFlags::HAS_TARGET_XY | RthPlanEntryFlags::HAS_TARGET_Z) {
            entry.duration_sec = self.duration()?;
        }
        if flags.contains(RthPlanEntryFlags::HAS_LANDING_ALTITUDE) {
            entry.landing_altitude = self.coordinate()?;
        }
        if flags.contains(RthPlanEntryFlags::HAS_PRE_DELAY) {
            entry.pre_delay_sec = self.duration()?;
        }
        if flags.contains(RthPlanEntryFlags::HAS_POST_DELAY) {
            entry.post_delay_sec = self.duration()?;
        }
        entry.flags |= flags;
        Ok(entry)
    }

    fn byte(&mut self) -> Result<u8, RthPlanError> {
        let byte = *self
            .plan
            .bytes
            .get(self.offset)
            .ok_or(RthPlanError::Corrupted)?;
        self.offset += 1;
        Ok(byte)
    }

    fn coordinate(&mut self) -> Result<f32, RthPlanError> {
        let value = self
            .plan
            .coordinate_at(self.offset)
            .ok_or(RthPlanError::Corrupted)?;
        self.offset += 2;
        Ok(value)
    }

    fn varuint(&mut self) -> Result<u32, RthPlanError> {
        let rest = self.plan.bytes.get(self.offset..).unwrap_or_default();
        let (value, length) = decode_varuint32(rest).map_err(|err| match err {
            VarintError::Truncated => RthPlanError::Corrupted,
            VarintError::Overflow { .. } => RthPlanError::VarintOverflow,
        })?;
        self.offset += length;
        Ok(value)
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "durations up to 2^24 seconds are exact floats"
    )]
    fn duration(&mut self) -> Result<f32, RthPlanError> {
        let value = self.varuint()?;
        if value > MAX_DURATION_SEC {
            return Err(RthPlanError::DurationTooLong);
        }
        Ok(value as f32)
    }
}

/// Checks that the body of an RTH plan block is well-formed: its header, its
/// points and its entries fit in it. Varints above 32 bits are not rejected
/// here; they are reported when the entry is evaluated.
fn validate(bytes: &[u8]) -> Result<(), RthPlanError> {
    let corrupted = RthPlanError::Corrupted;
    if bytes.len() < HEADER_LENGTH {
        return Err(corrupted);
    }
    let num_points = usize::from(read_u16(bytes, HEADER_LENGTH - 2).unwrap_or_default());
    let mut rest = bytes
        .get(HEADER_LENGTH + num_points * POINT_LENGTH..)
        .ok_or(corrupted)?;
    if rest.is_empty() {
        // No entry table; the plan has no entries
        return Ok(());
    }

    let (&count, after) = rest.split_first_chunk::<2>().ok_or(corrupted)?;
    rest = after;
    let skip_i16 = |rest: &mut &[u8]| -> Result<(), RthPlanError> {
        *rest = rest.get(2..).ok_or(corrupted)?;
        Ok(())
    };
    let skip_varuint = |rest: &mut &[u8]| -> Result<(), RthPlanError> {
        let length = match decode_varuint32(rest) {
            Ok((_, length)) | Err(VarintError::Overflow { len: length }) => length,
            Err(VarintError::Truncated) => return Err(corrupted),
        };
        *rest = rest.get(length..).ok_or(corrupted)?;
        Ok(())
    };
    for _ in 0..u16::from_le_bytes(count) {
        let (&flags, after) = rest.split_first().ok_or(corrupted)?;
        rest = after;
        let flags = RthPlanEntryFlags::from_bits_retain(flags);
        skip_varuint(&mut rest)?;
        if flags.contains(RthPlanEntryFlags::HAS_NECK) {
            skip_i16(&mut rest)?;
            skip_varuint(&mut rest)?;
        }
        if flags.contains(RthPlanEntryFlags::HAS_TARGET_XY) {
            skip_varuint(&mut rest)?;
        }
        if flags.contains(RthPlanEntryFlags::HAS_TARGET_Z) {
            skip_i16(&mut rest)?;
        }
        if flags.intersects(RthPlanEntryFlags::HAS_TARGET_XY | RthPlanEntryFlags::HAS_TARGET_Z) {
            skip_varuint(&mut rest)?;
        }
        if flags.contains(RthPlanEntryFlags::HAS_LANDING_ALTITUDE) {
            skip_i16(&mut rest)?;
        }
        if flags.contains(RthPlanEntryFlags::HAS_PRE_DELAY) {
            skip_varuint(&mut rest)?;
        }
        if flags.contains(RthPlanEntryFlags::HAS_POST_DELAY) {
            skip_varuint(&mut rest)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plan whose first entry lands with a post-delay at 10 s, and whose
    /// second entry, with the given bytes, cannot be decoded.
    fn plan_with_undecodable_second_entry(second_entry: &[u8]) -> Vec<u8> {
        let mut body = Vec::new();
        // Header: scale 10, acceleration 2000, landing velocity 1000, no
        // landing altitude, two points
        body.extend_from_slice(&[0x00, 0x0a, 0xd0, 0x07, 0xe8, 0x03, 0x00, 0x00, 0x02, 0x00]);
        body.extend_from_slice(&[0x2c, 0x01, 0x90, 0x01, 0x70, 0xfe, 0xd4, 0xfe]);
        // Two entries, the first of which is at 10 s
        body.extend_from_slice(&[0x02, 0x00, 0x20, 0x0a, 0x00]);
        body.extend_from_slice(second_entry);
        body
    }

    fn evaluate(second_entry: &[u8], time_sec: f32) -> Result<f32, RthPlanError> {
        let plan = RthPlan::from_bytes(plan_with_undecodable_second_entry(second_entry))
            .map_err(|_| RthPlanError::Corrupted)?;
        plan.evaluate_at(time_sec).map(|entry| entry.time_sec)
    }

    #[test]
    fn entries_before_an_undecodable_entry_can_be_evaluated() {
        for (second_entry, error) in [
            (
                &[0x00, 0x80, 0x80, 0x80, 0x80, 0x10][..],
                RthPlanError::VarintOverflow,
            ),
            (
                &[0x00, 0xff, 0xff, 0xff, 0xff, 0x0f][..],
                RthPlanError::TimeOverflow,
            ),
            (
                &[0x20, 0x0a, 0x81, 0x80, 0x80, 0x08][..],
                RthPlanError::DurationTooLong,
            ),
            (&[0x02, 0x0a, 0x02, 0x05][..], RthPlanError::MissingPoint),
        ] {
            assert_eq!(evaluate(second_entry, 5.0), Ok(10.0));
            assert_eq!(evaluate(second_entry, 10.0), Ok(10.0));
            assert_eq!(evaluate(second_entry, 15.0), Err(error));
            // Before the show, the plan lands in place without decoding
            assert_eq!(evaluate(second_entry, -1.0), Ok(-1.0));
        }
    }

    #[test]
    fn truncated_entries_make_the_plan_invalid() {
        let body = plan_with_undecodable_second_entry(&[0x02, 0x0a]);
        assert!(matches!(
            RthPlan::from_bytes(body),
            Err(RthPlanError::Corrupted)
        ));
    }
}
