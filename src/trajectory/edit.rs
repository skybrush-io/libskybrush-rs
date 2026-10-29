//! Modifying trajectories: cutting, clearing, and replacing the end with a
//! landing.
//!
//! A borrowed trajectory is never written through to the show file. Cutting
//! at the boundary of a segment narrows the borrowed bytes; other
//! modifications copy the bytes that are kept.

use alloc::borrow::Cow;
use alloc::collections::TryReserveError;
use alloc::vec::Vec;
use core::{error, fmt, mem};

use libm::{hypotf, roundf};

use super::builder::{EncodedSegment, float_to_msec};
use super::{LandingStats, Trajectory, TrajectoryBuilderError, TrajectoryPlayer, TrajectoryStats};
use crate::basic_types::{Vector3, Vector3WithYaw};
use crate::motion::cubic_bezier_from_velocity_constraints;
use crate::poly::bezier_cut_at;

/// The longest landing segment, in seconds.
const MAX_LANDING_DURATION_SEC: f32 = 60.0;

/// Why the end of a trajectory could not be replaced with a landing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LandingError {
    /// The statistics do not contain the landing time.
    MissingLandingTime,
    /// The landing velocity is not positive, or the terminal velocity is
    /// negative.
    InvalidVelocity,
    /// The landing position is not below the position at the landing time,
    /// so the landing segment would be shorter than a millisecond.
    NotBelow,
    /// The landing segment could not be written.
    Builder(TrajectoryBuilderError),
}

impl fmt::Display for LandingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingLandingTime => f.write_str("the statistics have no landing time"),
            Self::InvalidVelocity => f.write_str("the landing or terminal velocity is invalid"),
            Self::NotBelow => {
                f.write_str("the landing position is not below the position at the landing time")
            }
            Self::Builder(_) => f.write_str("the landing segment could not be written"),
        }
    }
}

impl error::Error for LandingError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Builder(err) => Some(err),
            _ => None,
        }
    }
}

impl From<TrajectoryBuilderError> for LandingError {
    fn from(err: TrajectoryBuilderError) -> Self {
        Self::Builder(err)
    }
}

impl From<TryReserveError> for LandingError {
    fn from(err: TryReserveError) -> Self {
        Self::Builder(err.into())
    }
}

/// How to cut a trajectory: the number of bytes to keep, and the last kept
/// segment rewritten to end earlier, with its offset.
struct Cut {
    keep: usize,
    rewrite: Option<(usize, EncodedSegment)>,
}

