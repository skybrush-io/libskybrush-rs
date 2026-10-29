//! Playing a yaw control object: finding the yaw and the yaw rate at given
//! time instants.

use core::ops::Deref;

use super::{DELTA_LENGTH, YawControl};
use crate::parsing::{read_i16, read_u16};

/// A setpoint of the yaw: a linear change of the yaw over a time interval,
/// decoded from a delta. The times and yaws are given both in the units of
/// the block and in seconds and degrees.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct YawSetpoint {
    /// The time when the setpoint starts, in seconds.
    pub start_time_sec: f32,
    /// The time when the setpoint starts, in milliseconds.
    pub start_time_msec: u32,
    /// The duration of the setpoint, in seconds.
    pub duration_sec: f32,
    /// The duration of the setpoint, in milliseconds.
    pub duration_msec: u32,
    /// The time when the setpoint ends, in seconds.
    pub end_time_sec: f32,
    /// The time when the setpoint ends, in milliseconds.
    pub end_time_msec: u32,
    /// The yaw at the start of the setpoint, in tenths of degrees.
    pub start_yaw_ddeg: i32,
    /// The yaw at the start of the setpoint, in degrees.
    pub start_yaw_deg: f32,
    /// The change of the yaw during the setpoint, in tenths of degrees.
    pub yaw_change_ddeg: i16,
    /// The change of the yaw during the setpoint, in degrees.
    pub yaw_change_deg: f32,
    /// The yaw at the end of the setpoint, in tenths of degrees.
    pub end_yaw_ddeg: i32,
    /// The yaw at the end of the setpoint, in degrees.
    pub end_yaw_deg: f32,
}

/// Plays a yaw control object, one setpoint at a time.
///
/// After the last delta, the player has a final setpoint that keeps the last
/// yaw forever. The player holds the yaw control object through any pointer
/// to it, e.g. a reference or an `Rc`.
///
/// # Examples
///
/// ```
/// use skybrush::yaw_control::{YawControl, YawPlayer};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// // A yaw of 90 degrees, then a turn by 45 degrees in 1 s; yaws are given in
/// // tenths of degrees
/// let ctrl = YawControl::from_buffer(&[0x00, 0x84, 0x03, 0xe8, 0x03, 0xc2, 0x01])?;
/// let mut player = YawPlayer::new(&ctrl);
/// assert_eq!(player.yaw_at(0.0), 90.0);
/// assert_eq!(player.yaw_at(0.5), 112.5);
/// assert_eq!(player.yaw_rate_at(0.5), 45.0);
/// assert_eq!(player.yaw_at(10.0), 135.0);
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct YawPlayer<T> {
    ctrl: T,
    /// The offset of the delta of the current setpoint.
    start: usize,
    /// The length of the delta of the current setpoint; zero for the final
    /// setpoint.
    length: usize,
    setpoint: YawSetpoint,
}

impl<'a, T: Deref<Target = YawControl<'a>>> YawPlayer<T> {
    /// A player of the given yaw control object, at its start.
    pub fn new(ctrl: T) -> Self {
        let mut player = Self {
            ctrl,
            start: 0,
            length: 0,
            setpoint: YawSetpoint::default(),
        };
        player.rewind();
        player
    }

