//! RGB and RGBW colors.
//!
//! The library works in RGB; RGBW colors are produced only by converting an
//! RGB color with an [`RgbwConversion`].

use libm::{logf, powf};

/// An RGB color.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct RgbColor {
    /// The red component.
    pub red: u8,
    /// The green component.
    pub green: u8,
    /// The blue component.
    pub blue: u8,
}

/// An RGBW color.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct RgbwColor {
    /// The red component.
    pub red: u8,
    /// The green component.
    pub green: u8,
    /// The blue component.
    pub blue: u8,
    /// The white component.
    pub white: u8,
}

/// Converts a float to a color component, truncating it towards zero.
///
/// The callers clamp the value to `[0, 255]` first, as the C library does.
/// Where the C library would convert NaN (undefined behaviour in C), this
/// returns zero.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "truncation towards zero is intended, and `as` saturates"
)]
fn component(value: f32) -> u8 {
    value as u8
}

impl RgbColor {
    /// Black.
    pub const BLACK: Self = Self::new(0, 0, 0);
    /// White.
    pub const WHITE: Self = Self::new(255, 255, 255);

    /// A color with the given components.
    #[must_use]
    pub const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }

    /// Decodes a color in RGB565 format.
    #[must_use]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the masks and shifts keep each component within 8 bits"
    )]
    pub const fn decode_rgb565(color: u16) -> Self {
        Self::new(
            ((color & 0xf800) >> 8) as u8,
            ((color & 0x07e0) >> 3) as u8,
            ((color & 0x001f) << 3) as u8,
        )
    }

    /// Encodes the color in RGB565 format, dropping the lowest bits of each
    /// component.
    #[must_use]
    pub fn encode_rgb565(self) -> u16 {
        ((u16::from(self.red) >> 3) << 11)
            | ((u16::from(self.green) >> 2) << 5)
            | (u16::from(self.blue) >> 3)
    }

    /// Whether the components of two colors differ by at most `eps`.
    #[must_use]
    pub const fn almost_equals(self, other: Self, eps: u8) -> bool {
        self.red.abs_diff(other.red) <= eps
            && self.green.abs_diff(other.green) <= eps
            && self.blue.abs_diff(other.blue) <= eps
    }

    /// Interpolates linearly between two colors. A ratio of zero gives this
    /// color and a ratio of one gives `other`; ratios outside `[0, 1]`
    /// extrapolate, with the components clamped to `[0, 255]`.
    #[must_use]
    pub fn linear_interpolation(self, other: Self, ratio: f32) -> Self {
        let interpolate = |from: u8, to: u8| {
            let delta = f32::from(i16::from(to) - i16::from(from));
            component((f32::from(from) + delta * ratio).clamp(0.0, 255.0))
        };
        Self::new(
            interpolate(self.red, other.red),
            interpolate(self.green, other.green),
            interpolate(self.blue, other.blue),
        )
    }

    /// Converts the color to RGBW.
    #[must_use]
    pub fn to_rgbw(self, conversion: &RgbwConversion) -> RgbwColor {
        match conversion.method {
            Method::FixedValue(white) => RgbwColor::new(self.red, self.green, self.blue, white),
            Method::SubtractMin => {
                let white = self.red.min(self.green).min(self.blue);
                RgbwColor::new(
                    self.red - white,
                    self.green - white,
                    self.blue - white,
                    white,
                )
            }
            Method::Reference { mul, div, .. } => {
                // Adapted from https://www.dmurph.com/posts/2021/1/cabinet-light-3.html
                // Each channel that the white LED emits limits the white level
                // to the amount of white that the channel can account for.
                let [div_red, div_green, div_blue] = div;
                let mut min_scaled: Option<f32> = None;
                for (value, mul) in [self.red, self.green, self.blue].into_iter().zip(mul) {
                    if let Some(mul) = mul {
                        let scaled = f32::from(value) * mul;
                        min_scaled = match min_scaled {
                            Some(min) if min <= scaled => Some(min),
                            _ => Some(scaled),
                        };
                    }
                }
                let white = match min_scaled {
                    // The white LED emits nothing; it is not used.
                    None => 0,
                    Some(min) if min <= 0.0 => 0,
                    Some(min) if min <= 255.0 => component(min),
                    Some(_) => 255,
                };
                let subtract = |value: u8, div: f32| {
                    let correction = f32::from(white) * div;
                    let value_f = f32::from(value);
                    if value_f > correction {
                        component(value_f - correction)
                    } else {
                        0
                    }
                };
                RgbwColor::new(
                    subtract(self.red, div_red),
                    subtract(self.green, div_green),
                    subtract(self.blue, div_blue),
                    white,
                )
            }
        }
    }

    /// The approximate color of the radiation of an ideal black body at the
    /// given temperature, in Kelvin. Temperatures are clamped to
    /// `[1000, 40000]`.
    ///
    /// See <https://tannerhelland.com/2012/09/18/convert-temperature-rgb-algorithm-code.html>.
    #[must_use]
    #[expect(
        clippy::excessive_precision,
        reason = "the constants of the C library, rounded to f32 like in C"
    )]
    pub fn from_color_temperature(temperature: f32) -> Self {
        let temp_div = if temperature < 1000.0 {
            10.0
        } else if temperature > 40000.0 {
            400.0
        } else {
            temperature / 100.0
        };
        let clamp = |value: f32| {
            if value < 0.0 {
                0
            } else if value > 255.0 {
                255
            } else {
                component(value)
            }
        };

        let red = if temp_div <= 66.0 {
            255
        } else {
            clamp(329.698_727_446 * powf(temp_div - 60.0, -0.133_204_759_2))
        };
        let green = clamp(if temp_div <= 66.0 {
            99.470_802_586_1 * logf(temp_div) - 161.119_568_166_1
        } else {
            288.122_169_528_3 * powf(temp_div - 60.0, -0.075_514_849_2)
        });
        let blue = if temp_div >= 66.0 {
            255
        } else {
            clamp(138.517_731_223_1 * logf(temp_div - 10.0) - 305.044_792_730_7)
        };
        Self::new(red, green, blue)
    }
}

