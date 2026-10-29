//! Yaw control: the yaw of the drone during a show, given as a yaw offset and
//! a list of relative yaw changes with durations.
//!
//! Like a trajectory, a yaw control object is kept in its encoded form, as in
//! the body of a yaw control block, and decoded one delta at a time by a
//! [`YawPlayer`]. One read from a show file in memory borrows its bytes from
//! the show file.

mod player;

use alloc::borrow::Cow;
use alloc::collections::TryReserveError;
use alloc::vec::Vec;
use core::{error, fmt};

pub use player::{YawPlayer, YawSetpoint};

use crate::formats::binary::{BinaryBlockType, BinaryFile, BinaryFileError};
use crate::parsing::read_i16;

/// The length of the header of a yaw control block: a flags byte and the yaw
/// offset.
const HEADER_LENGTH: usize = 3;

/// The length of a delta: a 16-bit duration and a 16-bit yaw change.
const DELTA_LENGTH: usize = 4;

/// Why a yaw control object could not be read or created.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum YawControlError {
    /// The body of the yaw control block is malformed: it is shorter than the
    /// header, or it ends in the middle of a delta.
    Corrupted,
    /// The show file could not be read.
    File(BinaryFileError),
    /// Memory could not be allocated.
    OutOfMemory,
}

impl fmt::Display for YawControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Corrupted => f.write_str("the yaw control block is malformed"),
            Self::File(_) => f.write_str("the show file could not be read"),
            Self::OutOfMemory => f.write_str("out of memory"),
        }
    }
}

impl error::Error for YawControlError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::File(err) => Some(err),
            Self::Corrupted | Self::OutOfMemory => None,
        }
    }
}

impl From<BinaryFileError> for YawControlError {
    fn from(err: BinaryFileError) -> Self {
        Self::File(err)
    }
}

impl From<TryReserveError> for YawControlError {
    fn from(_: TryReserveError) -> Self {
        Self::OutOfMemory
    }
}

/// The yaw control of a show: a yaw offset and a list of relative yaw changes
/// (deltas), each with a duration.
#[derive(Clone, Debug)]
pub struct YawControl<'a> {
    /// The header and the deltas, as in the body of a yaw control block.
    bytes: Cow<'a, [u8]>,
    /// The length of the header in `bytes`.
    header_length: usize,
    auto_yaw: bool,
    yaw_offset_ddeg: i16,
}

impl Default for YawControl<'static> {
    fn default() -> Self {
        Self::new()
    }
}

impl YawControl<'static> {
    /// An empty yaw control object, with a constant yaw of zero.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            bytes: Cow::Borrowed(&[]),
            header_length: 0,
            auto_yaw: false,
            yaw_offset_ddeg: 0,
        }
    }

    /// A yaw control object that owns the given body of a yaw control block.
    ///
    /// # Errors
    ///
    /// Returns [`YawControlError::Corrupted`] if the body is malformed.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, YawControlError> {
        Self::from_cow(Cow::Owned(bytes))
    }
}

impl<'a> YawControl<'a> {
    /// A yaw control object that borrows the given body of a yaw control
    /// block.
    ///
    /// # Errors
    ///
    /// Returns [`YawControlError::Corrupted`] if the body is malformed.
    pub fn from_buffer(bytes: &'a [u8]) -> Result<Self, YawControlError> {
        Self::from_cow(Cow::Borrowed(bytes))
    }

    /// The yaw control of a show file, borrowing its bytes from the show
    /// file, or `None` if the show file has no yaw control.
    ///
    /// # Errors
    ///
    /// Returns an error if the yaw control block is truncated or malformed.
    pub fn from_binary_file(file: &BinaryFile<'a>) -> Result<Option<Self>, YawControlError> {
        file.find_first_block_by_type(BinaryBlockType::YAW_CONTROL)?
            .map(|block| Self::from_buffer(block.body))
            .transpose()
    }

    fn from_cow(bytes: Cow<'a, [u8]>) -> Result<Self, YawControlError> {
        let Some(deltas) = bytes.len().checked_sub(HEADER_LENGTH) else {
            return Err(YawControlError::Corrupted);
        };
        if !deltas.is_multiple_of(DELTA_LENGTH) {
            return Err(YawControlError::Corrupted);
        }
        // The other bits of the flags are reserved
        let auto_yaw = bytes.first().is_some_and(|&flags| flags & 0x01 != 0);
        let yaw_offset_ddeg = read_i16(&bytes, 1).unwrap_or_default();
        Ok(Self {
            bytes,
            header_length: HEADER_LENGTH,
            auto_yaw,
            yaw_offset_ddeg,
        })
    }

    /// The bytes of the yaw control object, as in the body of a yaw control
    /// block.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Whether the drone should turn automatically towards the direction of
    /// its motion (the auto yaw flag of the block). The flag is exposed as it
    /// is; the yaw player does not use it.
    #[must_use]
    pub const fn auto_yaw(&self) -> bool {
        self.auto_yaw
    }

    /// The yaw at the start of the show, in tenths of degrees.
    #[must_use]
    pub const fn yaw_offset_ddeg(&self) -> i16 {
        self.yaw_offset_ddeg
    }

    /// The number of deltas.
    #[must_use]
    pub fn num_deltas(&self) -> usize {
        self.deltas().len() / DELTA_LENGTH
    }

    /// Whether the yaw control object has no deltas, i.e. the yaw is
    /// constant.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.num_deltas() == 0
    }

    /// The total duration of the deltas, in milliseconds.
    #[must_use]
    pub fn total_duration_msec(&self) -> u32 {
        YawPlayer::new(self).total_duration_msec()
    }

    /// Replaces the yaw control object with one that keeps the given yaw, in
    /// degrees, forever. The yaw is rounded to tenths of degrees and clamped
    /// to the range of the yaw offset, from -3276.8 to 3276.7 degrees; NaN
    /// becomes zero.
    ///
    /// # Errors
    ///
    /// Returns [`YawControlError::OutOfMemory`] if memory could not be
    /// allocated; the yaw control object is left intact then.
    pub fn set_constant_yaw(&mut self, yaw_deg: f32) -> Result<(), YawControlError> {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the float-to-integer cast saturates, and the result is clamped"
        )]
        let yaw_ddeg = libm::roundf(yaw_deg * 10.0) as i32;
        let yaw_ddeg =
            i16::try_from(yaw_ddeg).unwrap_or(if yaw_ddeg < 0 { i16::MIN } else { i16::MAX });
        let [low, high] = yaw_ddeg.to_le_bytes();
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(HEADER_LENGTH)?;
        bytes.extend_from_slice(&[0, low, high]);
        *self = YawControl::from_bytes(bytes)?;
        Ok(())
    }

    /// The bytes of the deltas.
    fn deltas(&self) -> &[u8] {
        self.bytes.get(self.header_length..).unwrap_or_default()
    }
}
