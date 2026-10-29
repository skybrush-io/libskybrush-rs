//! Time axes: mappings from the wall clock to the warped time of a scene,
//! where the time may run faster or slower than real time, or stop.
//!
//! A time axis starts at its origin on the wall clock and consists of
//! segments, each with a duration on the wall clock and a rate of time that
//! changes linearly from an initial to a final rate. Before the origin, and
//! on an axis without segments, the time runs in real time; after the last
//! segment, it runs at the final rate of the last segment.

use alloc::collections::TryReserveError;
use alloc::vec::Vec;
use core::{error, fmt};

use libm::{ceilf, fabsf, sqrtf};

/// Why a time axis could not be modified.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TimeAxisError {
    /// A rate of the segment is negative or NaN.
    InvalidSegment,
    /// The index is beyond the end of the time axis.
    IndexOutOfRange,
    /// The origin is negative, not finite, or out of the range of the axis.
    InvalidOrigin,
    /// Memory could not be allocated.
    OutOfMemory,
}

impl fmt::Display for TimeAxisError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidSegment => "a rate of the segment is negative or NaN",
            Self::IndexOutOfRange => "the index is beyond the end of the time axis",
            Self::InvalidOrigin => "the origin is out of range",
            Self::OutOfMemory => "out of memory",
        })
    }
}

impl error::Error for TimeAxisError {}

impl From<TryReserveError> for TimeAxisError {
    fn from(_: TryReserveError) -> Self {
        Self::OutOfMemory
    }
}

/// A segment of a time axis: a duration on the wall clock, during which the
/// rate of time changes linearly from the initial to the final rate. A rate
/// of 1 is real time, 0 is stopped time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimeSegment {
    /// The duration on the wall clock, in milliseconds; `u32::MAX` means
    /// infinite.
    pub duration_msec: u32,
    /// The rate of time at the start of the segment.
    pub initial_rate: f32,
    /// The rate of time at the end of the segment.
    pub final_rate: f32,
}

impl TimeSegment {
    /// A segment with the given duration, in milliseconds, and rates.
    #[must_use]
    pub const fn new(duration_msec: u32, initial_rate: f32, final_rate: f32) -> Self {
        Self {
            duration_msec,
            initial_rate,
            final_rate,
        }
    }

    /// A segment in real time.
    #[must_use]
    pub const fn realtime(duration_msec: u32) -> Self {
        Self::new(duration_msec, 1.0, 1.0)
    }

    /// A segment with a constant rate.
    #[must_use]
    pub const fn constant_rate(duration_msec: u32, rate: f32) -> Self {
        Self::new(duration_msec, rate, rate)
    }

    /// A segment that slows down from the given rate to a standstill.
    #[must_use]
    pub const fn slowdown_from(duration_msec: u32, initial_rate: f32) -> Self {
        Self::new(duration_msec, initial_rate, 0.0)
    }

    /// A segment that slows down from real time to a standstill.
    #[must_use]
    pub const fn slowdown_from_realtime(duration_msec: u32) -> Self {
        Self::slowdown_from(duration_msec, 1.0)
    }

    /// A segment that speeds up from a standstill to the given rate.
    #[must_use]
    pub const fn spinup_to(duration_msec: u32, final_rate: f32) -> Self {
        Self::new(duration_msec, 0.0, final_rate)
    }

    /// A segment that speeds up from a standstill to real time.
    #[must_use]
    pub const fn spinup_to_realtime(duration_msec: u32) -> Self {
        Self::spinup_to(duration_msec, 1.0)
    }

