//! The setup of interactive light control from the ground station: the pixel
//! that the drone represents in an LED matrix, the size of the matrix, and
//! the palette of colors that the ground station may use.

use core::{error, fmt};

use crate::basic_types::Vector2I16;
use crate::formats::binary::{BinaryBlockType, BinaryFile, BinaryFileError};
use crate::formats::tlv::{TlvEntry, TlvParser};
use crate::palette::ColorPalette;

/// The tags of the entries of a GCS light control setup block.
mod tags {
    pub const COORDINATES: u8 = 0x00;
    pub const PALETTE: u8 = 0x01;
    pub const MATRIX_SIZE: u8 = 0x02;
}

/// The largest valid coordinate and size of the matrix.
const MAX_COMPONENT_VALUE: i16 = 4095;

/// Why the setup of light control from the ground station could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GcsLightControlSetupError {
    /// The body of the block is not a valid tag-length-value stream, a known
    /// tag has a value of the wrong length, or the coordinates or the size
    /// of the matrix are not between 0 and 4095.
    Corrupted,
    /// The show file could not be read.
    File(BinaryFileError),
}

impl fmt::Display for GcsLightControlSetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Corrupted => f.write_str("the GCS light control setup block is malformed"),
            Self::File(err) => write!(f, "{err}"),
        }
    }
}

impl error::Error for GcsLightControlSetupError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Corrupted => None,
            Self::File(err) => Some(err),
        }
    }
}

impl From<BinaryFileError> for GcsLightControlSetupError {
    fn from(err: BinaryFileError) -> Self {
        Self::File(err)
    }
}

/// The setup of interactive light control from the ground station. Values
/// that the block does not contain are zero, and the palette is empty then.
///
/// The palette of a setup read from a show file in memory borrows its colors
/// from the show file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GcsLightControlSetup<'a> {
    coords: Vector2I16,
    size: Vector2I16,
    palette: ColorPalette<'a>,
}

impl GcsLightControlSetup<'static> {
    /// A setup with zero coordinates, a zero matrix size and an empty
    /// palette.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            coords: Vector2I16 { x: 0, y: 0 },
            size: Vector2I16 { x: 0, y: 0 },
            palette: ColorPalette::new(),
        }
    }
}

impl<'a> GcsLightControlSetup<'a> {
    /// The setup in the body of a GCS light control setup block; the palette
    /// borrows its colors from the body. Unknown tags are skipped; when a
    /// tag appears more than once, the last one wins.
    ///
    /// # Errors
    ///
    /// Returns [`GcsLightControlSetupError::Corrupted`] if the body is
    /// malformed.
    pub fn from_buffer(body: &'a [u8]) -> Result<Self, GcsLightControlSetupError> {
        let mut result = GcsLightControlSetup::new();
        for entry in TlvParser::new(body) {
            let entry = entry.map_err(|_| GcsLightControlSetupError::Corrupted)?;
            match entry.tag {
                tags::COORDINATES => result.coords = parse_vector2_i16(entry)?,
                tags::MATRIX_SIZE => result.size = parse_vector2_i16(entry)?,
                tags::PALETTE => {
                    result.palette = ColorPalette::from_buffer(entry.value)
                        .map_err(|_| GcsLightControlSetupError::Corrupted)?;
                }
                // Unknown tags are skipped, so that new tags can be added
                _ => {}
            }
        }
        Ok(result)
    }

    /// The setup of a show file, borrowing its palette from the show file,
    /// or `None` if the show file has no GCS light control setup block.
    ///
    /// # Errors
    ///
    /// Returns an error if the block is truncated or malformed.
    pub fn from_binary_file(
        file: &BinaryFile<'a>,
    ) -> Result<Option<Self>, GcsLightControlSetupError> {
        file.find_first_block_by_type(BinaryBlockType::GCS_LIGHT_CONTROL_SETUP)?
            .map(|block| Self::from_buffer(block.body))
            .transpose()
    }

    /// The setup with its palette owned, copying the colors if they are
    /// borrowed.
    #[must_use]
    pub fn into_owned(self) -> GcsLightControlSetup<'static> {
        GcsLightControlSetup {
            coords: self.coords,
            size: self.size,
            palette: self.palette.into_owned(),
        }
    }

    /// Resets the setup to zero coordinates, a zero matrix size and an empty
    /// palette.
    pub fn clear(&mut self) {
        *self = GcsLightControlSetup::new();
    }

    /// The pixel that the drone represents in the LED matrix.
    #[must_use]
    pub const fn coords(&self) -> Vector2I16 {
        self.coords
    }

    /// The size of the LED matrix: its width in `x` and its height in `y`.
    #[must_use]
    pub const fn size(&self) -> Vector2I16 {
        self.size
    }

    /// The colors that the ground station may use.
    #[must_use]
    pub const fn palette(&self) -> &ColorPalette<'a> {
        &self.palette
    }
}

/// Parses a coordinate pair or a matrix size: two 16-bit little-endian
/// integers, each between 0 and 4095.
fn parse_vector2_i16(entry: TlvEntry<'_>) -> Result<Vector2I16, GcsLightControlSetupError> {
    let &[x0, x1, y0, y1] = entry.value else {
        return Err(GcsLightControlSetupError::Corrupted);
    };
    let result = Vector2I16 {
        x: i16::from_le_bytes([x0, x1]),
        y: i16::from_le_bytes([y0, y1]),
    };
    let valid = 0..=MAX_COMPONENT_VALUE;
    if valid.contains(&result.x) && valid.contains(&result.y) {
        Ok(result)
    } else {
        Err(GcsLightControlSetupError::Corrupted)
    }
}
