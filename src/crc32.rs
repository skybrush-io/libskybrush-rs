//! The variant of CRC32 used by ArduPilot (AP-CRC32), for the checksums of
//! show files.
//!
//! AP-CRC32 uses the generator polynomial `0x104C11DB7`, bit reversal, an
//! initial value of zero and no final XOR, which makes it compatible with
//! MAVFTP. With the `crcmod` module of Python:
//!
//! ```text
//! crcmod.mkCrcFun(0x104C11DB7, initCrc=0, rev=True, xorOut=0)
//! ```

/// The generator polynomial in reversed bit order.
const POLYNOMIAL: u32 = 0xedb8_8320;

/// The CRC of each byte value, so that the checksum is updated a byte at a
/// time.
const TABLE: [u32; 256] = make_table();

#[expect(
    clippy::indexing_slicing,
    reason = "evaluated only at compile time, where an index out of bounds fails the build"
)]
const fn make_table() -> [u32; 256] {
    let mut table = [0; 256];
    let mut index = 0;
    let mut byte: u32 = 0;
    while index < table.len() {
        let mut crc = byte;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 == 0 {
                crc >> 1
            } else {
                (crc >> 1) ^ POLYNOMIAL
            };
            bit += 1;
        }
        table[index] = crc;
        index += 1;
        byte += 1;
    }
    table
}

/// Updates an AP-CRC32 checksum with the given bytes.
///
/// The checksum of a sequence of bytes is `ap_crc32_update(0, bytes)`; it can
/// be computed in chunks by passing the checksum of the previous chunks.
pub(crate) fn ap_crc32_update(crc: u32, bytes: &[u8]) -> u32 {
    bytes.iter().fold(crc, |crc, &byte| {
        let [low, ..] = crc.to_le_bytes();
        let index = usize::from(low ^ byte);
        #[expect(
            clippy::indexing_slicing,
            reason = "a u8 index is always within the 256 entries"
        )]
        let entry = TABLE[index];
        entry ^ (crc >> 8)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_matches_the_c_library() {
        // The first and last entries of crc32_tab in src/crc32.c
        assert_eq!(
            &TABLE[..4],
            [0x0000_0000, 0x7707_3096, 0xee0e_612c, 0x9909_51ba]
        );
        assert_eq!(
            &TABLE[252..],
            [0xb40b_be37, 0xc30c_8ea1, 0x5a05_df1b, 0x2d02_ef8d]
        );
    }

    #[test]
    fn checksums() {
        // Computed with zlib.crc32 of Python, which differs from AP-CRC32
        // only in the initial value and the final XOR
        assert_eq!(ap_crc32_update(0, b""), 0);
        assert_eq!(ap_crc32_update(0, b"\0"), 0);
        assert_eq!(ap_crc32_update(0, b"123456789"), 0x2dfd_2d88);
        let mut all = [0_u8; 256];
        for (slot, byte) in all.iter_mut().zip(0..=u8::MAX) {
            *slot = byte;
        }
        assert_eq!(ap_crc32_update(0, &all), 0x2493_092b);
    }

    #[test]
    fn checksums_can_be_computed_in_chunks() {
        let first = ap_crc32_update(0, b"1234");
        assert_eq!(ap_crc32_update(first, b"56789"), 0x2dfd_2d88);
    }
}
