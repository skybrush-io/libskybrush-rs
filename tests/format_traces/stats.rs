//! Replays the `stats` section of a format trace: the statistics of the
//! trajectory with the default settings for show files.

use skybrush::trajectory::{Trajectory, TrajectoryStatsCalculator};

use crate::common::Replay;
use crate::common::values;

pub fn replay(trajectory: Option<&Trajectory<'_>>) -> Replay {
    let Some(trajectory) = trajectory else {
        return Replay::Absent;
    };
    let stats = TrajectoryStatsCalculator::new(1000.0)
        .run(trajectory)
        .expect("the default settings are valid");
    Replay::Present(values::stats(&stats))
}
