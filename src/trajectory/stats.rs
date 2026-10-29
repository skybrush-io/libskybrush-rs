//! Statistics of trajectories that can be gathered in a single pass: the
//! duration, the proposed takeoff and landing times, and the distance between
//! the start and the end.

use core::ops::{BitOr, BitOrAssign, Deref};
use core::{error, fmt};

use libm::{fabsf, hypotf};

use super::{Trajectory, TrajectoryPlayer, TrajectoryPlayerState, TrajectorySegment};
use crate::basic_types::Vector3WithYaw;
use crate::motion::travel_time_for_distance;

/// A set of components of the statistics of a trajectory.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TrajectoryStatComponents(u8);

impl TrajectoryStatComponents {
    /// No components.
    pub const NONE: Self = Self(0);
    /// The duration of the trajectory.
    pub const DURATION: Self = Self(1);
    /// The distance between the start and the end, in the XY plane.
    pub const START_END_DISTANCE: Self = Self(2);
    /// The proposed takeoff time.
    pub const TAKEOFF_TIME: Self = Self(4);
    /// The proposed landing time, with the position and the velocity there.
    pub const LANDING_TIME: Self = Self(8);
    /// All components.
    pub const ALL: Self = Self(15);

    /// The set of components with the given bits, as in the C library.
    /// Unknown bits are ignored.
    #[must_use]
    pub const fn from_bits_truncate(bits: u8) -> Self {
        Self(bits & Self::ALL.0)
    }

    /// The bits of the components, as in the C library.
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Whether the set contains all components of `other`.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether the set is empty.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Removes the components of `other` from the set.
    pub const fn remove(&mut self, other: Self) {
        self.0 &= !other.0;
    }
}

impl BitOr for TrajectoryStatComponents {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl BitOrAssign for TrajectoryStatComponents {
    fn bitor_assign(&mut self, other: Self) {
        self.0 |= other.0;
    }
}

/// The statistics of a trajectory. Components that were not calculated are
/// `None`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TrajectoryStats {
    /// The position at the start; calculated with any other component.
    pub initial_pos: Option<Vector3WithYaw>,
    /// The total duration, in milliseconds.
    pub duration_msec: Option<u32>,
    /// The proposed takeoff time.
    pub takeoff: Option<TakeoffStats>,
    /// The distance between the first and the last point, in the XY plane.
    pub start_to_end_distance_xy: Option<f32>,
    /// The proposed landing time, with the position and the velocity there.
    pub landing: Option<LandingStats>,
}

impl TrajectoryStats {
    /// The total duration, in seconds.
    #[must_use]
    pub fn duration_sec(&self) -> Option<f32> {
        self.duration_msec.map(super::msec_to_sec)
    }
}

/// The proposed takeoff time of a trajectory.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TakeoffStats {
    /// The earliest time when the trajectory is above the takeoff altitude,
    /// in seconds; infinity if it never gets there.
    pub earliest_above_sec: f32,
    /// The time when the takeoff has to start to reach the takeoff altitude
    /// in time, in seconds; infinity if the trajectory never reaches the
    /// takeoff altitude. May be negative.
    pub takeoff_time_sec: f32,
}

/// The proposed landing time of a trajectory.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LandingStats {
    /// The time when the landing has to start, in seconds.
    pub landing_time_sec: f32,
    /// The position at the landing time.
    pub pos_at_landing_time: Vector3WithYaw,
    /// The velocity at the landing time.
    pub vel_at_landing_time: Vector3WithYaw,
}

/// Why the statistics of a trajectory could not be calculated: a setting of
/// the calculator is invalid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrajectoryStatsError {
    /// The acceleration is not positive.
    InvalidAcceleration,
    /// The takeoff speed is not positive or not finite.
    InvalidTakeoffSpeed,
    /// The minimum ascent is negative or not finite.
    InvalidMinAscent,
    /// The preferred descent is negative or not finite.
    InvalidPreferredDescent,
}

impl fmt::Display for TrajectoryStatsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidAcceleration => "the acceleration is not positive",
            Self::InvalidTakeoffSpeed => "the takeoff speed is not positive or not finite",
            Self::InvalidMinAscent => "the minimum ascent is negative or not finite",
            Self::InvalidPreferredDescent => "the preferred descent is negative or not finite",
        })
    }
}

impl error::Error for TrajectoryStatsError {}