    /// A segment with the given duration in warped time, in seconds, and
    /// rates. The duration on the wall clock is rounded up to the next
    /// millisecond. It is zero if the warped duration is zero, and infinite
    /// if the warped duration is infinite or both rates are zero.
    ///
    /// Returns `None` if the warped duration or a rate is negative, the
    /// warped duration is NaN, or a rate is not finite.
    #[must_use]
    pub fn warped(warped_duration_sec: f32, initial_rate: f32, final_rate: f32) -> Option<Self> {
        if warped_duration_sec < 0.0
            || initial_rate < 0.0
            || final_rate < 0.0
            || warped_duration_sec.is_nan()
            || !initial_rate.is_finite()
            || !final_rate.is_finite()
        {
            return None;
        }
        if warped_duration_sec == 0.0 {
            // Instantaneous, whatever the rates are
            return Some(Self::new(0, initial_rate, final_rate));
        }
        let average_rate = average(initial_rate, final_rate);
        if average_rate == 0.0 {
            return Some(Self::new(u32::MAX, initial_rate, final_rate));
        }
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a non-negative integer; durations beyond u32::MAX saturate to infinity"
        )]
        let duration_msec = ceilf(warped_duration_sec / average_rate * 1000.0) as u32;
        Some(Self::new(duration_msec, initial_rate, final_rate))
    }

    /// Whether the rate of time is the same at the start and at the end.
    #[must_use]
    #[expect(clippy::float_cmp, reason = "a constant rate is exactly constant")]
    pub fn has_constant_rate(&self) -> bool {
        self.initial_rate == self.final_rate
    }

    /// Whether the duration is infinite.
    #[must_use]
    pub const fn is_infinite(&self) -> bool {
        self.duration_msec == u32::MAX
    }

    /// The duration on the wall clock, in milliseconds; `u32::MAX` if it is
    /// infinite.
    #[must_use]
    pub const fn duration_in_wall_clock_time_msec(&self) -> u32 {
        self.duration_msec
    }

    /// The duration on the wall clock, in seconds.
    #[must_use]
    pub fn duration_in_wall_clock_time_sec(&self) -> f32 {
        if self.is_infinite() {
            f32::INFINITY
        } else {
            msec_to_sec(self.duration_msec)
        }
    }

    /// The duration in warped time, in seconds.
    #[must_use]
    pub fn duration_in_warped_time_sec(&self) -> f32 {
        let duration_sec = self.duration_in_wall_clock_time_sec();
        if duration_sec.is_infinite() {
            f32::INFINITY
        } else {
            duration_sec * average(self.initial_rate, self.final_rate)
        }
    }

    /// Checks that the rates are non-negative and not NaN; the duration may
    /// be infinite.
    fn validate(&self) -> Result<(), TimeAxisError> {
        if self.initial_rate.is_nan()
            || self.final_rate.is_nan()
            || self.initial_rate < 0.0
            || self.final_rate < 0.0
        {
            Err(TimeAxisError::InvalidSegment)
        } else {
            Ok(())
        }
    }
}

/// A time axis: an origin on the wall clock and a list of segments.
///
/// # Examples
///
/// ```
/// use skybrush::time_axis::{TimeAxis, TimeSegment};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// // Real time for 10 s, then slowing down to a standstill in 2 s
/// let mut axis = TimeAxis::new();
/// axis.append_segment(TimeSegment::realtime(10_000))?;
/// axis.append_segment(TimeSegment::slowdown_from_realtime(2000))?;
/// axis.append_segment(TimeSegment::constant_rate(u32::MAX, 0.0))?;
/// assert_eq!(axis.map(5000), 5.0);
/// // The time stops at 11 s
/// assert_eq!(axis.map(60_000), 11.0);
/// assert_eq!(axis.reverse_map(5.0), 5000);
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TimeAxis {
    /// The time on the wall clock when the warped time is zero, in
    /// milliseconds.
    origin_msec: i32,
    segments: Vec<TimeSegment>,
}

