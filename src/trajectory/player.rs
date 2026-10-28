//! Playing back a trajectory: positions, velocities and accelerations at given
//! time instants.

use core::ops::Deref;

use super::Trajectory;
use super::segment::TrajectorySegment;
use crate::basic_types::Vector3WithYaw;

/// The state of a trajectory player: the segment it is at.
#[derive(Clone, Copy, Debug)]
pub struct TrajectoryPlayerState {
    /// Offset of the current segment in the bytes of the trajectory.
    start: usize,
    /// Length of the current segment in the bytes; zero after the end of the
    /// trajectory.
    length: usize,
    /// The current segment.
    segment: TrajectorySegment,
}

impl TrajectoryPlayerState {
    /// Offset of the current segment in the bytes of the trajectory.
    pub(crate) const fn start(&self) -> usize {
        self.start
    }

    /// Length of the current segment in the bytes; zero after the end of the
    /// trajectory.
    pub(crate) const fn length(&self) -> usize {
        self.length
    }

    /// The segment of the state.
    #[must_use]
    pub const fn segment(&self) -> &TrajectorySegment {
        &self.segment
    }
}

/// Plays back a trajectory. It decodes one segment at a time, so it never
/// allocates.
///
/// The player holds the trajectory through any pointer to it, e.g. a
/// reference or an `Rc`.
#[derive(Clone, Debug)]
pub struct TrajectoryPlayer<T> {
    trajectory: T,
    state: TrajectoryPlayerState,
}

impl<'a, T: Deref<Target = Trajectory<'a>>> TrajectoryPlayer<T> {
    /// A player of the given trajectory, at its start.
    pub fn new(trajectory: T) -> Self {
        let state = Self::state_at(
            &trajectory,
            trajectory.header_length(),
            0,
            trajectory.start(),
        );
        Self { trajectory, state }
    }

    /// The trajectory being played.
    pub fn trajectory(&self) -> &Trajectory<'a> {
        &self.trajectory
    }

    /// Rewinds the player to the start of the trajectory.
    pub fn rewind(&mut self) {
        self.state = Self::state_at(
            &self.trajectory,
            self.trajectory.header_length(),
            0,
            self.trajectory.start(),
        );
    }

    /// Moves on to the next segment of the trajectory. After the end of the
    /// trajectory, the player stays at the segment that holds the last
    /// position.
    pub fn build_next_segment(&mut self) {
        let segment = &self.state.segment;
        self.state = Self::state_at(
            &self.trajectory,
            self.state.start + self.state.length,
            segment.end_time_msec(),
            segment.end(),
        );
    }

    /// The current segment of the player.
    pub fn current_segment(&mut self) -> &mut TrajectorySegment {
        &mut self.state.segment
    }

    /// Whether the player is at a segment of the trajectory, as opposed to
    /// after its end.
    #[must_use]
    pub fn has_more_segments(&self) -> bool {
        self.state.length > 0
    }

    /// The position at the given time, in seconds.
    pub fn position_at(&mut self, t: f32) -> Vector3WithYaw {
        let rel_t = self.seek_to_time(t);
        self.state.segment.poly().eval(rel_t)
    }

    /// The velocity at the given time, in seconds.
    pub fn velocity_at(&mut self, t: f32) -> Vector3WithYaw {
        let rel_t = self.seek_to_time(t);
        self.state.segment.dpoly().eval(rel_t)
    }

    /// The acceleration at the given time, in seconds.
    pub fn acceleration_at(&mut self, t: f32) -> Vector3WithYaw {
        let rel_t = self.seek_to_time(t);
        self.state.segment.ddpoly().eval(rel_t)
    }

    /// The total duration of the trajectory, in milliseconds. Rewinds the
    /// player.
    pub fn total_duration_msec(&mut self) -> u32 {
        let mut result: u32 = 0;
        self.rewind();
        while self.has_more_segments() {
            result = result.saturating_add(self.state.segment.duration_msec());
            self.build_next_segment();
        }
        result
    }

    /// The current state of the player, to be restored later.
    #[must_use]
    pub fn save_state(&self) -> TrajectoryPlayerState {
        self.state
    }

    /// Restores a state saved earlier from a player of the same trajectory.
    pub fn restore_state(&mut self, state: &TrajectoryPlayerState) {
        self.state = *state;
    }

    /// Moves to the segment that contains the given time, and returns the
    /// relative time within the segment, between 0 and 1 inclusive.
    pub(crate) fn seek_to_time(&mut self, t: f32) -> f32 {
        let t = if t <= 0.0 { 0.0 } else { t };
        loop {
            let segment = &self.state.segment;
            if segment.start_time_sec() > t {
                // Before the current segment: start again from the beginning
                self.rewind();
            } else if segment.end_time_sec() < t {
                // After the end of the trajectory, the segment that holds the
                // last position ends at infinity, so this terminates
                self.build_next_segment();
            } else if !t.is_finite() {
                return 1.0;
            } else if segment.duration_sec().abs() > 1.0e-6 {
                return (t - segment.start_time_sec()) / segment.duration_sec();
            } else {
                return 0.5;
            }
        }
    }

    /// The state of a player at the segment at the given offset of the bytes
    /// of the trajectory.
    fn state_at(
        trajectory: &Trajectory<'_>,
        offset: usize,
        start_time_msec: u32,
        start: Vector3WithYaw,
    ) -> TrajectoryPlayerState {
        let decoded = if trajectory.scale() == 0.0 {
            None
        } else {
            trajectory.bytes().get(offset..).and_then(|bytes| {
                TrajectorySegment::decode(bytes, start_time_msec, start, trajectory.scale())
            })
        };
        // The bytes of a trajectory are validated, so a segment fails to
        // decode only at the end of the bytes
        match decoded {
            Some((segment, length)) => TrajectoryPlayerState {
                start: offset,
                length,
                segment,
            },
            None => TrajectoryPlayerState {
                start: offset,
                length: 0,
                segment: TrajectorySegment::hold(start_time_msec, start),
            },
        }
    }
}
