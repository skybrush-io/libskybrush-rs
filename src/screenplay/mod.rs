//! Screenplays: the scenes of a show, placed on the show clock, and the
//! return-to-home plan of the show.
//!
//! Scenes are placed on the show clock by their origins, independently of
//! each other, like clips in a sequencer: a scene covers its duration from
//! its origin, and the first scene that contains a time instant is the
//! active one. There may be gaps between scenes, and scenes may overlap.

mod scene;

use alloc::collections::TryReserveError;
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::{error, fmt, ptr};

pub use scene::{ScreenplayScene, ScreenplaySceneFlags};

use crate::events::EventListError;
use crate::formats::binary::{BinaryFile, BinaryFileError};
use crate::lights::LightProgramError;
use crate::rth_plan::{RthPlan, RthPlanError};
use crate::trajectory::TrajectoryError;
use crate::yaw_control::YawControlError;

/// Why a screenplay could not be read or modified.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ScreenplayError {
    /// The show file has no trajectory.
    MissingTrajectory,
    /// The duration of a scene is negative, NaN or too long.
    InvalidDuration,
    /// The show file could not be read.
    File(BinaryFileError),
    /// The trajectory of the show file could not be read.
    Trajectory(TrajectoryError),
    /// The light program of the show file could not be read.
    LightProgram(LightProgramError),
    /// The yaw control of the show file could not be read.
    YawControl(YawControlError),
    /// The events of the show file could not be read.
    EventList(EventListError),
    /// The RTH plan of the show file could not be read.
    RthPlan(RthPlanError),
    /// Memory could not be allocated.
    OutOfMemory,
}

impl fmt::Display for ScreenplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingTrajectory => f.write_str("the show file has no trajectory"),
            Self::InvalidDuration => f.write_str("the duration of the scene is out of range"),
            Self::File(_) => f.write_str("the show file could not be read"),
            Self::Trajectory(_) => f.write_str("the trajectory could not be read"),
            Self::LightProgram(_) => f.write_str("the light program could not be read"),
            Self::YawControl(_) => f.write_str("the yaw control could not be read"),
            Self::EventList(_) => f.write_str("the events could not be read"),
            Self::RthPlan(_) => f.write_str("the RTH plan could not be read"),
            Self::OutOfMemory => f.write_str("out of memory"),
        }
    }
}

impl error::Error for ScreenplayError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::File(err) => Some(err),
            Self::Trajectory(err) => Some(err),
            Self::LightProgram(err) => Some(err),
            Self::YawControl(err) => Some(err),
            Self::EventList(err) => Some(err),
            Self::RthPlan(err) => Some(err),
            Self::MissingTrajectory | Self::InvalidDuration | Self::OutOfMemory => None,
        }
    }
}

impl From<BinaryFileError> for ScreenplayError {
    fn from(err: BinaryFileError) -> Self {
        Self::File(err)
    }
}

impl From<TrajectoryError> for ScreenplayError {
    fn from(err: TrajectoryError) -> Self {
        Self::Trajectory(err)
    }
}

impl From<LightProgramError> for ScreenplayError {
    fn from(err: LightProgramError) -> Self {
        Self::LightProgram(err)
    }
}

impl From<YawControlError> for ScreenplayError {
    fn from(err: YawControlError) -> Self {
        Self::YawControl(err)
    }
}

impl From<EventListError> for ScreenplayError {
    fn from(err: EventListError) -> Self {
        Self::EventList(err)
    }
}

impl From<RthPlanError> for ScreenplayError {
    fn from(err: RthPlanError) -> Self {
        Self::RthPlan(err)
    }
}

impl From<TryReserveError> for ScreenplayError {
    fn from(_: TryReserveError) -> Self {
        Self::OutOfMemory
    }
}

/// The scenes of a show and its return-to-home plan.
#[derive(Clone, Debug, Default)]
pub struct Screenplay<'a> {
    scenes: Vec<ScreenplayScene<'a>>,
    rth_plan: Option<Rc<RthPlan<'a>>>,
}