impl TimeAxis {
    /// A time axis at the origin of the wall clock, without segments.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            origin_msec: 0,
            segments: Vec::new(),
        }
    }

    /// The number of segments that the time axis can hold without
    /// allocating.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.segments.capacity()
    }

    /// The number of segments.
    #[must_use]
    pub const fn num_segments(&self) -> usize {
        self.segments.len()
    }

    /// The segment at the given index, or `None` if there is no such
    /// segment.
    #[must_use]
    pub fn segment(&self, index: usize) -> Option<&TimeSegment> {
        self.segments.get(index)
    }

    /// The segments of the time axis.
    #[must_use]
    pub fn segments(&self) -> &[TimeSegment] {
        &self.segments
    }

    /// The time on the wall clock when the warped time is zero, in
    /// milliseconds.
    #[must_use]
    pub const fn origin_msec(&self) -> i32 {
        self.origin_msec
    }

    /// The time on the wall clock when the warped time is zero, in seconds.
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "times in seconds are floats, as in the C library"
    )]
    pub fn origin_sec(&self) -> f32 {
        self.origin_msec as f32 / 1000.0
    }

    /// Sets the origin, in milliseconds.
    ///
    /// # Errors
    ///
    /// Returns [`TimeAxisError::InvalidOrigin`] if the origin is negative;
    /// the origin is left intact then.
    pub const fn set_origin_msec(&mut self, origin_msec: i32) -> Result<(), TimeAxisError> {
        if origin_msec < 0 {
            return Err(TimeAxisError::InvalidOrigin);
        }
        self.origin_msec = origin_msec;
        Ok(())
    }

    /// Sets the origin, in seconds; it is truncated to milliseconds.
    ///
    /// # Errors
    ///
    /// Returns [`TimeAxisError::InvalidOrigin`] if the origin is negative,
    /// not finite, or too large for milliseconds in an `i32`; the origin is
    /// left intact then.
    #[expect(
        clippy::cast_precision_loss,
        reason = "the limit is compared with a float, as in the C library"
    )]
    pub fn set_origin_sec(&mut self, origin_sec: f32) -> Result<(), TimeAxisError> {
        if !origin_sec.is_finite() || origin_sec < 0.0 || origin_sec > (i32::MAX / 1000) as f32 {
            return Err(TimeAxisError::InvalidOrigin);
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the origin is in the range of i32 and is truncated, as in the C library"
        )]
        let origin_msec = (origin_sec * 1000.0) as i32;
        self.origin_msec = origin_msec;
        Ok(())
    }

    /// The total duration on the wall clock, in milliseconds; `u32::MAX` if
    /// it is infinite or does not fit into 32 bits.
    #[must_use]
    pub fn total_duration_msec(&self) -> u32 {
        let mut total: u32 = 0;
        for segment in &self.segments {
            if segment.is_infinite() {
                return u32::MAX;
            }
            let Some(sum) = total.checked_add(segment.duration_msec) else {
                return u32::MAX;
            };
            total = sum;
        }
        total
    }

    /// The total duration on the wall clock, in seconds; infinite if it is
    /// infinite or does not fit into 32-bit milliseconds.
    #[must_use]
    pub fn total_duration_sec(&self) -> f32 {
        match self.total_duration_msec() {
            u32::MAX => f32::INFINITY,
            total => msec_to_sec(total),
        }
    }

    /// The total duration in warped time, in seconds. An axis with a segment
    /// of infinite duration is infinite, even if its time is stopped.
    #[must_use]
    pub fn total_warped_duration_sec(&self) -> f32 {
        self.segments
            .iter()
            .map(TimeSegment::duration_in_warped_time_sec)
            .fold(0.0, |total, duration| total + duration)
    }

    /// Removes every segment; the origin is kept.
    pub fn clear(&mut self) {
        self.segments.clear();
    }

    /// Appends a segment to the end of the time axis.
    ///
    /// # Errors
    ///
    /// Returns [`TimeAxisError::InvalidSegment`] if a rate is negative or
    /// NaN, and [`TimeAxisError::OutOfMemory`] if memory could not be
    /// allocated. The time axis is left intact then.
    pub fn append_segment(&mut self, segment: TimeSegment) -> Result<(), TimeAxisError> {
        self.insert_segment_at(self.segments.len(), segment)
    }

    /// Inserts a segment at the given index.
    ///
    /// # Errors
    ///
    /// Returns [`TimeAxisError::IndexOutOfRange`] if the index is beyond the
    /// end of the time axis, [`TimeAxisError::InvalidSegment`] if a rate is
    /// negative or NaN, and [`TimeAxisError::OutOfMemory`] if memory could
    /// not be allocated. The time axis is left intact then.
    pub fn insert_segment_at(
        &mut self,
        index: usize,
        segment: TimeSegment,
    ) -> Result<(), TimeAxisError> {
        if index > self.segments.len() {
            return Err(TimeAxisError::IndexOutOfRange);
        }
        segment.validate()?;
        self.segments.try_reserve(1)?;
        self.segments.insert(index, segment);
        Ok(())
    }

    /// Removes the segment at the given index and returns it, or returns
    /// `None` if there is no such segment.
    pub fn remove_segment_at(&mut self, index: usize) -> Option<TimeSegment> {
        (index < self.segments.len()).then(|| self.segments.remove(index))
    }

    /// The warped time, in seconds, at the given time of the wall clock, in
    /// milliseconds.
    #[must_use]
    pub fn map(&self, wall_clock_time_msec: i32) -> f32 {
        self.map_with_rate(wall_clock_time_msec).0
    }

    /// The warped time, in seconds, at the given time of the wall clock, in
    /// milliseconds, and the rate of time there. Segments contain their
    /// start but not their end: at the end of a segment, the rate is the
    /// initial rate of the next segment, or the final rate of the last one.
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "times in seconds are floats, as in the C library"
    )]
    pub fn map_with_rate(&self, wall_clock_time_msec: i32) -> (f32, f32) {
        // Before the origin, the time runs in real time
        let Ok(mut msec) =
            u32::try_from(i64::from(wall_clock_time_msec) - i64::from(self.origin_msec))
        else {
            let msec = i64::from(wall_clock_time_msec) - i64::from(self.origin_msec);
            return (msec as f32 / 1000.0, 1.0);
        };
        if self.segments.is_empty() {
            return (msec_to_sec(msec), 1.0);
        }

        let mut accumulated_sec = 0.0;
        for segment in &self.segments {
            let duration_msec = segment.duration_msec;
            if segment.is_infinite() || duration_msec > msec {
                // The time is within this segment
                let (in_segment_sec, rate) = if segment.is_infinite() || segment.has_constant_rate()
                {
                    (
                        msec_to_sec(msec) * segment.initial_rate,
                        segment.initial_rate,
                    )
                } else if duration_msec > 0 {
                    let delta_rate = segment.final_rate - segment.initial_rate;
                    let relative_time = msec as f32 / duration_msec as f32;
                    (
                        (segment.initial_rate + delta_rate / 2.0 * relative_time) * msec as f32
                            / 1000.0,
                        segment.initial_rate + delta_rate * relative_time,
                    )
                } else {
                    (0.0, average(segment.initial_rate, segment.final_rate))
                };
                return (accumulated_sec + in_segment_sec, rate);
            }
            msec -= duration_msec;
            accumulated_sec += segment.duration_in_warped_time_sec();
        }

        // After the last segment, the time runs at its final rate
        let final_rate = self
            .segments
            .last()
            .map_or(1.0, |segment| segment.final_rate);
        (accumulated_sec + msec_to_sec(msec) * final_rate, final_rate)
    }

    /// The time of the wall clock, in milliseconds, when the warped time is
    /// the given one, in seconds. Times that never come, or that do not fit
    /// into an `i32`, are `i32::MAX` (or `i32::MIN` if they are negative).
    #[must_use]
    pub fn reverse_map(&self, time_sec: f32) -> i32 {
        let wall_clock_sec = if time_sec.is_nan() {
            0.0
        } else if time_sec.is_infinite() {
            return if time_sec < 0.0 { i32::MIN } else { i32::MAX };
        } else if time_sec < 0.0 || self.segments.is_empty() {
            // Before the origin, the time runs in real time
            time_sec
        } else {
            self.reverse_map_segments(time_sec)
        };
        if !wall_clock_sec.is_finite() {
            return if wall_clock_sec < 0.0 {
                i32::MIN
            } else {
                i32::MAX
            };
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "truncated as in the C library; out of range values saturate"
        )]
        let msec = (wall_clock_sec * 1000.0) as i32;
        self.origin_msec.saturating_add(msec)
    }

    /// The time of the wall clock since the origin, in seconds, when the
    /// non-negative warped time is the given one.
    fn reverse_map_segments(&self, time_sec: f32) -> f32 {
        let mut remaining_sec = time_sec;
        let mut accumulated_sec = 0.0;
        for segment in &self.segments {
            if remaining_sec <= 0.0 {
                break;
            }
            let warped_duration_sec = segment.duration_in_warped_time_sec();
            if remaining_sec < warped_duration_sec {
                // The time is within this segment
                if !warped_duration_sec.is_finite() || segment.has_constant_rate() {
                    accumulated_sec += if segment.initial_rate != 0.0 {
                        remaining_sec / segment.initial_rate
                    } else if remaining_sec > 0.0 {
                        // The time is stopped, so it never comes
                        f32::INFINITY
                    } else {
                        0.0
                    };
                } else if warped_duration_sec > 0.0 {
                    // Solve remaining = (initial + delta / 2 * t / T) * t for
                    // the wall clock time t, where T is the duration
                    let roots = solve_quadratic(
                        (segment.final_rate - segment.initial_rate)
                            / (2.0 * segment.duration_in_wall_clock_time_sec()),
                        segment.initial_rate,
                        -remaining_sec,
                    );
                    // The positive root, if there is one
                    accumulated_sec += match roots {
                        [Some(first), Some(second)] => {
                            if first >= 0.0 {
                                first
                            } else {
                                second
                            }
                        }
                        [Some(root), None] => root,
                        _ => 0.0,
                    };
                }
                return accumulated_sec;
            }
            remaining_sec -= warped_duration_sec;
            accumulated_sec += segment.duration_in_wall_clock_time_sec();
        }

        // After the last segment, the time runs at its final rate
        if remaining_sec > 0.0 {
            let final_rate = self
                .segments
                .last()
                .map_or(1.0, |segment| segment.final_rate);
            accumulated_sec += if final_rate == 0.0 {
                f32::INFINITY
            } else {
                remaining_sec / final_rate
            };
        }
        accumulated_sec
    }
}

