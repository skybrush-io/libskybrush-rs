//! Tag-length-value streams, the encoding of the bodies of some blocks of
//! show files (e.g. the metadata and the setup of light control from the
//! ground station).
//!
//! Each entry is a one-byte tag, a 16-bit little-endian length and a value of
//! that many bytes.

use core::iter::FusedIterator;
use core::{error, fmt};

/// The length of the header of an entry: the tag and the length.
const HEADER_LENGTH: usize = 3;

/// An entry of a tag-length-value stream. The value is a view into the
/// stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TlvEntry<'a> {
    /// The tag of the entry.
    pub tag: u8,
    /// The value of the entry.
    pub value: &'a [u8],
}

/// Why a tag-length-value stream could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TlvError {
    /// The stream ends in the middle of the header of an entry.
    TruncatedHeader,
    /// The value of an entry extends past the end of the stream.
    TruncatedValue,
}

impl fmt::Display for TlvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::TruncatedHeader => "the header of an entry is truncated",
            Self::TruncatedValue => "the value of an entry is truncated",
        })
    }
}

impl error::Error for TlvError {}

/// Iterator over the entries of a tag-length-value stream.
///
/// If the stream ends in the middle of an entry, the iterator yields an
/// error and then ends.
///
/// # Examples
///
/// ```
/// use skybrush::formats::tlv::{TlvEntry, TlvEntries};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// // Tag 1 with two bytes, then tag 7 without a value
/// let stream = [0x01, 0x02, 0x00, 0xaa, 0xbb, 0x07, 0x00, 0x00];
/// let entries: Vec<TlvEntry<'_>> = TlvEntries::new(&stream).collect::<Result<_, _>>()?;
/// assert_eq!(entries[0], TlvEntry { tag: 1, value: &[0xaa, 0xbb] });
/// assert_eq!(entries[1], TlvEntry { tag: 7, value: &[] });
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct TlvEntries<'a> {
    /// The rest of the stream, starting at the header of the next entry.
    rest: &'a [u8],
}

impl<'a> TlvEntries<'a> {
    /// A parser of the given stream.
    #[must_use]
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self { rest: bytes }
    }
}

impl<'a> Iterator for TlvEntries<'a> {
    type Item = Result<TlvEntry<'a>, TlvError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.rest.is_empty() {
            return None;
        }
        let result = match self.rest.split_first_chunk::<HEADER_LENGTH>() {
            Some((&[tag, low, high], rest)) => {
                match rest.split_at_checked(usize::from(u16::from_le_bytes([low, high]))) {
                    Some((value, rest)) => {
                        self.rest = rest;
                        return Some(Ok(TlvEntry { tag, value }));
                    }
                    None => Err(TlvError::TruncatedValue),
                }
            }
            None => Err(TlvError::TruncatedHeader),
        };
        // After an error, the iteration ends.
        self.rest = &[];
        Some(result)
    }
}

impl FusedIterator for TlvEntries<'_> {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_are_views_into_the_stream() {
        let bytes = [0x01, 0x02, 0x00, 0xaa, 0xbb, 0x07, 0x00, 0x00];
        let mut parser = TlvEntries::new(&bytes);
        assert_eq!(
            parser.next(),
            Some(Ok(TlvEntry {
                tag: 1,
                value: &[0xaa, 0xbb]
            }))
        );
        assert_eq!(parser.next(), Some(Ok(TlvEntry { tag: 7, value: &[] })));
        assert_eq!(parser.next(), None);
    }

    #[test]
    fn truncation_ends_the_iteration() {
        let mut parser = TlvEntries::new(&[0x01, 0x02]);
        assert_eq!(parser.next(), Some(Err(TlvError::TruncatedHeader)));
        assert_eq!(parser.next(), None);

        let mut parser = TlvEntries::new(&[0x01, 0x02, 0x00, 0xaa]);
        assert_eq!(parser.next(), Some(Err(TlvError::TruncatedValue)));
        assert_eq!(parser.next(), None);
    }
}
