//! Scenes of a screenplay.

use alloc::rc::Rc;
use core::ops::BitOr;

use super::ScreenplayError;
use crate::events::EventList;
use crate::formats::binary::BinaryFile;
use crate::lights::LightProgram;
use crate::time_axis::{TimeAxis, TimeAxisError};
use crate::trajectory::Trajectory;
use crate::yaw_control::YawControl;

/// The flags of a scene; bits without a meaning are kept.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ScreenplaySceneFlags(pub u8);

impl ScreenplaySceneFlags {
    /// No flags.
    pub const NONE: Self = Self(0);
    /// The duration of the scene is determined by its trajectory.
    pub const DYNAMIC_DURATION: Self = Self(0x01);
    /// Every flag, as a mask.
    pub const ALL: Self = Self(0xff);
}

impl BitOr for ScreenplaySceneFlags {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// A scene of a screenplay: a trajectory, a light program, a yaw control
/// and events, played on the show clock from the origin of the time axis of
/// the scene for its duration.
///
/// The components are reference counted, so that scenes can share them.
#[derive(Clone, Debug)]
pub struct ScreenplayScene<'a> {
    /// The duration on the show clock, in milliseconds; `u32::MAX` means
    /// infinite.
    duration_msec: u32,
    time_axis: TimeAxis,
    trajectory: Option<Rc<Trajectory<'a>>>,
    light_program: Option<Rc<LightProgram<'a>>>,
    yaw_control: Option<Rc<YawControl<'a>>>,
    events: Option<Rc<EventList>>,
    tag: u8,
    flags: ScreenplaySceneFlags,
}

