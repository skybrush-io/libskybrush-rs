//! Decoding of the primitive values of the binary formats.

/// Reads a signed 16-bit little-endian integer at the given offset, or `None`
/// if the bytes end before it.
pub(crate) fn read_i16(bytes: &[u8], offset: usize) -> Option<i16> {
    let chunk = bytes.get(offset..)?.first_chunk::<2>()?;
    Some(i16::from_le_bytes(*chunk))
}

/// Reads an unsigned 16-bit little-endian integer at the given offset, or
/// `None` if the bytes end before it.
pub(crate) fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let chunk = bytes.get(offset..)?.first_chunk::<2>()?;
    Some(u16::from_le_bytes(*chunk))
}

/// Why a varint could not be decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VarintError {
    /// The bytes ended before the last byte of the varint.
    Truncated,
    /// The value does not fit into 32 bits. `len` is the number of bytes the
    /// varint occupies, so that a caller that clamps the value can continue
    /// after it.
    Overflow { len: usize },
}

/// Decodes an unsigned varint of at most 32 bits from the start of `bytes`,
/// and returns its value and the number of bytes it occupies.
///
/// A varint stores 7 bits per byte, least significant group first; the
/// highest bit of a byte is set if more bytes follow. A 32-bit value needs at
/// most five bytes, the fifth of which may hold at most four bits. Longer
/// encodings are an overflow even if their value would fit, like in the C
/// library.
pub(crate) fn decode_varuint32(bytes: &[u8]) -> Result<(u32, usize), VarintError> {
    let mut value: u32 = 0;
    // A fifth byte either ends the varint or overflows, so at most five
    // bytes are examined here.
    for (index, &byte) in bytes.iter().enumerate().take(5) {
        if index == 4 && byte >= 0x10 {
            return Err(skip_overflowing(bytes, index));
        }
        value |= u32::from(byte & 0x7f) << (7 * index);
        if byte & 0x80 == 0 {
            return Ok((value, index + 1));
        }
    }
    Err(VarintError::Truncated)
}

/// Finds the end of a varint that overflows at the byte at `index`.
fn skip_overflowing(bytes: &[u8], index: usize) -> VarintError {
    match bytes.iter().skip(index).position(|byte| byte & 0x80 == 0) {
        // The sum is at most the length of the slice.
        Some(position) => VarintError::Overflow {
            len: index + position + 1,
        },
        None => VarintError::Truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_values() {
        assert_eq!(decode_varuint32(&[0x00]), Ok((0, 1)));
        assert_eq!(decode_varuint32(&[0x7f, 0xff]), Ok((127, 1)));
        assert_eq!(decode_varuint32(&[0x80, 0x01]), Ok((128, 2)));
        assert_eq!(decode_varuint32(&[0xac, 0x02]), Ok((300, 2)));
    }

    #[test]
    fn leading_zero_groups_are_allowed() {
        assert_eq!(
            decode_varuint32(&[0x80, 0x80, 0x80, 0x80, 0x00]),
            Ok((0, 5))
        );
        assert_eq!(decode_varuint32(&[0x81, 0x80, 0x00]), Ok((1, 3)));
    }

    #[test]
    fn largest_value() {
        assert_eq!(
            decode_varuint32(&[0xff, 0xff, 0xff, 0xff, 0x0f]),
            Ok((u32::MAX, 5))
        );
    }

    #[test]
    fn overflow_reports_the_length_of_the_varint() {
        // 2^32
        assert_eq!(
            decode_varuint32(&[0x80, 0x80, 0x80, 0x80, 0x10, 0x00]),
            Err(VarintError::Overflow { len: 5 })
        );
        // A sixth byte, even if the value would fit
        assert_eq!(
            decode_varuint32(&[0x80, 0x80, 0x80, 0x80, 0x80, 0x00, 0x2a]),
            Err(VarintError::Overflow { len: 6 })
        );
        assert_eq!(
            decode_varuint32(&[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f, 0x2a]),
            Err(VarintError::Overflow { len: 7 })
        );
    }

    #[test]
    fn truncation() {
        assert_eq!(decode_varuint32(&[]), Err(VarintError::Truncated));
        assert_eq!(decode_varuint32(&[0x80]), Err(VarintError::Truncated));
        assert_eq!(
            decode_varuint32(&[0xff, 0xff, 0xff, 0xff]),
            Err(VarintError::Truncated)
        );
        // Truncation wins over an overflow that was already detected
        assert_eq!(
            decode_varuint32(&[0xff, 0xff, 0xff, 0xff, 0xff, 0xff]),
            Err(VarintError::Truncated)
        );
    }
}
