//! The show controller: what the drone should do at a time instant of a
//! screenplay, and which events it should fire.

use crate::flags::impl_flags;
use alloc::rc::Rc;
use core::{mem, ptr};

use crate::basic_types::Vector3;
use crate::colors::RgbColor;
use crate::events::{Event, EventList, EventListPlayer};
use crate::lights::{LightPlayer, LightProgram};
use crate::screenplay::{Screenplay, ScreenplayScene};
use crate::trajectory::{Trajectory, TrajectoryPlayer};
use crate::yaw_control::{YawControl, YawPlayer};

/// The longest warped time that is passed to the light player, in seconds.
const MAX_LIGHT_TIME_SEC: f32 = 86_400.0;

/// The components of a control output: a set of flags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ControlComponents(u8);

impl ControlComponents {
    /// No components.
    pub const NONE: Self = Self(0);
    /// The position.
    pub const POSITION: Self = Self(1);
    /// The velocity.
    pub const VELOCITY: Self = Self(2);
    /// The color of the lights.
    pub const LIGHTS: Self = Self(4);
    /// The yaw.
    pub const YAW: Self = Self(8);
    /// The yaw rate.
    pub const YAW_RATE: Self = Self(16);
    /// All components.
    pub const ALL: Self = Self(31);
}

impl_flags!(ControlComponents);

/// What the drone should do: some of a position, a velocity, a color, a yaw
/// and a yaw rate. Positions are in millimeters, yaws in degrees.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ControlOutput {
    mask: ControlComponents,
    position: Vector3,
    velocity: Vector3,
    color: RgbColor,
    yaw: f32,
    yaw_rate: f32,
}

impl ControlOutput {
    /// An output without components.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            mask: ControlComponents::NONE,
            position: Vector3::ZERO,
            velocity: Vector3::ZERO,
            color: RgbColor::BLACK,
            yaw: 0.0,
            yaw_rate: 0.0,
        }
    }

    /// The components that are set.
    #[must_use]
    pub const fn components(&self) -> ControlComponents {
        self.mask
    }

    /// Removes every component.
    pub const fn clear(&mut self) {
        self.mask = ControlComponents::NONE;
    }

    /// Removes the given components.
    pub const fn clear_components(&mut self, components: ControlComponents) {
        self.mask.remove(components);
    }

    /// Whether any of the given components is set.
    #[must_use]
    pub const fn has_any_component_in(&self, components: ControlComponents) -> bool {
        self.mask.intersects(components)
    }

    /// Whether all the given components are set.
    #[must_use]
    pub const fn has_all_components_in(&self, components: ControlComponents) -> bool {
        self.mask.contains(components)
    }

    /// The position, if it is set.
    #[must_use]
    pub const fn position(&self) -> Option<Vector3> {
        self.component(ControlComponents::POSITION, self.position)
    }

    /// The velocity, if it is set.
    #[must_use]
    pub const fn velocity(&self) -> Option<Vector3> {
        self.component(ControlComponents::VELOCITY, self.velocity)
    }

    /// The color, if it is set.
    #[must_use]
    pub const fn color(&self) -> Option<RgbColor> {
        self.component(ControlComponents::LIGHTS, self.color)
    }

    /// The yaw, if it is set.
    #[must_use]
    pub const fn yaw(&self) -> Option<f32> {
        self.component(ControlComponents::YAW, self.yaw)
    }

    /// The yaw rate, if it is set.
    #[must_use]
    pub const fn yaw_rate(&self) -> Option<f32> {
        self.component(ControlComponents::YAW_RATE, self.yaw_rate)
    }

    /// Sets the position.
    pub const fn set_position(&mut self, position: Vector3) {
        self.position = position;
        self.mask.insert(ControlComponents::POSITION);
    }

    /// Sets the velocity.
    pub const fn set_velocity(&mut self, velocity: Vector3) {
        self.velocity = velocity;
        self.mask.insert(ControlComponents::VELOCITY);
    }

    /// Sets the color.
    pub const fn set_color(&mut self, color: RgbColor) {
        self.color = color;
        self.mask.insert(ControlComponents::LIGHTS);
    }

    /// Sets the yaw.
    pub const fn set_yaw(&mut self, yaw: f32) {
        self.yaw = yaw;
        self.mask.insert(ControlComponents::YAW);
    }

    /// Sets the yaw rate.
    pub const fn set_yaw_rate(&mut self, yaw_rate: f32) {
        self.yaw_rate = yaw_rate;
        self.mask.insert(ControlComponents::YAW_RATE);
    }

    /// The given value if the given component is set.
    const fn component<T: Copy>(&self, component: ControlComponents, value: T) -> Option<T> {
        if self.has_any_component_in(component) {
            Some(value)
        } else {
            None
        }
    }
}