impl RgbwColor {
    /// A color with the given components.
    #[must_use]
    pub const fn new(red: u8, green: u8, blue: u8, white: u8) -> Self {
        Self {
            red,
            green,
            blue,
            white,
        }
    }

    /// Whether the components of two colors differ by at most `eps`.
    #[must_use]
    pub const fn almost_equals(self, other: Self, eps: u8) -> bool {
        self.red.abs_diff(other.red) <= eps
            && self.green.abs_diff(other.green) <= eps
            && self.blue.abs_diff(other.blue) <= eps
            && self.white.abs_diff(other.white) <= eps
    }
}

/// How an RGB color is converted to RGBW. The default turns the white
/// channel off.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RgbwConversion {
    method: Method,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Method {
    /// The RGB components are kept and white is a fixed value.
    FixedValue(u8),
    /// The minimum of the RGB components is moved to the white channel.
    SubtractMin,
    /// The white LED emits the color that the RGB LEDs emit with a reference
    /// color. For each channel, `div` is the amount of the channel that one
    /// unit of white emits (the reference normalized to its largest
    /// component), and `mul` is its reciprocal: the units of white that one
    /// unit of the channel accounts for, or `None` if the white LED does not
    /// emit the channel. `temperature` is the color temperature that the
    /// reference color was calculated from, if any.
    Reference {
        mul: [Option<f32>; 3],
        div: [f32; 3],
        temperature: Option<f32>,
    },
}

impl Default for Method {
    fn default() -> Self {
        Self::FixedValue(0)
    }
}

impl RgbwConversion {
    /// The white channel is always zero.
    #[must_use]
    pub const fn off() -> Self {
        Self::fixed_value(0)
    }

