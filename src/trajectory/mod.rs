//! Trajectories of drones.
//!
//! A trajectory is kept in its encoded form, as in the body of a trajectory
//! block of a show file, and decoded one segment at a time by a
//! [`TrajectoryPlayer`]. A trajectory read from a show file in memory borrows
//! its bytes from the show file.

mod player;
mod segment;
mod stats;

use alloc::borrow::Cow;
use alloc::vec::Vec;
use core::{error, fmt};

pub use player::{TrajectoryPlayer, TrajectoryPlayerState};
pub use segment::TrajectorySegment;
pub use stats::{
    LandingStats, TakeoffStats, TrajectoryStatComponents, TrajectoryStats,
    TrajectoryStatsCalculator, TrajectoryStatsError,
};

use crate::basic_types::{BoundingBox, Interval, Vector3WithYaw};
use crate::formats::binary::{BinaryBlockType, BinaryFile, BinaryFileError};
use crate::parsing::read_i16;
use crate::poly::Poly;
use segment::{coords_length, msec_to_sec};

/// The length of the header of a trajectory block: a flags byte and four
/// 16-bit start coordinates, the last one retired (it held the start yaw of
/// the legacy embedded yaw track).
const HEADER_LENGTH: usize = 9;

/// Why a trajectory could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrajectoryError {
    /// The body of the trajectory block is malformed: it is shorter than the
    /// header, or a segment does not fit in it.
    Corrupted,
    /// The show file could not be read.
    File(BinaryFileError),
}

impl fmt::Display for TrajectoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Corrupted => f.write_str("the trajectory is malformed"),
            Self::File(err) => write!(f, "{err}"),
        }
    }
}

impl error::Error for TrajectoryError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Corrupted => None,
            Self::File(err) => Some(err),
        }
    }
}

impl From<BinaryFileError> for TrajectoryError {
    fn from(err: BinaryFileError) -> Self {
        Self::File(err)
    }
}

/// The trajectory of a drone.
///
/// The bytes of the trajectory are borrowed when it is read from a show file
/// in memory, and owned when they were built or modified.
///
/// Modifying a borrowed trajectory (e.g. cutting it or replacing its end with
/// a landing) copies all of its bytes, like the C library does. An alternative
/// that would save that memory: keep borrowing the unmodified segments, and
/// store the few modified or appended segments at the end inline, in a small
/// fixed buffer. A cut rewrites at most one segment and a landing appends one,
/// so this would need no allocation, at the cost of a more complex player.
#[derive(Clone, Debug)]
pub struct Trajectory<'a> {
    /// The header and the segments, as in the body of a trajectory block.
    bytes: Cow<'a, [u8]>,
    /// The start position of the trajectory.
    start: Vector3WithYaw,
    /// The scale of the coordinates: each unit is this many millimeters.
    scale: f32,
    /// The length of the header in `bytes`.
    header_length: usize,
}

impl Default for Trajectory<'static> {
    fn default() -> Self {
        Self::new()
    }
}

impl Trajectory<'static> {
    /// An empty trajectory at the origin.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            bytes: Cow::Owned(Vec::new()),
            start: Vector3WithYaw {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                yaw: 0.0,
            },
            scale: 1.0,
            header_length: 0,
        }
    }

    /// A trajectory that owns the given body of a trajectory block.
    ///
    /// # Errors
    ///
    /// Returns [`TrajectoryError::Corrupted`] if the body is malformed.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, TrajectoryError> {
        Self::from_cow(Cow::Owned(bytes))
    }
}

impl<'a> Trajectory<'a> {
    /// A trajectory that borrows the given body of a trajectory block.
    ///
    /// # Errors
    ///
    /// Returns [`TrajectoryError::Corrupted`] if the body is malformed.
    pub fn from_buffer(bytes: &'a [u8]) -> Result<Self, TrajectoryError> {
        Self::from_cow(Cow::Borrowed(bytes))
    }

    /// The trajectory of a show file, borrowing its bytes from the show file,
    /// or `None` if the show file has no trajectory.
    ///
    /// # Errors
    ///
    /// Returns an error if the trajectory block is truncated or malformed.
    pub fn from_binary_file(file: &BinaryFile<'a>) -> Result<Option<Self>, TrajectoryError> {
        file.find_first_block_by_type(BinaryBlockType::TRAJECTORY)?
            .map(|block| Self::from_buffer(block.body))
            .transpose()
    }

    fn from_cow(bytes: Cow<'a, [u8]>) -> Result<Self, TrajectoryError> {
        validate(&bytes)?;
        let scale = f32::from(scale_of(bytes.first().copied().unwrap_or_default()));
        // The fourth start coordinate belongs to the legacy yaw track and is
        // ignored
        let coordinate = |offset| f32::from(read_i16(&bytes, offset).unwrap_or_default()) * scale;
        let start = Vector3WithYaw {
            x: coordinate(1),
            y: coordinate(3),
            z: coordinate(5),
            yaw: 0.0,
        };
        Ok(Self {
            bytes,
            start,
            scale,
            header_length: HEADER_LENGTH,
        })
    }