impl Trajectory<'_> {
    /// Removes all segments of the trajectory, making it empty. A borrowed
    /// trajectory stops borrowing the show file; the show file is left
    /// intact.
    pub fn clear(&mut self) {
        *self = Trajectory::new();
    }

    /// Cuts the trajectory at the given time, in seconds. The time is rounded
    /// to milliseconds. Cutting after the end does nothing.
    ///
    /// Cutting at the boundary of a segment only drops the segments after it,
    /// so a borrowed trajectory keeps borrowing. Cutting within a segment
    /// rewrites that segment, so the bytes of a borrowed trajectory up to the
    /// end of that segment are copied.
    ///
    /// # Errors
    ///
    /// Returns an error if memory could not be allocated, or a control point
    /// of the rewritten segment is out of range. The trajectory is left
    /// intact then.
    pub fn cut_at(&mut self, time_sec: f32) -> Result<(), TrajectoryBuilderError> {
        let cut = self.plan_cut(time_sec)?;
        self.rewrite(&cut, &[])?;
        Ok(())
    }

    /// Replaces the end of the trajectory with a smooth landing at the given
    /// position, arriving there at rest, and returns the updated statistics.
    /// See
    /// [`replace_end_to_land_at_with_terminal_velocity`](Self::replace_end_to_land_at_with_terminal_velocity).
    ///
    /// # Errors
    ///
    /// As for `replace_end_to_land_at_with_terminal_velocity`.
    pub fn replace_end_to_land_at(
        &mut self,
        stats: &TrajectoryStats,
        new_landing_position: Vector3,
        new_landing_velocity: f32,
    ) -> Result<TrajectoryStats, LandingError> {
        self.replace_end_to_land_at_with_terminal_velocity(
            stats,
            new_landing_position,
            new_landing_velocity,
            0.0,
        )
    }

    /// Replaces the end of the trajectory after the landing time in the
    /// statistics with a cubic Bézier curve to the given position, and
    /// returns the statistics of the new trajectory: the given statistics,
    /// with the components that were calculated updated.
    ///
    /// The duration of the landing segment is the vertical distance divided
    /// by `new_landing_velocity`, at most 60 seconds. The drone arrives at
    /// the landing position with `terminal_velocity`, downwards.
    ///
    /// The bytes of a borrowed trajectory up to the landing time are copied.
    ///
    /// # Errors
    ///
    /// Returns an error if the statistics have no landing time, a velocity is
    /// invalid, the landing position is not below the position at the landing
    /// time, a coordinate is out of range, or memory could not be allocated.
    /// The trajectory is left intact then.
    pub fn replace_end_to_land_at_with_terminal_velocity(
        &mut self,
        stats: &TrajectoryStats,
        new_landing_position: Vector3,
        new_landing_velocity: f32,
        terminal_velocity: f32,
    ) -> Result<TrajectoryStats, LandingError> {
        let landing = stats.landing.ok_or(LandingError::MissingLandingTime)?;
        if terminal_velocity < 0.0 || new_landing_velocity <= 0.0 {
            return Err(LandingError::InvalidVelocity);
        }

        let vertical_distance = landing.pos_at_landing_time.z - new_landing_position.z;
        let mut duration_sec = if vertical_distance < 0.0 {
            0.0
        } else {
            vertical_distance / new_landing_velocity
        };
        // A single segment cannot be longer
        if duration_sec > MAX_LANDING_DURATION_SEC {
            duration_sec = MAX_LANDING_DURATION_SEC;
        }
        let duration_msec = float_to_msec(duration_sec * 1000.0);
        if duration_msec == 0 {
            return Err(LandingError::NotBelow);
        }

        // The landing segment, with the yaw of the landing time
        let new_end = new_landing_position.with_yaw(landing.pos_at_landing_time.yaw);
        let end_velocity = Vector3WithYaw {
            z: -terminal_velocity,
            ..Vector3WithYaw::default()
        };
        let (control1, control2) = cubic_bezier_from_velocity_constraints(
            landing.pos_at_landing_time,
            landing.vel_at_landing_time,
            new_end,
            end_velocity,
            duration_sec,
        )
        .ok_or(LandingError::NotBelow)?;
        let segment = EncodedSegment::cubic_bezier(
            landing.pos_at_landing_time,
            [control1, control2, new_end],
            duration_msec,
            self.scale(),
        )?;

        // Everything that can fail is checked; cut the trajectory at the
        // landing time and append the landing segment
        let cut = self.plan_cut(landing.landing_time_sec)?;
        self.rewrite(&cut, segment.as_bytes())?;

        let mut stats = *stats;
        if let Some(msec) = &mut stats.duration_msec {
            *msec = float_to_msec(landing.landing_time_sec * 1000.0).saturating_add(duration_msec);
        }
        stats.landing = Some(LandingStats {
            landing_time_sec: landing.landing_time_sec + duration_sec,
            pos_at_landing_time: new_end,
            vel_at_landing_time: end_velocity,
        });
        if let Some(distance) = &mut stats.start_to_end_distance_xy {
            let start = self.start();
            *distance = hypotf(
                new_landing_position.x - start.x,
                new_landing_position.y - start.y,
            );
        }
        Ok(stats)
    }

    /// Decides how to cut the trajectory at the given time, in seconds.
    fn plan_cut(&self, time_sec: f32) -> Result<Cut, TrajectoryBuilderError> {
        let mut player = TrajectoryPlayer::new(self);
        player.seek_to_time(time_sec);
        let state = player.save_state();
        let segment = state.segment();

        // The cut is decided in integer milliseconds: the start and end times
        // of segments are exact in milliseconds, while their counterparts in
        // seconds are rounded, so a cut exactly at the end of a segment could
        // end up a fraction of a millisecond before it
        let cut_msec = if time_sec * 1000.0 >= 4_294_967_296.0 {
            u32::MAX
        } else if time_sec > 0.0 {
            float_to_msec(roundf(time_sec * 1000.0))
        } else {
            0
        };

        if state.length() == 0 || cut_msec <= segment.start_time_msec() {
            // At the start of a segment, or after the end of the trajectory
            return Ok(Cut {
                keep: state.start(),
                rewrite: None,
            });
        }
        let keep = state.start() + state.length();
        if cut_msec >= segment.end_time_msec() {
            return Ok(Cut {
                keep,
                rewrite: None,
            });
        }

        // Within the segment: shorten it, and cut its Bézier curves at the
        // same relative time. The legacy yaw coordinates are kept as they are.
        let elapsed_msec = cut_msec - segment.start_time_msec();
        let rel_time = msec_ratio(elapsed_msec, segment.duration_msec());
        let header = self.bytes().get(state.start()).copied().unwrap_or_default();
        let duration =
            u16::try_from(elapsed_msec).map_err(|_| TrajectoryBuilderError::DurationTooLong)?;
        let mut rewritten = EncodedSegment::new(header, duration);
        for axis in 0..3 {
            let points = segment.control_points(axis);
            let cut = bezier_cut_at(points, rel_time);
            for &point in cut.get(1..points.len()).unwrap_or_default() {
                rewritten.push_coordinate(point, self.scale())?;
            }
        }
        Ok(Cut {
            keep,
            rewrite: Some((state.start(), rewritten)),
        })
    }

    /// Keeps the first bytes of the trajectory, rewrites a segment in them,
    /// and appends bytes. Narrows the borrowed bytes if nothing is rewritten
    /// or appended; copies them otherwise. Nothing is modified if memory
    /// could not be allocated.
    fn rewrite(&mut self, cut: &Cut, append: &[u8]) -> Result<(), TryReserveError> {
        let keep = cut.keep.min(self.bytes.len());
        if cut.rewrite.is_none() && append.is_empty() {
            match &mut self.bytes {
                Cow::Borrowed(bytes) => *bytes = bytes.get(..keep).unwrap_or(bytes),
                Cow::Owned(bytes) => bytes.truncate(keep),
            }
            return Ok(());
        }

        let length = keep + append.len();
        let mut bytes = match &mut self.bytes {
            Cow::Owned(bytes) => {
                bytes.try_reserve(length.saturating_sub(bytes.len()))?;
                mem::take(bytes)
            }
            Cow::Borrowed(bytes) => {
                let mut copy = Vec::new();
                copy.try_reserve_exact(length)?;
                copy.extend_from_slice(bytes.get(..keep).unwrap_or(bytes));
                copy
            }
        };
        bytes.truncate(keep);
        if let Some((offset, segment)) = &cut.rewrite {
            let segment = segment.as_bytes();
            if let Some(slot) = bytes.get_mut(*offset..offset + segment.len()) {
                slot.copy_from_slice(segment);
            }
        }
        bytes.extend_from_slice(append);
        self.bytes = Cow::Owned(bytes);
        Ok(())
    }
}