/// The settings of the calculation of the statistics of trajectories.
///
/// Distances, speeds and accelerations are in the units of the trajectory
/// (millimeters for trajectories of show files).
///
/// # Examples
///
/// ```
/// use skybrush::basic_types::Vector3WithYaw;
/// use skybrush::trajectory::{TrajectoryBuilder, TrajectoryStatsCalculator};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// # let mut builder = TrajectoryBuilder::new(1)?;
/// # builder.set_start_position(Vector3WithYaw::default())?;
/// # builder.append_line(Vector3WithYaw { z: 5000.0, ..Vector3WithYaw::default() }, 5000)?;
/// # builder.append_line(Vector3WithYaw::default(), 5000)?;
/// // A trajectory that climbs to 5 m in 5 s and lands in 5 s
/// let trajectory = builder.build()?;
/// // Coordinates in millimeters
/// let calculator = TrajectoryStatsCalculator::new(1000.0);
/// let stats = calculator.run(&trajectory)?;
/// assert_eq!(stats.duration_msec, Some(10_000));
/// assert!(stats.takeoff.is_some() && stats.landing.is_some());
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrajectoryStatsCalculator {
    /// The components to calculate.
    pub components: TrajectoryStatComponents,
    /// The assumed speed of the takeoff, in units per second.
    pub takeoff_speed: f32,
    /// The assumed vertical acceleration of the takeoff, in units per second
    /// squared. Infinity means constant speed during the entire takeoff.
    pub acceleration: f32,
    /// The ascent of the takeoff.
    pub min_ascent: f32,
    /// The preferred length of the descent in the landing phase, when the
    /// drone lands on its own instead of following the trajectory.
    pub preferred_descent: f32,
    /// The largest distance between the start and the end of a segment along
    /// the X and Y axes for which the segment counts as vertical.
    pub verticality_threshold: f32,
}

impl TrajectoryStatsCalculator {
    /// A calculator of all components with the default settings, for
    /// trajectories with `scale` units per meter (1000 for show files, which
    /// are in millimeters). The scale only affects the default settings.
    #[must_use]
    pub fn new(scale: f32) -> Self {
        Self {
            components: TrajectoryStatComponents::ALL,
            takeoff_speed: 2.0 * scale,
            acceleration: 4.0 * scale,
            min_ascent: 2.5 * scale,
            preferred_descent: 2.5 * scale,
            verticality_threshold: scale * 0.05,
        }
    }

    /// Sets the components to calculate.
    pub const fn set_components(&mut self, components: TrajectoryStatComponents) {
        self.components = components;
    }

    /// Calculates the statistics of a trajectory.
    ///
    /// # Errors
    ///
    /// Returns an error if a setting of the calculator is invalid.
    pub fn run(
        &self,
        trajectory: &Trajectory<'_>,
    ) -> Result<TrajectoryStats, TrajectoryStatsError> {
        self.validate()?;

        let mut result = TrajectoryStats::default();
        let mut components = self.components;
        if components.is_empty() {
            return Ok(result);
        }

        let mut player = TrajectoryPlayer::new(trajectory);
        let start = player.position_at(0.0);
        result.initial_pos = Some(start);
        let takeoff_altitude = start.z + self.min_ascent;

        let mut duration_msec: u32 = 0;
        let mut earliest_above_sec = f32::INFINITY;
        // The landing at the end of the last segment that is not vertical
        let mut landing = None;
        // The last run of vertically descending segments
        let mut vertical: Option<VerticalRun> = None;

        while player.has_more_segments() {
            if components.contains(TrajectoryStatComponents::DURATION) {
                let segment = player.current_segment();
                duration_msec = duration_msec.saturating_add(segment.duration_msec());
            }

            if components.contains(TrajectoryStatComponents::TAKEOFF_TIME) {
                let segment = player.current_segment();
                if let Some(rel_t) = segment.poly().z.touches(takeoff_altitude) {
                    earliest_above_sec = segment.start_time_sec() + rel_t * segment.duration_sec();
                    // No need to check the rest of the segments
                    components.remove(TrajectoryStatComponents::TAKEOFF_TIME);
                    if components.is_empty() {
                        break;
                    }
                }
            }

            if components.contains(TrajectoryStatComponents::LANDING_TIME) {
                let descending = is_segment_descending_vertically(
                    player.current_segment(),
                    self.verticality_threshold,
                );
                if descending {
                    let end_altitude = player.current_segment().end().z;
                    // The state is saved at the first segment of a run of
                    // vertical segments
                    let run = vertical.get_or_insert_with(|| {
                        let state = player.save_state();
                        VerticalRun {
                            start_altitude: state.segment().start().z,
                            end_altitude,
                            state,
                        }
                    });
                    run.end_altitude = end_altitude;
                } else {
                    // The landing cannot be earlier than the end of a segment
                    // that is not vertical
                    landing = Some(landing_at_end(player.current_segment()));
                    vertical = None;
                }
            }

            player.build_next_segment();
        }

        if self.components.contains(TrajectoryStatComponents::DURATION) {
            result.duration_msec = Some(duration_msec);
        }

        if self
            .components
            .contains(TrajectoryStatComponents::TAKEOFF_TIME)
        {
            // The takeoff takes this long with the assumed speed and
            // acceleration
            let adjustment =
                travel_time_for_distance(self.min_ascent, self.takeoff_speed, self.acceleration);
            let takeoff_time_sec = if earliest_above_sec.is_finite() && adjustment.is_finite() {
                earliest_above_sec - adjustment
            } else {
                f32::INFINITY
            };
            result.takeoff = Some(TakeoffStats {
                earliest_above_sec,
                takeoff_time_sec,
            });
        }

        if self
            .components
            .contains(TrajectoryStatComponents::START_END_DISTANCE)
        {
            let end = player.position_at(f32::INFINITY);
            result.start_to_end_distance_xy = Some(hypotf(end.x - start.x, end.y - start.y));
        }

        if self
            .components
            .contains(TrajectoryStatComponents::LANDING_TIME)
        {
            result.landing = self.landing(&mut player, landing, vertical);
        }

        Ok(result)
    }