/// The average of two rates, computed as in the C library.
#[expect(clippy::manual_midpoint, reason = "rounded like the C library")]
fn average(first: f32, second: f32) -> f32 {
    (first + second) / 2.0
}

/// Solves `ax^2 + bx + c = 0`, with the smaller root first, like the C
/// library.
fn solve_quadratic(a: f32, b: f32, c: f32) -> [Option<f32>; 2] {
    if fabsf(a) < f32::MIN_POSITIVE {
        if fabsf(b) < f32::MIN_POSITIVE {
            return [None, None];
        }
        return [Some(-c / b), None];
    }
    let d = b * b - 4.0 * a * c;
    if fabsf(d) < f32::MIN_POSITIVE {
        return [Some(-b / (2.0 * a)), None];
    }
    if d > 0.0 {
        let d = sqrtf(d);
        let (low, high) = ((-b - d) / (2.0 * a), (-b + d) / (2.0 * a));
        // The parabola opens downwards if a < 0, which swaps the roots
        return if a < 0.0 {
            [Some(high), Some(low)]
        } else {
            [Some(low), Some(high)]
        };
    }
    [None, None]
}

#[expect(
    clippy::cast_precision_loss,
    reason = "times in seconds are floats, as in the C library"
)]
fn msec_to_sec(msec: u32) -> f32 {
    msec as f32 / 1000.0
}
