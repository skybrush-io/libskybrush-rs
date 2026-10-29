//! The metadata of a show: its unique ID and the index of the drone within
//! the show.

use core::{error, fmt};

use crate::formats::binary::{BinaryBlockType, BinaryFile, BinaryFileError};
use crate::formats::tlv::TlvEntries;

/// The length of the unique ID of a show, in bytes.
pub const SHOW_ID_LENGTH: usize = 4;

/// The tags of the entries of a show metadata block.
mod tags {
    pub const SHOW_ID: u8 = 0x00;
    pub const DRONE_INDEX: u8 = 0x01;
}

/// Why the metadata of a show could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ShowMetadataError {
    /// The body of the show metadata block is not a valid tag-length-value
    /// stream, or a known tag has a value of the wrong length.
    Corrupted,
    /// The show file could not be read.
    File(BinaryFileError),
}

impl fmt::Display for ShowMetadataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Corrupted => f.write_str("the show metadata block is malformed"),
            Self::File(_) => f.write_str("the show file could not be read"),
        }
    }
}

impl error::Error for ShowMetadataError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Corrupted => None,
            Self::File(err) => Some(err),
        }
    }
}

impl From<BinaryFileError> for ShowMetadataError {
    fn from(err: BinaryFileError) -> Self {
        Self::File(err)
    }
}

/// The metadata of a show. Values that the show metadata block does not
/// contain are zero.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShowMetadata {
    show_id: [u8; SHOW_ID_LENGTH],
    drone_index: u16,
}

impl ShowMetadata {
    /// Metadata with an all-zero show ID and a drone index of zero.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            show_id: [0; SHOW_ID_LENGTH],
            drone_index: 0,
        }
    }

    /// The metadata in the body of a show metadata block. Unknown tags are
    /// skipped; when a tag appears more than once, the last one wins.
    ///
    /// # Errors
    ///
    /// Returns [`ShowMetadataError::Corrupted`] if the body is not a valid
    /// tag-length-value stream, or the show ID is not four bytes long, or
    /// the drone index is not two bytes long.
    pub fn from_buffer(body: &[u8]) -> Result<Self, ShowMetadataError> {
        let mut result = Self::new();
        for entry in TlvEntries::new(body) {
            let entry = entry.map_err(|_| ShowMetadataError::Corrupted)?;
            match entry.tag {
                tags::SHOW_ID => {
                    result.show_id = entry
                        .value
                        .try_into()
                        .map_err(|_| ShowMetadataError::Corrupted)?;
                }
                tags::DRONE_INDEX => {
                    let value = entry
                        .value
                        .try_into()
                        .map_err(|_| ShowMetadataError::Corrupted)?;
                    result.drone_index = u16::from_le_bytes(value);
                }
                // Unknown tags are skipped, so that new tags can be added
                _ => {}
            }
        }
        Ok(result)
    }

    /// The metadata of a show file, or `None` if the show file has no show
    /// metadata block.
    ///
    /// # Errors
    ///
    /// Returns an error if the show metadata block is truncated or malformed.
    pub fn from_binary_file(file: &BinaryFile<'_>) -> Result<Option<Self>, ShowMetadataError> {
        file.find_first_block_by_type(BinaryBlockType::SHOW_METADATA)?
            .map(|block| Self::from_buffer(block.body))
            .transpose()
    }

    /// Resets the metadata to an all-zero show ID and a drone index of zero.
    pub const fn clear(&mut self) {
        *self = Self::new();
    }

    /// The unique ID of the show, an arbitrary binary blob.
    #[must_use]
    pub const fn show_id(&self) -> [u8; SHOW_ID_LENGTH] {
        self.show_id
    }

    /// The index of the drone within the show.
    #[must_use]
    pub const fn drone_index(&self) -> u16 {
        self.drone_index
    }
}