    fn validate(&self) -> Result<(), TrajectoryStatsError> {
        if self.acceleration <= 0.0 {
            Err(TrajectoryStatsError::InvalidAcceleration)
        } else if !self.takeoff_speed.is_finite() || self.takeoff_speed <= 0.0 {
            Err(TrajectoryStatsError::InvalidTakeoffSpeed)
        } else if !self.min_ascent.is_finite() || self.min_ascent < 0.0 {
            Err(TrajectoryStatsError::InvalidMinAscent)
        } else if !self.preferred_descent.is_finite() || self.preferred_descent < 0.0 {
            Err(TrajectoryStatsError::InvalidPreferredDescent)
        } else {
            Ok(())
        }
    }

    /// The landing, from the landing at the end of the last segment that is
    /// not vertical, and the run of vertical segments after it, if any. The
    /// player is after the end of the trajectory.
    fn landing<'a, T: Deref<Target = Trajectory<'a>>>(
        &self,
        player: &mut TrajectoryPlayer<T>,
        mut landing: Option<LandingStats>,
        vertical: Option<VerticalRun>,
    ) -> Option<LandingStats> {
        if let Some(run) = vertical {
            // The trajectory ends with a run of vertical segments. The drone
            // follows the trajectory until it is above the end of the run by
            // the preferred descent, then lands on its own.
            player.restore_state(&run.state);
            let mut to_descend = run.start_altitude - (run.end_altitude + self.preferred_descent);
            landing = None;

            if to_descend > 0.0 {
                let mut altitude = player.current_segment().start().z;
                while player.has_more_segments() {
                    let segment = player.current_segment();
                    let delta = altitude - segment.end().z;
                    if delta < 0.0 {
                        // An ascending segment; it cannot happen in a run of
                        // vertically descending segments. The C library
                        // leaves the landing time uncalculated then.
                        return None;
                    } else if delta <= to_descend {
                        // The drone follows the entire segment; it lands at
                        // its end if it is the last one
                        to_descend -= delta;
                        altitude = segment.end().z;
                        landing = Some(landing_at_end(segment));
                    } else {
                        // The drone lands within the segment
                        let poly = *segment.poly();
                        let rel_t = poly.z.touches(altitude - to_descend).unwrap_or(0.0);
                        landing = Some(LandingStats {
                            landing_time_sec: segment.start_time_sec()
                                + rel_t * segment.duration_sec(),
                            pos_at_landing_time: poly.eval(rel_t),
                            vel_at_landing_time: segment.dpoly().eval(rel_t),
                        });
                        break;
                    }
                    player.build_next_segment();
                }
            }
        }

        // Either the last run of vertical segments is too short and the
        // landing starts at its beginning, or the trajectory has no segments
        // and the landing starts at its start
        landing.or_else(|| {
            let segment = player.current_segment();
            Some(LandingStats {
                landing_time_sec: segment.start_time_sec(),
                pos_at_landing_time: segment.start(),
                vel_at_landing_time: segment.dpoly().eval(0.0),
            })
        })
    }
}

impl Default for TrajectoryStatsCalculator {
    /// The default calculator for trajectories in meters.
    fn default() -> Self {
        Self::new(1.0)
    }
}

/// A run of vertically descending segments: the state of the player at its
/// first segment, and the altitudes at its start and end.
struct VerticalRun {
    state: TrajectoryPlayerState,
    start_altitude: f32,
    end_altitude: f32,
}

/// The landing at the end of a segment, with the velocity at its end.
fn landing_at_end(segment: &mut TrajectorySegment) -> LandingStats {
    LandingStats {
        landing_time_sec: segment.end_time_sec(),
        pos_at_landing_time: segment.end(),
        vel_at_landing_time: segment.dpoly().eval(1.0),
    }
}

