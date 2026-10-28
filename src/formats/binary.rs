//! Reading Skybrush binary show files (`.skyb`).
//!
//! A show file is a header (signature, version, features and an optional
//! checksum) followed by blocks, each with a type, a length and a body. The
//! parser validates the header and the checksum when it is created, and gives
//! access to the bodies of the blocks as views into the show file.

use core::iter::FusedIterator;
use core::{error, fmt};

use crate::crc32::ap_crc32_update;

/// The signature at the start of every show file.
const SIGNATURE: &[u8; 4] = b"skyb";

/// Feature bit indicating that the header contains an AP-CRC32 checksum of the
/// entire file.
const FEATURE_CRC32: u8 = 1;

/// Offset and length of the checksum in the header of a version 2 file.
const CHECKSUM_OFFSET: usize = 6;
const CHECKSUM_LENGTH: usize = 4;

/// The length of the header of a block: one byte of type, two bytes of length.
const BLOCK_HEADER_LENGTH: usize = 3;

/// The type of a block in a show file.
///
/// Block types that the crate does not know are valid; they are skipped when
/// looking for a block.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BinaryBlockType(pub u8);

impl BinaryBlockType {
    /// A block that contains a trajectory.
    pub const TRAJECTORY: Self = Self(1);
    /// A block that contains a light program.
    pub const LIGHT_PROGRAM: Self = Self(2);
    /// A comment block that contains arbitrary text.
    pub const COMMENT: Self = Self(3);
    /// A block that contains a return-to-home plan.
    pub const RTH_PLAN: Self = Self(4);
    /// A block that contains yaw control setpoints.
    pub const YAW_CONTROL: Self = Self(5);
    /// A block that contains an event list.
    pub const EVENT_LIST: Self = Self(6);
    /// A block that contains the setup of interactive light control from the
    /// ground station.
    pub const GCS_LIGHT_CONTROL_SETUP: Self = Self(7);
    /// A block that contains the metadata of the show (show ID, index of the
    /// drone).
    pub const SHOW_METADATA: Self = Self(8);

    /// The reserved type that ends the list of blocks.
    const END: Self = Self(0);
}

/// A block of a show file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BinaryBlock<'a> {
    /// The type of the block.
    pub block_type: BinaryBlockType,
    /// The body of the block, a view into the show file.
    pub body: &'a [u8],
}

/// Why a show file could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryFileError {
    /// The file does not start with the signature `skyb`.
    InvalidSignature,
    /// The version of the file format is not supported.
    UnsupportedVersion(u8),
    /// The file ends in the middle of its header.
    TruncatedHeader,
    /// The checksum in the header does not match the contents of the file.
    ChecksumMismatch {
        /// The checksum in the header.
        expected: u32,
        /// The checksum of the contents.
        actual: u32,
    },
    /// The file ends in the middle of the header of a block.
    TruncatedBlockHeader,
    /// The file ends before the end of a block that had to be read or
    /// skipped.
    TruncatedBlock,
}

impl fmt::Display for BinaryFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSignature => f.write_str("not a Skybrush binary show file"),
            Self::UnsupportedVersion(version) => {
                write!(f, "unsupported version of the show file format: {version}")
            }
            Self::TruncatedHeader => f.write_str("the show file ends in its header"),
            Self::ChecksumMismatch { expected, actual } => write!(
                f,
                "checksum mismatch: the header says {expected:#010x}, the contents give {actual:#010x}"
            ),
            Self::TruncatedBlockHeader => {
                f.write_str("the show file ends in the header of a block")
            }
            Self::TruncatedBlock => f.write_str("the show file ends in the middle of a block"),
        }
    }
}

impl error::Error for BinaryFileError {}

/// A Skybrush binary show file in memory, with a valid header.
///
/// The counterpart of `sb_binary_file_parser_t` of the C library.
#[derive(Clone, Copy, Debug)]
pub struct BinaryFile<'a> {
    bytes: &'a [u8],
    version: u8,
    /// Offset of the first block.
    start_of_first_block: usize,
}