    /// The yaw control object being played.
    pub fn yaw_control(&self) -> &YawControl<'a> {
        &self.ctrl
    }

    /// The current setpoint.
    pub const fn current_setpoint(&self) -> &YawSetpoint {
        &self.setpoint
    }

    /// Whether the current setpoint comes from a delta, i.e. it is not the
    /// final setpoint that keeps the last yaw forever.
    pub const fn has_more_setpoints(&self) -> bool {
        self.length > 0
    }

    /// Moves on to the next setpoint.
    pub fn build_next_setpoint(&mut self) {
        self.build_setpoint(
            self.start + self.length,
            self.setpoint.end_time_msec,
            self.setpoint.end_yaw_ddeg,
        );
    }

    /// The yaw at the given time, in seconds, in degrees.
    pub fn yaw_at(&mut self, time_sec: f32) -> f32 {
        let rel_time = self.seek_to_time(time_sec);
        self.setpoint.start_yaw_deg + self.setpoint.yaw_change_deg * rel_time
    }

    /// The yaw rate at the given time, in seconds, in degrees per second. It
    /// is infinite within a setpoint of zero duration.
    pub fn yaw_rate_at(&mut self, time_sec: f32) -> f32 {
        self.seek_to_time(time_sec);
        if self.setpoint.duration_sec == 0.0 {
            f32::INFINITY
        } else {
            self.setpoint.yaw_change_deg / self.setpoint.duration_sec
        }
    }

    /// The total duration of the deltas, in milliseconds. The player is
    /// moved to the final setpoint.
    pub fn total_duration_msec(&mut self) -> u32 {
        self.rewind();
        let mut result: u32 = 0;
        while self.has_more_setpoints() {
            result = result.wrapping_add(self.setpoint.duration_msec);
            self.build_next_setpoint();
        }
        result
    }

    fn rewind(&mut self) {
        let start = self.ctrl.header_length;
        let yaw_offset_ddeg = i32::from(self.ctrl.yaw_offset_ddeg);
        self.build_setpoint(start, 0, yaw_offset_ddeg);
    }

    /// Builds the setpoint of the delta at the given offset, starting at the
    /// given time and yaw.
    fn build_setpoint(&mut self, offset: usize, start_time_msec: u32, start_yaw_ddeg: i32) {
        self.start = offset;
        self.length = 0;
        let mut setpoint = YawSetpoint {
            start_time_msec,
            start_time_sec: msec_to_sec(start_time_msec),
            start_yaw_ddeg,
            start_yaw_deg: ddeg_to_deg(start_yaw_ddeg),
            ..YawSetpoint::default()
        };

        let bytes = self.ctrl.bytes();
        let delta = (
            read_u16(bytes, offset),
            read_i16(bytes, offset + DELTA_LENGTH / 2),
        );
        if let (Some(duration_msec), Some(yaw_change_ddeg)) = delta {
            setpoint.duration_msec = u32::from(duration_msec);
            setpoint.duration_sec = msec_to_sec(setpoint.duration_msec);
            setpoint.end_time_msec = start_time_msec.wrapping_add(setpoint.duration_msec);
            setpoint.end_time_sec = msec_to_sec(setpoint.end_time_msec);
            setpoint.yaw_change_ddeg = yaw_change_ddeg;
            setpoint.yaw_change_deg = f32::from(yaw_change_ddeg) / 10.0;
            setpoint.end_yaw_ddeg = start_yaw_ddeg.wrapping_add(i32::from(yaw_change_ddeg));
            setpoint.end_yaw_deg = ddeg_to_deg(setpoint.end_yaw_ddeg);
            self.length = DELTA_LENGTH;
        } else {
            // There are no more deltas; the last yaw is kept forever
            setpoint.duration_msec = u32::MAX - start_time_msec;
            setpoint.duration_sec = f32::INFINITY;
            setpoint.end_time_msec = u32::MAX;
            setpoint.end_time_sec = f32::INFINITY;
            setpoint.end_yaw_ddeg = start_yaw_ddeg;
            setpoint.end_yaw_deg = setpoint.start_yaw_deg;
        }
        self.setpoint = setpoint;
    }

    /// Moves the player to the setpoint that contains the given time, in
    /// seconds, and returns the relative time within the setpoint, from 0 at
    /// its start to 1 at its end.
    fn seek_to_time(&mut self, time_sec: f32) -> f32 {
        let time_sec = if time_sec <= 0.0 { 0.0 } else { time_sec };
        loop {
            if self.setpoint.start_time_sec > time_sec {
                // Before the current setpoint; start from scratch
                self.rewind();
            } else if self.setpoint.end_time_sec < time_sec {
                // This terminates, since the final setpoint never ends
                self.build_next_setpoint();
            } else if !time_sec.is_finite() {
                // NaN, or the end of the final setpoint
                return 1.0;
            } else if self.setpoint.duration_sec.abs() > 1.0e-6 {
                return (time_sec - self.setpoint.start_time_sec) / self.setpoint.duration_sec;
            } else {
                return 0.5;
            }
        }
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "times are floats in seconds, as in the C library"
)]
fn msec_to_sec(msec: u32) -> f32 {
    msec as f32 / 1000.0
}

#[expect(
    clippy::cast_precision_loss,
    reason = "yaws are floats in degrees, as in the C library"
)]
fn ddeg_to_deg(ddeg: i32) -> f32 {
    ddeg as f32 / 10.0
}
