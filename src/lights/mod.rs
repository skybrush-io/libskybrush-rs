//! Light programs and the light player.
//!
//! A light program is the bytecode of a light program block of a show file.
//! It is not decoded when it is loaded; the [`LightPlayer`] executes it one
//! command at a time. Malformed bytecode does not make the program invalid:
//! execution stops at the first command that cannot be executed, and the
//! current color is held.

mod player;

use alloc::borrow::Cow;
use alloc::collections::TryReserveError;
use alloc::vec::Vec;
use core::{error, fmt};

pub use player::LightPlayer;

use crate::colors::RgbColor;
use crate::formats::binary::{BinaryBlockType, BinaryFile, BinaryFileError};

/// The opcodes of the bytecode of light programs.
mod opcodes {
    pub const END: u8 = 0x00;
    pub const NOP: u8 = 0x01;
    pub const SLEEP: u8 = 0x02;
    pub const WAIT_UNTIL: u8 = 0x03;
    pub const SET_COLOR: u8 = 0x04;
    pub const SET_GRAY: u8 = 0x05;
    pub const SET_BLACK: u8 = 0x06;
    pub const SET_WHITE: u8 = 0x07;
    pub const FADE_TO_COLOR: u8 = 0x08;
    pub const FADE_TO_GRAY: u8 = 0x09;
    pub const FADE_TO_BLACK: u8 = 0x0a;
    pub const FADE_TO_WHITE: u8 = 0x0b;
    pub const LOOP_BEGIN: u8 = 0x0c;
    pub const LOOP_END: u8 = 0x0d;
}

/// Why a light program could not be read or created.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightProgramError {
    /// The show file could not be read.
    File(BinaryFileError),
    /// Memory could not be allocated.
    OutOfMemory,
}

impl fmt::Display for LightProgramError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File(err) => write!(f, "{err}"),
            Self::OutOfMemory => f.write_str("out of memory"),
        }
    }
}

impl error::Error for LightProgramError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::File(err) => Some(err),
            Self::OutOfMemory => None,
        }
    }
}

impl From<BinaryFileError> for LightProgramError {
    fn from(err: BinaryFileError) -> Self {
        Self::File(err)
    }
}

impl From<TryReserveError> for LightProgramError {
    fn from(_: TryReserveError) -> Self {
        Self::OutOfMemory
    }
}

/// A light program: the bytecode of a light program block.
#[derive(Clone, Debug, Default)]
pub struct LightProgram<'a> {
    bytes: Cow<'a, [u8]>,
}

impl LightProgram<'static> {
    /// An empty light program, which is black and ends immediately.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            bytes: Cow::Borrowed(&[]),
        }
    }

    /// A light program that owns the given bytecode.
    #[must_use]
    pub const fn from_bytes(bytes: Vec<u8>) -> Self {
        Self {
            bytes: Cow::Owned(bytes),
        }
    }
}

impl<'a> LightProgram<'a> {
    /// A light program that borrows the given bytecode.
    #[must_use]
    pub const fn from_buffer(bytes: &'a [u8]) -> Self {
        Self {
            bytes: Cow::Borrowed(bytes),
        }
    }

    /// The light program of a show file, borrowing its bytecode from the show
    /// file, or `None` if the show file has no light program.
    ///
    /// # Errors
    ///
    /// Returns an error if the light program block is truncated.
    pub fn from_binary_file(file: &BinaryFile<'a>) -> Result<Option<Self>, LightProgramError> {
        Ok(file
            .find_first_block_by_type(BinaryBlockType::LIGHT_PROGRAM)?
            .map(|block| Self::from_buffer(block.body)))
    }

    /// The bytecode of the light program.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Removes the bytecode of the light program, making it empty. A borrowed
    /// light program stops borrowing the show file.
    pub fn clear(&mut self) {
        *self = LightProgram::new();
    }

    /// Replaces the light program with one that shows the given color
    /// forever.
    ///
    /// # Errors
    ///
    /// Returns an error if memory could not be allocated; the light program
    /// is left intact then.
    pub fn set_constant_color(&mut self, color: RgbColor) -> Result<(), LightProgramError> {
        let bytecode = [
            opcodes::SET_COLOR,
            color.red,
            color.green,
            color.blue,
            opcodes::END,
        ];
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(bytecode.len())?;
        bytes.extend_from_slice(&bytecode);
        self.bytes = Cow::Owned(bytes);
        Ok(())
    }
}