/// The time instant that a control output belongs to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ControlOutputTime {
    /// The time on the show clock, in milliseconds.
    pub time_msec: u32,
    /// The index of the scene, or `None` if no scene was active.
    pub scene: Option<usize>,
    /// The time since the origin of the scene, in milliseconds; zero if no
    /// scene was active.
    pub time_in_scene_msec: u32,
    /// The warped time of the scene, in seconds; zero if no scene was active.
    pub warped_time_in_scene_sec: f32,
}

/// The players of the components of the current scene. Each player holds
/// its component through an `Rc`, so it does not borrow the screenplay.
#[derive(Debug)]
struct Players<'a> {
    trajectory: Option<TrajectoryPlayer<Rc<Trajectory<'a>>>>,
    light: Option<LightPlayer<Rc<LightProgram<'a>>>>,
    yaw: Option<YawPlayer<Rc<YawControl<'a>>>>,
    events: Option<EventListPlayer<Rc<EventList>>>,
}

impl<'a> Players<'a> {
    const fn none() -> Self {
        Self {
            trajectory: None,
            light: None,
            yaw: None,
            events: None,
        }
    }

    /// Makes the players play the components of the given scene. A player
    /// whose component did not change keeps its state, e.g. the event list
    /// player does not fire the events again.
    fn sync(&mut self, scene: &ScreenplayScene<'a>) {
        sync_player(
            &mut self.trajectory,
            scene.trajectory(),
            TrajectoryPlayer::trajectory,
            TrajectoryPlayer::new,
        );
        sync_player(
            &mut self.light,
            scene.light_program(),
            LightPlayer::program,
            LightPlayer::new,
        );
        sync_player(
            &mut self.yaw,
            scene.yaw_control(),
            YawPlayer::yaw_control,
            YawPlayer::new,
        );
        sync_player(
            &mut self.events,
            scene.events(),
            EventListPlayer::events,
            EventListPlayer::new,
        );
    }
}

/// Replaces a player unless it plays the given component already.
fn sync_player<P, C>(
    player: &mut Option<P>,
    component: Option<&Rc<C>>,
    played: impl Fn(&P) -> &C,
    new: impl FnOnce(Rc<C>) -> P,
) {
    let up_to_date = match (player.as_ref(), component) {
        // The player reaches its component through the same Rc
        (Some(player), Some(component)) => ptr::eq(played(player), Rc::as_ptr(component)),
        (None, None) => true,
        _ => false,
    };
    if !up_to_date {
        *player = component.map(|component| new(Rc::clone(component)));
    }
}