impl<'a> BinaryFile<'a> {
    /// Validates the header and the checksum of a show file, and the header of
    /// its first block.
    ///
    /// # Errors
    ///
    /// Returns an error if the header is invalid or truncated, if the checksum
    /// does not match, or if the file ends in the middle of the header of the
    /// first block. Later blocks are checked when they are reached.
    pub fn new(bytes: &'a [u8]) -> Result<Self, BinaryFileError> {
        let (signature, rest) = bytes
            .split_first_chunk::<4>()
            .ok_or(BinaryFileError::InvalidSignature)?;
        if signature != SIGNATURE {
            return Err(BinaryFileError::InvalidSignature);
        }
        let (&version, rest) = rest.split_first().ok_or(BinaryFileError::TruncatedHeader)?;
        let (features, rest) = match version {
            1 => (0, rest),
            2 => {
                let (&features, rest) =
                    rest.split_first().ok_or(BinaryFileError::TruncatedHeader)?;
                (features, rest)
            }
            _ => return Err(BinaryFileError::UnsupportedVersion(version)),
        };
        let rest = if features & FEATURE_CRC32 == 0 {
            rest
        } else {
            let (checksum, rest) = rest
                .split_first_chunk::<CHECKSUM_LENGTH>()
                .ok_or(BinaryFileError::TruncatedHeader)?;
            let expected = u32::from_le_bytes(*checksum);
            let actual = checksum_of(bytes);
            if expected != actual {
                return Err(BinaryFileError::ChecksumMismatch { expected, actual });
            }
            rest
        };

        let parser = Self {
            bytes,
            version,
            start_of_first_block: bytes.len() - rest.len(),
        };
        parser.blocks().peek_header()?;
        Ok(parser)
    }

    /// The version of the file format, 1 or 2.
    #[must_use]
    pub const fn version(&self) -> u8 {
        self.version
    }

    /// The blocks of the show file, in the order they appear in the file.
    #[must_use]
    pub fn blocks(&self) -> Blocks<'a> {
        Blocks {
            rest: self
                .bytes
                .get(self.start_of_first_block..)
                .unwrap_or_default(),
        }
    }

    /// The first block of the given type, or `None` if the file has no such
    /// block.
    ///
    /// # Errors
    ///
    /// Returns an error if the file ends in the middle of the block, or of a
    /// block before it.
    pub fn find_first_block_by_type(
        &self,
        block_type: BinaryBlockType,
    ) -> Result<Option<BinaryBlock<'a>>, BinaryFileError> {
        for block in self.blocks() {
            let block = block?;
            if block.block_type == block_type {
                return Ok(Some(block));
            }
        }
        Ok(None)
    }
}

/// The checksum of a show file, with the bytes of the checksum in the header
/// taken as zeros.
fn checksum_of(bytes: &[u8]) -> u32 {
    let (head, rest) = bytes.split_at(CHECKSUM_OFFSET.min(bytes.len()));
    let tail = rest.get(CHECKSUM_LENGTH..).unwrap_or_default();
    let crc = ap_crc32_update(0, head);
    let crc = ap_crc32_update(crc, &[0; CHECKSUM_LENGTH]);
    ap_crc32_update(crc, tail)
}

/// Iterator over the blocks of a show file.
///
/// The iteration ends at the end of the file or at a block of type zero. If
/// the file ends in the middle of a block, the iterator yields an error and
/// then ends.
#[derive(Clone, Debug)]
pub struct Blocks<'a> {
    /// The rest of the file, starting at the header of the next block.
    rest: &'a [u8],
}