/// Whether a segment descends vertically: its start and end are not farther
/// than `threshold` along the X and Y axes, it does not ascend, and it moves
/// at least as much vertically as along either horizontal axis (so that
/// short horizontal segments do not count as vertical).
fn is_segment_descending_vertically(segment: &TrajectorySegment, threshold: f32) -> bool {
    let (start, end) = (segment.start(), segment.end());
    if start.z < end.z {
        return false;
    }
    let dx = fabsf(start.x - end.x);
    let dy = fabsf(start.y - end.y);
    let dz = start.z - end.z;
    dx <= threshold && dy <= threshold && dz >= dx && dz >= dy
}

impl Trajectory<'_> {
    /// Proposes a takeoff time for the trajectory, in seconds, assuming that
    /// the drone starts at the first point of the trajectory and takes off
    /// vertically by `min_ascent`, accelerating with `acceleration` up to
    /// `speed` and decelerating to a stop at the end. An infinite
    /// acceleration means constant speed during the entire takeoff.
    ///
    /// Returns infinity if the settings are invalid or the trajectory never
    /// gets `min_ascent` above its first point. The result may be negative.
    #[must_use]
    pub fn propose_takeoff_time_sec(&self, min_ascent: f32, speed: f32, acceleration: f32) -> f32 {
        let calc = TrajectoryStatsCalculator {
            components: TrajectoryStatComponents::TAKEOFF_TIME,
            acceleration,
            takeoff_speed: speed,
            min_ascent,
            ..TrajectoryStatsCalculator::new(1.0)
        };
        calc.run(self)
            .ok()
            .and_then(|stats| stats.takeoff)
            .map_or(f32::INFINITY, |takeoff| takeoff.takeoff_time_sec)
    }

    /// Proposes a landing time for the trajectory, in seconds, assuming that
    /// the drone lands directly below the last point of the trajectory. The
    /// landing starts when the drone is above the last point by at most
    /// `preferred_descent`, on the last run of vertical segments of the
    /// trajectory; a segment is vertical if its start and end are not farther
    /// than `verticality_threshold` along the X and Y axes.
    ///
    /// A preferred descent that is not positive and finite means landing at
    /// the end of the trajectory. A negative verticality threshold is treated
    /// as zero. The result is at most the duration of the trajectory.
    #[must_use]
    pub fn propose_landing_time_sec(
        &self,
        preferred_descent: f32,
        verticality_threshold: f32,
    ) -> f32 {
        if !verticality_threshold.is_finite()
            || !preferred_descent.is_finite()
            || preferred_descent <= f32::MIN_POSITIVE
        {
            return self.total_duration_sec();
        }
        let calc = TrajectoryStatsCalculator {
            components: TrajectoryStatComponents::LANDING_TIME,
            preferred_descent,
            verticality_threshold: verticality_threshold.max(0.0),
            ..TrajectoryStatsCalculator::new(1.0)
        };
        calc.run(self)
            .ok()
            .and_then(|stats| stats.landing)
            .map_or(f32::INFINITY, |landing| landing.landing_time_sec)
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec;

    use super::*;

    /// A trajectory with a scale of 1, starting at (0, 0, 3000), with a
    /// single segment of one second that moves linearly to (40, 0, 2970).
    fn short_sideways_descent() -> Trajectory<'static> {
        let mut bytes = vec![1];
        for coordinate in [0_i16, 0, 3000, 0] {
            bytes.extend(coordinate.to_le_bytes());
        }
        // Linear along X and Z, constant along Y
        bytes.push(0x11);
        bytes.extend(1000_u16.to_le_bytes());
        bytes.extend(40_i16.to_le_bytes());
        bytes.extend(2970_i16.to_le_bytes());
        Trajectory::from_bytes(bytes).unwrap()
    }

    #[test]
    fn segments_that_move_more_sideways_than_down_are_not_vertical() {
        // Both horizontal distances are within the threshold of 50, but the
        // segment moves farther along X than down, so the landing cannot be
        // earlier than its end
        let stats = TrajectoryStatsCalculator::new(1000.0)
            .run(&short_sideways_descent())
            .unwrap();
        let landing = stats.landing.unwrap();
        assert_eq!(landing.landing_time_sec, 1.0);
        assert_eq!(landing.pos_at_landing_time.x, 40.0);
    }

    #[test]
    fn no_components_calculate_nothing() {
        let mut calc = TrajectoryStatsCalculator::new(1000.0);
        calc.set_components(TrajectoryStatComponents::NONE);
        let stats = calc.run(&short_sideways_descent()).unwrap();
        assert_eq!(stats, TrajectoryStats::default());
    }

    #[test]
    fn invalid_settings_are_rejected() {
        let calc = TrajectoryStatsCalculator {
            min_ascent: f32::NAN,
            ..TrajectoryStatsCalculator::new(1000.0)
        };
        assert_eq!(
            calc.run(&short_sideways_descent()),
            Err(TrajectoryStatsError::InvalidMinAscent)
        );
    }
}
