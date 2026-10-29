//! The common API of the sets of flags of the crate.

/// Implements the methods and operators of a set of flags for a newtype
/// around `u8` that has the constants `NONE` and `ALL`.
///
/// Bits that are not in `ALL` can be kept with
/// [`from_bits_retain`](crate::control::ControlComponents::from_bits_retain),
/// so that a value read from a file can be written back unchanged; the
/// complement (`!`) and `from_bits_truncate` keep only the bits in `ALL`.
macro_rules! impl_flags {
    ($type:ident) => {
        impl $type {
            /// The set with the given bits, including bits without a meaning.
            #[must_use]
            pub const fn from_bits_retain(bits: u8) -> Self {
                Self(bits)
            }

            /// The set with the given bits; bits without a meaning are
            /// dropped.
            #[must_use]
            pub const fn from_bits_truncate(bits: u8) -> Self {
                Self(bits & Self::ALL.0)
            }

            /// The bits of the set.
            #[must_use]
            pub const fn bits(self) -> u8 {
                self.0
            }

            /// Whether the set is empty.
            #[must_use]
            pub const fn is_empty(self) -> bool {
                self.0 == 0
            }

            /// Whether the set contains every element of `other`.
            #[must_use]
            pub const fn contains(self, other: Self) -> bool {
                self.0 & other.0 == other.0
            }

            /// Whether the set contains any element of `other`.
            #[must_use]
            pub const fn intersects(self, other: Self) -> bool {
                self.0 & other.0 != 0
            }

            /// Adds the elements of `other` to the set.
            pub const fn insert(&mut self, other: Self) {
                self.0 |= other.0;
            }

            /// Removes the elements of `other` from the set.
            pub const fn remove(&mut self, other: Self) {
                self.0 &= !other.0;
            }

            /// Adds the elements of `other` to the set if `value` is true,
            /// removes them otherwise.
            pub const fn set(&mut self, other: Self, value: bool) {
                if value {
                    self.insert(other);
                } else {
                    self.remove(other);
                }
            }
        }

        impl core::ops::BitOr for $type {
            type Output = Self;

            fn bitor(self, other: Self) -> Self {
                Self(self.0 | other.0)
            }
        }

        impl core::ops::BitAnd for $type {
            type Output = Self;

            fn bitand(self, other: Self) -> Self {
                Self(self.0 & other.0)
            }
        }

        /// The difference of two sets.
        impl core::ops::Sub for $type {
            type Output = Self;

            fn sub(self, other: Self) -> Self {
                Self(self.0 & !other.0)
            }
        }

        /// The complement of a set, within the bits that have a meaning.
        impl core::ops::Not for $type {
            type Output = Self;

            fn not(self) -> Self {
                Self(!self.0 & Self::ALL.0)
            }
        }

        impl core::ops::BitOrAssign for $type {
            fn bitor_assign(&mut self, other: Self) {
                self.0 |= other.0;
            }
        }

        impl core::ops::BitAndAssign for $type {
            fn bitand_assign(&mut self, other: Self) {
                self.0 &= other.0;
            }
        }

        impl core::ops::SubAssign for $type {
            fn sub_assign(&mut self, other: Self) {
                self.0 &= !other.0;
            }
        }

        impl core::fmt::Binary for $type {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                core::fmt::Binary::fmt(&self.0, f)
            }
        }

        impl core::fmt::Octal for $type {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                core::fmt::Octal::fmt(&self.0, f)
            }
        }

        impl core::fmt::LowerHex for $type {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                core::fmt::LowerHex::fmt(&self.0, f)
            }
        }

        impl core::fmt::UpperHex for $type {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                core::fmt::UpperHex::fmt(&self.0, f)
            }
        }
    };
}

pub(crate) use impl_flags;

#[cfg(test)]
mod tests {
    use crate::control::ControlComponents as C;

    #[test]
    fn set_operations() {
        let both = C::POSITION | C::VELOCITY;
        assert!(both.contains(C::POSITION));
        assert!(!both.contains(C::POSITION | C::YAW));
        assert!(both.intersects(C::POSITION | C::YAW));
        assert_eq!(both & C::VELOCITY, C::VELOCITY);
        assert_eq!(both - C::VELOCITY, C::POSITION);
        assert_eq!(!both, C::LIGHTS | C::YAW | C::YAW_RATE);
        assert_eq!(!C::NONE, C::ALL);

        let mut set = C::NONE;
        assert!(set.is_empty());
        set.insert(C::YAW);
        set |= C::LIGHTS;
        set.set(C::POSITION, true);
        set.set(C::YAW, false);
        assert_eq!(set, C::LIGHTS | C::POSITION);
        set -= C::LIGHTS;
        set &= C::ALL;
        set.remove(C::POSITION);
        assert!(set.is_empty());
    }

    #[test]
    fn unknown_bits() {
        let set = C::from_bits_retain(0xe1);
        assert_eq!(set.bits(), 0xe1);
        assert_eq!(C::from_bits_truncate(0xe1), C::POSITION);
        assert_eq!(!set, C::ALL - C::POSITION);
    }

    #[test]
    fn formatting() {
        let set = C::POSITION | C::YAW;
        assert_eq!(
            alloc::format!("{set:#010b} {set:o} {set:x} {set:#X}"),
            "0b00001001 11 9 0x9"
        );
    }
}