impl<'a> Screenplay<'a> {
    /// A screenplay without scenes and without an RTH plan.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            scenes: Vec::new(),
            rth_plan: None,
        }
    }

    /// A screenplay with a single scene of infinite duration that plays the
    /// show file, and the RTH plan of the show file if it has one.
    ///
    /// # Errors
    ///
    /// Returns [`ScreenplayError::MissingTrajectory`] if the show file has
    /// no trajectory, and an error if a component cannot be read.
    pub fn from_binary_file(file: &BinaryFile<'a>) -> Result<Self, ScreenplayError> {
        let scene = ScreenplayScene::from_binary_file(file)?;
        let rth_plan = RthPlan::from_binary_file(file)?;
        let mut scenes = Vec::new();
        scenes.try_reserve_exact(1)?;
        scenes.push(scene);
        Ok(Self {
            scenes,
            rth_plan: rth_plan.map(Rc::new),
        })
    }

    /// The number of scenes that the screenplay can hold without allocating.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.scenes.capacity()
    }

    /// The number of scenes.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.scenes.len()
    }

    /// Whether the screenplay has no scenes.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.scenes.is_empty()
    }

    /// Whether the given scene is one of the scenes of the screenplay (not
    /// merely equal to one).
    #[must_use]
    pub fn contains_scene(&self, scene: &ScreenplayScene<'_>) -> bool {
        self.scenes.iter().any(|other| ptr::addr_eq(other, scene))
    }

    /// The scenes of the screenplay.
    #[must_use]
    pub fn scenes(&self) -> &[ScreenplayScene<'a>] {
        &self.scenes
    }

    /// The scene at the given index, or `None` if there is no such scene.
    #[must_use]
    pub fn scene(&self, index: usize) -> Option<&ScreenplayScene<'a>> {
        self.scenes.get(index)
    }

    /// The scene at the given index, to modify it, or `None` if there is no
    /// such scene.
    pub fn scene_mut(&mut self, index: usize) -> Option<&mut ScreenplayScene<'a>> {
        self.scenes.get_mut(index)
    }

    /// The first scene that contains the given time of the show clock, in
    /// milliseconds, and its index, or `None` if there is none.
    #[must_use]
    pub fn scene_at_time_msec(&self, time_msec: u32) -> Option<(usize, &ScreenplayScene<'a>)> {
        self.scenes
            .iter()
            .enumerate()
            .find(|(_, scene)| scene.contains_time_msec(time_msec))
    }

    /// The first scene with the given tag, or `None` if there is none.
    #[must_use]
    pub fn find_first_scene_with_tag(&self, tag: u8) -> Option<&ScreenplayScene<'a>> {
        self.scenes.iter().find(|scene| scene.tag() == tag)
    }

    /// The time of the show clock, in milliseconds, when the first scene
    /// with the given tag reaches the given warped time, or `None` if it
    /// never does: there is no such scene, the time is before its start or
    /// after its end, or an earlier scene is active then.
    #[must_use]
    pub fn time_msec_for_scene_tag_and_warped_time_in_scene(
        &self,
        tag: u8,
        warped_time_sec: f32,
    ) -> Option<u32> {
        if warped_time_sec.is_nan() {
            return None;
        }
        let index = self.scenes.iter().position(|scene| scene.tag() == tag)?;
        let scene = self.scenes.get(index)?;
        let origin_msec = scene.origin_msec();
        let time_msec = scene.time_axis().reverse_map(warped_time_sec);
        if time_msec < origin_msec || time_msec == i32::MAX {
            // Before the start of the scene, or never
            return None;
        }
        let elapsed_msec = i64::from(time_msec) - i64::from(origin_msec);
        if !scene.is_infinite() && elapsed_msec > i64::from(scene.duration_msec()) {
            // After the end of the scene
            return None;
        }
        // The end of the scene belongs to it here, but the lookup of scenes
        // considers it the start of the next one, so the earlier scenes are
        // checked explicitly
        let earlier = self.scenes.get(..index).unwrap_or_default();
        let time_msec = u32::try_from(time_msec).ok()?;
        if earlier
            .iter()
            .any(|other| other.contains_time_msec(time_msec))
        {
            return None;
        }
        Some(time_msec)
    }

    /// The time of the show clock, in seconds, when the first scene with the
    /// given tag reaches the given warped time, or infinity if it never does;
    /// see [`time_msec_for_scene_tag_and_warped_time_in_scene`](Self::time_msec_for_scene_tag_and_warped_time_in_scene).
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "times in seconds are floats, as in the C library"
    )]
    pub fn time_sec_for_scene_tag_and_warped_time_in_scene(
        &self,
        tag: u8,
        warped_time_sec: f32,
    ) -> f32 {
        self.time_msec_for_scene_tag_and_warped_time_in_scene(tag, warped_time_sec)
            .map_or(f32::INFINITY, |time_msec| time_msec as f32 / 1000.0)
    }

    /// Removes every scene and the RTH plan.
    pub fn clear(&mut self) {
        self.scenes.clear();
        self.rth_plan = None;
    }

    /// Appends an empty scene of infinite duration and returns it.
    ///
    /// # Errors
    ///
    /// Returns [`ScreenplayError::OutOfMemory`] if memory could not be
    /// allocated; the screenplay is left intact then.
    pub fn append_new_scene(&mut self) -> Result<&mut ScreenplayScene<'a>, ScreenplayError> {
        self.scenes.try_reserve(1)?;
        self.scenes.push(ScreenplayScene::new());
        self.scenes.last_mut().ok_or(ScreenplayError::OutOfMemory)
    }

    /// Removes the last scene and returns it, or returns `None` if there are
    /// no scenes.
    pub fn remove_last_scene(&mut self) -> Option<ScreenplayScene<'a>> {
        self.scenes.pop()
    }

    /// The RTH plan of the show.
    #[must_use]
    pub const fn rth_plan(&self) -> Option<&Rc<RthPlan<'a>>> {
        self.rth_plan.as_ref()
    }

    /// Sets the RTH plan of the show.
    pub fn set_rth_plan(&mut self, rth_plan: Option<Rc<RthPlan<'a>>>) {
        self.rth_plan = rth_plan;
    }
}
