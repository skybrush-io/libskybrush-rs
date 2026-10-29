//! Color palettes: lists of colors that are referred to by their indices.

use alloc::borrow::Cow;
use alloc::vec::Vec;
use core::{error, fmt};

use crate::colors::RgbColor;

/// The length of a color in a palette: red, green and blue.
const COLOR_LENGTH: usize = 3;

/// Why a color palette could not be created.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ColorPaletteError {
    /// The number of bytes is not a multiple of three.
    InvalidLength,
}

impl fmt::Display for ColorPaletteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLength => {
                f.write_str("the length of the palette is not a multiple of three")
            }
        }
    }
}

impl error::Error for ColorPaletteError {}

/// A list of colors, stored as packed red, green and blue bytes.
///
/// # Examples
///
/// ```
/// use skybrush::colors::RgbColor;
/// use skybrush::palette::ColorPalette;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let palette = ColorPalette::from_buffer(&[255, 0, 0, 0, 255, 0])?;
/// assert_eq!(palette.len(), 2);
/// assert_eq!(palette.color(1), Some(RgbColor::new(0, 255, 0)));
/// assert_eq!(palette.color(2), None);
/// assert_eq!(palette.color_or_default(2), RgbColor::BLACK);
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ColorPalette<'a> {
    bytes: Cow<'a, [u8]>,
}

impl ColorPalette<'static> {
    /// An empty palette.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            bytes: Cow::Borrowed(&[]),
        }
    }

    /// A palette that owns the given packed colors.
    ///
    /// # Errors
    ///
    /// Returns [`ColorPaletteError::InvalidLength`] if the number of bytes is
    /// not a multiple of three.
    pub fn from_vec(bytes: Vec<u8>) -> Result<Self, ColorPaletteError> {
        Self::from_cow(Cow::Owned(bytes))
    }
}

impl<'a> ColorPalette<'a> {
    /// A palette that borrows the given packed colors.
    ///
    /// # Errors
    ///
    /// Returns [`ColorPaletteError::InvalidLength`] if the number of bytes is
    /// not a multiple of three.
    pub fn from_buffer(bytes: &'a [u8]) -> Result<Self, ColorPaletteError> {
        Self::from_cow(Cow::Borrowed(bytes))
    }

    fn from_cow(bytes: Cow<'a, [u8]>) -> Result<Self, ColorPaletteError> {
        if bytes.len().is_multiple_of(COLOR_LENGTH) {
            Ok(Self { bytes })
        } else {
            Err(ColorPaletteError::InvalidLength)
        }
    }

    /// The palette with its colors owned, copying them if they are borrowed.
    #[must_use]
    pub fn into_owned(self) -> ColorPalette<'static> {
        ColorPalette {
            bytes: Cow::Owned(self.bytes.into_owned()),
        }
    }

    /// Removes all colors from the palette.
    pub fn clear(&mut self) {
        *self = ColorPalette::new();
    }

    /// The color at the given index, or `None` if there is no such color.
    #[must_use]
    pub fn color(&self, index: usize) -> Option<RgbColor> {
        let start = index.checked_mul(COLOR_LENGTH)?;
        match *self.bytes.get(start..start.checked_add(COLOR_LENGTH)?)? {
            [red, green, blue] => Some(RgbColor::new(red, green, blue)),
            _ => None,
        }
    }

    /// The color at the given index, or the given color if there is no such
    /// color.
    #[must_use]
    pub fn color_or(&self, index: usize, default: RgbColor) -> RgbColor {
        self.color(index).unwrap_or(default)
    }

    /// The color at the given index, or black (the default color) if there is
    /// no such color, as in the C library.
    #[must_use]
    pub fn color_or_default(&self, index: usize) -> RgbColor {
        self.color(index).unwrap_or_default()
    }

    /// The number of colors in the palette.
    #[must_use]
    pub fn len(&self) -> usize {
        self.bytes.len() / COLOR_LENGTH
    }

    /// Whether the palette has no colors.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// An iterator over the colors of the palette.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = RgbColor> + '_ {
        let (colors, _) = self.bytes.as_chunks::<COLOR_LENGTH>();
        colors
            .iter()
            .map(|&[red, green, blue]| RgbColor::new(red, green, blue))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_are_looked_up_by_index() {
        let palette = ColorPalette::from_buffer(&[1, 2, 3, 4, 5, 6]).unwrap_or_default();
        assert_eq!(palette.len(), 2);
        assert_eq!(palette.color(1), Some(RgbColor::new(4, 5, 6)));
        assert_eq!(palette.color(2), None);
        assert_eq!(palette.color(usize::MAX), None);
        assert_eq!(palette.color_or(0, RgbColor::WHITE), RgbColor::new(1, 2, 3));
        assert_eq!(palette.color_or(2, RgbColor::WHITE), RgbColor::WHITE);
        assert_eq!(palette.color_or_default(2), RgbColor::BLACK);
        assert_eq!(palette.iter().count(), 2);
    }

    #[test]
    fn partial_colors_are_rejected() {
        assert_eq!(
            ColorPalette::from_buffer(&[1, 2, 3, 4]),
            Err(ColorPaletteError::InvalidLength)
        );
    }
}