impl Blocks<'_> {
    /// Reads the header of the next block: its type and the length of its
    /// body, or `None` at the end of the list of blocks.
    fn peek_header(&self) -> Result<Option<(BinaryBlockType, usize)>, BinaryFileError> {
        let Some((&block_type, rest)) = self.rest.split_first() else {
            return Ok(None);
        };
        // The length is read even for the reserved type that ends the list.
        let length = rest
            .first_chunk::<2>()
            .ok_or(BinaryFileError::TruncatedBlockHeader)?;
        let block_type = BinaryBlockType(block_type);
        if block_type == BinaryBlockType::END {
            return Ok(None);
        }
        Ok(Some((block_type, usize::from(u16::from_le_bytes(*length)))))
    }
}

impl<'a> Iterator for Blocks<'a> {
    type Item = Result<BinaryBlock<'a>, BinaryFileError>;

    fn next(&mut self) -> Option<Self::Item> {
        let result = match self.peek_header() {
            Ok(None) => None,
            Ok(Some((block_type, length))) => {
                let rest = self.rest.get(BLOCK_HEADER_LENGTH..).unwrap_or_default();
                match rest.split_at_checked(length) {
                    Some((body, rest)) => {
                        self.rest = rest;
                        return Some(Ok(BinaryBlock { block_type, body }));
                    }
                    None => Some(Err(BinaryFileError::TruncatedBlock)),
                }
            }
            Err(err) => Some(Err(err)),
        };
        // At the end of the blocks or after an error, the iteration ends.
        self.rest = &[];
        result
    }
}

impl FusedIterator for Blocks<'_> {}

#[cfg(test)]
mod tests {
    use super::*;

    fn blocks(bytes: &[u8]) -> Result<([BinaryBlockType; 2], usize), BinaryFileError> {
        let parser = BinaryFile::new(bytes)?;
        let mut types = [BinaryBlockType::END; 2];
        let mut count = 0;
        for (slot, block) in types.iter_mut().zip(parser.blocks()) {
            *slot = block?.block_type;
            count += 1;
        }
        Ok((types, count))
    }

    #[test]
    fn blocks_are_views_into_the_file() {
        let bytes = b"skyb\x01\x03\x02\x00hi\x7f\x00\x00";
        let parser = BinaryFile::new(bytes).unwrap();
        let mut iter = parser.blocks();
        assert_eq!(
            iter.next(),
            Some(Ok(BinaryBlock {
                block_type: BinaryBlockType::COMMENT,
                body: b"hi"
            }))
        );
        assert_eq!(
            iter.next(),
            Some(Ok(BinaryBlock {
                block_type: BinaryBlockType(0x7f),
                body: b""
            }))
        );
        assert_eq!(iter.next(), None);
        assert_eq!(parser.version(), 1);
    }

    #[test]
    fn errors_end_the_iteration() {
        let parser = BinaryFile::new(b"skyb\x01\x03\x02\x00hi\x01\x05\x00ab").unwrap();
        let mut iter = parser.blocks();
        assert!(matches!(iter.next(), Some(Ok(_))));
        assert_eq!(iter.next(), Some(Err(BinaryFileError::TruncatedBlock)));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn type_zero_ends_the_list_but_needs_a_length() {
        assert_eq!(
            blocks(b"skyb\x01\x00\x00\x00\x03\x00\x00"),
            Ok(([BinaryBlockType::END; 2], 0))
        );
        assert_eq!(
            blocks(b"skyb\x01\x00"),
            Err(BinaryFileError::TruncatedBlockHeader)
        );
    }

    #[test]
    fn checksum_covers_the_file_without_itself() {
        // The checksum of the file with zeros in place of the checksum
        let mut bytes = *b"skyb\x02\x01\0\0\0\0\x03\x01\x00!";
        let crc = ap_crc32_update(0, &bytes);
        bytes[6..10].copy_from_slice(&crc.to_le_bytes());
        assert!(BinaryFile::new(&bytes).is_ok());
        bytes[13] = b'?';
        assert!(matches!(
            BinaryFile::new(&bytes),
            Err(BinaryFileError::ChecksumMismatch { expected, .. }) if expected == crc
        ));
    }
}