    /// The trajectory with its bytes owned, copying them if they are
    /// borrowed.
    #[must_use]
    pub fn into_owned(self) -> Trajectory<'static> {
        Trajectory {
            bytes: Cow::Owned(self.bytes.into_owned()),
            start: self.start,
            scale: self.scale,
            header_length: self.header_length,
        }
    }

    /// The bytes of the trajectory, as in the body of a trajectory block.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Whether the trajectory is empty: it has no bytes, or its scale is zero.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bytes.first().is_none_or(|&flags| scale_of(flags) == 0)
    }

    /// The scale of the coordinates: each unit is this many millimeters.
    #[must_use]
    pub const fn scale(&self) -> f32 {
        self.scale
    }

    /// The start position of the trajectory, as in its header.
    pub(crate) const fn start(&self) -> Vector3WithYaw {
        self.start
    }

    pub(crate) const fn header_length(&self) -> usize {
        self.header_length
    }

    /// The position at time zero.
    #[must_use]
    pub fn start_position(&self) -> Vector3WithYaw {
        TrajectoryPlayer::new(self).position_at(0.0)
    }

    /// The position after the end of the trajectory.
    #[must_use]
    pub fn end_position(&self) -> Vector3WithYaw {
        TrajectoryPlayer::new(self).position_at(f32::INFINITY)
    }

    /// The total duration of the trajectory, in milliseconds.
    #[must_use]
    pub fn total_duration_msec(&self) -> u32 {
        TrajectoryPlayer::new(self).total_duration_msec()
    }

    /// The total duration of the trajectory, in seconds.
    #[must_use]
    pub fn total_duration_sec(&self) -> f32 {
        msec_to_sec(self.total_duration_msec())
    }

    /// The segment that contains the given time, in seconds, with its
    /// derivatives calculated, and the relative time within the segment.
    #[must_use]
    pub fn segment_at(&self, time_sec: f32) -> (TrajectoryPlayerState, f32) {
        let mut player = TrajectoryPlayer::new(self);
        let rel_time = player.seek_to_time(time_sec);
        let segment = player.current_segment();
        segment.dpoly();
        segment.ddpoly();
        (player.save_state(), rel_time)
    }

    /// The axis-aligned bounding box of the segments of the trajectory. The
    /// intervals are empty (from infinity to minus infinity) if the trajectory
    /// has no segments.
    #[must_use]
    pub fn axis_aligned_bounding_box(&self) -> BoundingBox {
        let empty = Interval {
            min: f32::INFINITY,
            max: f32::NEG_INFINITY,
        };
        let mut result = BoundingBox {
            x: empty,
            y: empty,
            z: empty,
        };
        let mut player = TrajectoryPlayer::new(self);
        while player.has_more_segments() {
            let poly = *player.current_segment().poly();
            for (interval, poly) in [
                (&mut result.x, &poly.x),
                (&mut result.y, &poly.y),
                (&mut result.z, &poly.z),
            ] {
                let extrema = extrema_of(poly);
                if extrema.min < interval.min {
                    interval.min = extrema.min;
                }
                if extrema.max > interval.max {
                    interval.max = extrema.max;
                }
            }
            player.build_next_segment();
        }
        result
    }
}

/// The minimum and maximum of a polynomial of a segment on `[0, 1]`. Above
/// degree 3, where the extrema are not implemented, the minimum and maximum
/// of its control points are used instead, which bound a Bézier curve.
fn extrema_of(poly: &Poly) -> Interval {
    poly.extrema()
        .unwrap_or_else(|_| control_point_bounds(poly))
}

/// The minimum and maximum of the Bézier control points of a polynomial on
/// `[0, 1]`.
fn control_point_bounds(poly: &Poly) -> Interval {
    let coeffs = poly.coeffs();
    let n = coeffs.len().saturating_sub(1);
    let mut result = Interval {
        min: f32::INFINITY,
        max: f32::NEG_INFINITY,
    };
    // The k-th control point is the sum of binomial(k, i) / binomial(n, i)
    // times the i-th coefficient, over i from 0 to k
    for k in 0..=n {
        let mut point = 0.0;
        for (i, &coeff) in coeffs.iter().enumerate().take(k + 1) {
            point += binomial(k, i) / binomial(n, i) * coeff;
        }
        if point < result.min {
            result.min = point;
        }
        if point > result.max {
            result.max = point;
        }
    }
    result
}

#[expect(
    clippy::cast_precision_loss,
    reason = "binomials of numbers up to the maximum degree are small"
)]
fn binomial(n: usize, k: usize) -> f32 {
    let mut result: u32 = 1;
    for i in 0..k.min(n) {
        let (Ok(numerator), Ok(denominator)) = (u32::try_from(n - i), u32::try_from(i + 1)) else {
            return f32::NAN;
        };
        result = result.saturating_mul(numerator) / denominator;
    }
    result as f32
}

/// The scale in the flags byte of a trajectory: its lowest 7 bits. The highest
/// bit marked the legacy embedded yaw track and is ignored.
const fn scale_of(flags: u8) -> u8 {
    flags & 0x7f
}

/// Checks that the body of a trajectory block is well-formed: it has a
/// complete header, and every segment fits in it. Segments are not checked if
/// the scale is zero, since the trajectory is empty then.
fn validate(bytes: &[u8]) -> Result<(), TrajectoryError> {
    if bytes.len() < HEADER_LENGTH {
        return Err(TrajectoryError::Corrupted);
    }
    if bytes.first().is_some_and(|&flags| scale_of(flags) == 0) {
        return Ok(());
    }
    let mut rest = bytes.get(HEADER_LENGTH..).unwrap_or_default();
    while let Some(&header) = rest.first() {
        if rest.len() < 3 {
            return Err(TrajectoryError::Corrupted);
        }
        rest = rest
            .get(3 + coords_length(header)..)
            .ok_or(TrajectoryError::Corrupted)?;
    }
    Ok(())
}