impl Default for ScreenplayScene<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> ScreenplayScene<'a> {
    /// An empty scene of infinite duration at the origin of the show clock.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            duration_msec: u32::MAX,
            time_axis: TimeAxis::new(),
            trajectory: None,
            light_program: None,
            yaw_control: None,
            events: None,
            tag: 0,
            flags: ScreenplaySceneFlags::NONE,
        }
    }

    /// A scene of infinite duration with the components of a show file,
    /// borrowing their bytes from the show file where they are not modified.
    ///
    /// # Errors
    ///
    /// Returns [`ScreenplayError::MissingTrajectory`] if the show file has
    /// no trajectory, and an error if a component cannot be read.
    pub fn from_binary_file(file: &BinaryFile<'a>) -> Result<Self, ScreenplayError> {
        let trajectory =
            Trajectory::from_binary_file(file)?.ok_or(ScreenplayError::MissingTrajectory)?;
        let light_program = LightProgram::from_binary_file(file)?;
        let yaw_control = YawControl::from_binary_file(file)?;
        let events = EventList::from_binary_file(file)?;
        let mut scene = Self::new();
        scene.trajectory = Some(Rc::new(trajectory));
        scene.light_program = light_program.map(Rc::new);
        scene.yaw_control = yaw_control.map(Rc::new);
        scene.events = events.map(Rc::new);
        Ok(scene)
    }

    /// Whether the scene contains the given time of the show clock, in
    /// milliseconds: the scene covers its duration from its origin, without
    /// its end.
    #[must_use]
    pub fn contains_time_msec(&self, time_msec: u32) -> bool {
        let elapsed_msec = i64::from(time_msec) - i64::from(self.origin_msec());
        elapsed_msec >= 0 && (self.is_infinite() || i64::from(self.duration_msec) > elapsed_msec)
    }

    /// The duration on the show clock, in milliseconds; `u32::MAX` if it is
    /// infinite.
    #[must_use]
    pub const fn duration_msec(&self) -> u32 {
        self.duration_msec
    }

    /// The duration on the show clock, in seconds.
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "times in seconds are floats, as in the C library"
    )]
    pub fn duration_sec(&self) -> f32 {
        if self.is_infinite() {
            f32::INFINITY
        } else {
            self.duration_msec as f32 / 1000.0
        }
    }

    /// Whether the duration is infinite.
    #[must_use]
    pub const fn is_infinite(&self) -> bool {
        self.duration_msec == u32::MAX
    }

    /// Sets the duration on the show clock, in milliseconds; `u32::MAX`
    /// means infinite.
    pub const fn set_duration_msec(&mut self, duration_msec: u32) {
        self.duration_msec = duration_msec;
    }

    /// Makes the duration infinite.
    pub const fn set_infinite(&mut self) {
        self.duration_msec = u32::MAX;
    }

    /// Sets the duration on the show clock, in seconds, rounded to
    /// milliseconds; positive infinity makes it infinite.
    ///
    /// # Errors
    ///
    /// Returns [`ScreenplayError::InvalidDuration`] if the duration is
    /// negative, NaN, negative infinity, or too long for 32-bit
    /// milliseconds; the duration is left intact then.
    pub fn set_duration_sec(&mut self, duration_sec: f32) -> Result<(), ScreenplayError> {
        // The largest u32 that is a float
        const MAX_DURATION_MSEC: f32 = 4_294_967_040.0;
        if duration_sec == f32::INFINITY {
            self.set_infinite();
            return Ok(());
        }
        if duration_sec < 0.0 || !duration_sec.is_finite() {
            return Err(ScreenplayError::InvalidDuration);
        }
        let duration_msec = duration_sec * 1000.0;
        if duration_msec > MAX_DURATION_MSEC {
            return Err(ScreenplayError::InvalidDuration);
        }
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a non-negative float below u32::MAX, rounded as in the C library"
        )]
        let duration_msec = (duration_msec + 0.5) as u32;
        if duration_msec == u32::MAX {
            return Err(ScreenplayError::InvalidDuration);
        }
        self.duration_msec = duration_msec;
        Ok(())
    }

    /// The origin of the scene on the show clock, in milliseconds: the
    /// origin of its time axis.
    #[must_use]
    pub const fn origin_msec(&self) -> i32 {
        self.time_axis.origin_msec()
    }

    /// The origin of the scene on the show clock, in seconds.
    #[must_use]
    pub fn origin_sec(&self) -> f32 {
        self.time_axis.origin_sec()
    }

    /// Sets the origin of the scene, in milliseconds.
    ///
    /// # Errors
    ///
    /// Returns an error if the origin is negative; see
    /// [`TimeAxis::set_origin_msec`].
    pub const fn set_origin_msec(&mut self, origin_msec: i32) -> Result<(), TimeAxisError> {
        self.time_axis.set_origin_msec(origin_msec)
    }

    /// Sets the origin of the scene, in seconds.
    ///
    /// # Errors
    ///
    /// Returns an error if the origin is out of range; see
    /// [`TimeAxis::set_origin_sec`].
    pub fn set_origin_sec(&mut self, origin_sec: f32) -> Result<(), TimeAxisError> {
        self.time_axis.set_origin_sec(origin_sec)
    }

    /// The time axis of the scene.
    #[must_use]
    pub const fn time_axis(&self) -> &TimeAxis {
        &self.time_axis
    }

    /// The time axis of the scene, to modify it.
    pub const fn time_axis_mut(&mut self) -> &mut TimeAxis {
        &mut self.time_axis
    }

    /// The trajectory of the scene.
    #[must_use]
    pub const fn trajectory(&self) -> Option<&Rc<Trajectory<'a>>> {
        self.trajectory.as_ref()
    }

    /// The light program of the scene.
    #[must_use]
    pub const fn light_program(&self) -> Option<&Rc<LightProgram<'a>>> {
        self.light_program.as_ref()
    }

    /// The yaw control of the scene.
    #[must_use]
    pub const fn yaw_control(&self) -> Option<&Rc<YawControl<'a>>> {
        self.yaw_control.as_ref()
    }

    /// The events of the scene.
    #[must_use]
    pub const fn events(&self) -> Option<&Rc<EventList>> {
        self.events.as_ref()
    }

    /// Sets the trajectory of the scene.
    pub fn set_trajectory(&mut self, trajectory: Option<Rc<Trajectory<'a>>>) {
        self.trajectory = trajectory;
    }

    /// Sets the light program of the scene.
    pub fn set_light_program(&mut self, light_program: Option<Rc<LightProgram<'a>>>) {
        self.light_program = light_program;
    }

    /// Sets the yaw control of the scene.
    pub fn set_yaw_control(&mut self, yaw_control: Option<Rc<YawControl<'a>>>) {
        self.yaw_control = yaw_control;
    }

    /// Sets the events of the scene.
    pub fn set_events(&mut self, events: Option<Rc<EventList>>) {
        self.events = events;
    }

    /// The tag of the scene, which identifies it for the application.
    #[must_use]
    pub const fn tag(&self) -> u8 {
        self.tag
    }

    /// Sets the tag of the scene.
    pub const fn set_tag(&mut self, tag: u8) {
        self.tag = tag;
    }

    /// The flags of the scene.
    #[must_use]
    pub const fn flags(&self) -> ScreenplaySceneFlags {
        self.flags
    }

    /// Whether the scene has any of the flags of the given mask.
    #[must_use]
    pub const fn has_any_flag(&self, mask: ScreenplaySceneFlags) -> bool {
        self.flags.0 & mask.0 != 0
    }

    /// Whether the scene has every flag of the given mask.
    #[must_use]
    pub const fn has_all_flags(&self, mask: ScreenplaySceneFlags) -> bool {
        self.flags.0 & mask.0 == mask.0
    }

    /// Whether the scene has the given flag.
    #[must_use]
    pub const fn has_flag(&self, flag: ScreenplaySceneFlags) -> bool {
        self.has_any_flag(flag)
    }

    /// Sets the given flag.
    pub const fn set_flag(&mut self, flag: ScreenplaySceneFlags) {
        self.flags.0 |= flag.0;
    }

    /// Clears the given flag.
    pub const fn clear_flag(&mut self, flag: ScreenplaySceneFlags) {
        self.flags.0 &= !flag.0;
    }

    /// Sets the flags of the given mask to the given values; the other flags
    /// are kept.
    pub const fn set_flags(&mut self, mask: ScreenplaySceneFlags, flags: ScreenplaySceneFlags) {
        self.flags.0 = (self.flags.0 & !mask.0) | (flags.0 & mask.0);
    }

    /// How much of the trajectory, in warped time, is left after the end of
    /// the time axis, in seconds; zero if there is no trajectory or the time
    /// axis is infinite.
    #[must_use]
    pub fn warped_time_remaining_from_trajectory_at_end_of_time_axis(&self) -> f32 {
        let Some(trajectory) = &self.trajectory else {
            return 0.0;
        };
        let warped_duration_sec = self.time_axis.total_warped_duration_sec();
        if !warped_duration_sec.is_finite() {
            // The trajectory is played to its end
            return 0.0;
        }
        let mut result = trajectory.total_duration_sec() - warped_duration_sec;
        let origin_sec = self.origin_sec();
        if origin_sec < 0.0 {
            result += origin_sec;
        }
        if result > 0.0 { result } else { 0.0 }
    }

    /// How much of the trajectory is not covered by the time axis, in warped
    /// seconds; zero if there is no trajectory or the time axis is infinite,
    /// and negative if the time axis is longer than the trajectory. `None` if
    /// the warped duration of the time axis is NaN or negative infinity.
    #[must_use]
    pub fn uncovered_trajectory_duration_sec(&self) -> Option<f32> {
        let warped_duration_sec = self.time_axis.total_warped_duration_sec();
        if !warped_duration_sec.is_finite() {
            return (warped_duration_sec == f32::INFINITY).then_some(0.0);
        }
        Some(self.trajectory.as_ref().map_or(0.0, |trajectory| {
            trajectory.total_duration_sec() - warped_duration_sec
        }))
    }

    /// Removes the components of the scene; the duration, the time axis, the
    /// tag and the flags are kept.
    pub fn clear_contents(&mut self) {
        self.trajectory = None;
        self.light_program = None;
        self.yaw_control = None;
        self.events = None;
    }

    /// Shares the components of the given scene with this one.
    pub fn update_contents_from(&mut self, src: &Self) {
        self.trajectory.clone_from(&src.trajectory);
        self.light_program.clone_from(&src.light_program);
        self.yaw_control.clone_from(&src.yaw_control);
        self.events.clone_from(&src.events);
    }

    /// Resets the scene to an empty scene of infinite duration, without time
    /// axis segments, tag and flags. The origin is kept, as in the C
    /// library.
    pub fn reset(&mut self) {
        self.clear_contents();
        self.set_infinite();
        self.tag = 0;
        self.flags = ScreenplaySceneFlags::NONE;
        self.time_axis.clear();
    }
}
