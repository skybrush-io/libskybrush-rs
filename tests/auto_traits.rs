//! Which types can be sent to or shared with other threads. The
//! documentation of the crate (see `src/lib.rs` and
//! `doc/rust-api-deviations.md`) relies on these.

use std::rc::Rc;

use skybrush::control::ShowController;
use skybrush::events::{EventList, EventListPlayer};
use skybrush::formats::binary::BinaryFile;
use skybrush::lights::{LightPlayer, LightProgram};
use skybrush::rth_plan::RthPlan;
use skybrush::screenplay::{Screenplay, ScreenplayScene};
use skybrush::time_axis::TimeAxis;
use skybrush::trajectory::{Trajectory, TrajectoryBuilder, TrajectoryPlayer, TrajectorySegment};
use skybrush::yaw_control::{YawControl, YawPlayer};

const fn send_and_sync<T: Send + Sync>() {}
const fn send<T: Send>() {}

/// Fails to compile if `$type` implements the auto trait `$trait`.
macro_rules! assert_not_impl {
    ($type:ty: $trait:path) => {
        const _: () = {
            trait AmbiguousIfImpl<A> {
                fn some_item() {}
            }
            impl<T: ?Sized> AmbiguousIfImpl<()> for T {}
            struct Invalid;
            impl<T: ?Sized + $trait> AmbiguousIfImpl<Invalid> for T {}
            let _ = <$type as AmbiguousIfImpl<_>>::some_item;
        };
    };
}

// The data read from a show file can be shared freely
const _: () = {
    send_and_sync::<BinaryFile<'static>>();
    send_and_sync::<Trajectory<'static>>();
    send_and_sync::<LightProgram<'static>>();
    send_and_sync::<YawControl<'static>>();
    send_and_sync::<EventList>();
    send_and_sync::<RthPlan<'static>>();
    send_and_sync::<TimeAxis>();
    send_and_sync::<TrajectoryBuilder>();
};

// Players can be sent, but trajectory segments cache their polynomials in a
// `OnceCell`, so they and the players of trajectories are not `Sync`
const _: () = {
    send::<TrajectoryPlayer<&'static Trajectory<'static>>>();
    send::<LightPlayer<&'static LightProgram<'static>>>();
    send::<YawPlayer<&'static YawControl<'static>>>();
    send::<EventListPlayer<&'static EventList>>();
};
assert_not_impl!(TrajectorySegment: Sync);
assert_not_impl!(TrajectoryPlayer<&'static Trajectory<'static>>: Sync);

// Scenes share their components through `Rc`, so screenplays and the show
// controller stay on the thread that created them
assert_not_impl!(ScreenplayScene<'static>: Send);
assert_not_impl!(Screenplay<'static>: Send);
assert_not_impl!(ShowController<'static>: Send);
assert_not_impl!(TrajectoryPlayer<Rc<Trajectory<'static>>>: Send);