/// Plays a screenplay: computes what the drone should do at a time instant,
/// and fires the events of the scenes as the time moves forward.
///
/// The controller owns the screenplay. It may be modified during the show
/// through [`screenplay_mut`](Self::screenplay_mut); the players of the
/// current scene keep their state across modifications unless the scene or
/// its components change.
///
/// # Examples
///
/// ```
/// use std::rc::Rc;
///
/// use skybrush::control::ShowController;
/// use skybrush::screenplay::Screenplay;
/// use skybrush::time_axis::TimeSegment;
/// # use skybrush::basic_types::Vector3WithYaw;
/// # use skybrush::trajectory::TrajectoryBuilder;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// # let mut builder = TrajectoryBuilder::new(1)?;
/// # builder.set_start_position(Vector3WithYaw::default())?;
/// # builder.append_line(Vector3WithYaw { x: 10_000.0, ..Vector3WithYaw::default() }, 10_000)?;
/// # let trajectory = builder.build()?;
/// // A screenplay with a single scene that plays a trajectory
/// let mut screenplay = Screenplay::new();
/// let scene = screenplay.append_new_scene()?;
/// scene.set_trajectory(Some(Rc::new(trajectory)));
/// let mut controller = ShowController::new(screenplay);
/// controller.update_time_msec(1000);
///
/// // Pause the show 2 s in: real time until then, stopped time afterwards
/// let scene = controller.screenplay_mut().scene_mut(0).ok_or("no scene")?;
/// scene.time_axis_mut().append_segment(TimeSegment::realtime(2000))?;
/// scene.time_axis_mut().append_segment(TimeSegment::constant_rate(u32::MAX, 0.0))?;
///
/// controller.update_time_msec(5000);
/// let output_time = controller.current_output_time().ok_or("no valid output")?;
/// assert_eq!(output_time.warped_time_in_scene_sec, 2.0);
/// assert_eq!(controller.current_output().velocity().map(|v| v.x), Some(0.0));
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct ShowController<'a> {
    screenplay: Screenplay<'a>,
    /// The index of the current scene.
    current_scene: Option<usize>,
    players: Players<'a>,
    /// The output outside the scenes: zero velocity and zero yaw rate.
    default_output: ControlOutput,
    output: ControlOutput,
    output_time: Option<ControlOutputTime>,
    reached_end: bool,
}

impl<'a> ShowController<'a> {
    /// A controller of the given screenplay. Its output is not valid until
    /// the first update.
    #[must_use]
    pub fn new(screenplay: Screenplay<'a>) -> Self {
        let mut default_output = ControlOutput::new();
        default_output.set_velocity(Vector3::ZERO);
        default_output.set_yaw_rate(0.0);
        Self {
            screenplay,
            current_scene: None,
            players: Players::none(),
            default_output,
            output: default_output,
            output_time: None,
            reached_end: false,
        }
    }