    /// The white channel is always the given value.
    #[must_use]
    pub const fn fixed_value(value: u8) -> Self {
        Self {
            method: Method::FixedValue(value),
        }
    }

    /// Assumes a perfect white LED: the minimum of the red, green and blue
    /// components is subtracted from them and assigned to the white channel.
    #[must_use]
    pub const fn min_subtraction() -> Self {
        Self {
            method: Method::SubtractMin,
        }
    }

    /// Assumes that the white LED emits the color of a black body at the
    /// given temperature, in Kelvin; e.g. 4500 K is a typical warm white.
    #[must_use]
    pub fn color_temperature(temperature: f32) -> Self {
        Self::reference(
            RgbColor::from_color_temperature(temperature),
            Some(temperature),
        )
    }

    /// Assumes that the white LED emits the same color as the red, green and
    /// blue LEDs together with the given reference color. The largest
    /// component of the reference should be 255; it is normalized otherwise.
    ///
    /// A component of zero means that the white LED does not emit that
    /// channel: the channel does not limit the white level, and white takes
    /// nothing from it. A black reference means that the white LED is not
    /// used. (Unlike the C library; see `doc/differences-from-c.md`.)
    #[must_use]
    pub fn reference_color(reference: RgbColor) -> Self {
        Self::reference(reference, None)
    }

    fn reference(reference: RgbColor, temperature: Option<f32>) -> Self {
        let max_value = f32::from(
            reference
                .red
                .max(reference.green)
                .max(reference.blue)
                .max(1),
        );
        let mul = [reference.red, reference.green, reference.blue]
            .map(|value| (value >= 1).then(|| max_value / f32::from(value)));
        Self {
            method: Method::Reference {
                mul,
                div: mul.map(|mul| mul.map_or(0.0, |mul| 1.0 / mul)),
                temperature,
            },
        }
    }

    /// The color temperature that the conversion was set up with, if it was
    /// set up with [`RgbwConversion::color_temperature`]. Lets the caller skip
    /// the setup when the temperature has not changed.
    #[must_use]
    pub const fn temperature(&self) -> Option<f32> {
        match self.method {
            Method::Reference { temperature, .. } => temperature,
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_without_a_channel_does_not_limit_white() {
        // The white LED emits no blue, so a color without blue can still use it
        // (the C library gives (200, 200, 0, 0))
        let conversion = RgbwConversion::reference_color(RgbColor::new(255, 200, 0));
        assert_eq!(
            RgbColor::new(200, 200, 0).to_rgbw(&conversion),
            RgbwColor::new(0, 43, 0, 200)
        );
    }

    #[test]
    fn white_takes_nothing_from_a_channel_it_does_not_emit() {
        // Blue stays 50 (the C library gives 49)
        let conversion = RgbwConversion::reference_color(RgbColor::new(255, 200, 0));
        assert_eq!(
            RgbColor::new(100, 100, 50).to_rgbw(&conversion),
            RgbwColor::new(0, 21, 50, 100)
        );
    }

    #[test]
    fn black_reference_leaves_white_off() {
        // The C library gives (9, 19, 29, 255)
        let conversion = RgbwConversion::reference_color(RgbColor::BLACK);
        assert_eq!(
            RgbColor::new(10, 20, 30).to_rgbw(&conversion),
            RgbwColor::new(10, 20, 30, 0)
        );
    }

    #[test]
    fn low_color_temperatures_have_no_blue() {
        // Why references without a channel matter in practice
        assert_eq!(RgbColor::from_color_temperature(1000.0).blue, 0);
        let conversion = RgbwConversion::color_temperature(1000.0);
        assert_eq!(conversion.temperature(), Some(1000.0));
        // The color of the white LED is produced by the white LED alone (the C
        // library gives (255, 67, 0, 0))
        let white = RgbColor::from_color_temperature(1000.0).to_rgbw(&conversion);
        assert_eq!(white, RgbwColor::new(0, 0, 0, 255));
    }
}
