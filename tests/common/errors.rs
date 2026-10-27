//! Names of the errors of the crate, as the oracle records them.
//!
//! The oracle records errors by the names of the error codes of the C
//! library. Each error enum of the crate maps every variant to one name; the
//! matches are exhaustive, so a new variant does not compile until it is
//! mapped here.

use skybrush::formats::binary::BinaryFileError;

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

/// The name of the result of an operation: `SUCCESS` or the name of the
/// error.
pub fn result_name<T, E: ErrorName>(result: &Result<T, E>) -> &'static str {
    match result {
        Ok(_) => "SUCCESS",
        Err(err) => err.error_name(),
    }
}
