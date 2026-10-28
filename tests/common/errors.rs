//! Names of the errors of the crate, as the oracle records them.
//!
//! The oracle records errors by the names of the error codes of the C
//! library. Each error enum of the crate maps every variant to one name; the
//! matches are exhaustive, so a new variant does not compile until it is
//! mapped here.

use skybrush::events::EventListError;
use skybrush::formats::binary::BinaryFileError;
use skybrush::gcs_light_control::GcsLightControlSetupError;
use skybrush::lights::LightProgramError;
use skybrush::metadata::ShowMetadataError;
use skybrush::rth_plan::{RthPlanError, RthTrajectoryError};
use skybrush::screenplay::ScreenplayError;
use skybrush::time_axis::TimeAxisError;
use skybrush::trajectory::{
    LandingError, TrajectoryBuilderError, TrajectoryError, TrajectoryStatsError,
};
use skybrush::yaw_control::YawControlError;

/// The name of the C error code that corresponds to an error.
pub trait ErrorName {
    fn error_name(&self) -> &'static str;
}

impl ErrorName for BinaryFileError {
    fn error_name(&self) -> &'static str {
        match self {
            Self::InvalidSignature | Self::UnsupportedVersion(_) | Self::TruncatedHeader => {
                "EPARSE"
            }
            Self::ChecksumMismatch { .. } => "ECORRUPTED",
            Self::TruncatedBlockHeader | Self::TruncatedBlock => "EREAD",
        }
    }
}

impl ErrorName for TrajectoryError {
    fn error_name(&self) -> &'static str {
        match self {
            Self::Corrupted => "ECORRUPTED",
            Self::File(err) => err.error_name(),
        }
    }
}

impl ErrorName for TrajectoryStatsError {
    fn error_name(&self) -> &'static str {
        match self {
            Self::InvalidAcceleration
            | Self::InvalidTakeoffSpeed
            | Self::InvalidMinAscent
            | Self::InvalidPreferredDescent => "EINVAL",
        }
    }
}

impl ErrorName for TrajectoryBuilderError {
    fn error_name(&self) -> &'static str {
        match self {
            Self::InvalidScale
            | Self::ZeroDuration
            | Self::DurationTooLong
            | Self::CoordinateOutOfRange => "EINVAL",
            Self::StartPositionAlreadyUsed => "FAILURE",
            Self::OutOfMemory => "ENOMEM",
        }
    }
}

impl ErrorName for LandingError {
    fn error_name(&self) -> &'static str {
        match self {
            Self::MissingLandingTime | Self::InvalidVelocity | Self::NotBelow => "EINVAL",
            Self::Builder(err) => err.error_name(),
        }
    }
}

impl ErrorName for LightProgramError {
    fn error_name(&self) -> &'static str {
        match self {
            Self::File(err) => err.error_name(),
            Self::OutOfMemory => "ENOMEM",
        }
    }
}

impl ErrorName for EventListError {
    fn error_name(&self) -> &'static str {
        match self {
            Self::Corrupted => "ECORRUPTED",
            Self::OutOfOrder => "EINVAL",
            Self::File(err) => err.error_name(),
            Self::OutOfMemory => "ENOMEM",
        }
    }
}

impl ErrorName for YawControlError {
    fn error_name(&self) -> &'static str {
        match self {
            Self::Corrupted => "ECORRUPTED",
            Self::File(err) => err.error_name(),
            Self::OutOfMemory => "ENOMEM",
        }
    }
}

impl ErrorName for ShowMetadataError {
    fn error_name(&self) -> &'static str {
        match self {
            Self::Corrupted => "ECORRUPTED",
            Self::File(err) => err.error_name(),
        }
    }
}

impl ErrorName for GcsLightControlSetupError {
    fn error_name(&self) -> &'static str {
        match self {
            Self::Corrupted => "ECORRUPTED",
            Self::File(err) => err.error_name(),
        }
    }
}

impl ErrorName for RthPlanError {
    fn error_name(&self) -> &'static str {
        match self {
            Self::Corrupted => "ECORRUPTED",
            Self::File(err) => err.error_name(),
            Self::VarintOverflow | Self::TimeOverflow | Self::DurationTooLong => "EOVERFLOW",
            Self::MissingPoint => "EINVAL",
        }
    }
}

impl ErrorName for RthTrajectoryError {
    fn error_name(&self) -> &'static str {
        match self {
            Self::ScaleOverflow | Self::DurationOverflow => "EOVERFLOW",
            Self::InvalidDuration => "EINVAL",
            Self::Builder(err) => err.error_name(),
            Self::Trajectory(err) => err.error_name(),
        }
    }
}

impl ErrorName for TimeAxisError {
    fn error_name(&self) -> &'static str {
        match self {
            Self::InvalidSegment | Self::IndexOutOfRange | Self::InvalidOrigin => "EINVAL",
            Self::OutOfMemory => "ENOMEM",
        }
    }
}

impl ErrorName for ScreenplayError {
    fn error_name(&self) -> &'static str {
        match self {
            Self::MissingTrajectory => "ENOENT",
            Self::InvalidDuration => "EINVAL",
            Self::File(err) => err.error_name(),
            Self::Trajectory(err) => err.error_name(),
            Self::LightProgram(err) => err.error_name(),
            Self::YawControl(err) => err.error_name(),
            Self::EventList(err) => err.error_name(),
            Self::RthPlan(err) => err.error_name(),
            Self::OutOfMemory => "ENOMEM",
        }
    }
}

/// The name of the result of an operation: `SUCCESS` or the name of the
/// error.
pub fn result_name<T, E: ErrorName>(result: &Result<T, E>) -> &'static str {
    match result {
        Ok(_) => "SUCCESS",
        Err(err) => err.error_name(),
    }
}