    /// The screenplay being played.
    #[must_use]
    pub const fn screenplay(&self) -> &Screenplay<'a> {
        &self.screenplay
    }

    /// The screenplay being played, to modify it. The output becomes
    /// invalid until the next update.
    pub fn screenplay_mut(&mut self) -> &mut Screenplay<'a> {
        self.invalidate_output();
        &mut self.screenplay
    }

    /// Replaces the screenplay being played, and returns the old one. The
    /// output becomes invalid until the next update.
    ///
    /// The players of the current scene keep their state if the new
    /// screenplay has a scene at the same index with the same components,
    /// e.g. one made with [`ScreenplayScene::update_contents_from`]; the
    /// events of that scene are then not fired again.
    pub fn set_screenplay(&mut self, screenplay: Screenplay<'a>) -> Screenplay<'a> {
        mem::replace(self.screenplay_mut(), screenplay)
    }

    /// The screenplay, taken back from the controller.
    #[must_use]
    pub fn into_screenplay(self) -> Screenplay<'a> {
        self.screenplay
    }

    /// The current scene, or `None` if no scene was active at the last
    /// update.
    #[must_use]
    pub fn current_scene(&self) -> Option<&ScreenplayScene<'a>> {
        self.screenplay.scene(self.current_scene?)
    }

    /// The output at the time of the last update.
    #[must_use]
    pub const fn current_output(&self) -> &ControlOutput {
        &self.output
    }

    /// The time that the current output belongs to, or `None` if the output
    /// is not valid: before the first update, and after the screenplay was
    /// changed.
    #[must_use]
    pub const fn current_output_time(&self) -> Option<ControlOutputTime> {
        self.output_time
    }

    /// Whether the last update was after the end of the screenplay or
    /// between its scenes.
    #[must_use]
    pub const fn has_reached_end(&self) -> bool {
        self.reached_end
    }

    /// Whether the current output belongs to a time instant.
    #[must_use]
    pub const fn is_output_valid(&self) -> bool {
        self.output_time.is_some()
    }

    /// Computes the output at the given time of the show clock, in
    /// milliseconds. Outside the scenes, the output is zero velocity and zero
    /// yaw rate.
    pub fn update_time_msec(&mut self, time_msec: u32) {
        if self
            .output_time
            .is_some_and(|output_time| output_time.time_msec == time_msec)
        {
            // The output is up to date
            return;
        }

        // Time axes take an i32; later times are handled like times between
        // scenes
        let found = i32::try_from(time_msec)
            .ok()
            .and_then(|wall_clock_time_msec| {
                let (index, scene) = self.screenplay.scene_at_time_msec(time_msec)?;
                Some((index, scene, wall_clock_time_msec))
            });
        let scene_index = found.map(|(index, _, _)| index);
        if scene_index != self.current_scene {
            // The players of another scene start from scratch
            self.players = Players::none();
            self.current_scene = scene_index;
        }
        // Invalidated field by field, since the scene borrows the screenplay
        self.output = self.default_output;
        self.output_time = None;
        self.reached_end = false;

        let Some((_, scene, wall_clock_time_msec)) = found else {
            // After the end of the show, or between scenes
            self.output = self.default_output;
            self.reached_end = true;
            self.output_time = Some(ControlOutputTime {
                time_msec,
                scene: None,
                time_in_scene_msec: 0,
                warped_time_in_scene_sec: 0.0,
            });
            return;
        };
        self.players.sync(scene);

        let time_in_scene_msec = time_msec.wrapping_sub_signed(scene.origin_msec());
        let (warped_time_sec, warped_rate) = scene.time_axis().map_with_rate(wall_clock_time_msec);

        let output = &mut self.output;
        output.clear();
        if let Some(player) = &mut self.players.trajectory {
            output.set_position(player.position_at(warped_time_sec).xyz());
            output.set_velocity(player.velocity_at(warped_time_sec).xyz() * warped_rate);
        }
        if let Some(player) = &mut self.players.light {
            let light_time_sec = if warped_time_sec < 0.0 || !warped_time_sec.is_finite() {
                0.0
            } else {
                warped_time_sec.min(MAX_LIGHT_TIME_SEC)
            };
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a time between zero and a day, truncated as in the C library"
            )]
            let light_time_msec = (light_time_sec * 1000.0) as u64;
            output.set_color(player.color_at(light_time_msec));
        }
        if let Some(player) = &mut self.players.yaw {
            output.set_yaw(player.yaw_at(warped_time_sec));
            output.set_yaw_rate(player.yaw_rate_at(warped_time_sec) * warped_rate);
        }

        self.output_time = Some(ControlOutputTime {
            time_msec,
            scene: scene_index,
            time_in_scene_msec,
            warped_time_in_scene_sec: warped_time_sec,
        });
    }

    /// Fires the next event of the current scene that is due at the time of
    /// the last update, and returns it. Call it until it returns `None` to
    /// fire every event that is due.
    pub fn next_event(&mut self) -> Option<&Event> {
        let time_sec = self
            .output_time
            .map_or(0.0, |output_time| output_time.warped_time_in_scene_sec);
        self.players
            .events
            .as_mut()?
            .next_event_not_later_than(time_sec)
    }

    fn invalidate_output(&mut self) {
        self.output = self.default_output;
        self.output_time = None;
        self.reached_end = false;
    }
}