/// The ratio of two durations in milliseconds, as in the C library.
#[expect(
    clippy::cast_precision_loss,
    reason = "the conversion of the C library"
)]
fn msec_ratio(numerator: u32, denominator: u32) -> f32 {
    numerator as f32 / denominator as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trajectory::{HEADER_LENGTH, TrajectoryBuilder, TrajectoryStatsCalculator};

    fn point(x: f32, y: f32, z: f32) -> Vector3WithYaw {
        Vector3WithYaw { x, y, z, yaw: 0.0 }
    }

    /// Up 5 m in 5 s, then sideways 10 m in 10 s, in millimeters.
    fn up_and_sideways() -> Trajectory<'static> {
        let mut builder = TrajectoryBuilder::new(1).unwrap();
        builder.set_start_position(point(0.0, 0.0, 0.0)).unwrap();
        builder.append_line(point(0.0, 0.0, 5000.0), 5000).unwrap();
        builder
            .append_line(point(10_000.0, 0.0, 5000.0), 10_000)
            .unwrap();
        builder.build().unwrap()
    }

    #[test]
    fn cutting_within_a_segment_shortens_it() {
        let mut trajectory = up_and_sideways();
        trajectory.cut_at(10.0).unwrap();
        assert_eq!(trajectory.total_duration_msec(), 10_000);
        assert_eq!(trajectory.end_position(), point(5000.0, 0.0, 5000.0));
    }

    #[test]
    fn cutting_at_the_start_drops_every_segment() {
        let mut trajectory = up_and_sideways();
        trajectory.cut_at(0.0).unwrap();
        assert_eq!(trajectory.bytes().len(), HEADER_LENGTH);
    }

    #[test]
    fn cutting_a_borrowed_trajectory_within_a_segment_copies_it() {
        let owned = up_and_sideways();
        let mut borrowed = Trajectory::from_buffer(owned.bytes()).unwrap();
        borrowed.cut_at(10.0).unwrap();
        assert!(matches!(borrowed.bytes, Cow::Owned(_)));
        assert_eq!(borrowed.end_position(), point(5000.0, 0.0, 5000.0));
    }

    #[test]
    fn a_failed_landing_changes_nothing() {
        let mut trajectory = up_and_sideways();
        let stats = TrajectoryStatsCalculator::new(1000.0)
            .run(&trajectory)
            .unwrap();
        let bytes = trajectory.bytes().to_vec();
        // The landing position is below the trajectory but out of the range
        // of the scale
        let target = Vector3 {
            x: 40_000.0,
            y: 0.0,
            z: 0.0,
        };
        assert_eq!(
            trajectory.replace_end_to_land_at(&stats, target, 500.0),
            Err(LandingError::Builder(
                TrajectoryBuilderError::CoordinateOutOfRange
            ))
        );
        assert_eq!(trajectory.bytes(), bytes);
    }

    #[test]
    fn landing_needs_the_landing_time() {
        let mut trajectory = up_and_sideways();
        let stats = TrajectoryStats::default();
        assert_eq!(
            trajectory.replace_end_to_land_at(&stats, Vector3::default(), 500.0),
            Err(LandingError::MissingLandingTime)
        );
    }
}
